//! Execution trace data structures for the compare subcommand.
//!
//! An `ExecutionTrace` captures the full execution record of a single
//! contract invocation so that two traces can be compared side-by-side
//! for regression testing.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Top-level execution trace that is serialized to / deserialized from JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionTrace {
    /// Human-readable label for this trace (e.g. "v1.0 transfer test")
    #[serde(default)]
    pub label: Option<String>,

    /// Contract identifier (WASM path or contract ID)
    #[serde(default)]
    pub contract: Option<String>,

    /// Function that was invoked
    #[serde(default)]
    pub function: Option<String>,

    /// Arguments passed to the function
    #[serde(default)]
    pub args: Option<String>,

    /// Storage state after execution (key → value).
    /// Uses BTreeMap for deterministic ordering.
    #[serde(default)]
    pub storage: BTreeMap<String, serde_json::Value>,

    /// Resource budget consumed during execution
    #[serde(default)]
    pub budget: Option<BudgetTrace>,

    /// Return value of the invocation (serialized as JSON value)
    #[serde(default)]
    pub return_value: Option<serde_json::Value>,

    /// Ordered sequence of function calls observed during execution
    #[serde(default)]
    pub call_sequence: Vec<CallEntry>,

    /// Events emitted during execution
    #[serde(default)]
    pub events: Vec<EventEntry>,
}

/// Budget / resource usage captured in a trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetTrace {
    pub cpu_instructions: u64,
    pub memory_bytes: u64,
    #[serde(default)]
    pub cpu_limit: Option<u64>,
    #[serde(default)]
    pub memory_limit: Option<u64>,
}

/// A single entry in the call sequence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CallEntry {
    /// Name of the function that was called
    pub function: String,
    /// Optional arguments snapshot
    #[serde(default)]
    pub args: Option<String>,
    /// Nesting depth (0 = top-level)
    #[serde(default)]
    pub depth: u32,
}

/// A single event emitted during execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventEntry {
    #[serde(default)]
    pub contract_id: Option<String>,
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(default)]
    pub data: Option<String>,
}

impl ExecutionTrace {
    /// Load an execution trace from a JSON file.
    pub fn from_file<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        let path = path.as_ref();
        let contents = fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read trace file {:?}: {}", path, e))?;
        let trace: ExecutionTrace = serde_json::from_str(&contents)
            .map_err(|e| anyhow::anyhow!("Failed to parse trace file {:?}: {}", path, e))?;
        Ok(trace)
    }

    /// Serialize this trace to a pretty-printed JSON string.
    pub fn to_json(&self) -> crate::Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| anyhow::anyhow!("Failed to serialize trace: {}", e))
    }
}
