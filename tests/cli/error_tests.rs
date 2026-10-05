#![allow(deprecated)]
/// Integration tests for CLI error handling and edge cases
///
/// Tests error conditions including missing files, invalid arguments,
/// malformed JSON, and other failure scenarios.
#[allow(unused_imports)]
use assert_cmd::prelude::*;
use predicates::prelude::*;
use tempfile::TempDir;

#[test]
fn test_run_with_missing_contract_file() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args([
        "run",
        "--contract",
        "/nonexistent/path/contract.wasm",
        "--function",
        "test",
    ])
    .assert()
    .failure()
    .stderr(
        predicate::str::contains("not found")
            .or(predicate::str::contains("No such file"))
            .or(predicate::str::contains("Failed to read")),
    );
}

#[test]
fn test_inspect_with_missing_contract_file() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["inspect", "--contract", "/nonexistent/contract.wasm"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("not found")
                .or(predicate::str::contains("No such file"))
                .or(predicate::str::contains("Failed to read")),
        );
}

#[test]
fn test_optimize_with_missing_contract_file() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args([
        "optimize",
        "--contract",
        "/nonexistent/contract.wasm",
        "--function",
        "test",
    ])
    .assert()
    .failure()
    .stderr(
        predicate::str::contains("not found")
            .or(predicate::str::contains("No such file"))
            .or(predicate::str::contains("Failed to read")),
    );
}

#[test]
fn test_profile_with_missing_contract_file() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args([
        "profile",
        "--contract",
        "/nonexistent/contract.wasm",
        "--function",
        "test",
    ])
    .assert()
    .failure()
    .stderr(
        predicate::str::contains("not found")
            .or(predicate::str::contains("No such file"))
            .or(predicate::str::contains("Failed to read")),
    );
}

#[test]
fn test_upgrade_check_with_missing_old_file() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let new_file = temp_dir.path().join("new.wasm");
    std::fs::write(&new_file, b"dummy").expect("Failed to write temp file");

    cmd.args([
        "upgrade-check",
        "--old",
        "/nonexistent/old.wasm",
        "--new",
        new_file.to_str().unwrap(),
    ])
    .assert()
    .failure()
    .stderr(
        predicate::str::contains("not found")
            .or(predicate::str::contains("No such file"))
            .or(predicate::str::contains("Failed to read")),
    );
}

#[test]
fn test_upgrade_check_with_missing_new_file() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let old_file = temp_dir.path().join("old.wasm");
    std::fs::write(&old_file, b"dummy").expect("Failed to write temp file");

    cmd.args([
        "upgrade-check",
        "--old",
        old_file.to_str().unwrap(),
        "--new",
        "/nonexistent/new.wasm",
    ])
    .assert()
    .failure()
    .stderr(
        predicate::str::contains("not found")
            .or(predicate::str::contains("No such file"))
            .or(predicate::str::contains("Failed to read")),
    );
}

#[test]
fn test_compare_with_missing_trace_a() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let trace_b = temp_dir.path().join("trace_b.json");
    std::fs::write(&trace_b, b"{}").expect("Failed to write temp file");

    cmd.args([
        "compare",
        "/nonexistent/trace_a.json",
        trace_b.to_str().unwrap(),
    ])
    .assert()
    .failure()
    .stderr(
        predicate::str::contains("not found")
            .or(predicate::str::contains("No such file"))
            .or(predicate::str::contains("Failed to read")),
    );
}

#[test]
fn test_compare_with_missing_trace_b() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let trace_a = temp_dir.path().join("trace_a.json");
    std::fs::write(&trace_a, b"{}").expect("Failed to write temp file");

    cmd.args([
        "compare",
        trace_a.to_str().unwrap(),
        "/nonexistent/trace_b.json",
    ])
    .assert()
    .failure()
    .stderr(
        predicate::str::contains("not found")
            .or(predicate::str::contains("No such file"))
            .or(predicate::str::contains("Failed to read")),
    );
}

