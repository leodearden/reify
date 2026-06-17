#!/usr/bin/env bash
# tests/infra/test_cpu_load_governance.sh — §8 integration-gate leaf (task 4634).
#
# Proves that the α/β/γ primitives COMPOSE:
#   α  scripts/cpu-admit.sh          — PSI admission gate
#   β  scripts/agent-bin/cargo       — agent cargo shim (heavy-subcmd gate)
#   γ  scripts/cpu-governed-exec.sh  — cgroup-v2 cpu.weight placement wrapper
#
# §8 boundary-table rows covered:
#   Row 1  lone governed source, box idle → busy-core fraction ≥ 0.95·nproc,
#           cpu.max == max (no quota throttle)                        host-gated
#   Row 2  heavy mix → after warm-up avg10 < AGENT_THRESHOLD         host-gated
#   Row 3  governed probe under mix → slowdown within fair-share band host-gated
#   Row 4  merge-favored share ≥ W_merge/(W_merge+W_task)−tol        host-gated
#
# ALWAYS-ON (even on substrate-absent CI):
#   Cycle SELF  — pure-analyzer + instrument-reuse self-tests via
#                 cpu_gov_instrument.py selftest (hermetic, never vacuous)
#   Cycle FIXTURE — fixture-generator contract (PSI/proc-stat gated)
#
# Auto-discovered by tests/infra/run_all.sh (glob test_*.sh).
# Helper files (cpu_load_fixture.sh, cpu_gov_instrument.py) are NOT test_*.sh
# so are not auto-run.
#
# §8 rows map to Cycles ROW1/ROW2_3/ROW4, each individually skipped when
# the host precondition is unmet — never false-fails on a hot shared host.
#
# Design decisions honored here:
#   G6 CRUX: all bounds PSI-relative/ratio/self-relative with a STATED
#             fair-share floor; NEVER absolute load==32.
#   Q5: warm-up default 8 s (knob REIFY_CPU_GOV_TEST_WARMUP_S).
#   Q2: W_task=100 / W_merge=300 (γ defaults, not retuned).
#   Row 4: private hermetic slices via REIFY_CPU_GOVERN_SLICE_TASK/MERGE overrides.
#
# KNOBS:
#   REIFY_CPU_GOV_TEST_WARMUP_S         warm-up window seconds (default 8)
#   REIFY_CPU_GOV_TEST_BUDGET_S         overall live-section timeout (default 120)
#   REIFY_CPU_GOV_TEST_MIXFACTOR        oversubscription factor (default 1.5)
#   REIFY_CPU_GOV_TEST_SLOWDOWN_K       slowdown upper-band multiplier (default 4)
#   REIFY_CPU_GOV_TEST_QUIET_CEILING    avg10 max for quiet-box precondition (default 20)
#   REIFY_CPU_GOV_TEST_BURN_S           per-fixture burn duration seconds (default 4)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

CPU_ADMIT="$REPO_ROOT/scripts/cpu-admit.sh"
CPU_GOV_EXEC="$REPO_ROOT/scripts/cpu-governed-exec.sh"
LIB_CGROUP="$REPO_ROOT/scripts/lib_cgroup.sh"
FIXTURE="$SCRIPT_DIR/cpu_load_fixture.sh"
INSTRUMENT="$SCRIPT_DIR/cpu_gov_instrument.py"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== cpu-load-governance integration tests (task 4634) ==="

# ---------------------------------------------------------------------------
# Substrate skip-guards (a) and (b) — always checked first.
# ---------------------------------------------------------------------------

# (a) PSI must be readable — required for cpu-admit.sh and Row 2 avg10 sampling.
if [ ! -r /proc/pressure/cpu ]; then
    echo "SKIP: kernel lacks /proc/pressure/cpu (PSI gate is Linux-only)"
    # Still run the pure-analyzer self-tests below (they do NOT need PSI).
    _PSI_AVAILABLE=0
else
    _PSI_AVAILABLE=1
fi

# (b) python3 must be on PATH — required for cpu_gov_instrument.py.
if ! command -v python3 >/dev/null 2>&1; then
    echo "SKIP: python3 not on PATH — all instrument-based cycles will be skipped"
    _PYTHON_AVAILABLE=0
