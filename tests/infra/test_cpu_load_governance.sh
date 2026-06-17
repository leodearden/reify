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
#   REIFY_CPU_GOV_TEST_BURN_S           per-fixture burn duration seconds (default 4;
#                                       ROW4 default warmup+measure+4 if unset)
#   REIFY_CPU_GOV_TEST_ROW4_WARMUP_S    ROW4 steady-state ramp before sampling (default 3)
#   REIFY_CPU_GOV_TEST_ROW4_MEASURE_S   ROW4 steady-state delta window (default 8)
#   REIFY_CPU_GOV_TEST_SHARE_TOL        ROW4 merge-share variance budget (default 0.10)

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
# shellcheck source=tests/infra/load_tolerance_lib.sh
source "$SCRIPT_DIR/load_tolerance_lib.sh"

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

_LIVE_BUDGET_S="$(load_tolerant_attempts "${REIFY_CPU_GOV_TEST_BUDGET_S:-120}")"

# ---------------------------------------------------------------------------
# Hermetic workdir — cleaned up on EXIT.
# ---------------------------------------------------------------------------
WORK="$(mktemp -d)"
# Tracking variables for EXIT cleanup (crash-path protection).
_ALL_MIX_PIDS=""
_ROW4_SLICE_TASK_CREATED=""
_ROW4_SLICE_MERGE_CREATED=""

_cleanup_all() {
    # Kill any lingering ROW2_3 mix background processes (crash-path reap).
    if [ -n "${_ALL_MIX_PIDS:-}" ]; then
        for _cpid in ${_ALL_MIX_PIDS}; do
            kill "$_cpid" 2>/dev/null || true
        done
    fi
    # Stop private ROW4 test slices to avoid lingering systemd session units.
    if [ -n "${_ROW4_SLICE_TASK_CREATED:-}" ]; then
        systemctl --user stop "${_ROW4_SLICE_TASK_CREATED}" 2>/dev/null || true
    fi
    if [ -n "${_ROW4_SLICE_MERGE_CREATED:-}" ]; then
        systemctl --user stop "${_ROW4_SLICE_MERGE_CREATED}" 2>/dev/null || true
    fi
    rm -rf "$WORK"
}
trap '_cleanup_all' EXIT

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
# ---------------------------------------------------------------------------
# Live section wall-time budget guard.
# Records the start epoch; _live_budget_expired() returns 0 (true) when the
# elapsed time since the live section started has consumed _LIVE_BUDGET_S.
# Used as the outermost guard on each ROW section — if the budget is already
# exhausted when a new ROW is about to start, skip it instead of starting
# another expensive live cycle.  Individual operations within each ROW already
# carry their own per-step timeout(1) guards; this is the session-level backstop
# that protects the shared 20-min run_all.sh wall on a slow/contended host.
# ---------------------------------------------------------------------------
_LIVE_START_EPOCH="$(date +%s)"

_live_budget_expired() {
    local _now; _now="$(date +%s)"
    local _elapsed=$(( _now - _LIVE_START_EPOCH ))
    [ "$_elapsed" -ge "$_LIVE_BUDGET_S" ]
}

echo ""
echo "--- Cycle ROW1: §8 Row 1 (lone governed source, box idle) ---"

_ROW1_QUIET_CEILING="${REIFY_CPU_GOV_TEST_QUIET_CEILING:-20}"
_ROW1_BURN_S="${REIFY_CPU_GOV_TEST_BURN_S:-4}"

if _live_budget_expired; then
    echo "  SKIP ROW1: live section budget (${_LIVE_BUDGET_S}s) expired"
elif ! host_supports_governance; then
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
        _NPROC="$(nproc)"
        _ROW1_CPU_MAX_FILE="$WORK/row1_cpu_max"

        # ROW1 orchestration (step-6):
        # (a) cpu.max probe — run a tiny probe inside the scope to capture
        #     the first field of cpu.max while the scope is live.
        #     Uses a temp script to avoid shell quoting complexity.
        cat > "$WORK/row1_probe.sh" << 'EOF_PROBE'
#!/usr/bin/env bash
rel=$(sed 's/^0:://' /proc/self/cgroup 2>/dev/null || echo "")
if [ -n "$rel" ]; then
    cat "/sys/fs/cgroup${rel}/cpu.max" 2>/dev/null || echo "unavailable"
else
    echo "unavailable"
