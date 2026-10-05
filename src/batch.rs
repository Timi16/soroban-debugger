use crate::runtime::executor::ContractExecutor;
use crate::DebuggerError;
use crate::Result;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::thread_local;
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
    /// When true, use exact string match; when false (default), use semantic comparison
    #[serde(default)]
    pub strict: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum BatchItemInput {
    Structured {
        args: Value,
        #[serde(default)]
        expected: Option<Value>,
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        strict: bool,
    },
    RawArgs(Value),
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
    wasm_bytes: Arc<Vec<u8>>,
    function: String,
}

// Thread-local storage for executors to avoid re-initialization
thread_local! {
    static THREAD_EXECUTOR: RefCell<Option<(Arc<Vec<u8>>, ContractExecutor)>> = const { RefCell::new(None) };
}

impl BatchExecutor {
    /// Create a new batch executor
    pub fn new(wasm_bytes: Vec<u8>, function: String) -> Result<Self> {
        Ok(Self {
            wasm_bytes: Arc::new(wasm_bytes),
            function,
        })
    }

    /// Load batch items from a JSON file
    pub fn load_batch_file<P: AsRef<Path>>(path: P) -> Result<Vec<BatchItem>> {
        let content = fs::read_to_string(path.as_ref()).map_err(|e| {
            DebuggerError::FileError(format!(
                "Failed to read batch file {:?}: {}",
                path.as_ref(),
                e
            ))
        })?;

        let parsed: Vec<BatchItemInput> = serde_json::from_str(&content).map_err(|e| {
            DebuggerError::FileError(format!(
                "Failed to parse batch file as JSON array {:?}: {}",
                path.as_ref(),
                e
            ))
        })?;

        let items = parsed
            .into_iter()
            .map(BatchItem::from)
            .collect::<Vec<BatchItem>>();

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

        let (result_str, success, error) = THREAD_EXECUTOR.with(|executor_cell| {
            let mut executor_ref = executor_cell.borrow_mut();

            // Check if we need to create/recreate the executor
            if let Some((wasm_bytes, _)) = executor_ref.as_ref() {
                if Arc::ptr_eq(wasm_bytes, &self.wasm_bytes) {
                    // Reuse existing executor
                    if let Some(executor) = executor_ref.as_mut() {
                        return match executor.1.execute(&self.function, Some(&item.args)) {
                            Ok(result) => (result, true, None),
                            Err(e) => (String::new(), false, Some(format!("{:#}", e))),
                        };
                    }
                }
            }

            // Create new executor
            match ContractExecutor::new((*self.wasm_bytes).clone()) {
                Ok(mut executor) => {
                    let result = match executor.execute(&self.function, Some(&item.args)) {
                        Ok(result) => (result, true, None),
                        Err(e) => (String::new(), false, Some(format!("{:#}", e))),
                    };
                    *executor_ref = Some((Arc::clone(&self.wasm_bytes), executor));
                    result
                }
                Err(e) => (String::new(), false, Some(format!("{:#}", e))),
            }
        });

        let duration = start.elapsed().as_millis();

        let passed = if let Some(expected) = &item.expected {
            success && values_match(&result_str, expected, item.strict)
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
            duration_ms: duration,
        }
    }

    /// Generate summary from batch results
    pub fn summarize(results: &[BatchResult]) -> BatchSummary {
        let total = results.len();
        let passed = results.iter().filter(|r| r.passed).count();
        let failed = results.iter().filter(|r| r.success && !r.passed).count();
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

        crate::logging::log_display("", crate::logging::LogLevel::Info);
        crate::logging::log_display("=".repeat(80), crate::logging::LogLevel::Info);
        crate::logging::log_display("  Batch Execution Results", crate::logging::LogLevel::Info);
        crate::logging::log_display("=".repeat(80), crate::logging::LogLevel::Info);

        for result in results {
            let status = if result.passed {
                "PASS"
            } else if result.success {
                "FAIL"
            } else {
                "ERROR"
            };

            let default_label = format!("Test #{}", result.index);
            let label = result.label.as_deref().unwrap_or(&default_label);
            crate::logging::log_display(
                format!("\n{} {}", status, label),
                crate::logging::LogLevel::Info,
            );
            crate::logging::log_display(
                format!("  Args: {}", result.args),
                crate::logging::LogLevel::Info,
            );

            if result.success {
                crate::logging::log_display(
                    format!("  Result: {}", result.result),
                    crate::logging::LogLevel::Info,
                );
                if let Some(expected) = &result.expected {
                    crate::logging::log_display(
                        format!("  Expected: {}", expected),
                        crate::logging::LogLevel::Info,
                    );
                    if !result.passed {
                        crate::logging::log_display(
                            format!(
                                "  {}",
                                Formatter::warning("Result does not match expected value")
                            ),
                            crate::logging::LogLevel::Warn,
                        );
                    }
                }
            } else if let Some(error) = &result.error {
                crate::logging::log_display(
                    format!("  Error: {}", Formatter::error(error)),
                    crate::logging::LogLevel::Error,
                );
            }

            crate::logging::log_display(
                format!("  Duration: {}ms", result.duration_ms),
                crate::logging::LogLevel::Info,
            );
        }

        crate::logging::log_display("", crate::logging::LogLevel::Info);
        crate::logging::log_display("=".repeat(80), crate::logging::LogLevel::Info);
        crate::logging::log_display("  Summary", crate::logging::LogLevel::Info);
        crate::logging::log_display("=".repeat(80), crate::logging::LogLevel::Info);
        crate::logging::log_display(
            format!("  Total:    {}", summary.total),
            crate::logging::LogLevel::Info,
        );
        crate::logging::log_display(
            format!(
                "  {}",
                Formatter::success(format!("Passed:   {}", summary.passed))
            ),
            crate::logging::LogLevel::Info,
        );

        if summary.failed > 0 {
            crate::logging::log_display(
                format!(
                    "  {}",
                    Formatter::warning(format!("Failed:   {}", summary.failed))
                ),
                crate::logging::LogLevel::Warn,
            );
        }

        if summary.errors > 0 {
            crate::logging::log_display(
                format!(
                    "  {}",
                    Formatter::error(format!("Errors:   {}", summary.errors))
                ),
                crate::logging::LogLevel::Error,
            );
        }

        crate::logging::log_display(
            format!("  Duration: {}ms", summary.total_duration_ms),
            crate::logging::LogLevel::Info,
        );
        crate::logging::log_display("=".repeat(80), crate::logging::LogLevel::Info);
    }
}