else
    _PYTHON_AVAILABLE=1
fi

# ---------------------------------------------------------------------------
# host_supports_governance — gate helper for live cgroup placement scenarios.
# Copies the idiom from test_cpu_governed_exec.sh:46-54 verbatim.
# Returns 0 if the host can run governed placement, 1 otherwise.
# ---------------------------------------------------------------------------
host_supports_governance() {
    [ -f "$LIB_CGROUP" ] || return 1
    (
        # shellcheck source=scripts/lib_cgroup.sh
        source "$LIB_CGROUP"
        cgroup_governance_supported
    )
}

# ---------------------------------------------------------------------------
# live_or_skip — wrapper for the entire live (cgroup-dependent) section.
#
# Usage:
#   live_or_skip <label> <timeout_s> <function_name>
#
# Checks host_supports_governance; if not supported prints SKIP and returns 0.
# Otherwise runs <function_name> wrapped in a timeout of <timeout_s> seconds.
# If the timeout fires (exit 124) prints SKIP (not FAIL) and returns 0.
# This protects the shared 20-min run_all.sh wall on a slow/contended host.
#
# (stub at skeleton stage — implementations added per-row in later steps)
# ---------------------------------------------------------------------------
_LIVE_BUDGET_S="${REIFY_CPU_GOV_TEST_BUDGET_S:-120}"

live_or_skip() {
    local label="$1"
    local budget_s="$2"
    local fn_name="$3"

    if ! host_supports_governance; then
        echo "  SKIP ${label}: host does not support cgroup governance"
        return 0
    fi

    local rc=0
    timeout "${budget_s}" bash -c "
        # Re-source helpers inside the timeout subshell.
        source '${SCRIPT_DIR}/test_helpers.sh'
        ${fn_name}
    " || rc=$?

    if [ "$rc" -eq 124 ]; then
        echo "  SKIP ${label}: live section budget (${budget_s}s) expired — host too slow/contended"
        return 0
    fi
    return "$rc"
}

# ---------------------------------------------------------------------------
# Hermetic workdir — cleaned up on EXIT.
# ---------------------------------------------------------------------------
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# ============================================================================
# Cycle SELF — pure-analyzer + instrument-reuse self-tests.
# Always runs regardless of PSI/cgroup substrate availability.
# Hermetic, never vacuous GREEN even on substrate-less CI.
# ============================================================================
echo ""
echo "--- Cycle SELF: pure-analyzer self-tests via cpu_gov_instrument.py ---"

if [ "$_PYTHON_AVAILABLE" -eq 0 ]; then
    echo "  SKIP SELF: python3 not on PATH"
else
    # SELF-1: instrument file exists and is executable-by-python3.
    assert "SELF-1: cpu_gov_instrument.py exists" \
        test -f "$INSTRUMENT"

    # SELF-2: selftest subcommand exits 0 (covers all pure-analyzer assertions
    # with synthetic fixtures — hermetic, never vacuous).
    assert "SELF-2: cpu_gov_instrument.py selftest exits 0" \
        python3 "$INSTRUMENT" selftest

    # SELF-3: re-export contract — instrument exposes busy_fraction, _read_proc_stat,
    # NPROC (importlib reuse contract; verified via CLI probe subcommand).
    assert "SELF-3: cpu_gov_instrument.py exports busy-fraction CLI" \
        bash -c '
            # Provide two identical trivial /proc/stat lines; delta=0 → fraction=0.0
            f=$(mktemp)
            echo "cpu  100 0 50 800 10 0 0 0 0 0" > "$f"
            out=$(python3 "$1" busy-fraction "$f" "$f" 2>&1)
            rc=$?
            rm -f "$f"
            # Should print something like "0.0 0.0" (fraction busy_cores)
            [ "$rc" -eq 0 ]
        ' _ "$INSTRUMENT"

    # SELF-4: psi-avg10 CLI returns a number when PSI is available, or "unavailable".
    assert "SELF-4: cpu_gov_instrument.py psi-avg10 exits 0" \
        bash -c '
            python3 "$1" psi-avg10 >/dev/null 2>&1
        ' _ "$INSTRUMENT"

    # SELF-5: fair-share CLI: fair_share_floor(48, 32) = 1.5
    assert "SELF-5: fair-share 48 32 outputs 1.5" \
        bash -c '
            out=$(python3 "$1" fair-share 48 32 2>/dev/null)
            # Accept "1.5" or "1.50" — awk-style float
            echo "$out" | grep -qE "^1\.5(0+)?$"
        ' _ "$INSTRUMENT"
