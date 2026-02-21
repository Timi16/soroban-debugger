#![cfg(any())]
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