/// Compare a result against an expected value.
///
/// In loose mode (default, `strict = false`):
/// - If both strings parse as valid JSON, compare the decoded values so that
///   formatting differences (`{"a":1}` vs `{ "a": 1 }`) and equivalent number
///   representations (`1` vs `1.0`) are treated as equal.
/// - Otherwise fall back to trimmed-string comparison.
///
/// In strict mode (`strict = true`) the raw strings must be identical.
fn values_match(result: &str, expected: &str, strict: bool) -> bool {
    if strict {
        return result == expected;
    }

    // Semantic JSON comparison
    if let (Ok(r), Ok(e)) = (
        serde_json::from_str::<Value>(result),
        serde_json::from_str::<Value>(expected),
    ) {
        return json_values_equal(&r, &e);
    }

    // Fallback: whitespace-normalised string comparison
    result.trim() == expected.trim()
}

/// Recursively compare two JSON values, treating numerically equal numbers as
/// equal regardless of whether they were parsed as integers or floats
/// (e.g. `1` and `1.0` are considered the same).
fn json_values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(n1), Value::Number(n2)) => n1.as_f64() == n2.as_f64(),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| json_values_equal(x, y))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(k, v)| b.get(k).is_some_and(|bv| json_values_equal(v, bv)))
        }
        _ => a == b,
    }
}

impl From<BatchItemInput> for BatchItem {
    fn from(value: BatchItemInput) -> Self {
        match value {
            BatchItemInput::RawArgs(args) => Self {
                args: json_value_to_text(args),
                expected: None,
                label: None,
                strict: false,
            },
            BatchItemInput::Structured {
                args,
                expected,
                label,
                strict,
            } => Self {
                args: json_value_to_text(args),
                expected: expected.map(json_value_to_text),
                label,
                strict,
            },
        }
    }
}

fn json_value_to_text(value: Value) -> String {
    match value {
        Value::String(s) => s,
        other => other.to_string(),
    }
}

#[allow(dead_code)]
fn truncate_for_table(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }

    let mut truncated = value
        .chars()
        .take(limit.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
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
    fn test_values_match_loose_json() {
        // Different whitespace / key order still matches in loose mode
        assert!(values_match(
            r#"{"a":1,"b":2}"#,
            r#"{ "b": 2, "a": 1 }"#,
            false
        ));
        assert!(values_match("42", "42", false));
        // Equivalent numeric representations
        assert!(values_match("1", "1.0", false));
        // Trailing whitespace
        assert!(values_match("hello ", "hello", false));
    }

    #[test]
    fn test_values_match_strict() {
        // Strict mode: exact bytes must match
        assert!(values_match("42", "42", true));
        assert!(!values_match(r#"{"a":1}"#, r#"{ "a": 1 }"#, true));
        assert!(!values_match("hello ", "hello", true));
    }

    #[test]
    fn test_values_match_non_json_loose() {
        // Non-JSON falls back to trimmed comparison
        assert!(values_match("  ok  ", "ok", false));
        assert!(!values_match("ok", "fail", false));
    }

    #[test]
    fn test_batch_item_strict_field() {
        let json = r#"[
            {"args": "[1]", "expected": "1", "strict": true},
            {"args": "[2]", "expected": "2"}
        ]"#;
        let items: Vec<BatchItem> = serde_json::from_str(json).unwrap();
        assert!(items[0].strict);
        assert!(!items[1].strict);
    }

    #[test]
    fn test_batch_summary() {
        let results = vec![
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
            BatchResult {
                index: 2,
                label: None,
                args: "[]".to_string(),
                result: "ok".to_string(),
                success: true,
                error: None,
                expected: Some("ok".to_string()),
                passed: true,
                duration_ms: 10,
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

/////Working
