use crate::debugger::breakpoint::BreakpointManager;
use crate::debugger::instruction_pointer::StepMode;
use crate::debugger::state::DebugState;
use crate::debugger::stepper::Stepper;
use crate::runtime::executor::ContractExecutor;
use crate::runtime::instruction::Instruction;
use crate::runtime::instrumentation::Instrumenter;
use crate::Result;
use std::sync::{Arc, Mutex};
use tracing::info;

/// Core debugging engine that orchestrates execution and debugging.
pub struct DebuggerEngine {
    executor: ContractExecutor,
    breakpoints: BreakpointManager,
    state: Arc<Mutex<DebugState>>,
    stepper: Stepper,
    instrumenter: Instrumenter,
    paused: bool,
    instruction_debug_enabled: bool,
}

impl DebuggerEngine {
    /// Create a new debugger engine.
    pub fn new(executor: ContractExecutor, initial_breakpoints: Vec<String>) -> Self {
        let mut breakpoints = BreakpointManager::new();

        for bp in initial_breakpoints {
            breakpoints.add(&bp);
            info!("Breakpoint set at function: {}", bp);
        }

        Self {
            executor,
            breakpoints,
            state: Arc::new(Mutex::new(DebugState::new())),
            stepper: Stepper::new(),
            instrumenter: Instrumenter::new(),
            paused: false,
            instruction_debug_enabled: false,
        }
    }

    /// Enable instruction-level debugging.
    pub fn enable_instruction_debug(&mut self, wasm_bytes: &[u8]) -> Result<()> {
        let instructions = self
            .instrumenter
            .parse_instructions(wasm_bytes)
            .map_err(|e| anyhow::anyhow!("Failed to parse instructions: {}", e))?
            .to_vec();

        if let Ok(mut state) = self.state.lock() {
            state.set_instructions(instructions);
            state.enable_instruction_debug();
        }

        self.instrumenter.enable();
        self.instruction_debug_enabled = true;
        Ok(())
    }

    /// Disable instruction-level debugging.
    pub fn disable_instruction_debug(&mut self) {
        self.instrumenter.disable();
        self.instrumenter.remove_hook();
        if let Ok(mut state) = self.state.lock() {
            state.disable_instruction_debug();
        }
        self.instruction_debug_enabled = false;
    }

    /// Check if instruction-level debugging is enabled.
    pub fn is_instruction_debug_enabled(&self) -> bool {
        self.instruction_debug_enabled
    }

    /// Execute a contract function with debugging.
    pub fn execute(&mut self, function: &str, args: Option<&str>) -> Result<String> {
        info!("Executing function: {}", function);

        if let Ok(mut state) = self.state.lock() {
            state.set_current_function(function.to_string());
            state.call_stack_mut().clear();
            state.call_stack_mut().push(function.to_string(), None);
        }

        if self.breakpoints.should_break(function) {
            self.pause_at_function(function);
        }

        let start_time = std::time::Instant::now();
        let result = self.executor.execute(function, args);
        let duration = start_time.elapsed();

        self.update_call_stack(duration)?;

        if let Err(ref e) = result {
            println!("\n[ERROR] Execution failed: {}", e);
            if let Ok(state) = self.state.lock() {
                state.call_stack().display();
            }
        } else if self.is_paused() {
            if let Ok(state) = self.state.lock() {
                state.call_stack().display();
            }
        }

        result
    }

    fn update_call_stack(&mut self, total_duration: std::time::Duration) -> Result<()> {
        let events = self.executor.get_diagnostic_events()?;

        let current_func = if let Ok(state) = self.state.lock() {
            state.current_function().unwrap_or("entry").to_string()
        } else {
            "entry".to_string()
        };

        if let Ok(mut state) = self.state.lock() {
            let stack = state.call_stack_mut();
            stack.clear();
            stack.push(current_func, None);

            for event in events {
                let event_str = format!("{:?}", event);
                if event_str.contains("ContractCall")
                    || (event_str.contains("call") && event.contract_id.is_some())
                {
                    let contract_id = event.contract_id.as_ref().map(|cid| format!("{:?}", cid));
                    stack.push("nested_call".to_string(), contract_id);
                } else if (event_str.contains("ContractReturn") || event_str.contains("return"))
                    && stack.get_stack().len() > 1
                {
                    stack.pop();
                }
            }

            if let Some(mut frame) = stack.pop() {
                frame.duration = Some(total_duration);
                stack.push_frame(frame);
            }
        }

        Ok(())
    }

