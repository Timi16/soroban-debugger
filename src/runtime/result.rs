//! Result types and formatting utilities for contract execution.
//!
//! This module defines the data structures that capture the outcome of a
//! contract function invocation, including execution traces, storage diffs,
//! and instruction-level profiling data.

use crate::inspector::budget::BudgetInfo;
use soroban_env_host::xdr::ScVal;
use soroban_env_host::{ConversionError, TryFromVal};
use soroban_sdk::{InvokeError, Val};
use std::collections::HashMap;

/// Re-export for convenience.
pub use crate::runtime::mocking::MockCallLogEntry as MockCallEntry;

/// Represents a captured execution trace.
#[derive(Debug, Clone)]
pub struct ExecutionRecord {
    pub function: String,
    pub args: Vec<ScVal>,
    pub result: std::result::Result<ScVal, String>,
    pub budget: BudgetInfo,
    pub storage_before: HashMap<String, String>,
    pub storage_after: HashMap<String, String>,
}

/// Storage snapshot for dry-run rollback.
#[derive(Clone)]
pub struct StorageSnapshot {
    pub storage: soroban_env_host::storage::Storage,
}

/// Structure to hold instruction counts per function.
#[derive(Debug, Clone, serde::Serialize)]
pub struct InstructionCounts {
    pub function_counts: Vec<(String, u64)>,
    pub total: u64,
}

/// Format the result of `env.try_invoke_contract::<Val, InvokeError>(...)`.
///
/// In soroban-sdk v22, `try_invoke_contract::<Val, InvokeError>` returns:
///   `Result<Result<Val, ConversionError>, Result<InvokeError, InvokeError>>`
///
/// - `Ok(Ok(val))`       â†’ contract returned a value successfully
/// - `Ok(Err(conv_err))` â†’ return value could not be converted to `Val`
/// - `Err(Ok(inv_err))`  â†’ contract returned an `InvokeError` (panic/abort)
/// - `Err(Err(inv_err))` â†’ `InvokeError` itself failed to convert
pub(super) fn format_invocation_result(
    invocation_result: &std::result::Result<
        std::result::Result<Val, ConversionError>,
        std::result::Result<InvokeError, InvokeError>,
    >,
    host: &soroban_env_host::Host,
    error_db: &crate::debugger::error_db::ErrorDatabase,
) -> (crate::Result<String>, std::result::Result<ScVal, String>) {
    use tracing::{info, warn};

    match invocation_result {
        Ok(Ok(val)) => {
            info!("Function executed successfully");
            match ScVal::try_from_val(host, val) {
                Ok(sc_val) => (Ok(format!("{:?}", val)), Ok(sc_val)),
                Err(e) => {
                    let msg = format!("Result conversion failed: {:?}", e);
                    (
                        Err(crate::DebuggerError::ExecutionError(msg.clone()).into()),
                        Err(msg),
                    )
                }
            }
        }
        Ok(Err(conv_err)) => {
            warn!("Return value conversion failed: {:?}", conv_err);
            let msg = format!("Return value conversion failed: {:?}", conv_err);
            (
                Err(crate::DebuggerError::ExecutionError(msg.clone()).into()),
                Err(msg),
            )
        }
        Err(Ok(inv_err)) => {
            let msg = match inv_err {
                InvokeError::Contract(code) => {
                    warn!("Contract returned error code: {}", code);
                    error_db.display_error(*code);
                    format!(
                        "The contract returned an error code: {}. This typically indicates \
                         a business logic failure (e.g. `panic!` or `require!`).",
                        code
                    )
                }
                InvokeError::Abort => {
                    warn!("Contract execution aborted");
                    "Contract execution was aborted. This could be due to a trap, \
                     budget exhaustion, or an explicit abort call."
                        .to_string()
                }
            };
            (
                Err(crate::DebuggerError::ExecutionError(msg.clone()).into()),
                Err(msg),
            )
        }
        Err(Err(inv_err)) => {
            warn!("Invocation error conversion failed: {:?}", inv_err);
            let msg = format!("Invocation failed with internal error: {:?}", inv_err);
            (
                Err(crate::DebuggerError::ExecutionError(msg.clone()).into()),
                Err(msg),
            )
        }
    }
}

#[derive(Debug, Clone)]
pub enum RuntimeError {
    Timeout { elapsed_ms: u64, limit_ms: u64 },
    Cancelled { reason: String },
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeError::Timeout {
                elapsed_ms,
                limit_ms,
            } => {
                write!(
                    f,
                    "Execution timed out after {}ms (limit: {}ms)",
                    elapsed_ms, limit_ms
                )
            }
            RuntimeError::Cancelled { reason } => {
                write!(f, "Execution cancelled: {}", reason)
            }
        }
    }
}

impl RuntimeError {
    /// Create a timeout error with elapsed and limit durations.
    pub fn timeout(elapsed_ms: u64, limit_ms: u64) -> Self {
        Self::Timeout {
            elapsed_ms,
            limit_ms,
        }
    }

    /// Create a cancellation error with a reason.
    pub fn cancelled(reason: impl Into<String>) -> Self {
        Self::Cancelled {
            reason: reason.into(),
        }
    }

    /// Returns true if this error is a timeout.
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Timeout { .. })
    }

    /// Returns true if this error is a cancellation.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled { .. })
    }
}
