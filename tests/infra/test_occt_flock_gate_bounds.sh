#!/usr/bin/env bash
# Deterministic unit tests for the occt_flock_gate_lib.sh helpers.
# occt_serial3_n2_serialized runs on SYNTHETIC inputs only — no real wrapper
# invocations, no sleeps, cannot flake under load.
#
# SCOPE: this file covers what is OCCT-SPECIFIC. The general causal primitives
# (holder_max_concurrent, holder_wait_until_held, and the gated-owner pair) live
# in tests/infra/slot_holder_handshake_lib.sh and are unit-tested ONCE, in
# tests/infra/test_slot_holder_handshake_lib.sh. They used to be re-exported
# from occt_flock_gate_lib.sh under occt_* names and re-tested here over the
# very same five synthetic fixtures; the forwarders and the duplicate fixtures
# are both gone (task 6247 amendment). Nothing was dropped — the shared file's
# cases are a superset, and it adds the gated-owner coverage this file never had.
#
# WHY THE MILLISECOND BAND IS GONE (PRD docs/prds/infra-test-wallclock-deflake.md
# decision D1, task 6247): the retired `occt_serial3_n2_within_bounds` predicate
# asserted that three N=2 invocations finished inside an absolute [700,5000]ms
# window.  That ceiling was ratcheted 1200 -> 2000 -> 5000 as the merge-queue
# grew busier and was STILL observed at 5791ms.  D1 abandons absolute wall-clock
# upper bounds for this class rather than re-tuning them once more: what the
# tests actually want to know is whether the third invocation was SERIALIZED,
# and that is a causal fact readable from the slot event log.
#
# The band was also non-discriminating in the direction that matters, as
# occt_flock_gate_lib.sh's own COVERAGE GAP note admitted: an N->1
# over-serialization regression lands three serial invocations at ~1200ms,
# comfortably INSIDE [700,5000], so the band could not see it.
# occt_serial3_n2_serialized closes that gap by asserting the maximum concurrent
# hold count is EXACTLY 2 — which separates correct N=2 (2) from
# over-serialization (1) and from a lost cap (3), all three of which the band
# accepted alike.
#
# See tests/infra/occt_flock_gate_lib.sh for the helpers and their rationale,
# and tests/infra/slot_holder_handshake_lib.sh for the shared causal primitives
# the barrier and the event-log predicate now delegate to.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

[ -f "$SCRIPT_DIR/occt_flock_gate_lib.sh" ] || { echo "ERROR: occt_flock_gate_lib.sh not found at $SCRIPT_DIR/occt_flock_gate_lib.sh"; exit 1; }
source "$SCRIPT_DIR/occt_flock_gate_lib.sh"

echo "=== occt_flock_gate_lib.sh bounds predicate unit tests ==="

# ============================================================================
# Unit tests for occt_serial3_n2_serialized (causal serialization predicate)
# PRD docs/prds/infra-test-wallclock-deflake.md §2/T3 + D1 (task 6247).
#
# The causal replacement for the retired [700,5000]ms band.  Purely synthetic
# log inputs — no real wrapper invocations, no sleeps, cannot flake under load.
# Every rejection uses the `bash -c "! ..."` form so the EXPORTED helper runs
# for real in the child shell; an unexported helper would make the negation a
# vacuous command-not-found and every rejection would pass for the wrong reason.
# ============================================================================
echo ""
echo "--- occt_serial3_n2_serialized: causal serialization predicate ---"

# ACCEPT: three invocations at N=2 — two hold concurrently, the third waits.
# This is the correct-behaviour shape the retired band was trying to describe.
_f_ok3="$(mktemp)"
printf '100 1111 ACQUIRE slot-1\n200 2222 ACQUIRE slot-2\n300 1111 RELEASE\n400 2222 RELEASE\n500 3333 ACQUIRE slot-1\n600 3333 RELEASE\n' \
    > "$_f_ok3"
assert "serial3_n2_serialized: three invocations at N=2 (max 2) => accepted" \
    occt_serial3_n2_serialized "$_f_ok3"
rm -f "$_f_ok3"

# REJECT: N->1 over-serialization (max 1).  THIS IS THE COVERAGE GAP the ms band
# admitted it could not see: three fully-serial invocations land ~1200ms, inside
# [700,5000], so the band accepted the regression.
_f_over="$(mktemp)"
printf '100 1111 ACQUIRE slot-1\n200 1111 RELEASE\n300 2222 ACQUIRE slot-1\n400 2222 RELEASE\n500 3333 ACQUIRE slot-1\n600 3333 RELEASE\n' \
    > "$_f_over"
assert "serial3_n2_serialized: N->1 over-serialization (max 1) => rejected (closes the ms-band coverage gap)" \
    bash -c "! occt_serial3_n2_serialized '$_f_over'"
rm -f "$_f_over"

# REJECT: all three holding at once (max 3) — the under-serialization regression
# the retired >=700ms floor used to cover.  That coverage is preserved here.
_f_under="$(mktemp)"
printf '100 1111 ACQUIRE slot-1\n200 2222 ACQUIRE slot-2\n300 3333 ACQUIRE slot-3\n400 1111 RELEASE\n500 2222 RELEASE\n600 3333 RELEASE\n' \
    > "$_f_under"
assert "serial3_n2_serialized: all three holding at once (max 3) => rejected (slot cap lost)" \
    bash -c "! occt_serial3_n2_serialized '$_f_under'"
rm -f "$_f_under"

# REJECT: empty log (max 0) — a wrapper that never ran, or an event log that was
# never wired, must not pass vacuously.  The band had no equivalent guard.
_f_none="$(mktemp)"
assert "serial3_n2_serialized: empty log (max 0) => rejected (a wrapper that never ran cannot pass)" \
    bash -c "! occt_serial3_n2_serialized '$_f_none'"
rm -f "$_f_none"

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
