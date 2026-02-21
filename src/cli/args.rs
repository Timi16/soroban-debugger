use crate::config::Config;
use clap::{Parser, Subcommand};
use clap_complete::Shell;
use std::path::PathBuf;

/// Verbosity level for output control
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verbosity {
    Quiet,
    Normal,
    Verbose,
}

impl Verbosity {
    /// Convert verbosity to log level string for RUST_LOG
    pub fn to_log_level(self) -> String {
        match self {
            Verbosity::Quiet => "error".to_string(),
            Verbosity::Normal => "info".to_string(),
            Verbosity::Verbose => "debug".to_string(),
        }
    }
}

#[derive(Parser)]
#[command(name = "soroban-debug")]
#[command(about = "A debugger for Soroban smart contracts", long_about = None)]
#[command(version)]
pub struct Cli {
    /// Suppress non-essential output (errors and return value only)
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Show verbose output including internal details
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Show detailed version information
    #[arg(long)]
    pub version_verbose: bool,
}

impl Cli {
    /// Get the effective verbosity level
    pub fn verbosity(&self) -> Verbosity {
        if self.quiet {
            Verbosity::Quiet
        } else if self.verbose {
            Verbosity::Verbose
        } else {
            Verbosity::Normal
        }
    }
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run a contract function with the debugger
    Run(RunArgs),

    /// Start an interactive debugging session
    Interactive(InteractiveArgs),

    /// Inspect contract information without executing
    Inspect(InspectArgs),

    /// Generate shell completion scripts
    Completions(CompletionsArgs),
    /// Analyze contract and generate gas optimization suggestions
    Optimize(OptimizeArgs),

    /// Profile a single function execution and print hotspots + suggestions
    Profile(ProfileArgs),
    /// Check compatibility between two contract versions
    UpgradeCheck(UpgradeCheckArgs),

    /// Compare two execution trace JSON files side-by-side
    Compare(CompareArgs),
}

#[derive(Parser)]
pub struct RunArgs {
    /// Path to the contract WASM file
    #[arg(short, long)]
    pub contract: PathBuf,

    /// Function name to execute
    #[arg(short, long)]
    pub function: String,

    /// Function arguments as JSON array (e.g., '["arg1", "arg2"]')
    #[arg(short, long)]
    pub args: Option<String>,

    /// Initial storage state as JSON object
    #[arg(short, long)]
    pub storage: Option<String>,

    /// Set breakpoint at function name
    #[arg(short, long)]
    pub breakpoint: Vec<String>,

    /// Network snapshot file to load before execution
    #[arg(long)]
    pub network_snapshot: Option<PathBuf>,

    /// Enable verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Output format (text, json)
    #[arg(long)]
    pub format: Option<String>,

    /// Show contract events emitted during execution
    #[arg(long)]
    pub show_events: bool,

    /// Show authorization tree during execution
    #[arg(long)]
    pub show_auth: bool,

    /// Output format as JSON
    #[arg(long)]
    pub json: bool,

    /// Filter events by topic
    #[arg(long)]
    pub filter_topic: Option<String>,

    /// Execute the contract call N times for stress testing
    #[arg(long)]
    pub repeat: Option<u32>,

    /// Filter storage output by key pattern (repeatable). Supports:
    ///   prefix*       — match keys starting with prefix
    ///   re:<regex>    — match keys by regex
    ///   exact_key     — match key exactly
    #[arg(long, value_name = "PATTERN")]
    pub storage_filter: Vec<String>,

    /// Enable instruction-level debugging
    #[arg(long)]
    pub instruction_debug: bool,

    /// Start with instruction stepping enabled
    #[arg(long)]
    pub step_instructions: bool,

    /// Step mode for instruction debugging (into, over, out, block)
    #[arg(long, default_value = "into")]
    pub step_mode: String,
    /// Execute contract in dry-run mode: simulate execution without persisting storage changes
    #[arg(long)]
    pub dry_run: bool,

    /// Export storage state to JSON file after execution
    #[arg(long)]
    pub export_storage: Option<PathBuf>,

