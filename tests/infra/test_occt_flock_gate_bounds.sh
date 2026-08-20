#!/usr/bin/env bash
# Deterministic unit tests for the occt_flock_gate_lib.sh helpers.
# The bounds / event-log predicates (occt_serial3_n2_within_bounds,
# occt_max_concurrent_holders) run on SYNTHETIC inputs only — no real wrapper
# invocations, no sleeps, cannot flake under load.
#
# The occt_wait_until_slot_held barrier test (task 5258) DOES spawn a real
# background `flock` holder, but it asserts only a CAUSAL OUTCOME — the helper
# returns 0 once a holder holds the slot, non-zero when a free slot is never
# held within a tiny bound — NEVER an elapsed-time magnitude, so it too cannot
# flake under load.
#
# See tests/infra/occt_flock_gate_lib.sh for the helpers and their rationale
# (esc-3939-94: bounds upper edge raised 1200->2000->5000ms for load tolerance;
# task 5258: the causal flock-probe barrier + plan-grep-or-dump helpers).

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

# ============================================================================
# Unit tests for occt_wait_until_slot_held (causal flock-probe barrier)
# PRD docs/prds/merge-gate-health.md W4b (task 5258).
#
# UNLIKE the purely-synthetic predicates above, this barrier spawns a REAL
# background `flock` holder — but it asserts only the CAUSAL OUTCOME (the helper
# returns 0 once a holder holds the slot / non-zero when a free slot is never
# held within the bound), NEVER an elapsed magnitude.  Poll count varies with
# host load; the 0/non-zero outcome is deterministic, so this cannot flake.
# ============================================================================
echo ""
echo "--- occt_wait_until_slot_held: causal flock-probe barrier ---"

# POSITIVE: a live holder actually holds the slot → barrier confirms (returns 0).
# Spawn a real background flock holder on a dedicated slot file (${base}.slot-*
# idiom from the main suite's holders), then assert the barrier detects the held
# lock.  The barrier's `9>>"$slot"` open self-heals a probe that races ahead of
# the holder (both converge on the same inode).
_s_held="$(mktemp)"
( flock -x 9; sleep 5 ) 9>>"${_s_held}.slot-probe" &
_HOLDER_PROBE=$!
assert "occt_wait_until_slot_held: confirms a live holder (returns 0)" \
    occt_wait_until_slot_held "${_s_held}.slot-probe"
kill "$_HOLDER_PROBE" 2>/dev/null || true
wait "$_HOLDER_PROBE" 2>/dev/null || true
rm -f "$_s_held" "${_s_held}.slot-probe"

# NEGATIVE: a fresh FREE slot is never held → barrier exhausts its (tiny) bound
# and returns non-zero.  max_iters=2 (~0.4s) keeps the negative case fast.  The
# `bash -c "! ..."` form runs the EXPORTED helper (export -f in the lib), so the
# negation reflects the helper's REAL timeout return — not a command-not-found.
_s_free="$(mktemp)"
assert "occt_wait_until_slot_held: free slot times out within bound (non-zero)" \
    bash -c "! occt_wait_until_slot_held '${_s_free}.slot-freeprobe' 2"
rm -f "$_s_free" "${_s_free}.slot-freeprobe"

# ============================================================================
# Unit tests for occt_plan_grep_or_dump (plan grep with on-no-match stderr dump)
# PRD docs/prds/merge-gate-health.md W4d tail (task 5258).
#
# Purely synthetic — no flock, no verify.sh, no sleeps.  Greps a plan string for
# an ERE pattern; on NO-MATCH it echoes the captured child-plan stderr (errfile)
# to STDOUT so the assert() on-FAIL capture-dump (test_helpers.sh, esc-4959-57)
# surfaces the otherwise-swallowed --print-plan diagnostics, and returns non-zero.
# ============================================================================
echo ""
echo "--- occt_plan_grep_or_dump: plan grep with on-no-match stderr dump ---"

# (a) pattern present in the plan → returns 0 (no dump).
assert "occt_plan_grep_or_dump: pattern present => returns 0" \
    occt_plan_grep_or_dump 'nextest run --workspace' 'x timeout 60m cargo nextest run --workspace y' /dev/null

# (b) pattern absent → non-zero.  `bash -c "! ..."` runs the EXPORTED helper so
#     the negation reflects the real return, not a vacuous command-not-found.
assert "occt_plan_grep_or_dump: pattern absent => non-zero" \
    bash -c "! occt_plan_grep_or_dump 'ABSENT_XYZ' 'some plan text' /dev/null"

# (c) DUMP proof: on no-match the captured child stderr (errfile) is echoed to
#     stdout, so a real assert failure would surface it via the on-FAIL dump.
_errf="$(mktemp)"
printf 'SENTINEL_DIAG_XYZ\n' > "$_errf"
_dumpf="$(mktemp)"
occt_plan_grep_or_dump 'ABSENT_XYZ' 'some plan text' "$_errf" > "$_dumpf" 2>&1 || true
assert "occt_plan_grep_or_dump: no-match dumps captured child stderr (sentinel present)" \
    grep -q SENTINEL_DIAG_XYZ "$_dumpf"

# (d) NO spurious dump: on a MATCH the errfile is NOT echoed (sentinel absent),
#     so an all-green run stays byte-for-byte unchanged.
_dumpf2="$(mktemp)"
occt_plan_grep_or_dump 'plan' 'some plan text' "$_errf" > "$_dumpf2" 2>&1 || true
assert "occt_plan_grep_or_dump: match => no stderr dump (sentinel absent)" \
    bash -c "! grep -q SENTINEL_DIAG_XYZ '$_dumpf2'"

rm -f "$_errf" "$_dumpf" "$_dumpf2"

test_summary
