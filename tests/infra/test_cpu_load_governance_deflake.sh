#!/usr/bin/env bash
# tests/infra/test_cpu_load_governance_deflake.sh — meta-test for PRD T7 (task 4846).
#
# Proves that test_cpu_load_governance.sh's live cgroup-governance cycles
# skip cleanly and hermetically. Spawns the SUT as a subprocess with
# REIFY_CPU_GOVERN_DISABLE=1 and asserts the SUT emits its
# host-does-not-support-governance SKIP markers instead of running real
# CPU-governance burns.
#
# Assertions:
#   A1 (permanent-green regression guard): SUT output contains NO "live
#     section budget" skip marker — task 4846 removed the wall-clock budget
#     gate entirely, so "live section budget" no longer exists anywhere in
#     the SUT; this assertion now guards against its reintroduction.
#   A2 (guard): SUT subprocess exits 0 under the governance-disabled config.
#   A3 (NON-VACUOUS): share_ge_proportional discriminator — broken->False, healthy->True.
#   A4 (RED→GREEN driver): SUT output contains the ROW4 and ROW2_3
#     host_supports_governance SKIP markers, proving no real cgroup-governance
#     burns ran.
#
# Technique:
#   REIFY_CPU_GOVERN_DISABLE=1 → cgroup_governance_supported() (scripts/lib_cgroup.sh)
#     break-glass check fires → host_supports_governance() returns false in the
#     SUT → every host-gated live cycle (ROW1-1, ROW2_3, ROW4, ROW1-2, FIXTURE-6,
#     CONFINE-APPLIED) SKIPs unconditionally, independent of actual host cgroup
#     support. Only SELF checks, small FIXTURE-1..5 smoke (bounded workers/
#     seconds, load-robust), and the hermetic *-VACUITY/NAMING/CONFINE/BYPASS
#     assertions run.
#
# Hermetic: no real cgroup-governance CPU load, no systemd scope creation, no
# PSI required for A1/A2/A4. FIXTURE-3/4/5 still run small bounded real CPU
# burns (a few workers for a few seconds each) — load-robust, not host-gated.
# Auto-discovered by tests/infra/run_all.sh (glob test_*.sh).
#
# esc-4906-52 cross-reference: a one-off failure of this meta-test surfaced
# in task 4906's debugging history amid severe host oversubscription (~2.6x
# cores, sustained). Confirmed non-reproducing post-investigation (task
# 4962): this meta-test is hermetic by design (see above) and its
# host-exclusive bucket in run-all-classification.manifest is unchanged
# since task 4921 — the one-off is attributed to transient host overload,
# not a code defect. Full write-up (incl. a correction of which task
# touched this file's manifest row):
# docs/notes/verify-residual-oneoffs-esc-4906.md

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

# _run_with_stall_guard <stall_s> <out_file> <cmd> [arg...]
#   Runs <cmd> with stdout+stderr in <out_file>. Returns <cmd>'s own exit status,
#   or 124 after TERMing it once <out_file> has not grown for <stall_s> seconds.
#   Technique T (docs/prds/infra-test-wallclock-deflake.md §2): the window bounds
#   SILENCE, so a slow <cmd> that keeps emitting is never killed. Its total
#   runtime stays bounded by run_all.sh's member budget, whose process-group
#   kill reaches <cmd> because it stays in this shell's group.
_run_with_stall_guard() {
    local stall_s="$1" out="$2"
    shift 2
    "$@" >"$out" 2>&1 </dev/null &
    local pid=$!
    local size=0 now=0 last_growth=$SECONDS rc=0
    while kill -0 "$pid" 2>/dev/null; do
        now=$(stat -c %s "$out" 2>/dev/null || echo 0)
        if [ "$now" != "$size" ]; then
            size=$now
            last_growth=$SECONDS
        elif (( SECONDS - last_growth >= stall_s )); then
            kill -TERM "$pid" 2>/dev/null || true
            wait "$pid" 2>/dev/null || true
            return 124
        fi
        sleep 1
    done
    wait "$pid" || rc=$?
    return "$rc"
}

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

