use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tracing::warn;

/// Default configuration file name
pub const DEFAULT_CONFIG_FILE: &str = ".soroban-debug.toml";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub debug: DebugConfig,
    #[serde(default)]
    pub output: OutputConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DebugConfig {
    /// Default breakpoints to set
    #[serde(default)]
    pub breakpoints: Vec<String>,
    /// Default verbosity level (0-3)
    #[serde(default)]
    pub verbosity: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OutputConfig {
    /// Default output format (e.g., "text", "json")
    #[serde(default)]
    pub format: Option<String>,
    /// Show events by default
    #[serde(default)]
    pub show_events: Option<bool>,
}

impl Config {
    /// Load configuration from a file in the project root
    pub fn load() -> Result<Self> {
        let config_path = Path::new(DEFAULT_CONFIG_FILE);

        if !config_path.exists() {
            return Ok(Config::default());
        }

        let content = fs::read_to_string(config_path)
            .with_context(|| format!("Failed to read config file: {:?}", config_path))?;

        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse TOML config from: {:?}", config_path))?;

        Ok(config)
    }

    /// Load default config if file is missing, otherwise return error on parse failure
    pub fn load_or_default() -> Self {
        match Self::load() {
            Ok(config) => config,
            Err(e) => {
                warn!("Warning: Failed to load config: {}. Using defaults.", e);
                Config::default()
            }
        }
    }
}
