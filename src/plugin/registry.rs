use super::api::{OutputFormatter, PluginCommand, PluginError, PluginResult};
use super::events::{
    EventContext, ExecutionEvent, PluginInvocationKind, PluginInvocationOutcome,
    PluginTelemetryEvent,
};
use super::loader::{LoadedPlugin, PluginLoader, PluginRuntimeDescriptor, PluginTrustPolicy};
use super::manifest::PluginCapabilities;
use crate::logging;
use crate::output::{PluginIncidentReport, PluginIncidentType};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

static GLOBAL_PLUGIN_REGISTRY: OnceLock<Arc<RwLock<PluginRegistry>>> = OnceLock::new();

fn env_var_truthy(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .is_some_and(|v| env_value_truthy(&v))
}

fn env_value_truthy(value: &str) -> bool {
    let normalized = value.trim().to_lowercase();
    matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
}

/// Initialize the global plugin registry and load plugins from disk.
///
/// This is intended to be called once at startup.
pub fn init_global_plugin_registry() -> Arc<RwLock<PluginRegistry>> {
    GLOBAL_PLUGIN_REGISTRY
        .get_or_init(|| {
            let mut registry = PluginRegistry::new().unwrap_or_default();
            if env_var_truthy("SOROBAN_DEBUG_NO_PLUGINS") {
                info!("Plugins disabled via SOROBAN_DEBUG_NO_PLUGINS");
            } else {
                let load_results = registry.load_all_plugins();
                let total = load_results.len();
                let failed = load_results.iter().filter(|r| r.is_err()).count();
                if failed > 0 {
                    warn!(
                        "Plugin loading completed with {} failure(s) out of {} plugin(s)",
                        failed, total
                    );
                }
            }
            Arc::new(RwLock::new(registry))
        })
        .clone()
}

pub fn dispatch_global_event(event: &ExecutionEvent, context: &mut EventContext) {
    let Some(registry) = GLOBAL_PLUGIN_REGISTRY.get() else {
        return;
    };

    if let Ok(registry) = registry.read() {
        registry.dispatch_event(event, context);
    }
}

pub fn execute_global_command(command: &str, args: &[String]) -> PluginResult<Option<String>> {
    let Some(registry) = GLOBAL_PLUGIN_REGISTRY.get() else {
        return Ok(None);
    };

    let registry = registry
        .read()
        .map_err(|_| PluginError::ExecutionFailed("Failed to acquire registry lock".to_string()))?;
    registry.execute_command(command, args)
}

pub fn global_commands() -> Vec<PluginCommand> {
    let Some(registry) = GLOBAL_PLUGIN_REGISTRY.get() else {
        return Vec::new();
    };

    registry
        .read()
        .map(|r| r.all_commands())
        .unwrap_or_default()
}

pub fn global_formatters() -> Vec<OutputFormatter> {
    let Some(registry) = GLOBAL_PLUGIN_REGISTRY.get() else {
        return Vec::new();
    };

    registry
        .read()
        .map(|r| r.all_formatters())
        .unwrap_or_default()
}

pub fn format_global_output(formatter: &str, data: &str) -> PluginResult<Option<String>> {
    let Some(registry) = GLOBAL_PLUGIN_REGISTRY.get() else {
        return Ok(None);
    };

    let registry = registry
        .read()
        .map_err(|_| PluginError::ExecutionFailed("Failed to acquire registry lock".to_string()))?;
    registry.format_output(formatter, data)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PluginTrustSummary {
    pub name: String,
    pub version: String,
    pub author: String,
    pub trusted: bool,
    pub signer: Option<String>,
    pub fingerprint: Option<String>,
    pub warnings: Vec<String>,
    pub capabilities: super::manifest::PluginCapabilities,
}

pub fn get_global_trust_report() -> Vec<PluginTrustSummary> {
    let Some(registry) = GLOBAL_PLUGIN_REGISTRY.get() else {
        return Vec::new();
    };

    let Ok(registry) = registry.read() else {
        return Vec::new();
    };

    registry.trust_report()
}

pub fn get_global_plugin_info(name: &str) -> Option<PluginTrustSummary> {
    let registry = GLOBAL_PLUGIN_REGISTRY.get()?;

    let Ok(registry) = registry.read() else {
        return None;
    };

    registry.plugin_info(name)
}

pub fn global_command_conflicts() -> HashMap<String, Vec<String>> {
    let Some(registry) = GLOBAL_PLUGIN_REGISTRY.get() else {
        return HashMap::new();
    };

    registry
        .read()
        .map(|r| r.command_conflicts().clone())
        .unwrap_or_default()
}

pub fn global_formatter_conflicts() -> HashMap<String, Vec<String>> {
    let Some(registry) = GLOBAL_PLUGIN_REGISTRY.get() else {
        return HashMap::new();
    };

    registry
        .read()
        .map(|r| r.formatter_conflicts().clone())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Plugin Snapshot and Reload Diff
// ---------------------------------------------------------------------------

/// Snapshot of a plugin's state at a point in time
#[derive(Debug, Clone)]
struct PluginSnapshot {
    name: String,
    version: String,
    capabilities: PluginCapabilities,
    commands: Vec<String>,
    formatters: Vec<String>,
    dependencies: Vec<String>,
}

impl PluginSnapshot {
    fn from_loaded_plugin(plugin: &LoadedPlugin) -> Self {
        let manifest = plugin.manifest();
        Self {
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            capabilities: manifest.capabilities.clone(),
            commands: plugin
                .plugin()
                .commands()
                .iter()
                .map(|c| c.name.clone())
                .collect(),
            formatters: plugin
                .plugin()
                .formatters()
                .iter()
                .map(|f| f.name.clone())
                .collect(),
            dependencies: manifest.dependencies.clone(),
        }
    }
}

/// Represents changes detected during a plugin reload
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginReloadDiff {
    pub name: String,
    pub version_changed: Option<(String, String)>,
    pub capabilities_changed: Vec<String>,
    pub commands_added: Vec<String>,
    pub commands_removed: Vec<String>,
    pub formatters_added: Vec<String>,
    pub formatters_removed: Vec<String>,
    pub dependencies_added: Vec<String>,
    pub dependencies_removed: Vec<String>,
}

impl PluginReloadDiff {
    fn compute(old: &PluginSnapshot, new: &PluginSnapshot) -> Self {
        let version_changed = if old.version != new.version {
            Some((old.version.clone(), new.version.clone()))
        } else {
            None
        };

        let mut capabilities_changed = Vec::new();
        if old.capabilities.hooks_execution != new.capabilities.hooks_execution {
            capabilities_changed.push(format!(
                "hooks_execution: {} → {}",
                old.capabilities.hooks_execution, new.capabilities.hooks_execution
            ));
        }
        if old.capabilities.provides_commands != new.capabilities.provides_commands {
            capabilities_changed.push(format!(
                "provides_commands: {} → {}",
                old.capabilities.provides_commands, new.capabilities.provides_commands
            ));
        }
        if old.capabilities.provides_formatters != new.capabilities.provides_formatters {
            capabilities_changed.push(format!(
                "provides_formatters: {} → {}",
                old.capabilities.provides_formatters, new.capabilities.provides_formatters
            ));
        }
        if old.capabilities.supports_hot_reload != new.capabilities.supports_hot_reload {
            capabilities_changed.push(format!(
                "supports_hot_reload: {} → {}",
                old.capabilities.supports_hot_reload, new.capabilities.supports_hot_reload
            ));
        }

        let old_commands: HashSet<_> = old.commands.iter().cloned().collect();
        let new_commands: HashSet<_> = new.commands.iter().cloned().collect();
        let mut commands_added: Vec<_> = new_commands.difference(&old_commands).cloned().collect();
        let mut commands_removed: Vec<_> =
            old_commands.difference(&new_commands).cloned().collect();
        commands_added.sort();
        commands_removed.sort();

        let old_formatters: HashSet<_> = old.formatters.iter().cloned().collect();
        let new_formatters: HashSet<_> = new.formatters.iter().cloned().collect();
        let mut formatters_added: Vec<_> = new_formatters
            .difference(&old_formatters)
            .cloned()
            .collect();
        let mut formatters_removed: Vec<_> = old_formatters
            .difference(&new_formatters)
            .cloned()
            .collect();
        formatters_added.sort();
        formatters_removed.sort();

        let old_deps: HashSet<_> = old.dependencies.iter().cloned().collect();
        let new_deps: HashSet<_> = new.dependencies.iter().cloned().collect();
        let mut dependencies_added: Vec<_> = new_deps.difference(&old_deps).cloned().collect();
        let mut dependencies_removed: Vec<_> = old_deps.difference(&new_deps).cloned().collect();
        dependencies_added.sort();
        dependencies_removed.sort();

        Self {
            name: new.name.clone(),
            version_changed,
            capabilities_changed,
            commands_added,
            commands_removed,
            formatters_added,
            formatters_removed,
            dependencies_added,
            dependencies_removed,
        }
    }

    /// Returns true if there are any changes
    pub fn has_changes(&self) -> bool {
        self.version_changed.is_some()
            || !self.capabilities_changed.is_empty()
            || !self.commands_added.is_empty()
            || !self.commands_removed.is_empty()
            || !self.formatters_added.is_empty()
            || !self.formatters_removed.is_empty()
            || !self.dependencies_added.is_empty()
            || !self.dependencies_removed.is_empty()
    }

    /// Returns a concise summary of changes
    pub fn summary(&self) -> String {
        if !self.has_changes() {
            return format!("Plugin '{}' reloaded with no changes", self.name);
        }

        let mut lines = vec![format!("Plugin '{}' reload changes:", self.name)];

        if let Some((old_ver, new_ver)) = &self.version_changed {
            lines.push(format!("  Version: {} → {}", old_ver, new_ver));
        }

        if !self.capabilities_changed.is_empty() {
            lines.push("  Capabilities:".to_string());
            for change in &self.capabilities_changed {
                lines.push(format!("    {}", change));
            }
        }

        if !self.commands_added.is_empty() {
            lines.push(format!(
                "  Commands added: {}",
                self.commands_added.join(", ")
            ));
        }
        if !self.commands_removed.is_empty() {
            lines.push(format!(
                "  Commands removed: {}",
                self.commands_removed.join(", ")
            ));
        }

        if !self.formatters_added.is_empty() {
            lines.push(format!(
                "  Formatters added: {}",
                self.formatters_added.join(", ")
            ));
        }
        if !self.formatters_removed.is_empty() {
            lines.push(format!(
                "  Formatters removed: {}",
                self.formatters_removed.join(", ")
            ));
        }

        if !self.dependencies_added.is_empty() {
            lines.push(format!(
                "  Dependencies added: {}",
                self.dependencies_added.join(", ")
            ));
        }
        if !self.dependencies_removed.is_empty() {
            lines.push(format!(
                "  Dependencies removed: {}",
                self.dependencies_removed.join(", ")
            ));
        }

        lines.join("\n")
    }
}

impl fmt::Display for PluginReloadDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.summary())
    }
}

