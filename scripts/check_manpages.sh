#!/usr/bin/env bash
# Regenerates man pages into a temp directory and diffs against committed versions.
# Exits non-zero with a clear diff output if drift is detected.
#
# Usage: bash scripts/check_manpages.sh
#   or:  make check-man
#   or:  TMPDIR=/custom/tmp bash scripts/check_manpages.sh
#
# Environment variables:
#   TMPDIR  - Custom temp directory (defaults to /tmp if unset or invalid)
#   DEBUG   - Set to 1 for verbose output
#
# Portability notes:
#   - Uses `mktemp -d` with explicit template for BSD/macOS compatibility.
#   - Honours TMPDIR so CI and sandbox environments can control the temp root.
#   - Falls back gracefully with validation and clear error messages.
#   - Detects and reports when temp directories are unwritable.

set -euo pipefail

DEBUG="${DEBUG:-0}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMMITTED_DIR="$REPO_ROOT/man/man1"

# Function to print debug messages
debug_log() {
    if [[ "$DEBUG" == "1" ]]; then
        echo "[DEBUG] $*" >&2
    fi
}

# Function to find a writable temp directory
find_tmpdir() {
    local candidates=()

    # First, try TMPDIR if explicitly set and is a directory
    if [[ -n "${TMPDIR:-}" ]]; then
        # Normalize the path (resolve symlinks)
        local resolved_tmpdir
        resolved_tmpdir=$(cd "$TMPDIR" 2>/dev/null && pwd) || resolved_tmpdir="$TMPDIR"
        if [[ -d "$resolved_tmpdir" && -w "$resolved_tmpdir" ]]; then
            candidates+=("$resolved_tmpdir")
            debug_log "Found TMPDIR: $resolved_tmpdir"
        fi
    fi

    # Add standard fallback locations (platform-portable)
    # On macOS, /tmp is usually a symlink to /private/tmp
    candidates+=("/tmp" "/var/tmp" "$HOME/.tmp")

    for candidate in "${candidates[@]}"; do
        if [[ -d "$candidate" && -w "$candidate" ]]; then
            echo "$candidate"
            debug_log "Selected temp directory: $candidate"
            return 0
        fi
        debug_log "Temp directory not usable: $candidate (exists=$(test -d "$candidate"; echo $?) writable=$(test -w "$candidate" 2>/dev/null; echo $?))"
    done

    return 1
}

# Find and validate temp directory
TMPDIR_SELECTED=$(find_tmpdir) || {
    echo "ERROR: Could not find a writable temporary directory." >&2
    echo "  Checked: TMPDIR (if set), /tmp, /var/tmp, \$HOME/.tmp" >&2
    echo "  ACTION: Ensure at least one temp directory exists and is writable." >&2
    echo "  HINT: Set TMPDIR=/path/to/writable/dir if default locations are restricted." >&2
    exit 2
}

export TMPDIR="$TMPDIR_SELECTED"
debug_log "Using TMPDIR=$TMPDIR_SELECTED"

# Create temp dir with explicit template for portability (BSD mktemp requires
# the template to contain at least 3 trailing Xs).
TEMP_DIR=$(mktemp -d "${TMPDIR_SELECTED}/check_manpages.XXXXXXXX") || {
    echo "ERROR: Failed to create temporary directory in $TMPDIR_SELECTED" >&2
    exit 2
}

debug_log "Created TEMP_DIR: $TEMP_DIR"

# Always clean up temp dir, even on failure or Ctrl-C
trap 'rm -rf "$TEMP_DIR"' EXIT

TEMP_MAN_DIR="$TEMP_DIR/man/man1"

echo "Generating fresh man pages into $TEMP_DIR..."
debug_log "TMPDIR source: $TMPDIR_SELECTED"

# MAN_OUT_DIR is read by build.rs to redirect man page output to a temp directory.
# This avoids touching the committed man/man1/ during the diff check.
# The build script (which generates man pages) runs before the main crate is compiled,
# so we tolerate main-crate compilation failures with || true — man pages are still produced.
MAN_OUT_DIR="$TEMP_MAN_DIR" cargo build --quiet 2>/dev/null || true

if [ ! -d "$TEMP_MAN_DIR" ]; then
    echo "ERROR: Man page generation failed: $TEMP_MAN_DIR was not created." >&2
    exit 2
fi

if [ ! -d "$COMMITTED_DIR" ]; then
    echo "ERROR: Committed man page directory not found: $COMMITTED_DIR"
    echo "   Run 'make regen-man' to generate and commit man pages."
    exit 1
fi

echo "Diffing against committed man pages in $COMMITTED_DIR..."

diff_output=$(diff -r "$COMMITTED_DIR" "$TEMP_MAN_DIR" 2>&1) || diff_status=$?
diff_status=${diff_status:-0}

if [ "$diff_status" -eq 0 ]; then
    echo "OK: Man pages are in sync."
    exit 0
elif [ "$diff_status" -eq 1 ]; then
    echo ""
    echo "ERROR: Drift detected between committed man pages and current CLI source."
    echo "   Run 'make regen-man' and commit the updated .1 files."
    echo ""
    echo "--- diff output ---"
    echo "$diff_output"
    exit 1
else
    echo "ERROR: diff exited with error (status $diff_status)."
    echo "$diff_output"
    exit 2
fi
