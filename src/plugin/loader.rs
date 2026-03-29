use super::api::{
    InspectorPlugin, PluginConstructor, PluginError, PluginResult, PLUGIN_CONSTRUCTOR_SYMBOL,
};
use super::manifest::{PluginManifest, VerifiedPluginSignature};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tracing::{error, info, warn};

/// A loaded plugin instance
pub struct LoadedPlugin {
    /// The plugin instance
    plugin: Box<dyn InspectorPlugin>,

    /// The dynamic library handle
    #[allow(dead_code)]
    library: Option<libloading::Library>,

    /// Path to the plugin library
    path: PathBuf,

    /// Plugin manifest
    manifest: PluginManifest,

    /// Trust assessment captured at load time
    trust: PluginTrustAssessment,
}

impl LoadedPlugin {
    /// Get a reference to the plugin
    pub fn plugin(&self) -> &dyn InspectorPlugin {
        &*self.plugin
    }

    /// Get a mutable reference to the plugin
    pub fn plugin_mut(&mut self) -> &mut dyn InspectorPlugin {
        &mut *self.plugin
    }

    /// Get the plugin manifest
    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    /// Get the plugin path
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Get trust assessment details for the loaded plugin
    pub fn trust(&self) -> &PluginTrustAssessment {
        &self.trust
    }

    #[cfg(test)]
    pub(crate) fn from_parts_for_tests(
        plugin: Box<dyn InspectorPlugin>,
        path: PathBuf,
        manifest: PluginManifest,
        trust: PluginTrustAssessment,
    ) -> Self {
        Self {
            plugin,
            library: None,
            path,
            manifest,
            trust,
        }
    }
}

/// Plugin loader that handles dynamic loading of plugin libraries
pub struct PluginLoader {
    /// Base directory for plugins
    plugin_dir: PathBuf,

