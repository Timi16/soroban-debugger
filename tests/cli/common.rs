#![cfg(any())]
use std::path::PathBuf;
use tempfile::TempDir;

/// Test utilities and common functions for CLI integration tests
///
/// This module provides helper functions for setting up test environments,
/// creating temporary files, and running CLI commands.
/// Helper struct for managing test resources
pub struct TestContext {
    /// Temporary directory for test files
    pub temp_dir: TempDir,
}

impl TestContext {
    /// Create a new test context with a temporary directory
    pub fn new() -> Result<Self, std::io::Error> {
        let temp_dir = TempDir::new()?;
        Ok(TestContext { temp_dir })
    }

    /// Get the path to the temporary directory
    pub fn temp_path(&self) -> PathBuf {
        self.temp_dir.path().to_path_buf()
    }

    /// Create a test JSON file
    pub fn create_json_file(&self, name: &str, content: &str) -> Result<PathBuf, std::io::Error> {
        use std::fs;
        use std::io::Write;

        let file_path = self.temp_path().join(name);
        let mut file = fs::File::create(&file_path)?;
        file.write_all(content.as_bytes())?;
        Ok(file_path)
    }

    /// Create a temporary file with given content
    #[allow(dead_code)]
    pub fn create_file(&self, name: &str, content: &[u8]) -> Result<PathBuf, std::io::Error> {
        use std::fs;
        use std::io::Write;

        let file_path = self.temp_path().join(name);
        let mut file = fs::File::create(&file_path)?;
        file.write_all(content)?;
        Ok(file_path)
    }
}

impl Default for TestContext {
    fn default() -> Self {
        Self::new().expect("Failed to create test context")
    }
}

/// Get the soroban-debug command
#[allow(deprecated)]
pub fn cmd() -> assert_cmd::Command {
    assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find soroban-debug binary")
}

/// Helper to assert command contains text in output
/// Used for stdout/stderr validation
#[allow(dead_code)]
pub fn cmd_help_output() -> String {
    let output = cmd()
        .arg("--help")
        .output()
        .expect("Failed to run help command")
        .stdout;
    String::from_utf8(output).expect("Invalid UTF-8 in stdout")
}

/// Validate JSON output string
pub fn validate_json_string(json_str: &str) -> Result<serde_json::Value, serde_json::Error> {
    serde_json::from_str(json_str)
}

/// Sample valid arguments JSON for testing
pub fn sample_args_json() -> String {
    r#"["arg1", "arg2", 123]"#.to_string()
}

/// Sample valid storage JSON for testing
pub fn sample_storage_json() -> String {
    r#"{"key1": "value1", "key2": 42}"#.to_string()
}

/// Sample valid trace JSON for compare command
pub fn sample_trace_json(contract_hash: &str, functions_count: usize) -> String {
    let mut functions = Vec::new();
    for i in 0..functions_count {
        functions.push(format!(
            r#"{{"name": "func{}", "cost": {}, "budget_consumed": {}}}"#,
            i,
            1000 + i * 100,
            500 + i * 50
        ));
    }

    format!(
        r#"{{
  "version": "0.1.0",
  "contract_hash": "{}",
  "execution_time_ms": 1234,
  "total_gas_cost": 50000,
  "functions": [{}]
}}"#,
        contract_hash,
        functions.join(",")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_creation() {
        let ctx = TestContext::new().expect("Failed to create test context");
        assert!(ctx.temp_path().exists());
    }

    #[test]
    fn test_create_json_file() {
        let ctx = TestContext::new().expect("Failed to create test context");
        let json_content = r#"{"key": "value"}"#;
        let path = ctx
            .create_json_file("test.json", json_content)
            .expect("Failed to create JSON file");

        assert!(path.exists());
        let contents = std::fs::read_to_string(&path).expect("Failed to read file");
        assert_eq!(contents, json_content);
    }

    #[test]
    fn test_sample_json_validity() {
        let args = sample_args_json();
        assert!(validate_json_string(&args).is_ok());

        let storage = sample_storage_json();
        assert!(validate_json_string(&storage).is_ok());

        let trace = sample_trace_json("abc123", 3);
        assert!(validate_json_string(&trace).is_ok());
    }
}