fi

# ============================================================================
# Cycle FIXTURE — fixture-generator contract.
# Gated on PSI (/proc/pressure/cpu) and /proc/stat availability.
# ============================================================================
echo ""
echo "--- Cycle FIXTURE: cpu_load_fixture.sh contract ---"

# FIXTURE-1: script exists and is executable.
assert "FIXTURE-1: cpu_load_fixture.sh exists" \
    test -f "$FIXTURE"
assert "FIXTURE-2: cpu_load_fixture.sh is executable" \
    test -x "$FIXTURE"

# The remaining fixture tests need /proc/stat (for busy_fraction) and python3.
if [ ! -r /proc/stat ] || [ "$_PYTHON_AVAILABLE" -eq 0 ]; then
    echo "  SKIP FIXTURE-3..5: /proc/stat unreadable or python3 absent"
else
    # FIXTURE-3: fixture completes within bounded wall time.
    # Run 4 workers for 2s; allow up to 10s (generous timing for slow hosts).
    FIXTURE_3_START=$(date +%s)
    FIXTURE_3_RC=0
    timeout 10 bash "$FIXTURE" 4 2 >/dev/null 2>&1 || FIXTURE_3_RC=$?
    FIXTURE_3_END=$(date +%s)
    FIXTURE_3_ELAPSED=$(( FIXTURE_3_END - FIXTURE_3_START ))
    assert "FIXTURE-3: fixture 4 workers 2s completes within 10s (elapsed=${FIXTURE_3_ELAPSED}s)" \
        test "$FIXTURE_3_RC" -eq 0

    # FIXTURE-4: fixture measurably raised busy-core fraction.
    # Snapshot /proc/stat before and after a 3s burn (nproc workers).
    NPROC="$(nproc)"
    grep "^cpu " /proc/stat > "$WORK/stat_before_fixture"
    timeout 15 bash "$FIXTURE" "$NPROC" 3 >/dev/null 2>&1 || true
    grep "^cpu " /proc/stat > "$WORK/stat_after_fixture"
    # busy_fraction CLI prints "fraction busy_cores"
    BUSY_OUT="$(python3 "$INSTRUMENT" busy-fraction \
        "$WORK/stat_before_fixture" "$WORK/stat_after_fixture" 2>/dev/null || true)"
    BUSY_FRAC="$(echo "$BUSY_OUT" | awk '{print $1}')"
    assert "FIXTURE-4: fixture raised busy-core fraction above 0.05 (frac=${BUSY_FRAC:-?})" \
        bash -c '
            frac="${1:-0}"
            awk -v f="$frac" "BEGIN{exit !(f+0 > 0.05)}"
        ' _ "${BUSY_FRAC:-0}"

    # FIXTURE-5: composed-wrapper smoke — cpu-governed-exec --role task exits 0.
    FIXTURE_5_RC=0
    timeout 15 bash "$CPU_GOV_EXEC" --role task -- bash "$FIXTURE" 2 1 \
        >/dev/null 2>&1 || FIXTURE_5_RC=$?
    assert "FIXTURE-5: cpu-governed-exec --role task -- cpu_load_fixture.sh 2 1 exits 0 (rc=${FIXTURE_5_RC})" \
        test "$FIXTURE_5_RC" -eq 0

    # FIXTURE-6: (host-gated) placed scope's cpu.max first field == "max".
    if host_supports_governance; then
        SCOPE_MAX="$(timeout 10 bash "$CPU_GOV_EXEC" --role task -- \
            bash -c 'rel=$(sed "s/^0:://" /proc/self/cgroup); cat /sys/fs/cgroup"$rel"/cpu.max 2>/dev/null || echo "unavailable"' \
            2>/dev/null || echo "unavailable")"
        SCOPE_MAX_FIRST="${SCOPE_MAX%% *}"
        assert "FIXTURE-6: governed scope cpu.max first field == max (got '${SCOPE_MAX_FIRST}')" \
            test "${SCOPE_MAX_FIRST}" = "max"
    else
        echo "  SKIP FIXTURE-6: host does not support cgroup governance"
    fi
