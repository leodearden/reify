#!/usr/bin/env bash
# Deterministic unit tests for the occt_flock_gate_lib.sh bounds predicate.
# Tests the occt_serial3_n2_within_bounds predicate with SYNTHETIC elapsed
# values only — no real wrapper invocations, no sleeps, cannot flake under load.
#
# See tests/infra/occt_flock_gate_lib.sh for the bounds constants and rationale
# (esc-3939-94: upper bound raised 1200->2000ms for load tolerance).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

[ -f "$SCRIPT_DIR/occt_flock_gate_lib.sh" ] || { echo "ERROR: occt_flock_gate_lib.sh not found at $SCRIPT_DIR/occt_flock_gate_lib.sh"; exit 1; }
source "$SCRIPT_DIR/occt_flock_gate_lib.sh"

echo "=== occt_flock_gate_lib.sh bounds predicate unit tests ==="

# Helper for negative (must-reject) assertions: succeeds when predicate rejects.
reject_bound() { ! occt_serial3_n2_within_bounds "$1"; }

# -- Tests: values that must be ACCEPTED (within [LOW,HIGH]ms) ----------------
echo ""
echo "--- Accepted values (within [${OCCT_SERIAL3_N2_LOW_MS},${OCCT_SERIAL3_N2_HIGH_MS}]ms) ---"

assert "accepts 700 (lower edge, exact lower bound)" \
    occt_serial3_n2_within_bounds 700

assert "accepts 800 (typical idle N=2 serialized result ~800ms)" \
    occt_serial3_n2_within_bounds 800

assert "accepts 1473 (esc-3939-94 loaded serialized run — core regression guard)" \
    occt_serial3_n2_within_bounds 1473

assert "accepts 2000 (upper edge, exact upper bound)" \
    occt_serial3_n2_within_bounds 2000

# -- Tests: values that must be REJECTED (outside [LOW,HIGH]ms) ---------------
echo ""
echo "--- Rejected values (outside [${OCCT_SERIAL3_N2_LOW_MS},${OCCT_SERIAL3_N2_HIGH_MS}]ms) ---"

assert "rejects 400 (all-parallel N>=3, no serialization — lower-bound proof must stay tight)" \
    reject_bound 400

assert "rejects 699 (just below lower bound)" \
    reject_bound 699

assert "rejects 2100 (beyond load-tolerance ceiling — ceiling still bounded)" \
    reject_bound 2100

test_summary
