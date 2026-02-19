/// Pretty printing utilities for debugger output
pub struct Formatter;

impl Formatter {
    /// Format a value for display
    pub fn format_value(value: &str) -> String {
        // TODO: Add better formatting for different types
        value.to_string()
    }

    /// Format storage key-value pair
    pub fn format_storage_entry(key: &str, value: &str) -> String {
        format!("{} = {}", key, value)
    }

    /// Format a function call
    pub fn format_function_call(name: &str, args: Option<&str>) -> String {
        if let Some(args) = args {
            format!("{}({})", name, args)
        } else {
            format!("{}()", name)
        }
    }

    /// Format budget information
    pub fn format_budget(cpu: u64, cpu_limit: u64, mem: u64, mem_limit: u64) -> String {
        format!(
            "CPU: {}/{} ({:.1}%) | Memory: {}/{} bytes ({:.1}%)",
            cpu,
            cpu_limit,
            (cpu as f64 / cpu_limit as f64) * 100.0,
            mem,
            mem_limit,
            (mem as f64 / mem_limit as f64) * 100.0
        )
    }
}
