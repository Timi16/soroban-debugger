use crate::runtime::executor::ContractExecutor;
use crate::Result;
use anyhow::Context;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::Instant;

/// A single batch execution item with arguments and optional expected result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchItem {
    /// Arguments as JSON string
    pub args: String,
    /// Optional expected result for assertion
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    /// Optional label for this test case
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Result of a single batch execution
#[derive(Debug, Clone, Serialize)]
pub struct BatchResult {
    pub index: usize,
    pub label: Option<String>,
    pub args: String,
    pub result: String,
    pub success: bool,
    pub error: Option<String>,
    pub expected: Option<String>,
    pub passed: bool,
    pub duration_ms: u128,
}

/// Summary of batch execution results
#[derive(Debug, Serialize)]
pub struct BatchSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub errors: usize,
    pub total_duration_ms: u128,
}

/// Batch executor for running multiple contract calls in parallel
pub struct BatchExecutor {
    wasm_bytes: Vec<u8>,
    function: String,
}

impl BatchExecutor {
    /// Create a new batch executor
    pub fn new(wasm_bytes: Vec<u8>, function: String) -> Self {
        Self {
            wasm_bytes,
            function,
        }
    }

    /// Load batch items from a JSON file
    pub fn load_batch_file<P: AsRef<Path>>(path: P) -> Result<Vec<BatchItem>> {
        let content = fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read batch file: {:?}", path.as_ref()))?;

        let items: Vec<BatchItem> = serde_json::from_str(&content).with_context(|| {
            format!(
                "Failed to parse batch file as JSON array: {:?}",
                path.as_ref()
            )
        })?;

        Ok(items)
    }

    /// Execute all batch items in parallel
    pub fn execute_batch(&self, items: Vec<BatchItem>) -> Result<Vec<BatchResult>> {
        let results: Vec<BatchResult> = items
            .par_iter()
            .enumerate()
            .map(|(index, item)| self.execute_single(index, item))
            .collect();

        Ok(results)
    }

    /// Execute a single batch item
    fn execute_single(&self, index: usize, item: &BatchItem) -> BatchResult {
        let start = Instant::now();

        let executor_result = ContractExecutor::new(self.wasm_bytes.clone());

        let (result_str, success, error) = match executor_result {
            Ok(executor) => match executor.execute(&self.function, Some(&item.args)) {
                Ok(result) => (result.result, true, None),
                Err(e) => (String::new(), false, Some(format!("{:#}", e))),
            },
            Err(e) => (
                String::new(),
                false,
                Some(format!("Failed to create executor: {:#}", e)),
            ),
        };

        let duration_ms = start.elapsed().as_millis();

        let passed = if let Some(expected) = &item.expected {
            success && result_str.trim() == expected.trim()
        } else {
            success
        };

        BatchResult {
            index,
            label: item.label.clone(),
            args: item.args.clone(),
            result: result_str,
            success,
            error,
            expected: item.expected.clone(),
            passed,
            duration_ms,
        }
    }

    /// Generate summary from results
    pub fn summarize(results: &[BatchResult]) -> BatchSummary {
        let total = results.len();
        let passed = results.iter().filter(|r| r.passed).count();
        let failed = results.iter().filter(|r| !r.passed && r.success).count();
        let errors = results.iter().filter(|r| !r.success).count();
        let total_duration_ms = results.iter().map(|r| r.duration_ms).sum();

        BatchSummary {
            total,
            passed,
            failed,
            errors,
            total_duration_ms,
        }
    }

    /// Display results in a formatted way
    pub fn display_results(results: &[BatchResult], summary: &BatchSummary) {
        use crate::ui::formatter::Formatter;

        println!("\n{}", "=".repeat(80));
        println!("  Batch Execution Results");
        println!("{}", "=".repeat(80));

        for result in results {
            let status = if result.passed {
                Formatter::success("✓ PASS")
            } else if result.success {
                Formatter::warning("✗ FAIL")
            } else {
                Formatter::error("✗ ERROR")
            };

            let default_label = format!("Test #{}", result.index);
            let label = result.label.as_deref().unwrap_or(&default_label);
            println!("\n{} {}", status, label);
            println!("  Args: {}", result.args);

            if result.success {
                println!("  Result: {}", result.result);
                if let Some(expected) = &result.expected {
                    println!("  Expected: {}", expected);
                    if !result.passed {
                        println!(
                            "  {}",
                            Formatter::warning("Result does not match expected value")
                        );
                    }
                }
            } else if let Some(error) = &result.error {
                println!("  Error: {}", Formatter::error(error));
            }

            println!("  Duration: {}ms", result.duration_ms);
        }

        println!("\n{}", "=".repeat(80));
        println!("  Summary");
        println!("{}", "=".repeat(80));
        println!("  Total:    {}", summary.total);
        println!(
            "  {}",
            Formatter::success(format!("Passed:   {}", summary.passed))
        );

        if summary.failed > 0 {
            println!(
                "  {}",
                Formatter::warning(format!("Failed:   {}", summary.failed))
            );
        }

        if summary.errors > 0 {
            println!(
                "  {}",
                Formatter::error(format!("Errors:   {}", summary.errors))
            );
        }

        println!("  Duration: {}ms", summary.total_duration_ms);
        println!("{}", "=".repeat(80));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_item_deserialization() {
        let json = r#"[
            {"args": "[1, 2]", "expected": "3", "label": "Add 1+2"},
            {"args": "[5, 10]"}
        ]"#;

        let items: Vec<BatchItem> = serde_json::from_str(json).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].args, "[1, 2]");
        assert_eq!(items[0].expected, Some("3".to_string()));
        assert_eq!(items[0].label, Some("Add 1+2".to_string()));
        assert_eq!(items[1].args, "[5, 10]");
        assert_eq!(items[1].expected, None);
    }

    #[test]
    fn test_batch_summary() {
        let results = vec![
            BatchResult {
                index: 0,
                label: None,
                args: "[]".to_string(),
                result: "ok".to_string(),
                success: true,
                error: None,
                expected: None,
                passed: true,
                duration_ms: 10,
            },
            BatchResult {
                index: 1,
                label: None,
                args: "[]".to_string(),
                result: "fail".to_string(),
                success: true,
                error: None,
                expected: Some("ok".to_string()),
                passed: false,
                duration_ms: 15,
            },
        ];

        let summary = BatchExecutor::summarize(&results);
        assert_eq!(summary.total, 2);
        assert_eq!(summary.passed, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.errors, 0);
        assert_eq!(summary.total_duration_ms, 25);
    }
}