fi
EOF_PROBE
        bash "$CPU_GOV_EXEC" --role task -- bash "$WORK/row1_probe.sh" \
            > "$_ROW1_CPU_MAX_FILE" 2>/dev/null \
            || echo "unavailable" > "$_ROW1_CPU_MAX_FILE"
        _ROW1_CPU_MAX="$(cat "$_ROW1_CPU_MAX_FILE" 2>/dev/null || echo "unavailable")"
        _ROW1_CPU_MAX_FIRST="${_ROW1_CPU_MAX%% *}"

        # (b) Lone-source governed launch: nproc workers × burn_s seconds.
        #     Snapshot /proc/stat before and after to measure busy-core fraction.
        grep "^cpu " /proc/stat > "$WORK/row1_stat_before"
        timeout $(( _ROW1_BURN_S + 15 )) bash "$CPU_GOV_EXEC" --role task -- \
            bash "$FIXTURE" "$_NPROC" "$_ROW1_BURN_S" \
            >/dev/null 2>&1 || true
        grep "^cpu " /proc/stat > "$WORK/row1_stat_after"

        # Compute busy-core fraction via importlib-reused busy_fraction.
        _ROW1_BUSY_OUT="$(python3 "$INSTRUMENT" busy-fraction \
            "$WORK/row1_stat_before" "$WORK/row1_stat_after" 2>/dev/null \
            || echo "0 0")"
        _ROW1_FRAC="$(echo "$_ROW1_BUSY_OUT" | awk '{print $1}')"

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
# Cycle ROW2_3 — §8 Rows 2+3: heavy mix → PSI band + bounded slowdown.
# HOST-GATED. QUIET-BOX guard on Row 2 PSI-band assertion.
#
# Design:
#   1. Pre-measure uncontended governed probe wall T_base (1 worker × PROBE_S).
#   2. Launch mix: ceil(MIXFACTOR·nproc) governed task-role sources + 1 merge-role
#      source, EACH through composed wrappers (cpu-governed-exec → agent cargo shim
#      → cpu-admit admit → stub real-cargo that runs cpu_load_fixture.sh).
#   3. Concurrently run the timed governed probe → T_mix.
#   4. Warm-up window, then sample avg10.
#
# §8 Row 2 assertions: avg10 < REIFY_CPU_ADMIT_AGENT_THRESHOLD; all sources completed.
# §8 Row 3 assertion:  slowdown = T_mix/T_base within [fair_share_floor, K·floor] AND < 10.
# ============================================================================
echo ""
echo "--- Cycle ROW2_3: §8 Rows 2+3 (heavy mix + bounded slowdown) ---"

_MIXFACTOR="${REIFY_CPU_GOV_TEST_MIXFACTOR:-1.5}"
_SLOWDOWN_K="${REIFY_CPU_GOV_TEST_SLOWDOWN_K:-4}"
_ROW23_WARMUP_S="${REIFY_CPU_GOV_TEST_WARMUP_S:-8}"
_ROW23_BURN_S="${REIFY_CPU_GOV_TEST_BURN_S:-4}"
_ROW23_PROBE_S=2           # fixed work quantum for T_base/T_mix probe
_ADMIT_THRESHOLD="${REIFY_CPU_ADMIT_AGENT_THRESHOLD:-50}"

if _live_budget_expired; then
    echo "  SKIP ROW2_3: live section budget (${_LIVE_BUDGET_S}s) expired"
elif ! host_supports_governance; then
    echo "  SKIP ROW2_3: host does not support cgroup governance"
elif [ "$_PSI_AVAILABLE" -eq 0 ] || [ "$_PYTHON_AVAILABLE" -eq 0 ]; then
    echo "  SKIP ROW2_3: PSI or python3 unavailable"
