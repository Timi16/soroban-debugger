use crate::debugger::instruction_pointer::{InstructionPointer, StepMode};
use crate::inspector::stack::CallStackInspector;
use crate::runtime::instruction::Instruction;

/// Represents the current state of the debugger.
#[derive(Debug, Clone)]
pub struct DebugState {
    current_function: Option<String>,
    current_args: Option<String>,
    step_count: usize,
    instruction_pointer: InstructionPointer,
    current_instruction: Option<Instruction>,
    instructions: Vec<Instruction>,
    instruction_debug_enabled: bool,
    call_stack: CallStackInspector,
}

impl DebugState {
    /// Create a new debug state.
    pub fn new() -> Self {
        Self {
            current_function: None,
            current_args: None,
            step_count: 0,
            instruction_pointer: InstructionPointer::new(),
            current_instruction: None,
            instructions: Vec::new(),
            instruction_debug_enabled: false,
            call_stack: CallStackInspector::new(),
        }
    }

    /// Set the current function being executed
    pub fn set_current_function(&mut self, function: String, args: Option<String>) {
        self.current_function = Some(function);
        self.current_args = args;
    }

    pub fn current_function(&self) -> Option<&str> {
        self.current_function.as_deref()
    }

    /// Get current function arguments
    pub fn current_args(&self) -> Option<&str> {
        self.current_args.as_deref()
    }

    /// Increment step count
    pub fn increment_step(&mut self) {
        self.step_count += 1;
    }

    pub fn step_count(&self) -> usize {
        self.step_count
    }

    pub fn set_instructions(&mut self, instructions: Vec<Instruction>) {
        self.instructions = instructions;
        self.current_instruction = self.instructions.first().cloned();
        self.instruction_pointer.reset();
    }

    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }

    pub fn current_instruction(&self) -> Option<&Instruction> {
        self.current_instruction.as_ref()
    }

    pub fn instruction_pointer(&self) -> &InstructionPointer {
        &self.instruction_pointer
    }

    pub fn instruction_pointer_mut(&mut self) -> &mut InstructionPointer {
        &mut self.instruction_pointer
    }

    pub fn enable_instruction_debug(&mut self) {
        self.instruction_debug_enabled = true;
    }

    pub fn disable_instruction_debug(&mut self) {
        self.instruction_debug_enabled = false;
        self.instruction_pointer.stop_stepping();
    }

    pub fn is_instruction_debug_enabled(&self) -> bool {
        self.instruction_debug_enabled
    }

    pub fn start_instruction_stepping(&mut self, mode: StepMode) {
        if self.instruction_debug_enabled {
            self.instruction_pointer.start_stepping(mode);
        }
    }

    pub fn stop_instruction_stepping(&mut self) {
        self.instruction_pointer.stop_stepping();
    }

    pub fn advance_to_instruction(&mut self, index: usize) -> Option<&Instruction> {
        if index >= self.instructions.len() {
            return None;
        }

        self.instruction_pointer.advance_to(index);
        self.current_instruction = self.instructions.get(index).cloned();

        if let Some(inst) = &self.current_instruction {
            self.instruction_pointer.update_call_stack(inst);
        }

        self.current_instruction.as_ref()
    }

    pub fn next_instruction(&mut self) -> Option<&Instruction> {
        let next_index = self.instruction_pointer.current_index().saturating_add(1);
        self.advance_to_instruction(next_index)
    }

    pub fn previous_instruction(&mut self) -> Option<&Instruction> {
        let prev_index = self.instruction_pointer.step_back()?;
        self.current_instruction = self.instructions.get(prev_index).cloned();
        self.current_instruction.as_ref()
    }

    pub fn should_pause_execution(&self) -> bool {
        if !self.instruction_debug_enabled {
            return false;
        }

        self.current_instruction
            .as_ref()
            .map(|inst| self.instruction_pointer.should_pause_at(inst))
            .unwrap_or(false)
    }

    pub fn call_stack(&self) -> &CallStackInspector {
        &self.call_stack
    }

    pub fn call_stack_mut(&mut self) -> &mut CallStackInspector {
        &mut self.call_stack
    }

    pub fn reset(&mut self) {
        self.current_function = None;
        self.current_args = None;
        self.step_count = 0;
        self.instruction_pointer.reset();
        self.current_instruction = self.instructions.first().cloned();
        self.call_stack.clear();
    }

    pub fn get_instruction_context(&self, context_size: usize) -> Vec<(usize, &Instruction, bool)> {
        let current_idx = self.instruction_pointer.current_index();
        let start = current_idx.saturating_sub(context_size);
        let end = (current_idx + context_size + 1).min(self.instructions.len());

        (start..end)
            .filter_map(|i| {
                self.instructions
                    .get(i)
                    .map(|inst| (i, inst, i == current_idx))
            })
            .collect()
    }
}

impl Default for DebugState {
    fn default() -> Self {
        Self::new()
    }
}
