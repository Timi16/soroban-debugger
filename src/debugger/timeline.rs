use crate::debugger::source_map::SourceLocation;
use crate::debugger::state::PauseReason;
use crate::inspector::budget::BudgetInfo;
use crate::inspector::stack::CallFrame;
use crate::inspector::storage::StorageDiff;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Function call metadata embedded in timeline snapshots
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCallSnapshot {
    /// Caller address or identifier
    pub caller: String,
    /// Callee function name
    pub callee: String,
    /// Function arguments as strings
    pub arguments: Vec<String>,
    /// Call depth in the call stack
    pub depth: usize,
    /// Function result if available
    pub result: Option<String>,
    /// Function error if any
    pub error: Option<String>,
}

/// A snapshot of the execution state at a specific point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSnapshot {
    /// Step number in the timeline
    pub step: usize,
    /// Instruction index in the current function
    pub instruction_index: usize,
    /// Function name
    pub function: String,
    /// Call stack at this point
    pub call_stack: Vec<CallFrame>,
    /// Contract storage snapshot
    pub storage: HashMap<String, String>,
    /// Budget usage at this point
    pub budget: BudgetInfo,
    /// Number of events emitted so far
    pub events_count: usize,
    /// Timestamp of the snapshot
    pub timestamp: u128,
    /// Function call metadata if captured at this step
    pub function_call: Option<FunctionCallSnapshot>,
    /// Pause reason classification for this snapshot when execution is paused
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<PauseReason>,
}

/// Manages the timeline of execution snapshots for time-travel debugging.
pub struct TimelineManager {
    /// History of snapshots
    history: Vec<ExecutionSnapshot>,
    /// Current position in history (index in the history vector)
    current_pos: usize,
    /// Maximum history size
    max_history: usize,
}

impl TimelineManager {
    /// Create a new timeline manager
    pub fn new(max_history: usize) -> Self {
        Self {
            history: Vec::new(),
            current_pos: 0,
            max_history,
        }
    }

    /// Add a new snapshot to the history.
    /// If we are currently in a "back-stepped" state, this will truncate
    /// the history after the current position before adding the new snapshot.
    pub fn push(&mut self, snapshot: ExecutionSnapshot) {
        if self.current_pos < self.history.len().saturating_sub(1) {
            self.history.truncate(self.current_pos + 1);
        }

        if self.history.len() >= self.max_history {
            self.history.remove(0);
        } else {
            self.current_pos = self.history.len();
        }

        self.history.push(snapshot);
        self.current_pos = self.history.len() - 1;
    }

    /// Step back in time
    pub fn step_back(&mut self) -> Option<&ExecutionSnapshot> {
        if self.current_pos > 0 {
            self.current_pos -= 1;
            Some(&self.history[self.current_pos])
        } else {
            None
        }
    }

    /// Step forward in time
    pub fn step_forward(&mut self) -> Option<&ExecutionSnapshot> {
        if self.current_pos < self.history.len().saturating_sub(1) {
            self.current_pos += 1;
            Some(&self.history[self.current_pos])
        } else {
            None
        }
    }

    /// Jump to a specific step
    pub fn goto(&mut self, step: usize) -> Option<&ExecutionSnapshot> {
        if let Some(pos) = self.history.iter().position(|s| s.step == step) {
            self.current_pos = pos;
            Some(&self.history[pos])
        } else {
            None
        }
    }

    /// Get the current snapshot
    pub fn current(&self) -> Option<&ExecutionSnapshot> {
        self.history.get(self.current_pos)
    }

    /// Get all snapshots (for visualization)
    pub fn get_history(&self) -> &[ExecutionSnapshot] {
        &self.history
    }

    /// Clear all history
    pub fn clear(&mut self) {
        self.history.clear();
        self.current_pos = 0;
    }

    /// Get current position index
    pub fn current_pos(&self) -> usize {
        self.current_pos
    }

    /// Get total number of snapshots
    pub fn len(&self) -> usize {
        self.history.len()
    }

    /// Check if history is empty
    pub fn is_empty(&self) -> bool {
        self.history.is_empty()
    }
}

// -----------------------------------------------------------------------------
// Timeline export (compact execution narrative)
// -----------------------------------------------------------------------------