#[test]
fn test_run_with_invalid_json_args() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let contract_file = temp_dir.path().join("contract.wasm");
    std::fs::write(&contract_file, b"dummy").expect("Failed to write temp file");

    cmd.args([
        "run",
        "--contract",
        contract_file.to_str().unwrap(),
        "--function",
        "test",
        "--args",
        "this is not valid json",
    ])
    .assert()
    .failure()
    .stderr(
        predicate::str::contains("json")
            .or(predicate::str::contains("invalid"))
            .or(predicate::str::contains("parse")),
    );
}

#[test]
fn test_run_with_invalid_json_storage() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let contract_file = temp_dir.path().join("contract.wasm");
    std::fs::write(&contract_file, b"dummy").expect("Failed to write temp file");

    cmd.args([
        "run",
        "--contract",
        contract_file.to_str().unwrap(),
        "--function",
        "test",
        "--storage",
        "invalid json storage",
    ])
    .assert()
    .failure()
    .stderr(
        predicate::str::contains("json")
            .or(predicate::str::contains("invalid"))
            .or(predicate::str::contains("parse")),
    );
}

#[test]
fn test_run_with_invalid_snapshot_file() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let contract_file = temp_dir.path().join("contract.wasm");
    std::fs::write(&contract_file, b"dummy").expect("Failed to write temp file");

    cmd.args([
        "run",
        "--contract",
        contract_file.to_str().unwrap(),
        "--function",
        "test",
        "--network-snapshot",
        "/nonexistent/snapshot.json",
    ])
    .assert()
    .failure()
    .stderr(
        predicate::str::contains("not found")
            .or(predicate::str::contains("No such file"))
            .or(predicate::str::contains("Failed to read")),
    );
}

#[test]
fn test_optimize_with_invalid_snapshot_file() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let contract_file = temp_dir.path().join("contract.wasm");
    std::fs::write(&contract_file, b"dummy").expect("Failed to write temp file");

    cmd.args([
        "optimize",
        "--contract",
        contract_file.to_str().unwrap(),
        "--function",
        "test",
        "--network-snapshot",
        "/nonexistent/snapshot.json",
    ])
    .assert()
    .failure()
    .stderr(
        predicate::str::contains("not found")
            .or(predicate::str::contains("No such file"))
            .or(predicate::str::contains("Failed to read")),
    );
}

#[test]
fn test_run_with_invalid_batch_file() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let contract_file = temp_dir.path().join("contract.wasm");
    std::fs::write(&contract_file, b"dummy").expect("Failed to write temp file");

    cmd.args([
        "run",
        "--contract",
        contract_file.to_str().unwrap(),
        "--function",
        "test",
        "--batch-args",
        "/nonexistent/batch.json",
    ])
    .assert()
    .failure()
    .stderr(
        predicate::str::contains("not found")
            .or(predicate::str::contains("No such file"))
            .or(predicate::str::contains("Failed to read")),
    );
}

#[test]
fn test_run_with_invalid_import_storage() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let contract_file = temp_dir.path().join("contract.wasm");
    std::fs::write(&contract_file, b"dummy").expect("Failed to write temp file");

    cmd.args([
        "run",
        "--contract",
        contract_file.to_str().unwrap(),
        "--function",
        "test",
        "--import-storage",
        "/nonexistent/storage.json",
    ])
    .assert()
    .failure()
    .stderr(
        predicate::str::contains("not found")
            .or(predicate::str::contains("No such file"))
            .or(predicate::str::contains("Failed to read")),
    );
}

#[test]
fn test_compare_with_invalid_json_trace() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let trace_a = temp_dir.path().join("trace_a.json");
    std::fs::write(&trace_a, b"invalid json content").expect("Failed to write temp file");

    let trace_b = temp_dir.path().join("trace_b.json");
    std::fs::write(&trace_b, b"{}").expect("Failed to write temp file");

    cmd.args([
        "compare",
        trace_a.to_str().unwrap(),
        trace_b.to_str().unwrap(),
    ])
    .assert()
    .failure()
    .stderr(
        predicate::str::contains("json")
            .or(predicate::str::contains("invalid"))
            .or(predicate::str::contains("parse")),
    );
}

