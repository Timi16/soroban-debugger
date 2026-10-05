//! Instruction pointer management for WASM debugging

use crate::runtime::instruction::{Instruction, OwnedOperator};
use std::collections::VecDeque;

/// Step mode for instruction-level debugging
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StepMode {
    /// Step to the next instruction (step into calls)
    StepInto,
    /// Step over function calls (don't step into)
    StepOver,
    /// Step out of current function
    StepOut,
    /// Step to next basic block
    StepBlock,
}

/// Instruction pointer state
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InstructionPointer {
    /// Current instruction index in the instruction list
    current_index: usize,
    /// History of previous instruction indices for back-stepping
    history: VecDeque<usize>,
    /// Maximum history size
    max_history: usize,
    /// Whether we're currently stepping
    stepping: bool,
    /// Current step mode
    step_mode: StepMode,
    /// Target depth for step out
    target_depth: Option<u32>,
    /// Stack of return addresses for step-into WASM calls
    return_stack: Vec<usize>,
    /// Block depth for detecting end of function
    block_depth: u32,
}

impl InstructionPointer {
    /// Create a new instruction pointer
    pub fn new() -> Self {
        Self {
            current_index: 0,
            history: VecDeque::new(),
            max_history: 1000,
            stepping: false,
            step_mode: StepMode::StepInto,
            target_depth: None,
            return_stack: Vec::new(),
            block_depth: 0,
        }
    }

    /// Get current instruction index
    pub fn current_index(&self) -> usize {
        self.current_index
    }

    /// Get current call stack depth based on return stack
    pub fn call_stack_depth(&self) -> u32 {
        self.return_stack.len() as u32
    }

    /// Check if currently stepping
    pub fn is_stepping(&self) -> bool {
        self.stepping
    }

    /// Get current step mode
    pub fn step_mode(&self) -> StepMode {
        self.step_mode
    }

    /// Start stepping with the given mode
    pub fn start_stepping(&mut self, mode: StepMode) {
        self.stepping = true;
        self.step_mode = mode;

        match mode {
            StepMode::StepOver => {
                self.target_depth = Some(self.call_stack_depth());
            }
            StepMode::StepOut => {
                self.target_depth = if self.call_stack_depth() > 0 {
                    Some(self.call_stack_depth() - 1)
                } else {
                    None
                };
            }
            _ => {
                self.target_depth = None;
            }
        }
    }

    /// Stop stepping
    pub fn stop_stepping(&mut self) {
        self.stepping = false;
        self.target_depth = None;
    }

    /// Move to the next instruction
    pub fn advance_to(&mut self, index: usize) {
        // Add current position to history
        if self.history.len() >= self.max_history {
            self.history.pop_front();
        }
        self.history.push_back(self.current_index);

        self.current_index = index;
    }

    /// Move to previous instruction in history
    pub fn step_back(&mut self) -> Option<usize> {
        if let Some(prev_index) = self.history.pop_back() {
            self.current_index = prev_index;
            Some(prev_index)
        } else {
            None
        }
    }

    /// Update call stack depth based on instruction
    pub fn update_call_stack(&mut self, instruction: &Instruction) {
        match instruction.operator {
            OwnedOperator::Block | OwnedOperator::Loop | OwnedOperator::If => {
                self.block_depth += 1;
            }
            OwnedOperator::End if self.block_depth > 0 => {
                self.block_depth -= 1;
            }
            _ => {}
        }
    }

    /// Push a return address
    pub fn push_return_address(&mut self, index: usize) {
        self.return_stack.push(index);
        self.block_depth = 0; // Reset block depth for new function
    }

    /// Pop a return address
    pub fn pop_return_address(&mut self) -> Option<usize> {
        self.return_stack.pop()
    }

    /// Get current block depth
    pub fn block_depth(&self) -> u32 {
        self.block_depth
    }