/// Schema version for `TimelineExport` JSON artifacts.
pub const TIMELINE_EXPORT_SCHEMA_VERSION: u32 = 1;

/// A compact, shareable execution narrative that focuses on pause points and key deltas
/// instead of a full raw trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineExport {
    pub schema_version: u32,
    /// RFC3339 timestamp (UTC) for when the artifact was produced.
    pub created_at: String,
    pub run: TimelineRunInfo,
    pub pauses: Vec<TimelinePausePoint>,
    /// Best-effort call-stack summary at end-of-run.
    pub stack_summary: Vec<CallFrame>,
    pub deltas: TimelineDeltas,
    pub warnings: Vec<TimelineWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineRunInfo {
    pub contract_path: String,
    pub wasm_sha256: Option<String>,
    pub function: String,
    /// JSON string of args when available (e.g. `["a","b"]`), not parsed/normalized.
    pub args_json: Option<String>,
    /// Result string as returned by the runner (may be JSON-like, but stored verbatim).
    pub result: Option<String>,
    /// Error message when execution fails.
    pub error: Option<String>,
    pub budget: Option<BudgetInfo>,
    pub events_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelinePausePoint {
    /// Monotonic sequence number within this artifact.
    pub index: usize,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breakpoint_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_count: Option<u64>,
    pub location: Option<SourceLocation>,
    /// Call stack snapshot at the pause point (best-effort).
    pub call_stack: Vec<CallFrame>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineDeltas {
    pub storage: Option<TimelineStorageDelta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineStorageDelta {
    pub added: Vec<TimelineKeyValue>,
    pub modified: Vec<TimelineKeyDiff>,
    pub deleted: Vec<String>,
    pub triggered_alerts: Vec<String>,
    /// True when the delta lists were capped to avoid huge artifacts.
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineKeyValue {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineKeyDiff {
    pub key: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineWarning {
    pub kind: String,
    pub message: String,
}

impl TimelineStorageDelta {
    /// Convert a storage diff into a deterministic, size-capped delta summary.
    pub fn from_storage_diff(diff: &StorageDiff, cap: usize) -> Self {
        let mut added: Vec<TimelineKeyValue> = diff
            .added
            .iter()
            .map(|(k, v)| TimelineKeyValue {
                key: k.clone(),
                value: v.clone(),
            })
            .collect();
        added.sort_by(|a, b| a.key.cmp(&b.key));

        let mut modified: Vec<TimelineKeyDiff> = diff
            .modified
            .iter()
            .map(|(k, (before, after))| TimelineKeyDiff {
                key: k.clone(),
                before: before.clone(),
                after: after.clone(),
            })
            .collect();
        modified.sort_by(|a, b| a.key.cmp(&b.key));

        let mut deleted = diff.deleted.clone();
        deleted.sort();

        let mut triggered_alerts = diff.triggered_alerts.clone();
        triggered_alerts.sort();

        let mut truncated = false;
        if cap > 0 {
            if added.len() > cap {
                added.truncate(cap);
                truncated = true;
            }
            if modified.len() > cap {
                modified.truncate(cap);
                truncated = true;
            }
            if deleted.len() > cap {
                deleted.truncate(cap);
                truncated = true;
            }
            if triggered_alerts.len() > cap {
                triggered_alerts.truncate(cap);
                truncated = true;
            }
        }

        Self {
            added,
            modified,
            deleted,
            triggered_alerts,
            truncated,
        }
    }
}

/// Marks the set of storage keys mutated during a single contract execution,
/// so exports and inspectors can highlight state-changing steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageMutationMarker {
    pub mutated_keys: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_point_serializes_breakpoint_hit_count() {
        let pause = TimelinePausePoint {
            index: 0,
            reason: "breakpoint".to_string(),
            breakpoint_id: Some("transfer".to_string()),
            hit_count: Some(3),
            location: None,
            call_stack: Vec::new(),
        };

        let value = serde_json::to_value(pause).unwrap();

        assert_eq!(value["breakpoint_id"], "transfer");
        assert_eq!(value["hit_count"], 3);
    }
}
