//! Tests for structured timeout and cancellation error variants.

use soroban_debugger::runtime::result::RuntimeError;

#[test]
fn test_timeout_error_display() {
    let err = RuntimeError::timeout(1500, 2000);
    let msg = format!("{}", err);
    assert!(msg.contains("1500"), "should contain elapsed time");
    assert!(msg.contains("2000"), "should contain limit time");
}

#[test]
fn test_cancelled_error_display() {
    let err = RuntimeError::cancelled("user pressed Ctrl+C");
    let msg = format!("{}", err);
    assert!(msg.contains("cancelled"), "should indicate cancellation");
    assert!(msg.contains("user pressed Ctrl+C"), "should contain reason");
}

#[test]
fn test_timeout_predicate() {
    let err = RuntimeError::timeout(100, 200);
    assert!(err.is_timeout());
    assert!(!err.is_cancelled());
}

#[test]
fn test_cancelled_predicate() {
    let err = RuntimeError::cancelled("test");
    assert!(err.is_cancelled());
    assert!(!err.is_timeout());
}

#[test]
fn test_timeout_fields() {
    let err = RuntimeError::timeout(500, 1000);
    match err {
        RuntimeError::Timeout {
            elapsed_ms,
            limit_ms,
        } => {
            assert_eq!(elapsed_ms, 500);
            assert_eq!(limit_ms, 1000);
        }
        _ => panic!("expected Timeout variant"),
    }
}

#[test]
fn test_cancelled_fields() {
    let err = RuntimeError::cancelled("shutdown requested");
    match err {
        RuntimeError::Cancelled { reason } => {
            assert_eq!(reason, "shutdown requested");
        }
        _ => panic!("expected Cancelled variant"),
    }
}
