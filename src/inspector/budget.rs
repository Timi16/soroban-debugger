use crossterm::style::{Color, Stylize};
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

    /// Display budget information with warnings
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

        let warnings = Self::check_thresholds(&info);
        for warning in warnings {
            let color = match warning.severity {
                Severity::Yellow => Color::Yellow,
                Severity::Red => Color::Red,
                Severity::Critical => Color::DarkRed,
            };

            let prefix = match warning.severity {
                Severity::Yellow => "[WARNING]",
                Severity::Red => "[ALERT]",
                Severity::Critical => "[CRITICAL]",
            };

            crate::logging::log_display(
                format!(
                    "  {} {} usage at {:.1}%",
                    prefix.with(color),
                    warning.resource,
                    warning.percentage
                ),
                crate::logging::LogLevel::Warn,
            );

            if let Some(suggestion) = warning.suggestion {
                crate::logging::log_display(
                    format!("    Suggestion: {}", suggestion.italic()),
                    crate::logging::LogLevel::Warn,
                );
            }
        }
    }

    /// Check if usage exceeds defined thresholds
    pub fn check_thresholds(info: &BudgetInfo) -> Vec<BudgetWarning> {
        let mut warnings = Vec::new();

        // Check CPU
        let cpu_pct = info.cpu_percentage();
        if let Some(warning) = Self::create_warning("CPU", cpu_pct) {
            warnings.push(warning);
        }

        // Check Memory
        let mem_pct = info.memory_percentage();
        if let Some(warning) = Self::create_warning("Memory", mem_pct) {
            warnings.push(warning);
        }

        // Warn if approaching limits
        if cpu_pct > 80.0 {
            crate::logging::log_high_resource_usage("CPU", cpu_pct);
        }
        if mem_pct > 80.0 {
            crate::logging::log_high_resource_usage("memory", mem_pct);
        }

        warnings
    }

    fn create_warning(resource: &str, percentage: f64) -> Option<BudgetWarning> {
        if percentage >= 90.0 {
            Some(BudgetWarning {
                resource: resource.to_string(),
                percentage,
                severity: Severity::Critical,
                suggestion: Some(format!(
                    "High {} usage detected. Consider optimizing contract logic or reducing data complexity.",
                    resource
                )),
            })
        } else if percentage >= 85.0 {
            Some(BudgetWarning {
                resource: resource.to_string(),
                percentage,
                severity: Severity::Red,
                suggestion: None,
            })
        } else if percentage >= 70.0 {
            Some(BudgetWarning {
                resource: resource.to_string(),
                percentage,
                severity: Severity::Yellow,
                suggestion: None,
            })
        } else {
            None
        }
    }
}

/// Severity level for budget warnings
pub enum Severity {
    Yellow,
    Red,
    Critical,
}

/// Represents a warning about resource usage
pub struct BudgetWarning {
    pub resource: String,
    pub percentage: f64,
    pub severity: Severity,
    pub suggestion: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_budget_percentage_calculation() {
        let info = BudgetInfo {
            cpu_instructions: 50,
            cpu_limit: 100,
            memory_bytes: 25,
            memory_limit: 100,
        };
        assert_eq!(info.cpu_percentage(), 50.0);
        assert_eq!(info.memory_percentage(), 25.0);
    }

    #[test]
    fn test_check_thresholds_none() {
        let info = BudgetInfo {
            cpu_instructions: 50,
            cpu_limit: 100,
            memory_bytes: 50,
            memory_limit: 100,
        };
        let warnings = BudgetInspector::check_thresholds(&info);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_check_thresholds_yellow() {
        let info = BudgetInfo {
            cpu_instructions: 75,
            cpu_limit: 100,
            memory_bytes: 50,
            memory_limit: 100,
        };
        let warnings = BudgetInspector::check_thresholds(&info);
        assert_eq!(warnings.len(), 1);
        assert!(matches!(warnings[0].severity, Severity::Yellow));
    }

    #[test]
    fn test_check_thresholds_red() {
        let info = BudgetInfo {
            cpu_instructions: 86,
            cpu_limit: 100,
            memory_bytes: 50,
            memory_limit: 100,
        };
        let warnings = BudgetInspector::check_thresholds(&info);
        assert_eq!(warnings.len(), 1);
        assert!(matches!(warnings[0].severity, Severity::Red));
    }

    #[test]
    fn test_check_thresholds_critical() {
        let info = BudgetInfo {
            cpu_instructions: 91,
            cpu_limit: 100,
            memory_bytes: 50,
            memory_limit: 100,
        };
        let warnings = BudgetInspector::check_thresholds(&info);
        assert_eq!(warnings.len(), 1);
        assert!(matches!(warnings[0].severity, Severity::Critical));
        assert!(warnings[0].suggestion.is_some());
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
        crate::logging::log_display(
            "\n=== Memory Allocation Summary ===",
            crate::logging::LogLevel::Info,
        );
        crate::logging::log_display(
            format!("Peak Memory Usage: {} bytes", self.peak_memory),
            crate::logging::LogLevel::Info,
        );
        crate::logging::log_display(
            format!("Allocation Count: {}", self.allocation_count),
            crate::logging::LogLevel::Info,
        );
        crate::logging::log_display(
            format!(
                "Total Allocated Bytes: {} bytes",
                self.total_allocated_bytes
            ),
            crate::logging::LogLevel::Info,
        );
        crate::logging::log_display(
            format!("Initial Memory: {} bytes", self.initial_memory),
            crate::logging::LogLevel::Info,
        );
        crate::logging::log_display(
            format!("Final Memory: {} bytes", self.final_memory),
            crate::logging::LogLevel::Info,
        );
        crate::logging::log_display(
            format!(
                "Memory Delta: {} bytes",
                self.final_memory.saturating_sub(self.initial_memory)
            ),
            crate::logging::LogLevel::Info,
        );

        if !self.top_allocations.is_empty() {
            crate::logging::log_display(
                "\nTop 5 Largest Allocations:",
                crate::logging::LogLevel::Info,
            );
            for (idx, alloc) in self.top_allocations.iter().enumerate() {
                crate::logging::log_display(
                    format!("  {}. {} bytes at {}", idx + 1, alloc.size, alloc.location),
                    crate::logging::LogLevel::Info,
                );
            }
        }
        crate::logging::log_display("", crate::logging::LogLevel::Info);
    }

    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod memory_tests {
    use super::*;

    #[test]
    fn memory_tracker_tracks_count_peak_and_total() {
        let mut tracker = MemoryTracker::new(100);

        tracker.record_memory_change(100, 180, "alloc:a");
        tracker.record_memory_change(180, 240, "alloc:b");
        tracker.record_memory_change(240, 220, "free-ish");
        tracker.record_memory_change(220, 260, "alloc:c");

        assert_eq!(tracker.allocation_count(), 3);
        assert_eq!(tracker.total_allocated_bytes(), 80 + 60 + 40);
        assert_eq!(tracker.peak_memory(), 260);
    }

    #[test]
    fn memory_tracker_returns_top_five_largest_allocations_sorted() {
        let mut tracker = MemoryTracker::new(0);
        let mut current = 0;
        let sizes = [10_u64, 80, 30, 50, 20, 70, 40];

        for (idx, size) in sizes.iter().enumerate() {
            let previous = current;
            current += size;
            tracker.record_memory_change(previous, current, &format!("alloc:{idx}"));
        }

        let top = tracker.get_top_allocations(5);
        let top_sizes: Vec<u64> = top.into_iter().map(|a| a.size).collect();
        assert_eq!(top_sizes, vec![80, 70, 50, 40, 30]);
    }
}
