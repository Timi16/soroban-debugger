# Watch Mode

The `--watch` flag enables automatic reloading and re-execution when the target WASM file changes. This is particularly useful during development when you're iterating on contract code.

## Usage

```bash
soroban-debug run --contract path/to/contract.wasm --function my_function --watch
```

## Features

- **Automatic Reload**: Monitors the WASM file for changes and automatically reloads and re-executes
- **Debouncing**: Events are debounced (~500ms) to avoid repeated triggers during file saves
- **Clean Output**: Terminal is cleared on each run to show fresh output
- **Error Handling**: Errors don't kill watch mode - it continues waiting for the next change
- **Clean Exit**: Press Ctrl+C to exit cleanly

## Example

```bash
# Watch a contract and re-run on changes
soroban-debug run \
  --contract target/wasm32-unknown-unknown/release/my_contract.wasm \
  --function transfer \
  --args '["user1", "user2", 100]' \
  --watch

# Output:
# Starting watch mode for: "target/wasm32-unknown-unknown/release/my_contract.wasm"
# Press Ctrl+C to exit
#
# --- Initial Run ---
#
# --- Execution Start ---
#
# --- Execution Complete ---
# Result: Ok(...)
#
# Waiting for changes...
```

When you rebuild your contract (e.g., `cargo build --release`), the debugger will automatically detect the change and re-run:

```
File changed: "target/wasm32-unknown-unknown/release/my_contract.wasm"
Re-running...

--- Execution Start ---

--- Execution Complete ---
Result: Ok(...)

Waiting for changes...
```

## Workflow Integration

Watch mode integrates seamlessly with your development workflow:

1. Start the debugger in watch mode in one terminal
2. Edit your contract code in your editor
3. Build the contract (`cargo build --release`)
4. See the results immediately in the debugger terminal

## Options

All standard `run` command options work with `--watch`:

```bash
soroban-debug run \
  --contract contract.wasm \
  --function my_function \
  --args '[1, 2, 3]' \
  --storage '{"key": "value"}' \
  --show-events \
  --show-auth \
  --watch
```

## Implementation Details

- Uses the `notify` crate for efficient file system monitoring
- Watches the parent directory (more reliable than watching a single file)
- Debounces events to handle multiple file system events during a single save
- Clears terminal using ANSI escape codes for clean output
- Handles Ctrl+C gracefully using the `ctrlc` crate
- Errors during execution are displayed but don't terminate watch mode

## Limitations

- Watch mode is not compatible with `--batch-args` (batch execution mode)
- Interactive features like `--step-instructions` are not supported in watch mode
- The TUI dashboard cannot be used with watch mode
