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


To run all tests:

```sh
cargo test
```

To run a specific test or test file:

```sh
cargo test test_name
cargo test --test integration/basic-tests
```

Tests should be:
- Isolated and repeatable
- Well-named and descriptive
- Covering both typical and edge cases

Add new tests for every new feature or bug fix. Place integration tests in the `tests/` directory and unit tests alongside the code in `src/`.

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


## Code Style Guide

Please follow these guidelines to ensure code consistency and maintainability:

- **Formatting:**
	- Use `cargo fmt` before committing. Code should be auto-formatted.
	- Indent with 4 spaces, no tabs.
	- Keep lines under 100 characters when possible.
- **Linting:**
	- Run `cargo clippy` and address all warnings before submitting code.
- **Naming:**
	- Use `snake_case` for variables and function names.
	- Use `CamelCase` for type and struct names.
	- Use `SCREAMING_SNAKE_CASE` for constants and statics.
- **Documentation:**
	- Document all public functions, structs, and modules using Rust doc comments (`///`).
	- Add inline comments for complex logic.
- **Testing:**
	- Write unit and integration tests for new features and bug fixes.
	- Place integration tests in the `tests/` directory.
- **Error Handling:**
	- Prefer `Result<T, E>` over panics for recoverable errors.
	- Use meaningful error messages.
- **General:**
	- Remove unused code and imports.
	- Avoid commented-out code in commits.
	- Keep functions small and focused.

---

---
## Commit Messages


## Commit Message Conventions

We use [Conventional Commits](https://www.conventionalcommits.org/) for commit messages. This helps automate changelogs and makes the project history easier to understand.

**Format:**

```
<type>(optional scope): short summary

[optional body]

[optional footer(s)]
```

**Common types:**

- feat: new feature
- fix: bug fix
- docs: documentation changes
- style: formatting, missing semicolons, etc. (no code change)
- refactor: code change that neither fixes a bug nor adds a feature
- perf: performance improvement
- test: adding or correcting tests
- chore: maintenance tasks (build scripts, tooling, etc.)

**Examples:**

```
feat: add support for contract breakpoints

fix: resolve panic when loading invalid WASM

docs: update README with new usage example

style: reformat engine.rs for readability

refactor(debugger): extract stepper logic into module

perf: optimize storage inspection for large contracts

test: add integration tests for CLI parser

chore: update dependencies and build scripts
```

**Tips:**
- Use the imperative mood (e.g., "add" not "added" or "adds").
- Reference issues or PRs in the footer if relevant (e.g., `Closes #123`).

---

---
## Pull Request Process


**Quick checklist before submitting a PR:**

- [ ] All tests pass locally (`cargo test`)
- [ ] Code is formatted (`cargo fmt --all --check`)
- [ ] Clippy is clean (`cargo clippy --all-targets --all-features -- -D warnings`)
- [ ] Commit message follows [Conventional Commits](https://www.conventionalcommits.org/)
- [ ] README links to CONTRIBUTING.md (if relevant to your change)
- [ ] PR description mentions the related issue(s)

4. Push and open the PR.
5. Fill in the PR description with context, motivation, and any related issues.
6. Request a review from project maintainers.
7. Respond to feedback and make necessary revisions.
8. PRs will be merged after approval and successful CI checks.

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
