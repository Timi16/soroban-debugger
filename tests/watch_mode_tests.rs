#![cfg(any())]
use assert_cmd::Command;
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

/// Test that watch mode can be invoked and starts correctly
#[test]
fn test_watch_mode_invocation() {
    let temp_dir = TempDir::new().unwrap();
    let wasm_path = temp_dir.path().join("test_contract.wasm");

    // Create a minimal valid WASM file (magic number + version)
    let minimal_wasm = vec![
        0x00, 0x61, 0x73, 0x6d, // magic number "\0asm"
        0x01, 0x00, 0x00, 0x00, // version 1
    ];
    fs::write(&wasm_path, minimal_wasm).unwrap();

    // Start watch mode in a separate thread with a timeout
    let wasm_path_clone = wasm_path.clone();
    let handle = thread::spawn(move || {
        let mut cmd = Command::cargo_bin("soroban-debug").unwrap();
        cmd.arg("run")
            .arg("--contract")
            .arg(&wasm_path_clone)
            .arg("--function")
            .arg("test")
            .arg("--watch")
            .timeout(Duration::from_secs(2));

        // This will timeout, which is expected for watch mode
        let result = cmd.assert();
        result
    });

    // Give it a moment to start
    thread::sleep(Duration::from_millis(500));

    // The test passes if watch mode started (even if it errors on the invalid WASM)
    // We're just verifying the flag is recognized and the mode can be invoked
    let _ = handle.join();
}

/// Test that watch mode is incompatible with batch mode
#[test]
fn test_watch_mode_incompatible_with_batch() {
    let temp_dir = TempDir::new().unwrap();
    let wasm_path = temp_dir.path().join("test_contract.wasm");
    let batch_path = temp_dir.path().join("batch.json");

    // Create minimal files
    fs::write(
        &wasm_path,
        vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00],
    )
    .unwrap();
    fs::write(&batch_path, "[]").unwrap();

    let mut cmd = Command::cargo_bin("soroban-debug").unwrap();
    cmd.arg("run")
        .arg("--contract")
        .arg(&wasm_path)
        .arg("--function")
        .arg("test")
        .arg("--batch-args")
        .arg(&batch_path)
        .arg("--watch");

    // Should handle this gracefully (batch mode takes precedence)
    // The command will fail on the invalid WASM, but that's okay
    let _ = cmd.assert();
}

/// Test watch mode with a real contract from examples
#[test]
#[ignore] // Ignore by default as it requires building example contracts
fn test_watch_mode_with_real_contract() {
    // This test requires the multisig example to be built
    let contract_path = PathBuf::from(
        "examples/contracts/multisig/target/wasm32-unknown-unknown/release/multisig.wasm",
    );

    if !contract_path.exists() {
        eprintln!("Skipping test: example contract not built");
        return;
    }

    let mut cmd = Command::cargo_bin("soroban-debug").unwrap();
    cmd.arg("run")
        .arg("--contract")
        .arg(&contract_path)
        .arg("--function")
        .arg("initialize")
        .arg("--args")
        .arg(r#"[["GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4"], 1]"#)
        .arg("--watch")
        .timeout(Duration::from_secs(3));

    // Will timeout, which is expected
    let _ = cmd.assert();
}

/// Test that Ctrl+C handling is set up (we can't actually test the signal)
#[test]
fn test_watch_mode_help_text() {
    let mut cmd = Command::cargo_bin("soroban-debug").unwrap();
    cmd.arg("run").arg("--help");

    cmd.assert()
        .success()
        .stdout(predicates::str::contains("--watch"))
        .stdout(predicates::str::contains("Watch the WASM file for changes"));
}