    /// Step into next instruction.
    pub fn step_into(&mut self) -> Result<bool> {
        if !self.instruction_debug_enabled {
            return Err(anyhow::anyhow!("Instruction debugging not enabled"));
        }

        let stepped = if let Ok(mut state) = self.state.lock() {
            self.stepper.step_into(&mut state)
        } else {
            false
        };
        self.paused = stepped;
        Ok(stepped)
    }

    /// Step over function calls.
    pub fn step_over(&mut self) -> Result<bool> {
        if !self.instruction_debug_enabled {
            return Err(anyhow::anyhow!("Instruction debugging not enabled"));
        }

        let stepped = if let Ok(mut state) = self.state.lock() {
            self.stepper.step_over(&mut state)
        } else {
            false
        };
        self.paused = stepped;
        Ok(stepped)
    }

    /// Step out of current function.
    pub fn step_out(&mut self) -> Result<bool> {
        if !self.instruction_debug_enabled {
            return Err(anyhow::anyhow!("Instruction debugging not enabled"));
        }

        let stepped = if let Ok(mut state) = self.state.lock() {
            self.stepper.step_out(&mut state)
        } else {
            false
        };
        self.paused = stepped;
        Ok(stepped)
    }

    /// Step to next basic block.
    pub fn step_block(&mut self) -> Result<bool> {
        if !self.instruction_debug_enabled {
            return Err(anyhow::anyhow!("Instruction debugging not enabled"));
        }

        let stepped = if let Ok(mut state) = self.state.lock() {
            self.stepper.step_block(&mut state)
        } else {
            false
        };
        self.paused = stepped;
        Ok(stepped)
    }

    /// Step backwards to previous instruction.
    pub fn step_back(&mut self) -> Result<bool> {
        if !self.instruction_debug_enabled {
            return Err(anyhow::anyhow!("Instruction debugging not enabled"));
        }

        let stepped = if let Ok(mut state) = self.state.lock() {
            self.stepper.step_back(&mut state)
        } else {
            false
        };
        self.paused = stepped;
        Ok(stepped)
    }

    /// Start instruction stepping with given mode.
    pub fn start_instruction_stepping(&mut self, mode: StepMode) -> Result<()> {
        if !self.instruction_debug_enabled {
            return Err(anyhow::anyhow!("Instruction debugging not enabled"));
        }

        if let Ok(mut state) = self.state.lock() {
            self.stepper.start(mode, &mut state);
            self.paused = true;
        }

        Ok(())
    }

    /// Continue execution until next breakpoint.
    pub fn continue_execution(&mut self) -> Result<()> {
        self.paused = false;
        if let Ok(mut state) = self.state.lock() {
            self.stepper.continue_execution(&mut state);
        }
        Ok(())
    }

    fn pause_at_function(&mut self, function: &str) {
        crate::logging::log_breakpoint(function);
        self.paused = true;

        if let Ok(mut state) = self.state.lock() {
            state.set_current_function(function.to_string());
            state.call_stack().display();
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn state(&self) -> Arc<Mutex<DebugState>> {
        Arc::clone(&self.state)
    }

    pub fn current_instruction(&self) -> Option<Instruction> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.current_instruction().cloned())
    }

    pub fn get_instruction_context(&self, context_size: usize) -> Vec<(usize, Instruction, bool)> {
        if let Ok(state) = self.state.lock() {
            state
                .get_instruction_context(context_size)
                .into_iter()
                .map(|(idx, inst, current)| (idx, inst.clone(), current))
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn breakpoints_mut(&mut self) -> &mut BreakpointManager {
        &mut self.breakpoints
    }

    pub fn executor(&self) -> &ContractExecutor {
        &self.executor
    }

    pub fn executor_mut(&mut self) -> &mut ContractExecutor {
        &mut self.executor
    }

    /// Compatibility method for non-instruction stepping.
    pub fn step(&mut self) -> Result<()> {
        if self.instruction_debug_enabled {
            let _ = self.step_into()?;
        }
        if let Ok(mut state) = self.state.lock() {
            state.increment_step();
        }
        Ok(())
    }
}
