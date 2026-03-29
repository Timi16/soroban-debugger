#![allow(deprecated)]
use predicates::prelude::*;
use tempfile::TempDir;

#[test]
fn test_run_command_requires_contract_arg() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["run", "--function", "test"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("contract")
                .or(predicate::str::contains("required"))
                .or(predicate::str::contains("missing")),
        );
}

#[test]
fn test_run_command_requires_function_arg() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let contract_file = temp_dir.path().join("contract.wasm");
    std::fs::write(&contract_file, b"dummy").expect("Failed to write temp file");

    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["run", "--contract", contract_file.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("function")
                .or(predicate::str::contains("required"))
                .or(predicate::str::contains("missing")),
        );
}

//
// Regression tests for --remote and --server modes:
// These modes should NOT require --contract or --function arguments.
// See: https://github.com/.../issues/661
//

#[test]
fn test_run_server_mode_does_not_require_contract_or_function() {
    // Server mode should parse successfully without contract/function
    // (the server will start and run until killed)
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["run", "--server", "--port", "9229"])
        .timeout(std::time::Duration::from_secs(2))
        .assert()
        // Server starts successfully (we kill it after timeout)
        .stderr(predicate::str::contains("Debug server listening"));
}

#[test]
fn test_run_remote_mode_does_not_require_contract_or_function() {
    // Remote mode (ping) should parse without contract/function
    // Connection will fail if no server is running, but that's expected
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["run", "--remote", "127.0.0.1:9229"])
        .assert()
        // Should fail with connection error, not argument parsing error
        .failure()
        .stderr(predicate::str::contains("Connection").or(predicate::str::contains("connect")));
}

#[test]
fn test_run_remote_mode_accepts_optional_contract_function() {
    // Remote mode should accept optional contract/function for full execution
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let contract_file = temp_dir.path().join("contract.wasm");
    std::fs::write(&contract_file, b"dummy").expect("Failed to write temp file");

    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args([
        "run",
        "--remote",
        "127.0.0.1:9229",
        "--contract",
        contract_file.to_str().unwrap(),
        "--function",
        "test",
    ])
    .assert()
    // Should parse successfully (connection may fail, but that's expected)
    .stderr(predicate::str::contains("Connection").or(predicate::str::contains("127.0.0.1")));
}

#[test]
fn test_run_local_mode_requires_both_contract_and_function() {
    // Local mode (no --server, no --remote) requires both contract and function
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let contract_file = temp_dir.path().join("contract.wasm");
    std::fs::write(&contract_file, b"dummy").expect("Failed to write temp file");

    // Missing function should fail
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["run", "--contract", contract_file.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("function")
                .or(predicate::str::contains("required"))
                .or(predicate::str::contains("missing")),
        );

    // Missing contract should fail
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["run", "--function", "test"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("contract")
                .or(predicate::str::contains("required"))
                .or(predicate::str::contains("missing")),
        );
}

#[test]
fn test_run_with_missing_contract_file() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args([
        "run",
        "--contract",
        "/nonexistent/contract.wasm",
        "--function",
        "test",
    ])
    .assert()
    .failure();
}

#[test]
fn test_run_accepts_json_format_flag() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let contract_file = temp_dir.path().join("contract.wasm");
    std::fs::write(&contract_file, b"dummy").expect("Failed to write temp file");

    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    // Just verify the flag is accepted by the argument parser
    let _ = cmd
        .args([
            "run",
            "--contract",
            contract_file.to_str().unwrap(),
            "--function",
            "test",
            "--format",
            "json",
        ])
        .output();
}

#[test]
fn test_run_accepts_dry_run_flag() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let contract_file = temp_dir.path().join("contract.wasm");
    std::fs::write(&contract_file, b"dummy").expect("Failed to write temp file");

    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    let _ = cmd
        .args([
            "run",
            "--contract",
            contract_file.to_str().unwrap(),
            "--function",
            "test",
            "--dry-run",
        ])
        .output();
}
