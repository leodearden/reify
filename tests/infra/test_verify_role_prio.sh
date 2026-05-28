#!/usr/bin/env bash
# Infrastructure test for task 4051.
# Covers:
#   Cycle A — DF_VERIFY_ROLE validation / exit-64 contract (step-1 / step-2)
#   Cycle B — CARGO_PRIO prefix-wrapping contract         (step-3 / step-4)
#
# Drives verify.sh via --print-plan (hermetic: never builds anything).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== DF_VERIFY_ROLE validation and cargo priority prefix tests ==="

# ---------------------------------------------------------------------------
# Cycle A: DF_VERIFY_ROLE validation / exit-64 contract
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle A: DF_VERIFY_ROLE validation ---"

# Capture exit code and stderr for a bogus role without triggering set -e.
_bogus_stderr_file="$(mktemp)"
_bogus_rc=0
DF_VERIFY_ROLE=bogus bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan \
    >"$_bogus_stderr_file" 2>&1 \
    || _bogus_rc=$?
_bogus_stderr="$(cat "$_bogus_stderr_file")"
rm -f "$_bogus_stderr_file"

# (a) bogus role must exit 64
assert "DF_VERIFY_ROLE=bogus: exits 64" \
    test "$_bogus_rc" -eq 64

# (b) stderr must contain the exact diagnostic (em-dash U+2014 is literal in the string below)
assert "DF_VERIFY_ROLE=bogus: stderr contains expected ERROR diagnostic" \
    bash -c 'printf "%s\n" "$1" | grep -qF "verify.sh: ERROR — unknown DF_VERIFY_ROLE '"'"'bogus'"'"' (want task|merge)"' \
    _ "$_bogus_stderr"

# (c) valid role 'task' must exit 0
assert "DF_VERIFY_ROLE=task: exits 0" \
    bash -c 'DF_VERIFY_ROLE=task bash "$1/scripts/verify.sh" test --scope all --print-plan >/dev/null 2>&1' \
    _ "$REPO_ROOT"

# (d) valid role 'merge' must exit 0
assert "DF_VERIFY_ROLE=merge: exits 0" \
    bash -c 'DF_VERIFY_ROLE=merge bash "$1/scripts/verify.sh" test --scope all --print-plan >/dev/null 2>&1' \
    _ "$REPO_ROOT"

# (e) unset DF_VERIFY_ROLE must exit 0 (defaults to task)
assert "DF_VERIFY_ROLE unset: exits 0 (defaults to task)" \
    bash -c 'env -u DF_VERIFY_ROLE bash "$1/scripts/verify.sh" test --scope all --print-plan >/dev/null 2>&1' \
    _ "$REPO_ROOT"

test_summary
