use soroban_env_host::Host;

/// Tracks resource usage (CPU and memory budget)
pub struct BudgetInspector;

impl BudgetInspector {
    /// Get CPU instruction usage from host
    pub fn get_cpu_usage(host: &Host) -> BudgetInfo {
        let budget = host.budget_cloned();
        let cpu_consumed = budget.get_cpu_insns_consumed().unwrap_or(0);
        let cpu_remaining = budget.get_cpu_insns_remaining().unwrap_or(0);
        let mem_consumed = budget.get_mem_bytes_consumed().unwrap_or(0);
        let mem_remaining = budget.get_mem_bytes_remaining().unwrap_or(0);

        BudgetInfo {
            cpu_instructions: cpu_consumed,
            cpu_limit: cpu_consumed.saturating_add(cpu_remaining),
            memory_bytes: mem_consumed,
            memory_limit: mem_consumed.saturating_add(mem_remaining),
        }
    }

    /// Display budget information
    pub fn display(host: &Host) {
        let info = Self::get_cpu_usage(host);

        let cpu_percent = info.cpu_percentage();
        let mem_percent = info.memory_percentage();

        tracing::info!(
            cpu_insns = info.cpu_instructions,
            cpu_limit = info.cpu_limit,
            cpu_percent = cpu_percent,
            memory_bytes = info.memory_bytes,
            memory_limit = info.memory_limit,
            memory_percent = mem_percent,
            "Resource budget"
        );

        // Warn if approaching limits
        if cpu_percent > 80.0 {
            crate::logging::log_high_resource_usage("CPU", cpu_percent);
        }
        if mem_percent > 80.0 {
            crate::logging::log_high_resource_usage("memory", mem_percent);
        }
    }
}

/// Budget information snapshot
#[derive(Debug, Clone)]
pub struct BudgetInfo {
    pub cpu_instructions: u64,
    pub cpu_limit: u64,
    pub memory_bytes: u64,
    pub memory_limit: u64,
}

impl BudgetInfo {
    /// Calculate CPU usage percentage
    pub fn cpu_percentage(&self) -> f64 {
        if self.cpu_limit == 0 {
            0.0
        } else {
            (self.cpu_instructions as f64 / self.cpu_limit as f64) * 100.0
        }
    }

    /// Calculate memory usage percentage
    pub fn memory_percentage(&self) -> f64 {
        if self.memory_limit == 0 {
            0.0
        } else {
            (self.memory_bytes as f64 / self.memory_limit as f64) * 100.0
        }
    }
}
