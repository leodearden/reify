#!/usr/bin/env bash
# tests/infra/test_cpu_load_governance_deflake.sh — meta-test for PRD T7 (task 4846).
#
# Proves that every live cgroup-governance cycle of test_cpu_load_governance.sh
# SKIPs under the REIFY_CPU_GOVERN_DISABLE=1 break-glass.
#
# Assertions:
#   A2  the armed gates-only drive exits 0 under the `timeout 120` anti-hang guard.
#   A3  NON-VACUOUS: share_ge_proportional discriminator — broken->False, healthy->True.
#   A4  the drive emits the host-governance SKIP marker of all six live cycles.
#   A5  the drive short-circuited at the seam: the SUT printed no `Results:` line.
#   A6  GATES_ONLY without the REIFY_GOVTEST_TEST_MODE arming key is refused
#       loudly, and the suite runs on to Cycle SELF.
#
# Technique: the armed REIFY_CPU_GOV_TEST_GATES_ONLY=1 seam consults each live
# cycle's host gate and exits before any cycle runs, so the drive is sub-second
# and the guard never decides the verdict. The SUT's always-on body is its own
# run_all member's job, not this one's.
# Auto-discovered by tests/infra/run_all.sh (glob test_*.sh).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SUT="$SCRIPT_DIR/test_cpu_load_governance.sh"
INSTRUMENT="$SCRIPT_DIR/cpu_gov_instrument.py"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== cpu-load-governance de-flake meta-test (task 4846) ==="

# ── A3 — NON-VACUOUS guard (share_ge_proportional) ────────────────────────────
# Re-confirms the ROW4-1 discriminator works in both directions co-located with
# the budget-gate change, discharging the NON-VACUOUS mandate.
# Broken governance (merge_share=0.5 < floor=0.65) → False → ROW4-1 would be RED.
# Healthy governance (merge_share=0.75 >= floor=0.65) → True → ROW4-1 is GREEN.
echo ""
echo "--- A3: NON-VACUOUS guard (share_ge_proportional) ---"

if ! command -v python3 >/dev/null 2>&1; then
    echo "  SKIP A3: python3 not on PATH"
elif ! [ -f "$INSTRUMENT" ]; then
    echo "  SKIP A3: cpu_gov_instrument.py not found at $INSTRUMENT"
else
    # A3a: broken governance (merge=100,task=100 → share=0.5; floor=0.75-0.10=0.65)
    #      share_ge_proportional must return False → ROW4-1 would be RED.
    #      We assert the Python call exits 1 (not ok), meaning it correctly detected
    #      broken governance.
    assert "A3a: share_ge_proportional(100,100,300,100,0.10) is False (broken→ROW4-1 RED)" \
        bash -c '
            python3 -c "
import sys
sys.path.insert(0, sys.argv[1])
from cpu_gov_instrument import share_ge_proportional
ok = share_ge_proportional(100.0, 100.0, 300.0, 100.0, 0.10)
# We want False; exit 0 if False (test passes), exit 1 if True (test fails).
sys.exit(0 if not ok else 1)
" "$1"
        ' _ "$SCRIPT_DIR"

    # A3b: healthy governance (merge=300,task=100 → share=0.75; floor=0.65)
    #      share_ge_proportional must return True → ROW4-1 is GREEN.
    assert "A3b: share_ge_proportional(300,100,300,100,0.10) is True (healthy→ROW4-1 GREEN)" \
        bash -c '
            python3 -c "
import sys
sys.path.insert(0, sys.argv[1])
from cpu_gov_instrument import share_ge_proportional
ok = share_ge_proportional(300.0, 100.0, 300.0, 100.0, 0.10)
# We want True; exit 0 if True (test passes), exit 1 if False (test fails).
sys.exit(0 if ok else 1)
" "$1"
        ' _ "$SCRIPT_DIR"
fi

# ── A2 + A4 + A5 — the armed gates-only drive ─────────────────────────────────
echo ""
echo "--- A2+A4+A5: armed REIFY_CPU_GOV_TEST_GATES_ONLY drive ---"

assert "SUT exists at $SUT" \
    test -f "$SUT"

_SUT_OUT="$(mktemp)"
_SUT_RC=0
timeout 120 env -u REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS REIFY_CPU_GOVERN_DISABLE=1 REIFY_GOVTEST_TEST_MODE=1 REIFY_CPU_GOV_TEST_GATES_ONLY=1 bash "$SUT" >"$_SUT_OUT" 2>&1 || _SUT_RC=$?

_sut_exit0() {
    if [ "$_SUT_RC" -ne 0 ]; then
        echo "SUT rc=$_SUT_RC (124 = the anti-hang guard fired), want 0"
        tail -n 40 "$_SUT_OUT" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "A2: SUT exits 0 under the armed gates-only drive" \
    _sut_exit0

_sut_skipped_every_live_cycle() {
    local name missing=()
    for name in FIXTURE-6 ROW1-2 ROW1-1 ROW2_3 CONFINE-APPLIED ROW4; do
        grep -qF -- "SKIP $name: host does not support cgroup governance" "$_SUT_OUT" \
            || missing+=("$name")
    done
    if [ "${#missing[@]}" -ne 0 ]; then
        echo "missing host-governance SKIP markers: ${missing[*]}"
        tail -n 40 "$_SUT_OUT" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "A4: SUT output contains the host-governance SKIP marker of every live cycle" \
    _sut_skipped_every_live_cycle

_sut_exited_at_seam() {
    if grep -q '^Results:' "$_SUT_OUT"; then
        echo "SUT ran the FULL suite — the gates-only seam did not take effect:"
        tail -n 15 "$_SUT_OUT" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "A5: the gates-only seam short-circuits before any cycle (no Results: line)" \
    _sut_exited_at_seam

# ── A6 — unarmed GATES_ONLY is refused loudly ─────────────────────────────────
echo ""
echo "--- A6: GATES_ONLY without REIFY_GOVTEST_TEST_MODE ---"

_DISARM_OUT="$(mktemp)"

_gates_only_refused_without_arming() {
    local pid
    : > "$_DISARM_OUT"

    timeout 120 env -u REIFY_GOVTEST_TEST_MODE -u REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS REIFY_CPU_GOVERN_DISABLE=1 REIFY_CPU_GOV_TEST_GATES_ONLY=1 bash "$SUT" > "$_DISARM_OUT" 2>&1 &
    pid=$!

    until grep -q 'GATES_ONLY=1 IGNORED' "$_DISARM_OUT" 2>/dev/null \
        && grep -q 'Cycle SELF' "$_DISARM_OUT" 2>/dev/null; do
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.1
    done

    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true

    if ! grep -q 'GATES_ONLY=1 IGNORED' "$_DISARM_OUT" 2>/dev/null; then
        echo "unarmed GATES_ONLY was not refused loudly:"
        head -n 20 "$_DISARM_OUT" 2>/dev/null || true
        return 1
    fi
    if ! grep -q 'Cycle SELF' "$_DISARM_OUT" 2>/dev/null; then
        echo "unarmed GATES_ONLY short-circuited anyway — the suite never reached Cycle SELF:"
        head -n 20 "$_DISARM_OUT" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "A6: GATES_ONLY without REIFY_GOVTEST_TEST_MODE is refused loudly and the suite runs on" \
    _gates_only_refused_without_arming

rm -f "$_SUT_OUT" "$_DISARM_OUT"

# ── Final summary ──────────────────────────────────────────────────────────────
test_summary
