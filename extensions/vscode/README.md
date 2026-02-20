# Soroban Debugger Extension

A Visual Studio Code extension that integrates the Soroban smart contract debugger via the Debug Adapter Protocol (DAP).

## Features

- 🔍 **Breakpoint Management**: Set, clear, and manage breakpoints directly in the VS Code editor
- 📊 **Variable Inspection**: View and inspect contract storage state in the Variables panel
- 📚 **Call Stack Visualization**: Examine the function call stack during execution
- 🧵 **Thread Support**: Basic thread management for debugging sessions
- 📝 **Detailed Logging**: Optional trace logging for debugging adapter interactions
- ⚡ **Real-time Debugging**: Step through contract execution with next, step in, and step out

## Requirements

- Visual Studio Code 1.75.0 or higher
- Node.js 18+ (for extension development)
- Soroban CLI with debugger support
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
      "trace": false
    }
  ]
}
```

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

## Advanced Configuration

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
- Spawns the soroban-debugger CLI process
- Manages stdio communication
- Handles process lifecycle

### Protocol Types (src/dap/protocol.ts)
- TypeScript types for DAP events and commands
- Debugger state management
- Variable reference handling

## Troubleshooting

### Extension doesn't activate

- Verify VS Code is version 1.75.0 or higher: `Help > About`
- Check that the extension is properly installed in `~/.vscode/extensions/`

### Debugger fails to start

- Ensure `soroban-debugger` CLI is in your PATH
- Verify contract path points to a valid WASM file
- Check that snapshot.json exists and is valid JSON

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
├── test/                      # Test files
├── package.json              # Extension manifest
├── tsconfig.json            # TypeScript configuration
└── README.md                # This file
```

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
