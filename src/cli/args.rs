use crate::config::Config;
use clap::{Parser, Subcommand, ValueEnum};

use clap_complete::Shell;
use std::path::PathBuf;

/// Mapping of deprecated CLI flags to their new equivalents
/// Used to show deprecation warnings when old flags are used
pub const DEPRECATED_FLAGS: &[(&str, &str)] = &[
    ("--wasm", "--contract"),
    ("--contract-path", "--contract"),
    ("--snapshot", "--network-snapshot"),
];

/// Get a deprecation warning message for a deprecated flag
/// Returns None if the flag is not deprecated
pub fn get_deprecation_warning(deprecated_flag: &str) -> Option<String> {
    DEPRECATED_FLAGS
        .iter()
        .find(|(old, _)| *old == deprecated_flag)
        .map(|(old, new)| {
            format!(
                "⚠️  Flag '{}' is deprecated. Please use '{}' instead.",
                old, new
            )
        })
}

/// Verbosity level for output control
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verbosity {
    Quiet,
    Normal,
    Verbose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum OutputFormat {
    #[default]
    Pretty,
    Json,
}

/// Format for dependency graph output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum GraphFormat {
    Dot,
    Mermaid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum SymbolicProfile {
    Fast,
    #[default]
    Balanced,
    Deep,
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

    /// Suppress startup banner output
    #[arg(long, global = true)]
    pub no_banner: bool,

    /// Override the history file location (useful for CI, sandboxes, and per-project isolation)
    ///
    /// Equivalent to setting `SOROBAN_DEBUG_HISTORY_FILE`.
    #[arg(
        long,
        global = true,
        env = "SOROBAN_DEBUG_HISTORY_FILE",
        value_name = "FILE"
    )]
    pub history_file: Option<PathBuf>,

    /// Maximum number of history records to retain.
    ///
    /// When set, the oldest records are dropped after each appended run so the
    /// file never exceeds N entries.  Equivalent to setting
    /// `SOROBAN_DEBUG_HISTORY_MAX_RECORDS`.
    #[arg(
        long,
        global = true,
        env = "SOROBAN_DEBUG_HISTORY_MAX_RECORDS",
        value_name = "N"
    )]
    pub history_max_records: Option<usize>,

    /// Drop history records older than N days.
    ///
    /// Applied on every append and when running `history prune`.  Equivalent
    /// to setting `SOROBAN_DEBUG_HISTORY_MAX_AGE_DAYS`.
    #[arg(
        long,
        global = true,
        env = "SOROBAN_DEBUG_HISTORY_MAX_AGE_DAYS",
        value_name = "DAYS"
    )]
    pub history_max_age_days: Option<u64>,

    /// Show historical budget trend visualization
    #[arg(long)]
    pub budget_trend: bool,

    /// Filter budget trend by contract hash
    #[arg(long)]
    pub trend_contract: Option<String>,

    /// Filter budget trend by function name
    #[arg(long)]
    pub trend_function: Option<String>,

    /// Regression threshold percentage for `--budget-trend` warnings
    #[arg(long, default_value_t = 10.0, value_name = "PCT")]
    pub trend_regression_threshold_pct: f64,

    /// Lookback window (number of runs) for `--budget-trend` regression detection
    #[arg(long, default_value_t = 2, value_name = "N")]
    pub trend_regression_lookback: usize,

    /// Smoothing window (moving average) for `--budget-trend` regression detection (1 disables smoothing)
    #[arg(long, default_value_t = 1, value_name = "N")]
    pub trend_regression_smoothing: usize,

    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Show detailed version information
    #[arg(long)]
    pub version_verbose: bool,

    /// Show exported functions for a given contract (shorthand for inspect --functions)
    #[arg(long)]
    pub list_functions: Option<PathBuf>,
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
#[allow(clippy::large_enum_variant)]
pub enum Commands {
    /// Run a contract function with the debugger
    Run(RunArgs),

    /// Start an interactive debugging session
    Interactive(InteractiveArgs),

    /// Start an interactive REPL for contract exploration
    Repl(ReplArgs),

    /// Launch the full-screen TUI dashboard
    Tui(TuiArgs),

