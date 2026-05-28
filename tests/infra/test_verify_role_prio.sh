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

# ---------------------------------------------------------------------------
# Cycle B: CARGO_PRIO prefix-wrapping contract
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle B: prefix-wrapping contract ---"

# Capture command lines for each role/action combination.
# grep -v '^#' strips env-export and comment lines; command lines remain.
TASK_TEST_PLAN="$(DF_VERIFY_ROLE=task bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan \
    | grep -v '^#')"
TASK_ALL_PLAN="$(DF_VERIFY_ROLE=task bash "$REPO_ROOT/scripts/verify.sh" all --scope all --print-plan \
    | grep -v '^#')"
UNSET_TEST_PLAN="$(env -u DF_VERIFY_ROLE bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan \
    | grep -v '^#')"
MERGE_PLAN_FULL="$(DF_VERIFY_ROLE=merge bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan)"
MERGE_TEST_PLAN="$(printf '%s\n' "$MERGE_PLAN_FULL" | grep -v '^#')"
MERGE_ALL_PLAN_FULL="$(DF_VERIFY_ROLE=merge bash "$REPO_ROOT/scripts/verify.sh" all --scope all --print-plan)"
MERGE_ALL_PLAN="$(printf '%s\n' "$MERGE_ALL_PLAN_FULL" | grep -v '^#')"

# --- task / test / all ---

# Sanity: at least 2 cargo command lines in the plan.
# '(^| )cargo ' matches the real cargo token; 'cargo-test-occt-gated.sh' has a
# hyphen so it doesn't match the wrapper script name, only the 'cargo test' arg.
assert "task/test/all: at least 2 cargo command lines (sanity)" \
    bash -c '[ "$(printf "%s\n" "$1" | grep -cE "(^| )cargo " || echo 0)" -ge 2 ]' \
    _ "$TASK_TEST_PLAN"

# All cargo lines must carry the task prefix (zero unprefixed lines).
assert "task/test/all: all cargo lines prefixed with 'nice -n 15 ionice -c 2 -n 7 cargo'" \
    bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo " | grep -vq "nice -n 15 ionice -c 2 -n 7 cargo"' \
    _ "$TASK_TEST_PLAN"

# --- task / all / all (covers clippy + gated + ungated) ---

assert "task/all/all: at least 2 cargo command lines (sanity)" \
    bash -c '[ "$(printf "%s\n" "$1" | grep -cE "(^| )cargo " || echo 0)" -ge 2 ]' \
    _ "$TASK_ALL_PLAN"

assert "task/all/all: all cargo lines prefixed with 'nice -n 15 ionice -c 2 -n 7 cargo'" \
    bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo " | grep -vq "nice -n 15 ionice -c 2 -n 7 cargo"' \
    _ "$TASK_ALL_PLAN"

# Negative contract: non-cargo lines (tree-sitter-generate.sh, npm) must NOT carry
# the prefix.  Every plan line that contains the nice/ionice prefix must also
# contain 'cargo' — i.e. zero prefix-bearing lines lack 'cargo'.
assert "task/all/all: only cargo lines carry the nice/ionice prefix (non-cargo lines clean)" \
    bash -c '! printf "%s\n" "$1" | grep -F "nice -n 15 ionice -c 2 -n 7 " | grep -vq "cargo"' \
    _ "$TASK_ALL_PLAN"

# --- unset role defaults to task ---

assert "unset role: plan contains task prefix 'nice -n 15 ionice -c 2 -n 7 cargo'" \
    bash -c 'printf "%s\n" "$1" | grep -qF "nice -n 15 ionice -c 2 -n 7 cargo"' \
    _ "$UNSET_TEST_PLAN"

# --- merge / test / all ---

# Every cargo line prefixed with 'nice -n 5 cargo' (mild CPU nice, no ionice).
assert "merge/test/all: all cargo lines prefixed with 'nice -n 5 cargo'" \
    bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo " | grep -vq "nice -n 5 cargo"' \
    _ "$MERGE_TEST_PLAN"

# The full plan output (including header) must contain NO 'ionice'.
assert "merge/test/all: no 'ionice' anywhere in the full plan output" \
    bash -c '! printf "%s\n" "$1" | grep -q "ionice"' \
    _ "$MERGE_PLAN_FULL"

# --- merge / all / all (covers clippy + gated + ungated) ---
# Mirrors the task/all/all assertion so the merge path on lint/typecheck commands
# is also covered.

assert "merge/all/all: all cargo lines prefixed with 'nice -n 5 cargo'" \
    bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo " | grep -vq "nice -n 5 cargo"' \
    _ "$MERGE_ALL_PLAN"

assert "merge/all/all: no 'ionice' anywhere in the full plan output" \
    bash -c '! printf "%s\n" "$1" | grep -q "ionice"' \
    _ "$MERGE_ALL_PLAN_FULL"

test_summary
