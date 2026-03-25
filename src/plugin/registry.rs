use super::api::{OutputFormatter, PluginCommand, PluginError, PluginResult};
use super::events::{
    EventContext, ExecutionEvent, PluginInvocationKind, PluginInvocationOutcome, PluginTelemetryEvent,
};
use super::loader::{LoadedPlugin, PluginLoader, PluginTrustPolicy};
use std::collections::{HashMap, HashSet, VecDeque};
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
    total_failures: usize,
    total_timeouts: usize,
    total_panics: usize,
    last_error: Option<String>,
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
            let name = plugin.manifest().name.clone();
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
        let name = plugin.manifest().name.clone();

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
            .map_err(|_| PluginError::ExecutionFailed("Failed to update plugin health".to_string()))?
            .insert(name, PluginHealth::default());
        Ok(())
    }

    /// Get a plugin by name
    pub fn get_plugin(&self, name: &str) -> Option<Arc<RwLock<LoadedPlugin>>> {
        self.plugins.get(name).cloned()
    }

    /// Get all plugin names
    pub fn plugin_names(&self) -> Vec<String> {
        self.plugins.keys().cloned().collect()
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
            let Some(plugin_arc) = self.plugins.get(&name) else { continue };
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
    pub fn reload_plugin(&mut self, name: &str) -> PluginResult<()> {
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

        // Get plugin info before unloading
        let (manifest_path, saved_state) = {
            let plugin = plugin_arc.write().map_err(|_| {
                PluginError::ExecutionFailed("Failed to acquire plugin lock".to_string())
            })?;

            if !plugin.plugin().supports_hot_reload() {
                return Err(PluginError::ExecutionFailed(format!(
                    "Plugin '{}' does not support hot-reload",
                    name
                )));
            }

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

                self.register_plugin(new_plugin)?;
                info!("Successfully reloaded plugin: {}", name);
                Ok(())
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
                if let Some(health) = self
                    .health
                    .read()
                    .ok()
                    .and_then(|health| health.get(plugin.manifest().name.as_str()).cloned())
                {
                    stats.plugin_failures += health.total_failures;
                    stats.plugin_timeouts += health.total_timeouts;
                    stats.plugin_panics += health.total_panics;
                    if health.circuit_open {
                        stats.open_circuits += 1;
                    }
                }
            }
        }

        stats.total = self.plugins.len();
        stats
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

    /// Execute a plugin-provided command, if any plugin declares it.
    pub fn execute_command(&self, command: &str, args: &[String]) -> PluginResult<Option<String>> {
        let names: Vec<String> = self.plugins.keys().cloned().collect();
        for name in names {
            let Some(plugin_arc) = self.plugins.get(&name) else { continue };
            {
                let plugin = plugin_arc.read().map_err(|_| {
                    PluginError::ExecutionFailed(format!("Failed to acquire plugin lock: {}", name))
                })?;
                if !plugin.manifest().capabilities.provides_commands {
                    continue;
                }
                if !plugin.plugin().commands().iter().any(|cmd| cmd.name == command) {
                    continue;
                }
            }

            let mut health = self.health.write().map_err(|_| {
                PluginError::ExecutionFailed("Failed to update plugin health".to_string())
            })?;
            let result = self.run_command_with_policy(&mut health, &name, plugin_arc, command, args)?;
            return Ok(Some(result));
        }

        Ok(None)
    }

    pub fn format_output(&self, formatter: &str, data: &str) -> PluginResult<Option<String>> {
        let names: Vec<String> = self.plugins.keys().cloned().collect();
        for name in names {
            let Some(plugin_arc) = self.plugins.get(&name) else { continue };
            {
                let plugin = plugin_arc.read().map_err(|_| {
                    PluginError::ExecutionFailed(format!("Failed to acquire plugin lock: {}", name))
                })?;
                if !plugin.manifest().capabilities.provides_formatters {
                    continue;
                }
                if !plugin.plugin().formatters().iter().any(|fmt| fmt.name == formatter) {
                    continue;
                }
            }

            let mut health = self.health.write().map_err(|_| {
                PluginError::ExecutionFailed("Failed to update plugin health".to_string())
            })?;
            let result =
                self.run_formatter_with_policy(&mut health, &name, plugin_arc, formatter, data)?;
            return Ok(Some(result));
        }

        Ok(None)
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
            Self::push_telemetry(
                context,
                name,
                PluginInvocationKind::Hook,
                PluginInvocationOutcome::SkippedCircuitOpen,
                0,
                "Plugin hook skipped because the circuit breaker is open.".to_string(),
            );
            return Ok(());
        }

        let start = Instant::now();
        let result = {
            let mut plugin = plugin_arc.write().map_err(|_| {
                PluginError::ExecutionFailed(format!("Failed to acquire plugin lock: {}", name))
            })?;
            catch_unwind(AssertUnwindSafe(|| plugin.plugin_mut().on_event(event, context)))
        };
        self.record_outcome(
            health,
            Some(context),
            name,
            PluginInvocationKind::Hook,
            self.policy.hook_timeout,
            start.elapsed(),
            result.map_err(|_| PluginError::ExecutionFailed("Plugin panicked during hook execution".to_string())),
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
            return Err(PluginError::CircuitOpen(format!(
                "Plugin '{}' command '{}' skipped because the circuit breaker is open",
                name, command
            )));
        }

        let start = Instant::now();
        let result = {
            let mut plugin = plugin_arc.write().map_err(|_| {
                PluginError::ExecutionFailed(format!("Failed to acquire plugin lock: {}", name))
            })?;
            catch_unwind(AssertUnwindSafe(|| plugin.plugin_mut().execute_command(command, args)))
        };
        self.record_outcome(
            health,
            None,
            name,
            PluginInvocationKind::Command,
            self.policy.command_timeout,
            start.elapsed(),
            result.map_err(|_| PluginError::ExecutionFailed("Plugin panicked during command execution".to_string())),
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
            return Err(PluginError::CircuitOpen(format!(
                "Plugin '{}' formatter '{}' skipped because the circuit breaker is open",
                name, formatter
            )));
        }

        let start = Instant::now();
        let result = {
            let plugin = plugin_arc.write().map_err(|_| {
                PluginError::ExecutionFailed(format!("Failed to acquire plugin lock: {}", name))
            })?;
            catch_unwind(AssertUnwindSafe(|| plugin.plugin().format_output(formatter, data)))
        };
        self.record_outcome(
            health,
            None,
            name,
            PluginInvocationKind::Formatter,
            self.policy.formatter_timeout,
            start.elapsed(),
            result.map_err(|_| PluginError::ExecutionFailed("Plugin panicked during formatter execution".to_string())),
        )
    }

    fn record_outcome<T>(
        &self,
        health: &mut HashMap<String, PluginHealth>,
        mut context: Option<&mut EventContext>,
        name: &str,
        kind: PluginInvocationKind,
        timeout: Duration,
        elapsed: Duration,
        result: Result<PluginResult<T>, PluginError>,
    ) -> PluginResult<T> {
        let state = health.entry(name.to_string()).or_default();

        match result {
            Err(err) => {
                state.total_panics += 1;
                state.total_failures += 1;
                state.consecutive_failures += 1;
                state.last_error = Some(err.to_string());
                if state.consecutive_failures >= self.policy.max_consecutive_failures {
                    state.circuit_open = true;
                }
                if let Some(ctx) = context.as_deref_mut() {
                    Self::push_telemetry(
                        ctx,
                        name,
                        kind,
                        PluginInvocationOutcome::Panic,
                        elapsed.as_millis(),
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
                if let Some(ctx) = context.as_deref_mut() {
                    Self::push_telemetry(
                        ctx,
                        name,
                        kind,
                        PluginInvocationOutcome::Failure,
                        elapsed.as_millis(),
                        err.to_string(),
                    );
                }
                Err(err)
            }
            Ok(Ok(_value)) if elapsed > timeout => {
                state.total_timeouts += 1;
                state.total_failures += 1;
                state.timeout_count += 1;
                state.consecutive_failures += 1;
                let message = format!(
                    "Plugin '{}' exceeded the {:?} {:?} budget ({:?})",
                    name, kind, timeout, elapsed
                );
                state.last_error = Some(message.clone());
                if state.timeout_count >= self.policy.max_timeouts
                    || state.consecutive_failures >= self.policy.max_consecutive_failures
                {
                    state.circuit_open = true;
                }
                if let Some(ctx) = context.as_deref_mut() {
                    Self::push_telemetry(
                        ctx,
                        name,
                        kind,
                        PluginInvocationOutcome::Timeout,
                        elapsed.as_millis(),
                        message.clone(),
                    );
                }
                Err(PluginError::Timeout(message))
            }
            Ok(Ok(value)) => {
                state.consecutive_failures = 0;
                state.timeout_count = 0;
                state.circuit_open = false;
                state.last_error = None;
                if let Some(ctx) = context.as_deref_mut() {
                    Self::push_telemetry(
                        ctx,
                        name,
                        kind,
                        PluginInvocationOutcome::Success,
                        elapsed.as_millis(),
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
        health.get(name).map(|state| state.circuit_open).unwrap_or(false)
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
    use std::sync::{Arc, Mutex};
    use std::thread;

    #[derive(Clone)]
    enum Behavior {
        Success,
        Fail,
        Sleep(Duration),
        Panic,
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
            queue.lock().unwrap().pop_front().unwrap_or(Behavior::Success)
        }
    }

    impl InspectorPlugin for TestPlugin {
        fn metadata(&self) -> PluginManifest {
            self.manifest.clone()
        }

        fn on_event(&mut self, _event: &ExecutionEvent, _context: &mut EventContext) -> PluginResult<()> {
            match Self::next_behavior(&self.hook_behavior) {
                Behavior::Success => Ok(()),
                Behavior::Fail => Err(PluginError::ExecutionFailed("hook failed".to_string())),
                Behavior::Sleep(duration) => {
                    thread::sleep(duration);
                    Ok(())
                }
                Behavior::Panic => panic!("hook panic"),
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
                Behavior::Panic => panic!("command panic"),
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

    #[test]
    fn hook_failures_are_contained_and_open_circuit_after_budget() {
        let plugin = TestPlugin::new(
            "failing",
            vec![Behavior::Fail, Behavior::Fail, Behavior::Fail, Behavior::Success],
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
        assert!(context.plugin_telemetry.iter().any(|entry|
            entry.outcome == PluginInvocationOutcome::SkippedCircuitOpen
                && entry.kind == PluginInvocationKind::Hook
        ));
    }

    #[test]
    fn slow_hooks_trigger_timeout_telemetry_and_circuit_breaker() {
        let plugin = TestPlugin::new(
            "slow",
            vec![
                Behavior::Sleep(Duration::from_millis(20)),
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
        registry.dispatch_event(&event, &mut context);

        let stats = registry.statistics();
        assert_eq!(stats.plugin_timeouts, 2);
        assert_eq!(stats.open_circuits, 1);
        assert!(context.plugin_telemetry.iter().any(|entry|
            entry.outcome == PluginInvocationOutcome::Timeout
        ));
    }

    #[test]
    fn command_failures_return_errors_and_then_trip_circuit() {
        let plugin = TestPlugin::new(
            "commandy",
            vec![],
            vec![Behavior::Fail, Behavior::Fail, Behavior::Fail, Behavior::Success],
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
    fn successful_hook_resets_failure_streak() {
        let plugin = TestPlugin::new(
            "recovering",
            vec![Behavior::Fail, Behavior::Success, Behavior::Fail, Behavior::Success],
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
}
