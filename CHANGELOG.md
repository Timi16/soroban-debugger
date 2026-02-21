# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Watch mode (`--watch` flag) for automatic reload and re-execution when WASM file changes
  - Debounced file system events (~500ms) to avoid repeated triggers
  - Clean terminal output on each run
  - Graceful Ctrl+C exit handling
  - Errors don't terminate watch mode

### Deprecated

- CLI flag `--wasm` and `--contract-path` are deprecated in favor of `--contract`
- CLI flag `--snapshot` is deprecated in favor of `--network-snapshot`

## [0.1.0] - 2026-02-19

### Added

- Step-through execution of Soroban contracts
- Breakpoints at function boundaries
- Contract storage and state inspection
- CPU and memory budget tracking
- Call stack viewing for contract invocations
- Interactive terminal UI for debugging sessions
- Cross-contract call support
- Basic test coverage
- Contribution guidelines

[Unreleased]: https://github.com/Timi16/soroban-debugger/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Timi16/soroban-debugger/releases/tag/v0.1.0
