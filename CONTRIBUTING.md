# Contributing to Soroban Debugger

Thank you for your interest in contributing to the **Soroban Debugger** project! We welcome contributions from the community and are committed to fostering a collaborative, respectful, and productive environment.

---

## Table of Contents

1. [Development Environment Setup](#development-environment-setup)
2. [Project Setup](#project-setup)
3. [Running Tests](#running-tests)
4. [Fuzzing](#fuzzing)
5. [Code Style & Quality](#code-style--quality)
6. [Commit Message Conventions](#commit-message-conventions)
7. [Claiming and Working on Issues](#claiming-and-working-on-issues)
8. [Pull Request Process](#pull-request-process)
9. [Issue Guidelines](#issue-guidelines)
10. [Areas for Contribution](#areas-for-contribution)
11. [Project Structure](#project-structure)
12. [Code of Conduct](#code-of-conduct)
13. [Communication](#communication)
14. [Release Process](#release-process)

---

## Development Environment Setup

### Prerequisites

- Git
- Rust (stable toolchain, 1.75 or later)
- Soroban CLI (for contract testing)

### Install Rust (from scratch)

We recommend `rustup` to manage toolchains.

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Install the stable toolchain plus formatter and lints:

```bash
rustup toolchain install stable
rustup default stable
rustup component add rustfmt clippy
```

Verify:

```bash
rustc --version
cargo --version
```

### Pre-commit Hooks

We use the [pre-commit](https://pre-commit.com/) framework to automatically format and lint code before each commit to prevent CI failures.

To install:
1. Install `pre-commit` locally (e.g., `pip install pre-commit` or `brew install pre-commit`).
2. Run `pre-commit install` in the repository root to activate the hooks.

By default, pre-commit runs strict clippy with CI-equivalent flags via the `cargo-clippy` hook (`cargo clippy --workspace --all-targets --all-features -- -D warnings`).

If your commit only changes non-Rust files (for example docs or markdown) and you need to bypass clippy for that commit, use:

```bash
SKIP=cargo-clippy git commit -m "docs: update troubleshooting guide"
```

You can still run all hooks manually at any time:

```bash
pre-commit run --all-files
```

---

## Project Setup

1. **Fork** the repository on GitHub.
2. **Clone** your fork:

```bash
git clone https://github.com/<your-username>/soroban-debugger.git
cd soroban-debugger
```

3. **Add the upstream remote**:

```bash
git remote add upstream https://github.com/Timi16/soroban-debugger.git
```

4. **Create a branch** for your work:

```bash
git checkout -b feat/short-description
```

5. **Build** once to ensure the toolchain is working:

```bash
cargo build
```

### Running the CLI

```bash
cargo run -- run --contract path/to/contract.wasm --function function_name
```

---

## Running Tests

CI runs the full workspace test suite with all features enabled. Match that locally:

```bash
cargo test --workspace --all-features
```

Run a single test by name:

```bash
cargo test <test_name>
```

Run a specific integration test file:

```bash
cargo test --test <test_file>
```

Reproduce the benchmark regression gate locally without checking out `main` in place:

```bash
cargo install critcmp --version 0.1.7
bash scripts/check_benchmark_regressions.sh
```

The script benchmarks your current branch, then benchmarks `origin/main` in a temporary detached worktree when available so your checkout stays on your branch throughout the comparison.

Tests should be:
- Isolated and repeatable
- Well-named and descriptive
- Covering both typical and edge cases

Add new tests for every new feature or bug fix. Place integration tests in the `tests/` directory and unit tests alongside the code in `src/`.

---

## Fuzzing

Fuzzing helps discover crashes and panics in critical code paths like WASM parsing and argument parsing.

**Prerequisites:**
Install `cargo-fuzz`:
```bash
cargo install cargo-fuzz
```

**Running a fuzz target:**
```bash
# Run WASM loading fuzzer
cargo +nightly fuzz run wasm_loading

# Run argument parser fuzzer
cargo +nightly fuzz run arg_parser

# Run storage key parsing fuzzer
cargo +nightly fuzz run storage_keys
```

By default, fuzzers run indefinitely. You can limit the execution time with `-- -max_total_time=<seconds>`.

---

## Code Style & Quality

We follow standard Rust tooling and treat warnings as errors in CI.

### Formatting

```bash
cargo fmt --all
```

Check formatting (CI uses this):

```bash
cargo fmt --all -- --check
```

### Linting

```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Equivalent make target:

```bash
make lint-strict
```

### Guidelines

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

## Commit Message Conventions

We use [Conventional Commits](https://www.conventionalcommits.org/) (see `cliff.toml`). Format:

```
<type>(optional scope): short summary

[optional body]

[optional footer(s)]
```

Common types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `chore`.

Examples:

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

Tips:

- Use the imperative mood ("add", "fix", "update").
- Reference issues or PRs in the footer when applicable (e.g., `Closes #123`).

---

## Claiming and Working on Issues

- Check the issue tracker for open issues and labels like `good first issue` or `help wanted`.
- Before starting, comment on the issue to say you want to work on it.
- If an issue is already assigned, coordinate in the thread before beginning work.
- Keep one issue per PR when possible, and link the PR to the issue.

---

## Pull Request Process

**Quick checklist before submitting a PR:**

- [ ] All tests pass locally (`cargo test --workspace --all-features`)
- [ ] Code is formatted (`cargo fmt --all -- --check`)
- [ ] Clippy is clean (`cargo clippy --workspace --all-targets --all-features -- -D warnings`)
- [ ] Commit message follows [Conventional Commits](https://www.conventionalcommits.org/)
- [ ] PR description mentions the related issue(s)
- [ ] CI/test behavior changes documented in PR description (or marked N/A — see "CI/Test Behavior Changes" section in PR template)
- [ ] If CLI flags/subcommands/help text changed, man pages regenerated (`make regen-man`) and `.1` files committed

**Steps:**

1. Sync with upstream before finalizing your branch:

```bash
git fetch upstream
git rebase upstream/main
```

2. Ensure all checks pass locally:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
bash scripts/test_benchmark_regressions.sh
bash scripts/check_benchmark_regressions.sh
```

3. Push your branch and open a PR against `main`.
4. Include:

- A clear description of the change and motivation.
- The related issue number (e.g., `Closes #123`).
- Test results (commands you ran).

5. Request a review from project maintainers.
6. Address review feedback promptly. PRs are merged after approval and CI passes.

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

## Updating Man Pages

Man pages in `man/man1/` are generated automatically from the CLI source via `build.rs` and `clap_mangen`. **Do not hand-edit `.1` files** — changes will be overwritten on the next regeneration.

### When to regenerate

Regenerate whenever you:
- Add, remove, or rename a CLI subcommand
- Add, remove, or rename a CLI flag or argument
- Change any help text or description string

### How to regenerate

```bash
make regen-man
```

Commit the updated `.1` files alongside your CLI changes in the same PR.

### CI enforcement

The `check-manpages` CI job runs on every PR and push to `main`. It regenerates man pages into a temp directory and diffs them against the committed versions. The job fails if any drift is detected — PRs cannot be merged with stale man pages.

To verify locally before pushing:

```bash
# Regenerate from current source
make regen-man

# Verify in sync (should exit 0)
make check-man
```

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

---

## Release Process

Releases are gated by a single unified checklist that covers Rust/CLI, analyzers, VS Code extension checks, and benchmark thresholds:

- `docs/release-checklist.md`
