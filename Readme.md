# Soroban Debugger

[![CI](https://github.com/Timi16/soroban-debugger/actions/workflows/ci.yml/badge.svg)](https://github.com/Timi16/soroban-debugger/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/Timi16/soroban-debugger/branch/main/graph/badge.svg)](https://codecov.io/gh/Timi16/soroban-debugger)
[![Latest Release](https://img.shields.io/github/v/release/Timi16/soroban-debugger?logo=github)](https://github.com/Timi16/soroban-debugger/releases)

A command-line debugger for Soroban smart contracts on the Stellar network. Debug your contracts interactively with breakpoints, step-through execution, state inspection, and budget tracking.

## Table of Contents
- [Quick Start](#quick-start)
- [User Journeys](#user-journeys)
- [Command Index](#command-index)
- [Reference](#reference)
  - [Supported Argument Types](#supported-argument-types)
  - [Storage Filtering](#storage-filtering)
  - [Exporting Execution Traces](#exporting-execution-traces)
- [Interactive Commands](#interactive-command)
- [Examples](#examples)
- [Troubleshooting](#troubleshooting)
- [Contributing](#contributing)
- [License](#license)

---

## Quick Start

### 1. Installation

#### Using Cargo (Recommended)
```bash
cargo install --locked soroban-debugger
```

> The `--locked` flag pins dependency versions to those tested by the maintainers, ensuring a reproducible install.

#### From Source
```bash
git clone https://github.com/Timi16/soroban-debugger.git
cd soroban-debugger
cargo install --locked --path .
```

### 2. Your First Debug Run

Debug a contract by specifying the WASM file and function to execute:

```bash
soroban-debug run --contract token.wasm --function transfer --args '["Alice", "Bob", 100]'
```

For an interactive session with a terminal UI:
```bash
soroban-debug interactive --contract my_contract.wasm --function hello
```

For a comprehensive introduction, see the [Getting Started Guide](docs/getting-started.md).

---

## User Journeys

### 🛠️ Debugging Your First Contract
Learn how to execute functions, pass complex arguments, and use the interactive debugger.

- **Basic Execution**: Use the `run` command to execute functions and see results immediately.
- **Interactive Mode**: Use `interactive` for a step-by-step walkthrough with breakpoints.
- **REPL**: Use `repl` for repeated calls and exploration without restarting.
- **Complex Arguments**: Support for JSON-nested vectors, maps, and [typed annotations](#typed-annotations).

### 🔍 Source-Level Debugging
Debug your Rust code directly instead of raw WASM instructions.

- **Rust Source Mapping**: Automatically maps WASM offsets back to Rust lines using DWARF debug info.
- **Instruction Stepping**: Step into, over, or out of functions at the source level.
- **Source Map Caching**: Fast O(1) lookups for source locations after the first load.

See [Source-Level Debugging Guide](docs/source-level-debugging.md) for details.

### 🌐 Remote Debugging Sessions
Debug contracts running on remote servers or in CI environments.

- **Debug Server**: Start a `server` process to host a debugging session.
- **Remote Client**: Connect to a running server using the `remote` command.
- **Secure Connections**: Support for TLS and token-based authentication.

See [Remote Debugging Guide](docs/remote-debugging.md) for setup instructions.

### 📈 Analysis & Optimization
Analyze contract metadata, resource usage, and upgrade compatibility.

- **Inspection**: Use `inspect` to view contract functions and metadata without executing.
- **Profiling**: Use `profile` to find hotspots and budget-heavy execution paths.
- **Optimization**: Use `optimize` for automated gas and performance suggestions.
- **Upgrade Checks**: Use `upgrade-check` to ensure API compatibility between versions.

### 🤖 Regression & Automated Testing
Integrate debugging into your CI/CD pipeline and discover edge cases.

- **Scenarios**: Define multi-step integration tests in simple [TOML files](docs/tutorials/scenario-runner.md).
- **Batch Execution**: Run the same function with [multiple argument sets](docs/batch-execution.md) in parallel.
- **Symbolic Analysis**: Automatically explore input spaces to find panics and edge cases.
- **Test Generation**: Generate ready-to-run Rust unit tests from any debug session.

---

## Command Index

| Category | Commands |
| --- | --- |
| **Run & Debug** | `run`, `interactive`, `repl`, `tui`, `scenario`, `replay` |
| **Analyze & Compare** | `inspect`, `upgrade-check`, `optimize`, `profile`, `compare`, `symbolic`, `analyze` |
| **Remote & Server** | `server`, `remote` |
| **Utilities** | `completions`, `history-prune` |

> Use `soroban-debug <command> --help` for full flags and examples.

---

## Reference

### Supported Argument Types

The debugger supports passing typed arguments via the `--args` flag.

| JSON Value | Soroban Type | Example |
| --- | --- | --- |
| Number | `i128` | `10`, `-5` |
| String | `Symbol` | `"hello"` |
| Boolean | `Bool` | `true` |
| Array | `Vec<Val>` | `[1, 2, 3]` |
| Object | `Map` | `{"key": "value"}` |

#### Typed Annotations
For precise control, use `{"type": "...", "value": ...}`:
`u32`, `i32`, `u64`, `i64`, `u128`, `i128`, `bool`, `symbol`, `string`, `address`.

### Storage Filtering

Filter large storage outputs by key pattern using `--storage-filter`:

| Pattern          | Type   | Matches                                |
|------------------|--------|----------------------------------------|
| `balance:*`      | Prefix | Keys starting with `balance:`          |
| `re:^user_\d+$`  | Regex  | Keys matching the regex                |
| `total_supply`   | Exact  | Only the key `total_supply`            |

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
  --contract contract.wasm \
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
      --source-map-diagnostics
                            Print resolved mappings, missing DWARF sections, and fallback behavior
      --dependency-graph     Export cross-contract dependency graph (DOT + Mermaid)
```

Use `soroban-debug inspect --contract my_contract.wasm --source-map-diagnostics --format json`
when you want a non-interactive DWARF triage report for CI or editor tooling.

For full examples, see [docs/dependency-graph.md](https://github.com/Timi16/soroban-debugger/blob/main/docs/dependency-graph.md).

### Upgrade Check Command

Compare two contract binaries for API breakage and execution differences before releasing:

```bash
soroban-debug upgrade-check --old current.wasm --new upgraded.wasm
```

The debugger runs parallel traces and classifies the upgrade:
- **Safe:** The exported surface is unchanged, and sampled execution matches.
- **Caution:** New exported functions were added without changing existing signatures.
- **Breaking:** Functions were removed, signatures changed, or sampled execution diverged.

See [docs/upgrade-classes.md](docs/upgrade-classes.md) for the full mapping between contract changes and upgrade classes.

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

See [`doc/compare.md`](https://github.com/Timi16/soroban-debugger/blob/main/docs/doc/compare.md) for the full trace JSON format reference
and a regression testing workflow guide.

## Examples

For a comprehensive overview of available examples mapped to debugger concepts (auth, storage, plugins, etc.), see the **[Examples Index](examples/README.md)**.

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
soroban-debug run --contract token.wasm --function mint --storage-filter 'balance:*'
```
Supports `prefix*`, `re:<regex>`, and `exact_match`.

### Configuration File
Load default settings from `.soroban-debug.toml`:
```toml
[debug]
breakpoints = ["verify", "auth"]
[output]
show_events = true
```

---

## Troubleshooting

| Symptom | Likely Cause | Solution |
| --- | --- | --- |
| Request timed out | Slow host or low timeout | Increase `--timeout-ms`, or use `--inspect-timeout-ms` / `--storage-timeout-ms` for slower read-only requests. For flaky links, consider the `--retry-*` flags. |
| Incompatible protocol | Build version mismatch | Reinstall client/server from same release |
| Auth failed | Token mismatch | Verify `--token` values match |

For more scenarios, see the [Full FAQ](docs/faq.md) and [Remote Troubleshooting Guide](docs/remote-troubleshooting.md).

---

## Contributing
Please see [CONTRIBUTING.md](CONTRIBUTING.md) for setup and workflow.

## License
Licensed under [Apache 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT).
