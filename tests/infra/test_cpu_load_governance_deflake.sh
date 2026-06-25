#!/usr/bin/env bash
# tests/infra/test_cpu_load_governance_deflake.sh — meta-test for PRD T7 (task 4846).
#
# Proves that test_cpu_load_governance.sh does NOT gate verdict cycles on a
# wall-clock budget.  Spawns the SUT as a subprocess with BUDGET_S=0 +
# QUIET_CEILING=0 and asserts no "live section budget" skip marker appears.
#
# Assertions:
#   A1 (RED→GREEN driver): SUT output contains NO "live section budget" skip marker.
#   A2 (guard): SUT subprocess exits 0 under cheap-skip config.
#   A3 (NON-VACUOUS): share_ge_proportional discriminator — broken->False, healthy->True.
#
# Technique:
#   REIFY_CPU_GOV_TEST_BUDGET_S=0 → _live_budget_expired() fires at elapsed>=0
#     (pre-fix: all 3 ROWs emit the budget-skip line — host-independent since the
#     budget check precedes the host/PSI checks).
#   REIFY_CPU_GOV_TEST_QUIET_CEILING=0 → quiet-box gate fires on any numeric avg10
#     post-fix (any avg10 >= 0), so no actual CPU burns run; inner run stays cheap.
#
# Hermetic: no real CPU load, no cgroup substrate, no PSI required for A1/A2/A3.
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

# ── A1 + A2 — SUT must NOT emit wall-clock budget skip markers ─────────────────
echo ""
echo "--- A1+A2: SUT output must NOT contain 'live section budget' skip marker ---"

# Sanity: SUT file must exist.
assert "SUT exists at $SUT" \
    test -f "$SUT"

# Capture SUT stdout+stderr; preserve exit code without triggering set -e.
_SUT_OUT="$(mktemp)"
_SUT_RC=0
timeout 120 env \
    REIFY_CPU_GOV_TEST_BUDGET_S=0 \
    REIFY_CPU_GOV_TEST_QUIET_CEILING=0 \
    bash "$SUT" >"$_SUT_OUT" 2>&1 || _SUT_RC=$?

# A1: No "live section budget" skip marker in output.
# Pre-fix: BUDGET_S=0 → _live_budget_expired returns 0 (elapsed>=0) before
# host/PSI checks → all 3 ROWs emit "live section budget (0s) expired" →
# grep finds it → bash -c exits 1 → assert FAIL (RED).
# Post-fix: gate removed → no such line → grep finds nothing → bash -c exits 0
# → assert PASS (GREEN).
assert "A1: SUT output contains NO 'live section budget' skip marker" \
    bash -c '! grep -q "live section budget" "$1"' _ "$_SUT_OUT"

# A2: SUT exits 0 under cheap-skip config (SELF/FIXTURE/ROW4-BYPASS still run).
assert "A2: SUT exits 0 under cheap-skip config (rc=${_SUT_RC})" \
    test "${_SUT_RC}" -eq 0

rm -f "$_SUT_OUT"

# ── Final summary ──────────────────────────────────────────────────────────────
test_summary
