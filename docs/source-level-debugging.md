# Source-Level Debugging in Soroban Debugger

This document describes the architecture and implementation of source-level debugging for Soroban smart contracts.

## Overview

Soroban contracts are compiled from Rust to WebAssembly (WASM). While debugging the WASM bytecode directly is powerful, it is orignally more efficient for developers to see the corresponding Rust source code. The debugger achieves this by parsing DWARF debug information embedded in the WASM binary.

## Architecture

1. **DWARF Parsing**: The `SourceMap` module (`src/debugger/source_map.rs`) uses the `gimli` and `addr2line` crates to parse DWARF sections (`.debug_info`, `.debug_line`, etc.) from the WASM binary.
2. **Offset Mapping**: It builds a mapping from WASM instruction offsets to source locations (file path, line number, column).
3. **Source Management**: A source cache is maintained to avoid repeated disk reads when displaying code.
4. **Engine Integration**: The `DebuggerEngine` uses the `SourceMap` to resolve the current instruction's location and provides a `step_source` method for line-by-line stepping.
5. **UI Visualization**: The TUI Dashboard features a dedicated Source pane that highlights the current line and centers the view automatically.

## Requirements & Implementation

- **DWARF Support**: Full support for standard DWARF embedded in WASM.
- **Source Line Stepping**: Integrated into the stepping logic.
- **Caching**: Performance optimized with file and mapping caches.
- **Fallback**: Graceful fallback to WASM-only view if debug info is missing or stripped.

## Limitations

- **Stripped Binaries**: Production Soroban WASM files are often stripped to save space. Debug info is only available in binaries compiled with debug symbols (e.g., `cargo build`).
- **Optimization**: Highly optimized WASM (via `wasm-opt`) may have slightly inaccurate line mappings due to code movement and inlining.
- **Path Resolution**: DWARF often contains absolute paths from the build machine. If debugging on a different machine, source file loading may fail if paths don't match.

## Testing

Unit tests in `tests/source_map_test.rs` verify the lookup logic using mock mappings.
