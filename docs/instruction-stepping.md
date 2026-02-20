# Instruction-Level Stepping

The Soroban Debugger now supports instruction-by-instruction stepping through WASM bytecode, providing fine-grained control over contract execution.

## Overview

Instruction-level debugging allows you to:
- Step through individual WASM instructions
- Examine the exact execution flow at the bytecode level
- Understand performance characteristics at the instruction level
- Debug complex contract logic with precision
- Correlate high-level code with low-level WASM operations

## Usage

### Basic Instruction Debugging

Enable instruction-level debugging with the `--instruction-debug` flag:

```bash
soroban-debug run --contract token.wasm --function transfer --args '["Alice", "Bob", 100]' --instruction-debug
```

### Interactive Instruction Stepping

Start with instruction stepping enabled:

```bash
soroban-debug run --contract token.wasm --function transfer --args '["Alice", "Bob", 100]' --instruction-debug --step-instructions
```

This will enter an interactive stepping mode where you can control execution instruction by instruction.

### Step Modes

You can specify different step modes:

```bash
# Step into every instruction (default)
soroban-debug run --contract token.wasm --function transfer --instruction-debug --step-instructions --step-mode into

# Step over function calls
soroban-debug run --contract token.wasm --function transfer --instruction-debug --step-instructions --step-mode over

# Step to next basic block
soroban-debug run --contract token.wasm --function transfer --instruction-debug --step-instructions --step-mode block
```

## Interactive Commands

When in instruction stepping mode, the following commands are available:

### Stepping Commands

