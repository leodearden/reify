#!/usr/bin/env bash
# Infrastructure test for task 5738 (the shared warm-lane lock probe).
#
# This wrapper is the member run_all.sh actually discovers: it globs
# `test_*.sh` only, mirrored by classification_discovered_set in
# run-all-classification-lib.sh. The real assertions live in the sibling .py;
# without this file they would be silently never run, reading as coverage
# while asserting nothing.
#
# Covers scripts/lib_lane_lock.sh — the SINGLE tri-state lock probe
# (IDLE/BUSY/UNMEASURABLE) shared by scripts/warm-lane-audit.sh and
# scripts/warm-lane-lock-guard.sh. The lib owns the measurement and the two
# invariants both callers share; each caller's own fail direction on
# UNMEASURABLE is asserted in ITS suite (the guard's Block D, the audit's
# Block R). Reasoning: docs/design/merge-verify-lane-dispatch-seam.md §3.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_lane_lock_probe.py (stdlib unittest) exits 0
#   3. the lib is sourceable and defines lane_lock_probe (sourcing smoke)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_lane_lock_probe ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── Unit tests ────────────────────────────────────────────────────────────
assert "tests/infra/test_lane_lock_probe.py exits 0" \
    python3 "$SCRIPT_DIR/test_lane_lock_probe.py"

# ── Sourcing smoke ────────────────────────────────────────────────────────
assert "scripts/lib_lane_lock.sh is sourceable and defines lane_lock_probe" \
    bash -c 'set -euo pipefail; source "$1"; declare -F lane_lock_probe' \
    _ "$ROOT/scripts/lib_lane_lock.sh"

test_summary