    /// Inspect contract information without executing
    Inspect(InspectArgs),

    /// Check compatibility between two contract versions
    UpgradeCheck(UpgradeCheckArgs),

    /// Generate shell completion scripts
    Completions(CompletionsArgs),

    /// Analyze contract and generate gas optimization suggestions
    Optimize(OptimizeArgs),

    /// Profile a single function execution and print hotspots + suggestions
    Profile(ProfileArgs),

    /// Compare two execution trace JSON files side-by-side
    Compare(CompareArgs),

    /// Replay execution from a previously exported trace file
    Replay(ReplayArgs),

    /// Run symbolic execution to explore contract input space
    Symbolic(SymbolicArgs),

    /// Start debug server for remote connections
    Server(ServerArgs),

    /// Connect to remote debug server
    Remote(RemoteArgs),

    /// Analyze contract for security vulnerabilities
    Analyze(AnalyzeArgs),

    /// Run a multi-step scenario from a TOML file
    Scenario(ScenarioArgs),

    /// Prune or compact run history according to a retention policy
    HistoryPrune(HistoryPruneArgs),

    /// Plugin-provided subcommand (loaded at runtime)
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Parser)]
pub struct RunArgs {
    /// Path to the contract WASM file
    #[arg(short, long, required_unless_present_any = ["server", "remote"])]
    pub contract: Option<PathBuf>,

    /// Deprecated: use --contract instead
    #[arg(long, hide = true, alias = "wasm", alias = "contract-path")]
    pub wasm: Option<PathBuf>,

    /// Function name to execute
    #[arg(short, long, required_unless_present_any = ["server", "remote"])]
    pub function: Option<String>,

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

    /// Deprecated: use --network-snapshot instead
    #[arg(long, hide = true, alias = "snapshot")]
    pub snapshot: Option<PathBuf>,

    /// Enable verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Start in server mode
    #[arg(long)]
    pub server: bool,

    /// Port to listen on or connect to
    #[arg(short, long, default_value = "9229")]
    pub port: u16,

    /// Connect to a remote debugger (address:port)
    #[arg(long)]
    pub remote: Option<String>,

    /// Authentication token
    #[arg(short, long)]
    pub token: Option<String>,

    /// Path to TLS certificate file
    #[arg(long)]
    pub tls_cert: Option<std::path::PathBuf>,

    /// Path to TLS key file
    #[arg(long)]
    pub tls_key: Option<std::path::PathBuf>,
    /// Output format (text, json)
    #[arg(long)]
    pub format: Option<String>,

    /// Output mode for command result rendering (pretty, json)
    #[arg(long = "output", value_enum, default_value_t = OutputFormat::Pretty)]
    pub output_format: OutputFormat,

    /// Show contract events emitted during execution
    #[arg(long)]
    pub show_events: bool,

    /// Show authorization tree during execution
    #[arg(long)]
    pub show_auth: bool,

    /// Output format as JSON
    #[arg(long)]
    pub json: bool,

    /// Filter events by topic (deprecated single value). Prefer using --event-filter (repeatable).
    #[arg(long)]
    pub filter_topic: Option<String>,

    /// Filter events by topic pattern (repeatable)
    #[arg(long, value_name = "PATTERN")]
    pub event_filter: Vec<String>,

    /// Execute the contract call N times for stress testing
    #[arg(long)]
    pub repeat: Option<u32>,

    /// Mock cross-contract return: CONTRACT_ID.function=return_value (repeatable)
    #[arg(long, value_name = "CONTRACT_ID.function=return_value")]
    pub mock: Vec<String>,

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

    /// Automatically generate a unit test file from the execution trace
    #[arg(long, value_name = "FILE")]
    pub generate_test: Option<PathBuf>,

    /// Overwrite the test file if it already exists (default: append)

    #[arg(long)]
    pub overwrite: bool,

    /// Execution timeout in seconds (default: 30)
    #[arg(long, default_value = "30")]
    pub timeout: u64,

    /// Trigger a prominent alert when a critical storage key is modified (repeatable)
    #[arg(long, value_name = "KEY_PATTERN")]
    pub alert_on_change: Vec<String>,

