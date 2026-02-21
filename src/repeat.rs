use crate::debugger::engine::DebuggerEngine;
use crate::inspector::budget::{BudgetInfo, BudgetInspector};
use crate::logging;
use crate::runtime::executor::ContractExecutor;
use crate::Result;
use std::time::{Duration, Instant};

/// Stats captured from a single execution run.
#[derive(Debug, Clone)]
pub struct RunStats {
    pub iteration: u32,
    pub duration: Duration,
    pub budget: BudgetInfo,
    pub result: String,
}

/// Aggregate statistics computed over N runs.
#[derive(Debug)]
pub struct AggregateStats {
    pub runs: Vec<RunStats>,
    pub min_duration: Duration,
    pub max_duration: Duration,
    pub avg_duration: Duration,
    pub min_cpu: u64,
    pub max_cpu: u64,
    pub avg_cpu: u64,
    pub min_memory: u64,
    pub max_memory: u64,
    pub avg_memory: u64,
    pub inconsistent_results: bool,
}

impl AggregateStats {
    /// Compute aggregate stats from a list of run results.
    pub fn from_runs(runs: Vec<RunStats>) -> Self {
        assert!(!runs.is_empty(), "Cannot aggregate zero runs");

        let n = runs.len() as u64;

        let mut min_dur = runs[0].duration;
        let mut max_dur = runs[0].duration;
        let mut total_dur = Duration::ZERO;

        let mut min_cpu = runs[0].budget.cpu_instructions;
        let mut max_cpu = runs[0].budget.cpu_instructions;
        let mut total_cpu: u64 = 0;

        let mut min_mem = runs[0].budget.memory_bytes;
        let mut max_mem = runs[0].budget.memory_bytes;
        let mut total_mem: u64 = 0;

        let first_result = &runs[0].result;
        let mut inconsistent = false;

        for run in &runs {
            // Duration
            if run.duration < min_dur {
                min_dur = run.duration;
            }
            if run.duration > max_dur {
                max_dur = run.duration;
            }
            total_dur += run.duration;

            // CPU
            let cpu = run.budget.cpu_instructions;
            if cpu < min_cpu {
                min_cpu = cpu;
            }
            if cpu > max_cpu {
                max_cpu = cpu;
            }
            total_cpu = total_cpu.saturating_add(cpu);

            // Memory
            let mem = run.budget.memory_bytes;
            if mem < min_mem {
                min_mem = mem;
            }
            if mem > max_mem {
                max_mem = mem;
            }
            total_mem = total_mem.saturating_add(mem);

            // Consistency
            if run.result != *first_result {
                inconsistent = true;
            }
        }

        AggregateStats {
            runs,
            min_duration: min_dur,
            max_duration: max_dur,
            avg_duration: total_dur / n as u32,
            min_cpu,
            max_cpu,
            avg_cpu: total_cpu / n,
            min_memory: min_mem,
            max_memory: max_mem,
            avg_memory: total_mem / n,
            inconsistent_results: inconsistent,
        }
    }

    /// Pretty-print the aggregate stats to stdout.
    pub fn display(&self) {
        let n = self.runs.len();

        // Log aggregate statistics with structured fields
        tracing::info!(
            runs = n,
            min_duration_ms = self.min_duration.as_secs_f64() * 1000.0,
            max_duration_ms = self.max_duration.as_secs_f64() * 1000.0,
            avg_duration_ms = self.avg_duration.as_secs_f64() * 1000.0,
            min_cpu = self.min_cpu,
            max_cpu = self.max_cpu,
            avg_cpu = self.avg_cpu,
            min_memory = self.min_memory,
            max_memory = self.max_memory,
            avg_memory = self.avg_memory,
            inconsistent = self.inconsistent_results,
            "Repeat run summary"
        );

        // Log individual inconsistent results if any
        if self.inconsistent_results {
            let first = &self.runs[0].result;
            for run in &self.runs {
                if run.result != *first {
                    tracing::warn!(
                        iteration = run.iteration,
                        result = %run.result,
                        "Inconsistent result detected"
                    );
                }
            }
        }
    }
}

/// Truncate a string to `max_len` characters, adding "…" if truncated.
#[allow(dead_code)]
fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len - 1).collect();
        format!("{}…", truncated)
    }
}

/// Orchestrates repeated contract execution.
pub struct RepeatRunner {
    wasm_bytes: Vec<u8>,
    breakpoints: Vec<String>,
    initial_storage: Option<String>,
}

impl RepeatRunner {
    pub fn new(
        wasm_bytes: Vec<u8>,
        breakpoints: Vec<String>,
        initial_storage: Option<String>,
    ) -> Self {
        Self {
            wasm_bytes,
            breakpoints,
            initial_storage,
        }
    }