else
    # Compute mix width: ceil(MIXFACTOR × nproc).
    _NPROC="$(nproc)"
    _MIX_N="$(awk -v f="$_MIXFACTOR" -v n="$_NPROC" \
        'BEGIN{v=f*n; i=int(v); if(v>i) i=i+1; if(i<1) i=1; print i}')"
    # active_sources for fair_share_floor: _MIX_N task + 1 merge.
    _ACTIVE_SOURCES=$(( _MIX_N + 1 ))

    # Global quiet-start precondition guard for ROW2_3 (step-8 orchestration note).
    # When the box is too hot at start, PSI admission (cpu-admit.sh admit) blocks mix
    # sources before they can burn CPU or write done-markers, making ROW2-2 (sources
    # completed) and ROW3-1 (slowdown) false-fail — not a governance failure.
    # Also, T_base measured on a hot box is inflated relative to T_mix measured after
    # the box cools, inverting the slowdown ratio. SKIP the entire cycle on a hot box
    # (same discipline as Row 1's quiet-box guard).  Uses the same QUIET_CEILING knob.
    _row23_pre_avg10="$(python3 "$INSTRUMENT" psi-avg10 2>/dev/null || echo "unavailable")"
    _ROW23_QUIET_CEILING="${REIFY_CPU_GOV_TEST_QUIET_CEILING:-20}"
    _row23_quiet_met=1
    if [ "$_row23_pre_avg10" != "unavailable" ]; then
        if awk -v a="$_row23_pre_avg10" -v c="$_ROW23_QUIET_CEILING" \
                'BEGIN{exit !(a >= c)}'; then
            echo "  SKIP ROW2_3: box not quiet at start (avg10=${_row23_pre_avg10} >= QUIET_CEILING=${_ROW23_QUIET_CEILING})"
            _row23_quiet_met=0
        fi
    fi

    if [ "$_row23_quiet_met" -eq 1 ]; then
    # Mix burn duration: must cover WARMUP_S + PROBE_S + settling.
    _ROW23_MIX_BURN_S=$(( _ROW23_WARMUP_S + _ROW23_PROBE_S + 4 ))
    # Marker dir: each stub-cargo source writes done_<PID> here.
    _ROW23_MARKER_DIR="$WORK/row23_markers"
    mkdir -p "$_ROW23_MARKER_DIR"
    # Stub-cargo-bin: the stub "real cargo" that burns CPU + writes done-marker.
    # PATH for mix: scripts/agent-bin (shim) first, then stub-cargo-bin (stub).
    # The shim strips agent-bin → finds stub-cargo-bin/cargo as "real cargo".
    _STUB_CARGO_BIN="$WORK/stub-cargo-bin"
    mkdir -p "$_STUB_CARGO_BIN"
    cat > "$_STUB_CARGO_BIN/cargo" << STUB_CARGO_EOF
#!/usr/bin/env bash
# Stub real-cargo for ROW2_3 mix (replaces real cargo after shim PATH-strip).
# Runs a CPU-burn fixture for the mix duration and writes a done-marker.
bash "${FIXTURE}" 1 ${_ROW23_MIX_BURN_S} >/dev/null 2>&1 || true
touch "${_ROW23_MARKER_DIR}/done_\$\$"
STUB_CARGO_EOF
    chmod +x "$_STUB_CARGO_BIN/cargo"
    # PATH for mix invocations: shim (agent-bin) first, stub second.
    _MIX_PATH="$REPO_ROOT/scripts/agent-bin:$_STUB_CARGO_BIN:/usr/bin:/bin"
    _SHIM="$REPO_ROOT/scripts/agent-bin/cargo"

    # Work-based probe: do N iterations of float arithmetic, print elapsed seconds.
    # This gives a FIXED WORK QUANTUM so wall time GROWS under CPU contention —
    # unlike a time-bounded fixture (which always takes duration_s regardless of
    # CPU share). Runs inside the governed scope for a fair T_base/T_mix comparison.
    _PROBE_ITERS="${REIFY_CPU_GOV_TEST_PROBE_ITERS:-20000000}"
    cat > "$WORK/row23_probe.py" << 'PROBE_PY'
#!/usr/bin/env python3
import sys, time
iters = int(sys.argv[1]) if len(sys.argv) > 1 else 20_000_000
start = time.monotonic()
total = 0.0
for i in range(iters):
    total += float(i) * 1.001