    /// Expected SHA-256 hash of the WASM file. If provided, loading will fail if the computed hash does not match.
    #[arg(long)]
    pub expected_hash: Option<String>,

    /// Show ledger entries accessed during execution
    #[arg(long)]
    pub show_ledger: bool,

    /// TTL warning threshold in ledger sequence numbers (default: 1000)
    #[arg(long, default_value = "1000")]
    pub ttl_warning_threshold: u32,

    /// Export execution trace to JSON file
    #[arg(long)]
    pub trace_output: Option<PathBuf>,
    /// Path to file where execution results should be saved
    #[arg(long, value_name = "FILE")]
    pub save_output: Option<PathBuf>,

    /// Append to output file instead of overwriting (used with --save-output)
    #[arg(long)]
    pub append: bool,
}

impl RunArgs {
    pub fn is_json_output(&self) -> bool {
        self.output_format == OutputFormat::Json
            || self.json
            || self
                .format
                .as_deref()
                .map(|f| f.eq_ignore_ascii_case("json"))
                .unwrap_or(false)
    }

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

    /// Deprecated: use --contract instead
    #[arg(long, hide = true, alias = "wasm", alias = "contract-path")]
    pub wasm: Option<PathBuf>,

    /// Network snapshot file to load before starting interactive session
    #[arg(long)]
    pub network_snapshot: Option<PathBuf>,

    /// Deprecated: use --network-snapshot instead
    #[arg(long, hide = true, alias = "snapshot")]
    pub snapshot: Option<PathBuf>,

    /// Function name to execute (staged; use 'continue' inside the session)
    #[arg(short, long)]
    pub function: String,

    /// Function arguments as JSON array (e.g., '["arg1", "arg2"]')
    #[arg(short, long)]
    pub args: Option<String>,

    /// Initial storage state as JSON object
    #[arg(short, long)]
    pub storage: Option<String>,

    /// Import storage state from JSON file before starting the session
    #[arg(long)]
    pub import_storage: Option<PathBuf>,

    /// Set breakpoint at function name
    #[arg(short, long)]
    pub breakpoint: Vec<String>,

    /// Mock cross-contract return: CONTRACT_ID.function=return_value (repeatable)
    #[arg(long, value_name = "CONTRACT_ID.function=return_value")]
    pub mock: Vec<String>,

    /// Execution timeout in seconds (default: 30)
    #[arg(long, default_value = "30")]
    pub timeout: u64,

    /// Enable instruction-level debugging
    #[arg(long)]
    pub instruction_debug: bool,

    /// Start with instruction stepping enabled
    #[arg(long)]
    pub step_instructions: bool,

    /// Step mode for instruction debugging (into, over, out, block)
    #[arg(long, default_value = "into")]
    pub step_mode: String,

    /// Expected SHA-256 hash of the WASM file. If provided, loading will fail if the computed hash does not match.
    #[arg(long)]
    pub expected_hash: Option<String>,
}

impl InteractiveArgs {
    pub fn merge_config(&mut self, _config: &Config) {
        // Future interactive-specific config could go here
    }
}

#[derive(Parser)]
pub struct ReplArgs {
    /// Path to the contract WASM file
    #[arg(short, long)]
    pub contract: PathBuf,

    /// Deprecated: use --contract instead
    #[arg(long, hide = true, alias = "wasm", alias = "contract-path")]
    pub wasm: Option<PathBuf>,

    /// Network snapshot file to load before starting REPL session
    /// Network snapshot file to load before starting interactive session
    #[arg(long)]
    pub network_snapshot: Option<PathBuf>,

    /// Deprecated: use --network-snapshot instead
    #[arg(long, hide = true, alias = "snapshot")]
    pub snapshot: Option<PathBuf>,

    /// Initial storage state as JSON object
    #[arg(short, long)]
    pub storage: Option<String>,

    /// Expected SHA-256 hash of the WASM file. If provided, loading will fail if the computed hash does not match.
    #[arg(long)]
    pub expected_hash: Option<String>,
}

impl ReplArgs {
    pub fn merge_config(&mut self, _config: &Config) {
        // Future REPL-specific config could go here
    }
}

