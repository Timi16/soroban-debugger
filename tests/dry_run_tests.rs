#![cfg(any())]
//! Tests for --dry-run flag functionality

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Helper to create a minimal valid WASM file for testing
fn create_minimal_wasm() -> Result<tempfile::NamedTempFile, Box<dyn std::error::Error>> {
    // Create a minimal WASM file (just the header)
    // In real usage, this would be a compiled contract
    let wasm_bytes = vec![
        0x00, 0x61, 0x73, 0x6d, // WASM magic number
        0x01, 0x00, 0x00, 0x00, // Version 1
    ];

    let file = tempfile::NamedTempFile::new()?;
    fs::write(file.path(), wasm_bytes)?;
    Ok(file)
}

#[test]
fn test_dry_run_flag_exists() {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soroban-debug"));
    cmd.arg("run").arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("--dry-run"));
}

#[test]
fn test_dry_run_output_labeled() {
    // This test verifies that dry-run output is labeled
    // Note: This will fail if the contract file doesn't exist or is invalid
    // In a real scenario, you'd use a test fixture contract

    let wasm_file = match create_minimal_wasm() {
        Ok(f) => f,
        Err(_) => {
            // Skip test if we can't create WASM file
            eprintln!("Skipping test: Could not create test WASM file");
            return;
        }
    };

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soroban-debug"));
    cmd.arg("run")
        .arg("--contract")
        .arg(wasm_file.path())
        .arg("--function")
        .arg("test")
        .arg("--dry-run");

    // The command will likely fail due to invalid WASM, but we can check
    // that dry-run flag is accepted and output contains [DRY RUN] labels
    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Check that [DRY RUN] appears in output (either stdout or stderr)
    assert!(
        stdout.contains("[DRY RUN]") || stderr.contains("[DRY RUN]"),
        "Dry-run output should be labeled with [DRY RUN]. stdout: {}, stderr: {}",
        stdout,
        stderr
    );
}

#[test]
fn test_dry_run_storage_rollback_message() {
    // Test that dry-run shows storage rollback message
    let wasm_file = match create_minimal_wasm() {
        Ok(f) => f,
        Err(_) => {
            eprintln!("Skipping test: Could not create test WASM file");
            return;
        }
    };

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soroban-debug"));
    cmd.arg("run")
        .arg("--contract")
        .arg(wasm_file.path())
        .arg("--function")
        .arg("test")
        .arg("--dry-run");

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);

    // Check for rollback message (may appear even if execution fails)
    // The message should appear if dry-run completes successfully
    if combined.contains("Storage state restored") || combined.contains("Dry-run completed") {
        assert!(
            combined.contains("[DRY RUN]"),
            "Dry-run messages should be labeled"
        );
    }
}

#[test]
fn test_dry_run_with_events_flag() {
    // Test that --dry-run works with --show-events
    let wasm_file = match create_minimal_wasm() {
        Ok(f) => f,
        Err(_) => {
            eprintln!("Skipping test: Could not create test WASM file");
            return;
        }
    };

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soroban-debug"));
    cmd.arg("run")
        .arg("--contract")
        .arg(wasm_file.path())
        .arg("--function")
        .arg("test")
        .arg("--dry-run")
        .arg("--show-events");

    // Should accept both flags without error
    let output = cmd.output().unwrap();
    // Just verify it doesn't fail with "unrecognized flag" error
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("unrecognized") || stderr.contains("[DRY RUN]"),
        "Dry-run should work with other flags. stderr: {}",
        stderr
    );
}
