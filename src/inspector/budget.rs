use serde::{Deserialize, Serialize};
use soroban_env_host::Host;
use std::collections::VecDeque;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryAllocation {
    pub size: u64,
    pub location: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryTracker {
    allocations: VecDeque<MemoryAllocation>,
    peak_memory: u64,
    initial_memory: u64,
    allocation_count: u64,
    total_allocated_bytes: u64,
}

impl MemoryTracker {
    pub fn new(initial_memory: u64) -> Self {
        Self {
            allocations: VecDeque::new(),
            peak_memory: initial_memory,
            initial_memory,
            allocation_count: 0,
            total_allocated_bytes: 0,
        }
    }

    pub fn record_snapshot(&mut self, host: &Host, location: &str) {
        let budget = host.budget_cloned();
        let current_memory = budget.get_mem_bytes_consumed().unwrap_or(0);

        if current_memory > self.peak_memory {
            self.peak_memory = current_memory;
        }

        if let Some(_last_allocation) = self.allocations.back() {
            let last_total = self.initial_memory + self.total_allocated_bytes;
            if current_memory > last_total {
                let delta = current_memory - last_total;
                let allocation = MemoryAllocation {
                    size: delta,
                    location: location.to_string(),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                };

                if self.allocations.len() >= 100 {
                    self.allocations.pop_front();
                }
                self.allocations.push_back(allocation);
                self.allocation_count += 1;
                self.total_allocated_bytes = self.total_allocated_bytes.saturating_add(delta);
            }
        } else {
            let memory_delta = current_memory.saturating_sub(self.initial_memory);
            if memory_delta > 0 {
                let allocation = MemoryAllocation {
                    size: memory_delta,
                    location: location.to_string(),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                };

                self.allocations.push_back(allocation);
                self.allocation_count += 1;
                self.total_allocated_bytes = memory_delta;
            }
        }
    }

    pub fn record_memory_change(
        &mut self,
        previous_memory: u64,
        current_memory: u64,
        location: &str,
    ) {
        if current_memory > self.peak_memory {
            self.peak_memory = current_memory;
        }

        if current_memory > previous_memory {
            let delta = current_memory - previous_memory;
            let allocation = MemoryAllocation {
                size: delta,
                location: location.to_string(),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            };

            if self.allocations.len() >= 100 {
                self.allocations.pop_front();
            }
            self.allocations.push_back(allocation);
            self.allocation_count += 1;
            self.total_allocated_bytes = self.total_allocated_bytes.saturating_add(delta);
        }
    }

    pub fn get_top_allocations(&self, count: usize) -> Vec<MemoryAllocation> {
        let mut sorted: Vec<MemoryAllocation> = self.allocations.iter().cloned().collect();
        sorted.sort_by(|a, b| b.size.cmp(&a.size));
        sorted.into_iter().take(count).collect()
    }

    pub fn peak_memory(&self) -> u64 {
        self.peak_memory
    }

    pub fn allocation_count(&self) -> u64 {
        self.allocation_count
    }

    pub fn total_allocated_bytes(&self) -> u64 {
        self.total_allocated_bytes
    }

    pub fn finalize(&mut self, host: &Host) -> MemorySummary {
        let budget = host.budget_cloned();
        let final_memory = budget.get_mem_bytes_consumed().unwrap_or(0);

        if final_memory > self.peak_memory {
            self.peak_memory = final_memory;
        }

        MemorySummary {
            peak_memory: self.peak_memory,
            allocation_count: self.allocation_count,
            total_allocated_bytes: self.total_allocated_bytes,
            final_memory,
            initial_memory: self.initial_memory,
            top_allocations: self.get_top_allocations(5),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySummary {
    pub peak_memory: u64,
    pub allocation_count: u64,
    pub total_allocated_bytes: u64,
    pub final_memory: u64,
    pub initial_memory: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub top_allocations: Vec<MemoryAllocation>,
}

impl MemorySummary {
    pub fn display(&self) {
        println!("\n=== Memory Allocation Summary ===");
        println!("Peak Memory Usage: {} bytes", self.peak_memory);
        println!("Allocation Count: {}", self.allocation_count);
        println!(
            "Total Allocated Bytes: {} bytes",
            self.total_allocated_bytes
        );
        println!("Initial Memory: {} bytes", self.initial_memory);
        println!("Final Memory: {} bytes", self.final_memory);
        println!(
            "Memory Delta: {} bytes",
            self.final_memory.saturating_sub(self.initial_memory)
        );

        if !self.top_allocations.is_empty() {
            println!("\nTop 5 Largest Allocations:");
            for (idx, alloc) in self.top_allocations.iter().enumerate() {
                println!("  {}. {} bytes at {}", idx + 1, alloc.size, alloc.location);
            }
        }
        println!();
    }

    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}
