//! End-to-end fixture coverage for `ExecutionTrace::validate_for_replay`.
//!
//! The unit tests in `src/compare/trace.rs` exercise the validator against
//! in-memory structs. These tests cover the on-disk path: deserialization +
//! validation against pinned fixtures under `tests/schemas/replay_traces/`,
//! which is the same code path the `replay` CLI command goes through.

use std::path::PathBuf;

use soroban_debugger::compare::ExecutionTrace;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("schemas")
        .join("replay_traces")
        .join(name)
}

#[test]
fn valid_current_schema_trace_loads_and_validates() {
    let trace =
        ExecutionTrace::from_file(fixture("valid_current.json")).expect("fixture must parse");
    assert_eq!(trace.schema_version.as_deref(), Some("1.0.0"));
    trace
        .validate_for_replay(None)
        .expect("current-schema fixture must validate");
}

#[test]
fn legacy_trace_without_schema_version_still_validates() {
    let trace = ExecutionTrace::from_file(fixture("valid_legacy_no_version.json"))
        .expect("legacy fixture must parse");
    assert!(trace.schema_version.is_none());
    trace
        .validate_for_replay(None)
        .expect("legacy trace must remain replayable");
}

#[test]
fn malformed_trace_missing_function_is_rejected_fast() {
    let trace = ExecutionTrace::from_file(fixture("malformed_missing_function.json"))
        .expect("fixture must parse JSON even if logically invalid");
    let err = trace
        .validate_for_replay(None)
        .expect_err("missing function must be rejected before replay starts");
    let msg = err.to_string();
    assert!(
        msg.contains("`function`"),
        "error should name the missing field, got: {msg}"
    );
}

#[test]
fn unsupported_schema_version_is_rejected_with_versioning_hint() {
    let trace = ExecutionTrace::from_file(fixture("unsupported_future_schema.json"))
        .expect("fixture must parse");
    let err = trace
        .validate_for_replay(None)
        .expect_err("future major schema version must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("2.0.0"),
        "error should name the version: {msg}"
    );
    assert!(
        msg.contains("Supported"),
        "error should mention supported versions: {msg}"
    );
}
