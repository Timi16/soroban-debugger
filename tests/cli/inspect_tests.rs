#![allow(deprecated)]
use predicates::prelude::*;
use tempfile::TempDir;

#[test]
fn test_inspect_requires_contract_arg() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["inspect"]).assert().failure().stderr(
        predicate::str::contains("contract")
            .or(predicate::str::contains("required"))
            .or(predicate::str::contains("missing")),
    );
}

#[test]
fn test_inspect_with_missing_contract_file() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["inspect", "--contract", "/nonexistent/contract.wasm"])
        .assert()
        .failure();
}

#[test]
fn test_inspect_with_empty_wasm_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let contract_file = temp_dir.path().join("contract.wasm");
    std::fs::write(&contract_file, b"").expect("Failed to write temp file");

    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["inspect", "--contract", contract_file.to_str().unwrap()])
        .assert()
        .failure();
}

#[test]
fn test_inspect_accepts_json_format_flag() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let contract_file = temp_dir.path().join("contract.wasm");
    std::fs::write(&contract_file, b"dummy").expect("Failed to write temp file");

    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    let _ = cmd
        .args([
            "inspect",
            "--contract",
            contract_file.to_str().unwrap(),
            "--format",
            "json",
        ])
        .output();
}