#[derive(Parser)]
pub struct CompletionsArgs {
    /// Shell to generate completion script for
    #[arg(value_enum)]
    pub shell: Shell,
}
#[derive(Parser)]
pub struct InspectArgs {
    /// Path to the contract WASM file
    #[arg(short, long)]
    pub contract: PathBuf,

    /// Deprecated: use --contract instead
    #[arg(long, hide = true, alias = "wasm", alias = "contract-path")]
    pub wasm: Option<PathBuf>,

    /// Show exported functions
    #[arg(long)]
    pub functions: bool,

    /// Show contract metadata
    #[arg(long)]
    pub metadata: bool,

    /// Output format: pretty (default) or json
    #[arg(long, value_enum, default_value = "pretty")]
    pub format: OutputFormat,

    /// Expected SHA-256 hash of the WASM file. If provided, loading will fail if the computed hash does not match.
    #[arg(long)]
    pub expected_hash: Option<String>,

    /// Show cross-contract dependency graph in specified format
    #[arg(long, value_enum)]
    pub dependency_graph: Option<GraphFormat>,
}

#[derive(Parser)]
pub struct UpgradeCheckArgs {
    /// Path to the old (current) contract WASM file
    #[arg(long)]
    pub old: PathBuf,

    /// Path to the new (upgraded) contract WASM file
    #[arg(long)]
    pub new: PathBuf,

    /// Output format: text (default) or json
    #[arg(long, default_value = "text")]
    pub output: String,

    /// Write report to file instead of stdout
    #[arg(long)]
    pub output_file: Option<PathBuf>,

    /// Test inputs as JSON object mapping function names to argument arrays
    /// e.g. '{"vote": [1, true], "create_proposal": ["title", "desc"]}'
    #[arg(long)]
    pub test_inputs: Option<String>,
}

#[derive(Parser)]
pub struct OptimizeArgs {
    /// Path to the contract WASM file
    #[arg(short, long)]
    pub contract: PathBuf,

    /// Deprecated: use --contract instead
    #[arg(long, hide = true, alias = "wasm", alias = "contract-path")]
    pub wasm: Option<PathBuf>,

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

    /// Expected SHA-256 hash of the WASM file. If provided, loading will fail if the computed hash does not match.
    #[arg(long)]
    pub expected_hash: Option<String>,

    /// Deprecated: use --network-snapshot instead
    #[arg(long, hide = true, alias = "snapshot")]
    pub snapshot: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::{Cli, Commands, OutputFormat, SymbolicProfile};
    use clap::Parser;

    #[test]
    fn run_output_defaults_to_pretty() {
        let cli = Cli::parse_from([
            "soroban-debug",
            "run",
            "--contract",
            "contract.wasm",
            "--function",
            "increment",
        ]);

        let Commands::Run(args) = cli.command.expect("run command expected") else {
            panic!("run command expected");
        };

        assert_eq!(args.output_format, OutputFormat::Pretty);
        assert!(!args.is_json_output());
    }

    #[test]
    fn run_output_json_enables_json_mode() {
        let cli = Cli::parse_from([
            "soroban-debug",
            "run",
            "--contract",
            "contract.wasm",
            "--function",
            "increment",
            "--output",
            "json",
        ]);

        let Commands::Run(args) = cli.command.expect("run command expected") else {
            panic!("run command expected");
        };

        assert_eq!(args.output_format, OutputFormat::Json);
        assert!(args.is_json_output());
    }

    #[test]
    fn legacy_json_flag_still_enables_json_mode() {
        let cli = Cli::parse_from([
            "soroban-debug",
            "run",
            "--contract",
            "contract.wasm",
            "--function",
            "increment",
            "--json",
        ]);

        let Commands::Run(args) = cli.command.expect("run command expected") else {
            panic!("run command expected");
        };

        assert!(args.is_json_output());
    }

    #[test]
    fn legacy_format_json_still_enables_json_mode() {
        let cli = Cli::parse_from([
            "soroban-debug",
            "run",
            "--contract",
            "contract.wasm",
            "--function",
            "increment",
            "--format",
            "json",
        ]);

        let Commands::Run(args) = cli.command.expect("run command expected") else {
            panic!("run command expected");
        };

        assert!(args.is_json_output());
    }

