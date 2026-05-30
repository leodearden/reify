#!/usr/bin/env bash
# Shared serialization-timing bounds for OCCT flock-gate Tests 20 and 21B
# (tests/infra/test_occt_flock_gate.sh).
#
# WHY A SHARED LIB:
# Tests 20 and 21B both spawn 3 concurrent wrapper invocations with N=2 slots
# and assert wall-clock is within [LOW,HIGH]ms to prove the 3rd was serialized.
# Extracting the constants and predicate here ensures the two tests cannot drift
# out of sync (one source of truth) and makes the bounds unit-testable with
# synthetic inputs (see test_occt_flock_gate_bounds.sh), avoiding another
# sleep-based timing test that could itself flake under load.
#
# UPPER BOUND RATIONALE (esc-3939-94):
# The original upper bound of 1200ms was raised to 2000ms because the merge-queue
# verify pipeline runs concurrently with cargo clippy + OCCT/GUI builds, inflating
# process-spawn and flock-acquire latency of the serialized 3rd invocation.
# An observed run measured 1473ms (FAIL) while its semantically identical twin
# Test 21B measured 948ms (PASS) in the SAME run — non-determinism, not a logic
# defect. On an idle host both pass deterministically (2026-05-30: Test 20=984ms,
# Test 21B=939ms, 41 passed/0 failed).
#
# At 2000ms the upper bound no longer discriminates N=2 (~800ms) from fully-serial
# N=1 (~1200ms); it becomes a load-tolerant sanity ceiling that still flags gross
# wedges (a true hang lands in LOCK_WAIT/timeout territory, orders of magnitude
# larger). The >=700ms LOWER bound remains the serialization proof (all-parallel
# N>=3 finishes ~400ms), and Test 19 independently guards the 2-invocation
# parallel case (<900ms).

# Source guard — prevent double-sourcing.
if [ "${_REIFY_OCCT_FLOCK_GATE_LIB_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_OCCT_FLOCK_GATE_LIB_SH_SOURCED=1

# Lower bound (ms): proves the 3rd invocation was serialized.
# All-parallel N>=3 finishes ~400ms; >=700ms means at least one invocation waited.
OCCT_SERIAL3_N2_LOW_MS=700

# Upper bound (ms): load-tolerant sanity ceiling, raised 1200->2000 per esc-3939-94.
# An observed 1473ms under merge-queue verify load was misidentified as a failure;
# 2000ms clears this with ~35% headroom while still catching gross wedges.
OCCT_SERIAL3_N2_HIGH_MS=2000

# occt_serial3_n2_within_bounds MS
# Returns 0 (success) if MS is in [OCCT_SERIAL3_N2_LOW_MS, OCCT_SERIAL3_N2_HIGH_MS].
# Returns 1 (failure) otherwise.
occt_serial3_n2_within_bounds() {
    local ms="$1"
    [ "$ms" -ge "$OCCT_SERIAL3_N2_LOW_MS" ] && [ "$ms" -le "$OCCT_SERIAL3_N2_HIGH_MS" ]
}