    /// Check if we should pause at this instruction based on step mode
    pub fn should_pause_at(&self, instruction: &Instruction) -> bool {
        if !self.stepping {
            return false;
        }

        match self.step_mode {
            StepMode::StepInto => true,
            StepMode::StepOver => {
                // Pause if we're at the same depth or returned
                self.target_depth
                    .map(|target| self.call_stack_depth() <= target)
                    .unwrap_or(true)
            }
            StepMode::StepOut => {
                // Pause if we've returned to target depth
                if let Some(target) = self.target_depth {
                    self.call_stack_depth() <= target
                } else {
                    false
                }
            }
            StepMode::StepBlock => {
                // Pause at control flow instructions or function boundaries
                instruction.is_control_flow() || instruction.local_index == 0
            }
        }
    }

    /// Reset to beginning
    pub fn reset(&mut self) {
        self.current_index = 0;
        self.history.clear();
        self.stepping = false;
        self.target_depth = None;
        self.return_stack.clear();
        self.block_depth = 0;
    }

    /// Get history size
    pub fn history_size(&self) -> usize {
        self.history.len()
    }

    /// Clear history
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Set maximum history size
    pub fn set_max_history(&mut self, max: usize) {
        self.max_history = max;
        while self.history.len() > max {
            self.history.pop_front();
        }
    }
}

impl Default for InstructionPointer {
    fn default() -> Self {
        Self::new()
    }
}

/// Instruction execution context
#[derive(Debug, Clone)]
pub struct InstructionContext {
    /// Current instruction being executed
    pub instruction: Instruction,
    /// Instruction pointer state
    pub pointer: InstructionPointer,
    /// Whether execution should pause
    pub should_pause: bool,
}

impl InstructionContext {
    /// Create a new instruction context
    pub fn new(instruction: Instruction, pointer: InstructionPointer) -> Self {
        let should_pause = pointer.should_pause_at(&instruction);
        Self {
            instruction,
            pointer,
            should_pause,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::instruction::Instruction;
    use wasmparser::Operator;

    #[test]
    fn test_instruction_pointer_advance() {
        let mut ip = InstructionPointer::new();
        assert_eq!(ip.current_index(), 0);

        ip.advance_to(5);
        assert_eq!(ip.current_index(), 5);
        assert_eq!(ip.history_size(), 1);
    }

    #[test]
    fn test_step_back() {
        let mut ip = InstructionPointer::new();
        ip.advance_to(5);
        ip.advance_to(10);

        assert_eq!(ip.step_back(), Some(5));
        assert_eq!(ip.current_index(), 5);
        assert_eq!(ip.step_back(), Some(0));
        assert_eq!(ip.current_index(), 0);
        assert_eq!(ip.step_back(), None);
    }

    #[test]
    fn test_stepping_modes() {
        let mut ip = InstructionPointer::new();

        assert!(!ip.is_stepping());

        ip.start_stepping(StepMode::StepInto);
        assert!(ip.is_stepping());
        assert_eq!(ip.step_mode(), StepMode::StepInto);

        ip.stop_stepping();
        assert!(!ip.is_stepping());
    }

    #[test]
    fn test_call_stack_tracking() {
        let mut ip = InstructionPointer::new();

        // Simulate jumping into a call
        ip.push_return_address(10);
        assert_eq!(ip.call_stack_depth(), 1);

        let block_inst = Instruction::new(
            0x100,
            Operator::Block {
                blockty: wasmparser::BlockType::Empty,
            },
            1,
            0,
        );
        ip.update_call_stack(&block_inst);
        assert_eq!(ip.block_depth(), 1);

        let end_inst = Instruction::new(0x200, Operator::End, 1, 10);
        ip.update_call_stack(&end_inst);
        assert_eq!(ip.block_depth(), 0);
        assert_eq!(ip.call_stack_depth(), 1); // Depth still 1 until we pop

        ip.pop_return_address();
        assert_eq!(ip.call_stack_depth(), 0);
    }
}
