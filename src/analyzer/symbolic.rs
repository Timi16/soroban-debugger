use crate::runtime::executor::ContractExecutor;
use crate::Result;
use serde::Serialize;
use std::collections::HashSet;
use std::fmt::Write;
use wasmparser::{Parser, Payload};

#[derive(Debug, Clone, Serialize)]
pub struct PathResult {
    pub inputs: String, // json array of args
    pub return_value: Option<String>,
    pub panic: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SymbolicReport {
    pub function: String,
    pub paths_explored: usize,
    pub panics_found: usize,
    pub paths: Vec<PathResult>,
}

#[derive(Default)]
pub struct SymbolicAnalyzer;

impl SymbolicAnalyzer {
    pub fn new() -> Self {
        Self
    }

    pub fn analyze(&self, wasm: &[u8], function: &str) -> Result<SymbolicReport> {
        let arg_count = self.get_arg_count(wasm, function).unwrap_or(0);
        let combinations = self.generate_input_combinations(arg_count);

        let mut report = SymbolicReport {
            function: function.to_string(),
            paths_explored: 0,
            panics_found: 0,
            paths: Vec::new(),
        };

        // We use a set of strings to consider a "path" as unique by its return value or panic
        let mut unique_outcomes = HashSet::new();

        for args_json in combinations.iter().take(100) {
            let executor_res = std::panic::catch_unwind(|| {
                if let Ok(executor) = ContractExecutor::new(wasm.to_vec()) {
                    executor.execute(function, Some(args_json))
                } else {
                    Err(crate::DebuggerError::ExecutionError("Init fail".into()).into())
                }
            });

            match executor_res {
                Ok(Ok(val)) => {
                    if unique_outcomes.insert(format!("OK:{}", val)) {
                        report.paths.push(PathResult {
                            inputs: args_json.clone(),
                            return_value: Some(val),
                            panic: None,
                        });
                    }
                }
                Ok(Err(err)) => {
                    let err_str = err.to_string();
                    if unique_outcomes.insert(format!("ERR:{}", err_str)) {
                        report.panics_found += 1;
                        report.paths.push(PathResult {
                            inputs: args_json.clone(),
                            return_value: None,
                            panic: Some(err_str),
                        });
                    }
                }
                Err(_) => {
                    if unique_outcomes.insert("PANIC:HOST".to_string()) {
                        report.panics_found += 1;
                        report.paths.push(PathResult {
                            inputs: args_json.clone(),
                            return_value: None,
                            panic: Some("Host Panic".to_string()),
                        });
                    }
                }
            }
            report.paths_explored += 1;
        }

        Ok(report)
    }

    fn get_arg_count(&self, wasm: &[u8], target: &str) -> Result<usize> {
        let parser = Parser::new(0);
        let mut type_definitions = Vec::new();
        let mut function_types = Vec::new();
        let mut exports = Vec::new();

        for payload in parser.parse_all(wasm) {
            match payload? {
                Payload::TypeSection(reader) => {
                    for rec_group in reader {
                        let rec_group = rec_group?;
                        for ty in rec_group.types() {
                            if let wasmparser::CompositeType::Func(func_type) = &ty.composite_type {
                                type_definitions.push(func_type.clone());
                            }
                        }
                    }
                }
                Payload::FunctionSection(reader) => {
                    for type_idx in reader {
                        function_types.push(type_idx?);
                    }
                }
                Payload::ExportSection(reader) => {
                    for export in reader {
                        let export = export?;
                        if let wasmparser::ExternalKind::Func = export.kind {
                            exports.push((export.name.to_string(), export.index));
                        }
                    }
                }
                _ => {}
            }
        }

        for (name, func_idx) in exports {
            if name == target {
                if let Some(&type_idx) = function_types.get(func_idx as usize) {
                    if let Some(func_type) = type_definitions.get(type_idx as usize) {
                        return Ok(func_type.params().len());
                    }
                }
            }
        }

        anyhow::bail!("Function not found in exports");
    }

    fn generate_input_combinations(&self, arg_count: usize) -> Vec<String> {
        // Values representing symbolic extremes
        let values = vec!["0", "1", "-1", "42", "2147483647", "-2147483648"];

        let mut combinations = Vec::new();
        if arg_count == 0 {
            combinations.push("[]".to_string());
            return combinations;
        }

        if arg_count == 1 {
            for v in &values {
                combinations.push(format!("[{}]", v));
            }
            return combinations;
        }

        if arg_count == 2 {
            for v1 in &values {
                for v2 in &values {
                    combinations.push(format!("[{}, {}]", v1, v2));
                }
            }
            return combinations;
        }

        // Fallback or generic loop for multiple args (cartesian product limited)
        combinations.push("[]".to_string());
        combinations
    }

    pub fn generate_scenario_toml(&self, report: &SymbolicReport) -> String {
        let mut toml = String::new();
        writeln!(toml, "# Generated Symbolic Execution Scenarios").unwrap();
        writeln!(toml, "function = \"{}\"", report.function).unwrap();
        writeln!(toml, "paths_explored = {}", report.paths_explored).unwrap();
        writeln!(toml, "panics_found = {}\n", report.panics_found).unwrap();

        for (i, path) in report.paths.iter().enumerate() {
            writeln!(toml, "[[scenario]]").unwrap();
            writeln!(toml, "id = {}", i).unwrap();
            writeln!(toml, "inputs = '{}'", path.inputs).unwrap();

            if let Some(ref val) = path.return_value {
                writeln!(toml, "expected_return = '{}'", val).unwrap();
            }
            if let Some(ref panic) = path.panic {
                let clean_panic = panic.replace("\"", "\\\"");
                writeln!(toml, "panic = \"{}\"", clean_panic).unwrap();
            }
            writeln!(toml).unwrap();
        }

        toml
    }
}