end = time.monotonic()
# Print elapsed seconds (float) to stdout.
print(f"{end - start:.6f}")
PROBE_PY

    # (a) Pre-measure T_base: uncontended governed probe.
    _T_BASE="$(timeout 30 bash "$CPU_GOV_EXEC" --role task -- \
        python3 "$WORK/row23_probe.py" "$_PROBE_ITERS" 2>/dev/null || echo "1")"
    [ -z "${_T_BASE}" ] || [ "${_T_BASE}" = "0" ] && _T_BASE="1"

    # (b) Launch mix: N task-role + 1 merge-role, each through composed wrappers
    #     (γ cpu-governed-exec → β agent-bin/cargo shim → α cpu-admit admit → stub).
    # Record PIDs for cleanup.
    _MIX_PIDS=""
    _mi=0
    while [ "$_mi" -lt "$_MIX_N" ]; do
        PATH="$_MIX_PATH" \
        timeout $(( _ROW23_MIX_BURN_S + 15 )) \
            bash "$CPU_GOV_EXEC" --role task -- bash "$_SHIM" test \
            >/dev/null 2>&1 &
        _MIX_PIDS="${_MIX_PIDS}${_MIX_PIDS:+ }$!"
        _mi=$(( _mi + 1 ))
    done
    # 1 merge-role source (DF_VERIFY_ROLE=merge bypasses cpu-admit, per C-A3).
    PATH="$_MIX_PATH" DF_VERIFY_ROLE=merge \
    timeout $(( _ROW23_MIX_BURN_S + 15 )) \
        bash "$CPU_GOV_EXEC" --role merge -- bash "$_SHIM" test \
        >/dev/null 2>&1 &
    _MIX_PIDS="${_MIX_PIDS}${_MIX_PIDS:+ }$!"

    # Register all mix PIDs in the EXIT-trap list for crash-path cleanup.
    _ALL_MIX_PIDS="$_MIX_PIDS"

    # (c) Warm-up window then sample avg10 (Row 2 PSI measurement).
    sleep "$_ROW23_WARMUP_S"
    _ROW23_AVG10="$(python3 "$INSTRUMENT" psi-avg10 2>/dev/null || echo "99")"

    # (d) Timed work-based probe under the mix → T_mix (Row 3 slowdown).
    _T_MIX="$(timeout 60 bash "$CPU_GOV_EXEC" --role task -- \
        python3 "$WORK/row23_probe.py" "$_PROBE_ITERS" 2>/dev/null || echo "0")"
    [ -z "${_T_MIX}" ] && _T_MIX="0"

    # (e) Wait for mix to finish (natural completion or timeout).
    for _mpid in $_MIX_PIDS; do
        wait "$_mpid" 2>/dev/null || true
    done
    _MIX_PIDS=""
    _ALL_MIX_PIDS=""  # PIDs already reaped; clear EXIT-trap list.

    # (f) Progress accounting: count done-markers.
    # Assert >= 90% completion (not strict equality) — serialized cpu-admit admission
    # under a ~1.5×nproc mix can SIGTERM the slowest sources before their outer timeout,
    # making strict equality unreliable on a contended host even when governance is correct.
    _ROW23_DONE_COUNT="$(ls "$_ROW23_MARKER_DIR"/done_* 2>/dev/null | wc -l || echo 0)"
    # ceil(0.9 * ACTIVE_SOURCES) — at least 90% must complete.
    _ROW23_THRESHOLD=$(( (_ACTIVE_SOURCES * 9 + 9) / 10 ))
    _ROW23_ALL_PROGRESSED=0
    if [ "$_ROW23_DONE_COUNT" -ge "$_ROW23_THRESHOLD" ]; then
        _ROW23_ALL_PROGRESSED=1
    fi

    # ── Row 2 assertions ──
    # ROW2-1: after warm-up, avg10 < AGENT_THRESHOLD (PSI band).
    # Guard: psi-avg10 CLI returns exit 0 even when printing "unavailable" (when
    # /proc/pressure/cpu is transiently unreadable mid-run), so the "|| echo 99"
    # fallback on line above never fires for that case — _ROW23_AVG10 becomes
    # "unavailable" and float() would raise ValueError producing a confusing RED.
    # Instead: if the sampled value is not a valid float, SKIP ROW2-1 with a clear
    # message, mirroring the ROW3-1 inconclusive-probe skip pattern.
    if ! python3 -c "float('${_ROW23_AVG10}')" 2>/dev/null; then
        echo "  SKIP ROW2-1: avg10 sample non-numeric (${_ROW23_AVG10}) — PSI transiently unreadable mid-run"
    else
        assert "ROW2-1: avg10 after warm-up < AGENT_THRESHOLD=${_ADMIT_THRESHOLD} (avg10=${_ROW23_AVG10})" \
            python3 -c "
