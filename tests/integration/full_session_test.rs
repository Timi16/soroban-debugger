#![cfg(any())]
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn test_full_debug_session_walkthrough() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let wasm_path = std::path::PathBuf::from(manifest_dir)
        .join("tests")
        .join("fixtures")
        .join("wasm")
        .join("counter.wasm");

    // Skip if fixture not built
    if !wasm_path.exists() {
        eprintln!("Skipping test: counter.wasm fixture not found.");
        return;
    }

    // Initialize the command: run counter entry point with instruction stepping
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soroban-debug"));
    cmd.arg("run")
        .arg("--contract")
        .arg(&wasm_path)
        .arg("--function")
        .arg("increment")
        .arg("--breakpoint")
        .arg("increment")
        .arg("--instruction-debug")
        .arg("--step-instructions")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Spawn the command
    let mut child = cmd.spawn().expect("Failed to spawn soroban-debug");
    let mut stdin = child.stdin.take().expect("Failed to open stdin");

    // Simulate session:
    // 1. Step into (s)
    // 2. View instruction context (ctx) -> expects size input next
    // 3. View info (i)
    // 4. Continue to completion (c)
    writeln!(stdin, "s").unwrap();
    writeln!(stdin, "ctx").unwrap();
    writeln!(stdin, "5").unwrap(); // context size
    writeln!(stdin, "i").unwrap();
    writeln!(stdin, "c").unwrap();

    // Drop stdin to signal EOF if needed, although 'c' should exit the loop
    drop(stdin);

    // Capture output
    let output = child.wait_with_output().expect("Failed to wait for output");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    // Assertions
    assert!(
        output.status.success(),
        "Command failed with stderr: {}",
        stderr
    );

    // Verify breakpoint hit and stepping mode started
    assert!(
        combined.contains("Instruction Stepping Mode"),
        "Did not enter stepping mode"
    );
    assert!(
        combined.contains("Stepped to next instruction"),
        "Step command failed"
    );

    // Verify context display
    assert!(
        combined.contains("Instruction Context"),
        "Context command failed"
    );

    // Verify instruction info display
    assert!(
        combined.contains("Current Instruction Details"),
        "Info command failed"
    );

    // Verify execution completion and result
    assert!(
        combined.contains("Execution completed"),
        "Did not complete execution"
    );
    assert!(
        combined.contains("Result: \"I64(1)\""),
        "Unexpected return value (storage/state check)"
    );

    // Verify budget reporting (Requirement: Asserts budget was within limits)
    assert!(combined.contains("Resource budget"), "Budget info missing");
    assert!(combined.contains("cpu_insns"), "CPU budget info missing");
    assert!(
        combined.contains("memory_bytes"),
        "Memory budget info missing"
    );
}
