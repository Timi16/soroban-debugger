use soroban_env_host::Host;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, serde::Serialize)]
pub struct ExecutionMetrics {
    pub cpu_instructions: u64,
    pub memory_bytes: u64,
    pub wall_time: Duration,
}

/// Call tree node capturing function hierarchies (issue #503).
#[derive(Debug, Clone, serde::Serialize)]
pub struct CallFrame {
    pub function: String,
    pub depth: usize,
    pub cpu_cost: u64,
    pub memory_cost: u64,
    pub children: Vec<CallFrame>,
}

impl CallFrame {
    pub fn new(function: String, depth: usize) -> Self {
        Self {
            function,
            depth,
            cpu_cost: 0,
            memory_cost: 0,
            children: Vec::new(),
        }
    }
}

pub struct ProfileSession {
    cpu_start: u64,
    mem_start: u64,
    start_time: Instant,
    call_stack: Vec<CallFrame>,
}

impl ProfileSession {
    pub fn start(host: &Host) -> Self {
        let budget = crate::inspector::budget::BudgetInspector::get_cpu_usage(host);

        Self {
            cpu_start: budget.cpu_instructions,
            mem_start: budget.memory_bytes,
            start_time: Instant::now(),
            call_stack: Vec::new(),
        }
    }

    pub fn finish(self, host: &Host) -> ExecutionMetrics {
        let budget_end = crate::inspector::budget::BudgetInspector::get_cpu_usage(host);

        ExecutionMetrics {
            cpu_instructions: budget_end.cpu_instructions.saturating_sub(self.cpu_start),
            memory_bytes: budget_end.memory_bytes.saturating_sub(self.mem_start),
            wall_time: self.start_time.elapsed(),
        }
    }

    pub fn enter_call(&mut self, function_name: String) {
        let depth = self.call_stack.len();
        self.call_stack.push(CallFrame::new(function_name, depth));
    }

    pub fn exit_call(&mut self) {
        self.call_stack.pop();
    }

    pub fn get_call_tree(&self) -> Vec<CallFrame> {
        self.call_stack.clone()
    }
}
