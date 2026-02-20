//! Integration tests for the compare subcommand.
//!
//! These tests exercise the full pipeline: writing trace JSON files to
//! disk, loading them via `ExecutionTrace::from_file`, running the
//! comparison engine, and verifying the report output.

use soroban_debugger::compare::{CompareEngine, ExecutionTrace};
use std::io::Write;
use tempfile::NamedTempFile;

/// Helper: write JSON to a temp file and return its path.
fn write_trace(json: &str) -> NamedTempFile {
    let mut f = NamedTempFile::new().expect("failed to create temp file");
    f.write_all(json.as_bytes()).expect("write failed");
    f.flush().expect("flush failed");
    f
}

const TRACE_A_JSON: &str = r#"{
  "label": "baseline",
  "storage": {
    "balance:Alice": 900,
    "balance:Bob": 100,
    "total_supply": 1000
  },
  "budget": {
    "cpu_instructions": 45000,
    "memory_bytes": 15360
  },
  "return_value": "ok",
  "call_sequence": [
    { "function": "transfer", "depth": 0 },
    { "function": "get_balance", "args": "Alice", "depth": 1 },
    { "function": "set_balance", "args": "Alice, 900", "depth": 1 }
  ],
  "events": [
    { "topics": ["transfer"], "data": "Alice→Bob 100" }
  ]
}"#;

const TRACE_B_JSON: &str = r#"{
  "label": "with-fee",
  "storage": {
    "balance:Alice": 895,
    "balance:Bob": 100,
    "total_supply": 1000,
    "fee_pool": 5
  },
  "budget": {
    "cpu_instructions": 38000,
    "memory_bytes": 14000
  },
  "return_value": { "status": "ok", "fee": 5 },
  "call_sequence": [
    { "function": "transfer", "depth": 0 },
    { "function": "check_allowance", "args": "Alice", "depth": 1 },
    { "function": "get_balance", "args": "Alice", "depth": 1 },
    { "function": "set_balance", "args": "Alice, 895", "depth": 1 }
  ],
  "events": [
    { "topics": ["transfer"], "data": "Alice→Bob 100" },
    { "topics": ["fee"], "data": "5" }
  ]
}"#;

// ── File loading ─────────────────────────────────────────────────────

#[test]
fn test_load_trace_from_file() {
    let f = write_trace(TRACE_A_JSON);
    let trace = ExecutionTrace::from_file(f.path()).expect("load failed");
    assert_eq!(trace.label.as_deref(), Some("baseline"));
    assert_eq!(trace.storage.len(), 3);
    assert!(trace.budget.is_some());
}

#[test]
fn test_load_invalid_json_returns_error() {
    let f = write_trace("NOT JSON {}{{");
    let result = ExecutionTrace::from_file(f.path());
    assert!(result.is_err());
}

#[test]
fn test_load_nonexistent_file_returns_error() {
    let result = ExecutionTrace::from_file("/tmp/does_not_exist_trace_xyz.json");
    assert!(result.is_err());
}

// ── Full comparison pipeline ─────────────────────────────────────────

#[test]
fn test_compare_pipeline_storage_diff() {
    let fa = write_trace(TRACE_A_JSON);
    let fb = write_trace(TRACE_B_JSON);

    let a = ExecutionTrace::from_file(fa.path()).unwrap();
    let b = ExecutionTrace::from_file(fb.path()).unwrap();

    let report = CompareEngine::compare(&a, &b);

    // fee_pool is only in B
    assert!(report.storage_diff.only_in_b.contains_key("fee_pool"));
    // balance:Alice changed
    assert!(report.storage_diff.modified.contains_key("balance:Alice"));
    // balance:Bob and total_supply unchanged
    assert_eq!(report.storage_diff.unchanged_count, 2);
}

#[test]
fn test_compare_pipeline_budget_delta() {
    let fa = write_trace(TRACE_A_JSON);
    let fb = write_trace(TRACE_B_JSON);

    let a = ExecutionTrace::from_file(fa.path()).unwrap();
    let b = ExecutionTrace::from_file(fb.path()).unwrap();

    let report = CompareEngine::compare(&a, &b);

    assert_eq!(report.budget_diff.cpu_delta, Some(-7000));
    assert_eq!(report.budget_diff.memory_delta, Some(-1360));
}

#[test]
fn test_compare_pipeline_return_value_mismatch() {
    let fa = write_trace(TRACE_A_JSON);
    let fb = write_trace(TRACE_B_JSON);

    let a = ExecutionTrace::from_file(fa.path()).unwrap();
    let b = ExecutionTrace::from_file(fb.path()).unwrap();

    let report = CompareEngine::compare(&a, &b);
    assert!(!report.return_value_diff.equal);
}

#[test]
fn test_compare_pipeline_flow_diff() {
    let fa = write_trace(TRACE_A_JSON);
    let fb = write_trace(TRACE_B_JSON);

    let a = ExecutionTrace::from_file(fa.path()).unwrap();
    let b = ExecutionTrace::from_file(fb.path()).unwrap();

    let report = CompareEngine::compare(&a, &b);
    assert!(!report.flow_diff.identical);
}

#[test]
fn test_compare_pipeline_event_diff() {
    let fa = write_trace(TRACE_A_JSON);
    let fb = write_trace(TRACE_B_JSON);

    let a = ExecutionTrace::from_file(fa.path()).unwrap();
    let b = ExecutionTrace::from_file(fb.path()).unwrap();

    let report = CompareEngine::compare(&a, &b);
    assert!(!report.event_diff.identical);
}

#[test]
fn test_compare_identical_traces() {
    let fa = write_trace(TRACE_A_JSON);
    let fb = write_trace(TRACE_A_JSON);

    let a = ExecutionTrace::from_file(fa.path()).unwrap();
    let b = ExecutionTrace::from_file(fb.path()).unwrap();

    let report = CompareEngine::compare(&a, &b);

    assert!(report.storage_diff.only_in_a.is_empty());
    assert!(report.storage_diff.only_in_b.is_empty());
    assert!(report.storage_diff.modified.is_empty());
    assert!(report.return_value_diff.equal);
    assert!(report.flow_diff.identical);
    assert!(report.event_diff.identical);
}

#[test]
fn test_render_report_contains_sections() {
    let fa = write_trace(TRACE_A_JSON);
    let fb = write_trace(TRACE_B_JSON);

    let a = ExecutionTrace::from_file(fa.path()).unwrap();
    let b = ExecutionTrace::from_file(fb.path()).unwrap();

    let report = CompareEngine::compare(&a, &b);
    let text = CompareEngine::render_report(&report);

    assert!(text.contains("Storage Changes"));
    assert!(text.contains("Budget Usage"));
    assert!(text.contains("Return Values"));
    assert!(text.contains("Execution Flow"));
    assert!(text.contains("Events"));
    assert!(text.contains("baseline"));
    assert!(text.contains("with-fee"));
}

// ── Minimal / empty traces ──────────────────────────────────────────

#[test]
fn test_compare_empty_traces() {
    let empty = r#"{}"#;
    let fa = write_trace(empty);
    let fb = write_trace(empty);

    let a = ExecutionTrace::from_file(fa.path()).unwrap();
    let b = ExecutionTrace::from_file(fb.path()).unwrap();

    let report = CompareEngine::compare(&a, &b);

    assert!(report.storage_diff.only_in_a.is_empty());
    assert!(report.storage_diff.only_in_b.is_empty());
    assert!(report.return_value_diff.equal);
    assert!(report.flow_diff.identical);
    assert!(report.event_diff.identical);
    assert!(report.budget_diff.cpu_delta.is_none());
}