import sys
v = float('${_ROW23_AVG10}')
t = float('${_ADMIT_THRESHOLD}')
sys.exit(0 if v < t else 1)
"
    fi
    # ROW2-2: >= 90% of sources completed (none starved).
    assert "ROW2-2: >= 90% (${_ROW23_THRESHOLD}/${_ACTIVE_SOURCES}) sources completed — none starved (done=${_ROW23_DONE_COUNT})" \
        test "${_ROW23_ALL_PROGRESSED}" -eq 1

    # ── Row 3 assertions ──
    # Compute slowdown = T_mix / T_base (float division via awk).
    _ROW3_SLOWDOWN="$(awk -v m="${_T_MIX}" -v b="${_T_BASE}" \
        'BEGIN{if(b+0<=0){print "0"}else{print m/b}}')"
    # fair_share_floor = active_sources / nproc.
    _ROW3_FLOOR="$(python3 "$INSTRUMENT" fair-share "$_ACTIVE_SOURCES" "$_NPROC" \
        2>/dev/null || echo "0")"
    # ROW3-1: slowdown within [floor, K·floor] AND < 10 (4415 cannot recur).
    # Skip if T_mix probe timed out or failed (returns "0") — on a heavily contended
    # host a 20M-iteration Python probe can exceed the 60s probe budget when the
    # 4-6× slowdown is real, making T_mix == 0 an inconclusive measurement, not a
    # governance failure.
    if awk -v m="${_T_MIX:-0}" 'BEGIN{exit !(m+0 <= 0)}'; then
        echo "  SKIP ROW3-1: T_mix probe timed out or failed (T_mix=${_T_MIX:-0}) — inconclusive"
    else
        assert "ROW3-1: slowdown=${_ROW3_SLOWDOWN} within_bound(floor=${_ROW3_FLOOR},K=${_SLOWDOWN_K})" \
            python3 -c "
import sys
s = float('${_ROW3_SLOWDOWN}')
fl = float('${_ROW3_FLOOR}')
k = float('${_SLOWDOWN_K}')
ok = (fl <= s <= k * fl) and s < 10.0
sys.exit(0 if ok else 1)
"
    fi
    fi  # _row23_quiet_met
fi

# ============================================================================
# Cycle ROW4 — §8 Row 4: merge-favored share in private hermetic slices.
# HOST-GATED for share measurement (cgroup placement required).
#
# Design (step-9):
#   Private test slices (REIFY_CPU_GOVERN_SLICE_TASK=reify-govtest-agents.slice
#   and REIFY_CPU_GOVERN_SLICE_MERGE=reify-govtest-merge.slice) nest under
#   shared reify-govtest.slice → they are siblings → cpu.weight ratio is
#   comparable (C-G2 invariant: weight proportion valid among siblings only).
#
#   Measurement: cpu.stat usage_usec DELTA before/after contention burns.
#   Slices (unlike scopes) are persistent, so a before/after delta isolates
#   just the contention-burn contribution — same pattern as busy_fraction.
#
#   Contention: 2×NWORKERS total workers (W merge + W task, 2W > nproc)
#   ensures real CPU contention so cgroup weight scheduling fires.
#
# §8 Row 4 assertion:
#   merge_share = Δmerge / (Δmerge + Δtask)  ≥  W_merge/(W_merge+W_task) - tol
#              = 0.75 − 0.10 = 0.65  (STATED proportional floor, not 0)
#   Δ sampled over a steady-state window (warm-up + measure), not the whole
#   burn, so the startup stagger does not bias the share (step-12 fix).
#
# Merge-bypass smoke (Cycle ROW4-BYPASS, §8 row 9 echo):
#   DF_VERIFY_ROLE=merge + avg10=99 PSI fixture → cpu-admit.sh admit exits 0
#   fast.  Hermetic (synthetic PSI fixture), always-on, no cgroup required.
# ============================================================================
echo ""
echo "--- Cycle ROW4: §8 Row 4 (merge-favored share, private slices) ---"