fi

# ============================================================================
# Cycle ROW1 — §8 Row 1: lone governed source, box idle.
# HOST-GATED (host_supports_governance + PSI + python3).
# QUIET-BOX: pre-check avg10 < QUIET_CEILING; SKIP if box already hot.
# ============================================================================
echo ""
echo "--- Cycle ROW1: §8 Row 1 (lone governed source, box idle) ---"

_ROW1_QUIET_CEILING="${REIFY_CPU_GOV_TEST_QUIET_CEILING:-20}"
_ROW1_BURN_S="${REIFY_CPU_GOV_TEST_BURN_S:-4}"

if ! host_supports_governance; then
    echo "  SKIP ROW1: host does not support cgroup governance"
elif [ "$_PSI_AVAILABLE" -eq 0 ] || [ "$_PYTHON_AVAILABLE" -eq 0 ]; then
    echo "  SKIP ROW1: PSI or python3 unavailable"
else
    # Quiet-box precondition guard (§8 row 1 precondition: box idle).
    _row1_avg10="$(python3 "$INSTRUMENT" psi-avg10 2>/dev/null || echo "unavailable")"
    _row1_quiet_met=1
    if [ "$_row1_avg10" != "unavailable" ]; then
        # Compare avg10 (float) >= QUIET_CEILING using awk.
        if awk -v a="$_row1_avg10" -v c="$_ROW1_QUIET_CEILING" 'BEGIN{exit !(a >= c)}'; then
            echo "  SKIP ROW1: box not quiet (avg10=${_row1_avg10} >= QUIET_CEILING=${_ROW1_QUIET_CEILING})"
            _row1_quiet_met=0
        fi
    fi

    if [ "$_row1_quiet_met" -eq 1 ]; then
        # ROW1 measurement variables — wired in step-6.
        # Initialized to failing defaults; step-6 fills in real measurements
        # (before/after /proc/stat snapshots + scope cpu.max probe).
        _ROW1_FRAC="0"
        _ROW1_CPU_MAX_FIRST=""
        # ── ROW1 ORCHESTRATION SEAM ────────────────────────────────────────
        # (step-6 adds: snapshot /proc/stat before, launch lone-source
        # governed fixture, snapshot after, compute frac via busy-fraction,
        # capture scope cpu.max via cgroup probe)
        # ──────────────────────────────────────────────────────────────────

        # ROW1-1: busy-core fraction >= 0.95 (≥95% of nproc, §8 row 1 floor).
        assert "ROW1-1: lone governed source busy-core fraction >= 0.95 (frac=${_ROW1_FRAC})" \
            bash -c '
                frac="${1:-0}"
                awk -v f="$frac" "BEGIN{exit !(f+0 >= 0.95)}"
            ' _ "${_ROW1_FRAC}"

        # ROW1-2: scope cpu.max first field == "max" (no static cap, C-G1).
        assert "ROW1-2: governed scope cpu.max first field == max (got '${_ROW1_CPU_MAX_FIRST:-?}')" \
            test "${_ROW1_CPU_MAX_FIRST:-}" = "max"
    fi
fi

# ============================================================================
# Cycle ROW2_3 — §8 Rows 2+3: heavy mix + bounded slowdown.
# (Tests added in step-7, implementation in step-8.)
# ============================================================================
echo ""
echo "--- Cycle ROW2_3: §8 Rows 2+3 (heavy mix + bounded slowdown) ---"

if ! host_supports_governance; then
    echo "  SKIP ROW2_3: host does not support cgroup governance"
fi

# ============================================================================
# Cycle ROW4 — §8 Row 4: merge-favored share in private slices.
# (Tests added in step-9, implementation in step-10.)
# ============================================================================
echo ""
echo "--- Cycle ROW4: §8 Row 4 (merge-favored share, private slices) ---"

if ! host_supports_governance; then
    echo "  SKIP ROW4: host does not support cgroup governance"
fi

# ---------------------------------------------------------------------------
# Final summary — PASS/FAIL count from test_helpers.sh.
# ---------------------------------------------------------------------------
test_summary
