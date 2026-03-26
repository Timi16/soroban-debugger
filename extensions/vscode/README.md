# Soroban Debugger Extension

A Visual Studio Code extension that integrates the Soroban smart contract debugger via the Debug Adapter Protocol (DAP).

## Features

- **Launch Preflight Command**: Validate a Soroban launch configuration from the command palette without starting the backend
- **Debugger Version Info**: Shows backend and protocol versions in the status bar and Soroban Debugger output panel during sessions

- 🔍 **Breakpoint Management**: Set, clear, and manage breakpoints directly in the VS Code editor
- 📊 **Variable Inspection**: View and inspect contract storage state in the Variables panel
- 📚 **Call Stack Visualization**: Examine the function call stack during execution
- 🧵 **Thread Support**: Basic thread management for debugging sessions
- 📝 **Detailed Logging**: Optional trace logging for debugging adapter interactions
- ⚡ **Real-time Debugging**: Step through contract execution with next, step in, and step out

## Requirements

- Visual Studio Code 1.75.0 or higher
- Node.js 18+ (for extension development)
- `soroban-debug` CLI built from this repository or installed in your PATH
- Rust toolchain with `wasm32-unknown-unknown` target

## Installation

### From Source

1. Clone the soroban-debugger repository:

```bash
git clone https://github.com/stellar/soroban-debugger.git
cd soroban-debugger
```

2. Navigate to the extension directory:

```bash
cd extensions/vscode
```

3. Install dependencies:

```bash
npm install
```

4. Compile the extension:

```bash
npm run compile
```

5. Open VS Code and load the extension:
   - Press `Ctrl+Shift+P` (or `Cmd+Shift+P` on macOS)
   - Select "Extensions: Install from VSIX..."
   - Navigate to the extension directory and select it

### From Marketplace (Coming Soon)

The extension will be published to the VS Code Marketplace. Once available, search for "Soroban Debugger" in the Extensions panel.

## Quick Start

### 1. Create a Debug Configuration

Add the following to your project's `.vscode/launch.json`:

```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "name": "Soroban: Debug Contract",
      "type": "soroban",
      "request": "launch",
      "contractPath": "${workspaceFolder}/target/wasm32-unknown-unknown/release/contract.wasm",
      "snapshotPath": "${workspaceFolder}/snapshot.json",
      "entrypoint": "main",
      "args": [],
      "trace": false,
      "binaryPath": "${workspaceFolder}/target/debug/soroban-debug"
    }
  ]
}
```

### 1a. Run Launch Preflight

Before starting a debug session, you can validate the Soroban launch configuration directly from the command palette:

1. Press `Ctrl+Shift+P` (or `Cmd+Shift+P` on macOS)
2. Run `Soroban: Run Launch Preflight`
3. Pick the Soroban launch configuration you want to validate when prompted

If preflight finds a problem, the extension reports the issue and offers quick fixes such as opening `launch.json`, generating a launch config, or selecting a missing file.

### 2. Build Your Contract

Ensure your contract is compiled to WASM:

```bash
cargo build --target wasm32-unknown-unknown --release
```

### 3. Prepare a Snapshot

Create a `snapshot.json` file with the initial state for your debugger session. See [examples/snapshot.json](../../examples/snapshot.json) for the format.

### 4. Start Debugging

1. Open your contract source code in VS Code
2. Set breakpoints by clicking on the line numbers
3. Select "Soroban: Debug Contract" from the debug configuration dropdown
4. Press F5 or click the Run button to start debugging

## Debug Configuration Options

### Required Parameters

- **contractPath** (string): Path to the compiled WASM contract file
  - Default: `${workspaceFolder}/target/wasm32-unknown-unknown/release/contract.wasm`

### Optional Parameters

- **snapshotPath** (string): Path to the snapshot JSON file containing the initial state
  - Default: `${workspaceFolder}/snapshot.json`

- **entrypoint** (string): The contract function to debug
  - Default: `main`

- **args** (array): Arguments to pass to the contract function
  - Default: `[]`
  - Example: `["arg1", "arg2"]`

- **trace** (boolean): Enable detailed trace logging for debugging the adapter itself
  - Default: `false`

- **binaryPath** (string): Optional path to the `soroban-debug` binary
  - Default: resolved from `${workspaceFolder}/target/debug/soroban-debug`, then PATH

- **requestTimeoutMs** (number): Per-request timeout (wire protocol) before failing the session as unhealthy
  - Default: `30000`
  - Tip: If you’re debugging on a slower machine/CI, increase this.

- **connectTimeoutMs** (number): Timeout to wait for the backend server to accept connections on startup
  - Default: `10000`

### Environment Overrides (Advanced)

If you can’t (or don’t want to) set timeouts in `launch.json`, you can also use:

