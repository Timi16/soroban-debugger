//! Execution trace data structures for the compare subcommand.
//!
//! An `ExecutionTrace` captures the full execution record of a single
//! contract invocation so that two traces can be compared side-by-side
//! for regression testing.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Trace schema versions this build accepts when validating a replay input.
/// The current writer always stamps traces with [`crate::output::SCHEMA_VERSION`]
/// (see `to_replay_artifact_manifest`); legacy traces written before
/// versioning was added carry no `schema_version` field and are accepted via
/// the absent-version code path in [`ExecutionTrace::validate_for_replay`].
pub const SUPPORTED_TRACE_SCHEMA_MAJORS: &[&str] = &["1"];

/// Top-level execution trace that is serialized to / deserialized from JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionTrace {
    /// Trace file schema version (semver-like). Optional for backward
    /// compatibility with traces written before the field existed; new
    /// writers always populate it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<String>,

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

impl std::fmt::Display for CallEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let indent = "  ".repeat(self.depth as usize);
        if let Some(ref args) = self.args {
            write!(f, "{}{}({})", indent, self.function, args)
        } else {
            write!(f, "{}{}()", indent, self.function)
        }
    }
}

impl std::fmt::Display for EventEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let contract = self.contract_id.as_deref().unwrap_or("<unknown-contract>");
        let topics = self.topics.join(", ");
        let data = self.data.as_deref().unwrap_or("<no-data>");
        write!(f, "[{}] topics=[{}] data={}", contract, topics, data)
    }
}

impl ExecutionTrace {
    /// Load an execution trace from a JSON file.
    pub fn from_file<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        let path = path.as_ref();
        let contents = fs::read_to_string(path).map_err(|e| {
            crate::DebuggerError::FileError(format!("Failed to read trace file {:?}: {}", path, e))
        })?;
        let trace: ExecutionTrace = serde_json::from_str(&contents).map_err(|e| {
            crate::DebuggerError::FileError(format!(
                "Failed to parse trace file {:?} as JSON: {}. \
                 The file may be truncated, written by an incompatible version, \
                 or not actually a trace file.",
                path, e
            ))
        })?;
        Ok(trace)
    }

    /// Validate that this trace is well-formed enough to be replayed and that
    /// its declared schema version is one this build understands.
    ///
    /// `contract_override` reflects an out-of-band `--contract` flag: when
    /// supplied, the trace does not need to carry its own contract path.
    ///
    /// Returns an error early so replay fails fast on malformed input rather
    /// than blowing up partway through execution. See `tests/schemas/` for
    /// fixtures covering the failure modes.
    pub fn validate_for_replay(&self, contract_override: Option<&Path>) -> crate::Result<()> {
        // 1. Schema version compatibility — absent means legacy and is allowed.
        if let Some(version) = self.schema_version.as_deref() {
            let major = version.split('.').next().unwrap_or("");
            if major.is_empty() {
                return Err(crate::DebuggerError::FileError(format!(
                    "Trace declares an unparseable schema_version {:?}. \
                     Expected a semver-like string such as \"1.0.0\".",
                    version
                ))
                .into());
            }
            if !SUPPORTED_TRACE_SCHEMA_MAJORS.contains(&major) {
                return Err(crate::DebuggerError::FileError(format!(
                    "Trace schema version {} is not supported by this build. \
                     Supported major versions: {}. \
                     Suggestion: re-capture the trace with the current debugger version, \
                     or upgrade/downgrade the debugger to a compatible release.",
                    version,
                    SUPPORTED_TRACE_SCHEMA_MAJORS.join(", ")
                ))
                .into());
            }
        }

        // 2. Required fields for a replay to even start.
        let function = self.function.as_deref().unwrap_or("").trim();
        if function.is_empty() {
            return Err(crate::DebuggerError::FileError(
                "Trace is missing the required `function` field — \
                 cannot replay without knowing which entry point to call. \
                 Suggestion: re-capture the trace; this usually means the original \
                 run was aborted before the function was recorded."
                    .to_string(),
            )
            .into());
        }

        // Contract may come from the trace itself OR a CLI override.
        let has_trace_contract = self
            .contract
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        if !has_trace_contract && contract_override.is_none() {
            return Err(crate::DebuggerError::FileError(
                "Trace does not record a `contract` path and no --contract was supplied. \
                 Suggestion: pass --contract <path/to/contract.wasm> on the command line."
                    .to_string(),
            )
            .into());
        }

        Ok(())
    }

    /// Serialize this trace to a pretty-printed JSON string.
    pub fn to_json(&self) -> crate::Result<String> {
        Ok(serde_json::to_string_pretty(self).map_err(|e| {
            crate::DebuggerError::FileError(format!("Failed to serialize trace: {}", e))
        })?)
    }

    pub fn manifest_path_for_trace(trace_path: &Path) -> PathBuf {
        trace_path.with_extension("manifest.json")
    }

    pub fn to_replay_artifact_manifest(
        &self,
        trace_path: &Path,
    ) -> crate::output::ReplayArtifactManifest {
        crate::output::ReplayArtifactManifest {
            schema_version: crate::output::SCHEMA_VERSION.to_string(),
            artifact_group: "replay_artifacts".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            label: self.label.clone(),
            contract: self.contract.clone(),
            function: self.function.clone(),
            files: vec![crate::output::ReplayArtifactFile {
                kind: crate::output::ReplayArtifactKind::Trace,
                path: trace_path.display().to_string(),
                description: Some("Primary execution trace used for replay".to_string()),
                compression: None,
            }],
        }
    }
}

