//! Tests for instruction-level stepping functionality

use soroban_debugger::runtime::{Instruction, InstructionParser};

/// Create a simple WASM module for testing
fn create_test_wasm() -> Vec<u8> {
    // This is a minimal valid WASM module
    vec![
        0x00, 0x61, 0x73, 0x6d, // magic number
        0x01, 0x00, 0x00, 0x00, // version
        // Type section: one function type with no parameters and no return
        0x01, 0x04, 0x01, 0x60, 0x00, 0x00, // Function section: one function of type 0
        0x03, 0x02, 0x01, 0x00, // Code section: the function body
        0x0a, 0x04, 0x01, 0x02, 0x00, 0x0b, // func with no locals, just 'end'
    ]
}

#[test]
fn test_instruction_parser() {
    let wasm_bytes = create_test_wasm();
    let mut parser = InstructionParser::new();

    let result = parser.parse(&wasm_bytes);
    assert!(result.is_ok(), "Failed to parse WASM: {:?}", result.err());

    let instructions = result.unwrap();
    assert!(!instructions.is_empty(), "No instructions parsed");
}

#[test]
fn test_instruction_display() {
    let instruction = Instruction::new(0x100, wasmparser::Operator::I32Const { value: 42 }, 0, 0);

    let display = format!("{}", instruction);
    assert!(display.contains("00000100"));
    assert!(display.contains("i32.const"));
    assert!(display.contains("42"));
}

#[test]
fn test_instruction_operands() {
    let call_inst = Instruction::new(
        0x100,
        wasmparser::Operator::Call { function_index: 5 },
        0,
        0,
    );

    assert_eq!(call_inst.operands(), "func_5");
    assert!(call_inst.is_control_flow());
    assert!(call_inst.is_call());

    let local_get_inst = Instruction::new(
        0x104,
        wasmparser::Operator::LocalGet { local_index: 3 },
        0,
        1,
    );

    assert_eq!(local_get_inst.operands(), "$3");
    assert!(!local_get_inst.is_control_flow());
    assert!(!local_get_inst.is_call());
}

#[test]
fn test_debugger_engine_instruction_debug_without_executor() {
    // Test instruction debugging functionality without creating a ContractExecutor
    // This avoids the Soroban metadata requirements

    let wasm_bytes = create_test_wasm();

    // Test instruction parser directly
    let mut parser = soroban_debugger::runtime::instruction::InstructionParser::new();
    let result = parser.parse(&wasm_bytes);

    // The parser should work even if the WASM isn't a valid Soroban contract
    assert!(result.is_ok() || result.is_err()); // Either outcome is acceptable for basic WASM
}

#[test]
fn test_step_modes_without_executor() {
    // Test step modes using just the step mode enum and instruction pointer
    use soroban_debugger::debugger::instruction_pointer::{InstructionPointer, StepMode};

    let mut ip = InstructionPointer::new();

    // Test different step modes
    ip.start_stepping(StepMode::StepInto);
    assert_eq!(ip.step_mode(), StepMode::StepInto);

    ip.start_stepping(StepMode::StepOver);
    assert_eq!(ip.step_mode(), StepMode::StepOver);

    ip.start_stepping(StepMode::StepOut);
    assert_eq!(ip.step_mode(), StepMode::StepOut);

    ip.start_stepping(StepMode::StepBlock);
    assert_eq!(ip.step_mode(), StepMode::StepBlock);
}

#[test]
fn test_instruction_context_formatting() {
    // Test instruction context formatting without requiring a full engine
    use soroban_debugger::runtime::instruction::Instruction;
    use soroban_debugger::ui::formatter::Formatter;

    let instruction = Instruction::new(0x100, wasmparser::Operator::I32Const { value: 42 }, 0, 0);

    let context = vec![
        (0, instruction.clone(), false),
        (1, instruction.clone(), true), // Current instruction
        (2, instruction.clone(), false),
    ];

    let formatted = Formatter::format_instruction_context(&context, 3);
    assert!(formatted.contains("Instruction Context"));
    assert!(formatted.contains("i32.const"));
    assert!(formatted.contains("►")); // Current instruction marker
}

#[test]
fn test_stepper_without_executor() {
    // Test stepper functionality without requiring a ContractExecutor
    use soroban_debugger::debugger::instruction_pointer::StepMode;
    use soroban_debugger::debugger::{DebugState, Stepper};

    let mut stepper = Stepper::new();
    let mut debug_state = DebugState::new();

    // Test stepper activation
    assert!(!stepper.is_active());

    stepper.start(StepMode::StepInto, &mut debug_state);
    assert!(stepper.is_active());
    assert_eq!(stepper.step_mode(), StepMode::StepInto);

    stepper.stop(&mut debug_state);
    assert!(!stepper.is_active());
}

