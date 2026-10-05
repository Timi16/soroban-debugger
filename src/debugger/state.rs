use crate::debugger::instruction_pointer::{InstructionPointer, StepMode};
use crate::inspector::stack::CallStackInspector;
use crate::output::InvocationReason;
use crate::runtime::instruction::Instruction;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PauseReason {
    Breakpoint,
    StepBoundary,
    Panic,
    EndOfExecution,
    UserInterrupt,
}

impl PauseReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Breakpoint => "breakpoint",
            Self::StepBoundary => "step_boundary",
            Self::Panic => "panic",
            Self::EndOfExecution => "end_of_execution",
            Self::UserInterrupt => "user_interrupt",
        }
    }
}

/// Represents the current state of the debugger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugState {
    current_function: Option<String>,
    current_args: Option<String>,
    current_invocation_reason: Option<InvocationReason>,
    step_count: usize,
    instruction_pointer: InstructionPointer,
    #[serde(skip)]
    current_instruction: Option<Instruction>,
    #[serde(skip)]
    instructions: Vec<Instruction>,
    instruction_debug_enabled: bool,
    call_stack: CallStackInspector,
    pause_reason: Option<PauseReason>,
}

impl DebugState {
    /// Create a new debug state.
    pub fn new() -> Self {
        Self {
            current_function: None,
            current_args: None,
            current_invocation_reason: None,
            step_count: 0,
            instruction_pointer: InstructionPointer::new(),
            current_instruction: None,
            instructions: Vec::new(),
            instruction_debug_enabled: false,
            call_stack: CallStackInspector::new(),
            pause_reason: None,
        }
    }

    /// Set the current function being executed
    pub fn set_current_function(
        &mut self,
        function: String,
        args: Option<String>,
        invocation_reason: Option<InvocationReason>,
    ) {
        self.current_function = Some(function);
        self.current_args = args;
        self.current_invocation_reason = invocation_reason;
    }

    pub fn current_function(&self) -> Option<&str> {
        self.current_function.as_deref()
    }

    /// Get current function arguments
    pub fn current_args(&self) -> Option<&str> {
        self.current_args.as_deref()
    }

    pub fn current_invocation_reason(&self) -> Option<InvocationReason> {
        self.current_invocation_reason
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
        let current_index = self.instruction_pointer.current_index();
        let mut next_index = current_index.saturating_add(1);

        if let Some(inst) = self.current_instruction.clone() {
            // Check if it's a call and we are stepping into it
            if inst.is_call() && self.instruction_pointer.step_mode() == StepMode::StepInto {
                if let crate::runtime::instruction::OwnedOperator::Call { function_index } =
                    inst.operator
                {
                    // Find target function
                    if let Some((idx, _)) = self
                        .instructions
                        .iter()
                        .enumerate()
                        .find(|(_, i)| i.function_index == function_index && i.local_index == 0)
                    {
                        self.instruction_pointer.push_return_address(next_index);
                        next_index = idx;

                        // Push to active call stack for adapter.ts
                        let next_func_name = format!("func_{}", function_index);
                        self.call_stack_mut().push(next_func_name, None);
                    }
                }
            } else if matches!(
                inst.operator,
                crate::runtime::instruction::OwnedOperator::Return
            ) || (matches!(
                inst.operator,
                crate::runtime::instruction::OwnedOperator::End
            ) && self.instruction_pointer.block_depth() == 0)
            {
                // Function end, return to caller
                if let Some(ret_addr) = self.instruction_pointer.pop_return_address() {
                    next_index = ret_addr;
                    // Pop from active call stack
                    if self.call_stack().get_stack().len() > 1 {
                        self.call_stack_mut().pop();
                    }
                }
            }
        }

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

    pub fn set_pause_reason(&mut self, reason: PauseReason) {
        self.pause_reason = Some(reason);
    }

    pub fn clear_pause_reason(&mut self) {
        self.pause_reason = None;
    }

    pub fn pause_reason(&self) -> Option<PauseReason> {
        self.pause_reason
    }

    pub fn reset(&mut self) {
        self.current_function = None;
        self.current_args = None;
        self.step_count = 0;
        self.instruction_pointer.reset();
        self.current_instruction = self.instructions.first().cloned();
        self.call_stack.clear();
        self.pause_reason = None;
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
