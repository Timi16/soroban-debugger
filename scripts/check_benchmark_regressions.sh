#!/usr/bin/env bash
# Runs the benchmark regression gate without mutating the caller's checkout.
# It benchmarks the current tree, benchmarks a baseline ref in a temporary
# detached worktree, and compares the saved Criterion baselines with critcmp.
#
# Usage:
#   bash scripts/check_benchmark_regressions.sh
#   bash scripts/check_benchmark_regressions.sh coverage-percent-from-json < summary.json
#   bash scripts/check_benchmark_regressions.sh selftest-coverage-missing-field
#
# Optional environment variables:
#   BASELINE_REF            Git ref to benchmark as the baseline.
#   BENCHMARK_THRESHOLD     critcmp percentage threshold (default: 10).
#   CURRENT_BASELINE_NAME   Criterion baseline name for the current tree.
#   BASELINE_NAME           Criterion baseline name for the baseline ref.

set -euo pipefail

require_jq() {
    if ! command -v jq >/dev/null 2>&1; then
        echo "ERROR: jq is required to parse coverage JSON but was not found on PATH." >&2
        echo "Install jq or use an environment image that provides jq." >&2
        return 2
    fi
}

emit_schema_debug() {
    local input_json="$1"
    local top_keys
    local first_keys
    local totals_keys
    local lines_keys

    top_keys="$(printf '%s' "$input_json" | jq -r 'keys_unsorted | join(",")' 2>/dev/null || true)"
    first_keys="$(printf '%s' "$input_json" | jq -r '.data[0] | keys_unsorted | join(",")' 2>/dev/null || true)"
    totals_keys="$(printf '%s' "$input_json" | jq -r '.data[0].totals | keys_unsorted | join(",")' 2>/dev/null || true)"
    lines_keys="$(printf '%s' "$input_json" | jq -r '.data[0].totals.lines | keys_unsorted | join(",")' 2>/dev/null || true)"

    [ -n "$top_keys" ] && echo "DEBUG: top-level keys: $top_keys" >&2
    [ -n "$first_keys" ] && echo "DEBUG: .data[0] keys: $first_keys" >&2
    [ -n "$totals_keys" ] && echo "DEBUG: .data[0].totals keys: $totals_keys" >&2
    [ -n "$lines_keys" ] && echo "DEBUG: .data[0].totals.lines keys: $lines_keys" >&2
}

coverage_percent_from_json() {
    require_jq || return $?

    local input_json
    input_json="$(cat)"

    if [ -z "$input_json" ]; then
        echo "ERROR: No JSON input provided to coverage-percent-from-json." >&2
        echo "Pipe cargo-llvm-cov JSON output into this command." >&2
        return 1
    fi

    if ! printf '%s' "$input_json" | jq -e '.' >/dev/null 2>&1; then
        echo "ERROR: Input is not valid JSON." >&2
        return 1
    fi

    if ! printf '%s' "$input_json" | jq -e '.data | type == "array" and length > 0' >/dev/null 2>&1; then
        echo "ERROR: Coverage JSON schema changed; expected non-empty '.data' array." >&2
        emit_schema_debug "$input_json"
        return 1
    fi

    if ! printf '%s' "$input_json" | jq -e '.data[0].totals | type == "object"' >/dev/null 2>&1; then
        echo "ERROR: Coverage JSON schema changed; missing required object '.data[0].totals'." >&2
        emit_schema_debug "$input_json"
        return 1
    fi

    if ! printf '%s' "$input_json" | jq -e '.data[0].totals.lines | type == "object"' >/dev/null 2>&1; then
        echo "ERROR: Coverage JSON schema changed; missing required object '.data[0].totals.lines'." >&2
        emit_schema_debug "$input_json"
        return 1
    fi

    if ! printf '%s' "$input_json" | jq -e '.data[0].totals.lines.percent | type == "number"' >/dev/null 2>&1; then
        echo "ERROR: Coverage JSON schema changed; missing required numeric field '.data[0].totals.lines.percent'." >&2
        emit_schema_debug "$input_json"
        return 1
    fi

    printf '%s' "$input_json" | jq -r '.data[0].totals.lines.percent'
}

selftest_coverage_missing_field() {
    local broken_json
    local output
    local status

    if ! require_jq; then
        echo "ERROR: Cannot run coverage parser self-test without jq." >&2
        return 2
    fi

    broken_json='{"data":[{"totals":{"lines":{}}}]}'

    set +e
    output="$(printf '%s' "$broken_json" | coverage_percent_from_json 2>&1)"
    status=$?
    set -e

    if [ "$status" -eq 0 ]; then
        echo "ERROR: Expected coverage parser to fail when '.data[0].totals.lines.percent' is missing." >&2
        return 1
    fi

    echo "$output"
    if ! printf '%s' "$output" | grep -Fq "missing required numeric field '.data[0].totals.lines.percent'"; then
        echo "ERROR: Coverage parser failure was not actionable enough." >&2
        return 1
    fi

    echo "Coverage parser self-test passed: missing field emitted actionable error."
}

