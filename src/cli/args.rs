use clap::{Parser, Subcommand};
use std::path::PathBuf;
use crate::config::Config;

#[derive(Parser)]
#[command(name = "soroban-debug")]
#[command(about = "A debugger for Soroban smart contracts", long_about = None)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run a contract function with the debugger
    Run(RunArgs),

    /// Start an interactive debugging session
    Interactive(InteractiveArgs),

    /// Inspect contract information without executing
    Inspect(InspectArgs),

    /// Analyze contract and generate gas optimization suggestions
    Optimize(OptimizeArgs),

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
    #[arg(short, long)]
    pub format: Option<String>,

    /// Show contract events emitted during execution
    #[arg(long)]
    pub show_events: bool,

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

    /// Enable verbose output
    #[arg(short, long)]
    pub verbose: bool,
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