- `n`, `next` - Step to the next instruction
- `s`, `step`, `into` - Step into the next instruction (same as next in instruction mode)
- `o`, `over` - Step over function calls (don't step into)
- `u`, `out` - Step out of the current function
- `b`, `block` - Step to the next basic block (control flow instruction)
- `p`, `prev`, `back` - Step back to the previous instruction

### Information Commands

- `i`, `info` - Show detailed instruction and execution state
- `ctx`, `context` - Display instruction context (prompts for context size)
- `h`, `help` - Show all available commands

### Execution Control

- `c`, `continue` - Continue execution until completion
- `q`, `quit`, `exit` - Exit instruction stepping mode

### Example Session

```
=== Instruction Stepping Mode ===
Type 'help' for available commands

┌─ Instruction Context ─────────────────────────────┐
│  0: ► 00000100: i32.const 42                      │
│  1:   00000105: local.set $0                      │
│  2:   00000107: local.get $0                      │
│  3:   0000010a: i32.const 1                       │
│  4:   0000010f: i32.add                           │
└───────────────────────────────────────────────────┘

(step) > n
Stepped to: 00000105: local.set $0

(step) > info
┌─ Instruction Pointer ─────────────────────────────┐
│ Current Index:     1                              │
│ Call Depth:        0                              │
│ Step Mode:     Step Into                          │
│ Stepping:      Active                             │
└───────────────────────────────────────────────────┘

┌─ Execution Progress ──────────────────────────────┐
│ Total Instructions:         126                   │
│ Current Position:             1                   │
│ Instructions Executed:        2                   │
│ Progress:                   0.8%                  │
│ [█░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░] │
└───────────────────────────────────────────────────┘

(step) > continue
Continuing execution...
Execution completed. Result: "Success"
```

## Architecture

### Core Components

#### 1. Instruction Parser (`src/runtime/instruction.rs`)

Parses WASM bytecode and extracts individual instructions with their metadata:

- **Instruction struct**: Represents a single WASM instruction with offset, opcode, and context
- **InstructionParser**: Parses WASM modules and extracts all instructions
- **Instruction formatting**: Human-readable display of instructions

#### 2. Instruction Pointer (`src/debugger/instruction_pointer.rs`)

Manages execution position and stepping state:

- **InstructionPointer**: Tracks current position, call stack depth, and execution history
- **StepMode enum**: Defines different stepping modes (into, over, out, block)
- **History management**: Maintains execution history for backward stepping

#### 3. Enhanced Debug State (`src/debugger/state.rs`)

Extended to support instruction-level information:

- **Instruction storage**: Holds all parsed instructions for the contract
- **Current instruction tracking**: Maintains reference to currently executing instruction
- **Stepping state management**: Tracks whether instruction debugging is enabled

#### 4. Enhanced Stepper (`src/debugger/stepper.rs`)

Implements instruction-level stepping logic:

- **Step modes**: Implements different stepping strategies
- **Execution control**: Manages when to pause and continue
- **State coordination**: Works with debug state to track execution

#### 5. Enhanced Debugger Engine (`src/debugger/engine.rs`)

Core orchestrator with instruction-level capabilities:

- **Instruction debugging control**: Enable/disable instruction-level debugging
- **Stepping interface**: Provides step_into, step_over, step_out, step_back methods
- **Execution coordination**: Integrates with WASM execution to enable stepping

### Data Flow

1. **WASM Parsing**: `InstructionParser` extracts all instructions from WASM bytecode
2. **State Initialization**: `DebugState` stores instructions and initializes instruction pointer
3. **Instrumentation**: `Instrumenter` (future) will add callbacks for execution control
4. **Stepping Control**: User commands trigger stepping operations via `Stepper`
5. **Execution Management**: `DebuggerEngine` coordinates between stepping and execution
6. **Display**: `Formatter` provides user-friendly instruction and state display

## Instrumentation Details

### Current Implementation

The current implementation provides a foundation for instruction-level debugging:

- **Parsing-based**: Extracts instructions from WASM for display and analysis
- **Simulation**: Steps through instructions conceptually without runtime integration
- **Interactive UI**: Provides full stepping interface and state display

### Future Enhancement: Runtime Integration

For full runtime integration, the instrumentation system will:

1. **WASM Modification**: Use `walrus` to inject debug callbacks before each instruction
2. **Runtime Hooks**: Add callback functions that pause execution and return control to debugger
3. **State Synchronization**: Keep debugger state synchronized with actual execution state
4. **Performance Optimization**: Provide debug mode toggle to minimize overhead

### Limitations

Current limitations and future improvements:

1. **Simulation vs. Runtime**: Current implementation simulates stepping rather than controlling actual execution
2. **WASM Integration**: Full integration requires hooking into Soroban's WASM execution environment
3. **Performance**: Runtime instrumentation may impact execution speed
4. **Source Mapping**: Future versions could correlate instructions with Rust source code

## Performance Considerations

### Debug Mode Toggle

Instruction-level debugging can be toggled on/off to minimize performance impact:

```bash
# Normal execution (fast)
soroban-debug run --contract token.wasm --function transfer

# With instruction debugging (slower but detailed)
soroban-debug run --contract token.wasm --function transfer --instruction-debug
```

### Optimization Strategies

1. **Lazy Instrumentation**: Only instrument when debug mode is enabled
2. **Basic Block Stepping**: Step through groups of instructions for better performance
3. **Selective Instrumentation**: Only instrument functions of interest
4. **History Limits**: Limit instruction history to prevent memory bloat

## Examples

### Basic Contract Debugging

```bash
# Debug a simple token transfer with instruction stepping
soroban-debug run \
  --contract examples/token.wasm \
  --function transfer \
  --args '["ALICE", "BOB", 1000]' \
  --instruction-debug \
  --step-instructions
```

### Advanced Debugging with Breakpoints

```bash
# Combine function breakpoints with instruction stepping
soroban-debug run \
  --contract examples/complex.wasm \
  --function process \
  --args '{"data": [1,2,3,4,5]}' \
  --breakpoint validate \
  --breakpoint compute \
  --instruction-debug
```

### Performance Analysis

```bash
# Use block stepping for performance analysis
soroban-debug run \
  --contract examples/heavy.wasm \
  --function compute_heavy \
  --instruction-debug \
  --step-instructions \
  --step-mode block \
  --verbose
```

## Testing

Comprehensive tests ensure reliability:

### Unit Tests

- Instruction parsing accuracy
- Step mode behavior
- Instruction pointer management
- State synchronization

### Integration Tests

- Full stepping workflow
- Command interface
- Error handling
- Performance benchmarks

### Performance Tests

- Instruction parsing speed
- Memory usage with large contracts
- Step operation latency

Run tests with:

```bash
cargo test instruction_stepping
cargo test --test instruction_stepping_tests
```

## Future Enhancements

### Planned Features

1. **Source Code Mapping**: Correlate WASM instructions with Rust source code
2. **Runtime Integration**: Full integration with Soroban execution environment
3. **Visual Debugger**: GUI interface for instruction stepping
4. **Advanced Breakpoints**: Instruction-level and conditional breakpoints
5. **Execution Recording**: Record and replay execution sessions

### Advanced Debugging

1. **Conditional Stepping**: Step only when certain conditions are met
2. **Instruction Breakpoints**: Break at specific instruction addresses
3. **Memory Watches**: Track memory changes during execution
4. **Gas/Budget Analysis**: Correlate instructions with resource usage

This instruction-level stepping feature provides a powerful foundation for detailed contract debugging and analysis, enabling developers to understand their code at the most granular level possible.