    #[test]
    fn run_server_mode_does_not_require_contract_or_function() {
        let cli = Cli::try_parse_from([
            "soroban-debug",
            "run",
            "--server",
            "-p",
            "8888",
            "-t",
            "secret",
        ])
        .expect("failed to parse run --server");

        let Commands::Run(args) = cli.command.expect("run command expected") else {
            panic!("run command expected");
        };

        assert!(args.server);
        assert_eq!(args.port, 8888);
        assert_eq!(args.token, Some("secret".to_string()));
        assert!(args.contract.is_none());
        assert!(args.function.is_none());
    }

    #[test]
    fn symbolic_defaults_to_balanced_profile() {
        let cli = Cli::parse_from([
            "soroban-debug",
            "symbolic",
            "--contract",
            "contract.wasm",
            "--function",
            "increment",
        ]);

        let Commands::Symbolic(args) = cli.command.expect("symbolic command expected") else {
            panic!("symbolic command expected");
        };

        assert_eq!(args.profile, SymbolicProfile::Balanced);
        assert_eq!(args.input_combination_cap, None);
        assert_eq!(args.path_cap, None);
        assert_eq!(args.timeout, None);
    }

    #[test]
    fn symbolic_accepts_explicit_caps_and_profile() {
        let cli = Cli::parse_from([
            "soroban-debug",
            "symbolic",
            "--contract",
            "contract.wasm",
            "--function",
            "increment",
            "--profile",
            "deep",
            "--input-combination-cap",
            "512",
            "--path-cap",
            "200",
            "--timeout",
            "45",
        ]);

        let Commands::Symbolic(args) = cli.command.expect("symbolic command expected") else {
            panic!("symbolic command expected");
        };

        assert_eq!(args.profile, SymbolicProfile::Deep);
        assert_eq!(args.input_combination_cap, Some(512));
        assert_eq!(args.path_cap, Some(200));
        assert_eq!(args.timeout, Some(45));
    }

    #[test]
    fn symbolic_accepts_seed_flag() {
        let cli = Cli::parse_from([
            "soroban-debug",
            "symbolic",
            "--contract",
            "contract.wasm",
            "--function",
            "increment",
            "--seed",
            "42",
        ]);

        let Commands::Symbolic(args) = cli.command.expect("symbolic command expected") else {
            panic!("symbolic command expected");
        };

        assert_eq!(args.seed, Some(42));
        assert_eq!(args.replay, None);
    }

    #[test]
    fn symbolic_accepts_replay_flag() {
        let cli = Cli::parse_from([
            "soroban-debug",
            "symbolic",
            "--contract",
            "contract.wasm",
            "--function",
            "increment",
            "--replay",
            "99",
        ]);

        let Commands::Symbolic(args) = cli.command.expect("symbolic command expected") else {
            panic!("symbolic command expected");
        };

        assert_eq!(args.replay, Some(99));
        assert_eq!(args.seed, None);
    }

    #[test]
    fn symbolic_seed_and_replay_conflict() {
        let result = Cli::try_parse_from([
            "soroban-debug",
            "symbolic",
            "--contract",
            "contract.wasm",
            "--function",
            "increment",
            "--seed",
            "1",
            "--replay",
            "2",
        ]);

        assert!(
            result.is_err(),
            "--seed and --replay should be mutually exclusive"
        );
    }

    #[test]
    fn symbolic_no_seed_or_replay_defaults_to_none() {
        let cli = Cli::parse_from([
            "soroban-debug",
            "symbolic",
            "--contract",
            "contract.wasm",
            "--function",
            "increment",
        ]);

        let Commands::Symbolic(args) = cli.command.expect("symbolic command expected") else {
            panic!("symbolic command expected");
        };

        assert_eq!(args.seed, None);
        assert_eq!(args.replay, None);
    }
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

    /// Number of context lines to show around the first divergence
    #[arg(short = 'C', long, default_value_t = 3)]
    pub context: usize,
}

/// Arguments for the TUI dashboard subcommand
#[derive(Parser)]
pub struct TuiArgs {
    /// Path to the contract WASM file
    #[arg(short, long)]
    pub contract: PathBuf,

