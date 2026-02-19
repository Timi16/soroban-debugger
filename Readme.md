# Soroban Debugger

[![CI](https://github.com/Timi16/soroban-debugger/actions/workflows/ci.yml/badge.svg)](https://github.com/Timi16/soroban-debugger/actions/workflows/ci.yml)

A command-line debugger for Soroban smart contracts on the Stellar network. Debug your contracts interactively with breakpoints, step-through execution, state inspection, and budget tracking.

## Features

- Step-through execution of Soroban contracts
- Set breakpoints at function boundaries
- Inspect contract storage and state
- Track resource usage (CPU and memory budget)
- View call stacks for contract invocations
- Interactive terminal UI for debugging sessions
- Support for cross-contract calls

## Installation

### From Source

```bash
git clone https://github.com/Timi16/soroban-debugger.git
cd soroban-debugger
cargo install --path .
```

### Using Cargo

```bash
cargo install soroban-debugger
```

## Quick Start

### Basic Usage

Debug a contract by specifying the WASM file and function to execute:

```bash
soroban-debug run --contract token.wasm --function transfer --args '["Alice", "Bob", 100]'
```

### Interactive Mode

Start an interactive debugging session:

```bash
soroban-debug interactive --contract my_contract.wasm
```

Then use commands like:
- `s` or `step` - Execute next instruction
- `c` or `continue` - Run until next breakpoint
- `i` or `inspect` - Show current state
- `storage` - Display contract storage
- `budget` - Show resource usage
- `q` or `quit` - Exit debugger

## Commands

### Run Command

Execute a contract function with the debugger:

```bash
soroban-debug run [OPTIONS]

Options:
  -c, --contract <FILE>     Path to the contract WASM file
  -f, --function <NAME>     Function name to execute
  -a, --args <JSON>         Function arguments as JSON array
  -s, --storage <JSON>      Initial storage state as JSON
  -b, --breakpoint <NAME>   Set breakpoint at function name
```

### Interactive Command

Start an interactive debugging session:

```bash
soroban-debug interactive [OPTIONS]

Options:
  -c, --contract <FILE>     Path to the contract WASM file
```

### Inspect Command

View contract information without executing:

```bash
soroban-debug inspect [OPTIONS]

Options:
  -c, --contract <FILE>     Path to the contract WASM file
```

## Examples

### Example 1: Debug a Token Transfer

```bash
soroban-debug run \
  --contract token.wasm \
  --function transfer \
  --args '["user1", "user2", 100]'
```

Output:
```
> Debugger started
> Paused at: transfer
> Args: from=user1, to=user2, amount=100

(debug) s
> Executing: get_balance(user1)
> Storage: balances[user1] = 500

(debug) s
> Executing: set_balance(user1, 400)

(debug) storage
Storage:
  balances[user1] = 400
  balances[user2] = 100

(debug) c
> Execution completed
> Result: Ok(())
```

### Example 2: Set Breakpoints

```bash
soroban-debug run \
  --contract dao.wasm \
  --function execute \
  --breakpoint verify_signature \
  --breakpoint update_state
```

### Example 3: Initial Storage State

```bash
soroban-debug run \
  --contract token.wasm \
  --function mint \
  --storage '{"balances": {"Alice": 1000}, "total_supply": 5000}'
```

### Example 4: Track Budget Usage

```bash
soroban-debug run --contract complex.wasm --function expensive_operation

> Budget: CPU 45000/100000 (45%), Memory 15KB/40KB (37%)
> Warning: High CPU usage detected
```

## Interactive Commands Reference

During an interactive debugging session, you can use:

```
Commands:
  s, step              Execute next instruction
  c, continue          Run until breakpoint or completion
  n, next              Step over function calls
  i, inspect           Show current execution state
  storage              Display all storage entries
  stack                Show call stack
  budget               Show resource usage (CPU/memory)
  args                 Display function arguments
  break <function>     Set breakpoint at function
  list-breaks          List all breakpoints
  clear <function>     Remove breakpoint
  help                 Show this help message
  q, quit              Exit debugger
```

## Use Cases

### Debugging Failed Transactions

When your contract transaction fails without clear error messages, use the debugger to step through execution and identify where and why it fails.

### Storage Inspection

Verify that your contract is reading and writing storage correctly by inspecting storage state at each step.

### Budget Optimization

Identify which operations consume the most CPU or memory to optimize your contract's resource usage.

### Cross-Contract Call Tracing

Debug interactions between multiple contracts by following the call stack through contract boundaries.

### Testing Edge Cases

Quickly test different input scenarios interactively without redeploying your contract.
<!-- 
## Project Structure

```
soroban-debugger/
├── src/
│   ├── main.rs              CLI entry point
│   ├── lib.rs               Library exports
│   ├── cli/                 Command-line interface
│   ├── debugger/            Core debugging engine
│   ├── runtime/             WASM execution environment
│   ├── inspector/           State inspection tools
│   ├── ui/                  Terminal user interface
│   └── utils/               Helper utilities
├── tests/                   Integration tests
└── examples/                Example contracts and tutorials
``` -->

## Development

### Building from Source

```bash
git clone https://github.com/Timi16/soroban-debugger.git
cd soroban-debugger
cargo build --release
```

### Running Tests

```bash
cargo test
```

### Running Examples

```bash
cargo run --example simple_token
```

## Requirements

- Rust 1.75 or later
- Soroban SDK 22.0.0 or later

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.

### Development Setup

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run tests: `cargo test`
5. Submit a pull request

### Code Style

This project follows standard Rust formatting:

```bash
cargo fmt
cargo clippy
```

<!-- ## Roadmap

### Phase 1 (Current)
- Basic CLI and command parsing
- Simple step-through execution
- Storage inspection
- Budget tracking

### Phase 2
- Breakpoint management
- Enhanced terminal UI
- Call stack visualization
- Replay execution from trace

### Phase 3
- WASM instrumentation for precise breakpoints
- Source map support
- Memory profiling
- Performance analysis tools -->

## License

Licensed under either of:

- Apache License, Version 2.0 (LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license (LICENSE-MIT or http://opensource.org/licenses/MIT)

at your option.

## Resources

- Soroban Documentation: https://soroban.stellar.org/docs
- Stellar Developer Discord: https://discord.gg/stellardev
- Issue Tracker: https://github.com/Timi16/soroban-debugger/issues
- [CHANGELOG](CHANGELOG.md) - Release history and changes

## Acknowledgments

Built for the Stellar ecosystem to improve the Soroban smart contract development experience.