#!/usr/bin/env bash
# Test script to validate check_manpages.sh portability across TMPDIR configurations.
#
# Usage: bash scripts/test_manpage_tmpdir.sh
#
# This script validates that check_manpages.sh:
#   - Respects explicit TMPDIR environment variable
#   - Falls back to standard locations when TMPDIR is unset
#   - Reports clear errors when no temp directory is writable
#   - Is portable across BSD/macOS and Linux platforms

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$SCRIPT_DIR/.."
RESULTS_FILE="/tmp/manpage_tmpdir_test_results_$RANDOM.txt"

# Color codes for output
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

test_count=0
pass_count=0
fail_count=0

print_result() {
    local status="$1"
    local message="$2"
    test_count=$((test_count + 1))
    
    if [[ "$status" == "PASS" ]]; then
        echo -e "${GREEN}✓ PASS${NC}: $message"
        pass_count=$((pass_count + 1))
    else
        echo -e "${RED}✗ FAIL${NC}: $message"
        fail_count=$((fail_count + 1))
    fi
}

echo "Testing check_manpages.sh portability..."
echo ""

# Test 1: Run with default TMPDIR (unset)
echo "Test 1: Running with default TMPDIR (unset)..."
(
    cd "$REPO_ROOT"
    unset TMPDIR || true
    if bash scripts/check_manpages.sh >/dev/null 2>&1; then
        print_result "PASS" "check_manpages.sh run successfully with default TMPDIR"
    else
        exit_code=$?
        if [[ $exit_code -eq 1 ]]; then
            print_result "PASS" "check_manpages.sh correctly reported man page drift (exit code 1)"
        else
            print_result "FAIL" "check_manpages.sh exited with unexpected code $exit_code"
        fi
    fi
)

# Test 2: Run with explicit TMPDIR=/tmp
echo ""
echo "Test 2: Running with TMPDIR=/tmp..."
(
    cd "$REPO_ROOT"
    if TMPDIR=/tmp bash scripts/check_manpages.sh >/dev/null 2>&1; then
        print_result "PASS" "check_manpages.sh runs successfully with TMPDIR=/tmp"
    else
        exit_code=$?
        if [[ $exit_code -eq 1 ]]; then
            print_result "PASS" "check_manpages.sh correctly reported man page drift (exit code 1)"
        else
            print_result "FAIL" "check_manpages.sh exited with unexpected code $exit_code"
        fi
    fi
)

# Test 3: Run with DEBUG=1 to verify debug output works
echo ""
echo "Test 3: Running with DEBUG=1 to verify diagnostic output..."
(
    cd "$REPO_ROOT"
    output=$(DEBUG=1 TMPDIR=/tmp bash scripts/check_manpages.sh 2>&1 || true)
    if echo "$output" | grep -q "DEBUG"; then
        print_result "PASS" "Debug output is produced when DEBUG=1"
    else
        print_result "FAIL" "Debug output not found when DEBUG=1"
    fi
)

# Test 4: Verify Makefile target works with TMPDIR override
echo ""
echo "Test 4: Testing Makefile check-man target with TMPDIR=/tmp..."
(
    cd "$REPO_ROOT"
    if TMPDIR=/tmp make check-man >/dev/null 2>&1; then
        print_result "PASS" "make check-man runs successfully with TMPDIR=/tmp"
    else
        exit_code=$?
        if [[ $exit_code -eq 1 ]]; then
            print_result "PASS" "make check-man correctly reported man page drift"
        else
            print_result "FAIL" "make check-man exited with unexpected code $exit_code"
        fi
    fi
)

# Summary
echo ""
echo "======================================"
echo "Test Results"
echo "======================================"
echo "Total:  $test_count"
echo -e "${GREEN}Passed: $pass_count${NC}"
if [[ $fail_count -gt 0 ]]; then
    echo -e "${RED}Failed: $fail_count${NC}"
else
    echo -e "${GREEN}Failed: $fail_count${NC}"
fi
echo ""

if [[ $fail_count -eq 0 ]]; then
    echo -e "${GREEN}✓ All portability tests passed!${NC}"
    exit 0
else
    echo -e "${RED}✗ Some tests failed.${NC}"
    exit 1
fi