    /// Trust policy used before dynamic loading
    trust_policy: PluginTrustPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginTrustMode {
    Off,
    Warn,
    Enforce,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginTrustPolicy {
    pub mode: PluginTrustMode,
    pub allowlist: BTreeSet<String>,
    pub denylist: BTreeSet<String>,
    pub allowed_signers: BTreeSet<String>,
}

impl Default for PluginTrustPolicy {
    fn default() -> Self {
        Self::from_env()
    }
}

impl PluginTrustPolicy {
    pub fn from_env() -> Self {
        let mode = match std::env::var("SOROBAN_DEBUG_PLUGIN_TRUST_MODE")
            .unwrap_or_else(|_| "warn".to_string())
            .to_ascii_lowercase()
            .as_str()
        {
            "off" => PluginTrustMode::Off,
            "enforce" => PluginTrustMode::Enforce,
            _ => PluginTrustMode::Warn,
        };

        Self {
            mode,
            allowlist: parse_csv_env("SOROBAN_DEBUG_PLUGIN_ALLOWLIST"),
            denylist: parse_csv_env("SOROBAN_DEBUG_PLUGIN_DENYLIST"),
            allowed_signers: parse_csv_env("SOROBAN_DEBUG_PLUGIN_ALLOWED_SIGNERS"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginTrustAssessment {
    pub trusted: bool,
    pub warnings: Vec<String>,
    pub signer: Option<VerifiedPluginSignature>,
}

impl PluginLoader {
    /// Create a new plugin loader
    pub fn new(plugin_dir: PathBuf) -> Self {
        Self::with_trust_policy(plugin_dir, PluginTrustPolicy::default())
    }

    /// Create a new plugin loader with an explicit trust policy
    pub fn with_trust_policy(plugin_dir: PathBuf, trust_policy: PluginTrustPolicy) -> Self {
        Self {
            plugin_dir,
            trust_policy,
        }
    }

    /// Get the default plugin directory (~/.soroban-debug/plugins/)
    pub fn default_plugin_dir() -> PluginResult<PathBuf> {
        let home = dirs::home_dir().ok_or_else(|| {
            PluginError::InitializationFailed("Could not determine home directory".to_string())
        })?;

        Ok(home.join(".soroban-debug").join("plugins"))
    }

    /// Load a plugin from a manifest file
    pub fn load_from_manifest(&self, manifest_path: &Path) -> PluginResult<LoadedPlugin> {
        info!("Loading plugin from manifest: {:?}", manifest_path);

        // Load and validate manifest
        let manifest = PluginManifest::from_file(&manifest_path.to_path_buf())
            .map_err(|e| PluginError::Invalid(format!("Failed to load manifest: {}", e)))?;

        manifest
            .validate()
            .map_err(|e| PluginError::Invalid(format!("Invalid manifest: {}", e)))?;

        // Resolve library path relative to manifest
        let manifest_dir = manifest_path
            .parent()
            .ok_or_else(|| PluginError::Invalid("Invalid manifest path".to_string()))?;

        let mut library_path = manifest_dir.join(&manifest.library);
        if !library_path.exists() {
            if let Some(fallback) = resolve_platform_library_path(manifest_dir, &manifest.library) {
                library_path = fallback;
            }
        }

        if !library_path.exists() {
            return Err(PluginError::NotFound(format!(
                "Plugin library not found: {:?}",
                library_path
            )));
        }

        let library_bytes = std::fs::read(&library_path).map_err(|e| {
            PluginError::InitializationFailed(format!(
                "Failed to read plugin library for trust verification: {}",
                e
            ))
        })?;
        let trust = self.assess_trust(&manifest, &library_path, &library_bytes)?;
        for warning in &trust.warnings {
            warn!("{}", warning);
        }

        // Load the dynamic library
        self.load_library(&library_path, manifest, trust)
    }

    /// Load a plugin directly from a library path
    pub fn load_library(
        &self,
        library_path: &Path,
        manifest: PluginManifest,
        trust: PluginTrustAssessment,
    ) -> PluginResult<LoadedPlugin> {
        info!("Loading plugin library: {:?}", library_path);

        unsafe {
            // Load the library
            let library = libloading::Library::new(library_path).map_err(|e| {
                PluginError::InitializationFailed(format!("Failed to load library: {}", e))
            })?;

            // Get the constructor symbol
            let constructor: libloading::Symbol<PluginConstructor> = library
                .get(PLUGIN_CONSTRUCTOR_SYMBOL.as_bytes())
                .map_err(|e| {
                    PluginError::Invalid(format!(
                        "Plugin does not export '{}': {}",
                        PLUGIN_CONSTRUCTOR_SYMBOL, e
                    ))
                })?;

            // Create the plugin instance
            let plugin_ptr = constructor();
            if plugin_ptr.is_null() {
                return Err(PluginError::InitializationFailed(
                    "Plugin constructor returned null".to_string(),
                ));
            }

            let mut plugin = Box::from_raw(plugin_ptr);

            // Verify manifest matches
            let plugin_manifest = plugin.metadata();
            if plugin_manifest.name != manifest.name {
                warn!(
                    "Plugin manifest name mismatch: expected '{}', got '{}'",
                    manifest.name, plugin_manifest.name
                );
            }

            // Initialize the plugin
            plugin.initialize().map_err(|e| {
                PluginError::InitializationFailed(format!("Plugin initialization failed: {}", e))
            })?;

            info!(
                "Successfully loaded plugin: {} v{}",
                manifest.name, manifest.version
            );

            Ok(LoadedPlugin {
                plugin,
                library: Some(library),
                path: library_path.to_path_buf(),
                manifest: manifest.clone(),
                trust,
            })
        }
    }

    /// Discover all plugins in the plugin directory.
    ///
    /// Results are sorted by path so the discovery order is deterministic
    /// across platforms and file-system implementations.  The registry's
    /// topological sort handles dependency ordering; this sort ensures that
    /// unrelated plugins always appear in the same sequence, making behaviour
    /// reproducible and tests stable.
    pub fn discover_plugins(&self) -> Vec<PathBuf> {
        let mut manifests = Vec::new();

        if !self.plugin_dir.exists() {
            info!("Plugin directory does not exist: {:?}", self.plugin_dir);
            return manifests;
        }

        // Look for plugin.toml files in subdirectories
        if let Ok(entries) = std::fs::read_dir(&self.plugin_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let manifest_path = path.join("plugin.toml");
                    if manifest_path.exists() {
                        manifests.push(manifest_path);
                    }
                }
            }
        }

        // Sort for deterministic, platform-independent discovery order.
        // Dependency ordering is handled by the registry's topological sort.
        manifests.sort();

        info!("Discovered {} plugin manifests", manifests.len());
        manifests
    }

    /// Load all discovered plugins
    pub fn load_all(&self) -> Vec<PluginResult<LoadedPlugin>> {
        let manifests = self.discover_plugins();

        manifests
            .iter()
            .map(|manifest_path| self.load_from_manifest(manifest_path))
            .collect()
    }

    pub(crate) fn assess_trust(
        &self,
        manifest: &PluginManifest,
        library_path: &Path,
        library_bytes: &[u8],
    ) -> PluginResult<PluginTrustAssessment> {
        if self.trust_policy.mode == PluginTrustMode::Off {
            return Ok(PluginTrustAssessment {
                trusted: true,
                warnings: Vec::new(),
                signer: None,
            });
        }

        let mut warnings = Vec::new();
        let plugin_name = manifest.name.as_str();

        if self.trust_policy.denylist.contains(plugin_name) {
            return Err(PluginError::TrustViolation(format!(
                "Plugin '{}' is denied by policy. Remove it from SOROBAN_DEBUG_PLUGIN_DENYLIST or delete the plugin directory before retrying.",
                plugin_name
            )));
        }

        let allowlisted = self.trust_policy.allowlist.contains(plugin_name);
        let mut trusted = allowlisted;
        let mut signer = None;

        match manifest.verify_signatures(library_bytes) {
            Ok(verified) => {
                let signer_allowed = self.trust_policy.allowed_signers.is_empty()
                    || self.trust_policy.allowed_signers.contains(&verified.signer)
                    || self
                        .trust_policy
                        .allowed_signers
                        .contains(&verified.fingerprint);
                if signer_allowed {
                    trusted = true;
                } else {
                    warnings.push(format!(
                        "Plugin '{}' is signed by '{}' ({}) but that signer is not allowlisted. Add the signer or fingerprint to SOROBAN_DEBUG_PLUGIN_ALLOWED_SIGNERS, or add the plugin to SOROBAN_DEBUG_PLUGIN_ALLOWLIST if you intend to trust it explicitly.",
                        plugin_name,
                        verified.signer,
                        verified.fingerprint
                    ));
                }
                signer = Some(verified);
            }
            Err(err) => {
                warnings.push(format!(
                    "Plugin '{}' at {:?} is unsigned or failed signature verification: {}. Sign the manifest and library, add the plugin to SOROBAN_DEBUG_PLUGIN_ALLOWLIST, or set SOROBAN_DEBUG_PLUGIN_TRUST_MODE=off for local-only debugging.",
                    plugin_name,
                    library_path,
                    err
                ));
            }
        }

        if !self.trust_policy.allowlist.is_empty() && !allowlisted && !trusted {
            warnings.push(format!(
                "Plugin '{}' is not in the plugin allowlist. Add it to SOROBAN_DEBUG_PLUGIN_ALLOWLIST after reviewing the source and signer.",
                plugin_name
            ));
        }

        if self.trust_policy.mode == PluginTrustMode::Enforce && !trusted {
            return Err(PluginError::TrustViolation(warnings.join(" ")));
        }

        Ok(PluginTrustAssessment {
            trusted,
            warnings,
            signer,
        })
    }
}

fn parse_csv_env(name: &str) -> BTreeSet<String> {
    std::env::var(name)
        .ok()
        .into_iter()
        .flat_map(|value| value.split(',').map(str::to_string).collect::<Vec<_>>())
        .map(|entry| entry.trim().to_string())
        .filter(|entry| !entry.is_empty())
        .collect()
}

fn resolve_platform_library_path(manifest_dir: &Path, library: &str) -> Option<PathBuf> {
    let wanted_ext = match std::env::consts::OS {
        "windows" => "dll",
        "macos" => "dylib",
        _ => "so",
    };

    let base = Path::new(library);
    let stem = base.file_stem()?.to_string_lossy();
    let file_name = base.file_name()?.to_string_lossy();

    let mut candidates = Vec::new();

    // 1) Same stem, correct extension.
    candidates.push(format!("{stem}.{wanted_ext}"));

    // 2) Try toggling the `lib` prefix for cross-platform portability.
    if wanted_ext == "dll" && file_name.starts_with("lib") {
        candidates.push(format!("{}.dll", stem.trim_start_matches("lib")));
    } else if wanted_ext != "dll" && !file_name.starts_with("lib") {
        candidates.push(format!("lib{stem}.{wanted_ext}"));
    }

    for candidate in candidates {
        let p = manifest_dir.join(candidate);
        if p.exists() {
            return Some(p);
        }
    }

    None
}

impl Drop for LoadedPlugin {
    fn drop(&mut self) {
        info!("Unloading plugin: {}", self.manifest.name);

        if let Err(e) = self.plugin.shutdown() {
            error!("Error shutting down plugin {}: {}", self.manifest.name, e);
        }
    }
}



#[cfg(test)]
impl LoadedPlugin {
    pub(crate) fn from_parts_for_tests(
        plugin: Box<dyn InspectorPlugin>,
        path: PathBuf,
        manifest: PluginManifest,
        trust: PluginTrustAssessment,
    ) -> Self {
        Self {
            plugin,
            library: None,
            path,
            manifest,
            trust,
        }
/// Checks if the plugin API version matches the host's expected version.
pub fn check_api_version(plugin_version: u32) -> Result<(), crate::plugin::api::PluginError> {
    use crate::plugin::api::{PluginError, PLUGIN_API_VERSION};
    if plugin_version != PLUGIN_API_VERSION {
        return Err(PluginError::VersionMismatch {
            expected: PLUGIN_API_VERSION,
            found: plugin_version,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::manifest::PluginSignature;
mod version_tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
    use base64::Engine;
    use ed25519_dalek::{Signer, SigningKey};
    use std::collections::BTreeSet;
    use std::path::Path;

    fn base_manifest(name: &str) -> PluginManifest {
        PluginManifest {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            description: "test plugin".to_string(),
            author: "test".to_string(),
            license: Some("MIT".to_string()),
            min_debugger_version: Some("0.1.0".to_string()),
            capabilities: Default::default(),
            library: "plugin.so".to_string(),
            dependencies: vec![],
            signature: None,
        }
    }

    fn sign_manifest(
        mut manifest: PluginManifest,
        signer_name: &str,
        seed: u8,
        library_bytes: &[u8],
    ) -> PluginManifest {
        let signing_key = SigningKey::from_bytes(&[seed; 32]);
        let verifying_key = signing_key.verifying_key();
        let manifest_payload = manifest.canonical_manifest_payload().unwrap();
        let manifest_signature = signing_key.sign(&manifest_payload);
        let library_signature = signing_key.sign(library_bytes);
        manifest.signature = Some(PluginSignature {
            signer: signer_name.to_string(),
            public_key: BASE64_STANDARD.encode(verifying_key.to_bytes()),
            manifest_signature: BASE64_STANDARD.encode(manifest_signature.to_bytes()),
            library_signature: BASE64_STANDARD.encode(library_signature.to_bytes()),
        });
        manifest
    }
    use crate::plugin::api::{PluginError, PLUGIN_API_VERSION};

    #[test]
    fn test_default_plugin_dir() {
        let dir = PluginLoader::default_plugin_dir();
        assert!(dir.is_ok());

        let path = dir.unwrap();
        assert!(path.ends_with(".soroban-debug/plugins"));
    }
    fn test_api_version_check() {
        let result = check_api_version(999);
        assert!(matches!(result, Err(PluginError::VersionMismatch { .. })));

    #[test]
    fn test_loader_creation() {
        let temp_dir = std::env::temp_dir();
        let loader = PluginLoader::new(temp_dir.clone());
        assert_eq!(loader.plugin_dir, temp_dir);
    }

    /// `discover_plugins` must return paths in sorted order so that repeated
    /// calls on the same directory yield the same sequence regardless of the
    /// order the OS returns directory entries.
    #[test]
    fn discover_plugins_returns_sorted_paths() {
        use std::fs;

        let base = std::env::temp_dir().join("soroban-loader-sort-test");
        let _ = fs::remove_dir_all(&base);

        // Create three plugin sub-directories in reverse alphabetical order so
        // a naive read_dir would likely return them unsorted.
        //............
        for name in &["plugin-c", "plugin-a", "plugin-b"] {
            let dir = base.join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("plugin.toml"), "").unwrap();
        }

        let loader = PluginLoader::new(base.clone());
        let paths = loader.discover_plugins();

        let names: Vec<&str> = paths
            .iter()
            .filter_map(|p| p.parent()?.file_name()?.to_str())
            .collect();

        assert_eq!(names, vec!["plugin-a", "plugin-b", "plugin-c"]);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn trust_policy_warns_but_allows_unsigned_plugins_by_default() {
        let loader = PluginLoader::with_trust_policy(
            std::env::temp_dir(),
            PluginTrustPolicy {
                mode: PluginTrustMode::Warn,
                allowlist: BTreeSet::new(),
                denylist: BTreeSet::new(),
                allowed_signers: BTreeSet::new(),
            },
        );
        let manifest = base_manifest("unsigned-plugin");

        let assessment = loader
            .assess_trust(&manifest, Path::new("unsigned-plugin.so"), b"library")
            .expect("warn mode should allow unsigned plugin");

        assert!(!assessment.trusted);
        assert!(!assessment.warnings.is_empty());
    }

    #[test]
    fn trust_policy_blocks_unsigned_plugins_in_enforce_mode() {
        let loader = PluginLoader::with_trust_policy(
            std::env::temp_dir(),
            PluginTrustPolicy {
                mode: PluginTrustMode::Enforce,
                allowlist: BTreeSet::new(),
                denylist: BTreeSet::new(),
                allowed_signers: BTreeSet::new(),
            },
        );
        let manifest = base_manifest("unsigned-plugin");

        let err = loader
            .assess_trust(&manifest, Path::new("unsigned-plugin.so"), b"library")
            .unwrap_err();
        assert!(
            matches!(err, PluginError::TrustViolation(message) if message.contains("unsigned") || message.contains("signature"))
        );
    }

    #[test]
    fn trust_policy_blocks_denylisted_plugins() {
        let mut denylist = BTreeSet::new();
        denylist.insert("blocked-plugin".to_string());
        let loader = PluginLoader::with_trust_policy(
            std::env::temp_dir(),
            PluginTrustPolicy {
                mode: PluginTrustMode::Warn,
                allowlist: BTreeSet::new(),
                denylist,
                allowed_signers: BTreeSet::new(),
            },
        );

        let err = loader
            .assess_trust(
                &base_manifest("blocked-plugin"),
                Path::new("blocked.so"),
                b"library",
            )
            .unwrap_err();
        assert!(
            matches!(err, PluginError::TrustViolation(message) if message.contains("denied by policy"))
        );
    }

    #[test]
    fn trust_policy_accepts_valid_signed_plugins_from_allowed_signer() {
        let library_bytes = b"signed library";
        let manifest = sign_manifest(
            base_manifest("signed-plugin"),
            "trusted-signer",
            9,
            library_bytes,
        );
        let mut allowed_signers = BTreeSet::new();
        allowed_signers.insert("trusted-signer".to_string());
        let loader = PluginLoader::with_trust_policy(
            std::env::temp_dir(),
            PluginTrustPolicy {
                mode: PluginTrustMode::Enforce,
                allowlist: BTreeSet::new(),
                denylist: BTreeSet::new(),
                allowed_signers,
            },
        );

        let assessment = loader
            .assess_trust(&manifest, Path::new("signed.so"), library_bytes)
            .expect("trusted signed plugin should load");

        assert!(assessment.trusted);
        assert!(assessment.warnings.is_empty());
        assert_eq!(
            assessment.signer.as_ref().map(|s| s.signer.as_str()),
            Some("trusted-signer")
        );
        let result_ok = check_api_version(PLUGIN_API_VERSION);
        assert!(result_ok.is_ok());