    /// Import storage state from JSON file before execution
    #[arg(long)]
    pub import_storage: Option<PathBuf>,
    /// Path to JSON file containing array of argument sets for batch execution
    #[arg(long)]
    pub batch_args: Option<PathBuf>,
}

impl RunArgs {
    pub fn merge_config(&mut self, config: &Config) {
        // Breakpoints
        if self.breakpoint.is_empty() && !config.debug.breakpoints.is_empty() {
            self.breakpoint = config.debug.breakpoints.clone();
        }

        // Show events
        if !self.show_events {
            if let Some(show) = config.output.show_events {
                self.show_events = show;
            }
        }

        // Output Format
        if self.format.is_none() {
            self.format = config.output.format.clone();
        }

        // Verbosity: if config has a level > 0 and CLI verbose is false, enable it
        if !self.verbose {
            if let Some(level) = config.debug.verbosity {
                if level > 0 {
                    self.verbose = true;
                }
            }
        }
    }
}

#[derive(Parser)]
pub struct InteractiveArgs {
    /// Path to the contract WASM file
    #[arg(short, long)]
    pub contract: PathBuf,

    /// Network snapshot file to load before starting interactive session
    #[arg(long)]
    pub network_snapshot: Option<PathBuf>,
}

impl InteractiveArgs {
    pub fn merge_config(&mut self, _config: &Config) {
        // Future interactive-specific config could go here
    }
}

#[derive(Parser)]
pub struct InspectArgs {
    /// Path to the contract WASM file
    #[arg(short, long)]
    pub contract: PathBuf,

    /// Show exported functions
    #[arg(long)]
    pub functions: bool,

    /// Show contract metadata
    #[arg(long)]
    pub metadata: bool,
}

#[derive(Parser)]
pub struct OptimizeArgs {
    /// Path to the contract WASM file
    #[arg(short, long)]
    pub contract: PathBuf,

    /// Function name to analyze (can be specified multiple times)
    #[arg(short, long)]
    pub function: Vec<String>,

    /// Function arguments as JSON array (e.g., '["arg1", "arg2"]')
    #[arg(short, long)]
    pub args: Option<String>,

    /// Output file for the optimization report (default: stdout)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Initial storage state as JSON object
    #[arg(short, long)]
    pub storage: Option<String>,

    /// Network snapshot file to load before analysis
    #[arg(long)]
    pub network_snapshot: Option<PathBuf>,
}

#[derive(Parser)]
pub struct UpgradeCheckArgs {
    /// Path to the old contract WASM file
    #[arg(short, long)]
    pub old: PathBuf,

    /// Path to the new contract WASM file
    #[arg(short, long)]
    pub new: PathBuf,

    /// Function name to test side-by-side (optional)
    #[arg(short, long)]
    pub function: Option<String>,

    /// Function arguments as JSON array for side-by-side test
    #[arg(short, long)]
    pub args: Option<String>,

    /// Output file for the compatibility report (default: stdout)
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Parser)]
pub struct CompareArgs {
    /// Path to the first execution trace JSON file (trace A)
    #[arg(value_name = "TRACE_A")]
    pub trace_a: PathBuf,

    /// Path to the second execution trace JSON file (trace B)
    #[arg(value_name = "TRACE_B")]
    pub trace_b: PathBuf,

    /// Output file for the comparison report (default: stdout)
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

#[derive(Parser)]
pub struct CompletionsArgs {
    /// Shell to generate completion script for
    #[arg(value_enum)]
    pub shell: Shell,
}

#[derive(Parser)]
pub struct ProfileArgs {
    /// Path to the contract WASM file
    #[arg(short, long)]
    pub contract: PathBuf,

    /// Function name to execute
    #[arg(short, long)]
    pub function: String,

    /// Function arguments as JSON array (e.g., '["arg1", "arg2"]')
    #[arg(short, long)]
    pub args: Option<String>,

    /// Output file for the profile report (default: stdout)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Initial storage state as JSON object
    #[arg(short, long)]
    pub storage: Option<String>,
}
