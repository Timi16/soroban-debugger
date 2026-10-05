use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn fixture_wasm(name: &str) -> PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("wasm")
        .join(format!("{name}.wasm"))
}

#[test]
fn symbolic_execution_with_storage_seed() {
    let wasm_path = fixture_wasm("counter");
    if !wasm_path.exists() {
        eprintln!(
            "Skipping test: fixture not found at {}. Run tests/fixtures/build.sh to build fixtures.",
            wasm_path.display()
        );
        return;
    }

    // Create a temporary storage seed file
    let tmpdir = TempDir::new().unwrap();
    let storage_path = tmpdir.path().join("storage.json");
    fs::write(&storage_path, r#"{"c": 41}"#).unwrap();

    // Run symbolic execution with storage seed
    let wasm = fs::read(&wasm_path).unwrap();
    let analyzer = soroban_debugger::analyzer::symbolic::SymbolicAnalyzer::new();
    let config = soroban_debugger::analyzer::symbolic::SymbolicConfig {
        max_paths: 10,
        max_input_combinations: 36,
        timeout_secs: 30,
        max_breadth: 10,
        max_depth: 3,
        seed: None,
        storage_seed: Some(fs::read_to_string(&storage_path).unwrap()),
        storage_read_pressure_threshold: None,
    };

    let report = analyzer
        .analyze_with_config(&wasm, "get", &config)
        .expect("symbolic analysis with storage seed should complete");

    // Verify that the storage seed was used in the configuration
    assert_eq!(
        report.metadata.config.storage_seed,
        Some(r#"{"c": 41}"#.to_string())
    );

    // The report should show that paths were explored
    assert!(report.paths_explored > 0);
}

#[test]
fn symbolic_execution_without_storage_seed() {
    let wasm_path = fixture_wasm("counter");
    if !wasm_path.exists() {
        return;
    }

    let wasm = fs::read(&wasm_path).unwrap();
    let analyzer = soroban_debugger::analyzer::symbolic::SymbolicAnalyzer::new();
    let config = soroban_debugger::analyzer::symbolic::SymbolicConfig {
        max_paths: 5,
        max_input_combinations: 10,
        timeout_secs: 30,
        max_breadth: 5,
        max_depth: 2,
        seed: None,
        storage_seed: None,
        storage_read_pressure_threshold: None,
    };

    let report = analyzer
        .analyze_with_config(&wasm, "get", &config)
        .expect("symbolic analysis should complete");

    // Verify no storage seed was set
    assert_eq!(report.metadata.config.storage_seed, None);
    assert!(report.paths_explored > 0);
}
