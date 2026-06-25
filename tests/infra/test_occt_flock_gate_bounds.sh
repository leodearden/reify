#!/usr/bin/env bash
# Deterministic unit tests for the occt_flock_gate_lib.sh bounds predicate.
# Tests the occt_serial3_n2_within_bounds predicate with SYNTHETIC elapsed
# values only — no real wrapper invocations, no sleeps, cannot flake under load.
#
# See tests/infra/occt_flock_gate_lib.sh for the bounds constants and rationale
# (esc-3939-94: upper bound raised 1200->2000->5000ms for load tolerance).

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

assert "accepts 2000 (former upper edge — still accepted post-5000ms raise)" \
    occt_serial3_n2_within_bounds 2000

assert "accepts 3317 (esc task/3443 loaded run — raised 2000->5000 to clear)" \
    occt_serial3_n2_within_bounds 3317

assert "accepts 5000 (upper edge, exact upper bound)" \
    occt_serial3_n2_within_bounds 5000

# -- Tests: values that must be REJECTED (outside [LOW,HIGH]ms) ---------------
echo ""
echo "--- Rejected values (outside [${OCCT_SERIAL3_N2_LOW_MS},${OCCT_SERIAL3_N2_HIGH_MS}]ms) ---"

assert "rejects 400 (all-parallel N>=3, no serialization — lower-bound proof must stay tight)" \
    reject_bound 400

assert "rejects 699 (just below lower bound)" \
    reject_bound 699

assert "rejects 6000 (beyond load-tolerance ceiling — ceiling still bounded)" \
    reject_bound 6000

# ============================================================================
# Unit tests for occt_max_concurrent_holders (R-technique predicate)
# PRD docs/prds/infra-test-wallclock-deflake.md §2/T3
#
# Purely synthetic log inputs — no real wrapper invocations, no sleeps, cannot
# flake under load.  Mirrors the occt_serial3_n2_within_bounds pattern above.
#
# Log format (lib_slot_acquire.sh REIFY_SLOT_EVENT_LOG contract):
#   <epoch_ns> <pid> ACQUIRE slot-N
#   <epoch_ns> <pid> RELEASE
# sort -n orders by the leading epoch-ns field (concurrent O_APPEND may scramble
# physical line order; ns timestamps give the canonical ordering).
# ============================================================================
echo ""
echo "--- occt_max_concurrent_holders: R-technique event-log predicate ---"

# (a) PARALLEL log: two ACQUIRE lines before any RELEASE → 2 [GREEN case]
_f_par="$(mktemp)"
printf '100 1111 ACQUIRE slot-1\n200 2222 ACQUIRE slot-2\n300 1111 RELEASE\n400 2222 RELEASE\n' \
    > "$_f_par"
assert "max_concurrent_holders: PARALLEL log (A/A/R/R) → 2" \
    test "$(occt_max_concurrent_holders "$_f_par")" -eq 2
rm -f "$_f_par"

# (b) SERIALIZED log: A/R/A/R interleave → 1
# [NON-VACUOUS catch: a >=2 gate must REJECT this, so the live assertion goes
#  RED under an N→1 serialization regression]
_f_ser="$(mktemp)"
printf '100 1111 ACQUIRE slot-1\n200 1111 RELEASE\n300 2222 ACQUIRE slot-1\n400 2222 RELEASE\n' \
    > "$_f_ser"
assert "max_concurrent_holders: SERIALIZED log (A/R/A/R) → 1 (proves N→1 regression goes RED)" \
    test "$(occt_max_concurrent_holders "$_f_ser")" -eq 1
rm -f "$_f_ser"

# (c) THREE-invocation N=2 log → 2 (cap honored, never 3)
_f_3inv="$(mktemp)"
printf '100 1111 ACQUIRE slot-1\n200 2222 ACQUIRE slot-2\n300 1111 RELEASE\n400 2222 RELEASE\n500 3333 ACQUIRE slot-1\n600 3333 RELEASE\n' \
    > "$_f_3inv"
assert "max_concurrent_holders: THREE-invocation N=2 log → 2 (cap honored, never 3)" \
    test "$(occt_max_concurrent_holders "$_f_3inv")" -eq 2
rm -f "$_f_3inv"

# (d) SCRAMBLED physical line order: epoch-ns field still orders to A/A/R/R → 2
# [proves helper sorts by ns field, not physical append order]
_f_scr="$(mktemp)"
printf '200 2222 ACQUIRE slot-2\n100 1111 ACQUIRE slot-1\n400 2222 RELEASE\n300 1111 RELEASE\n' \
    > "$_f_scr"
assert "max_concurrent_holders: SCRAMBLED lines (epoch-ns orders A/A/R/R) → 2" \
    test "$(occt_max_concurrent_holders "$_f_scr")" -eq 2
rm -f "$_f_scr"

# (e) Empty log → 0
_f_empty="$(mktemp)"
assert "max_concurrent_holders: EMPTY log → 0" \
    test "$(occt_max_concurrent_holders "$_f_empty")" -eq 0
rm -f "$_f_empty"

test_summary
