# CLI Command Groups

The Soroban Debugger CLI provides a wide range of commands to support different stages of the smart contract development lifecycle. To make these tools easier to navigate, they are organized into four main workflow categories.

---

## 🏃 Run and Debug
Commands used for executing contract functions and interactive troubleshooting.

- **`run`**: The primary entry point for executing a function. Use this for single runs, batch testing, or generating trace exports.
- **`interactive`**: Starts a step-by-step terminal debugger. Use this when you need to walk through code line-by-line.
- **`repl`**: Opens a persistent session for repeated calls. Ideal for exploring contract state changes across multiple invocations.
- **`tui`**: A full-screen dashboard for a more visual debugging experience.
- **`scenario`**: Runs complex, multi-step integration tests defined in TOML.
- **`replay`**: Reproduces a previous execution from a trace file.

## 🔍 Analyze and Compare
Commands for static analysis, profiling, and version comparisons.

- **`inspect`**: View contract metadata, exported functions, and DWARF source mappings without executing code.
- **`upgrade-check`**: Compares two WASM files and reports upgrade classes based on exported function compatibility and optional execution diffs. See [Upgrade Classes](upgrade-classes.md) for the exact mapping.
- **`optimize`**: Provides automated suggestions for reducing gas (CPU/memory) consumption.
- **`profile`**: Identifies performance hotspots and budget-heavy instruction sequences.
- **`compare`**: Renders a side-by-side diff of two execution traces to catch regressions.
- **`symbolic`**: Uses symbolic execution to automatically discover inputs that trigger panics or edge cases.
- **`analyze`**: Runs security-focused linting rules against the contract.

## 🌐 Remote and Server
Commands for distributed debugging and CI integration.

- **`server`**: Hosts a debugging session that can be connected to by remote clients (CLI or VS Code).
- **`remote`**: Connects to an existing server to perform debugging tasks.

## 🛠️ Developer Utilities
Helper tools for environment setup and maintenance.

- **`completions`**: Generates shell completion scripts for Bash, Zsh, Fish, and PowerShell.
- **`history-prune`**: Manages the local execution history database to prevent unbounded growth.

---

## 💡 Workflow Recommendation

1. **Development**: Use `run` and `interactive` for local testing.
2. **Hardening**: Use `symbolic` to find edge cases, then capture them in `scenario` files.
3. **CI/CD**: Run `scenario` and `batch-args` to prevent regressions.
4. **Optimization**: Use `profile` and `optimize` to minimize contract fees before deployment.