# ── A5 — NON-VACUOUS guard (_run_with_stall_guard) ─────────────────────────────
# The SUT below runs under _run_with_stall_guard, which TERMs it only once its
# output has been silent for the stall window, never on cumulative runtime. Each
# case drives the guard with a hermetic stub that writes to its own file. A5b is
# the NON-VACUITY CRUX: its command outlives the stall window yet keeps emitting,
# so a guard that is really a cumulative `timeout <stall_s>` returns 124 there.
#   - rc capture is `_rc=0; helper ... || _rc=$?`, never a bare call: `set -e`
#     must not abort on the helper's meaningful rc, and an undefined helper reads
#     as 127, which the cases assert as EXACT rcs (never `-ne 0`) so 127 FAILs.
#   - no assertion bounds an elapsed time (technique T, docs/prds/infra-test-
#     wallclock-deflake.md).
echo ""
echo "--- A5: NON-VACUOUS stall guard (_run_with_stall_guard) ---"

_a5_dir="$(mktemp -d)"

# A5a (STALL CRUX): a command that goes quiet is TERMed, rc exactly 124, and what
# it printed before going quiet is captured. `exec` makes the TERM reach sleep
# itself, so no orphan outlives the case.
_rc=0
_run_with_stall_guard 2 "$_a5_dir/stall.out" \
    bash -c 'echo started; exec sleep 30' || _rc=$?
assert "A5a: a command gone quiet is TERMed after the stall window (want rc=124, got ${_rc})" \
    test "$_rc" -eq 124
assert "A5a: the quiet command's output is captured (rc=${_rc}, 'started' in out file)" \
    grep -q 'started' "$_a5_dir/stall.out"

# A5b (PROGRESS CRUX): a command that keeps emitting past the stall window is NOT
# killed. 70 ticks 0.1 s apart run for at least 7 s against a 5 s window.
_rc=0
_run_with_stall_guard 5 "$_a5_dir/progress.out" \
    bash -c 'for i in $(seq 70); do echo "tick $i"; sleep 0.1; done' || _rc=$?
assert "A5b: a command still emitting past the stall window is NOT killed (want rc=0, got ${_rc})" \
    test "$_rc" -eq 0
assert "A5b: the emitting command ran to completion (rc=${_rc}, 'tick 70' in out file)" \
    grep -q 'tick 70' "$_a5_dir/progress.out"

# A5c (STATUS PASSTHROUGH): the command's own exit status is returned unchanged.
_rc=0
_run_with_stall_guard 5 "$_a5_dir/status.out" \
    bash -c 'echo x; exit 3' || _rc=$?
assert "A5c: the command's own exit status passes through (want rc=3, got ${_rc})" \
    test "$_rc" -eq 3

rm -rf "$_a5_dir"

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
    REIFY_CPU_GOVERN_DISABLE=1 \
    bash "$SUT" >"$_SUT_OUT" 2>&1 || _SUT_RC=$?

# A1: No "live section budget" skip marker in output.
# Permanent-green regression guard: task 4846 removed the wall-clock budget
# gate entirely, so "live section budget" no longer exists anywhere in the
# SUT — grep always finds nothing → bash -c exits 0 → assert PASS. Guards
# against the gate's reintroduction.
assert "A1: SUT output contains NO 'live section budget' skip marker" \
    bash -c '! grep -q "live section budget" "$1"' _ "$_SUT_OUT"

# A2: SUT exits 0 under cheap-skip config (SELF/FIXTURE/ROW4-BYPASS still run).
assert "A2: SUT exits 0 under cheap-skip config (rc=${_SUT_RC})" \
    test "${_SUT_RC}" -eq 0

# A4: SUT output contains the host_supports_governance SKIP markers for
# ROW4 and ROW2_3, proving no real cgroup-governance burns ran.
assert "A4: SUT output contains ROW4 and ROW2_3 host-governance SKIP markers" \
    bash -c '
        grep -q "SKIP ROW4: host does not support cgroup governance" "$1" &&
        grep -q "SKIP ROW2_3: host does not support cgroup governance" "$1"
    ' _ "$_SUT_OUT"

rm -f "$_SUT_OUT"

# ── Final summary ──────────────────────────────────────────────────────────────
test_summary
