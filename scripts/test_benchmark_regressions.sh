#!/usr/bin/env bash

set -euo pipefail

SOURCE_SCRIPT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/check_benchmark_regressions.sh"
TEST_ROOT="$(mktemp -d)"

cleanup() {
    rm -rf "$TEST_ROOT"
}

trap cleanup EXIT

setup_repo() {
    local case_root="$1"
    local repo_root="$case_root/repo"
    local bin_dir="$case_root/bin"

    mkdir -p "$repo_root/scripts" "$bin_dir"
    cp "$SOURCE_SCRIPT" "$repo_root/scripts/check_benchmark_regressions.sh"
    chmod +x "$repo_root/scripts/check_benchmark_regressions.sh"

    git init -b main "$repo_root" >/dev/null
    git -C "$repo_root" config user.name "Codex Test"
    git -C "$repo_root" config user.email "codex@example.com"

    cat > "$repo_root/branch.txt" <<'EOF'
main
EOF
    git -C "$repo_root" add branch.txt scripts/check_benchmark_regressions.sh
    git -C "$repo_root" commit -m "main branch" >/dev/null

    git -C "$repo_root" checkout -b feature >/dev/null
    cat > "$repo_root/branch.txt" <<'EOF'
feature
EOF
    git -C "$repo_root" add branch.txt
    git -C "$repo_root" commit -m "feature branch" >/dev/null

    printf '%s\n' "$repo_root|$bin_dir"
}

write_fake_cargo() {
    local bin_dir="$1"
    local mode="$2"

    cat > "$bin_dir/cargo" <<EOF
#!/usr/bin/env bash
set -euo pipefail

baseline=""
args=("\$@")
for ((i = 0; i < \${#args[@]}; i++)); do
    if [ "\${args[\$i]}" = "--save-baseline" ]; then
        baseline="\${args[\$((i + 1))]}"
        break
    fi
done

if [ -z "\$baseline" ]; then
    echo "missing --save-baseline" >&2
    exit 1
fi

branch_value="\$(cat branch.txt)"
printf 'cargo|%s|%s\\n' "\$baseline" "\$PWD|\$branch_value" >> "\$LOG_FILE"

if [ "${mode}" = "fail-base" ] && [ "\$baseline" = "base" ]; then
    echo "simulated baseline benchmark failure" >&2
    exit 42
fi

mkdir -p "\$CARGO_TARGET_DIR/criterion/fake/\$baseline"
printf '%s\\n' "\$branch_value" > "\$CARGO_TARGET_DIR/criterion/fake/\$baseline/source.txt"
EOF
    chmod +x "$bin_dir/cargo"
}

write_fake_critcmp() {
    local bin_dir="$1"

    cat > "$bin_dir/critcmp" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

printf 'critcmp|%s\n' "$PWD" >> "$LOG_FILE"
test -f "$PWD/target/criterion/fake/base/source.txt"
test -f "$PWD/target/criterion/fake/new/source.txt"
printf 'comparison ok\n'
EOF
    chmod +x "$bin_dir/critcmp"
}

run_success_case() {
    local case_root="$TEST_ROOT/success"
    local paths
    local repo_root
    local bin_dir
    local log_file="$case_root/invocations.log"

    paths="$(setup_repo "$case_root")"
    repo_root="${paths%|*}"
    bin_dir="${paths#*|}"

    write_fake_cargo "$bin_dir" "pass"
    write_fake_critcmp "$bin_dir"

    LOG_FILE="$log_file" PATH="$bin_dir:$PATH" bash "$repo_root/scripts/check_benchmark_regressions.sh" >/dev/null

    if [ "$(git -C "$repo_root" branch --show-current)" != "feature" ]; then
        echo "expected to remain on feature branch" >&2
        exit 1
    fi

    if ! grep -Fq "cargo|new|$repo_root|feature" "$log_file"; then
        echo "expected current benchmark run in feature checkout" >&2
        cat "$log_file" >&2
        exit 1
    fi

    if ! grep -Eq "cargo\|base\|.*/baseline-worktree\|main" "$log_file"; then
        echo "expected baseline benchmark run in detached worktree" >&2
        cat "$log_file" >&2
        exit 1
    fi

    if ! grep -Eq "critcmp\|.*/critcmp-root" "$log_file"; then
        echo "expected critcmp run from isolated comparison root" >&2
        cat "$log_file" >&2
        exit 1
    fi
}

run_failure_cleanup_case() {
    local case_root="$TEST_ROOT/failure"
    local paths
    local repo_root
    local bin_dir
    local output_file="$case_root/output.log"
    local status=0
    local worktree_path

    paths="$(setup_repo "$case_root")"
    repo_root="${paths%|*}"
    bin_dir="${paths#*|}"

    write_fake_cargo "$bin_dir" "fail-base"
    write_fake_critcmp "$bin_dir"

    set +e
    LOG_FILE="$case_root/invocations.log" PATH="$bin_dir:$PATH" bash "$repo_root/scripts/check_benchmark_regressions.sh" >"$output_file" 2>&1
    status=$?
    set -e

    if [ "$status" -eq 0 ]; then
        echo "expected injected baseline benchmark failure" >&2
        cat "$output_file" >&2
        exit 1
    fi

    if ! grep -Fq "[bench-regression] cleanup start" "$output_file"; then
        echo "expected cleanup telemetry on failure" >&2
        cat "$output_file" >&2
        exit 1
    fi

    if ! grep -Fq "[bench-regression] worktree state" "$output_file"; then
        echo "expected worktree state telemetry on failure" >&2
        cat "$output_file" >&2
        exit 1
    fi

    worktree_path="$(grep -Eo '\[bench-regression\] worktree path: .*baseline-worktree' "$output_file" | sed 's/^\[bench-regression\] worktree path: //;q')"
    if [ -z "$worktree_path" ]; then
        echo "expected cleanup telemetry to include worktree path" >&2
        cat "$output_file" >&2
        exit 1
    fi

    if [ -d "$worktree_path" ]; then
        echo "expected failing case cleanup to remove worktree path" >&2
        cat "$output_file" >&2
        exit 1
    fi
}

run_success_case
run_failure_cleanup_case

echo "benchmark regression script validates isolated worktree usage and cleanup telemetry"
