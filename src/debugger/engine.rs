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

    /// Execute a contract function with debugging and storage tracking
    pub fn execute(&mut self, function: &str, args: Option<&str>) -> Result<String> {
        info!("Executing function: {}", function);

        // Capture storage state before execution
        let storage_before = self.executor.get_storage_snapshot()?;

        // Check if we should break at function entry
        if self.breakpoints.should_break(function) {
            self.pause_at_function(function);
        }

        // Execute the contract
        let result = self.executor.execute(function, args)?;

        // Capture storage state after execution
        let storage_after = self.executor.get_storage_snapshot()?;

        // Calculate and display storage diff if requested via some flag
        // or just store it in state for the CLI to use.
        let diff = crate::inspector::StorageInspector::compute_diff(&storage_before, &storage_after);
        if !diff.is_empty() {
             crate::inspector::StorageInspector::display_diff(&diff);
        }

        info!("Execution completed");
        Ok(result)
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
        println!("\n[BREAKPOINT] Paused at function: {}", function);
        self.paused = true;
        self.state.set_current_function(function.to_string());
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
