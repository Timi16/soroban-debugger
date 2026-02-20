pub mod analyzer;
pub mod cli;
pub mod compare;
pub mod config;
pub mod debugger;
pub mod inspector;
pub mod logging;
pub mod profiler;
pub mod repeat;
pub mod runtime;
pub mod simulator;
pub mod ui;
pub mod utils;

pub use debugger::engine::DebuggerEngine;
pub use runtime::executor::ContractExecutor;

/// Result type alias for the debugger
pub type Result<T> = anyhow::Result<T>;

/// Error types for the debugger
#[derive(Debug, thiserror::Error)]
pub enum DebuggerError {
    #[error("Failed to load WASM file: {0}")]
    WasmLoadError(String),

    #[error("Failed to execute contract: {0}")]
    ExecutionError(String),

    #[error("Invalid function name: {0}")]
    InvalidFunction(String),

    #[error("Invalid arguments: {0}")]
    InvalidArguments(String),

    #[error("Breakpoint error: {0}")]
    BreakpointError(String),

    #[error("Storage error: {0}")]
    StorageError(String),
}