#[test]
fn test_run_with_empty_contract_file() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let contract_file = temp_dir.path().join("contract.wasm");
    std::fs::write(&contract_file, b"").expect("Failed to write temp file");

    cmd.args([
        "run",
        "--contract",
        contract_file.to_str().unwrap(),
        "--function",
        "test",
    ])
    .assert()
    .failure();
    // Error could be about invalid WASM or other parsing issues
}
#[test]
fn test_inspect_with_empty_contract_file() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let contract_file = temp_dir.path().join("contract.wasm");
    std::fs::write(&contract_file, b"").expect("Failed to write temp file");

    cmd.args(["inspect", "--contract", contract_file.to_str().unwrap()])
        .assert()
        .failure();
    // Error could be about invalid WASM or other parsing issues
}

#[test]
fn test_json_error_wasm_load_failure() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    let output = cmd
        .args([
            "run",
            "--contract",
            "/nonexistent/path/contract.wasm",
            "--function",
            "test",
            "--output",
            "json",
        ])
        .output()
        .expect("Failed to execute command");

    // The exit status should still be failure
    assert!(!output.status.success());

    let stdout_str = String::from_utf8(output.stdout).expect("Stdout is not valid UTF-8");
    let json_val: serde_json::Value = serde_json::from_str(&stdout_str)
        .unwrap_or_else(|_| panic!("Failed to parse JSON output: {}", stdout_str));

    assert_eq!(json_val["schema_version"], "1.0.0");
    assert_eq!(json_val["command"], "run");
    assert_eq!(json_val["status"], "error");
    assert!(json_val["result"].is_null());

    let error_obj = json_val["error"]
        .as_object()
        .expect("error field should be an object");
    assert!(error_obj
        .get("message")
        .unwrap()
        .as_str()
        .unwrap()
        .contains("Failed to read WASM file"));
    assert_eq!(
        error_obj.get("code").unwrap().as_str().unwrap(),
        "debugger::wasm_load_failed"
    );
    assert_eq!(
        error_obj.get("category").unwrap().as_str().unwrap(),
        "contract_failure"
    );
    assert!(error_obj
        .get("suggestion")
        .unwrap()
        .as_str()
        .unwrap()
        .contains("Check the file path"));
}

#[test]
fn test_json_error_invalid_function() {
    let wasm_path = "tests/fixtures/wasm/counter.wasm";
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    let output = cmd
        .args([
            "run",
            "--contract",
            wasm_path,
            "--function",
            "nonexistent_function",
            "--output",
            "json",
        ])
        .output()
        .expect("Failed to execute command");

    // The exit status should still be failure
    assert!(!output.status.success());

    let stdout_str = String::from_utf8(output.stdout).expect("Stdout is not valid UTF-8");
    let json_val: serde_json::Value = serde_json::from_str(&stdout_str)
        .unwrap_or_else(|_| panic!("Failed to parse JSON output: {}", stdout_str));

    assert_eq!(json_val["schema_version"], "1.0.0");
    assert_eq!(json_val["command"], "run");
    assert_eq!(json_val["status"], "error");
    assert!(json_val["result"].is_null());

    let error_obj = json_val["error"]
        .as_object()
        .expect("error field should be an object");
    assert!(error_obj
        .get("message")
        .unwrap()
        .as_str()
        .unwrap()
        .contains("Invalid function name"));
    assert_eq!(
        error_obj.get("code").unwrap().as_str().unwrap(),
        "debugger::invalid_function"
    );
    assert_eq!(
        error_obj.get("category").unwrap().as_str().unwrap(),
        "parser_failure"
    );
    assert!(error_obj
        .get("suggestion")
        .unwrap()
        .as_str()
        .unwrap()
        .contains("Ensure the function name is spelled exactly"));
}