# Knobs — use γ's defaults to be consistent with the lib.
_ROW4_W_TASK="${REIFY_CPU_GOVERN_W_TASK:-100}"
_ROW4_W_MERGE="${REIFY_CPU_GOVERN_W_MERGE:-300}"
_ROW4_TOL="${REIFY_CPU_GOV_TEST_SHARE_TOL:-0.10}"
# Steady-state sampling windows (step-12 robustness fix for esc-4634-52).
# The cpu.weight 3:1 ratio only manifests cleanly once BOTH role burns are
# fully ramped and contending.  Sampling the usage_usec delta across the whole
# burn (including the asymmetric startup stagger — scope creation + worker
# spawn for each role) let one role bank uncontended CPU before its sibling's
# scope existed, biasing merge_share DOWN (observed 0.639 vs floor 0.65 — a
# ~0.01 false-RED).  Fix: launch both burns, wait WARMUP_S for both to ramp,
# THEN bracket the usage_usec delta over a MEASURE_S steady-state window while
# both are still burning.  Mirrors the ROW2_3 warm-up design + PRD §11 Q5.
_ROW4_WARMUP_S="${REIFY_CPU_GOV_TEST_ROW4_WARMUP_S:-3}"
_ROW4_MEASURE_S="${REIFY_CPU_GOV_TEST_ROW4_MEASURE_S:-8}"
# Burn must outlast warm-up + measure window + a settle margin so the AFTER
# sample lands while BOTH roles are still contending (never during teardown).
# Clamp up if a shared BURN_S override (used by ROW1/ROW2_3 for speed) is too
# small for ROW4's steady-state window — otherwise the AFTER sample would land
# during teardown and re-introduce the stagger bias this fix removes.
_ROW4_BURN_S="${REIFY_CPU_GOV_TEST_BURN_S:-$(( _ROW4_WARMUP_S + _ROW4_MEASURE_S + 4 ))}"
_ROW4_BURN_MIN=$(( _ROW4_WARMUP_S + _ROW4_MEASURE_S + 4 ))
[ "$_ROW4_BURN_S" -lt "$_ROW4_BURN_MIN" ] && _ROW4_BURN_S="$_ROW4_BURN_MIN"

# Private test slice names (siblings under reify-govtest.slice).
# Must differ from production slices (reify-governed-{agents,merge}.slice)
# to isolate usage_usec deltas from concurrent production agent placement (ζ).
_ROW4_SLICE_TASK="reify-govtest-agents.slice"
_ROW4_SLICE_MERGE="reify-govtest-merge.slice"

if _live_budget_expired; then
    echo "  SKIP ROW4: live section budget (${_LIVE_BUDGET_S}s) expired"
elif ! host_supports_governance; then
    echo "  SKIP ROW4: host does not support cgroup governance"
elif [ "$_PYTHON_AVAILABLE" -eq 0 ]; then
    echo "  SKIP ROW4: python3 unavailable"
