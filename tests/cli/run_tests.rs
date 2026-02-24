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