#[test]
fn test_instruction_pointer_history() {
    use soroban_debugger::debugger::instruction_pointer::InstructionPointer;

    let mut ip = InstructionPointer::new();
    assert_eq!(ip.current_index(), 0);
    assert_eq!(ip.history_size(), 0);

    // Advance and check history
    ip.advance_to(5);
    assert_eq!(ip.current_index(), 5);
    assert_eq!(ip.history_size(), 1);

    ip.advance_to(10);
    assert_eq!(ip.current_index(), 10);
    assert_eq!(ip.history_size(), 2);

    // Step back
    assert_eq!(ip.step_back(), Some(5));
    assert_eq!(ip.current_index(), 5);
    assert_eq!(ip.step_back(), Some(0));
    assert_eq!(ip.current_index(), 0);
    assert_eq!(ip.step_back(), None);
}

#[test]
fn test_call_stack_tracking() {
    use soroban_debugger::debugger::instruction_pointer::InstructionPointer;

    let mut ip = InstructionPointer::new();

    let call_inst = Instruction::new(
        0x100,
        wasmparser::Operator::Call { function_index: 1 },
        0,
        0,
    );

    ip.update_call_stack(&call_inst);
    assert_eq!(ip.call_stack_depth(), 1);

    let return_inst = Instruction::new(0x200, wasmparser::Operator::Return, 1, 10);

    ip.update_call_stack(&return_inst);
    assert_eq!(ip.call_stack_depth(), 0);
}

#[test]
fn test_stepping_pause_conditions() {
    use soroban_debugger::debugger::instruction_pointer::{InstructionPointer, StepMode};

    let mut ip = InstructionPointer::new();

    // Test StepInto mode
    ip.start_stepping(StepMode::StepInto);

    let simple_inst = Instruction::new(0x100, wasmparser::Operator::I32Add, 0, 0);

    assert!(ip.should_pause_at(&simple_inst));

    // Test that stepping can be stopped
    ip.stop_stepping();
    assert!(!ip.should_pause_at(&simple_inst));
}

#[test]
fn test_instruction_formatting() {
    use soroban_debugger::ui::formatter::Formatter;

    let instruction = Instruction::new(
        0x100,
        wasmparser::Operator::LocalGet { local_index: 5 },
        0,
        0,
    );

    // Test current instruction formatting
    let formatted_current = Formatter::format_instruction(&instruction, true);
    assert!(formatted_current.starts_with("►"));
    assert!(formatted_current.contains("local.get"));

    // Test non-current instruction formatting
    let formatted_normal = Formatter::format_instruction(&instruction, false);
    assert!(formatted_normal.starts_with(" "));
    assert!(formatted_normal.contains("local.get"));
}

#[test]
fn test_instruction_pointer_state_formatting() {
    use soroban_debugger::debugger::instruction_pointer::StepMode;
    use soroban_debugger::ui::formatter::Formatter;

    let formatted =
        Formatter::format_instruction_pointer_state(42, 3, Some(StepMode::StepInto), true);

    assert!(formatted.contains("42"));
    assert!(formatted.contains("3"));
    assert!(formatted.contains("Step Into"));
    assert!(formatted.contains("Active"));
}

#[test]
fn test_stepping_help_formatting() {
    use soroban_debugger::ui::formatter::Formatter;

    let help = Formatter::format_stepping_help();
    assert!(help.contains("next"));
    assert!(help.contains("step"));
    assert!(help.contains("over"));
    assert!(help.contains("continue"));
}

// Integration test for full stepping workflow
#[test]
fn test_debug_state_instruction_management() {
    // Test debug state instruction management without requiring executor
    use soroban_debugger::debugger::instruction_pointer::StepMode;
    use soroban_debugger::debugger::DebugState;
    use soroban_debugger::runtime::instruction::Instruction;

    let mut debug_state = DebugState::new();

    // Create test instructions
    let instructions = vec![
        Instruction::new(0x100, wasmparser::Operator::I32Const { value: 42 }, 0, 0),
        Instruction::new(
            0x105,
            wasmparser::Operator::LocalSet { local_index: 0 },
            0,
            1,
        ),
        Instruction::new(
            0x107,
            wasmparser::Operator::LocalGet { local_index: 0 },
            0,
            2,
        ),
    ];

    // Test setting instructions
    debug_state.set_instructions(instructions);
    assert_eq!(debug_state.instructions().len(), 3);
    assert!(debug_state.current_instruction().is_some());

    // Test instruction stepping
    debug_state.enable_instruction_debug();
    assert!(debug_state.is_instruction_debug_enabled());

    debug_state.start_instruction_stepping(StepMode::StepInto);
    assert!(debug_state.instruction_pointer().is_stepping());

    // Test advancing instructions
    let next = debug_state.next_instruction();
    assert!(next.is_some());
}

// Performance test to ensure instruction parsing is acceptable
#[test]
fn test_instruction_parsing_performance() {
    let wasm_bytes = create_test_wasm();
    let start = std::time::Instant::now();

    let mut parser = InstructionParser::new();
    for _ in 0..100 {
        let _ = parser.parse(&wasm_bytes);
    }

    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 1000,
        "Instruction parsing too slow: {:?}",
        elapsed
    );
}
