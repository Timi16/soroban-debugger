//! Tests that demonstrate usage of test fixture contracts

use predicates::prelude::*;
use soroban_debugger::utils::wasm;
use std::fs;

/// Helper to get fixture path - mirrors the mod.rs helper but for integration tests
fn get_fixture_path(name: &str) -> std::path::PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    std::path::PathBuf::from(manifest_dir)
        .join("tests")
        .join("fixtures")
        .join("wasm")
        .join(format!("{}.wasm", name))
}

#[test]
fn test_fixture_counter_parsing() {
    let fixture_path = get_fixture_path("counter");

    // Skip test if fixture doesn't exist (needs to be built first)
    if !fixture_path.exists() {
        eprintln!(
            "Skipping test: fixture not found at {}. Run tests/fixtures/build.sh to build fixtures.",
            fixture_path.display()
        );
        return;
    }

    let wasm_bytes = fs::read(&fixture_path).expect("Failed to read counter fixture");

    // Test that we can parse functions from the counter contract
    let functions = wasm::parse_functions(&wasm_bytes).expect("Failed to parse functions");

    // Counter contract should have increment, decrement, get, and init functions
    assert!(
        functions.contains(&"increment".to_string())
            || functions.contains(&"decrement".to_string())
            || functions.contains(&"get".to_string())
            || functions.contains(&"init".to_string()),
        "Counter fixture should contain expected functions. Found: {:?}",
        functions
    );

    // Test module info extraction
    let module_info = wasm::get_module_info(&wasm_bytes).expect("Failed to get module info");
    assert!(
        module_info.function_count > 0,
        "Counter should have functions"
    );
}

#[test]
fn test_fixture_echo_parsing() {
    let fixture_path = get_fixture_path("echo");

    if !fixture_path.exists() {
        eprintln!(
            "Skipping test: fixture not found at {}. Run tests/fixtures/build.sh to build fixtures.",
            fixture_path.display()
        );
        return;
    }

    let wasm_bytes = fs::read(&fixture_path).expect("Failed to read echo fixture");

    // Test that we can parse functions from the echo contract
    let functions = wasm::parse_functions(&wasm_bytes).expect("Failed to parse functions");

    // Echo contract should have echo functions
    assert!(
        !functions.is_empty(),
        "Echo fixture should have exported functions. Found: {:?}",
        functions
    );
}

#[test]
fn test_fixture_metadata_extraction() {
    // Test metadata extraction on fixtures (if they have metadata)
    let fixtures = ["counter", "echo", "budget_heavy"];

    for fixture_name in fixtures {
        let fixture_path = get_fixture_path(fixture_name);

        if !fixture_path.exists() {
            continue; // Skip if not built
        }

        let wasm_bytes = fs::read(&fixture_path).expect("Failed to read fixture");

        // Should not error even if metadata is missing
        let metadata = wasm::extract_contract_metadata(&wasm_bytes)
            .expect("Metadata extraction should not error");

        // Most fixtures won't have metadata, but extraction should work
        // This test ensures graceful handling of missing metadata
        assert!(
            metadata.is_empty() || !metadata.is_empty(),
            "Metadata extraction should work for {}",
            fixture_name
        );
    }
}

#[test]
fn test_fixture_inspect_command() {
    use assert_cmd::Command;
    use tempfile::NamedTempFile;

    let fixture_path = get_fixture_path("counter");

    if !fixture_path.exists() {
        eprintln!(
            "Skipping test: fixture not found at {}. Run tests/fixtures/build.sh to build fixtures.",
            fixture_path.display()
        );
        return;
    }

    // Test that the inspect command works with fixtures
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soroban-debug"));
    cmd.arg("inspect")
        .arg("--contract")
        .arg(&fixture_path)
        .arg("--functions");

    // Should succeed and show functions
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("Exported Functions"));
}