    /// Function name to execute inside the TUI
    #[arg(short, long)]
    pub function: String,

    /// Function arguments as JSON array (e.g., '["arg1", "arg2"]')
    #[arg(short, long)]
    pub args: Option<String>,

    /// Initial storage state as JSON object
    #[arg(short, long)]
    pub storage: Option<String>,

    /// Set breakpoints at function names
    #[arg(short, long)]
    pub breakpoint: Vec<String>,

    /// Network snapshot file to load before execution
    #[arg(long)]
    pub network_snapshot: Option<PathBuf>,
}

#[derive(Parser)]
pub struct ProfileArgs {
    /// Path to the contract WASM file
    #[arg(short, long)]
    pub contract: PathBuf,

    /// Deprecated: use --contract instead
    #[arg(long, hide = true, alias = "wasm", alias = "contract-path")]
    pub wasm: Option<PathBuf>,

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

    /// Expected SHA-256 hash of the WASM file. If provided, loading will fail if the computed hash does not match.
    #[arg(long)]
    pub expected_hash: Option<String>,

    /// Export flame graph as SVG file
    #[arg(long)]
    pub flamegraph: Option<PathBuf>,

    /// Export collapsed stack format (intermediate format for flame graph)
    #[arg(long)]
    pub flamegraph_stacks: Option<PathBuf>,

    /// Width for flame graph SVG rendering in pixels
    #[arg(long, default_value = "1200")]
    pub flamegraph_width: usize,

    /// Height for flame graph SVG rendering in pixels
    #[arg(long, default_value = "800")]
    pub flamegraph_height: usize,
}

#[derive(Parser)]
pub struct SymbolicArgs {
    /// Path to the contract WASM file
    #[arg(short, long)]
    pub contract: PathBuf,

    /// Function name to execute
    #[arg(short, long)]
    pub function: String,

    /// Output file for the scenario TOML
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Preset symbolic exploration budget profile
    #[arg(long, value_enum, default_value_t = SymbolicProfile::Balanced)]
    pub profile: SymbolicProfile,

    /// Maximum number of input combinations to generate deterministically
    #[arg(long, value_name = "N")]
    pub input_combination_cap: Option<usize>,

    /// Maximum number of generated inputs to execute
    #[arg(long, value_name = "N")]
    pub path_cap: Option<usize>,

    /// Overall symbolic analysis timeout in seconds
    #[arg(long, value_name = "SECONDS")]
    pub timeout: Option<u64>,

    /// Seed the exploration order with this integer so the run is fully
    /// reproducible.  The emitted "Replay token" value can be passed here
    /// or to `--replay` on any subsequent run to reproduce the exact same
    /// path ordering.  Mutually exclusive with `--replay`.
    #[arg(long, value_name = "N", conflicts_with = "replay")]
    pub seed: Option<u64>,

    /// Replay a previous symbolic run by providing its replay token (the seed
    /// value printed at the end of the original run).  Equivalent to
    /// `--seed <TOKEN>`.  Mutually exclusive with `--seed`.
    #[arg(long, value_name = "TOKEN", conflicts_with = "seed")]
    pub replay: Option<u64>,
}

#[derive(Parser)]
pub struct ReplayArgs {
    /// Path to the trace JSON file to replay
    #[arg(value_name = "TRACE_FILE")]
    pub trace_file: PathBuf,

    /// Path to the contract WASM file (optional, defaults to trace file's contract path)
    #[arg(short, long)]
    pub contract: Option<PathBuf>,

    /// Stop replay at step N (0-based index into call sequence)
    #[arg(long)]
    pub replay_until: Option<usize>,

    /// Output file for the diff report (default: stdout)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Show verbose output during replay
    #[arg(short, long)]
    pub verbose: bool,

    /// Number of context lines to show for divergence (default: 3)
    #[arg(short = 'C', long, default_value_t = 3)]
    pub context: usize,
}

#[derive(Parser)]
pub struct ServerArgs {
    /// Port to listen on
    #[arg(short, long, default_value = "9229")]
    pub port: u16,

    /// Authentication token (optional, if not provided no auth required)
    #[arg(short, long)]
    pub token: Option<String>,

