use crate::profiler::analyzer::OptimizationReport;
use crate::Result;
use std::fmt::Write as _;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct FlameGraphStack {
    pub stack: Vec<String>,
    pub count: u64,
}

pub struct FlameGraphGenerator;

impl FlameGraphGenerator {
    pub fn from_report(report: &OptimizationReport) -> Vec<FlameGraphStack> {
        let mut stacks = Vec::new();

        for function in &report.functions {
            let cpu_per_unit = if function.total_cpu > 0 {
                function.total_cpu as f64 / 1000.0
            } else {
                1.0
            };

            let stack_count = (function.total_cpu as f64 / cpu_per_unit).max(1.0) as u64;
            stacks.push(FlameGraphStack {
                stack: vec![function.name.clone()],
                count: stack_count,
            });

            for op in &function.operations {
                let op_cost = (op.cpu_cost + op.memory_cost) as f64;
                if op_cost > 0.0 {
                    let op_count = (op_cost / cpu_per_unit).max(1.0) as u64;
                    stacks.push(FlameGraphStack {
                        stack: vec![
                            function.name.clone(),
                            format!("{};{}", op.operation, op.location),
                        ],
                        count: op_count,
                    });
                }
            }

            for (key, access) in &function.storage_accesses {
                let cost = access.total_cpu as f64;
                if cost > 0.0 {
                    let access_count = (cost / cpu_per_unit).max(1.0) as u64;
                    stacks.push(FlameGraphStack {
                        stack: vec![
                            function.name.clone(),
                            format!("storage;key={};access_count={}", key, access.access_count),
                        ],
                        count: access_count,
                    });
                }
            }
        }

        stacks
    }

    pub fn to_collapsed_stack_format(stacks: &[FlameGraphStack]) -> String {
        let mut out = String::new();
        for stack in stacks {
            let line = stack.stack.join(";");
            let _ = writeln!(out, "{} {}", line, stack.count);
        }
        out
    }

    pub fn generate_svg(
        stacks: &[FlameGraphStack],
        width: usize,
        _height: usize,
    ) -> Result<String> {
        let collapsed = Self::to_collapsed_stack_format(stacks);
        let reader = std::io::Cursor::new(collapsed);

        let mut opts = inferno::flamegraph::Options::default();
        opts.image_width = Some(width);

        let mut svg = Vec::new();
        inferno::flamegraph::from_reader(&mut opts, reader, &mut svg).map_err(|e| {
            crate::DebuggerError::ExecutionError(format!("Flamegraph render error: {e}"))
        })?;

        String::from_utf8(svg)
            .map_err(|e| crate::DebuggerError::ExecutionError(format!("UTF-8 error: {e}")).into())
    }

    pub fn write_collapsed_stack_file<P: AsRef<Path>>(
        stacks: &[FlameGraphStack],
        path: P,
    ) -> Result<()> {
        let collapsed = Self::to_collapsed_stack_format(stacks);
        std::fs::write(&path, collapsed).map_err(|e| {
            crate::DebuggerError::FileError(format!(
                "Failed to write flamegraph to {:?}: {}",
                path.as_ref(),
                e
            ))
        })?;
        Ok(())
    }

    pub fn write_svg_file<P: AsRef<Path>>(
        stacks: &[FlameGraphStack],
        path: P,
        width: usize,
        height: usize,
    ) -> Result<()> {
        let svg = Self::generate_svg(stacks, width, height)?;
        std::fs::write(&path, svg).map_err(|e| {
            crate::DebuggerError::FileError(format!(
                "Failed to write SVG to {:?}: {}",
                path.as_ref(),
                e
            ))
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiler::analyzer::FunctionProfile;
    use std::collections::HashMap;

    fn create_test_report() -> OptimizationReport {
        OptimizationReport {
            contract_path: "/test/contract.wasm".to_string(),
            functions: vec![FunctionProfile {
                name: "test_function".to_string(),
                total_cpu: 1000,
                total_memory: 5000,
                wall_time_ms: 100,
                operations: vec![],
                storage_accesses: HashMap::new(),
                call_tree: None,
            }],
            suggestions: vec![],
            total_cpu: 1000,
            total_memory: 5000,
            potential_cpu_savings: 0,
            potential_memory_savings: 0,
        }
    }

    #[test]
    #[ignore = "Stubbed until inferno dependency is resolved"]
    fn test_flame_graph_generation_from_report() {
        let report = create_test_report();
        let stacks = FlameGraphGenerator::from_report(&report);

        assert!(!stacks.is_empty());
        assert_eq!(stacks[0].stack[0], "test_function");
        assert!(stacks[0].count > 0);
    }

    #[test]
    #[ignore = "Stubbed until inferno dependency is resolved"]
    fn test_collapsed_stack_format() {
        let stacks = vec![FlameGraphStack {
            stack: vec!["func1".to_string(), "func2".to_string()],
            count: 42,
        }];

        let output = FlameGraphGenerator::to_collapsed_stack_format(&stacks);
        assert!(output.contains("func1;func2 42"));
    }

    #[test]
    #[ignore = "Stubbed until inferno dependency is resolved"]
    fn test_write_collapsed_stack_file() {
        let stacks = vec![FlameGraphStack {
            stack: vec!["test_func".to_string()],
            count: 100,
        }];

        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("test_flamegraph.stacks");

        assert!(FlameGraphGenerator::write_collapsed_stack_file(&stacks, &file_path).is_ok());
        assert!(file_path.exists());

        let content = std::fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("test_func 100"));

        let _ = std::fs::remove_file(&file_path);
    }
}
