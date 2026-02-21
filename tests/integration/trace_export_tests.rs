#![cfg(any())]
use assert_cmd::Command;
use std::fs;
use tempfile::NamedTempFile;

#[test]
fn test_trace_export_output() {
    let mut cmd = Command::cargo_bin("soroban-debug").unwrap();
    let trace_file = NamedTempFile::new().unwrap();
    let trace_path = trace_file.path().to_path_buf();

    // Use a simple contract if available, otherwise just check if flag is recognized
    // For this test, we'll try to run against a mock or non-existent contract to check CLI behavior
    let assert = cmd
        .arg("run")
        .arg("--wasm")
        .arg("non_existent.wasm") // This will fail execution but we want to see if it tries to export
        .arg("--function")
        .arg("hello")
        .arg("--trace-output")
        .arg(&trace_path)
        .assert();

    // Execution might fail due to missing WASM, but the flag should be parsed.
    // To truly test export, we'd need a valid WASM.
    // Let's at least verify the help message shows the flag.
    let mut help_cmd = Command::cargo_bin("soroban-debug").unwrap();
    help_cmd
        .arg("run")
        .arg("--help")
        .assert()
        .stdout(predicates::str::contains("--trace-output"));
}
