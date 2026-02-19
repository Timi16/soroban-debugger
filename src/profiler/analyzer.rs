use crate::runtime::executor::ContractExecutor;
use crate::Result;
use std::collections::HashMap;
use std::fmt;
use std::fmt::Write;

#[derive(Debug, Clone)]
pub struct OperationCost {
    pub operation: String,
    pub cpu_cost: u64,
    pub memory_cost: u64,
    pub location: String,
}

#[derive(Debug, Clone)]
pub struct StorageAccess {
    pub key: String,
    pub access_count: u32,
    pub total_cpu: u64,
    pub total_memory: u64,
    pub locations: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FunctionProfile {
    pub name: String,
    pub total_cpu: u64,
    pub total_memory: u64,
    pub operations: Vec<OperationCost>,
    pub storage_accesses: HashMap<String, StorageAccess>,
}

#[derive(Debug, Clone)]
pub struct OptimizationSuggestion {
    pub category: String,
    pub title: String,
    pub description: String,
    pub estimated_cpu_savings: u64,
    pub estimated_memory_savings: u64,
    pub location: String,
    pub priority: Priority,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Low,
    Medium,
    High,
    Critical,
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Priority::Low => write!(f, "Low"),
            Priority::Medium => write!(f, "Medium"),
            Priority::High => write!(f, "High"),
            Priority::Critical => write!(f, "Critical"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OptimizationReport {
    pub contract_path: String,
    pub functions: Vec<FunctionProfile>,
    pub suggestions: Vec<OptimizationSuggestion>,
    pub total_cpu: u64,
    pub total_memory: u64,
    pub potential_cpu_savings: u64,
    pub potential_memory_savings: u64,
}

pub struct GasOptimizer {
    executor: ContractExecutor,
    function_profiles: HashMap<String, FunctionProfile>,
}

impl GasOptimizer {
    pub fn new(executor: ContractExecutor) -> Self {
        Self {
            executor,
            function_profiles: HashMap::new(),
        }
    }

    pub fn analyze_function(&mut self, function_name: &str, args: Option<&str>) -> Result<FunctionProfile> {
        let host = self.executor.host();
        let budget_start = crate::inspector::budget::BudgetInspector::get_cpu_usage(host);

        let operations = Vec::new();
        let storage_accesses: HashMap<String, StorageAccess> = HashMap::new();

        self.executor.execute(function_name, args)?;

        let budget_end = crate::inspector::budget::BudgetInspector::get_cpu_usage(host);

        let total_cpu = budget_end.cpu_instructions.saturating_sub(budget_start.cpu_instructions);
        let total_memory = budget_end.memory_bytes.saturating_sub(budget_start.memory_bytes);

        let profile = FunctionProfile {
            name: function_name.to_string(),
            total_cpu,
            total_memory,
            operations,
            storage_accesses,
        };

        self.function_profiles.insert(function_name.to_string(), profile.clone());
        Ok(profile)
    }

    pub fn generate_report(&self, contract_path: &str) -> OptimizationReport {
        let functions: Vec<FunctionProfile> = self.function_profiles.values().cloned().collect();
        
        let total_cpu = functions.iter().map(|f| f.total_cpu).sum();
        let total_memory = functions.iter().map(|f| f.total_memory).sum();

        let mut suggestions = Vec::new();

        for function in &functions {
            suggestions.extend(self.analyze_expensive_operations(function));
            suggestions.extend(self.analyze_redundant_storage(function));
            suggestions.extend(self.analyze_type_alternatives(function));
        }

        suggestions.sort_by(|a, b| {
            let priority_order = |p: &Priority| match p {
                Priority::Critical => 0,
                Priority::High => 1,
                Priority::Medium => 2,
                Priority::Low => 3,
            };
            priority_order(&a.priority).cmp(&priority_order(&b.priority))
            .then_with(|| (a.estimated_cpu_savings + a.estimated_memory_savings)
                .cmp(&(b.estimated_cpu_savings + b.estimated_memory_savings))
                .reverse())
        });

        let potential_cpu_savings: u64 = suggestions.iter().map(|s| s.estimated_cpu_savings).sum();
        let potential_memory_savings: u64 = suggestions.iter().map(|s| s.estimated_memory_savings).sum();

        OptimizationReport {
            contract_path: contract_path.to_string(),
            functions,
            suggestions,
            total_cpu,
            total_memory,
            potential_cpu_savings,
            potential_memory_savings,
        }
    }

    fn analyze_expensive_operations(&self, function: &FunctionProfile) -> Vec<OptimizationSuggestion> {
        let mut suggestions = Vec::new();

        if function.total_cpu > 1000000 {
            suggestions.push(OptimizationSuggestion {
                category: "Expensive Operations".to_string(),
                title: format!("High CPU usage in function '{}'", function.name),
                description: format!(
                    "Function uses {} CPU instructions. Consider breaking into smaller functions or optimizing hot paths.",
                    function.total_cpu
                ),
                estimated_cpu_savings: function.total_cpu / 10,
                estimated_memory_savings: 0,
                location: function.name.clone(),
                priority: if function.total_cpu > 5000000 {
                    Priority::Critical
                } else if function.total_cpu > 2000000 {
                    Priority::High
                } else {
                    Priority::Medium
                },
            });
        }

        if function.total_memory > 1000000 {
            suggestions.push(OptimizationSuggestion {
                category: "Memory Usage".to_string(),
                title: format!("High memory usage in function '{}'", function.name),
                description: format!(
                    "Function uses {} bytes of memory. Consider using smaller data structures or releasing unused memory earlier.",
                    function.total_memory
                ),
                estimated_cpu_savings: 0,
                estimated_memory_savings: function.total_memory / 5,
                location: function.name.clone(),
                priority: if function.total_memory > 5000000 {
                    Priority::Critical
                } else if function.total_memory > 2000000 {
                    Priority::High
                } else {
                    Priority::Medium
                },
            });
        }

        suggestions
    }

    fn analyze_redundant_storage(&self, function: &FunctionProfile) -> Vec<OptimizationSuggestion> {
        let mut suggestions = Vec::new();

        for (key, access) in &function.storage_accesses {
            if access.access_count > 2 {
                let savings_per_read = access.total_cpu / access.access_count as u64;
                let potential_savings = savings_per_read * (access.access_count - 1) as u64;

                suggestions.push(OptimizationSuggestion {
                    category: "Redundant Storage Reads".to_string(),
                    title: format!("Cache storage key '{}' in function '{}'", key, function.name),
                    description: format!(
                        "Storage key '{}' is read {} times. Cache the value after first read to save ~{} CPU instructions.",
                        key, access.access_count, potential_savings
                    ),
                    estimated_cpu_savings: potential_savings,
                    estimated_memory_savings: 0,
                    location: format!("{}:{}", function.name, access.locations.first().unwrap_or(&"unknown".to_string())),
                    priority: if access.access_count > 5 {
                        Priority::High
                    } else if access.access_count > 3 {
                        Priority::Medium
                    } else {
                        Priority::Low
                    },
                });
            }
        }

        suggestions
    }

    fn analyze_type_alternatives(&self, function: &FunctionProfile) -> Vec<OptimizationSuggestion> {
        let mut suggestions = Vec::new();

        if function.total_memory > 500000 {
            suggestions.push(OptimizationSuggestion {
                category: "Type Optimization".to_string(),
                title: format!("Consider lighter-weight types in function '{}'", function.name),
                description: "Consider using u32 instead of u64, or Vec<u8> instead of String where possible. Use Symbol for string constants instead of String.".to_string(),
                estimated_cpu_savings: function.total_cpu / 20,
                estimated_memory_savings: function.total_memory / 10,
                location: function.name.clone(),
                priority: Priority::Medium,
            });
        }

        suggestions
    }

    pub fn generate_markdown_report(&self, report: &OptimizationReport) -> String {
        let mut output = String::new();

        writeln!(output, "# Gas Optimization Report").unwrap();
        writeln!(output, "").unwrap();
        writeln!(output, "**Contract:** `{}`", report.contract_path).unwrap();
        writeln!(output, "").unwrap();

        writeln!(output, "## Summary").unwrap();
        writeln!(output, "").unwrap();
        writeln!(output, "- **Total CPU Instructions:** {}", report.total_cpu).unwrap();
        writeln!(output, "- **Total Memory Bytes:** {}", report.total_memory).unwrap();
        writeln!(output, "- **Potential CPU Savings:** {}", report.potential_cpu_savings).unwrap();
        writeln!(output, "- **Potential Memory Savings:** {}", report.potential_memory_savings).unwrap();
        writeln!(output, "").unwrap();

        writeln!(output, "## Function Profiles").unwrap();
        writeln!(output, "").unwrap();
        for function in &report.functions {
            writeln!(output, "### {}", function.name).unwrap();
            writeln!(output, "").unwrap();
            writeln!(output, "- **CPU Instructions:** {}", function.total_cpu).unwrap();
            writeln!(output, "- **Memory Bytes:** {}", function.total_memory).unwrap();
            writeln!(output, "").unwrap();

            if !function.operations.is_empty() {
                writeln!(output, "#### Top 5 Most Expensive Operations").unwrap();
                writeln!(output, "").unwrap();
                writeln!(output, "| Operation | CPU Cost | Memory Cost | Location |").unwrap();
                writeln!(output, "|-----------|----------|-------------|----------|").unwrap();
                
                let mut sorted_ops = function.operations.clone();
                sorted_ops.sort_by(|a, b| (b.cpu_cost + b.memory_cost).cmp(&(a.cpu_cost + a.memory_cost)));
                
                for op in sorted_ops.iter().take(5) {
                    writeln!(output, "| {} | {} | {} | {} |", op.operation, op.cpu_cost, op.memory_cost, op.location).unwrap();
                }
                writeln!(output, "").unwrap();
            }
        }

        writeln!(output, "## Optimization Suggestions").unwrap();
        writeln!(output, "").unwrap();
        
        if report.suggestions.is_empty() {
            writeln!(output, "No optimization suggestions found.").unwrap();
        } else {
            for (idx, suggestion) in report.suggestions.iter().enumerate() {
                writeln!(output, "### {}. {} [{}]", idx + 1, suggestion.title, suggestion.priority).unwrap();
                writeln!(output, "").unwrap();
                writeln!(output, "**Category:** {}", suggestion.category).unwrap();
                writeln!(output, "").unwrap();
                writeln!(output, "{}", suggestion.description).unwrap();
                writeln!(output, "").unwrap();
                writeln!(output, "- **Estimated CPU Savings:** {}", suggestion.estimated_cpu_savings).unwrap();
                writeln!(output, "- **Estimated Memory Savings:** {}", suggestion.estimated_memory_savings).unwrap();
                writeln!(output, "- **Location:** {}", suggestion.location).unwrap();
                writeln!(output, "").unwrap();
            }
        }

        output
    }
}