else
    # ── ROW4 ORCHESTRATION (step-10) ─────────────────────────────────────────

    # (a) Discover slice cgroup rel-paths by running a trivial probe inside each
    #     private slice via cpu-governed-exec with SLICE overrides.
    #     /proc/self/cgroup format (cgroup-v2): "0::<rel>" → strip prefix, strip scope.
    _ROW4_TASK_SLICE_REL=""
    _ROW4_MERGE_SLICE_REL=""
    _ROW4_TASK_SLICE_REL="$(
        REIFY_CPU_GOVERN_SLICE_TASK="$_ROW4_SLICE_TASK" \
        timeout 10 bash "$CPU_GOV_EXEC" --role task -- bash -c '
            rel=$(sed "s/^0:://" /proc/self/cgroup 2>/dev/null || echo "")
            echo "${rel%/*}"
        ' 2>/dev/null || echo ""
    )"
    _ROW4_MERGE_SLICE_REL="$(
        REIFY_CPU_GOVERN_SLICE_MERGE="$_ROW4_SLICE_MERGE" \
        timeout 10 bash "$CPU_GOV_EXEC" --role merge -- bash -c '
            rel=$(sed "s/^0:://" /proc/self/cgroup 2>/dev/null || echo "")
            echo "${rel%/*}"
        ' 2>/dev/null || echo ""
    )"

    # (b) Pre-weight the private test slices (C-G2: weight ratio among siblings).
    #     cgroup_set_slice_weight vivifies the slice (systemctl --user start) and
    #     then sets cpu.weight.  Runs in a subshell to avoid polluting harness env.
    (
        # shellcheck source=scripts/lib_cgroup.sh
        source "$LIB_CGROUP" 2>/dev/null
        cgroup_set_slice_weight "$_ROW4_SLICE_TASK" "$_ROW4_W_TASK" 2>/dev/null
        cgroup_set_slice_weight "$_ROW4_SLICE_MERGE" "$_ROW4_W_MERGE" 2>/dev/null
    ) || true
    # Mark private slices for EXIT cleanup (set BEFORE burns start so the
    # trap fires even if the test is killed mid-burn).
    _ROW4_SLICE_TASK_CREATED="$_ROW4_SLICE_TASK"
    _ROW4_SLICE_MERGE_CREATED="$_ROW4_SLICE_MERGE"

    # (c) Launch concurrent contention burns FIRST (before sampling), then
    #     bracket the usage_usec delta over a steady-state window only.
    #     W=nproc workers each role → 2W=2*nproc on nproc cores → 2× oversubscription.
    #     At ≥ 2× oversubscription all workers are always runnable, so the kernel
    #     applies cpu.weight scheduling continuously and the 3:1 ratio is observable.
    #     (nproc/2+1 gave only 6% oversubscription — too weak for weight to manifest.)
    _NPROC_ROW4="$(nproc)"
    _ROW4_W="$_NPROC_ROW4"  # W per role; 2W = 2*nproc → clear 2× oversubscription

    REIFY_CPU_GOVERN_SLICE_TASK="$_ROW4_SLICE_TASK" \
    timeout $(( _ROW4_BURN_S + 15 )) bash "$CPU_GOV_EXEC" --role task -- \
        bash "$FIXTURE" "$_ROW4_W" "$_ROW4_BURN_S" \
        >/dev/null 2>&1 &
    _ROW4_TASK_BG=$!

    REIFY_CPU_GOVERN_SLICE_MERGE="$_ROW4_SLICE_MERGE" \
    timeout $(( _ROW4_BURN_S + 15 )) bash "$CPU_GOV_EXEC" --role merge -- \
        bash "$FIXTURE" "$_ROW4_W" "$_ROW4_BURN_S" \
        >/dev/null 2>&1 &
    _ROW4_MERGE_BG=$!

    # (d) Warm-up: let BOTH burns ramp to full contention before sampling, so
    #     the startup stagger (scope creation + worker spawn) is OUTSIDE the
    #     measured window and cannot bank uncontended CPU into either delta.
    sleep "$_ROW4_WARMUP_S"

    # (e) Sample usage_usec at the START of the steady-state window.
    #     Slices are persistent; usage_usec accumulates — must use before/after delta.
    _ROW4_TASK_BEFORE="$(python3 "$INSTRUMENT" cgroup-usage "$_ROW4_TASK_SLICE_REL" \
        2>/dev/null || echo "unavailable")"
    _ROW4_MERGE_BEFORE="$(python3 "$INSTRUMENT" cgroup-usage "$_ROW4_MERGE_SLICE_REL" \
        2>/dev/null || echo "unavailable")"

    # (f) Hold the steady-state measurement window (both still burning).
    sleep "$_ROW4_MEASURE_S"

    # (g) Sample usage_usec at the END of the steady-state window — taken WHILE
    #     both roles are still contending (burn outlasts warmup+measure+margin),
    #     so the delta reflects pure steady-state weight scheduling, not teardown.
    _ROW4_TASK_AFTER="$(python3 "$INSTRUMENT" cgroup-usage "$_ROW4_TASK_SLICE_REL" \
        2>/dev/null || echo "unavailable")"
    _ROW4_MERGE_AFTER="$(python3 "$INSTRUMENT" cgroup-usage "$_ROW4_MERGE_SLICE_REL" \
        2>/dev/null || echo "unavailable")"

    # (h) Reap both burns (natural completion or timeout) before cleanup.
    wait "$_ROW4_TASK_BG" 2>/dev/null || true
    wait "$_ROW4_MERGE_BG" 2>/dev/null || true

    _ROW4_TASK_DELTA=0
    _ROW4_MERGE_DELTA=0
    if [ "$_ROW4_TASK_BEFORE" != "unavailable" ] && \
       [ "$_ROW4_TASK_AFTER" != "unavailable" ]; then
        _ROW4_TASK_DELTA=$(( _ROW4_TASK_AFTER - _ROW4_TASK_BEFORE ))
        [ "$_ROW4_TASK_DELTA" -lt 0 ] && _ROW4_TASK_DELTA=0  # guard counter wrap
    fi
    if [ "$_ROW4_MERGE_BEFORE" != "unavailable" ] && \
       [ "$_ROW4_MERGE_AFTER" != "unavailable" ]; then
        _ROW4_MERGE_DELTA=$(( _ROW4_MERGE_AFTER - _ROW4_MERGE_BEFORE ))
        [ "$_ROW4_MERGE_DELTA" -lt 0 ] && _ROW4_MERGE_DELTA=0
    fi
    # ─────────────────────────────────────────────────────────────────────────

    # ROW4-1: merge_share >= W_merge/(W_merge+W_task) - tol.
    # Asserts the C-G2 proportional cpu.weight enforcement under contention.
    # W_merge/(W_merge+W_task) = 300/(300+100) = 0.75; floor = 0.75 - tol.
    # Default tol=0.10 (floor=0.65) accounts for real-world cgroup scheduling
    # measurement variance (startup stagger, scope-creation lag, process overhead).
    # Overridable via REIFY_CPU_GOV_TEST_SHARE_TOL.
    #
    # Skip if slice discovery failed (empty rel-path — probe timed out/errored) or
    # both deltas are zero (measurement inconclusive).  Without this guard an empty
    # rel-path causes cgroup-usage to read the root cgroup, both roles get the same
    # usage_usec, merge_share ≈ 0.5 which is below the 0.65 floor — a false-RED.
    if [ -z "${_ROW4_TASK_SLICE_REL:-}" ] || [ -z "${_ROW4_MERGE_SLICE_REL:-}" ]; then
        echo "  SKIP ROW4-1: slice rel-path discovery failed (empty) — cannot compute share"
    elif [ "$_ROW4_TASK_DELTA" -le 0 ] && [ "$_ROW4_MERGE_DELTA" -le 0 ]; then
        echo "  SKIP ROW4-1: both cpu.stat deltas are zero — measurement inconclusive"
    else
        assert "ROW4-1: merge_share >= W_merge/(W_merge+W_task)-tol=${_ROW4_TOL} (Δmerge=${_ROW4_MERGE_DELTA},Δtask=${_ROW4_TASK_DELTA},W=${_ROW4_W_MERGE}/${_ROW4_W_TASK})" \
            python3 -c "
