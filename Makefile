# Soroban Debugger — developer convenience targets
#
# Targets:
#   regen-man   Regenerate all man pages from current CLI source
#   check-man   Verify committed man pages match generated output (used in CI)
#   test-man-tmpdir  Run portability tests for man page temp directory handling
#   fmt         Check Rust formatting
#   lint        Run Rust clippy lints (strict)
#   lint-strict Run Rust clippy with CI-equivalent strict flags
#   hooks-install Install pre-commit hooks for local validation
#   hooks-check  Run pre-commit hooks against all files
#   test-rust   Run Rust backend tests
#   test-vscode Run VS Code extension tests
#   ci-local    Run all practical gates developers must satisfy before pushing

.PHONY: all build fmt lint lint-strict hooks-install hooks-check test-rust test-vscode ci-local clean regen-man check-man test-man-tmpdir

all: build

build:
	cargo build
	cd extensions/vscode && npm install && npm run build

fmt:
	cargo fmt --all -- --check

lint-strict:
	cargo clippy --workspace --all-targets --all-features -- -D warnings

lint:
	$(MAKE) lint-strict

hooks-install:
	pre-commit install

hooks-check:
	pre-commit run --all-files

test-rust:
	cargo test

test-vscode:
	cd extensions/vscode && npm install && npm run test

# Regenerate all man pages from current CLI source.
# Run after any CLI flag, subcommand, or help text change, then commit the .1 files.
regen-man:
	@echo "Regenerating man pages..."
	cargo build --quiet
	@echo "Man pages updated in man/man1/ — remember to commit the .1 files."

# Verify committed man pages match generated output.
# Exits non-zero with a diff if drift is detected.
# Environment: TMPDIR can be exported to override temp directory for restricted environments.
#   Usage: export TMPDIR=/custom/tmp && make check-man
#   or:    TMPDIR=/custom/tmp bash scripts/check_manpages.sh
check-man:
	@bash scripts/check_manpages.sh

# Test portability of man page generation across different temp directory configurations.
test-man-tmpdir:
	@bash scripts/test_manpage_tmpdir.sh

# The single local entrypoint for developers
ci-local: fmt lint test-rust test-vscode check-man
	@echo "======================================="
	@echo "✅ All local CI gates passed successfully!"
	@echo "======================================="

# Sandbox-safe local gate for restricted environments.
# Runs deterministic checks and explicitly reports skipped network/temp-dependent gates.
ci-sandbox:
	@bash run_local_ci.sh --sandbox

clean:
	cargo clean
	rm -rf extensions/vscode/node_modules extensions/vscode/dist