case "${1:-}" in
    coverage-percent-from-json)
        coverage_percent_from_json
        exit 0
        ;;
    selftest-coverage-missing-field)
        selftest_coverage_missing_field
        exit 0
        ;;
esac

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BENCHMARK_THRESHOLD="${BENCHMARK_THRESHOLD:-10}"

log() {
    printf '[bench-regression] %s\n' "$*"
}

log_worktree_state() {
    log "worktree state"
    git -C "$REPO_ROOT" worktree list --porcelain || log "unable to read worktree list"
}

if [ -z "${BASELINE_REF:-}" ]; then
    if git -C "$REPO_ROOT" rev-parse --verify --quiet refs/remotes/origin/main >/dev/null; then
        BASELINE_REF="origin/main"
    else
        BASELINE_REF="main"
    fi
fi

TEMP_DIR="$(mktemp -d)"
WORKTREE_DIR="$TEMP_DIR/baseline-worktree"
WORKTREE_ADDED=0

cleanup() {
    local cleanup_failed=0

    log "cleanup start"
    log "temp dir: $TEMP_DIR"
    log "worktree path: $WORKTREE_DIR"
    log_worktree_state

    if [ "$WORKTREE_ADDED" -eq 1 ]; then
        if git -C "$REPO_ROOT" worktree remove --force "$WORKTREE_DIR"; then
            log "worktree remove succeeded"
        else
            log "worktree remove failed; running fallback prune/remove"
            git -C "$REPO_ROOT" worktree prune --expire now || cleanup_failed=1
            if [ -d "$WORKTREE_DIR" ]; then
                rm -rf "$WORKTREE_DIR" || cleanup_failed=1
            fi
        fi
    else
        log "worktree add was not completed"
    fi

    rm -rf "$TEMP_DIR" || cleanup_failed=1
    log_worktree_state

    if [ "$cleanup_failed" -eq 0 ]; then
        log "cleanup complete"
    else
        log "cleanup complete with fallback errors"
    fi
}

trap cleanup EXIT

if ! command -v critcmp >/dev/null 2>&1; then
    echo "critcmp is required but was not found on PATH."
    echo "Install it with: cargo install critcmp --version 0.1.7"
    exit 2
fi

log "baseline ref: $BASELINE_REF"
log "adding detached worktree"
git -C "$REPO_ROOT" worktree add --detach "$WORKTREE_DIR" "$BASELINE_REF"
WORKTREE_ADDED=1
log_worktree_state

log "running benchmarks for current checkout"
CURRENT_JSON="$TEMP_DIR/current.json"
BASELINE_JSON="$TEMP_DIR/baseline.json"
CURRENT_TARGET="$TEMP_DIR/current-target"
BASELINE_TARGET="$TEMP_DIR/baseline-target"

echo "Running benchmarks for the current checkout..."
(
    cd "$REPO_ROOT"
    CARGO_TARGET_DIR="$CURRENT_TARGET" cargo bench --benches -- --noplot
    cargo run --quiet --bin bench-regression -- record \
        --criterion "$CURRENT_TARGET/criterion" \
        --out "$CURRENT_JSON"
)

log "running benchmarks for baseline checkout"
(
    cd "$WORKTREE_DIR"
    CARGO_TARGET_DIR="$BASELINE_TARGET" cargo bench --benches -- --noplot
    cargo run --quiet --bin bench-regression -- record \
        --criterion "$BASELINE_TARGET/criterion" \
        --out "$BASELINE_JSON"
)

echo "Comparing baselines (threshold: ${BENCHMARK_THRESHOLD}%)..."
set +e
output="$(
    cd "$REPO_ROOT"
    cargo run --quiet --bin bench-regression -- compare \
        --baseline "$BASELINE_JSON" \
        --current "$CURRENT_JSON" \
        --warn-pct "$BENCHMARK_THRESHOLD" \
        --fail-pct 20 2>&1
)"
status=$?
set -e

echo "$output"

if [ "$status" -eq 0 ]; then
    exit 0
fi

cp -R "$BENCH_TARGET_DIR/criterion" "$CRITCMP_ROOT/target/criterion"

log "comparing baselines with critcmp (threshold: ${BENCHMARK_THRESHOLD}%)"
(
    cd "$CRITCMP_ROOT"

    set +e
    output="$(critcmp "$BASELINE_NAME" "$CURRENT_BASELINE_NAME" --threshold "$BENCHMARK_THRESHOLD" 2>&1)"
    status=$?
    set -e

    echo "$output"

    if [ "$status" -eq 0 ]; then
        exit 0
    fi

    if echo "$output" | grep -Fq "no benchmark comparisons to show"; then
        echo "No overlapping benchmark IDs between '$BASELINE_NAME' and '$CURRENT_BASELINE_NAME'; skipping regression gate."
        exit 0
    fi
if echo "$output" | grep -Fq "no benchmark comparisons to show"; then
    echo "No overlapping benchmark IDs; skipping regression gate."
    exit 0
fi

exit "$status"
