use crate::debugger::breakpoint::BreakpointManager;
use crate::debugger::state::DebugState;
use crate::runtime::executor::ContractExecutor;
use crate::Result;
use tracing::info;

/// Core debugging engine that orchestrates execution and debugging
pub struct DebuggerEngine {
    executor: ContractExecutor,
    breakpoints: BreakpointManager,
    state: DebugState,
    paused: bool,
}

impl DebuggerEngine {
    /// Create a new debugger engine
    pub fn new(executor: ContractExecutor, initial_breakpoints: Vec<String>) -> Self {
        let mut breakpoints = BreakpointManager::new();

        // Add initial breakpoints
        for bp in initial_breakpoints {
            breakpoints.add(&bp);
            info!("Breakpoint set at function: {}", bp);
        }

        Self {
            executor,
            breakpoints,
            state: DebugState::new(),
            paused: false,
        }
    }

    /// Execute a contract function with debugging
    pub fn execute(&mut self, function: &str, args: Option<&str>) -> Result<String> {
        info!("Executing function: {}", function);

        // Initialize stack state
        self.state.set_current_function(function.to_string());
        self.state.call_stack_mut().clear();
        self.state.call_stack_mut().push(function.to_string(), None);

        // Check if we should break at function entry
        if self.breakpoints.should_break(function) {
            self.pause_at_function(function);
        }

        // Execute the contract
        let start_time = std::time::Instant::now();
        let result = self.executor.execute(function, args);
        let duration = start_time.elapsed();

        // Update call stack from diagnostic events
        self.update_call_stack(duration)?;

        // If it failed, show the stack
        if let Err(ref e) = result {
            println!("\n[ERROR] Execution failed: {}", e);
            self.state.call_stack().display();
        } else if self.is_paused() {
            // If we paused (only at entry for now), show current stack
            self.state.call_stack().display();
        }

        result
    }

    /// Update the call stack from diagnostic events
    fn update_call_stack(&mut self, total_duration: std::time::Duration) -> Result<()> {
        let events = self.executor.get_diagnostic_events()?;
        let current_func = self.state.current_function().unwrap_or("entry").to_string();
        
        let stack = self.state.call_stack_mut();
        stack.clear();

        // Push the entry function as the root of the stack
        stack.push(current_func, None);

        for event in events {
            // We use the debug string to identify call/return events for now
            // as specific diagnostic event schemas can vary between host versions.
            let event_str = format!("{:?}", event);
            
            // Look for patterns indicating a contract invocation
            if event_str.contains("ContractCall") || (event_str.contains("call") && event.contract_id.is_some()) {
                let contract_id = event.contract_id.as_ref().map(|cid| format!("{:?}", cid));
                // Note: Function name extraction from diagnostic events can be complex;
                // for this tracking phase, we identify cross-contract call boundaries.
                stack.push("nested_call".to_string(), contract_id);
            } else if event_str.contains("ContractReturn") || event_str.contains("return") {
                // Only pop if we are in a nested call (don't pop the entry function)
                if stack.get_stack().len() > 1 {
                    stack.pop();
                }
            }
        }

        // Finalize the entry frame with the measured duration
        if let Some(mut frame) = self.state.call_stack_mut().pop() {
            frame.duration = Some(total_duration);
            self.state.call_stack_mut().push_frame(frame);
        }

        Ok(())
    }

    /// Step through one instruction
    pub fn step(&mut self) -> Result<()> {
        info!("Stepping...");
        self.paused = false;
        // TODO: Implement actual stepping logic
        Ok(())
    }

    /// Continue execution until next breakpoint
    pub fn continue_execution(&mut self) -> Result<()> {
        info!("Continuing execution...");
        self.paused = false;
        // TODO: Implement continue logic
        Ok(())
    }

    /// Pause execution at a function
    fn pause_at_function(&mut self, function: &str) {
        crate::logging::log_breakpoint(function);
        self.paused = true;
        self.state.set_current_function(function.to_string());
        self.state.call_stack().display();
    }

    /// Check if debugger is currently paused
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Get current debug state
    pub fn state(&self) -> &DebugState {
        &self.state
    }

    /// Get mutable reference to breakpoint manager
    pub fn breakpoints_mut(&mut self) -> &mut BreakpointManager {
        &mut self.breakpoints
    }

    /// Get reference to executor
    pub fn executor(&self) -> &ContractExecutor {
        &self.executor
    }
}
