# Soroban Debugger FAQ

This page covers common questions, confusing behaviors, and troubleshooting tips for the Soroban Debugger (`soroban-debug`).

## Categories
- [Installation](#installation)
- [Running Contracts](#running-contracts)
- [Breakpoints](#breakpoints)
- [Budget](#budget)
- [Output and Trace](#output-and-trace)
- [Argument Parsing](#argument-parsing)
- [CLI vs VS Code Extension](#cli-vs-vs-code-extension---feature-differences)
- [Local and CI Environment](#local-and-ci-environment)

---

## Installation

### 1. `cargo install` fails with "linker 'cc' not found"
**Cause:** Your system lacks the necessary build tools (C compiler and linker) required to compile Rust dependencies.
**Fix:**
- **Windows:** Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) and select the "Desktop development with C++" workload.
- **Linux:** Install `build-essential` (Ubuntu/Debian) or `base-devel` (Arch).
- **macOS:** Run `xcode-select --install`.

### 2. Can't access `man` pages after installation
**Cause:** `cargo install` only installs the binary, not the manual pages.
**Fix:** If building from source, manually copy the man pages:
```bash
sudo cp man/man1/soroban-debug* /usr/local/share/man/man1/
```
Then use `man soroban-debug`.

### 3. Error: "Rust 1.75 or later required"
**Cause:** The project uses modern Rust features.
**Fix:** Update your Rust toolchain:
```bash
rustup update
```

---

## Running Contracts

### 4. WASM load failure: "No such file or directory"
**Cause:** The path provided to `--contract` is incorrect or relative to a different directory.
**Fix:** Ensure the path is correct relative to your current working directory. Use an absolute path if unsure.
```bash
soroban-debug run --contract ./target/wasm32-unknown-unknown/release/my_contract.wasm ...
```

### 5. "Function not found" error
**Cause:** The function name specified with `--function` is not exported by the WASM contract or is misspelled.
**Fix:** Use the `inspect` command to see all available exported functions:
```bash
soroban-debug inspect --contract my_contract.wasm
```

### 6. Contract panics with "Host error: Unknown error"
**Cause:** This usually happens when an assertion fails or an unexpected state is reached during execution.
**Fix:** Start an interactive session to step through the code and identify the exact instruction causing the panic:
```bash
soroban-debug interactive --contract my_contract.wasm
```

### 7. Watch mode (`--watch`) doesn't reload when I save my Rust code
**Cause:** Watch mode monitors the **WASM file**, not your Rust source files.
**Fix:** You need a separate process (like `cargo watch`) to rebuild your WASM file. Once the WASM file is updated on disk, `soroban-debug` will detect the change and re-run.

---

## Breakpoints

### 8. Breakpoints are not triggering
**Cause:** You might be setting a breakpoint on a function that is never called, or the function name is slightly different (e.g., due to name mangling, though Soroban usually keeps them clean).
**Fix:** Verify the function name using `soroban-debug inspect`. In `interactive` mode, use `list-breaks` to ensure your breakpoints are registered.

### 9. Can I set a breakpoint on a specific line number?
**Answer:** Currently, the debugger supports setting breakpoints only at **function boundaries**.
**Workaround:** Set a breakpoint at the function containing the line, then use `s` (step) or `n` (next) to reach the specific line you're interested in.

### 10. Why does VS Code show `verified=false` but the breakpoint still hits?
**Cause:** Source verification and runtime binding are different decisions in the adapter.
**Meaning:**
- `verified=false` means an exact source map proof was not available.
- `setBreakpoint=true` means the adapter still bound a runtime function breakpoint.
- `HEURISTIC_NO_DWARF` means DWARF source mapping was unavailable and the adapter used heuristic function mapping.

**Example:** A breakpoint on `lib.rs:10` may return `verified=false` with `HEURISTIC_NO_DWARF`, yet still pause at runtime if that line is inside an exported entrypoint function.

---

## Budget

### 11. Why am I getting "Warning: High CPU usage detected"?
**Cause:** The contract has consumed a significant portion of the Soroban CPU budget.
**Fix:** Optimize expensive loops, reduce deep recursion, or minimize complex storage operations. Use the `budget` command in interactive mode to see which parts of your code are the most "expensive".

### 12. "Budget exceeded" error during debugging
**Cause:** The execution hit the maximum allowed Soroban resource limits.
**Fix:** Check for infinite loops or extremely inefficient algorithms. You can also try to provide a larger initial budget if your local environment allows (though on-chain limits will still apply).

### 13. Debugger budget numbers don't match exactly with on-chain execution
**Cause:** The debugger environment might have slight overhead or use a different version of the Soroban host than the network you are targeting.
**Fix:** Use budget numbers as a relative guide for optimization rather than an absolute guarantee for on-chain costs.

---

## Argument Parsing

### 14. My JSON arguments are failing to parse
**Cause:** Shell quoting issues are common. If your JSON contains double quotes, the shell might be stripping them.
**Fix:** Wrap the entire JSON string in single quotes:
```bash
soroban-debug run --args '["Alice", "Bob", 100]'
```

### 15. Error: "Type/value mismatch: expected u32 but got 5000000000"
**Cause:** The value provided exceeds the range of the target type (e.g., `u32` max is ~4.29 billion).
**Fix:** Ensure your input fits within the specified type, or use a larger type like `u64` or `i128` (default).

### 16. How do I pass a Soroban Address as an argument?
**Answer:** Use the explicit type annotation for addresses.
**Fix:**
```json
{"type": "address", "value": "CCV6S6F6..."}
```
Or, if it's a 56-character string starting with 'C' or 'G', the debugger will often auto-detect it as an Address.

---

## Output and Trace

### 17. The trace file is too large and hard to read
**Cause:** Exporting every storage change and event can lead to huge JSON files.
**Fix:** Use `--storage-filter` to only include the keys you care about in the output, which will also reduce the trace size.
```bash
soroban-debug run --trace-output trace.json --storage-filter 'balance:*'
```

### 18. The terminal output looks garbled or has weird characters
**Cause:** Your terminal might not support Unicode box-drawing characters or ANSI colors.
**Fix:** Use the `--no-unicode` flag and set the `NO_COLOR=1` environment variable:
```bash
NO_COLOR=1 soroban-debug run --no-unicode ...
```

---

## CLI vs VS Code Extension - Feature Differences

### 19. A feature works in the CLI but is not available in the VS Code extension (or vice versa)

**Answer:** The CLI and the VS Code extension do not have full feature parity. The CLI exposes the complete debugger surface; the extension exposes a focused subset via the Debug Adapter Protocol (DAP).

The authoritative reference is the **[Feature Matrix](feature-matrix.md)**. It lists every feature, which surface supports it, and any relevant limitations.

Key asymmetries at a glance:

- **CLI-only features:** instruction-level stepping (`--instruction-debug`, `--step-instructions`, `--step-mode`), storage filters (`--storage-filter`), auth tree display (`--show-auth`), batch execution (`--batch-args`, `--repeat`), remote client mode (`soroban-debug remote`), TLS configuration, storage export (`--export-storage`), event filtering (`--show-events`, `--event-filter`), dry-run mode (`--dry-run`), cross-contract mocking (`--mock`), and all analysis subcommands (`analyze`, `symbolic`, `optimize`, `profile`, `compare`, `replay`, `upgrade-check`, `scenario`, `tui`, `repl`).
- **Extension-only features:** hover evaluation (expression evaluation on mouse-hover while paused).
- **Shared features:** function breakpoints, step in/over/out, continue, call stack inspection, variable and storage inspection when paused, expression evaluation in the Debug Console.

**Which surface to use:**

- Use the **VS Code extension** for a visual IDE experience: set breakpoints by clicking, inspect variables in the sidebar, navigate the call stack with keyboard shortcuts.
- Use the **CLI** for full debugging power: instruction-level stepping, storage filtering, auth analysis, batch runs, remote/CI scenarios, and any of the analysis subcommands.

### 20. The VS Code extension shows all storage keys but I only want to see a subset

**Cause:** Storage filtering via `--storage-filter` is not exposed in the extension's launch configuration. All storage keys are shown unfiltered in the Variables panel.

**Workaround:** Either run `soroban-debug run --storage-filter '<pattern>'` from the terminal to get a targeted view, or use `snapshotPath` in `launch.json` to provide a pre-filtered initial storage state. See the [Feature Matrix — Storage Filters](feature-matrix.md#storage-filters) for details.

### 21. I want to debug a contract on a remote server from VS Code

**Cause:** The VS Code extension only connects to a debug server it spawns locally as a subprocess. The `soroban-debug remote` client mode is not exposed through the extension.

**Workaround:** Use an SSH tunnel to bridge the remote server to your local machine:
```bash
# On the remote machine
soroban-debug server --port 9229 --token $MY_TOKEN

# On your local machine (in a separate terminal)
ssh -L 9229:localhost:9229 user@remote-host
```
Then set `"port": 9229` and `"token": "$MY_TOKEN"` in your `launch.json`. The extension will connect to the tunnel as if the server were local.

For full remote debugging documentation, see [Remote Debugging](remote-debugging.md) and the [Feature Matrix — Remote Debugging](feature-matrix.md#remote-debugging).

---

## History Retention

### 22. The history file keeps growing. How do I limit it?

**Cause:** `soroban-debug` appends one record per `run` invocation. Without a configured limit the history file grows indefinitely.

**Fix:** Use the global `--history-max-records` flag (or its environment variable) to cap how many records are kept:

```bash
# Keep only the 100 most-recent runs
soroban-debug --history-max-records 100 run --contract my.wasm --function increment

# Use the env var for a persistent per-shell default
export SOROBAN_DEBUG_HISTORY_MAX_RECORDS=100
soroban-debug run --contract my.wasm --function increment
```

The pruning happens atomically during the append — the same tmp-file-rename mechanism that prevents corruption on normal writes.

---

### 23. How do I drop records that are older than a certain number of days?

Use `--history-max-age-days` (or `SOROBAN_DEBUG_HISTORY_MAX_AGE_DAYS`):

```bash
# Drop runs older than 30 days on every append
soroban-debug --history-max-age-days 30 run --contract my.wasm --function increment

# Persist the policy in the environment
export SOROBAN_DEBUG_HISTORY_MAX_AGE_DAYS=30
```

Both constraints can be combined — the stricter one wins:

```bash
# Keep at most 50 records AND discard anything older than 14 days
soroban-debug \
  --history-max-records 50 \
  --history-max-age-days 14 \
  run --contract my.wasm --function increment
```

---

### 24. How do I prune or compact the history file without running a contract?

Use the `history-prune` subcommand:

```bash
# Prune to the 200 most-recent records
soroban-debug history-prune --max-records 200

# Drop records older than 30 days
soroban-debug history-prune --max-age-days 30

# Combine: keep newest 200 and drop anything older than 30 days
soroban-debug history-prune --max-records 200 --max-age-days 30
```

**Dry-run mode** — preview what would be removed without writing any changes:

```bash
soroban-debug history-prune --max-records 50 --dry-run
# [dry-run] Would remove 143 record(s), 50 would remain.
```

The subcommand also honours the global `--history-file` flag:

```bash
soroban-debug --history-file /path/to/custom-history.json history-prune --max-records 100
```

---

### 25. Which records are kept when `--history-max-records` is used?

The **newest** N records (by their parsed `date` field, sorted chronologically) are kept; the oldest are removed. This preserves deterministic ordering for `--budget-trend` regression analysis.

Records whose `date` field cannot be parsed are **kept** rather than silently dropped, to avoid data loss from formatting differences.

---

## Error Hints and JSON Output

### 26. How do I interpret standardized error hints?

The Soroban Debugger provides standardized remediation hints for most common failures. When an error like an incorrect WASM path or a bad port connection occurs, the debugger will print an actionable diagnostic:

**Example:**
```
  × Network/transport error: Failed to connect to 127.0.0.1:9000: Connection refused (os error 61)
  help: Action: Ensure the remote debug server is online, address is correct, and network firewall permits the connection.
        Context: The transport connection failed to establish or dropped unexpectedly.
```

If you specify `--json` or set `SOROBAN_DEBUG_JSON=1`, these hints are also securely placed inside a machine-readable `"hints"` array on the output block, allowing your scripts or testing wrappers to automatically process validation suggestions.

```json
{
  "status": "error",
  "errors": [
    "Authentication failed: Invalid security token"
  ],
  "hints": [
    "Action: Ensure the shared security token matches the server, and the transport protocol is correct.\nContext: The server rejected communication because authentication wasn't verified."
  ]
}
```

---

## Local and CI Environment

### 27. I'm getting `listen EPERM` or `mktemp` failures in my CI environment
**Answer:** These are often caused by environment restrictions in sandboxed CI runners or missing permissions for temp directories.
**Fix:** See the [Local and CI Sandbox Failures](remote-troubleshooting.md#local-and-ci-sandbox-failures) section in the troubleshooting guide for a matrix of common failures and their fixes.
