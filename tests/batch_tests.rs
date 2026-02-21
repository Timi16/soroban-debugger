use soroban_debugger::batch::{BatchExecutor, BatchItem};
use std::fs;
use tempfile::TempDir;

/// Create a simple test contract that adds two numbers
#[allow(dead_code)]
fn create_test_contract() -> Vec<u8> {
    // This would be a real WASM contract in production
    // For now, we'll use a placeholder
    vec![0x00, 0x61, 0x73, 0x6d] // WASM magic number
}

#[test]
fn test_load_batch_file() {
    let temp_dir = TempDir::new().unwrap();
    let batch_file = temp_dir.path().join("batch.json");

    let content = r#"[
        {"args": "[1, 2]", "expected": "3", "label": "Test 1"},
        {"args": "[5, 10]", "label": "Test 2"}
    ]"#;

    fs::write(&batch_file, content).unwrap();

    let items = BatchExecutor::load_batch_file(&batch_file).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].args, "[1, 2]");
    assert_eq!(items[0].expected, Some("3".to_string()));
    assert_eq!(items[0].label, Some("Test 1".to_string()));
}

#[test]
fn test_batch_item_without_expected() {
    let json = r#"{"args": "[1, 2]", "label": "Test"}"#;
    let item: BatchItem = serde_json::from_str(json).unwrap();

    assert_eq!(item.args, "[1, 2]");
    assert_eq!(item.expected, None);
    assert_eq!(item.label, Some("Test".to_string()));
}

#[test]
fn test_batch_item_minimal() {
    let json = r#"{"args": "[1, 2]"}"#;
    let item: BatchItem = serde_json::from_str(json).unwrap();

    assert_eq!(item.args, "[1, 2]");
    assert_eq!(item.expected, None);
    assert_eq!(item.label, None);
}

#[test]
fn test_batch_summary_calculation() {
    use soroban_debugger::batch::BatchResult;

    let results = vec![
        BatchResult {
            index: 0,
            label: Some("Pass".to_string()),
            args: "[]".to_string(),
            result: "ok".to_string(),
            success: true,
            error: None,
            expected: Some("ok".to_string()),
            passed: true,
            duration_ms: 10,
        },
        BatchResult {
            index: 1,
            label: Some("Fail".to_string()),
            args: "[]".to_string(),
            result: "fail".to_string(),
            success: true,
            error: None,
            expected: Some("ok".to_string()),
            passed: false,
            duration_ms: 15,
        },
        BatchResult {
            index: 2,
            label: Some("Error".to_string()),
            args: "[]".to_string(),
            result: String::new(),
            success: false,
            error: Some("execution error".to_string()),
            expected: None,
            passed: false,
            duration_ms: 5,
        },
    ];

    let summary = BatchExecutor::summarize(&results);

    assert_eq!(summary.total, 3);
    assert_eq!(summary.passed, 1);
    assert_eq!(summary.failed, 1);
    assert_eq!(summary.errors, 1);
    assert_eq!(summary.total_duration_ms, 30);
}

#[test]
fn test_invalid_batch_file() {
    let temp_dir = TempDir::new().unwrap();
    let batch_file = temp_dir.path().join("invalid.json");

    fs::write(&batch_file, "not valid json").unwrap();

    let result = BatchExecutor::load_batch_file(&batch_file);
    assert!(result.is_err());
}

#[test]
fn test_batch_file_not_array() {
    let temp_dir = TempDir::new().unwrap();
    let batch_file = temp_dir.path().join("not_array.json");

    fs::write(&batch_file, r#"{"args": "[1, 2]"}"#).unwrap();

    let result = BatchExecutor::load_batch_file(&batch_file);
    assert!(result.is_err());
}
