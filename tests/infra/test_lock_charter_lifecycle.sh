#!/usr/bin/env bash
# tests/infra/test_lock_charter_lifecycle.sh — integration gate for the
# task module-lock charter lifecycle end-to-end behavior.
#
# §8 boundary-test table from docs/prds/task-lock-charter-lifecycle.md:
#   Rows 1-2   — guard predicate (always-on, drives real α scripts/lock-charter-guard.sh)
#   Row 3      — C-P3 predicate determinism / no-drift (shared α/γ test vector)
#   Rows 4-5   — set-to-plan release + waiter dispatch (C-S1, curl-stub hermetic)
#   Rows 6-7   — BRE acquire-before-edit + no-release-pre-acquire (C-S2/C-K1)
#   Row 8      — staleness re-pend + revalidate preserved (C-K1)
#   Rows 9-10  — anti-anchored first architect + revalidation-exempt (C-A1/C-A2)
#   Row 13     — live submit-site dir-reject smoke (opt-in REIFY_LOCK_CHARTER_LIVE=1)
#
# Architecture: two-mode harness.
#   HERMETIC (always-on, merge-gate GREEN): rows 1-3 drive the real predicate;
#     rows 4-13 use PATH-stubbed curl with canned JSON-RPC MCP responses.
#   OPT-IN LIVE (REIFY_LOCK_CHARTER_LIVE=1): scheduler/submit rows drive the
#     real fused-memory HTTP MCP.  Without the flag the scheduler scenarios
#     SKIP cleanly (exit 0 + clear message) — never auto-enabled by reachability.
#
# Auto-discovered by tests/infra/run_all.sh (test_*.sh glob).
# Lib (lock_charter_harness_lib.sh) is *_lib.sh so it stays out of the glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: tests/infra/test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

[ -f "$SCRIPT_DIR/lock_charter_harness_lib.sh" ] || {
    echo "ERROR: lock_charter_harness_lib.sh not found at $SCRIPT_DIR/lock_charter_harness_lib.sh" >&2
    exit 1
}
# shellcheck source=tests/infra/lock_charter_harness_lib.sh
source "$SCRIPT_DIR/lock_charter_harness_lib.sh"

echo "=== Lock-charter lifecycle integration gate (task #4678) ==="

# ──────────────────────────────────────────────────────────────────────────────
# §8 rows 1-3: Guard surface — always-on, drives real α predicate
# (scripts/lock-charter-guard.sh — landed by task #4676)
# ──────────────────────────────────────────────────────────────────────────────
echo "--- §8 rows 1-3: guard surface (always-on, drives real α predicate) ---"

# Row 1 (OBSERVED firing — G6 non-vacuous mandate): dir-path is REJECT
lcl_run_guard classify "crates/reify-eval/src/"
assert "row 1: classify dir 'crates/reify-eval/src/' exits 1 (REJECT)" test "$LCL_GUARD_RC" -eq 1
assert "row 1: classify dir stdout contains REJECT" test "${LCL_GUARD_OUT#*REJECT}" != "$LCL_GUARD_OUT"

# Negative control: proves the positive assertion is non-vacuous
lcl_run_guard classify "crates/x/src/foo.rs"
assert "row 1 neg: classify file 'crates/x/src/foo.rs' exits 0 (ACCEPT)" test "$LCL_GUARD_RC" -eq 0
assert "row 1 neg: classify file stdout contains ACCEPT" test "${LCL_GUARD_OUT#*ACCEPT}" != "$LCL_GUARD_OUT"

# Row 2: check with empty stdin ([] defer-to-architect value) → ACCEPT
lcl_run_guard check </dev/null
assert "row 2: check empty stdin exits 0 (ACCEPT)" test "$LCL_GUARD_RC" -eq 0

# Row 3 (C-P3 determinism/no-drift):
# (a) --list-extensions equals the canonical α/γ shared test vector
_lcl_canonical="$(lcl_canonical_extensions)"
lcl_run_guard --list-extensions
assert "row 3a: --list-extensions exits 0" test "$LCL_GUARD_RC" -eq 0
assert "row 3a: --list-extensions matches canonical α/γ test vector" test "$LCL_GUARD_OUT" = "$_lcl_canonical"
# (b) same dir path yields byte-identical REJECT via both classify and check invocation styles
lcl_run_guard classify "crates/reify-eval/src/"
_lcl_classify_out="$LCL_GUARD_OUT"
_lcl_classify_rc="$LCL_GUARD_RC"
lcl_run_guard check "crates/reify-eval/src/"
assert "row 3b: classify+check agree on exit code for dir" test "$LCL_GUARD_RC" -eq "$_lcl_classify_rc"
assert "row 3b: classify+check produce byte-identical REJECT verdict" test "$LCL_GUARD_OUT" = "$_lcl_classify_out"

test_summary