- `SOROBAN_DEBUG_REQUEST_TIMEOUT_MS`
- `SOROBAN_DEBUG_CONNECT_TIMEOUT_MS`

## Usage Guide

### Setting Breakpoints

1. Click on the line number in your source code to set a breakpoint
2. A red dot will appear when the breakpoint is set
3. Breakpoints are managed in the Breakpoints panel on the left sidebar

### Inspecting Variables

When execution is paused:

1. Open the **Run and Debug** panel (Ctrl+Shift+D)
2. Expand the **Variables** section to see contract storage state
3. Hover over variables to see detailed information

#### Large / Nested Values

- Arrays and objects expand lazily and are paginated with an explicit `… show more` entry to avoid freezing the UI.
- Long string values are truncated with a `(truncated, expand)` hint; expanding reveals the full value.
- Typed argument annotations like `{"type":"bytes","value":"0x..."}` render as `bytes(n)` previews; expanding shows hex/base64/utf8 details.

### Using the Call Stack

The **Call Stack** panel shows:

- Current function being executed
- Parent function context
- Line and column information
- Click any frame to jump to that location in your code

### Step Controls

Use the following keyboard shortcuts:

- **F10** or **Step Over**: Execute the next line without stepping into functions
- **F11** or **Step In**: Step into the next function call
- **Shift+F11** or **Step Out**: Continue execution until the current function returns
- **F5** or **Continue**: Resume execution until the next breakpoint
- **Shift+F5** or **Stop**: Terminate the debugging session

## Feature Limitations

The VS Code extension exposes a focused subset of the full `soroban-debug` CLI.
The following features are **not available** in the extension.

### Not supported in the extension

| CLI feature                 | CLI flag                                                                                       | Workaround                                                             |
| --------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| Instruction-level stepping  | `--instruction-debug`, `--step-instructions`, `--step-mode [block]`                            | Use `soroban-debug interactive --instruction-debug` in a terminal      |
| Storage key filtering       | `--storage-filter <pattern>`                                                                   | All storage is shown unfiltered in the Variables panel; filter via CLI |
| Auth tree display           | `--show-auth`                                                                                  | Use `soroban-debug run --show-auth` in a terminal                      |
| Batch execution             | `--batch-args <file>`, `--repeat N`                                                            | Use `soroban-debug run --batch-args` in a terminal                     |
| Remote client mode          | `soroban-debug remote --remote host:port`                                                      | Use CLI; see [Remote Debugging](../../docs/remote-debugging.md)        |
| TLS configuration           | `--tls-cert`, `--tls-key`                                                                      | Use CLI server/remote commands directly                                |
| Storage export              | `--export-storage <file>`                                                                      | Use `soroban-debug run --export-storage` in a terminal                 |
| Storage import              | `--import-storage <file>`                                                                      | Use `snapshotPath` in `launch.json` for initial state                  |
| Event display and filtering | `--show-events`, `--event-filter`                                                              | Use `soroban-debug run --show-events` in a terminal                    |
| Dry-run mode                | `--dry-run`                                                                                    | Use `soroban-debug run --dry-run` in a terminal                        |
| Cross-contract mocking      | `--mock CONTRACT.fn=value`                                                                     | Use `soroban-debug run --mock` in a terminal                           |
| Conditional breakpoints     | (not in CLI either)                                                                            | Not supported on either surface                                        |
| Hit-count conditions        | (not in CLI either)                                                                            | Not supported on either surface                                        |
| Log points                  | (not in CLI either)                                                                            | Not supported on either surface                                        |
| Analysis subcommands        | `analyze`, `symbolic`, `optimize`, `profile`, `compare`, `replay`, `upgrade-check`, `scenario` | Use CLI subcommands directly                                           |

### Supported in the extension

| Feature                         | Details                                                                           |
| ------------------------------- | --------------------------------------------------------------------------------- |
| Step in / over / out            | F11, F10, Shift+F11                                                               |
| Continue                        | F5                                                                                |
| Breakpoints                     | Set by clicking source line; resolves to the enclosing exported function boundary |
| Variable inspection — storage   | Shown in the Variables panel (Storage scope) when paused                          |
| Variable inspection — arguments | Shown in the Variables panel (Arguments scope) when paused                        |
| Call stack                      | Up to 50 frames, clickable to navigate to frame source                            |
| Expression evaluation           | Debug Console when paused; hover evaluation over identifiers                      |

For the full feature comparison, see [docs/feature-matrix.md](../../docs/feature-matrix.md).

---

## Advanced Configuration

### Timeouts

To avoid “frozen” sessions when the backend stalls, the extension enforces deterministic timeouts for every backend request.

You can configure timeouts in either place:

- VS Code Settings: `soroban-debugger.requestTimeoutMs`, `soroban-debugger.connectTimeoutMs`
- `launch.json`: `requestTimeoutMs`, `connectTimeoutMs` (overrides settings)

