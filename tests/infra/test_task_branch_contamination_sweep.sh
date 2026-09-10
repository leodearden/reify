#!/usr/bin/env bash
# tests/infra/test_task_branch_contamination_sweep.sh
# Hermetic tests for scripts/task-branch-contamination-sweep.sh (task #7244).
#
# The SUT is a read-only, non-gating audit primitive with two modes:
#   --task <id>   single branch (the per-merge advisory consult seam)
#   --audit       fleet sweep
# It reads a Taskmaster store and a git repo and prints a report. It must
# never write to either, and must exit 0 on every valid invocation in both
# modes — 2 is reserved for usage errors, so no caller can gate on its status
# by accident.
#
# run_helper captures STDOUT, STDERR and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks (added incrementally across task #7244's TDD steps):
#   step-5  — arg-parsing / usage taxonomy
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/task-branch-contamination-sweep.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/task-branch-contamination-sweep.sh hermetic tests (task 7244) ==="

# ─────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ─────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

ERR_FILE="$(mktemp "${TMPDIR:-/tmp}/test-task-branch-sweep-err-XXXXXX")"
_TMPDIRS+=("$ERR_FILE")

# ── run_helper ────────────────────────────────────────────────────────────────
# Invokes the script under test with no PATH stub.
# Sets OUT (stdout), ERR_OUT (stderr), RC (exit code) as globals.
run_helper() {
    local rc=0
    > "$ERR_FILE"
    OUT="$(bash "$SCRIPT" "$@" 2>"$ERR_FILE")" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# A missing SUT cannot degrade into a partial report: every assertion below
# invokes it. Report the absence as one FAIL line (so run_all.sh still gets a
# parseable Results line) and stop.
if [ ! -f "$SCRIPT" ]; then
    assert "scripts/task-branch-contamination-sweep.sh exists" test -f "$SCRIPT"
    test_summary
    exit 1
fi

# ── usage-error assertion bundle ──────────────────────────────────────────────
# Every usage error must satisfy all three halves of the contract at once:
# exit 2, a diagnostic on stderr, and NOTHING on stdout. Bundling them means a
# new usage case cannot accidentally assert only the exit code — the stdout
# half is the one that matters most here, because stdout is the SUT's only
# result channel and a caller parsing it must never see a half-written report.
assert_usage_error() {
    local desc="$1"; shift
    run_helper "$@"
    assert "U[$desc]: exits 2" test "$RC" -eq 2
    assert "U[$desc]: writes a diagnostic to stderr" test -n "$ERR_OUT"
    assert "U[$desc]: writes NOTHING to stdout" test -z "$OUT"
}

# ─────────────────────────────────────────────────────────────────────────────
# Block 1 (step-5) — arg-parsing / usage taxonomy
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 1: arg parsing and usage taxonomy ---"

assert_usage_error "unknown flag" --audit --no-such-flag
assert_usage_error "unknown short flag" --audit -Z

# Every value-taking flag, checked for the "given as the last argument with no
# value" shape. Enumerated one by one rather than looped over, so a flag that
# silently stops taking a value shows up as its own named FAIL.
assert_usage_error "--db without a value"            --audit --db
assert_usage_error "--tag without a value"           --audit --tag
assert_usage_error "--repo without a value"          --audit --repo
assert_usage_error "-C without a value"              --audit -C
assert_usage_error "--main-ref without a value"      --audit --main-ref
assert_usage_error "--branch-prefix without a value" --audit --branch-prefix
assert_usage_error "--format without a value"        --audit --format
assert_usage_error "--task without a value"          --task

assert_usage_error "invalid --format value"   --audit --format yaml
assert_usage_error "empty --format value"     --audit --format ''

assert_usage_error "both --task and --audit"  --task 1 --audit
assert_usage_error "neither --task nor --audit"

assert_usage_error "non-numeric --task"       --task abc
assert_usage_error "negative --task"          --task -1
assert_usage_error "mixed alnum --task"       --task 12x
assert_usage_error "empty --task"             --task ''

assert_usage_error "unexpected positional argument" --audit extra
assert_usage_error "bare positional argument"       12345

# ── help ──────────────────────────────────────────────────────────────────────
# Usage goes to STDERR and exits 0, matching the
# warm-lane-degenerate-ref-check.sh precedent: stdout is reserved for the
# report, so even --help must not put a byte on it.
for _h in -h --help; do
    run_helper "$_h"
    assert "H[$_h]: exits 0" test "$RC" -eq 0
    assert "H[$_h]: prints usage to stderr" \
        bash -c 'printf "%s\n" "$1" | grep -qi "usage:"' _ "$ERR_OUT"
    assert "H[$_h]: prints NOTHING to stdout" test -z "$OUT"
done

# --help wins over an otherwise-invalid invocation, so a confused caller gets
# the usage text rather than a bare exit 2.
run_helper --help --no-such-flag
assert "H[--help beats a later bad flag]: exits 0" test "$RC" -eq 0

test_summary
