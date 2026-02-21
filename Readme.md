# Soroban Debugger

[![CI](https://github.com/Timi16/soroban-debugger/actions/workflows/ci.yml/badge.svg)](https://github.com/Timi16/soroban-debugger/actions/workflows/ci.yml)

A command-line debugger for Soroban smart contracts on the Stellar network. Debug your contracts interactively with breakpoints, step-through execution, state inspection, and budget tracking.

## Features

- Step-through execution of Soroban contracts
- **Source-Level Debugging**: Map WASM execution back to Rust source lines
- **Time-Travel Debugging**: Step backward and navigate execution history
- Set breakpoints at function boundaries
- Inspect contract storage and state
- Track resource usage (CPU and memory budget)
- View call stacks for contract invocations
- Interactive terminal UI for debugging sessions
- Support for cross-contract calls
- Parallel batch execution for regression testing

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

### Man Page

A Unix man page is automatically generated for the CLI and all subcommands during the build process. To install them:

```bash
# After building from source
sudo cp man/man1/soroban-debug* /usr/local/share/man/man1/
```

Once installed, you can access the documentation using:

```bash
man soroban-debug
# For subcommands:
man soroban-debug-run
```

## Quick Start

### Basic Usage

Debug a contract by specifying the WASM file and function to execute:

```bash
# Array arguments
soroban-debug run --contract token.wasm --function transfer --args '["Alice", "Bob", 100]'

# Map argument (JSON object)
soroban-debug run --contract token.wasm --function update --args '{"user":"Alice","balance":1000}'
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
      --storage-filter <PATTERN>  Filter storage by key pattern (repeatable)
      --batch-args <FILE>   Path to JSON file with array of argument sets for batch execution
      --watch               Watch the WASM file for changes and automatically re-run
```

### Watch Mode

Automatically reload and re-run when the WASM file changes:

```bash
soroban-debug run \
  --contract target/wasm32-unknown-unknown/release/my_contract.wasm \
  --function transfer \
  --args '["user1", "user2", 100]' \
  --watch
```

Perfect for development - edit your contract, rebuild, and see results immediately. See [docs/watch-mode.md](docs/watch-mode.md) for details.

### Batch Execution

Run the same contract function with multiple argument sets in parallel for regression testing:

```bash
soroban-debug run \
  --contract token.wasm \
  --function transfer \
  --batch-args batch_tests.json
```

The batch args file should contain a JSON array of test cases:

```json
[
  {
    "args": "[\"Alice\", \"Bob\", 100]",
    "expected": "Ok(())",
    "label": "Transfer 100 tokens"
  },
  {
    "args": "[\"Charlie\", \"Dave\", 50]",
    "expected": "Ok(())",
    "label": "Transfer 50 tokens"
  }
]
```

See [docs/batch-execution.md](docs/batch-execution.md) for detailed documentation.

### Storage Filtering

Filter large storage outputs by key pattern using `--storage-filter`:

```bash
# Prefix match: keys starting with "balance:"
soroban-debug run --contract token.wasm --function mint \
  --storage-filter 'balance:*'

# Regex match: keys matching a pattern
soroban-debug run --contract token.wasm --function mint \
  --storage-filter 're:^user_\d+$'

# Exact match
soroban-debug run --contract token.wasm --function mint \
  --storage-filter 'total_supply'

# Multiple filters (combined with OR)
soroban-debug run --contract token.wasm --function mint \
  --storage-filter 'balance:*' \
  --storage-filter 'total_supply'
```

#### Exporting Execution Traces

You can export a full record of the contract execution to a JSON file using the `--trace-output` flag. This trace captures function calls, arguments, return values, storage snapshots (before and after), events, and budget consumption.

```bash
soroban-debug run \
  --wasm contract.wasm \
  --function hello \
  --trace-output execution_trace.json
```

These traces can later be used with the `compare` command to identify regressions or differences between runs.

##### Example Trace Output (JSON)

An exported trace includes versioning, metadata, and full execution state:

```json
{
  "version": "1.0",
  "label": "Execution of hello",
  "contract": "CA7QYNF5GE5XEC4HALXWFVQQ5TQWQ5LF7WMXMEQG7BWHBQV26YCWL5",
  "function": "hello",
  "args": "[\"world\"]",
  "storage_before": {
    "counter": "0"
  },
  "storage": {
    "counter": "1"
  },
  "budget": {
    "cpu_instructions": 1540,
    "memory_bytes": 450,
    "cpu_limit": 1000000,
    "memory_limit": 1000000
  },
  "return_value": "void",
  "call_sequence": [
    {
      "function": "hello",
      "args": "[\"world\"]",
      "depth": 0,
      "budget": {
        "cpu_instructions": 1540,
        "memory_bytes": 450
      }
    }
  ],
  "events": [
    {
      "contract_id": "CA7Q...",
      "topics": ["\"greeting\""],
      "data": "\"Hello, world!\""
    }
  ]
}
```

| Pattern          | Type   | Matches                                |
|------------------|--------|----------------------------------------|
| `balance:*`      | Prefix | Keys starting with `balance:`          |
| `re:^user_\d+$`  | Regex  | Keys matching the regex                |
| `total_supply`   | Exact  | Only the key `total_supply`            |
| Pattern         | Type   | Matches                       |
| --------------- | ------ | ----------------------------- |
| `balance:*`     | Prefix | Keys starting with `balance:` |
| `re:^user_\d+$` | Regex  | Keys matching the regex       |
| `total_supply`  | Exact  | Only the key `total_supply`   |

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
      --dependency-graph     Export cross-contract dependency graph (DOT + Mermaid)
```

For full examples, see [docs/dependency-graph.md](docs/dependency-graph.md).

### Completions Command

Generate shell completion scripts for your favorite shell:

```bash
soroban-debug completions bash > /usr/local/etc/bash_completion.d/soroban-debug
```

Supported shells: `bash`, `zsh`, `fish`, `powershell`.

#### Installation Instructions

**Bash:**

```bash
soroban-debug completions bash > /usr/local/etc/bash_completion.d/soroban-debug
```

**Zsh:**

```bash
soroban-debug completions zsh > /usr/local/share/zsh/site-functions/_soroban-debug
```

**Fish:**

```bash
soroban-debug completions fish > ~/.config/fish/completions/soroban-debug.fish
```

**PowerShell:**

```powershell
soroban-debug completions powershell >> $PROFILE
```

### Compare Command

Compare two execution trace JSON files side-by-side to identify
differences and regressions in storage, budget, return values, and
execution flow:

```bash
soroban-debug compare <TRACE_A> <TRACE_B> [OPTIONS]

Options:
  -o, --output <FILE>       Output file for the comparison report (default: stdout)
```

Example:

```bash
# Compare two saved execution traces
soroban-debug compare examples/trace_a.json examples/trace_b.json

# Save report to a file
soroban-debug compare baseline.json new.json --output diff_report.txt
```

See [`doc/compare.md`](doc/compare.md) for the full trace JSON format reference
and a regression testing workflow guide.

## Examples

### Example 1: Debug a Token Transfer

```bash
soroban-debug run \
  --contract token.wasm \
  --function transfer \
  --args '["user1", "user2", 100]'
```

### Example 1a: Debug with Map Arguments

Pass JSON objects as Map arguments:

```bash
# Flat map argument
soroban-debug run \
  --contract token.wasm \
  --function update_user \
  --args '{"user":"ABC","balance":1000}'

# Nested map argument
soroban-debug run \
  --contract token.wasm \
  --function update_user \
  --args '{"user":"ABC","balance":1000,"metadata":{"verified":true,"level":"premium"}}'

# Mixed-type values in map
soroban-debug run \
  --contract dao.wasm \
  --function create_proposal \
  --args '{"title":"Proposal 1","votes":42,"active":true,"tags":["important","urgent"]}'
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

## Supported Argument Types

The debugger supports passing typed arguments to contract functions via the `--args` flag. You can use **bare values** for quick usage or **type annotations** for precise control.

### Bare Values (Default Types)

| JSON Value | Soroban Type | Example            |
| ---------- | ------------ | ------------------ |
| Number     | `i128`       | `10`, `-5`, `999`  |
| String     | `Symbol`     | `"hello"`          |
| Boolean    | `Bool`       | `true`, `false`    |
| Array      | `Vec<Val>`   | `[1, 2, 3]`        |
| Object     | `Map`        | `{"key": "value"}` |

```bash
# Bare values (numbers default to i128, strings to Symbol)
soroban-debug run --contract counter.wasm --function add --args '[10]'
soroban-debug run --contract token.wasm --function transfer --args '["Alice", "Bob", 100]'
```

### Type Annotations

For precise type control, use `{"type": "<type>", "value": <value>}`:

| Type     | Description                | Example                                    |
| -------- | -------------------------- | ------------------------------------------ |
| `u32`    | Unsigned 32-bit integer    | `{"type": "u32", "value": 42}`             |
| `i32`    | Signed 32-bit integer      | `{"type": "i32", "value": -5}`             |
| `u64`    | Unsigned 64-bit integer    | `{"type": "u64", "value": 1000000}`        |
| `i64`    | Signed 64-bit integer      | `{"type": "i64", "value": -999}`           |
| `u128`   | Unsigned 128-bit integer   | `{"type": "u128", "value": 100}`           |
| `i128`   | Signed 128-bit integer     | `{"type": "i128", "value": -100}`          |
| `bool`   | Boolean value              | `{"type": "bool", "value": true}`          |
| `symbol` | Soroban Symbol (≤32 chars) | `{"type": "symbol", "value": "hello"}`     |
| `string`  | Soroban String (any len)   | `{"type": "string", "value": "long text"}` |
| `address` | Soroban Address (Contract/Acc) | `{"type": "address", "value": "C..."}`     |

```bash
# Typed arguments for precise control
soroban-debug run --contract counter.wasm --function add --args '[{"type": "u32", "value": 10}]'

# Mixed typed and bare values
soroban-debug run --contract token.wasm --function transfer \
  --args '[{"type": "symbol", "value": "Alice"}, {"type": "symbol", "value": "Bob"}, {"type": "u64", "value": 100}]'

# Soroban String for longer text
soroban-debug run --contract dao.wasm --function create_proposal \
  --args '[{"type": "string", "value": "My proposal title"}]'

# Address type (contract or account addresses)
soroban-debug run --contract token.wasm --function balance_of \
  --args '[{"type": "address", "value": "GD3IYSAL6Z2A3A4A3A4A3A4A3A4A3A4A3A4A3A4A3A4A3A4A3A4A3A4A"}]'

# Bare address (auto-detected if starts with C or G and is 56 chars)
soroban-debug run --contract token.wasm --function transfer \
  --args '["CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADUI", "GBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB", 100]'
```

### Error Handling

The parser provides clear error messages for common issues:

- **Unsupported type**: `Unsupported type: bytes. Supported types: u32, i32, u64, i64, u128, i128, bool, string, symbol, address`
- **Out of range**: `Value out of range for type u32: 5000000000 (valid range: 0..=4294967295)`
- **Type mismatch**: `Type/value mismatch: expected u32 (non-negative integer) but got "hello"`
- **Invalid JSON**: `JSON parsing error: ...`

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

## Configuration File

The debugger supports loading default settings from a `.soroban-debug.toml` file in the project root. CLI flags always override settings defined in the configuration file.

### Example `.soroban-debug.toml`

```toml
[debug]
# Default breakpoints to set
breakpoints = ["verify", "auth"]

[output]
# Show events by default
show_events = true
```

### Supported Settings

| Setting       | Path                 | Description                                        |
| ------------- | -------------------- | -------------------------------------------------- |
| `breakpoints` | `debug.breakpoints`  | List of function names to set as breakpoints       |
| `show_events` | `output.show_events` | Whether to show events by default (`true`/`false`) |

## Accessibility

The CLI supports **screen-reader compatible** and **low-complexity** output so that all information is conveyed via text, not only color or Unicode symbols.

- **`NO_COLOR`**  
  If the `NO_COLOR` environment variable is set and not empty, the debugger disables all ANSI color output. Status is then shown with text labels (e.g. `[PASS]`, `[FAIL]`, `[INFO]`, `[WARN]`) instead of colored text.

- **`--no-unicode`**  
  Use ASCII-only output: no Unicode box-drawing characters (e.g. `┌`, `─`, `│`) or symbols. Box-drawing is replaced with `+`, `-`, `|`; bullets and arrows use `*` and `>`. Spinners are replaced with static text such as `[WORKING...]`.

**Example (screen reader friendly):**

```bash
NO_COLOR=1 soroban-debug run --contract app.wasm --function main --no-unicode
```

For best compatibility with screen readers, set both `NO_COLOR` and use `--no-unicode`.

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

## Benchmarks

The project includes a benchmark suite using [Criterion.rs](https://github.com/bheisler/criterion.rs) to track performance and prevent regressions.

To run the full benchmark suite:

```bash
cargo bench
```

### Baseline Results (v0.1.0)

| Component | Operation | Time (Baseline) |
|-----------|-----------|-----------------|
| Runtime | WASM Loading (100KB) | ~15 µs |
| Runtime | Breakpoint Check (100 set) | ~20 ns |
| Runtime | Call Stack Push/Pop | ~50 ns |
| Parser | Argument Parsing (Complex) | ~100 µs |
| Inspector | Storage Snapshot (1000 items) | ~0.5 ns |
| Inspector | Storage Diff (1000 items) | ~60 µs |

Benchmarks are run automatically in CI to ensure performance stays within acceptable bounds.

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

### Phase 3 (Current)
- Source map support for Rust debugging
- Time-travel debugging (step back)
- Visual execution timeline
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

## Docker

### Build Locally

```bash
docker build -t soroban-debugger:local .
```

### Run with a Mounted WASM

```bash
docker run --rm -v "$(pwd):/contracts" ghcr.io/your-org/soroban-debug run --contract /contracts/token.wasm --function transfer
```

### Interactive Mode (TTY)

```bash
docker run --rm -it -v "$(pwd):/contracts" ghcr.io/your-org/soroban-debug interactive --contract /contracts/token.wasm
```

### Docker Compose

```bash
docker compose run --rm soroban-debug run --contract /contracts/token.wasm --function transfer
```
