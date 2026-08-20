#!/usr/bin/env bash
# tests/infra/test_run_all_ambient_isolation_lib.sh — unit test for
# ambient_isolation_check_one() in run_all_ambient_isolation_lib.sh (task 5259,
# PRD docs/prds/merge-gate-health.md W4c).
#
# The lib function decides, per ambient ledger var, whether test_run_all.sh
# going red under a HOSTILE ambient env is a GENUINE ambient-isolation bug or
# merely DERIVATIVE of a pre-existing (env-independent) test_run_all.sh
# failure. Only the former must fail the guard; the latter must be SKIPped
# distinctly so run_all.sh's classifier does not double-count one
# test_run_all.sh failure as two FAILED names (W4c).
#
# This is the only place ambient_isolation_check_one can be exercised in
# isolation: the consumer test_run_all_ambient_isolation.sh cannot self-test
# (self-invocation recurses — see its header) and drives the REAL suite. Here
# we drive the function against tiny FAKE target scripts, so each decision
# branch (SKIP / FAIL / PASS) is hermetic and fast (no real ~103-test suite,
# no --print-plan derivation).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

[ -f "$SCRIPT_DIR/plan_capture_lib.sh" ] || { echo "ERROR: plan_capture_lib.sh not found at $SCRIPT_DIR/plan_capture_lib.sh"; exit 1; }
source "$SCRIPT_DIR/plan_capture_lib.sh"

[ -f "$SCRIPT_DIR/run_all_ambient_isolation_lib.sh" ] || { echo "ERROR: run_all_ambient_isolation_lib.sh not found at $SCRIPT_DIR/run_all_ambient_isolation_lib.sh"; exit 1; }
source "$SCRIPT_DIR/run_all_ambient_isolation_lib.sh"

echo "=== ambient_isolation_check_one unit test (task 5259 / W4c) ==="

# Hermetic scratch dir for the fake target scripts (one per sub-case).
FAKE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/reify-ambient-iso-lib.XXXXXX")"
trap 'rm -rf "$FAKE_DIR"' EXIT

# Synthetic ambient ledger: the keys the lib UNSETs before each run (and, for
# the hostile run, re-exports the one under test). Includes the key the fakes
# react to (REIFY_RUN_ALL_EXCLUDE_HOST_INFRA) so the clean-env baseline is
# genuinely clean even when this test itself runs under the merge-tier verify
# env (which ambiently exports that var).
MKEYS=$'REIFY_RUN_ALL_EXCLUDE_HOST_INFRA\nREIFY_GATE_EXCLUDE_HEAVY'

# ---------------------------------------------------------------------------
# Sub-case A "forced-red-both": a fake test_run_all.sh that is red REGARDLESS
# of env (always exits 1). Hostile red + baseline red => DERIVATIVE failure =>
# the function must emit the distinct SKIP marker and return verdict 0 (so
# run_all.sh's exit-code classifier names only test_run_all.sh, not this too).
# ---------------------------------------------------------------------------
echo ""
echo "--- Sub-case A: forced-red-both (derivative failure -> SKIP) ---"

FAKE_RED_BOTH="$FAKE_DIR/fake_red_both.sh"
cat > "$FAKE_RED_BOTH" <<'SH'
#!/usr/bin/env bash
# Red independent of ambient env — stands in for a test_run_all.sh that is
# already broken for an unrelated reason.
echo "Results: 2 passed, 1 failed"
exit 1
SH
chmod +x "$FAKE_RED_BOTH"

_rc=0
_out="$(ambient_isolation_check_one "$FAKE_RED_BOTH" REIFY_RUN_ALL_EXCLUDE_HOST_INFRA 1 "$MKEYS" 2>&1)" || _rc=$?

assert "forced-red-both: emits the distinct SKIP marker (derivative failure, not an isolation bug)" \
    plan_match "$_out" 'SKIP: baseline test_run_all\.sh already red \(not an isolation bug\)'

assert "forced-red-both: verdict rc is 0 (derivative failure is not counted) [rc=$_rc]" \
    test "$_rc" -eq 0

# ---------------------------------------------------------------------------
# Sub-case B "isolation-bug": a fake test_run_all.sh that is GREEN under a
# clean env but RED when REIFY_RUN_ALL_EXCLUDE_HOST_INFRA is set. Hostile red
# + baseline GREEN => the hostile ambient env ALONE flipped it red => a
# GENUINE ambient-isolation bug => the function must FAIL (return 1) and must
# NOT emit the derivative SKIP marker (isolation coverage must be preserved,
# not masked).
# ---------------------------------------------------------------------------
echo ""
echo "--- Sub-case B: isolation-bug (hostile-red only -> FAIL) ---"

FAKE_ISO_BUG="$FAKE_DIR/fake_iso_bug.sh"
cat > "$FAKE_ISO_BUG" <<'SH'
#!/usr/bin/env bash
# Red ONLY when the ambient var is present — a genuine isolation bug.
if [ -n "${REIFY_RUN_ALL_EXCLUDE_HOST_INFRA:-}" ]; then
    echo "Results: 2 passed, 1 failed"
    exit 1
fi
echo "Results: 3 passed, 0 failed"
exit 0
SH
chmod +x "$FAKE_ISO_BUG"

_rc=0
_out="$(ambient_isolation_check_one "$FAKE_ISO_BUG" REIFY_RUN_ALL_EXCLUDE_HOST_INFRA 1 "$MKEYS" 2>&1)" || _rc=$?

assert "isolation-bug: verdict rc is 1 (genuine ambient-isolation bug fails the guard) [rc=$_rc]" \
    test "$_rc" -eq 1

if plan_match "$_out" 'SKIP: baseline test_run_all\.sh already red'; then
    assert "isolation-bug: must NOT emit the derivative SKIP marker (got: $_out)" false
else
    assert "isolation-bug: does not emit the derivative SKIP marker (coverage preserved)" true
fi

# ---------------------------------------------------------------------------
# Sub-case C "green happy-path": a fake test_run_all.sh that is GREEN
# regardless of env. Hostile GREEN => PASS immediately (no baseline run) =>
# return 0, no SKIP marker, PASS line present. Rides along as a regression
# guard that the common all-green path is never mis-skipped.
# ---------------------------------------------------------------------------
echo ""
echo "--- Sub-case C: green happy-path (hostile green -> PASS) ---"

FAKE_GREEN="$FAKE_DIR/fake_green.sh"
cat > "$FAKE_GREEN" <<'SH'
#!/usr/bin/env bash
# Green regardless of ambient env.
echo "Results: 4 passed, 0 failed"
exit 0
SH
chmod +x "$FAKE_GREEN"

_rc=0
_out="$(ambient_isolation_check_one "$FAKE_GREEN" REIFY_RUN_ALL_EXCLUDE_HOST_INFRA 1 "$MKEYS" 2>&1)" || _rc=$?

assert "green happy-path: verdict rc is 0 (no isolation bug) [rc=$_rc]" \
    test "$_rc" -eq 0

if plan_match "$_out" 'SKIP: baseline test_run_all\.sh already red'; then
    assert "green happy-path: must NOT emit the SKIP marker (got: $_out)" false
else
    assert "green happy-path: does not emit the SKIP marker" true
fi

assert "green happy-path: emits the hostile-green PASS line" \
    plan_match "$_out" 'AMBIENT-ISOLATION PASS:'

test_summary