### Debugging the Extension Itself

To debug the extension code:

1. Open the extension folder in VS Code
2. Press F5 to launch the Extension Development Host
3. A new VS Code window opens with the extension loaded
4. Set breakpoints in the extension source code (TypeScript files in `src/`)

### Enabling Trace Logging

For troubleshooting the Debug Adapter Protocol communication:

```json
{
  "name": "Debug with Trace",
  "type": "soroban",
  "request": "launch",
  "contractPath": "...",
  "trace": true
}
```

Trace output appears in the Debug Console (Ctrl+Shift+U).

### Diagnostic Logging

The extension now maintains persistent, structured logs for all debug sessions. These are invaluable for diagnosing environment-specific failures or backend crashes.

- **Real-time logs**: View the "Soroban Debugger" output channel in the Output panel.
- **Persistent logs**: Session logs are stored in the extension's global storage directory and rotated when they reach 10MB.
- **Phased tracking**: Logs are categorized into phases such as `Spawn`, `Connect`, `Auth`, `Load`, and `Execution`.
- **Privacy**: Authentication tokens are automatically redacted from all log files.

## Architecture

The extension consists of three main components:

### Extension Host (extension.ts)

- Initializes the extension
- Registers the debug adapter factory
- Manages extension lifecycle

### Debug Adapter (src/dap/adapter.ts)

- Implements the Debug Adapter Protocol
- Handles breakpoints, stepping, and variable inspection
- Manages debug session state

### CLI Process Wrapper (src/cli/debuggerProcess.ts)

- Spawns the `soroban-debug server` process
- Connects to the remote debug protocol over TCP
- Handles process lifecycle and request/response transport

### Protocol Types (src/dap/protocol.ts)

- TypeScript types for DAP events and commands
- Debugger state management
- Variable reference handling

## Troubleshooting

### Extension doesn't activate

- Verify VS Code is version 1.75.0 or higher: `Help > About`
- Check that the extension is properly installed in `~/.vscode/extensions/`

### Debugger fails to start

- Ensure the `soroban-debug` CLI is in your PATH, or set `binaryPath`
- Verify contract path points to a valid WASM file
- Check that snapshot.json exists and is valid JSON
- Run `Soroban: Run Launch Preflight` from the command palette to catch launch configuration issues before starting a session

### Breakpoints not working

- Confirm breakpoints are set before starting the debug session
- Check Debug Console for any error messages
- Try enabling trace logging for more details

### Low performance during debugging

- Large snapshot files can slow down initialization
- Consider using a minimal snapshot for testing
- Disable trace logging if enabled

## Development

### Build and Test

```bash
# Compile TypeScript
npm run compile

# Watch for changes
npm run watch

# Run tests
npm test

# Package for distribution
npm run vscode:prepublish
```

### Project Structure

```
├── src/
│   ├── extension.ts          # Extension entry point
│   ├── debug/
│   │   └── adapter.ts        # VSCode debug adapter factory
│   ├── dap/
│   │   ├── adapter.ts        # DAP session implementation
│   │   └── protocol.ts       # Protocol types and utilities
│   └── cli/
│       └── debuggerProcess.ts # CLI process wrapper
│   ├── test/
│   │   ├── runSmokeTest.ts   # Smoke test entrypoint
│   │   ├── runDapE2E.ts      # DAP end-to-end entrypoint
│   │   ├── runTest.ts        # Combined compatibility wrapper
│   │   └── suites.ts         # Shared test suite helpers
├── package.json              # Extension manifest
├── tsconfig.json            # TypeScript configuration
└── README.md                # This file
```

### Running Tests

- `npm test` runs the smoke suite and the DAP end-to-end suite sequentially.
- `npm run test:smoke` runs the smoke checks only.
- `npm run test:dap-e2e` runs the DAP adapter end-to-end suite only.

## Contributing

We welcome contributions! Please:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/my-feature`)
3. Make your changes with clear, optimized code
4. Write tests for new functionality
5. Submit a pull request

## License

This extension is part of the Soroban project and is licensed under the MIT License. See the root [LICENSE](../../LICENSE) file for details.

## Support & Feedback

- 📮 Report bugs via [GitHub Issues](https://github.com/stellar/soroban-debugger/issues)
- 💡 Request features in [GitHub Discussions](https://github.com/stellar/soroban-debugger/discussions)
- 📖 Read the [main README](../../README.md) for general Soroban documentation

## Related Resources

- [Soroban Documentation](https://developers.stellar.org/networks/stellar-public/learn/soroban)
- [Debug Adapter Protocol](https://microsoft.github.io/debug-adapter-protocol/)
- [VS Code Extension Guide](https://code.visualstudio.com/api)