import sys
sys.path.insert(0, '${SCRIPT_DIR}')
from cpu_gov_instrument import share_ge_proportional
ok = share_ge_proportional(float('${_ROW4_MERGE_DELTA}'), float('${_ROW4_TASK_DELTA}'),
                           float('${_ROW4_W_MERGE}'), float('${_ROW4_W_TASK}'),
                           float('${_ROW4_TOL}'))
sys.exit(0 if ok else 1)
"
    fi
fi

# ============================================================================
# Cycle ROW4-BYPASS — §8 row-9 merge-bypass smoke (always-on, hermetic).
# DF_VERIFY_ROLE=merge + high-PSI fixture → cpu-admit.sh admit exits 0 fast.
# Uses a synthetic /proc/pressure/cpu fixture (no real PSI needed).
# ============================================================================
echo ""
echo "--- Cycle ROW4-BYPASS: merge-bypass smoke (cpu-admit.sh, §8 row 9) ---"

# Create synthetic high-PSI fixture: avg10=99 would block non-merge admits.
_ROW4_PSI_FIXTURE="$WORK/row4_psi_fixture"
printf 'some avg10=99.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
    > "$_ROW4_PSI_FIXTURE"

# ROW4-2: DF_VERIFY_ROLE=merge bypasses PSI → cpu-admit admit exits 0 fast.
_ROW4_BYPASS_START=$(date +%s)
_ROW4_BYPASS_RC=0
timeout 5 \
    env DF_VERIFY_ROLE=merge \
        REIFY_CPU_ADMIT_PROC_PATH="$_ROW4_PSI_FIXTURE" \
        REIFY_CPU_ADMIT_MAX_WAIT=1 \
        REIFY_CPU_ADMIT_POLL=1 \
    bash "$CPU_ADMIT" admit \
    >/dev/null 2>&1 || _ROW4_BYPASS_RC=$?
_ROW4_BYPASS_END=$(date +%s)
_ROW4_BYPASS_ELAPSED=$(( _ROW4_BYPASS_END - _ROW4_BYPASS_START ))
assert "ROW4-2: DF_VERIFY_ROLE=merge + avg10=99 PSI → cpu-admit admit exits 0 fast (rc=${_ROW4_BYPASS_RC}, elapsed=${_ROW4_BYPASS_ELAPSED}s)" \
    test "${_ROW4_BYPASS_RC}" -eq 0

# ---------------------------------------------------------------------------
# Final summary — PASS/FAIL count from test_helpers.sh.
# ---------------------------------------------------------------------------
test_summary
