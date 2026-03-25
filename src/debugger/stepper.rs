use crate::debugger::instruction_pointer::StepMode;
use crate::debugger::source_map::SourceMap;
use crate::debugger::state::DebugState;
use crate::runtime::instruction::Instruction;

const STEP_GUARD: usize = 100_000;

pub struct Stepper {
    active: bool,
    step_mode: StepMode,
    pause_next: bool,
}

impl Stepper {
    pub fn new() -> Self {
        Self {
            active: false,
            step_mode: StepMode::StepInto,
            pause_next: false,
        }
    }

    pub fn start(&mut self, mode: StepMode, debug_state: &mut DebugState) {
        self.active = true;
        self.step_mode = mode;
        self.pause_next = true;
        debug_state.start_instruction_stepping(mode);
    }

    pub fn stop(&mut self, debug_state: &mut DebugState) {
        self.active = false;
        self.pause_next = false;
        debug_state.stop_instruction_stepping();
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn step_mode(&self) -> StepMode {
        self.step_mode
    }

    pub fn step_into(&mut self, debug_state: &mut DebugState) -> bool {
        if !self.active {
            return false;
        }
        self.step_mode = StepMode::StepInto;
        debug_state.start_instruction_stepping(StepMode::StepInto);
        debug_state.next_instruction().is_some()
    }

    pub fn step_over(&mut self, debug_state: &mut DebugState) -> bool {
        if !self.active {
            return false;
        }
        self.step_mode = StepMode::StepOver;
        debug_state.start_instruction_stepping(StepMode::StepOver);
        self.advance_to_depth(debug_state, false)
    }

    /// Step over to the next distinct source line within the same call frame.
    pub fn step_over_source_line(
        &mut self,
        debug_state: &mut DebugState,
        source_map: &SourceMap,
    ) -> bool {
        if !self.active {
            return false;
        }

        self.step_mode = StepMode::StepOver;
        debug_state.start_instruction_stepping(StepMode::StepOver);

        let start_depth = debug_state.instruction_pointer().call_stack_depth();
        let start_loc = debug_state
            .current_instruction()
            .and_then(|i| source_map.lookup(i.offset));

        for _ in 0..STEP_GUARD {
            if debug_state.next_instruction().is_none() {
                break;
            }
            let depth = debug_state.instruction_pointer().call_stack_depth();
            if depth > start_depth {
                continue;
            }
            let loc = debug_state
                .current_instruction()
                .and_then(|i| source_map.lookup(i.offset));

            let is_different_line = match (&start_loc, &loc) {
                (Some(s), Some(l)) => s.file != l.file || s.line != l.line,
                (None, Some(_)) | (Some(_), None) => true,
                (None, None) => false,
            };

            if is_different_line {
                return true;
            }
        }

        false
    }

    pub fn step_out(&mut self, debug_state: &mut DebugState) -> bool {
        if !self.active {
            return false;
        }
        self.step_mode = StepMode::StepOut;
        debug_state.start_instruction_stepping(StepMode::StepOut);
        self.advance_to_depth(debug_state, true)
    }

    pub fn step_block(&mut self, debug_state: &mut DebugState) -> bool {
        if !self.active {
            return false;
        }
        self.step_mode = StepMode::StepBlock;
        debug_state.start_instruction_stepping(StepMode::StepBlock);
        self.find_next_control_flow(debug_state)
    }

    pub fn step_back(&mut self, debug_state: &mut DebugState) -> bool {
        if !self.active {
            return false;
        }
        debug_state.previous_instruction().is_some()
    }

    pub fn continue_execution(&mut self, debug_state: &mut DebugState) {
        self.active = false;
        debug_state.stop_instruction_stepping();
    }

    pub fn should_pause(&self, instruction: &Instruction, debug_state: &DebugState) -> bool {
        if !self.active {
            return false;
        }
        if self.pause_next {
            return true;
        }

        debug_state
            .instruction_pointer()
            .should_pause_at(instruction)
    }

    pub fn on_instruction(
        &mut self,
        instruction: &Instruction,
        debug_state: &mut DebugState,
    ) -> bool {
        if !self.active {
            return false;
        }
        if self.should_pause(instruction, debug_state) {
            self.pause_next = false;
            return true;
        }
        false
    }

    pub fn reset(&mut self) {
        self.active = false;
        self.pause_next = false;
    }

    fn advance_to_depth(&self, debug_state: &mut DebugState, strictly_lower: bool) -> bool {
        let target = debug_state.instruction_pointer().call_stack_depth();
        for _ in 0..STEP_GUARD {
            if debug_state.next_instruction().is_none() {
                break;
            }
            let depth = debug_state.instruction_pointer().call_stack_depth();
            if strictly_lower && depth < target {
                return true;
            }
            if !strictly_lower && depth <= target {
                return true;
            }
        }
        false
    }
    #[allow(dead_code)]
    /// Find next instruction at lower call depth (step out)
    fn find_next_instruction_at_lower_depth(&self, debug_state: &mut DebugState) -> bool {
        let current_depth = debug_state.instruction_pointer().call_stack_depth();

        // Already at root — cannot step out at the instruction level
        if current_depth == 0 {
            return false;
        }

        let target_depth = current_depth - 1;

        for _ in 0..10_000 {
            if debug_state.next_instruction().is_none() {
                break;
            }
            if debug_state.instruction_pointer().call_stack_depth() <= target_depth {
                return true;
            }
        }
        false
    }

    fn find_next_control_flow(&self, debug_state: &mut DebugState) -> bool {
        for _ in 0..STEP_GUARD {
            match debug_state.next_instruction() {
                Some(inst) if inst.is_control_flow() => return true,
                None => break,
                _ => {}
            }
        }
        false
    }
}

impl Default for Stepper {
    fn default() -> Self {
        Self::new()
    }
}
