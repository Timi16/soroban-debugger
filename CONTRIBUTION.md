# Contributing to Soroban Debugger

Thank you for your interest in contributing to the **Soroban Debugger** project! We welcome contributions from the community and are committed to fostering a collaborative, respectful, and productive environment.

---
## Table of Contents

1. [Getting Started](#getting-started)
2. [Development Workflow](#development-workflow)
3. [Code Style & Quality](#code-style--quality)
4. [Commit Messages](#commit-messages)
5. [Pull Request Process](#pull-request-process)
6. [Issue Guidelines](#issue-guidelines)
7. [Areas for Contribution](#areas-for-contribution)
8. [Project Structure](#project-structure)
9. [Code of Conduct](#code-of-conduct)
10. [Communication](#communication)

---
## Getting Started

To begin contributing:

1. **Fork** the repository on GitHub.
2. **Clone** your fork:
	```sh
	git clone https://github.com/yourusername/soroban-debugger.git
	```
3. **Create a feature branch**:
	```sh
	git checkout -b feature/your-feature-name
	```
4. **Make your changes** (see [Development Workflow](#development-workflow)).
5. **Test and lint** your code.
6. **Commit** your changes with a clear message.
7. **Push** to your fork:
	```sh
	git push origin feature/your-feature-name
	```
8. **Open a Pull Request** (PR) against the `main` branch.

---
## Development Workflow

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) 1.75 or later
- Soroban CLI (for contract testing)

### Building

```sh
cargo build
```

### Running Tests

```sh
cargo test
```

### Linting & Formatting

```sh
cargo fmt
cargo clippy
```

### Running the CLI

```sh
cargo run -- run --contract path/to/contract.wasm --function function_name
```

---
## Code Style & Quality

We follow standard Rust conventions and best practices:

- **Formatting:** Run `cargo fmt` before committing.
- **Linting:** Run `cargo clippy` and address all warnings.
- **Testing:** Write and update tests for all new or changed functionality.
- **Documentation:** Update documentation and code comments as needed.
- **Code Review:** All code is subject to review by project maintainers.

---
## Commit Messages

Please use clear, descriptive commit messages. Follow these guidelines:

- Start with a verb in present tense (e.g., Add, Fix, Update).
- Keep the subject line under 72 characters.
- Use the body to explain _why_ the change was made, if necessary.
- Reference related issues or PRs when applicable.

---
## Pull Request Process

1. Ensure your branch is up to date with `main`.
2. Confirm all tests pass and code is linted/formatted.
3. Update `README.md` and `CHANGELOG.md` if you have added or changed features.
4. Provide a clear description of your changes in the PR.
5. Request a review from project maintainers.
6. Respond to feedback and make necessary revisions.
7. PRs will be merged after approval and successful CI checks.

---
## Issue Guidelines

### Reporting Bugs

When reporting a bug, please include:
- Steps to reproduce
- Expected and actual behavior
- Error messages and logs
- Contract WASM file (if relevant)
- Environment details (OS, Rust version, etc.)

### Suggesting Features

When suggesting a feature, please include:
- A clear description of the feature
- Use cases and motivation
- Expected behavior
- Any relevant examples or references

---
## Areas for Contribution

We welcome contributions in the following areas:

**Current Focus:**
- CLI improvements
- Enhanced error messages
- Storage inspection
- Budget tracking

**Upcoming:**
- Breakpoint management
- Terminal UI enhancements
- Call stack visualization
- Execution replay

**Future:**
- WASM instrumentation
- Source map support
- Memory profiling
- Performance analysis

If you have ideas outside these areas, feel free to discuss them by opening an issue.

---
## Project Structure

- `src/cli/` — Command-line interface
- `src/debugger/` — Core debugging engine
- `src/runtime/` — WASM execution environment
- `src/inspector/` — State inspection tools
- `src/ui/` — Terminal user interface
- `src/utils/` — Utility functions
- `tests/` — Integration tests
- `examples/` — Example usage

---
## Code of Conduct

We are committed to providing a welcoming and inclusive environment for everyone. All interactions must be respectful and constructive. Please review our [Code of Conduct](CODE_OF_CONDUCT.md) for details.

---

## Communication

- For questions, open an issue or start a discussion on GitHub.
- For security concerns, please contact the maintainers directly.
- Join our community channels (if available) for real-time discussion.

---

Thank you for helping make Soroban Debugger better!
# Contributing to Soroban Debugger

Thanks for your interest in contributing to the Soroban Debugger project!

## Getting Started

1. Fork the repository
2. Clone your fork: `git clone https://github.com/yourusername/soroban-debugger.git`
3. Create a new branch: `git checkout -b feature/your-feature-name`
4. Make your changes
5. Run tests: `cargo test`
6. Run formatting: `cargo fmt`
7. Run linter: `cargo clippy`
8. Commit your changes: `git commit -am 'Add some feature'`
9. Push to the branch: `git push origin feature/your-feature-name`
10. Create a Pull Request

## Development Setup

### Prerequisites

- Rust 1.75 or later
- Soroban CLI (for testing)

### Building

```bash
cargo build
```

### Running Tests

```bash
cargo test
```

### Running the CLI

```bash
cargo run -- run --contract path/to/contract.wasm --function function_name
```

## Project Structure

- `src/cli/` - Command-line interface
- `src/debugger/` - Core debugging engine
- `src/runtime/` - WASM execution environment
- `src/inspector/` - State inspection tools
- `src/ui/` - Terminal user interface
- `src/utils/` - Utility functions
- `tests/` - Integration tests
- `examples/` - Example usage

## Code Style

This project follows standard Rust conventions:

- Run `cargo fmt` before committing
- Run `cargo clippy` and fix any warnings
- Write tests for new functionality
- Update documentation as needed

## Commit Messages

- Use clear, descriptive commit messages
- Start with a verb in present tense (Add, Fix, Update, etc.)
- Keep the first line under 72 characters
- Add detailed description in the body if needed

## Pull Request Process

1. Ensure all tests pass
2. Update README.md if you've added features
3. Update CHANGELOG.md with your changes
4. Request review from maintainers

## Issue Guidelines

### Reporting Bugs

Include:
- Steps to reproduce
- Expected behavior
- Actual behavior
- Contract WASM file (if relevant)
- Error messages
- Environment details

### Suggesting Features

Include:
- Clear description of the feature
- Use cases
- Expected behavior
- Any relevant examples

## Areas for Contribution

### Phase 1 (Current)
- Basic CLI improvements
- Better error messages
- Storage inspection enhancements
- Budget tracking improvements

### Phase 2 (Upcoming)
- Breakpoint management
- Enhanced terminal UI
- Call stack visualization
- Execution replay

### Phase 3 (Future)
- WASM instrumentation
- Source map support
- Memory profiling
- Performance analysis

## Questions?

Feel free to open an issue or reach out to the maintainers.

## Code of Conduct

Be respectful and constructive in all interactions.