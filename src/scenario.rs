use crate::cli::args::{ScenarioArgs, Verbosity};
use crate::debugger::engine::DebuggerEngine;
use crate::inspector::budget::{BudgetInfo, BudgetInspector};
use crate::inspector::events::{ContractEvent, EventInspector};
use crate::logging;
use crate::runtime::executor::{ContractExecutor, DEFAULT_EXECUTION_TIMEOUT_SECS};
use crate::ui::formatter::Formatter;
use crate::{DebuggerError, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Serialize)]
pub struct Scenario {
    /// Optional list of fragment TOML files whose steps are prepended to this scenario.
    /// Paths are resolved relative to the directory that contains this file.
    /// Includes are processed recursively; cycles are detected and reported as errors.
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub defaults: ScenarioDefaults,
    pub steps: Vec<ScenarioStep>,
}

#[derive(Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ScenarioDefaults {
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ScenarioStep {
    pub name: Option<String>,
    pub function: String,
    pub args: Option<String>,
    pub timeout_secs: Option<u64>,
    pub expected_return: Option<String>,
    pub expected_storage: Option<HashMap<String, String>>,
    pub expected_events: Option<Vec<ScenarioEventAssertion>>,
    pub budget_limits: Option<ScenarioBudgetAssertion>,
    /// When set, the step is expected to fail with an error message containing this substring.
    pub expected_error: Option<String>,
    /// When set, the step is expected to panic with a message containing this substring.
    pub expected_panic: Option<String>,
    /// When set, the return value of this step is stored in a variable with this name.
    /// Later steps can reference the value using `{{var_name}}` in their `args` or
    /// `expected_return` fields.
    pub capture: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ScenarioEventAssertion {
    pub contract_id: Option<String>,
    pub topics: Vec<String>,
    pub data: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ScenarioBudgetAssertion {
    pub max_cpu_instructions: Option<u64>,
    pub max_memory_bytes: Option<u64>,
}

/// Load a scenario file, recursively resolving `include` directives.
///
/// `visiting` tracks canonical paths currently on the call stack so that
/// cycles (A includes B includes A) are detected and reported immediately.
pub fn load_scenario(path: &Path, visiting: &mut HashSet<PathBuf>) -> Result<Vec<ScenarioStep>> {
    let canonical = path.canonicalize().map_err(|e| {
        DebuggerError::FileError(format!("Cannot resolve scenario path {:?}: {}", path, e))
    })?;

    if !visiting.insert(canonical.clone()) {
        return Err(DebuggerError::FileError(format!(
            "Cycle detected: scenario file {:?} is already being loaded",
            canonical
        ))
        .into());
    }

    let content = fs::read_to_string(&canonical).map_err(|e| {
        DebuggerError::FileError(format!(
            "Failed to read scenario file {:?}: {}",
            canonical, e
        ))
    })?;

    let scenario: Scenario = toml::from_str(&content).map_err(|e| {
        DebuggerError::FileError(format!(
            "Failed to parse scenario TOML {:?}: {}",
            canonical, e
        ))
    })?;

    let base_dir = canonical.parent().unwrap_or(Path::new("."));

    // Collect steps from all includes first (prepended), then this file's own steps.
    let mut all_steps: Vec<ScenarioStep> = Vec::new();

    for include_path in &scenario.include {
        let resolved = base_dir.join(include_path);
        let fragment_steps = load_scenario(&resolved, visiting)?;
        all_steps.extend(fragment_steps);
    }

    all_steps.extend(scenario.steps);

    visiting.remove(&canonical);
    Ok(all_steps)
}

pub fn run_scenario(args: ScenarioArgs, _verbosity: Verbosity) -> Result<()> {
    println!(
        "{}",
        Formatter::info(format!("Loading scenario file: {:?}", args.scenario))
    );

    let root_content = fs::read_to_string(&args.scenario).map_err(|e| {
        DebuggerError::ExecutionError(format!("Failed to read root scenario file: {}", e))
    })?;
    let root_scenario: Scenario = toml::from_str(&root_content).map_err(|e| {
        DebuggerError::ExecutionError(format!("Failed to parse root scenario file: {}", e))
    })?;

    let mut visiting = HashSet::new();
    let steps = load_scenario(&args.scenario, &mut visiting)?;

    println!(
        "{}",
        Formatter::info(format!("Loading contract: {:?}", args.contract))
    );
    logging::log_loading_contract(&args.contract.to_string_lossy());

    let wasm_file = crate::utils::wasm::load_wasm(&args.contract).map_err(|e| {
        DebuggerError::WasmLoadError(format!("Failed to load WASM {:?}: {}", args.contract, e))
    })?;

    let mut executor = ContractExecutor::new(wasm_file.bytes)?;

    if let Some(storage_json) = &args.storage {
        serde_json::from_str::<serde_json::Value>(storage_json).map_err(|e| {
            DebuggerError::StorageError(format!("Failed to parse initial storage JSON: {}", e))
        })?;
        executor.set_initial_storage(storage_json.clone())?;
    }

    println!(
        "{}",
        Formatter::success(format!("Running {} scenario steps...\n", steps.len()))
    );

    let mut engine = DebuggerEngine::new(executor, vec![]);
    let mut all_passed = true;
    let mut variables: HashMap<String, String> = HashMap::new();

    for (i, step) in steps.iter().enumerate() {
        let step_label = step.name.as_deref().unwrap_or(&step.function);
        let effective_timeout = resolve_step_timeout(
            step.timeout_secs,
            root_scenario.defaults.timeout_secs,
            args.timeout,
        );
        engine.executor_mut().set_timeout(effective_timeout);
        println!(
            "{}",
            Formatter::info(format!("Step {}: {}", i + 1, step_label))
        );

        let resolved_args = if let Some(args_json) = &step.args {
            Some(interpolate_variables(args_json, &variables)?)
        } else {
            None
        };

        let resolved_expected_return = if let Some(expected) = &step.expected_return {
            Some(interpolate_variables(expected, &variables)?)
        } else {
            None
        };

        let parsed_args = if let Some(args_json) = &resolved_args {
            Some(crate::cli::commands::parse_args(args_json)?)
        } else {
            None
        };

        let events_before_len = engine.executor().get_events()?.len();
        let result = engine.execute(&step.function, parsed_args.as_deref());

        let mut step_passed = true;
        let expects_failure = step.expected_error.is_some() || step.expected_panic.is_some();

        match result {
            Ok(res) => {
                if expects_failure {
                    println!(
                        "  {}",
                        Formatter::error(format!(
                            "? Step succeeded with '{}', but was expected to fail",
                            res
                        ))
                    );
                    step_passed = false;
                } else {
                    println!("  Result: {}", res);

                    if let Some(var_name) = &step.capture {
                        variables.insert(var_name.clone(), res.trim().to_string());
                        println!(
                            "  {}",
                            Formatter::info(format!(
                                "Captured return value as '{}' = '{}'",
                                var_name,
                                res.trim()
                            ))
                        );
                    }

                    if let Some(expected) = &resolved_expected_return {
                        if res.trim() == expected.trim() {
                            println!(
                                "  {}",
                                Formatter::success("? Return value assertion passed")
                            );
                        } else {
                            println!(
                                "  {}",
                                Formatter::error(format!(
                                    "? Return value assertion failed! Expected '{}', got '{}'",
                                    expected, res
                                ))
                            );
                            step_passed = false;
                        }
                    }
                }
            }
            Err(e) => {
                let err_msg = format!("{}", e);
                if let Some(expected_error) = &step.expected_error {
                    if err_msg.contains(expected_error.as_str()) {
                        println!(
                            "  {}",
                            Formatter::success(format!(
                                "? Expected error assertion passed (matched '{}')",
                                expected_error
                            ))
                        );
                    } else {
                        println!(
                            "  {}",
                            Formatter::error(format!(
                                "? Expected error '{}', but got '{}'",
                                expected_error, err_msg
                            ))
                        );
                        step_passed = false;
                    }
                } else if let Some(expected_panic) = &step.expected_panic {
                    if err_msg.contains(expected_panic.as_str()) {
                        println!(
                            "  {}",
                            Formatter::success(format!(
                                "? Expected panic assertion passed (matched '{}')",
                                expected_panic
                            ))
                        );
                    } else {
                        println!(
                            "  {}",
                            Formatter::error(format!(
                                "? Expected panic '{}', but got '{}'",
                                expected_panic, err_msg
                            ))
                        );
                        step_passed = false;
                    }
                } else {
                    println!(
                        "  {}",
                        Formatter::error(format!("? Execution failed: {}", e))
                    );
                    step_passed = false;
                }
            }
        }

        if step_passed {
            let events_after = engine.executor().get_events()?;
            let step_events = EventInspector::events_since(&events_after, events_before_len);
            if let Some(expected_events) = &step.expected_events {
                match assert_expected_events(expected_events, &step_events) {
                    Ok(message) => println!("  {}", Formatter::success(message)),
                    Err(message) => {
                        println!("  {}", Formatter::error(message));
                        step_passed = false;
                    }
                }
            }
        }

        if step_passed {
            if let Some(expected_budget) = &step.budget_limits {
                let step_budget = BudgetInspector::get_cpu_usage(engine.executor().host());
                match assert_budget_limits(expected_budget, &step_budget) {
                    Ok(messages) => {
                        for message in messages {
                            println!("  {}", Formatter::success(message));
                        }
                    }
                    Err(messages) => {
                        for message in messages {
                            println!("  {}", Formatter::error(message));
                        }
                        step_passed = false;
                    }
                }
            }
        }

        if step_passed {
            if let Some(expected_storage) = &step.expected_storage {
                let snapshot = engine.executor().get_storage_snapshot()?;
                let mut storage_passed = true;
                for (key, expected_val) in expected_storage {
                    if let Some(actual_val) = snapshot.get(key) {
                        if actual_val.trim() == expected_val.trim() {
                            println!(
                                "  {}",
                                Formatter::success(format!(
                                    "? Storage assertion passed for key '{}'",
                                    key
                                ))
                            );
                        } else {
                            println!("  {}", Formatter::error(format!("? Storage assertion failed for key '{}'! Expected '{}', got '{}'", key, expected_val, actual_val)));
                            storage_passed = false;
                        }
                    } else {
                        println!(
                            "  {}",
                            Formatter::error(format!(
                                "? Storage assertion failed! Key '{}' not found",
                                key
                            ))
                        );
                        storage_passed = false;
                    }
                }
                if !storage_passed {
                    step_passed = false;
                }
            }
        }

        if step_passed {
            println!(
                "{}",
                Formatter::success(format!("Step {} passed.\n", i + 1))
            );
        } else {
            println!(
                "{}",
                Formatter::warning(format!("Step {} failed.\n", i + 1))
            );
            all_passed = false;
            break;
        }
    }

    if all_passed {
        println!(
            "{}",
            Formatter::success("All scenario steps passed successfully!")
        );
        Ok(())
    } else {
        Err(DebuggerError::ExecutionError("Scenario execution failed".into()).into())
    }
}

/// Replaces `{{var_name}}` placeholders in `template` with values from `variables`.
fn interpolate_variables(template: &str, variables: &HashMap<String, String>) -> Result<String> {
    let re = Regex::new(r"\{\{(\w+)\}\}").unwrap();

    let missing: Vec<String> = re
        .captures_iter(template)
        .filter_map(|caps| {
            let var_name = caps[1].to_string();
            if variables.contains_key(&var_name) {
                None
            } else {
                Some(var_name)
            }
        })
        .collect();

    if !missing.is_empty() {
        let available: Vec<&String> = variables.keys().collect();
        let available_str = if available.is_empty() {
            "(none)".to_string()
        } else {
            available
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        return Err(DebuggerError::ExecutionError(format!(
            "Undefined variable(s) referenced in scenario step: [{}]. Available variables: [{}]",
            missing.join(", "),
            available_str
        ))
        .into());
    }

    let result = re.replace_all(template, |caps: &regex::Captures| {
        variables[&caps[1]].clone()
    });

    Ok(result.into_owned())
}

fn assert_expected_events(
    expected_events: &[ScenarioEventAssertion],
    actual_events: &[ContractEvent],
) -> std::result::Result<String, String> {
    if actual_events.len() != expected_events.len() {
        return Err(format!(
            "Event assertion failed! Expected {} event(s), got {}",
            expected_events.len(),
            actual_events.len()
        ));
    }

    for (index, (expected, actual)) in expected_events.iter().zip(actual_events.iter()).enumerate()
    {
        if expected.contract_id.as_deref() != actual.contract_id.as_deref()
            || expected.topics != actual.topics
            || expected.data.trim() != actual.data.trim()
        {
            return Err(format!(
                "Event assertion failed for event #{}! Expected {:?}, got {:?}",
                index, expected, actual
            ));
        }
    }

    Ok(format!(
        "? Event assertion passed ({} event(s) matched)",
        actual_events.len()
    ))
}

fn assert_budget_limits(
    expected_budget: &ScenarioBudgetAssertion,
    actual_budget: &BudgetInfo,
) -> std::result::Result<Vec<String>, Vec<String>> {
    let mut passed = Vec::new();
    let mut failed = Vec::new();

    if let Some(max_cpu) = expected_budget.max_cpu_instructions {
        if actual_budget.cpu_instructions <= max_cpu {
            passed.push(format!(
                "? CPU budget assertion passed (used {}, limit {})",
                actual_budget.cpu_instructions, max_cpu
            ));
        } else {
            failed.push(format!(
                "? CPU budget assertion failed! Used {}, limit {}",
                actual_budget.cpu_instructions, max_cpu
            ));
        }
    }

    if let Some(max_memory) = expected_budget.max_memory_bytes {
        if actual_budget.memory_bytes <= max_memory {
            passed.push(format!(
                "? Memory budget assertion passed (used {}, limit {})",
                actual_budget.memory_bytes, max_memory
            ));
        } else {
            failed.push(format!(
                "? Memory budget assertion failed! Used {}, limit {}",
                actual_budget.memory_bytes, max_memory
            ));
        }
    }

    if failed.is_empty() {
        Ok(passed)
    } else {
        Err(failed)
    }
}

fn resolve_step_timeout(
    step_timeout_secs: Option<u64>,
    scenario_default_timeout_secs: Option<u64>,
    cli_timeout_secs: Option<u64>,
) -> u64 {
    step_timeout_secs
        .or(scenario_default_timeout_secs)
        .or(cli_timeout_secs)
        .unwrap_or(DEFAULT_EXECUTION_TIMEOUT_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_interpolate_variables_replaces_known_placeholders() {
        let mut vars = HashMap::new();
        vars.insert("count".to_string(), "I64(1)".to_string());
        vars.insert("name".to_string(), "Alice".to_string());

        let result = interpolate_variables("[{{count}}, \"{{name}}\"]", &vars).unwrap();
        assert_eq!(result, "[I64(1), \"Alice\"]");
    }

    #[test]
    fn test_interpolate_variables_no_placeholders_is_identity() {
        let vars: HashMap<String, String> = HashMap::new();
        let result = interpolate_variables("[1, 2, 3]", &vars).unwrap();
        assert_eq!(result, "[1, 2, 3]");
    }

    #[test]
    fn test_interpolate_variables_errors_on_undefined_variable() {
        let mut vars = HashMap::new();
        vars.insert("defined".to_string(), "42".to_string());

        let err = interpolate_variables("{{defined}} and {{missing}}", &vars).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("missing"),
            "error should name the missing variable: {}",
            msg
        );
        assert!(
            msg.contains("defined"),
            "error should list available variables: {}",
            msg
        );
    }

    #[test]
    fn test_interpolate_variables_errors_on_undefined_with_no_available_vars() {
        let vars: HashMap<String, String> = HashMap::new();
        let err = interpolate_variables("{{unknown}}", &vars).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unknown"),
            "error should name the missing variable: {}",
            msg
        );
        assert!(
            msg.contains("(none)"),
            "error should say no vars are available: {}",
            msg
        );
    }

    #[test]
    fn test_capture_field_deserialization() {
        let toml_str = r#"
            [[steps]]
            function = "increment"
            args = "[]"
            capture = "my_result"

            [[steps]]
            function = "get"
            expected_return = "{{my_result}}"
        "#;

        let scenario: Scenario = toml::from_str(toml_str).unwrap();
        assert_eq!(scenario.defaults, ScenarioDefaults::default());
        assert_eq!(scenario.steps[0].capture.as_deref(), Some("my_result"));
        assert!(scenario.steps[1].capture.is_none());
        assert_eq!(
            scenario.steps[1].expected_return.as_deref(),
            Some("{{my_result}}")
        );
    }

    #[test]
    fn test_scenario_deserialization() {
        let toml_str = r#"
            [defaults]
            timeout_secs = 45

            [[steps]]
            name = "Init"
            function = "init"
            args = '["admin", 10]'
            expected_return = "()"

            [[steps]]
            name = "Get Counter"
            function = "get"
            expected_return = "1"
            [[steps.expected_events]]
            contract_id = "contract-1"
            topics = ["topic-a"]
            data = "payload"
            [steps.budget_limits]
            max_cpu_instructions = 100
            max_memory_bytes = 200
            [steps.expected_storage]
            "Counter" = "1"
        "#;

        let scenario: Scenario = toml::from_str(toml_str).unwrap();
        assert_eq!(scenario.steps.len(), 2);
        assert!(scenario.include.is_empty());
    }

    #[test]
    fn test_include_field_deserialization() {
        let toml_str = r#"
            include = ["setup.toml", "auth.toml"]

            [[steps]]
            function = "increment"
        "#;

        let scenario: Scenario = toml::from_str(toml_str).unwrap();
        assert_eq!(scenario.include, vec!["setup.toml", "auth.toml"]);
        assert_eq!(scenario.steps.len(), 1);
    }

    #[test]
    fn test_load_scenario_no_includes() {
        let dir = TempDir::new().unwrap();
        let path = write_file(
            dir.path(),
            "main.toml",
            r#"
[[steps]]
function = "increment"
args = "[]"
"#,
        );

        let mut visiting = HashSet::new();
        let steps = load_scenario(&path, &mut visiting).unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].function, "increment");
    }

    #[test]
    fn test_load_scenario_with_single_include() {
        let dir = TempDir::new().unwrap();

        write_file(
            dir.path(),
            "setup.toml",
            r#"
[[steps]]
function = "init"
args = "[]"
"#,
        );

        let main = write_file(
            dir.path(),
            "main.toml",
            r#"
include = ["setup.toml"]

[[steps]]
function = "increment"
args = "[]"
"#,
        );

        let mut visiting = HashSet::new();
        let steps = load_scenario(&main, &mut visiting).unwrap();

        // setup steps come first, then main steps
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].function, "init");
        assert_eq!(steps[1].function, "increment");
    }

    #[test]
    fn test_load_scenario_with_nested_includes() {
        let dir = TempDir::new().unwrap();

        write_file(
            dir.path(),
            "base.toml",
            r#"
[[steps]]
function = "base_setup"
"#,
        );

        write_file(
            dir.path(),
            "middle.toml",
            r#"
include = ["base.toml"]

[[steps]]
function = "middle_step"
"#,
        );

        let main = write_file(
            dir.path(),
            "main.toml",
            r#"
include = ["middle.toml"]

[[steps]]
function = "final_step"
"#,
        );

        let mut visiting = HashSet::new();
        let steps = load_scenario(&main, &mut visiting).unwrap();

        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].function, "base_setup");
        assert_eq!(steps[1].function, "middle_step");
        assert_eq!(steps[2].function, "final_step");
    }

    #[test]
    fn test_load_scenario_cycle_detection() {
        let dir = TempDir::new().unwrap();

        // a.toml includes b.toml, b.toml includes a.toml � cycle
        write_file(
            dir.path(),
            "a.toml",
            r#"
include = ["b.toml"]

[[steps]]
function = "step_a"
"#,
        );

        write_file(
            dir.path(),
            "b.toml",
            r#"
include = ["a.toml"]

[[steps]]
function = "step_b"
"#,
        );

        let a = dir.path().join("a.toml");
        let mut visiting = HashSet::new();
        let err = load_scenario(&a, &mut visiting).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Cycle detected"),
            "expected cycle error, got: {}",
            msg
        );
    }

    #[test]
    fn test_load_scenario_missing_include_file() {
        let dir = TempDir::new().unwrap();

        let main = write_file(
            dir.path(),
            "main.toml",
            r#"
include = ["nonexistent.toml"]

[[steps]]
function = "increment"
"#,
        );

        let mut visiting = HashSet::new();
        let err = load_scenario(&main, &mut visiting).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("nonexistent") || msg.contains("Cannot resolve"),
            "expected file-not-found error, got: {}",
            msg
        );
    }

    #[test]
    fn test_expected_error_deserialization() {
        let toml_str = r#"
            [[steps]]
            name = "Should fail"
            function = "bad_fn"
            expected_error = "unauthorized"
        "#;

        let scenario: Scenario = toml::from_str(toml_str).unwrap();
        assert_eq!(scenario.steps.len(), 1);
        assert_eq!(
            scenario.steps[0].expected_error.as_deref(),
            Some("unauthorized")
        );
    }

    #[test]
    fn test_expected_panic_deserialization() {
        let toml_str = r#"
            [[steps]]
            name = "Should panic"
            function = "panic_fn"
            expected_panic = "index out of bounds"
        "#;

        let scenario: Scenario = toml::from_str(toml_str).unwrap();
        assert_eq!(
            scenario.steps[0].expected_panic.as_deref(),
            Some("index out of bounds")
        );
    }

    #[test]
    fn test_backward_compat_no_error_fields() {
        let toml_str = r#"
            [[steps]]
            function = "increment"
            args = "[]"
            expected_return = "1"
        "#;

        let scenario: Scenario = toml::from_str(toml_str).unwrap();
        assert_eq!(scenario.steps.len(), 1);
        assert!(scenario.steps[0].expected_error.is_none());
        assert!(scenario.steps[0].expected_panic.is_none());
    }

    #[test]
    fn test_step_timeout_override_deserialization() {
        let toml_str = r#"
            [defaults]
            timeout_secs = 30

            [[steps]]
            function = "increment"
            timeout_secs = 0
            expected_return = "1"
        "#;

        let scenario: Scenario = toml::from_str(toml_str).unwrap();
        assert_eq!(scenario.defaults.timeout_secs, Some(30));
        assert_eq!(scenario.steps[0].timeout_secs, Some(0));
    }

    #[test]
    fn test_effective_timeout_prefers_step_override() {
        let effective = resolve_step_timeout(Some(5), Some(20), Some(30));
        assert_eq!(effective, 5);
    }

    #[test]
    fn test_effective_timeout_prefers_scenario_default_over_cli() {
        let effective = resolve_step_timeout(None, Some(20), Some(30));
        assert_eq!(effective, 20);
    }

    #[test]
    fn test_effective_timeout_falls_back_to_cli_or_runtime_default() {
        assert_eq!(resolve_step_timeout(None, None, Some(30)), 30);
        assert_eq!(
            resolve_step_timeout(None, None, None),
            DEFAULT_EXECUTION_TIMEOUT_SECS
        );
    }

    #[test]
    fn test_event_assertion_passes_for_exact_match() {
        let expected = vec![ScenarioEventAssertion {
            contract_id: None,
            topics: vec!["topic".to_string()],
            data: "payload".to_string(),
        }];
        let actual = vec![ContractEvent {
            contract_id: None,
            topics: vec!["topic".to_string()],
            data: "payload".to_string(),
        }];

        assert!(assert_expected_events(&expected, &actual).is_ok());
    }

    #[test]
    fn test_event_assertion_fails_for_unexpected_event() {
        let expected = vec![];
        let actual = vec![ContractEvent {
            contract_id: None,
            topics: vec!["topic".to_string()],
            data: "payload".to_string(),
        }];

        let err = assert_expected_events(&expected, &actual).unwrap_err();
        assert!(err.contains("Expected 0 event(s), got 1"));
    }

    #[test]
    fn test_budget_assertion_passes_within_limits() {
        let expected = ScenarioBudgetAssertion {
            max_cpu_instructions: Some(10),
            max_memory_bytes: Some(20),
        };
        let actual = BudgetInfo {
            cpu_instructions: 8,
            cpu_limit: 100,
            memory_bytes: 15,
            memory_limit: 100,
        };

        assert!(assert_budget_limits(&expected, &actual).is_ok());
    }

    #[test]
    fn test_budget_assertion_fails_when_limits_exceeded() {
        let expected = ScenarioBudgetAssertion {
            max_cpu_instructions: Some(10),
            max_memory_bytes: Some(20),
        };
        let actual = BudgetInfo {
            cpu_instructions: 12,
            cpu_limit: 100,
            memory_bytes: 30,
            memory_limit: 100,
        };

        let err = assert_budget_limits(&expected, &actual).unwrap_err();
        assert_eq!(err.len(), 2);
        assert!(err[0].contains("CPU budget assertion failed"));
        assert!(err[1].contains("Memory budget assertion failed"));
    }
}