// ---------------------------------------------------------------------------
// Topological sort — pure helper, no I/O, no plugin loading
// ---------------------------------------------------------------------------

/// Sort `(name, dependencies)` pairs into a safe registration order using
/// Kahn's BFS algorithm.
///
/// Returns `(ordered_indices, errors)`:
/// - `ordered_indices` – indices into the original slice, in an order where
///   every dependency appears before the plugin that declares it.
/// - `errors` – one `PluginError` for every entry that could not be placed,
///   either because a declared dependency is absent from the set or because
///   it participates in a cycle.
///
/// This function is intentionally free of `LoadedPlugin` so it can be
/// exercised in unit tests without touching the file system.
pub(crate) fn toposort_names(entries: &[(String, Vec<String>)]) -> (Vec<usize>, Vec<PluginError>) {
    let n = entries.len();

    // Map name → index for O(1) dependency look-up.
    let name_to_idx: HashMap<&str, usize> = entries
        .iter()
        .enumerate()
        .map(|(i, (name, _))| (name.as_str(), i))
        .collect();

    // in_degree[i] = number of within-set dependencies still unresolved for entry i.
    let mut in_degree = vec![0usize; n];
    // dependents[i] = indices of entries that list entry i as a dependency.
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); n];
    // Accumulate missing-dependency errors per entry (at most one stored).
    let mut missing: Vec<Option<PluginError>> = (0..n).map(|_| None).collect();

    for (i, (name, deps)) in entries.iter().enumerate() {
        for dep in deps {
            match name_to_idx.get(dep.as_str()) {
                Some(&j) => {
                    in_degree[i] += 1;
                    dependents[j].push(i);
                }
                None => {
                    // Dependency is not present in this batch at all.
                    missing[i] = Some(PluginError::DependencyError(format!(
                        "Plugin '{}' requires '{}' which is not available in the plugin set",
                        name, dep
                    )));
                }
            }
        }
    }

    // Seed the queue with every entry that has no unresolved in-set deps and
    // no missing external dep.
    let mut queue: VecDeque<usize> = (0..n)
        .filter(|&i| in_degree[i] == 0 && missing[i].is_none())
        .collect();

    let mut order: Vec<usize> = Vec::with_capacity(n);

    while let Some(i) = queue.pop_front() {
        order.push(i);
        for &j in &dependents[i] {
            if missing[j].is_some() {
                continue; // already errored; skip to avoid spurious decrement
            }
            in_degree[j] -= 1;
            if in_degree[j] == 0 {
                queue.push_back(j);
            }
        }
    }

    // Every entry not in `order` either had a missing dep or is in a cycle.
    let in_order: HashSet<usize> = order.iter().copied().collect();
    let mut errors: Vec<PluginError> = Vec::new();

    for (i, err) in missing.into_iter().enumerate() {
        if let Some(e) = err {
            errors.push(e);
        } else if !in_order.contains(&i) {
            errors.push(PluginError::DependencyError(format!(
                "Plugin '{}' is part of a dependency cycle and cannot be loaded",
                entries[i].0
            )));
        }
    }

    (order, errors)
}

/// Consume a `Vec<LoadedPlugin>`, topologically sort it, and return
/// `(ordered_plugins, sort_errors)`.
fn toposort_plugins(plugins: Vec<LoadedPlugin>) -> (Vec<LoadedPlugin>, Vec<PluginError>) {
    // Build the name/deps table that `toposort_names` expects.
    let entries: Vec<(String, Vec<String>)> = plugins
        .iter()
        .map(|p| (p.manifest().name.clone(), p.manifest().dependencies.clone()))
        .collect();

    let (order, errors) = toposort_names(&entries);

    // Move plugins out of the Vec by index using Option slots.
    let mut slots: Vec<Option<LoadedPlugin>> = plugins.into_iter().map(Some).collect();
    let ordered = order
        .into_iter()
        .map(|i| slots[i].take().expect("each index appears exactly once"))
        .collect();

    (ordered, errors)
}

// ---------------------------------------------------------------------------
// PluginRegistry
// ---------------------------------------------------------------------------

/// Registry that manages all loaded plugins
pub struct PluginRegistry {
    /// Loaded plugins indexed by name
    plugins: HashMap<String, Arc<RwLock<LoadedPlugin>>>,

    /// Plugin loader
    loader: PluginLoader,

    /// Whether hot-reload is enabled
    hot_reload_enabled: bool,

    /// Runtime containment policy for plugin execution
    policy: PluginExecutionPolicy,

    /// Per-plugin health state used for containment and telemetry
    health: RwLock<HashMap<String, PluginHealth>>,

    /// Resolved plugin command winners, keyed by normalized command name
    command_winners: HashMap<String, String>,

    /// Resolved formatter winners, keyed by normalized formatter name
    formatter_winners: HashMap<String, String>,

    /// All providers for each normalized command name, winner first
    command_conflicts: HashMap<String, Vec<String>>,

