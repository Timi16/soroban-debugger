use crate::debugger::instruction_pointer::StepMode;
use crate::debugger::state::DebugState;
use crate::runtime::instruction::Instruction;

/// Handles step-through execution of contracts at instruction level
pub struct Stepper {
    /// Whether stepper is active
    active: bool,
    /// Current step mode
    step_mode: StepMode,
    /// Whether to pause at next instruction
    pause_next: bool,
}

impl Stepper {
    /// Create a new stepper
    pub fn new() -> Self {
        Self {
            active: false,
            step_mode: StepMode::StepInto,
            pause_next: false,
        }
    }

    /// Start stepping with the given mode
    pub fn start(&mut self, mode: StepMode, debug_state: &mut DebugState) {
        self.active = true;
        self.step_mode = mode;
        self.pause_next = true;
        debug_state.start_instruction_stepping(mode);
    }

    /// Stop stepping
    pub fn stop(&mut self, debug_state: &mut DebugState) {
        self.active = false;
        self.pause_next = false;
        debug_state.stop_instruction_stepping();
    }

    /// Check if stepper is active
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Get current step mode
    pub fn step_mode(&self) -> StepMode {
        self.step_mode
    }

    /// Step into next instruction
    pub fn step_into(&mut self, debug_state: &mut DebugState) -> bool {
        if !self.active {
            return false;
        }

        self.step_mode = StepMode::StepInto;
        debug_state.start_instruction_stepping(StepMode::StepInto);
        debug_state.next_instruction().is_some()
    }

    /// Step over function call (don't step into calls)
    pub fn step_over(&mut self, debug_state: &mut DebugState) -> bool {
        if !self.active {
            return false;
        }

        self.step_mode = StepMode::StepOver;
        debug_state.start_instruction_stepping(StepMode::StepOver);

        // Find next instruction at same or lower call depth
        self.find_next_instruction_at_depth(debug_state)
    }

    /// Step out of current function
    pub fn step_out(&mut self, debug_state: &mut DebugState) -> bool {
        if !self.active {
            return false;
        }

        self.step_mode = StepMode::StepOut;
        debug_state.start_instruction_stepping(StepMode::StepOut);

        // Find next instruction at lower call depth
        self.find_next_instruction_at_lower_depth(debug_state)
    }

    /// Step to next basic block
    pub fn step_block(&mut self, debug_state: &mut DebugState) -> bool {
        if !self.active {
            return false;
        }

        self.step_mode = StepMode::StepBlock;
        debug_state.start_instruction_stepping(StepMode::StepBlock);

        // Find next control flow instruction
        self.find_next_control_flow_instruction(debug_state)
    }

    /// Step backwards to previous instruction
    pub fn step_back(&mut self, debug_state: &mut DebugState) -> bool {
        if !self.active {
            return false;
        }

        debug_state.previous_instruction().is_some()
    }

    /// Continue execution until next breakpoint or pause condition
    pub fn continue_execution(&mut self, debug_state: &mut DebugState) {
        self.active = false;
        debug_state.stop_instruction_stepping();
    }

    /// Check if execution should pause at the given instruction
    pub fn should_pause(&self, instruction: &Instruction, debug_state: &DebugState) -> bool {
        if !self.active {
            return false;
        }

        // Always pause if explicitly requested
        if self.pause_next {
            return true;
        }

        // Check step mode specific conditions
        match self.step_mode {
            StepMode::StepInto => true,
            StepMode::StepOver => {
                // Pause if we're at same or lower call depth
                let target_depth = debug_state.instruction_pointer().call_stack_depth();
                debug_state.instruction_pointer().call_stack_depth() <= target_depth
            }
            StepMode::StepOut => {
                // Pause if we've stepped out of the function
                let target_depth = debug_state.instruction_pointer().call_stack_depth();
                debug_state.instruction_pointer().call_stack_depth() < target_depth
            }
            StepMode::StepBlock => {
                // Pause at control flow instructions
                instruction.is_control_flow()
            }
        }
    }

    /// Handle instruction execution
    pub fn on_instruction(
        &mut self,
        instruction: &Instruction,
        debug_state: &mut DebugState,
    ) -> bool {
        if !self.active {
            return false; // Continue execution
        }

        let should_pause = self.should_pause(instruction, debug_state);

        if should_pause {
            self.pause_next = false;
            return true; // Pause execution
        }

        false // Continue execution
    }

    /// Find next instruction at same or lower call depth
    fn find_next_instruction_at_depth(&self, debug_state: &mut DebugState) -> bool {
        let target_depth = debug_state.instruction_pointer().call_stack_depth();

        // Simulate stepping through instructions to find the right depth
        for _ in 0..1000 {
            // Prevent infinite loop
            if debug_state.next_instruction().is_none() {
                break;
            }
            if debug_state.instruction_pointer().call_stack_depth() <= target_depth {
                return true;
            }
        }

        false
    }

    /// Find next instruction at lower call depth (step out)
    fn find_next_instruction_at_lower_depth(&self, debug_state: &mut DebugState) -> bool {
        let target_depth = debug_state.instruction_pointer().call_stack_depth();

        for _ in 0..1000 {
            // Prevent infinite loop
            if debug_state.next_instruction().is_none() {
                break;
            }
            if debug_state.instruction_pointer().call_stack_depth() < target_depth {
                return true;
            }
        }

        false
    }

    /// Find next control flow instruction
    fn find_next_control_flow_instruction(&self, debug_state: &mut DebugState) -> bool {
        for _ in 0..1000 {
            // Prevent infinite loop
            if let Some(inst) = debug_state.next_instruction() {
                if inst.is_control_flow() {
                    return true;
                }
            } else {
                break;
            }
        }

        false
    }

    /// Reset stepper state
    pub fn reset(&mut self) {
        self.active = false;
        self.pause_next = false;
    }
}

impl Default for Stepper {
    fn default() -> Self {
        Self::new()
    }
}