    /// TLS certificate file path (optional)
    #[arg(long)]
    pub tls_cert: Option<PathBuf>,

    /// TLS private key file path (optional)
    #[arg(long)]
    pub tls_key: Option<PathBuf>,
}

#[derive(Parser)]
pub struct RemoteArgs {
    /// Remote server address (e.g., localhost:9229)
    #[arg(short, long)]
    pub remote: String,

    /// Authentication token (if required by server)
    #[arg(short, long)]
    pub token: Option<String>,

    /// Path to the contract WASM file
    #[arg(short, long)]
    pub contract: Option<PathBuf>,

    /// Function name to execute
    #[arg(short, long)]
    pub function: Option<String>,

    /// Function arguments as JSON array
    #[arg(short, long)]
    pub args: Option<String>,

    /// Default request timeout in milliseconds (applies when no per-request override is set)
    #[arg(long, default_value = "30000")]
    pub timeout_ms: u64,

    /// Ping request timeout in milliseconds
    #[arg(long, default_value = "2000")]
    pub ping_timeout_ms: u64,

    /// Inspect request timeout in milliseconds
    #[arg(long, default_value = "5000")]
    pub inspect_timeout_ms: u64,

    /// GetStorage request timeout in milliseconds
    #[arg(long, default_value = "10000")]
    pub storage_timeout_ms: u64,

    /// Retry attempts for idempotent operations (Ping/Inspect/GetStorage/etc.)
    #[arg(long, default_value = "3")]
    pub retry_attempts: u32,

    /// Base backoff delay in milliseconds for retries
    #[arg(long, default_value = "200")]
    pub retry_base_delay_ms: u64,

    /// Maximum backoff delay in milliseconds for retries
    #[arg(long, default_value = "2000")]
    pub retry_max_delay_ms: u64,
}

#[derive(Parser)]
pub struct AnalyzeArgs {
    /// Path to the contract WASM file
    #[arg(short, long)]
    pub contract: PathBuf,

    /// Function name to execute for dynamic analysis (optional)
    #[arg(short, long)]
    pub function: Option<String>,

    /// Function arguments as JSON array for dynamic analysis (optional)
    #[arg(short, long)]
    pub args: Option<String>,

    /// Initial storage state as JSON object (optional)
    #[arg(short, long)]
    pub storage: Option<String>,

    /// Execution timeout in seconds for dynamic analysis (default: 30)
    #[arg(long, default_value = "30")]
    pub timeout: u64,

    /// Output format (text, json)
    #[arg(long, default_value = "text")]
    pub format: String,

    /// Filter rules to enable (can be specified multiple times)
    #[arg(long, value_name = "RULE_ID")]
    pub enable_rule: Vec<String>,

    /// Filter rules to disable (can be specified multiple times)
    #[arg(long, value_name = "RULE_ID")]
    pub disable_rule: Vec<String>,

    /// Minimum finding severity to report (low, medium, high)
    #[arg(long, default_value = "low")]
    pub min_severity: String,
}

#[derive(Parser)]
pub struct ScenarioArgs {
    /// Path to the scenario TOML file
    #[arg(long)]
    pub scenario: PathBuf,

    /// Path to the contract WASM file
    #[arg(short, long)]
    pub contract: PathBuf,

    /// Initial storage state as JSON object
    #[arg(long)]
    pub storage: Option<String>,
}

/// Arguments for the `history prune` subcommand.
#[derive(Parser, Debug)]
#[command(about = "Prune or compact run history according to a retention policy")]
pub struct HistoryPruneArgs {
    /// Keep only the N most-recent history records; oldest are removed first.
    ///
    /// Takes precedence over `--history-max-records` (the global flag) when
    /// both are supplied, so you can override the default policy for a single
    /// explicit prune operation.
    #[arg(long, value_name = "N")]
    pub max_records: Option<usize>,

    /// Remove records older than N days.
    ///
    /// Takes precedence over `--history-max-age-days` (the global flag).
    #[arg(long, value_name = "DAYS")]
    pub max_age_days: Option<u64>,

    /// Show what would be removed without writing any changes to disk.
    #[arg(long)]
    pub dry_run: bool,
}