    /// All providers for each normalized formatter name, winner first
    formatter_conflicts: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginExecutionPolicy {
    pub hook_timeout: Duration,
    pub command_timeout: Duration,
    pub formatter_timeout: Duration,
    pub max_consecutive_failures: usize,
    pub max_timeouts: usize,
}

impl Default for PluginExecutionPolicy {
    fn default() -> Self {
        Self {
            hook_timeout: Duration::from_millis(250),
            command_timeout: Duration::from_secs(2),
            formatter_timeout: Duration::from_millis(500),
            max_consecutive_failures: 3,
            max_timeouts: 2,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct PluginHealth {
    consecutive_failures: usize,
    timeout_count: usize,
    circuit_open: bool,
    session_disabled: bool,
    total_failures: usize,
    total_timeouts: usize,
    total_panics: usize,
    last_error: Option<String>,
    last_incident: Option<PluginIncidentReport>,
}

struct InvocationMetadata<'a> {
    name: &'a str,
    descriptor: PluginRuntimeDescriptor,
    kind: PluginInvocationKind,
    timeout: Duration,
    elapsed: Duration,
}

impl PluginRegistry {
    /// Create a new plugin registry with the default plugin directory
    pub fn new() -> PluginResult<Self> {
        let plugin_dir = PluginLoader::default_plugin_dir()?;
        Self::with_plugin_dir(plugin_dir)
    }

    /// Create a new plugin registry with a custom plugin directory
    pub fn with_plugin_dir(plugin_dir: PathBuf) -> PluginResult<Self> {
        Self::with_plugin_dir_and_trust_policy(plugin_dir, PluginTrustPolicy::default())
    }

    /// Create a new plugin registry with a custom plugin directory and trust policy
    pub fn with_plugin_dir_and_trust_policy(
        plugin_dir: PathBuf,
        trust_policy: PluginTrustPolicy,
    ) -> PluginResult<Self> {
        Self::with_plugin_dir_trust_and_policy(
            plugin_dir,
            trust_policy,
            PluginExecutionPolicy::default(),
        )
    }

    pub fn with_plugin_dir_trust_and_policy(
        plugin_dir: PathBuf,
        trust_policy: PluginTrustPolicy,
        policy: PluginExecutionPolicy,
    ) -> PluginResult<Self> {
        // Ensure plugin directory exists
        if !plugin_dir.exists() {
            info!("Creating plugin directory: {:?}", plugin_dir);
            std::fs::create_dir_all(&plugin_dir).map_err(|e| {
                PluginError::InitializationFailed(format!(
                    "Failed to create plugin directory: {}",
                    e
                ))
            })?;
        }

        Ok(Self {
            plugins: HashMap::new(),
            loader: PluginLoader::with_trust_policy(plugin_dir, trust_policy),
            hot_reload_enabled: false,
            policy,
            health: RwLock::new(HashMap::new()),
            command_winners: HashMap::new(),
            formatter_winners: HashMap::new(),
            command_conflicts: HashMap::new(),
            formatter_conflicts: HashMap::new(),
        })
    }

    /// Enable hot-reload functionality
    pub fn enable_hot_reload(&mut self) {
        self.hot_reload_enabled = true;
        info!("Plugin hot-reload enabled");
    }

    /// Disable hot-reload functionality
    pub fn disable_hot_reload(&mut self) {
        self.hot_reload_enabled = false;
        info!("Plugin hot-reload disabled");
    }

    /// Load all plugins from the plugin directory.
    ///
    /// Plugins are topologically sorted by their declared dependencies before
    /// registration, so a valid plugin set loads successfully regardless of
    /// the order in which the file-system returns the manifest files.
    pub fn load_all_plugins(&mut self) -> Vec<PluginResult<()>> {
        info!("Loading all plugins from plugin directory");

        // ── Phase 1: load every plugin from disk ──────────────────────────
        // Separate successful loads from immediate load failures so we can
        // sort only the plugins we actually have in hand.
        let mut loaded_plugins: Vec<LoadedPlugin> = Vec::new();
        let mut load_results: Vec<PluginResult<()>> = Vec::new();

        for result in self.loader.load_all() {
            match result {
                Ok(plugin) => {
                    info!("Loaded plugin from disk: {}", plugin.manifest().name);
                    loaded_plugins.push(plugin);
                }
                Err(e) => {
                    error!("Failed to load plugin from disk: {}", e);
                    load_results.push(Err(e));
                }
            }
        }

        // ── Phase 2: topological sort ──────────────────────────────────────
        // This guarantees that every dependency is registered before the
        // plugin that declares it, regardless of directory enumeration order.
        let (sorted_plugins, sort_errors) = toposort_plugins(loaded_plugins);

        for e in sort_errors {
            error!("Dependency sort error: {}", e);
            load_results.push(Err(e));
        }

        // ── Phase 3: register in dependency order ──────────────────────────
        for plugin in sorted_plugins {
            let name = Self::plugin_registration_key(&plugin);
            match self.register_plugin(plugin) {
                Ok(_) => {
                    info!("Successfully registered plugin: {}", name);
                    load_results.push(Ok(()));
                }
                Err(e) => {
                    error!("Failed to register plugin {}: {}", name, e);
                    load_results.push(Err(e));
                }
            }
        }

        info!("Loaded {} plugins successfully", self.plugins.len());
        load_results
    }

    /// Register a loaded plugin
    fn register_plugin(&mut self, plugin: LoadedPlugin) -> PluginResult<()> {
        let name = Self::plugin_registration_key(&plugin);

        // Check for duplicates
        if self.plugins.contains_key(&name) {
            return Err(PluginError::Invalid(format!(
                "Plugin with name '{}' is already registered",
                name
            )));
        }

        // Check dependencies — after topological sort these should always be
        // present, but we keep this guard as a safety net for plugins
        // registered via other code paths (e.g. `reload_plugin`).
        for dep in &plugin.manifest().dependencies {
            if !self.plugins.contains_key(dep) {
                return Err(PluginError::DependencyError(format!(
                    "Plugin '{}' requires plugin '{}' which is not loaded",
                    name, dep
                )));
            }
        }

        self.plugins
            .insert(name.clone(), Arc::new(RwLock::new(plugin)));
        self.health
            .write()
            .map_err(|_| {
                PluginError::ExecutionFailed("Failed to update plugin health".to_string())
            })?
            .insert(name, PluginHealth::default());
        self.rebuild_command_and_formatter_maps();
        Ok(())
    }

    /// Get a plugin by name
    pub fn get_plugin(&self, name: &str) -> Option<Arc<RwLock<LoadedPlugin>>> {
        self.plugins.get(name).cloned()
    }

    /// Normalize a command or formatter name for conflict detection and lookup.
    fn normalize_plugin_item_name(name: &str) -> String {
        name.trim().to_lowercase()
    }

    /// Compute a stable plugin registration key, using manifest name when available.
    fn plugin_registration_key(plugin: &LoadedPlugin) -> String {
        let manifest_name = plugin.manifest().name.trim();
        if !manifest_name.is_empty() {
            manifest_name.to_string()
        } else if let Some(stem) = plugin.path().file_stem() {
            stem.to_string_lossy().to_string()
        } else {
            plugin.path().to_string_lossy().to_string()
        }
    }

    /// Compute a stable precedence key for plugin sorting.
    fn plugin_precedence_key(plugin: &LoadedPlugin) -> (String, String) {
        let registration_key = Self::plugin_registration_key(plugin);
        let path_key = plugin.path().to_string_lossy().to_string();
        (registration_key.to_lowercase(), path_key)
    }

    /// Get all plugin names
    pub fn plugin_names(&self) -> Vec<String> {
        self.plugins.keys().cloned().collect()
    }

    /// Rebuild command and formatter conflict maps after plugin registration changes.
    fn rebuild_command_and_formatter_maps(&mut self) {
        self.command_winners.clear();
        self.formatter_winners.clear();
        self.command_conflicts.clear();
        self.formatter_conflicts.clear();

        let mut plugins: Vec<_> = self.plugins.values().cloned().collect();
        plugins.sort_by(|a, b| {
            let a = a.read();
            let b = b.read();
            match (a, b) {
                (Ok(a), Ok(b)) => {
                    Self::plugin_precedence_key(&a).cmp(&Self::plugin_precedence_key(&b))
                }
                _ => std::cmp::Ordering::Equal,
            }
        });

        for plugin_arc in plugins {
            if let Ok(plugin) = plugin_arc.read() {
                let plugin_key = Self::plugin_registration_key(&plugin);

                if plugin.manifest().capabilities.provides_commands {
                    for command in plugin.plugin().commands() {
                        let key = Self::normalize_plugin_item_name(&command.name);
                        self.command_conflicts
                            .entry(key.clone())
                            .or_default()
                            .push(plugin_key.clone());
                        self.command_winners
                            .entry(key)
                            .or_insert_with(|| plugin_key.clone());
                    }
                }

                if plugin.manifest().capabilities.provides_formatters {
                    for formatter in plugin.plugin().formatters() {
                        let key = Self::normalize_plugin_item_name(&formatter.name);
                        self.formatter_conflicts
                            .entry(key.clone())
                            .or_default()
                            .push(plugin_key.clone());
                        self.formatter_winners
                            .entry(key)
                            .or_insert_with(|| plugin_key.clone());
                    }
                }
            }
        }

        for (command, providers) in &self.command_conflicts {
            if providers.len() > 1 {
                let ignored = providers[1..].join(", ");
                warn!(
                    "Plugin command collision: '{}' winner: {} ignored: {}",
                    command, providers[0], ignored
                );
            }
        }

        for (formatter, providers) in &self.formatter_conflicts {
            if providers.len() > 1 {
                let ignored = providers[1..].join(", ");
                warn!(
                    "Plugin formatter collision: '{}' winner: {} ignored: {}",
                    formatter, providers[0], ignored
                );
            }
        }
    }

    /// Get the number of loaded plugins
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    /// Dispatch an event to all plugins
    pub fn dispatch_event(&self, event: &ExecutionEvent, context: &mut EventContext) {
        debug!("Dispatching event to {} plugins", self.plugins.len());

        let names: Vec<String> = self.plugins.keys().cloned().collect();
        for name in names {
            let Some(plugin_arc) = self.plugins.get(&name) else {
                continue;
            };
            let mut health = match self.health.write() {
                Ok(health) => health,
                Err(_) => {
                    warn!("Failed to acquire plugin health lock for '{}'", name);
                    continue;
                }
            };
            let outcome = self.run_hook_with_policy(&mut health, &name, plugin_arc, event, context);
            if let Err(err) = outcome {
                warn!("Plugin '{}' error handling event: {}", name, err);
            }
        }
    }

    /// Reload a specific plugin
    pub fn reload_plugin(&mut self, name: &str) -> PluginResult<PluginReloadDiff> {
        if !self.hot_reload_enabled {
            return Err(PluginError::ExecutionFailed(
                "Hot-reload is not enabled".to_string(),
            ));
        }

        let plugin_arc = self
            .plugins
            .get(name)
            .ok_or_else(|| PluginError::NotFound(format!("Plugin '{}' not found", name)))?
            .clone();

        // Capture old plugin state before unloading
        let old_snapshot = {
            let plugin = plugin_arc.write().map_err(|_| {
                PluginError::ExecutionFailed("Failed to acquire plugin lock".to_string())
            })?;

            if !plugin.plugin().supports_hot_reload() {
                return Err(PluginError::ExecutionFailed(format!(
                    "Plugin '{}' does not support hot-reload",
                    name
                )));
            }

            PluginSnapshot::from_loaded_plugin(&plugin)
        };

        // Get plugin info before unloading
        let (manifest_path, saved_state) = {
            let plugin = plugin_arc.write().map_err(|_| {
                PluginError::ExecutionFailed("Failed to acquire plugin lock".to_string())
            })?;

            let manifest_path = plugin
                .path()
                .parent()
                .ok_or_else(|| PluginError::Invalid("Invalid plugin path".to_string()))?
                .join("plugin.toml");

            let state = plugin.plugin().prepare_reload().map_err(|e| {
                PluginError::ExecutionFailed(format!("Failed to prepare plugin for reload: {}", e))
            })?;

            (manifest_path, state)
        };

        // Remove old plugin
        self.plugins.remove(name);
        if let Ok(mut health) = self.health.write() {
            health.remove(name);
        }

        // Load new version
        match self.loader.load_from_manifest(&manifest_path) {
            Ok(mut new_plugin) => {
                // Restore state
                if let Err(e) = new_plugin.plugin_mut().restore_from_reload(saved_state) {
                    error!("Failed to restore plugin state: {}", e);
                }

                // Capture new plugin state
                let new_snapshot = PluginSnapshot::from_loaded_plugin(&new_plugin);

                self.register_plugin(new_plugin)?;

                // Compute and emit diff
                let diff = PluginReloadDiff::compute(&old_snapshot, &new_snapshot);
                info!("Successfully reloaded plugin: {}\n{}", name, diff.summary());
                Ok(diff)
            }
            Err(e) => {
                error!("Failed to reload plugin '{}': {}", name, e);
                Err(e)
            }
        }
    }

    /// Unload all plugins
    pub fn unload_all(&mut self) {
        info!("Unloading all plugins");
        self.plugins.clear();
        self.command_winners.clear();
        self.formatter_winners.clear();
        self.command_conflicts.clear();
        self.formatter_conflicts.clear();
        if let Ok(mut health) = self.health.write() {
            health.clear();
        }
    }

    /// Get plugin statistics
    pub fn statistics(&self) -> PluginStatistics {
        let mut stats = PluginStatistics::default();

        for plugin_arc in self.plugins.values() {
            if let Ok(plugin) = plugin_arc.read() {
                let caps = &plugin.manifest().capabilities;

                if caps.hooks_execution {
                    stats.hooks_execution += 1;
                }
                if caps.provides_commands {
                    stats.provides_commands += 1;
                }
                if caps.provides_formatters {
                    stats.provides_formatters += 1;
                }
                if caps.supports_hot_reload {
                    stats.supports_hot_reload += 1;
                }
                let plugin_key = Self::plugin_registration_key(&plugin);
                if let Some(health) = self
                    .health
                    .read()
                    .ok()
                    .and_then(|health| health.get(plugin_key.as_str()).cloned())
                {
                    stats.plugin_failures += health.total_failures;
                    stats.plugin_timeouts += health.total_timeouts;
                    stats.plugin_panics += health.total_panics;
                    if health.circuit_open {
                        stats.open_circuits += 1;
                    }
                    if health.session_disabled {
                        stats.session_disabled += 1;
                    }
                    if health.last_incident.is_some() {
                        stats.plugin_incidents += 1;
                    }
                }
            }
        }

        stats.total = self.plugins.len();
        stats
    }

    /// Get all plugin command conflicts, including the winner first.
    pub fn command_conflicts(&self) -> &HashMap<String, Vec<String>> {
        &self.command_conflicts
    }

    /// Get all plugin formatter conflicts, including the winner first.
    pub fn formatter_conflicts(&self) -> &HashMap<String, Vec<String>> {
        &self.formatter_conflicts
    }

    pub fn all_commands(&self) -> Vec<PluginCommand> {
        let mut out = Vec::new();
        for plugin_arc in self.plugins.values() {
            if let Ok(plugin) = plugin_arc.read() {
                let caps = &plugin.manifest().capabilities;
                if !caps.provides_commands {
                    continue;
                }
                out.extend(plugin.plugin().commands());
            }
        }
        out
    }

    pub fn all_formatters(&self) -> Vec<OutputFormatter> {
        let mut out = Vec::new();
        for plugin_arc in self.plugins.values() {
            if let Ok(plugin) = plugin_arc.read() {
                let caps = &plugin.manifest().capabilities;
                if !caps.provides_formatters {
                    continue;
                }
                out.extend(plugin.plugin().formatters());
            }
        }
        out
    }

    pub fn trust_report(&self) -> Vec<PluginTrustSummary> {
        let mut report = Vec::new();
        for plugin in self.plugins.values() {
            if let Ok(p) = plugin.read() {
                let trust = p.trust();
                report.push(PluginTrustSummary {
                    name: p.manifest().name.clone(),
                    version: p.manifest().version.clone(),
                    author: p.manifest().author.clone(),
                    trusted: trust.trusted,
                    signer: trust.signer.as_ref().map(|s| s.signer.clone()),
                    fingerprint: trust.signer.as_ref().map(|s| s.fingerprint.clone()),
                    warnings: trust.warnings.clone(),
                    capabilities: p.manifest().capabilities.clone(),
                });
            }
        }
        report
    }

    pub fn plugin_info(&self, name: &str) -> Option<PluginTrustSummary> {
        self.plugins.get(name).and_then(|plugin| {
            if let Ok(p) = plugin.read() {
                let trust = p.trust();
                Some(PluginTrustSummary {
                    name: p.manifest().name.clone(),
                    version: p.manifest().version.clone(),
                    author: p.manifest().author.clone(),
                    trusted: trust.trusted,
                    signer: trust.signer.as_ref().map(|s| s.signer.clone()),
                    fingerprint: trust.signer.as_ref().map(|s| s.fingerprint.clone()),
                    warnings: trust.warnings.clone(),
                    capabilities: p.manifest().capabilities.clone(),
                })
            } else {
                None
            }
        })
    }

    /// Execute a plugin-provided command, if any plugin declares it.
    pub fn execute_command(&self, command: &str, args: &[String]) -> PluginResult<Option<String>> {
        let key = Self::normalize_plugin_item_name(command);
        let plugin_name = match self.command_winners.get(&key) {
            Some(name) => name.clone(),
            None => return Ok(None),
        };

        let plugin_arc = self
            .plugins
            .get(&plugin_name)
            .ok_or_else(|| {
                PluginError::ExecutionFailed(format!(
                    "Plugin '{}' registered as command winner but is missing",
                    plugin_name
                ))
            })?
            .clone();

        let mut health = self.health.write().map_err(|_| {
            PluginError::ExecutionFailed("Failed to update plugin health".to_string())
        })?;
        let result =
            self.run_command_with_policy(&mut health, &plugin_name, &plugin_arc, command, args)?;
        Ok(Some(result))
    }

    pub fn format_output(&self, formatter: &str, data: &str) -> PluginResult<Option<String>> {
        let key = Self::normalize_plugin_item_name(formatter);
        let plugin_name = match self.formatter_winners.get(&key) {
            Some(name) => name.clone(),
            None => return Ok(None),
        };

        let plugin_arc = self
            .plugins
            .get(&plugin_name)
            .ok_or_else(|| {
                PluginError::ExecutionFailed(format!(
                    "Plugin '{}' registered as formatter winner but is missing",
                    plugin_name
                ))
            })?
            .clone();

        let mut health = self.health.write().map_err(|_| {
            PluginError::ExecutionFailed("Failed to update plugin health".to_string())
        })?;
        let result = self.run_formatter_with_policy(
            &mut health,
            &plugin_name,
            &plugin_arc,
            formatter,
            data,
        )?;
        Ok(Some(result))
    }

    fn run_hook_with_policy(
        &self,
        health: &mut HashMap<String, PluginHealth>,
        name: &str,
        plugin_arc: &Arc<RwLock<LoadedPlugin>>,
        event: &ExecutionEvent,
        context: &mut EventContext,
    ) -> PluginResult<()> {
        if Self::circuit_open(health, name) {
            let message = Self::disabled_message(health, name, PluginInvocationKind::Hook)
                .unwrap_or_else(|| {
                    "Plugin hook skipped because the circuit breaker is open.".to_string()
                });
            Self::push_telemetry(
                context,
                name,
                PluginInvocationKind::Hook,
                PluginInvocationOutcome::SkippedCircuitOpen,
                0,
                message,
            );
            return Ok(());
        }

        let (descriptor, result, elapsed) = {
            let start = Instant::now();
            let mut plugin = plugin_arc.write().map_err(|_| {
                PluginError::ExecutionFailed(format!("Failed to acquire plugin lock: {}", name))
            })?;
            let descriptor = plugin.runtime_descriptor();
            let result = catch_unwind(AssertUnwindSafe(|| {
                plugin.plugin_mut().on_event(event, context)
            }));
            (descriptor, result, start.elapsed())
        };
        self.record_outcome(
            health,
            Some(context),
            InvocationMetadata {
                name,
                descriptor,
                kind: PluginInvocationKind::Hook,
                timeout: self.policy.hook_timeout,
                elapsed,
            },
            result.map_err(|payload| PluginError::Panic {
                plugin: name.to_string(),
                operation: "hook execution".to_string(),
                details: Self::panic_payload_message(payload),
            }),
        )
    }

    fn run_command_with_policy(
        &self,
        health: &mut HashMap<String, PluginHealth>,
        name: &str,
        plugin_arc: &Arc<RwLock<LoadedPlugin>>,
        command: &str,
        args: &[String],
    ) -> PluginResult<String> {
        if Self::circuit_open(health, name) {
            return Err(Self::blocked_invocation_error(
                health,
                name,
                PluginInvocationKind::Command,
                format!("command '{}'", command),
            ));
        }

        let (descriptor, result, elapsed) = {
            let start = Instant::now();
            let mut plugin = plugin_arc.write().map_err(|_| {
                PluginError::ExecutionFailed(format!("Failed to acquire plugin lock: {}", name))
            })?;
            let descriptor = plugin.runtime_descriptor();
            let result = catch_unwind(AssertUnwindSafe(|| {
                plugin.plugin_mut().execute_command(command, args)
            }));
            (descriptor, result, start.elapsed())
        };
        self.record_outcome(
            health,
            None,
            InvocationMetadata {
                name,
                descriptor,
                kind: PluginInvocationKind::Command,
                timeout: self.policy.command_timeout,
                elapsed,
            },
            result.map_err(|payload| PluginError::Panic {
                plugin: name.to_string(),
                operation: "command execution".to_string(),
                details: Self::panic_payload_message(payload),
            }),
        )
    }

    fn run_formatter_with_policy(
        &self,
        health: &mut HashMap<String, PluginHealth>,
        name: &str,
        plugin_arc: &Arc<RwLock<LoadedPlugin>>,
        formatter: &str,
        data: &str,
    ) -> PluginResult<String> {
        if Self::circuit_open(health, name) {
            return Err(Self::blocked_invocation_error(
                health,
                name,
                PluginInvocationKind::Formatter,
                format!("formatter '{}'", formatter),
            ));
        }

        let (descriptor, result, elapsed) = {
            let start = Instant::now();
            let plugin = plugin_arc.write().map_err(|_| {
                PluginError::ExecutionFailed(format!("Failed to acquire plugin lock: {}", name))
            })?;
            let descriptor = plugin.runtime_descriptor();
            let result = catch_unwind(AssertUnwindSafe(|| {
                plugin.plugin().format_output(formatter, data)
            }));
            (descriptor, result, start.elapsed())
        };
        self.record_outcome(
            health,
            None,
            InvocationMetadata {
                name,
                descriptor,
                kind: PluginInvocationKind::Formatter,
                timeout: self.policy.formatter_timeout,
                elapsed,
            },
            result.map_err(|payload| PluginError::Panic {
                plugin: name.to_string(),
                operation: "formatter execution".to_string(),
                details: Self::panic_payload_message(payload),
            }),
        )
    }

    fn record_outcome<T>(
        &self,
        health: &mut HashMap<String, PluginHealth>,
        context: Option<&mut EventContext>,
        meta: InvocationMetadata,
        result: Result<PluginResult<T>, PluginError>,
    ) -> PluginResult<T> {
        let state = health.entry(meta.name.to_string()).or_default();

        match result {
            Err(err @ PluginError::Panic { .. }) => {
                state.total_panics += 1;
                state.total_failures += 1;
                state.consecutive_failures += 1;
                state.last_error = Some(err.to_string());
                let report =
                    Self::build_incident_report(&meta, PluginIncidentType::Panic, err.to_string());
                Self::disable_for_session(state, report.clone());
                logging::log_plugin_incident(&report);
                if let Some(ctx) = context {
                    Self::push_telemetry(
                        ctx,
                        meta.name,
                        meta.kind,
                        PluginInvocationOutcome::Panic,
                        meta.elapsed.as_millis(),
                        report.summary_line(),
                    );
                }
                Err(err)
            }
            Err(err) => {
                state.total_failures += 1;
                state.consecutive_failures += 1;
                state.last_error = Some(err.to_string());
                if state.consecutive_failures >= self.policy.max_consecutive_failures {
                    state.circuit_open = true;
                }
                if let Some(ctx) = context {
                    Self::push_telemetry(
                        ctx,
                        meta.name,
                        meta.kind,
                        PluginInvocationOutcome::Failure,
                        meta.elapsed.as_millis(),
                        err.to_string(),
                    );
                }
                Err(err)
            }
            Ok(Err(err)) => {
                state.total_failures += 1;
                state.consecutive_failures += 1;
                state.last_error = Some(err.to_string());
                if state.consecutive_failures >= self.policy.max_consecutive_failures {
                    state.circuit_open = true;
                }
                if let Some(ctx) = context {
                    Self::push_telemetry(
                        ctx,
                        meta.name,
                        meta.kind,
                        PluginInvocationOutcome::Failure,
                        meta.elapsed.as_millis(),
                        err.to_string(),
                    );
                }
                Err(err)
            }
            Ok(Ok(_value)) if meta.elapsed > meta.timeout => {
                state.total_timeouts += 1;
                state.total_failures += 1;
                state.timeout_count += 1;
                state.consecutive_failures += 1;
                let message = format!(
                    "Plugin '{}' exceeded the {:?} {:?} budget ({:?})",
                    meta.name, meta.kind, meta.timeout, meta.elapsed
                );
                state.last_error = Some(message.clone());
                let report = Self::build_incident_report(
                    &meta,
                    PluginIncidentType::Timeout,
                    message.clone(),
                );
                Self::disable_for_session(state, report.clone());
                logging::log_plugin_incident(&report);
                if let Some(ctx) = context {
                    Self::push_telemetry(
                        ctx,
                        meta.name,
                        meta.kind,
                        PluginInvocationOutcome::Timeout,
                        meta.elapsed.as_millis(),
                        report.summary_line(),
                    );
                }
                Err(PluginError::SessionDisabled {
                    plugin: meta.name.to_string(),
                    reason: report.summary_line(),
                })
            }
            Ok(Ok(value)) => {
                state.consecutive_failures = 0;
                state.timeout_count = 0;
                state.last_error = None;
                if !state.session_disabled {
                    state.circuit_open = false;
                }
                if let Some(ctx) = context {
                    Self::push_telemetry(
                        ctx,
                        meta.name,
                        meta.kind,
                        PluginInvocationOutcome::Success,
                        meta.elapsed.as_millis(),
                        "Plugin invocation completed successfully.".to_string(),
                    );
                }
                Ok(value)
            }
        }
    }

    fn push_telemetry(
        context: &mut EventContext,
        plugin: &str,
        kind: PluginInvocationKind,
        outcome: PluginInvocationOutcome,
        duration_ms: u128,
        message: String,
    ) {
        context.plugin_telemetry.push(PluginTelemetryEvent {
            plugin: plugin.to_string(),
            kind,
            outcome,
            duration_ms,
            message,
        });
    }

    fn circuit_open(health: &HashMap<String, PluginHealth>, name: &str) -> bool {
        health
            .get(name)
            .map(|state| state.circuit_open || state.session_disabled)
            .unwrap_or(false)
    }

    fn disable_for_session(state: &mut PluginHealth, report: PluginIncidentReport) {
        state.circuit_open = true;
        state.session_disabled = true;
        state.last_error = Some(report.summary_line());
        state.last_incident = Some(report);
    }

    fn build_incident_report(
        meta: &InvocationMetadata<'_>,
        incident: PluginIncidentType,
        message: String,
    ) -> PluginIncidentReport {
        PluginIncidentReport {
            plugin: meta.descriptor.name.clone(),
            plugin_version: Some(meta.descriptor.version.clone()),
            library_path: Some(meta.descriptor.library_path.display().to_string()),
            invocation_kind: format!("{:?}", meta.kind).to_lowercase(),
            incident,
            action_taken: "disabled for this session".to_string(),
            core_debugger_status: "core debugger remains available; only the plugin was isolated"
                .to_string(),
            message,
        }
    }

    fn disabled_message(
        health: &HashMap<String, PluginHealth>,
        name: &str,
        kind: PluginInvocationKind,
    ) -> Option<String> {
        let state = health.get(name)?;
        if let Some(report) = &state.last_incident {
            return Some(format!(
                "{} Subsequent {} invocations are skipped for this session.",
                report.summary_line(),
                format!("{:?}", kind).to_lowercase()
            ));
        }
        if state.session_disabled {
            return Some(format!(
                "Plugin '{}' is disabled for this session and {} invocations are skipped.",
                name,
                format!("{:?}", kind).to_lowercase()
            ));
        }
        if state.circuit_open {
            return Some(format!(
                "Plugin '{}' {} invocation skipped because the circuit breaker is open.",
                name,
                format!("{:?}", kind).to_lowercase()
            ));
        }
        None
    }

    fn blocked_invocation_error(
        health: &HashMap<String, PluginHealth>,
        name: &str,
        kind: PluginInvocationKind,
        operation_label: String,
    ) -> PluginError {
        let reason = Self::disabled_message(health, name, kind).unwrap_or_else(|| {
            format!(
                "Plugin '{}' {} skipped because the plugin is unavailable.",
                name, operation_label
            )
        });
        match health.get(name) {
            Some(state) if state.session_disabled => PluginError::SessionDisabled {
                plugin: name.to_string(),
                reason,
            },
            _ => PluginError::CircuitOpen(reason),
        }
    }

    fn panic_payload_message(payload: Box<dyn std::any::Any + Send>) -> String {
        if let Some(message) = payload.downcast_ref::<&str>() {
            (*message).to_string()
        } else if let Some(message) = payload.downcast_ref::<String>() {
            message.clone()
        } else {
            "plugin panicked with a non-string payload".to_string()
        }
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        // Use a fallback temporary directory if default creation fails
        Self::new().unwrap_or_else(|e| {
            warn!(
                "Failed to create default plugin registry: {}. Using temporary directory.",
                e
            );
            let temp_dir = std::env::temp_dir().join("soroban-debugger-plugins");
            Self::with_plugin_dir(temp_dir)
                .expect("Failed to create plugin registry even with temp directory")
        })
    }
}

impl Drop for PluginRegistry {
    fn drop(&mut self) {
        self.unload_all();
    }
}

/// Statistics about loaded plugins
#[derive(Debug, Default, Clone)]
pub struct PluginStatistics {
    pub total: usize,
    pub hooks_execution: usize,
    pub provides_commands: usize,
    pub provides_formatters: usize,
    pub supports_hot_reload: usize,
    pub plugin_failures: usize,
    pub plugin_timeouts: usize,
    pub plugin_panics: usize,
    pub open_circuits: usize,
    pub session_disabled: usize,
    pub plugin_incidents: usize,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::events::{PluginInvocationKind, PluginInvocationOutcome};
    use crate::plugin::loader::PluginTrustAssessment;
    use crate::plugin::manifest::{PluginCapabilities, PluginManifest};
    use crate::plugin::InspectorPlugin;
    use std::any::Any;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::thread;

    #[allow(dead_code)]
    #[derive(Clone)]
    enum Behavior {
        Success,
        Fail,
        Sleep(Duration),
        Panic(&'static str),
    }

    struct TestPlugin {
        manifest: PluginManifest,
        hook_behavior: Arc<Mutex<VecDeque<Behavior>>>,
        command_behavior: Arc<Mutex<VecDeque<Behavior>>>,
    }

    impl TestPlugin {
        fn new(name: &str, hook_behavior: Vec<Behavior>, command_behavior: Vec<Behavior>) -> Self {
            Self {
                manifest: PluginManifest {
                    schema_version: crate::plugin::manifest::MANIFEST_SCHEMA_VERSION.to_string(),
                    name: name.to_string(),
                    version: "1.0.0".to_string(),
                    description: "test plugin".to_string(),
                    author: "test".to_string(),
                    license: Some("MIT".to_string()),
                    min_debugger_version: Some("0.1.0".to_string()),
                    capabilities: PluginCapabilities {
                        hooks_execution: true,
                        provides_commands: true,
                        provides_formatters: false,
                        supports_hot_reload: true,
                    },
                    library: "test.so".to_string(),
                    dependencies: vec![],
                    signature: None,
                },
                hook_behavior: Arc::new(Mutex::new(hook_behavior.into())),
                command_behavior: Arc::new(Mutex::new(command_behavior.into())),
            }
        }

        fn next_behavior(queue: &Arc<Mutex<VecDeque<Behavior>>>) -> Behavior {
            queue
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Behavior::Success)
        }
    }

    impl InspectorPlugin for TestPlugin {
        fn metadata(&self) -> PluginManifest {
            self.manifest.clone()
        }

        fn on_event(
            &mut self,
            _event: &ExecutionEvent,
            _context: &mut EventContext,
        ) -> PluginResult<()> {
            match Self::next_behavior(&self.hook_behavior) {
                Behavior::Success => Ok(()),
                Behavior::Fail => Err(PluginError::ExecutionFailed("hook failed".to_string())),
                Behavior::Sleep(duration) => {
                    thread::sleep(duration);
                    Ok(())
                }
                Behavior::Panic(message) => panic!("{message}"),
            }
        }

        fn commands(&self) -> Vec<PluginCommand> {
            vec![PluginCommand {
                name: "test-command".to_string(),
                description: "test".to_string(),
                arguments: vec![],
            }]
        }

        fn execute_command(&mut self, _command: &str, _args: &[String]) -> PluginResult<String> {
            match Self::next_behavior(&self.command_behavior) {
                Behavior::Success => Ok("ok".to_string()),
                Behavior::Fail => Err(PluginError::ExecutionFailed("command failed".to_string())),
                Behavior::Sleep(duration) => {
                    thread::sleep(duration);
                    Ok("slow".to_string())
                }
                Behavior::Panic(message) => panic!("{message}"),
            }
        }

        fn prepare_reload(&self) -> PluginResult<Box<dyn Any + Send>> {
            Ok(Box::new(()))
        }
    }

    fn registry_with_plugin_and_policy(
        plugin: TestPlugin,
        policy: PluginExecutionPolicy,
    ) -> PluginRegistry {
        let temp_dir = std::env::temp_dir().join("soroban-debug-registry-policy-tests");
        let mut registry = PluginRegistry::with_plugin_dir_trust_and_policy(
            temp_dir,
            PluginTrustPolicy::default(),
            policy,
        )
        .unwrap();
        let manifest = plugin.metadata();
        let loaded = LoadedPlugin::from_parts_for_tests(
            Box::new(plugin),
            PathBuf::from("test.so"),
            manifest,
            PluginTrustAssessment {
                trusted: true,
                warnings: Vec::new(),
                signer: None,
            },
        );
        registry.register_plugin(loaded).unwrap();
        registry
    }

    // ── helpers ─────────────────────────────────────────────────────────────

    /// Convenience: build the `(name, deps)` table that `toposort_names` takes.
    fn entries(pairs: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
        pairs
            .iter()
            .map(|(n, ds)| (n.to_string(), ds.iter().map(|d| d.to_string()).collect()))
            .collect()
    }

    /// Return the plugin names in the order `toposort_names` produces them,
    /// ignoring the error list (callers assert on it separately when needed).
    fn sorted_names(pairs: &[(&str, &[&str])]) -> Vec<String> {
        let e = entries(pairs);
        let (order, _) = toposort_names(&e);
        order.into_iter().map(|i| e[i].0.clone()).collect()
    }

    #[test]
    fn env_value_truthy_accepts_common_truthy_variants() {
        let cases = ["1", "true", "TRUE", "True", "yes", "YES", "on", "ON"];

        for case in cases {
            assert!(
                env_value_truthy(case),
                "expected '{case}' to be considered truthy"
            );
        }
    }

    #[test]
    fn env_value_truthy_rejects_non_truthy_values() {
        let cases = ["", "   ", "0", "false", "FALSE", "off", "no", "random"];

        for case in cases {
            assert!(
                !env_value_truthy(case),
                "expected '{case}' to be considered non-truthy"
            );
        }
    }

    // ── toposort_names — basic ordering ─────────────────────────────────────

    /// No dependencies: order is stable (matches input order).
    #[test]
    fn toposort_no_deps_preserves_input_order() {
        let names = sorted_names(&[("alpha", &[]), ("beta", &[]), ("gamma", &[])]);
        assert_eq!(names, vec!["alpha", "beta", "gamma"]);
    }

    /// B depends on A.  When given in natural order (A then B) the result
    /// must still be [A, B].
    #[test]
    fn toposort_dep_natural_order() {
        let names = sorted_names(&[("a", &[]), ("b", &["a"])]);
        assert_eq!(names, vec!["a", "b"]);
    }

    /// **Core regression** — B depends on A but B is discovered first
    /// (simulating a filesystem that enumerates "b/" before "a/").
    /// After the sort, A must appear before B.
    #[test]
    fn toposort_dep_reverse_discovery_order() {
        // B is listed first, as the OS might return it first from read_dir.
        let names = sorted_names(&[("b", &["a"]), ("a", &[])]);
        let a_pos = names.iter().position(|n| n == "a").unwrap();
        let b_pos = names.iter().position(|n| n == "b").unwrap();
        assert!(
            a_pos < b_pos,
            "a must be registered before b; got order: {:?}",
            names
        );
    }

    /// Three-level chain C → B → A in worst-case discovery order (C, B, A).
    #[test]
    fn toposort_three_level_chain_worst_discovery_order() {
        // Worst case: leaf discovered first, root last.
        let names = sorted_names(&[("c", &["b"]), ("b", &["a"]), ("a", &[])]);
        let pos = |n: &str| names.iter().position(|x| x == n).unwrap();
        assert!(pos("a") < pos("b"), "a before b");
        assert!(pos("b") < pos("c"), "b before c");
    }

    /// Diamond: both B and C depend on A; D depends on B and C.
    #[test]
    fn toposort_diamond_dependency() {
        // Worst discovery: D, C, B, A
        let names = sorted_names(&[("d", &["b", "c"]), ("c", &["a"]), ("b", &["a"]), ("a", &[])]);
        let pos = |n: &str| names.iter().position(|x| x == n).unwrap();
        assert!(pos("a") < pos("b"), "a before b");
        assert!(pos("a") < pos("c"), "a before c");
        assert!(pos("b") < pos("d"), "b before d");
        assert!(pos("c") < pos("d"), "c before d");
    }

    // ── toposort_names — error cases ─────────────────────────────────────────

    /// A plugin whose dependency is not in the set at all must produce exactly
    /// one `DependencyError` and must not appear in the ordered list.
    #[test]
    fn toposort_missing_external_dep_produces_error() {
        let e = entries(&[("b", &["nonexistent"])]);
        let (order, errors) = toposort_names(&e);
        assert!(order.is_empty(), "b cannot be ordered without its dep");
        assert_eq!(errors.len(), 1);
        assert!(
            matches!(&errors[0], PluginError::DependencyError(msg) if msg.contains("nonexistent"))
        );
    }

    /// A two-plugin cycle (A → B → A) must produce two `DependencyError`s
    /// (one per plugin) and an empty ordered list.
    #[test]
    fn toposort_cycle_produces_errors_for_both_plugins() {
        let e = entries(&[("a", &["b"]), ("b", &["a"])]);
        let (order, errors) = toposort_names(&e);
        assert!(order.is_empty(), "neither plugin can load in a cycle");
        assert_eq!(errors.len(), 2, "one error per plugin in the cycle");
        for err in &errors {
            assert!(
                matches!(err, PluginError::DependencyError(_)),
                "expected DependencyError, got {:?}",
                err
            );
        }
    }

    /// A self-referential plugin (depends on itself) must be detected as a
    /// cycle and produce a single error.
    #[test]
    fn toposort_self_cycle_produces_error() {
        let e = entries(&[("a", &["a"])]);
        let (order, errors) = toposort_names(&e);
        assert!(order.is_empty());
        assert_eq!(errors.len(), 1);
    }

    /// Plugins with no deps load fine even when others in the set have cycles.
    #[test]
    fn toposort_independent_plugins_unaffected_by_cycle() {
        let e = entries(&[
            ("good", &[]),
            ("cycle-a", &["cycle-b"]),
            ("cycle-b", &["cycle-a"]),
        ]);
        let (order, errors) = toposort_names(&e);
        assert_eq!(order, vec![0], "only 'good' (index 0) should be ordered");
        assert_eq!(errors.len(), 2, "cycle-a and cycle-b both error");
    }

    /// Empty input must not panic and must return empty results.
    #[test]
    fn toposort_empty_input() {
        let (order, errors) = toposort_names(&[]);
        assert!(order.is_empty());
        assert!(errors.is_empty());
    }

    // ── pre-existing registry tests (unchanged) ──────────────────────────────

    #[test]
    fn test_registry_creation() {
        let temp_dir = std::env::temp_dir().join("soroban-debug-test-plugins");
        let registry = PluginRegistry::with_plugin_dir(temp_dir.clone());
        assert!(registry.is_ok());
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_plugin_statistics() {
        let temp_dir = std::env::temp_dir().join("soroban-debug-test-plugins-stats");
        let registry = PluginRegistry::with_plugin_dir(temp_dir.clone()).unwrap();
        let stats = registry.statistics();
        assert_eq!(stats.total, 0);
        assert_eq!(stats.hooks_execution, 0);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    struct NamedCommandPlugin {
        manifest: PluginManifest,
        command_name: String,
        response: String,
    }

    impl NamedCommandPlugin {
        fn new(name: &str, command_name: &str, response: &str) -> Self {
            Self {
                manifest: PluginManifest {
                    schema_version: crate::plugin::manifest::MANIFEST_SCHEMA_VERSION.to_string(),
                    name: name.to_string(),
                    version: "1.0.0".to_string(),
                    description: "test plugin".to_string(),
                    author: "test".to_string(),
                    license: Some("MIT".to_string()),
                    min_debugger_version: Some("0.1.0".to_string()),
                    capabilities: PluginCapabilities {
                        hooks_execution: true,
                        provides_commands: true,
                        provides_formatters: false,
                        supports_hot_reload: false,
                    },
                    library: "test.so".to_string(),
                    dependencies: vec![],
                    signature: None,
                },
                command_name: command_name.to_string(),
                response: response.to_string(),
            }
        }
    }

    impl InspectorPlugin for NamedCommandPlugin {
        fn metadata(&self) -> PluginManifest {
            self.manifest.clone()
        }

        fn on_event(
            &mut self,
            _event: &ExecutionEvent,
            _context: &mut EventContext,
        ) -> PluginResult<()> {
            Ok(())
        }

        fn commands(&self) -> Vec<PluginCommand> {
            vec![PluginCommand {
                name: self.command_name.clone(),
                description: "test".to_string(),
                arguments: vec![],
            }]
        }

        fn execute_command(&mut self, _command: &str, _args: &[String]) -> PluginResult<String> {
            Ok(self.response.clone())
        }
    }

    #[test]
    fn plugin_command_conflicts_are_detected_and_winner_is_deterministic() {
        let temp_dir = std::env::temp_dir().join("soroban-debug-test-plugins-conflicts");
        let mut registry = PluginRegistry::with_plugin_dir(temp_dir.clone()).unwrap();

        let plugin_a = NamedCommandPlugin::new("plugin-a", "foo", "result-a");
        let loaded_a = LoadedPlugin::from_parts_for_tests(
            Box::new(plugin_a),
            PathBuf::from("plugin-a.so"),
            PluginManifest {
                schema_version: crate::plugin::manifest::MANIFEST_SCHEMA_VERSION.to_string(),
                name: "plugin-a".to_string(),
                version: "1.0.0".to_string(),
                description: "test plugin".to_string(),
                author: "test".to_string(),
                license: Some("MIT".to_string()),
                min_debugger_version: Some("0.1.0".to_string()),
                capabilities: PluginCapabilities {
                    hooks_execution: true,
                    provides_commands: true,
                    provides_formatters: false,
                    supports_hot_reload: false,
                },
                library: "plugin-a.so".to_string(),
                dependencies: vec![],
                signature: None,
            },
            PluginTrustAssessment {
                trusted: true,
                warnings: Vec::new(),
                signer: None,
            },
        );

        let plugin_b = NamedCommandPlugin::new("plugin-b", " Foo ", "result-b");
        let loaded_b = LoadedPlugin::from_parts_for_tests(
            Box::new(plugin_b),
            PathBuf::from("plugin-b.so"),
            PluginManifest {
                schema_version: crate::plugin::manifest::MANIFEST_SCHEMA_VERSION.to_string(),
                name: "plugin-b".to_string(),
                version: "1.0.0".to_string(),
                description: "test plugin".to_string(),
                author: "test".to_string(),
                license: Some("MIT".to_string()),
                min_debugger_version: Some("0.1.0".to_string()),
                capabilities: PluginCapabilities {
                    hooks_execution: true,
                    provides_commands: true,
                    provides_formatters: false,
                    supports_hot_reload: false,
                },
                library: "plugin-b.so".to_string(),
                dependencies: vec![],
                signature: None,
            },
            PluginTrustAssessment {
                trusted: true,
                warnings: Vec::new(),
                signer: None,
            },
        );

        registry.register_plugin(loaded_a).unwrap();
        registry.register_plugin(loaded_b).unwrap();

        let conflicts = registry.command_conflicts();
        assert_eq!(
            conflicts.get("foo").unwrap(),
            &vec!["plugin-a".to_string(), "plugin-b".to_string()]
        );

        let result = registry.execute_command("FOO", &[]).unwrap().unwrap();
        assert_eq!(result, "result-a");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn plugin_formatter_conflicts_are_detected_and_winner_is_deterministic() {
        struct NamedFormatterPlugin {
            manifest: PluginManifest,
            formatter_name: String,
            response: String,
        }

        impl InspectorPlugin for NamedFormatterPlugin {
            fn metadata(&self) -> PluginManifest {
                self.manifest.clone()
            }

            fn formatters(&self) -> Vec<OutputFormatter> {
                vec![OutputFormatter {
                    name: self.formatter_name.clone(),
                    supported_types: vec!["text".to_string()],
                }]
            }

            fn format_output(&self, _formatter: &str, _data: &str) -> PluginResult<String> {
                Ok(self.response.clone())
            }
        }

        let temp_dir = std::env::temp_dir().join("soroban-debug-test-formatter-conflicts");
        let mut registry = PluginRegistry::with_plugin_dir(temp_dir.clone()).unwrap();

        let manifest_a = PluginManifest {
            schema_version: crate::plugin::manifest::MANIFEST_SCHEMA_VERSION.to_string(),
            name: "plugin-a".to_string(),
            version: "1.0.0".to_string(),
            description: "test plugin".to_string(),
            author: "test".to_string(),
            license: Some("MIT".to_string()),
            min_debugger_version: Some("0.1.0".to_string()),
            capabilities: PluginCapabilities {
                hooks_execution: true,
                provides_commands: false,
                provides_formatters: true,
                supports_hot_reload: false,
            },
            library: "plugin-a.so".to_string(),
            dependencies: vec![],
            signature: None,
        };
        let plugin_a = NamedFormatterPlugin {
            manifest: manifest_a.clone(),
            formatter_name: "jsonx".to_string(),
            response: "formatted-a".to_string(),
        };
        let loaded_a = LoadedPlugin::from_parts_for_tests(
            Box::new(plugin_a),
            PathBuf::from("plugin-a.so"),
            manifest_a,
            PluginTrustAssessment {
                trusted: true,
                warnings: Vec::new(),
                signer: None,
            },
        );

        let manifest_b = PluginManifest {
            schema_version: crate::plugin::manifest::MANIFEST_SCHEMA_VERSION.to_string(),
            name: "plugin-b".to_string(),
            version: "1.0.0".to_string(),
            description: "test plugin".to_string(),
            author: "test".to_string(),
            license: Some("MIT".to_string()),
            min_debugger_version: Some("0.1.0".to_string()),
            capabilities: PluginCapabilities {
                hooks_execution: true,
                provides_commands: false,
                provides_formatters: true,
                supports_hot_reload: false,
            },
            library: "plugin-b.so".to_string(),
            dependencies: vec![],
            signature: None,
        };
        let plugin_b = NamedFormatterPlugin {
            manifest: manifest_b.clone(),
            formatter_name: " jsonx ".to_string(),
            response: "formatted-b".to_string(),
        };
        let loaded_b = LoadedPlugin::from_parts_for_tests(
            Box::new(plugin_b),
            PathBuf::from("plugin-b.so"),
            manifest_b,
            PluginTrustAssessment {
                trusted: true,
                warnings: Vec::new(),
                signer: None,
            },
        );

        registry.register_plugin(loaded_a).unwrap();
        registry.register_plugin(loaded_b).unwrap();

        let conflicts = registry.formatter_conflicts();
        assert_eq!(
            conflicts.get("jsonx").unwrap(),
            &vec!["plugin-a".to_string(), "plugin-b".to_string()]
        );

        let result = registry.format_output("JSONX", "data").unwrap().unwrap();
        assert_eq!(result, "formatted-a");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn hook_failures_are_contained_and_open_circuit_after_budget() {
        let plugin = TestPlugin::new(
            "failing",
            vec![
                Behavior::Fail,
                Behavior::Fail,
                Behavior::Fail,
                Behavior::Success,
            ],
            vec![],
        );
        let registry = registry_with_plugin_and_policy(
            plugin,
            PluginExecutionPolicy {
                max_consecutive_failures: 3,
                ..PluginExecutionPolicy::default()
            },
        );
        let mut context = EventContext::new();
        let event = ExecutionEvent::ExecutionResumed;

        registry.dispatch_event(&event, &mut context);
        registry.dispatch_event(&event, &mut context);
        registry.dispatch_event(&event, &mut context);
        registry.dispatch_event(&event, &mut context);

        let stats = registry.statistics();
        assert_eq!(stats.plugin_failures, 3);
        assert_eq!(stats.open_circuits, 1);
        assert!(context.plugin_telemetry.iter().any(|entry| entry.outcome
            == PluginInvocationOutcome::SkippedCircuitOpen
            && entry.kind == PluginInvocationKind::Hook));
    }

    #[test]
    fn slow_hooks_trigger_timeout_telemetry_and_circuit_breaker() {
        let plugin = TestPlugin::new(
            "slow",
            vec![
                Behavior::Sleep(Duration::from_millis(20)),
                Behavior::Success,
            ],
            vec![],
        );
        let registry = registry_with_plugin_and_policy(
            plugin,
            PluginExecutionPolicy {
                hook_timeout: Duration::from_millis(5),
                max_timeouts: 2,
                ..PluginExecutionPolicy::default()
            },
        );
        let mut context = EventContext::new();
        let event = ExecutionEvent::ExecutionResumed;

        registry.dispatch_event(&event, &mut context);
        registry.dispatch_event(&event, &mut context);

        let stats = registry.statistics();
        assert_eq!(stats.plugin_timeouts, 1);
        assert_eq!(stats.open_circuits, 1);
        assert_eq!(stats.session_disabled, 1);
        assert_eq!(stats.plugin_incidents, 1);
        assert!(context
            .plugin_telemetry
            .iter()
            .any(|entry| entry.outcome == PluginInvocationOutcome::Timeout));
    }

    #[test]
    fn command_failures_return_errors_and_then_trip_circuit() {
        let plugin = TestPlugin::new(
            "commandy",
            vec![],
            vec![
                Behavior::Fail,
                Behavior::Fail,
                Behavior::Fail,
                Behavior::Success,
            ],
        );
        let registry = registry_with_plugin_and_policy(
            plugin,
            PluginExecutionPolicy {
                max_consecutive_failures: 3,
                ..PluginExecutionPolicy::default()
            },
        );

        assert!(registry.execute_command("test-command", &[]).is_err());
        assert!(registry.execute_command("test-command", &[]).is_err());
        assert!(registry.execute_command("test-command", &[]).is_err());
        let err = registry.execute_command("test-command", &[]).unwrap_err();
        assert!(matches!(err, PluginError::CircuitOpen(_)));
    }

    #[test]
    fn panic_incident_disables_plugin_for_current_session() {
        let plugin = TestPlugin::new(
            "panicky",
            vec![Behavior::Panic("boom"), Behavior::Success],
            vec![],
        );
        let registry = registry_with_plugin_and_policy(plugin, PluginExecutionPolicy::default());
        let mut context = EventContext::new();
        let event = ExecutionEvent::ExecutionResumed;

        registry.dispatch_event(&event, &mut context);
        registry.dispatch_event(&event, &mut context);

        let stats = registry.statistics();
        assert_eq!(stats.plugin_panics, 1);
        assert_eq!(stats.session_disabled, 1);
        assert_eq!(stats.plugin_incidents, 1);
        assert!(context.plugin_telemetry.iter().any(|entry| {
            entry.outcome == PluginInvocationOutcome::Panic
                && entry.message.contains("Core debugger status")
        }));
        assert!(context.plugin_telemetry.iter().any(|entry| {
            entry.outcome == PluginInvocationOutcome::SkippedCircuitOpen
                && entry.message.contains("disabled for this session")
        }));
    }

    #[test]
    fn timeout_incident_returns_session_disabled_error_for_commands() {
        let plugin = TestPlugin::new(
            "slow-command",
            vec![],
            vec![
                Behavior::Sleep(Duration::from_millis(20)),
                Behavior::Success,
            ],
        );
        let registry = registry_with_plugin_and_policy(
            plugin,
            PluginExecutionPolicy {
                command_timeout: Duration::from_millis(5),
                ..PluginExecutionPolicy::default()
            },
        );

        let err = registry.execute_command("test-command", &[]).unwrap_err();
        assert!(matches!(err, PluginError::SessionDisabled { .. }));
        let err = registry.execute_command("test-command", &[]).unwrap_err();
        assert!(matches!(err, PluginError::SessionDisabled { .. }));

        let stats = registry.statistics();
        assert_eq!(stats.plugin_timeouts, 1);
        assert_eq!(stats.session_disabled, 1);
        assert_eq!(stats.plugin_incidents, 1);
    }

    #[test]
    fn successful_hook_resets_failure_streak() {
        let plugin = TestPlugin::new(
            "recovering",
            vec![
                Behavior::Fail,
                Behavior::Success,
                Behavior::Fail,
                Behavior::Success,
            ],
            vec![],
        );
        let registry = registry_with_plugin_and_policy(
            plugin,
            PluginExecutionPolicy {
                max_consecutive_failures: 2,
                ..PluginExecutionPolicy::default()
            },
        );
        let mut context = EventContext::new();
        let event = ExecutionEvent::ExecutionResumed;

        registry.dispatch_event(&event, &mut context);
        registry.dispatch_event(&event, &mut context);
        registry.dispatch_event(&event, &mut context);
        registry.dispatch_event(&event, &mut context);

        let stats = registry.statistics();
        assert_eq!(stats.open_circuits, 0);
        assert_eq!(stats.plugin_failures, 2);
        assert!(context
            .plugin_telemetry
            .iter()
            .any(|entry| entry.outcome == PluginInvocationOutcome::Success));
    }

    // ── Plugin reload diff tests ────────────────────────────────────────────

    #[test]
    fn reload_diff_detects_version_change() {
        let old = PluginSnapshot {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            capabilities: PluginCapabilities::default(),
            commands: vec![],
            formatters: vec![],
            dependencies: vec![],
        };
        let new = PluginSnapshot {
            version: "1.1.0".to_string(),
            ..old.clone()
        };

        let diff = PluginReloadDiff::compute(&old, &new);
        assert!(diff.has_changes());
        assert_eq!(
            diff.version_changed,
            Some(("1.0.0".to_string(), "1.1.0".to_string()))
        );
        assert!(diff.summary().contains("Version: 1.0.0 → 1.1.0"));
    }

    #[test]
    fn reload_diff_detects_capability_changes() {
        let old = PluginSnapshot {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            capabilities: PluginCapabilities {
                hooks_execution: true,
                provides_commands: false,
                provides_formatters: false,
                supports_hot_reload: false,
            },
            commands: vec![],
            formatters: vec![],
            dependencies: vec![],
        };
        let new = PluginSnapshot {
            capabilities: PluginCapabilities {
                hooks_execution: true,
                provides_commands: true,
                provides_formatters: false,
                supports_hot_reload: true,
            },
            ..old.clone()
        };

        let diff = PluginReloadDiff::compute(&old, &new);
        assert!(diff.has_changes());
        assert_eq!(diff.capabilities_changed.len(), 2);
        assert!(diff
            .capabilities_changed
            .iter()
            .any(|c| c.contains("provides_commands")));
        assert!(diff
            .capabilities_changed
            .iter()
            .any(|c| c.contains("supports_hot_reload")));
    }

    #[test]
    fn reload_diff_detects_added_and_removed_commands() {
        let old = PluginSnapshot {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            capabilities: PluginCapabilities::default(),
            commands: vec!["cmd1".to_string(), "cmd2".to_string()],
            formatters: vec![],
            dependencies: vec![],
        };
        let new = PluginSnapshot {
            commands: vec!["cmd2".to_string(), "cmd3".to_string()],
            ..old.clone()
        };

        let diff = PluginReloadDiff::compute(&old, &new);
        assert!(diff.has_changes());
        assert_eq!(diff.commands_added, vec!["cmd3"]);
        assert_eq!(diff.commands_removed, vec!["cmd1"]);
        assert!(diff.summary().contains("Commands added: cmd3"));
        assert!(diff.summary().contains("Commands removed: cmd1"));
    }

    #[test]
    fn reload_diff_detects_added_and_removed_formatters() {
        let old = PluginSnapshot {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            capabilities: PluginCapabilities::default(),
            commands: vec![],
            formatters: vec!["json".to_string()],
            dependencies: vec![],
        };
        let new = PluginSnapshot {
            formatters: vec!["json".to_string(), "yaml".to_string()],
            ..old.clone()
        };

        let diff = PluginReloadDiff::compute(&old, &new);
        assert!(diff.has_changes());
        assert_eq!(diff.formatters_added, vec!["yaml"]);
        assert!(diff.formatters_removed.is_empty());
        assert!(diff.summary().contains("Formatters added: yaml"));
    }

    #[test]
    fn reload_diff_detects_dependency_changes() {
        let old = PluginSnapshot {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            capabilities: PluginCapabilities::default(),
            commands: vec![],
            formatters: vec![],
            dependencies: vec!["dep1".to_string()],
        };
        let new = PluginSnapshot {
            dependencies: vec!["dep1".to_string(), "dep2".to_string()],
            ..old.clone()
        };

        let diff = PluginReloadDiff::compute(&old, &new);
        assert!(diff.has_changes());
        assert_eq!(diff.dependencies_added, vec!["dep2"]);
        assert!(diff.dependencies_removed.is_empty());
    }

    #[test]
    fn reload_diff_reports_no_changes_when_identical() {
        let snapshot = PluginSnapshot {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            capabilities: PluginCapabilities::default(),
            commands: vec!["cmd1".to_string()],
            formatters: vec!["fmt1".to_string()],
            dependencies: vec![],
        };

        let diff = PluginReloadDiff::compute(&snapshot, &snapshot);
        assert!(!diff.has_changes());
        assert!(diff.summary().contains("no changes"));
    }

    #[test]
    fn reload_diff_summary_is_concise_and_readable() {
        let old = PluginSnapshot {
            name: "example-plugin".to_string(),
            version: "1.0.0".to_string(),
            capabilities: PluginCapabilities {
                hooks_execution: true,
                provides_commands: true,
                provides_formatters: false,
                supports_hot_reload: false,
            },
            commands: vec!["old-cmd".to_string()],
            formatters: vec![],
            dependencies: vec![],
        };
        let new = PluginSnapshot {
            name: "example-plugin".to_string(),
            version: "2.0.0".to_string(),
            capabilities: PluginCapabilities {
                hooks_execution: true,
                provides_commands: true,
                provides_formatters: true,
                supports_hot_reload: true,
            },
            commands: vec!["new-cmd".to_string()],
            formatters: vec!["json".to_string()],
            dependencies: vec!["dep1".to_string()],
        };

        let diff = PluginReloadDiff::compute(&old, &new);
        let summary = diff.summary();

        assert!(summary.contains("example-plugin"));
        assert!(summary.contains("Version: 1.0.0 → 2.0.0"));
        assert!(summary.contains("provides_formatters: false → true"));
        assert!(summary.contains("supports_hot_reload: false → true"));
        assert!(summary.contains("Commands added: new-cmd"));
        assert!(summary.contains("Commands removed: old-cmd"));
        assert!(summary.contains("Formatters added: json"));
        assert!(summary.contains("Dependencies added: dep1"));
    }
}
