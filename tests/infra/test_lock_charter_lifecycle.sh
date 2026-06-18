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

# (no assertions yet — prereq-1 scaffold; trivially passes)

test_summary
