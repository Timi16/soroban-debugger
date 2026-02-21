//! Test fixture utilities for loading pre-compiled WASM contracts
//!
//! This module provides helpers to load test fixture WASM files for use in tests.

use std::path::PathBuf;

/// Get the path to a test fixture WASM file
///
/// # Arguments
/// * `name` - Name of the fixture contract (without .wasm extension)
///
/// # Returns
/// PathBuf pointing to the fixture WASM file
///
/// # Panics
/// Panics if the fixture file doesn't exist. Use `fixture_exists()` to check first.
pub fn get_fixture_path(name: &str) -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = PathBuf::from(manifest_dir)
        .join("tests")
        .join("fixtures")
        .join("wasm")
        .join(format!("{}.wasm", name));
    
    if !path.exists() {
        panic!(
            "Fixture '{}' not found at {}. Run tests/fixtures/build.sh to build fixtures.",
            name,
            path.display()
        );
    }
    
    path
}

/// Check if a fixture WASM file exists
///
/// # Arguments
/// * `name` - Name of the fixture contract (without .wasm extension)
///
/// # Returns
/// `true` if the fixture exists, `false` otherwise
pub fn fixture_exists(name: &str) -> bool {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .join("tests")
        .join("fixtures")
        .join("wasm")
        .join(format!("{}.wasm", name))
        .exists()
}

/// Load a test fixture WASM file as bytes
///
/// # Arguments
/// * `name` - Name of the fixture contract (without .wasm extension)
///
/// # Returns
/// Vec<u8> containing the WASM bytes
///
/// # Panics
/// Panics if the fixture file doesn't exist or can't be read
pub fn load_fixture(name: &str) -> Vec<u8> {
    let path = get_fixture_path(name);
    std::fs::read(&path).unwrap_or_else(|e| {
        panic!("Failed to read fixture '{}' from {}: {}", name, path.display(), e);
    })
}

/// Available fixture contracts
pub mod names {
    pub const COUNTER: &str = "counter";
    pub const ECHO: &str = "echo";
    pub const PANIC: &str = "panic";
    pub const BUDGET_HEAVY: &str = "budget_heavy";
    pub const CROSS_CONTRACT: &str = "cross_contract";
}