#[cfg(test)]
mod validate_for_replay_tests {
    use super::*;
    use std::path::PathBuf;

    fn minimal_valid() -> ExecutionTrace {
        ExecutionTrace {
            schema_version: Some(crate::output::SCHEMA_VERSION.to_string()),
            label: None,
            contract: Some("contract.wasm".to_string()),
            function: Some("transfer".to_string()),
            args: None,
            storage: BTreeMap::new(),
            budget: None,
            return_value: None,
            call_sequence: Vec::new(),
            events: Vec::new(),
        }
    }

    #[test]
    fn current_version_trace_passes() {
        let trace = minimal_valid();
        assert!(trace.validate_for_replay(None).is_ok());
    }

    #[test]
    fn legacy_trace_without_schema_version_passes() {
        let mut trace = minimal_valid();
        trace.schema_version = None;
        assert!(trace.validate_for_replay(None).is_ok());
    }

    #[test]
    fn unsupported_major_is_rejected_with_actionable_message() {
        let mut trace = minimal_valid();
        trace.schema_version = Some("2.0.0".to_string());
        let err = trace.validate_for_replay(None).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("2.0.0"),
            "error should name the bad version: {msg}"
        );
        assert!(
            msg.contains("re-capture") || msg.contains("upgrade"),
            "error should suggest a remediation: {msg}"
        );
    }

    #[test]
    fn unparseable_schema_version_is_rejected() {
        let mut trace = minimal_valid();
        trace.schema_version = Some(String::new());
        let err = trace.validate_for_replay(None).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("unparseable"), "got: {msg}");
    }

    #[test]
    fn missing_function_is_rejected() {
        let mut trace = minimal_valid();
        trace.function = None;
        let err = trace.validate_for_replay(None).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("`function`"), "got: {msg}");
        assert!(msg.contains("re-capture"));
    }

    #[test]
    fn whitespace_function_is_rejected() {
        let mut trace = minimal_valid();
        trace.function = Some("   ".to_string());
        assert!(trace.validate_for_replay(None).is_err());
    }

    #[test]
    fn missing_contract_with_no_override_is_rejected() {
        let mut trace = minimal_valid();
        trace.contract = None;
        let err = trace.validate_for_replay(None).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("--contract"), "got: {msg}");
    }

    #[test]
    fn missing_contract_is_accepted_when_override_provided() {
        let mut trace = minimal_valid();
        trace.contract = None;
        let override_path = PathBuf::from("override.wasm");
        assert!(trace.validate_for_replay(Some(&override_path)).is_ok());
    }
}