    /// Run the contract function `n` times and return aggregate stats.
    pub fn run(&self, function: &str, args: Option<&str>, n: u32) -> Result<AggregateStats> {
        logging::log_repeat_execution(function, n as usize);

        let mut all_runs = Vec::with_capacity(n as usize);

        for i in 1..=n {
            tracing::debug!(
                iteration = i,
                total = n,
                "Starting repeat execution iteration"
            );

            // Fresh executor and engine per run for isolation
            let mut executor = ContractExecutor::new(self.wasm_bytes.clone())?;

            if let Some(ref storage) = self.initial_storage {
                executor.set_initial_storage(storage.clone())?;
            }

            let mut engine = DebuggerEngine::new(executor, self.breakpoints.clone());

            let start = Instant::now();
            let result = engine.execute(function, args)?;
            let duration = start.elapsed();

            let budget = BudgetInspector::get_cpu_usage(engine.executor().host());

            tracing::debug!(
                iteration = i,
                duration_ms = duration.as_secs_f64() * 1000.0,
                cpu = budget.cpu_instructions,
                memory = budget.memory_bytes,
                "Iteration complete"
            );

            all_runs.push(RunStats {
                iteration: i,
                duration,
                budget,
                result,
            });
        }

        let stats = AggregateStats::from_runs(all_runs);
        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_budget(cpu: u64, mem: u64) -> BudgetInfo {
        BudgetInfo {
            cpu_instructions: cpu,
            cpu_limit: 100_000,
            memory_bytes: mem,
            memory_limit: 40_000,
        }
    }

    fn make_run(iter: u32, duration_ms: u64, cpu: u64, mem: u64, result: &str) -> RunStats {
        RunStats {
            iteration: iter,
            duration: Duration::from_millis(duration_ms),
            budget: make_budget(cpu, mem),
            result: result.to_string(),
        }
    }

    #[test]
    fn test_aggregate_single_run() {
        let runs = vec![make_run(1, 100, 5000, 2000, "Ok(())")];
        let stats = AggregateStats::from_runs(runs);

        assert_eq!(stats.min_duration, Duration::from_millis(100));
        assert_eq!(stats.max_duration, Duration::from_millis(100));
        assert_eq!(stats.avg_duration, Duration::from_millis(100));
        assert_eq!(stats.min_cpu, 5000);
        assert_eq!(stats.max_cpu, 5000);
        assert_eq!(stats.avg_cpu, 5000);
        assert_eq!(stats.min_memory, 2000);
        assert_eq!(stats.max_memory, 2000);
        assert_eq!(stats.avg_memory, 2000);
        assert!(!stats.inconsistent_results);
    }

    #[test]
    fn test_aggregate_multiple_runs() {
        let runs = vec![
            make_run(1, 100, 3000, 1000, "Ok(())"),
            make_run(2, 200, 6000, 3000, "Ok(())"),
            make_run(3, 150, 4500, 2000, "Ok(())"),
        ];
        let stats = AggregateStats::from_runs(runs);

        assert_eq!(stats.min_duration, Duration::from_millis(100));
        assert_eq!(stats.max_duration, Duration::from_millis(200));
        assert_eq!(stats.avg_duration, Duration::from_millis(150));
        assert_eq!(stats.min_cpu, 3000);
        assert_eq!(stats.max_cpu, 6000);
        assert_eq!(stats.avg_cpu, 4500);
        assert_eq!(stats.min_memory, 1000);
        assert_eq!(stats.max_memory, 3000);
        assert_eq!(stats.avg_memory, 2000);
        assert!(!stats.inconsistent_results);
    }

    #[test]
    fn test_inconsistent_results_detected() {
        let runs = vec![
            make_run(1, 100, 3000, 1000, "Ok(())"),
            make_run(2, 100, 3000, 1000, "Err(42)"),
        ];
        let stats = AggregateStats::from_runs(runs);

        assert!(stats.inconsistent_results);
    }

    #[test]
    fn test_consistent_results() {
        let runs = vec![
            make_run(1, 100, 3000, 1000, "Ok(())"),
            make_run(2, 200, 4000, 2000, "Ok(())"),
        ];
        let stats = AggregateStats::from_runs(runs);

        assert!(!stats.inconsistent_results);
    }

    #[test]
    fn test_display_does_not_panic() {
        let runs = vec![
            make_run(1, 100, 3000, 1000, "Ok(())"),
            make_run(2, 200, 6000, 3000, "Err(1)"),
        ];
        let stats = AggregateStats::from_runs(runs);
        // Just ensure display() doesn't panic
        stats.display();
    }

    #[test]
    fn test_truncate_short_string() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_long_string() {
        let long = "a]".repeat(20);
        let result = truncate(&long, 10);
        assert_eq!(result.chars().count(), 10);
        assert!(result.ends_with('…'));
    }
}
