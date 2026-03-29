#!/usr/bin/env bash
set -euo pipefail

SANDBOX_MODE=0
ALLOW_TEMP_CHECKS=1

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --sandbox)
            SANDBOX_MODE=1
            ALLOW_TEMP_CHECKS=0
            shift
            ;;
        --with-temp-checks)
            ALLOW_TEMP_CHECKS=1
            shift
            ;;
        *)
            echo "Unknown argument: $1"
            exit 1
            ;;
    esac
done

# Determine repo root relative to this script's location
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$SCRIPT_DIR"

# If you put this script in a subdir (e.g., scripts/), uncomment the next line:
# REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$REPO_ROOT"

if [[ ! -f "Cargo.toml" ]]; then
  echo "error: Cargo.toml not found in $REPO_ROOT"
  echo "hint: move this script to the repo root, or adjust REPO_ROOT to point to it."
  exit 1
fi

echo "==> Repo root: $REPO_ROOT"
echo "==> Rust toolchain"
rustc -Vv
cargo -V
cargo clippy -V

if [[ "$SANDBOX_MODE" -eq 1 ]]; then
  echo
  echo "==> Sandbox mode enabled"
  echo "This gate is deterministic and avoids network/temp-dependent checks."
fi

echo
echo "==> Formatting (cargo fmt --check)"
cargo fmt --all -- --check

echo
echo "==> Clippy (deny warnings)"
cargo clippy --workspace --all-targets --all-features -- -D warnings

echo
echo "==> Tests (deny rustc warnings via RUSTFLAGS)"
RUSTFLAGS="-D warnings" cargo test --workspace --all-features

if [[ "$ALLOW_TEMP_CHECKS" -eq 1 ]]; then
  echo
  echo "==> Man pages (check drift against source)"
  bash scripts/check_manpages.sh
else
  echo
  echo "==> SKIP: Man page check (requires writable temp directory)"
fi

if [[ "$SANDBOX_MODE" -eq 1 && "$ALLOW_TEMP_CHECKS" -eq 0 ]]; then
  echo
  echo "==> Sandbox skip report"
  echo "SKIP: VS Code E2E/loopback gates (depends on local TCP loopback availability)."
  echo "SKIP: Temp-dir constrained scenarios (depends on writable system temp directories)."
  echo "Result: ci-sandbox completed successfully."
  exit 0
fi

if [[ "$SANDBOX_MODE" -eq 0 ]]; then
  echo
  echo "======================================="
  echo "✅ All local CI gates passed successfully!"
  echo "======================================="
  exit 0
fi
