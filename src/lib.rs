pub mod analyzer;
pub mod batch;
pub mod cli;
pub mod codegen;
pub mod compare;
pub mod config;
pub mod debugger;
pub mod history;
pub mod inspector;
pub mod logging;
pub mod profiler;
pub mod repeat;
pub mod runtime;
pub mod simulator;
pub mod ui;
pub mod utils;
use miette::Diagnostic;

pub use debugger::engine::DebuggerEngine;
pub use runtime::executor::ContractExecutor;

/// Result type alias for the debugger
pub type Result<T> = miette::Result<T>;

/// Error types for the debugger
#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum DebuggerError {
    #[error("Failed to load WASM file: {0}")]
    #[diagnostic(
        code(debugger::wasm_load_failed),
        help("Make sure the path is correct and the file is a valid Soroban WASM contract. Try rebuilding your contract with `cargo contract build`.")
    )]
    WasmLoadError(String),

    #[error("Failed to execute contract: {0}")]
    #[diagnostic(
        code(debugger::execution_failed),
        help("Check the contract logs for more details. If this is a panic, try running with RUST_BACKTRACE=1.")
    )]
    ExecutionError(String),

    #[error("Invalid function name: {0}")]
    #[diagnostic(
        code(debugger::invalid_function),
        help("Use `soroban-debug inspect --functions` to see the list of available functions in this contract.")
    )]
    InvalidFunction(String),

    #[error("Invalid arguments: {0}")]
    #[diagnostic(
        code(debugger::invalid_arguments),
        help("Ensure arguments are provided as a JSON array. Example: --args '[\"Alice\", 1000]'. Use the --help flag for details on argument types.")
    )]
    InvalidArguments(String),

    #[error("Breakpoint error: {0}")]
    #[diagnostic(
        code(debugger::breakpoint_error),
        help("Verify that the function name exists and the condition syntax is correct.")
    )]
    BreakpointError(String),

    #[error("Storage error: {0}")]
    #[diagnostic(
        code(debugger::storage_error),
        help("Ensure the storage snapshot file is valid JSON and accessible.")
    )]
    StorageError(String),

    #[error("File operation failed: {0}")]
    #[diagnostic(
        code(debugger::file_error),
        help("Check if you have necessary permissions and that the path exists.")
    )]
    FileError(String),
}
