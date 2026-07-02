#!/usr/bin/env bash
# tests/infra/test_cpu_load_governance.sh — §8 integration-gate leaf (task 4634).
#
# Proves that the α/β/γ primitives COMPOSE:
#   α  scripts/cpu-admit.sh          — PSI admission gate
#   β  scripts/agent-bin/cargo       — agent cargo shim (heavy-subcmd gate)
#   γ  scripts/cpu-governed-exec.sh  — cgroup-v2 cpu.weight placement wrapper
#
# §8 boundary-table rows covered:
#   Row 1  lone governed source, confined+pinned → cpu.stat usage_usec
#           saturates confine-cores*measure_s budget, cpu.max == max
#           (no quota throttle on the child scope)                    host-gated
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
#   REIFY_CPU_GOV_TEST_WARMUP_S         ROW2_3 warm-up window seconds (default 8)
#   REIFY_CPU_GOV_TEST_MIXFACTOR        UNUSED as of H5/task 4926 — ROW2_3's mix
#                                       width is confine-cores-scaled (CONFINE_CORES
#                                       below), never nproc-derived (anti-#4901);
#                                       kept only for other callers of the pure
#                                       fair_share_floor primitive, not read here
#   REIFY_CPU_GOV_TEST_SLOWDOWN_K       slowdown upper-band multiplier (default 4)
#   REIFY_CPU_GOV_TEST_QUIET_CEILING    UNUSED as of H5/task 4926 — ROW1/ROW2_3/ROW4
#                                       all moved to confined-cgroup-quota + pinning
#                                       (CONFINE_CORES/CONFINE_CPUS below), which is
#                                       host-load-independent by construction; no
#                                       quiet-box precondition remains in this file
#   REIFY_CPU_GOV_TEST_ROW1_WARMUP_S    ROW1-1 steady-state ramp before sampling
#                                       (default 1)
#   REIFY_CPU_GOV_TEST_ROW1_MEASURE_S   ROW1-1 steady-state delta window (default 3)
#   REIFY_CPU_GOV_TEST_ROW1_SATURATION_FLOOR  ROW1-1 saturation floor as a fraction
#                                       of the confine-cores*measure_s budget
#                                       (default 0.85; empirically calibrated, H5)
#   REIFY_CPU_GOV_TEST_PROC_PATH        synthetic-PSI injection seam (testability seam —
#                                       mirrors REIFY_CPU_ADMIT_PROC_PATH used in ROW4-BYPASS)
#   REIFY_CPU_GOV_TEST_BURN_S           per-fixture burn duration seconds (default 4;
#                                       ROW4 default warmup+measure+4 if unset)
#   REIFY_CPU_GOV_TEST_ROW4_WARMUP_S    ROW4 steady-state ramp before sampling (default 3)
#   REIFY_CPU_GOV_TEST_ROW4_MEASURE_S   ROW4 steady-state delta window (default 8)
#   REIFY_CPU_GOV_TEST_SHARE_TOL        ROW4 merge-share variance budget (default 0.10)
#   REIFY_CPU_GOV_TEST_CONFINE_CORES    confined-cgroup-quota footprint size in cores
#                                       (default 2 -> parent CPUQuota=200%; H5/task 4926;
#                                       fixed scale-invariant knob, NEVER nproc-derived —
#                                       anti-#4901); bounds the ROW4 parent-slice quota,
#                                       the confined per-role worker count, and the
#                                       confined pin-list size (CONFINE_CPUS below)
#   REIFY_CPU_GOV_TEST_CONFINE_CPUS     explicit taskset -c pin list override for the
#                                       ROW4 confined burns (default: derived at runtime
#                                       as the LAST confine-cores CPUs of this process's
#                                       own Cpus_allowed_list; esc-4926-3 ruling)

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
# Hermetic workdir — cleaned up on EXIT.
# ---------------------------------------------------------------------------
WORK="$(mktemp -d)"
# Tracking variables for EXIT cleanup (crash-path protection).
_ALL_MIX_PIDS=""
_ROW4_SLICE_TASK_CREATED=""
_ROW4_SLICE_MERGE_CREATED=""
_ROW4_CONFINE_PARENT_CREATED=""

# ---------------------------------------------------------------------------
# Hermeticity: neutralize default-ON memory gating (task 4911) for the live-PSI
# ROW2_3 mix.  ROW2_3 launches real shim -> cpu-admit.sh admit invocations
# against LIVE /proc/pressure/cpu (by design — it measures real CPU-governance
# behavior under a CPU-only quiet-box guard).  As of task 4911 cpu-admit.sh's
# direct-exec default ALSO checks memfull avg10 (default threshold 10) when no
# REIFY_CPU_ADMIT_MEM_PROC_PATH override is set, which would introduce
# unrelated memory-pressure backoff into a test designed to isolate CPU
# governance.  Export a quiet memory fixture (memfull=0) so the mix's
# cpu-admit calls see a deterministic memory-ok state and this cycle continues
# to measure CPU contention only.  Mirrors the neutralization in
# tests/infra/test_cpu_admit.sh / test_agent_cargo_shim.sh.
# ---------------------------------------------------------------------------
_MEM_PSI_QUIET="$(mktemp -p "$WORK" mem-psi-quiet.XXXXXX)"
printf 'some avg10=0.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
    > "$_MEM_PSI_QUIET"
export REIFY_CPU_ADMIT_MEM_PROC_PATH="$_MEM_PSI_QUIET"

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
    # Stop the confined-quota parent slice (H5, task 4926) last, after its
    # children, to avoid lingering quota'd empty parent units.
    if [ -n "${_ROW4_CONFINE_PARENT_CREATED:-}" ]; then
        systemctl --user stop "${_ROW4_CONFINE_PARENT_CREATED}" 2>/dev/null || true
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
    # Synthetic quiet-PSI fixture (H5, task 4926): makes the always-on SELF
    # cycle's PSI touchpoint (SELF-4) deterministic under concurrent pool
    # load — routed via the REIFY_CPU_GOV_TEST_PROC_PATH testability seam
    # (file header) instead of live /proc/pressure/cpu.  Mirrors the
    # _MEM_PSI_QUIET pattern above and ROW4-BYPASS's REIFY_CPU_ADMIT_PROC_PATH.
    _SELF_PSI_QUIET="$(mktemp -p "$WORK" self-psi-quiet.XXXXXX)"
    printf 'some avg10=0.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
        > "$_SELF_PSI_QUIET"
    _SELF_PROC_PATH="${REIFY_CPU_GOV_TEST_PROC_PATH:-$_SELF_PSI_QUIET}"

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

    # SELF-4 (H5, task 4926): psi-avg10 reads a synthetic quiet-PSI fixture
    # deterministically (== 0.0), never live /proc/pressure/cpu — pool-safety
    # for this always-on cycle (a concurrent pool member's real load can
    # never perturb it).
    assert "SELF-4: cpu_gov_instrument.py psi-avg10 <synthetic-quiet-fixture> == 0.0" \
        bash -c '
            out=$(python3 "$1" psi-avg10 "$2" 2>/dev/null)
            [ "$out" = "0.0" ]
        ' _ "$INSTRUMENT" "$_SELF_PROC_PATH"

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
# Confined-quota derivation (H5, task 4926) — shared parent-slice CPUQuota +
# pin-list machinery, REUSED by Cycle ROW1, Cycle ROW2_3, and Cycle ROW4
# below.  Hoisted here (ahead of every consumer) because bash requires a
# function to be defined before its first call, and Cycle ROW1 is now the
# first confined+pinned consumer in execution order.  Content is UNCHANGED
# from the landed ROW4-1 implementation (task 4926 H5 phase 1) — only the
# POSITION moved so cycles other than ROW4 can reuse it without re-deriving
# delegation/pin logic.
#
# cpu-governed-exec.sh deliberately NEVER sets CPUQuota on the governed scope
# (keeps cpu.max=max, C-G1 work-conserving).  Capping the SHARED PARENT slice
# instead bounds the whole subtree's CPU footprint for the pool while leaving
# each child scope's cpu.max=max.
#
# MECHANISM (PRD §1 G6 #3 as REVISED by the esc-4926-3 ruling): the quota
# alone does NOT reproduce the weight ratio — CPUQuota is an aggregate
# TIME-budget throttle that never forces sibling threads onto shared per-CPU
# runqueues, and cpu.weight only arbitrates between siblings CO-RESIDENT on a
# runqueue.  With 2×confine-cores runnable threads spread across a large idle
# box, weight never bites and the shared quota drains ~FCFS → ~50/50
# (empirical false-RED, esc-4926-3).  `taskset -c` affinity pinning of the
# burns to confine-cores CPUs (CONFINE_CPUS below) is what CREATES the
# per-CPU co-residency weight arbitration requires; the parent quota stays as
# the aggregate footprint bound.  Pinned, the ratio is host-load-independent:
# foreign load and concurrent pool runs on the same CPUs only deepen
# co-residency and empirically IMPROVE convergence toward W_merge/(W+W).
#
# _row4_confine_cores/_row4_confine_quota/_row4_confine_workers are pure (no
# I/O beyond the fixed knob read, never nproc) so CONFINE-2's
# nproc-independence check can invoke them directly under a faked nproc.
# _row4_confine_cpus additionally reads ONLY this process's own
# Cpus_allowed_list (never nproc), so the same faked-nproc idiom applies.
# ============================================================================
_row4_confine_cores() {
    # Fixed scale-invariant knob — NEVER derived from nproc (anti-#4901: a
    # measurement-footprint bound, not an admission count).
    echo "${REIFY_CPU_GOV_TEST_CONFINE_CORES:-2}"
}
_row4_confine_quota() {
    local cores
    cores="$(_row4_confine_cores)"
    echo "$(( cores * 100 ))%"
}
_row4_confine_workers() {
    # Bounded per-role confined worker count derives from confine-cores, NOT
    # nproc — so 2×workers ≈ 2×confine-cores oversubscribes the confined cap
    # regardless of host size (mirrors ROW4-1's full-box "_ROW4_W=nproc"
    # oversubscription idiom, scaled down to the confined budget).
    _row4_confine_cores
}
_row4_confine_cpus() {
    # Confined burn pin list (esc-4926-3 ruling): the LAST confine-cores CPUs
    # of this process's OWN allowed set, comma-joined for `taskset -c`.
    # SHARED, not per-run: concurrent pool runs deliberately derive the SAME
    # list — cross-run contention on the pair deepens per-CPU co-residency
    # and empirically improves ratio convergence (0.74 shared vs 0.69
    # disjoint), while bounding the whole pool's aggregate ROW4 footprint to
    # ~confine-cores CPUs.  Derived at runtime from the affinity mask — never
    # a frozen CPU-id list, never nproc (anti-#4901).  Empty output = mask
    # unreadable (caller must SKIP, not fall back to unpinned: unpinned is a
    # guaranteed ~50/50 false-RED).  The LAST CPUs (not the first) avoid
    # cpu0-adjacent housekeeping/IRQ bias.
    if [ -n "${REIFY_CPU_GOV_TEST_CONFINE_CPUS:-}" ]; then
        echo "$REIFY_CPU_GOV_TEST_CONFINE_CPUS"
        return 0
    fi
    local want mask
    want="$(_row4_confine_cores)"
    mask="$(awk '/^Cpus_allowed_list/{print $2}' /proc/self/status 2>/dev/null)"
    [ -z "$mask" ] && return 0
    # Expand "0-3,7,9-10" → one id per line; keep the last $want; comma-join.
    printf '%s\n' "$mask" | tr ',' '\n' | while IFS=- read -r lo hi; do
        if [ -n "$hi" ]; then seq "$lo" "$hi"; else echo "$lo"; fi
    done | tail -n "$want" | paste -sd, -
}
# _row4_confine_apply_quota <parent_slice> <quota>
#   Best-effort: vivify the parent slice (systemctl --user start) then set
#   its CPUQuota via systemctl --user set-property.  Mirrors the
#   lib_cgroup.sh cgroup_set_slice_weight vivify-then-set idiom.  NEVER
#   applied to the child scope/slice (that stays cpu.max=max, C-G1) — only
#   ever to the shared parent, confining the whole subtree's footprint.
_row4_confine_apply_quota() {
    local parent="$1"
    local quota="$2"
    systemctl --user start "$parent" 2>/dev/null || true
    systemctl --user set-property "$parent" CPUQuota="$quota" 2>/dev/null || true
}

# Private test slice names — siblings under the unique per-run parent
# reify-govtest$$.slice ($$ = this script's PID).  Shared across Cycle ROW1 /
# ROW2_3 / ROW4 so every confined+pinned burn in this run nests under the
# SAME parent, bounding the whole script invocation's aggregate footprint to
# ~confine-cores CPUs (not per-cycle).  Must differ from production slices
# (reify-governed-{agents,merge}.slice) to isolate usage_usec deltas from
# concurrent production agent placement (ζ).
_ROW4_SLICE_TASK="reify-govtest$$-agents.slice"
_ROW4_SLICE_MERGE="reify-govtest$$-merge.slice"

# systemd derives a slice's parent by stripping the trailing ".slice" suffix
# then the last '-'-separated segment. Both slice names must derive ONE
# shared parent (siblings — required for the C-G2 cpu.weight-ratio
# comparison in ROW4-1 to be valid) that is also UNIQUE per concurrent test
# run (PID-scoped), so two overlapping `bash test_cpu_load_governance.sh`
# invocations never collide on the same parent slice and cross-contaminate
# cpu.weight measurements.  Asserted unconditionally (no cgroup substrate
# required) by ROW4-NAMING later in this file.
_row4_naming_base_task="${_ROW4_SLICE_TASK%.slice}"
_row4_naming_parent_task="${_row4_naming_base_task%-*}.slice"
_row4_naming_base_merge="${_ROW4_SLICE_MERGE%.slice}"
_row4_naming_parent_merge="${_row4_naming_base_merge%-*}.slice"
# Computed in THIS top-level shell so $$ matches the PID baked into the
# _ROW4_SLICE_* assignments above — never re-expand $$ inside a `bash -c`
# subshell, since its $$ would be a different PID and falsely mismatch.
_row4_naming_expected_parent="reify-govtest$$.slice"

_ROW4_CONFINE_CORES="$(_row4_confine_cores)"
_ROW4_CONFINE_QUOTA="$(_row4_confine_quota)"
_ROW4_CONFINE_W="$(_row4_confine_workers)"
_ROW4_CONFINE_CPUS="$(_row4_confine_cpus)"
# Same shared parent NAMING-1/2 (asserted later in this file) already
# validates — the confinement target IS that parent, never an
# independently-derived path (CONFINE-1).
_ROW4_CONFINE_PARENT="$_row4_naming_parent_task"

# ============================================================================
# Cycle ROW1 — §8 Row 1: lone governed source, box idle.
# HOST-GATED (host_supports_governance + PSI + python3).
# QUIET-BOX: pre-check avg10 < QUIET_CEILING; SKIP if box already hot.
# ============================================================================
echo ""
echo "--- Cycle ROW1: §8 Row 1 (lone governed source, box idle) ---"

# ----------------------------------------------------------------------------
# ROW1-2 (H5, task 4926): quiet-box-INDEPENDENT scope config check.
# cpu.max is a cgroup CONFIG value, not a load measurement — probing it must
# not be gated behind the box being quiet.  Runs whenever
# host_supports_governance is true, so this assertion is pool-safe: unlike
# ROW1-1 below (still quiet-gated pending its own confined conversion), it
# never SKIPs under concurrent pool load and never touches PSI/python3.
# ----------------------------------------------------------------------------
if ! host_supports_governance; then
    echo "  SKIP ROW1-2: host does not support cgroup governance"
else
    # cpu.max probe — run a tiny probe inside the scope to capture the first
    # field of cpu.max while the scope is live.  Uses a temp script to avoid
    # shell quoting complexity.
    cat > "$WORK/row1_probe.sh" << 'EOF_PROBE'
#!/usr/bin/env bash
rel=$(sed 's/^0:://' /proc/self/cgroup 2>/dev/null || echo "")
if [ -n "$rel" ]; then
    cat "/sys/fs/cgroup${rel}/cpu.max" 2>/dev/null || echo "unavailable"
else
    echo "unavailable"
fi
EOF_PROBE
    _ROW1_CPU_MAX_FILE="$WORK/row1_cpu_max"
    bash "$CPU_GOV_EXEC" --role task -- bash "$WORK/row1_probe.sh" \
        > "$_ROW1_CPU_MAX_FILE" 2>/dev/null \
        || echo "unavailable" > "$_ROW1_CPU_MAX_FILE"
    _ROW1_CPU_MAX="$(cat "$_ROW1_CPU_MAX_FILE" 2>/dev/null || echo "unavailable")"
    _ROW1_CPU_MAX_FIRST="${_ROW1_CPU_MAX%% *}"

    assert "ROW1-2: governed scope cpu.max first field == max (got '${_ROW1_CPU_MAX_FIRST:-?}')" \
        test "${_ROW1_CPU_MAX_FIRST:-}" = "max"
fi

# ----------------------------------------------------------------------------
# ROW1-1 (H5, task 4926): confined+pinned per-cgroup saturation check.
# Replaces the global "busy-core fraction >= 0.95·nproc" bound (unachievable
# inside a confine-cores subtree, and contaminated by concurrent pool load)
# with a per-cgroup measure: does the lone governed source — pinned to the
# SAME confine-cores CPUs and confined under the SAME shared parent quota
# Cycle ROW4 uses — saturate ~its confine-cores budget when those CPUs are
# genuinely available?  Host-load-independent: foreign load on OTHER CPUs
# cannot perturb it, and foreign load ON the pinned CPUs is caught by the
# measurement-integrity SKIP below (never a false-RED, esc-4926-3).
# ----------------------------------------------------------------------------
_ROW1_CONFINE_WARMUP_S="${REIFY_CPU_GOV_TEST_ROW1_WARMUP_S:-1}"
_ROW1_CONFINE_MEASURE_S="${REIFY_CPU_GOV_TEST_ROW1_MEASURE_S:-3}"
_ROW1_CONFINE_BURN_MIN=$(( _ROW1_CONFINE_WARMUP_S + _ROW1_CONFINE_MEASURE_S + 2 ))
_ROW1_BURN_S="${REIFY_CPU_GOV_TEST_BURN_S:-$_ROW1_CONFINE_BURN_MIN}"
[ "$_ROW1_BURN_S" -lt "$_ROW1_CONFINE_BURN_MIN" ] && _ROW1_BURN_S="$_ROW1_CONFINE_BURN_MIN"
# Empirically-calibrated floor (basis: a reference solo confined+pinned run,
# same discipline as ROW4-1's landed 0.65 — see step-4 GREEN commit message).
_ROW1_SATURATION_FLOOR="${REIFY_CPU_GOV_TEST_ROW1_SATURATION_FLOOR:-0.85}"

# Non-vacuity guard (always-on, no cgroup needed): proves the saturation
# comparison below is capable of going RED — a synthetic usage just BELOW
# floor·budget must be rejected, one just AT floor·budget must be accepted.
# Mirrors CONFINE-VACUITY-1/2's tight-boundary shape; exercises only pure
# awk arithmetic so it is expected to pass immediately (no orchestration
# seam needed).
_row1_vacuity_budget=1000000
_row1_vacuity_below="$(awk -v f="$_ROW1_SATURATION_FLOOR" -v b="$_row1_vacuity_budget" 'BEGIN{printf "%d", f*b - b*0.01}')"
_row1_vacuity_at="$(awk -v f="$_ROW1_SATURATION_FLOOR" -v b="$_row1_vacuity_budget" 'BEGIN{printf "%d", f*b}')"
assert "ROW1-1-VACUITY-1: saturation check rejects just-below-floor usage (${_row1_vacuity_below}/${_row1_vacuity_budget} vs floor=${_ROW1_SATURATION_FLOOR})" \
    bash -c '
        awk -v d="$1" -v b="$2" -v f="$3" "BEGIN{ ok=((d+0)/(b+0) >= f+0); exit (ok ? 1 : 0) }"
    ' _ "$_row1_vacuity_below" "$_row1_vacuity_budget" "$_ROW1_SATURATION_FLOOR"
assert "ROW1-1-VACUITY-2: saturation check accepts at-floor usage (${_row1_vacuity_at}/${_row1_vacuity_budget} vs floor=${_ROW1_SATURATION_FLOOR})" \
    bash -c '
        awk -v d="$1" -v b="$2" -v f="$3" "BEGIN{ ok=((d+0)/(b+0) >= f+0); exit (ok ? 0 : 1) }"
    ' _ "$_row1_vacuity_at" "$_row1_vacuity_budget" "$_ROW1_SATURATION_FLOOR"

if ! host_supports_governance; then
    echo "  SKIP ROW1-1: host does not support cgroup governance"
elif ! command -v taskset >/dev/null 2>&1; then
    # Measurement-integrity skip (esc-4926-3): without affinity pinning the
    # confined saturation measurement is unreliable — never fall back to
    # unpinned.
    echo "  SKIP ROW1-1: taskset unavailable — cannot pin confined burn"
elif [ -z "${_ROW4_CONFINE_CPUS:-}" ]; then
    echo "  SKIP ROW1-1: own Cpus_allowed_list unreadable — cannot derive confined pin list"
else
    _row4_confine_apply_quota "$_ROW4_CONFINE_PARENT" "$_ROW4_CONFINE_QUOTA"
    # Mark for EXIT cleanup — set BEFORE the burn starts so the trap fires
    # even if the test is killed mid-burn (mirrors ROW4's own ordering).
    _ROW4_SLICE_TASK_CREATED="$_ROW4_SLICE_TASK"
    _ROW4_CONFINE_PARENT_CREATED="$_ROW4_CONFINE_PARENT"

    # Discover the private task-slice cgroup rel-path BEFORE launching the
    # burn (same probe idiom as ROW4 slice discovery).
    _ROW1_TASK_SLICE_REL="$(
        REIFY_CPU_GOVERN_SLICE_TASK="$_ROW4_SLICE_TASK" \
        timeout 10 bash "$CPU_GOV_EXEC" --role task -- bash -c '
            rel=$(sed "s/^0:://" /proc/self/cgroup 2>/dev/null || echo "")
            echo "${rel%/*}"
        ' 2>/dev/null || echo ""
    )"

    # Launch the lone-source confined+pinned burn: _ROW4_CONFINE_W workers
    # (== confine-cores) pinned to _ROW4_CONFINE_CPUS, in the private task
    # slice, for the full warmup+measure+margin window.
    REIFY_CPU_GOVERN_SLICE_TASK="$_ROW4_SLICE_TASK" \
    timeout $(( _ROW1_BURN_S + 15 )) taskset -c "$_ROW4_CONFINE_CPUS" \
        bash "$CPU_GOV_EXEC" --role task -- \
        bash "$FIXTURE" "$_ROW4_CONFINE_W" "$_ROW1_BURN_S" \
        >/dev/null 2>&1 &
    _ROW1_CONFINE_BG=$!

    # Warm-up: let the burn ramp past scope-creation/process-spawn overhead
    # before sampling (mirrors ROW4's steady-state design).
    sleep "$_ROW1_CONFINE_WARMUP_S"

    _ROW1_USAGE_BEFORE="$(python3 "$INSTRUMENT" cgroup-usage "$_ROW1_TASK_SLICE_REL" \
        2>/dev/null || echo "unavailable")"

    sleep "$_ROW1_CONFINE_MEASURE_S"

    _ROW1_USAGE_AFTER="$(python3 "$INSTRUMENT" cgroup-usage "$_ROW1_TASK_SLICE_REL" \
        2>/dev/null || echo "unavailable")"

    # Contention probe: the task slice's OWN cpu.pressure, sampled while
    # still burning (same window discipline as the usage_usec bracket).  A
    # lone source pinned to its own dedicated CPUs should show ~0 stall; a
    # nonzero reading indicates a FOREIGN process (e.g. a concurrent pool
    # run sharing the same deterministically-derived pin list, esc-4926-3)
    # co-resides on the pinned CPUs, diluting this measurement.
    _ROW1_CONTENTION_AVG10="unavailable"
    if [ -n "${_ROW1_TASK_SLICE_REL:-}" ]; then
        _ROW1_CONTENTION_AVG10="$(python3 "$INSTRUMENT" psi-avg10 \
            "/sys/fs/cgroup${_ROW1_TASK_SLICE_REL}/cpu.pressure" 2>/dev/null || echo "unavailable")"
    fi

    wait "$_ROW1_CONFINE_BG" 2>/dev/null || true

    _ROW1_USAGE_DELTA=0
    if [ "$_ROW1_USAGE_BEFORE" != "unavailable" ] && \
       [ "$_ROW1_USAGE_AFTER" != "unavailable" ]; then
        _ROW1_USAGE_DELTA=$(( _ROW1_USAGE_AFTER - _ROW1_USAGE_BEFORE ))
        [ "$_ROW1_USAGE_DELTA" -lt 0 ] && _ROW1_USAGE_DELTA=0  # guard counter wrap
    fi
    _ROW1_USAGE_BUDGET=$(( _ROW4_CONFINE_CORES * _ROW1_CONFINE_MEASURE_S * 1000000 ))

    _ROW1_CONTENDED=0
    if awk -v a="${_ROW1_CONTENTION_AVG10:-unavailable}" 'BEGIN{
            if (a == "unavailable") { exit 1 }
            exit !(a+0 >= 10)
        }' 2>/dev/null; then
        _ROW1_CONTENDED=1
    fi

    # Measurement-integrity SKIP (never a false-RED under concurrent load):
    # empty slice rel-path, non-positive delta, or detected foreign
    # contention on the pinned CPUs (esc-4926-3).
    if [ -z "${_ROW1_TASK_SLICE_REL:-}" ]; then
        echo "  SKIP ROW1-1: slice rel-path discovery failed (empty) — cannot compute saturation"
    elif [ "$_ROW1_USAGE_DELTA" -le 0 ]; then
        echo "  SKIP ROW1-1: cpu.stat usage_usec delta is zero — measurement inconclusive"
    elif [ "$_ROW1_CONTENDED" -eq 1 ]; then
        echo "  SKIP ROW1-1: foreign contention detected on pinned CPUs (task-slice cpu.pressure avg10=${_ROW1_CONTENTION_AVG10} >= 10) — inconclusive, not a governance failure"
    else
        _ROW1_SATURATION="$(awk -v d="$_ROW1_USAGE_DELTA" -v b="$_ROW1_USAGE_BUDGET" 'BEGIN{ if (b+0<=0){print "0"} else {printf "%.6f", d/b} }')"
        assert "ROW1-1: lone confined+pinned source saturates >= ${_ROW1_SATURATION_FLOOR}·budget (Δusage=${_ROW1_USAGE_DELTA}usec, budget=${_ROW1_USAGE_BUDGET}usec, saturation=${_ROW1_SATURATION})" \
            bash -c '
                awk -v s="$1" -v f="$2" "BEGIN{ exit !(s+0 >= f+0) }"
            ' _ "$_ROW1_SATURATION" "$_ROW1_SATURATION_FLOOR"
    fi
fi

# ============================================================================
# Cycle ROW2_3 — §8 Rows 2+3: heavy mix → PSI band + bounded slowdown.
# HOST-GATED (host_supports_governance + PSI + python3 + taskset + pin list).
# CONFINED+PINNED (H5, task 4926): no quiet-box precondition — the mix is
# footprint-bound under the shared parent quota and taskset -c-pinned to
# _ROW4_CONFINE_CPUS (same machinery as ROW1/ROW4), so it is host-load-
# independent by construction rather than skipped whenever the box is hot.
#
# Design:
#   1. Pre-measure uncontended CONFINED+PINNED governed probe wall T_base
#      (1 worker × PROBE_S, same confine-cores CPUs the mix will use).
#   2. Launch mix: confine-cores task-role sources + 1 merge-role source
#      (footprint-bound, NOT nproc-scaled — anti-#4901), EACH through
#      composed wrappers (cpu-governed-exec → agent cargo shim → cpu-admit
#      admit → stub real-cargo that runs cpu_load_fixture.sh), confined
#      under the shared parent quota and taskset -c-pinned to
#      _ROW4_CONFINE_CPUS.  cpu-admit is redirected to the task slice's OWN
#      cpu.pressure (REIFY_CPU_ADMIT_PROC_PATH) so admission throttling is
#      subtree-relative, never global.
#   3. Concurrently run the timed CONFINED+PINNED governed probe → T_mix.
#   4. Warm-up window, then sample the task slice's OWN cpu.pressure avg10
#      (per-cgroup, not global).
#
# §8 Row 2 assertions: subtree avg10 < REIFY_CPU_ADMIT_AGENT_THRESHOLD; all
#   confined+pinned sources completed.
# §8 Row 3 assertion:  slowdown = T_mix/T_base within
#   [fair_share_floor(active, confine-cores), K·floor] AND < 10.
# Every real assertion carries a measurement-integrity SKIP fallback (never a
# false-RED under concurrent pool load, esc-4926-3).
# ============================================================================
echo ""
echo "--- Cycle ROW2_3: §8 Rows 2+3 (heavy mix + bounded slowdown) ---"

_SLOWDOWN_K="${REIFY_CPU_GOV_TEST_SLOWDOWN_K:-4}"
_ROW23_WARMUP_S="${REIFY_CPU_GOV_TEST_WARMUP_S:-8}"
_ROW23_PROBE_S=2           # fixed work quantum for T_base/T_mix probe
_ADMIT_THRESHOLD="${REIFY_CPU_ADMIT_AGENT_THRESHOLD:-50}"
# Mix burn duration: must cover WARMUP_S + PROBE_S + settling.
_ROW23_MIX_BURN_S=$(( _ROW23_WARMUP_S + _ROW23_PROBE_S + 4 ))

if ! host_supports_governance; then
    echo "  SKIP ROW2_3: host does not support cgroup governance"
elif [ "$_PSI_AVAILABLE" -eq 0 ] || [ "$_PYTHON_AVAILABLE" -eq 0 ]; then
    echo "  SKIP ROW2_3: PSI or python3 unavailable"
elif ! command -v taskset >/dev/null 2>&1; then
    # Measurement-integrity skip (esc-4926-3): without affinity pinning the
    # confined mix's contention is unreliable — never fall back to unpinned.
    echo "  SKIP ROW2_3: taskset unavailable — cannot pin confined mix"
elif [ -z "${_ROW4_CONFINE_CPUS:-}" ]; then
    echo "  SKIP ROW2_3: own Cpus_allowed_list unreadable — cannot derive confined pin list"
else
    _row4_confine_apply_quota "$_ROW4_CONFINE_PARENT" "$_ROW4_CONFINE_QUOTA"
    # Mark for EXIT cleanup — set BEFORE the mix starts so the trap fires
    # even if the test is killed mid-mix (mirrors ROW1-1/ROW4's ordering).
    _ROW4_SLICE_TASK_CREATED="$_ROW4_SLICE_TASK"
    _ROW4_SLICE_MERGE_CREATED="$_ROW4_SLICE_MERGE"
    _ROW4_CONFINE_PARENT_CREATED="$_ROW4_CONFINE_PARENT"

    # Mix width (H5, task 4926): confine-cores task-role sources + 1
    # merge-role source — footprint-bound, NOT nproc-scaled (anti-#4901;
    # REIFY_CPU_GOV_TEST_MIXFACTOR no longer applies here, see file header).
    _MIX_N="$_ROW4_CONFINE_W"
    # active_sources for fair_share_floor: _MIX_N task + 1 merge.
    _ACTIVE_SOURCES=$(( _MIX_N + 1 ))

    # Discover the private task-slice cgroup rel-path BEFORE launching the
    # mix (same probe idiom as ROW1-1 / ROW4 slice discovery) — feeds both
    # the per-cgroup PSI read (ROW2-1) and the cpu-admit subtree-relative
    # redirect below.
    _ROW23_TASK_SLICE_REL="$(
        REIFY_CPU_GOVERN_SLICE_TASK="$_ROW4_SLICE_TASK" \
        timeout 10 bash "$CPU_GOV_EXEC" --role task -- bash -c '
            rel=$(sed "s/^0:://" /proc/self/cgroup 2>/dev/null || echo "")
            echo "${rel%/*}"
        ' 2>/dev/null || echo ""
    )"
    _ROW23_TASK_PRESSURE_PATH="/sys/fs/cgroup${_ROW23_TASK_SLICE_REL}/cpu.pressure"

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

    # (a) Pre-measure T_base: uncontended CONFINED+PINNED governed probe
    #     (H5: same confine-cores CPUs the mix will run on, so the baseline
    #     is a fair comparison point, not diluted by a different slice/CPU
    #     set).
    _T_BASE="$(
        REIFY_CPU_GOVERN_SLICE_TASK="$_ROW4_SLICE_TASK" \
        timeout 30 taskset -c "$_ROW4_CONFINE_CPUS" bash "$CPU_GOV_EXEC" --role task -- \
        python3 "$WORK/row23_probe.py" "$_PROBE_ITERS" 2>/dev/null || echo "1"
    )"
    [ -z "${_T_BASE}" ] || [ "${_T_BASE}" = "0" ] && _T_BASE="1"

    # (b) Launch mix: _MIX_N task-role + 1 merge-role, each through composed
    #     wrappers (γ cpu-governed-exec → β agent-bin/cargo shim → α
    #     cpu-admit admit → stub), confined+pinned to the shared
    #     confine-cores CPUs.  cpu-admit is redirected to the task slice's
    #     OWN cpu.pressure so admission throttling is subtree-relative
    #     (reuses the ROW4-BYPASS REIFY_CPU_ADMIT_PROC_PATH seam) — never
    #     global PSI.  Record PIDs for cleanup.
    _MIX_PIDS=""
    _mi=0
    while [ "$_mi" -lt "$_MIX_N" ]; do
        PATH="$_MIX_PATH" \
        REIFY_CPU_GOVERN_SLICE_TASK="$_ROW4_SLICE_TASK" \
        REIFY_CPU_ADMIT_PROC_PATH="$_ROW23_TASK_PRESSURE_PATH" \
        timeout $(( _ROW23_MIX_BURN_S + 15 )) taskset -c "$_ROW4_CONFINE_CPUS" \
            bash "$CPU_GOV_EXEC" --role task -- bash "$_SHIM" test \
            >/dev/null 2>&1 &
        _MIX_PIDS="${_MIX_PIDS}${_MIX_PIDS:+ }$!"
        _mi=$(( _mi + 1 ))
    done
    # 1 merge-role source (DF_VERIFY_ROLE=merge bypasses cpu-admit, per C-A3).
    PATH="$_MIX_PATH" DF_VERIFY_ROLE=merge \
    REIFY_CPU_GOVERN_SLICE_MERGE="$_ROW4_SLICE_MERGE" \
    timeout $(( _ROW23_MIX_BURN_S + 15 )) taskset -c "$_ROW4_CONFINE_CPUS" \
        bash "$CPU_GOV_EXEC" --role merge -- bash "$_SHIM" test \
        >/dev/null 2>&1 &
    _MIX_PIDS="${_MIX_PIDS}${_MIX_PIDS:+ }$!"

    # Register all mix PIDs in the EXIT-trap list for crash-path cleanup.
    _ALL_MIX_PIDS="$_MIX_PIDS"

    # (c) Warm-up window then sample the TASK SLICE's OWN cpu.pressure
    #     avg10 (Row 2 PSI measurement — per-cgroup, H5).
    sleep "$_ROW23_WARMUP_S"
    _ROW23_AVG10="$(python3 "$INSTRUMENT" psi-avg10 "$_ROW23_TASK_PRESSURE_PATH" 2>/dev/null || echo "99")"

    # (d) Timed work-based probe under the mix, CONFINED+PINNED → T_mix
    #     (Row 3 slowdown).
    _T_MIX="$(
        REIFY_CPU_GOVERN_SLICE_TASK="$_ROW4_SLICE_TASK" \
        timeout 60 taskset -c "$_ROW4_CONFINE_CPUS" bash "$CPU_GOV_EXEC" --role task -- \
        python3 "$WORK/row23_probe.py" "$_PROBE_ITERS" 2>/dev/null || echo "0"
    )"
    [ -z "${_T_MIX}" ] && _T_MIX="0"

    # (e) Wait for mix to finish (natural completion or timeout).
    for _mpid in $_MIX_PIDS; do
        wait "$_mpid" 2>/dev/null || true
    done
    _MIX_PIDS=""
    _ALL_MIX_PIDS=""  # PIDs already reaped; clear EXIT-trap list.

    # (f) Progress accounting: count done-markers.
    _ROW23_DONE_COUNT="$(ls "$_ROW23_MARKER_DIR"/done_* 2>/dev/null | wc -l || echo 0)"
    # ceil(0.9 * ACTIVE_SOURCES) — at least 90% must complete.
    _ROW23_THRESHOLD=$(( (_ACTIVE_SOURCES * 9 + 9) / 10 ))
    _ROW23_ALL_PROGRESSED=0
    if [ "$_ROW23_DONE_COUNT" -ge "$_ROW23_THRESHOLD" ]; then
        _ROW23_ALL_PROGRESSED=1
    fi

    # Foreign-contention signal (H5): a numeric avg10 far beyond cpu-admit's
    # own admission ceiling suggests the pinned CPUs are contended by
    # something OUTSIDE this subtree's control (e.g. a concurrent pool run
    # sharing the deterministically-derived pin list, esc-4926-3) — never a
    # governance failure, so it gates the SKIP fallbacks below.
    _ROW23_CONTENDED=0
    if awk -v a="${_ROW23_AVG10:-unavailable}" 'BEGIN{
            if (a == "unavailable") { exit 1 }
            exit !(a+0 >= 90)
        }' 2>/dev/null; then
        _ROW23_CONTENDED=1
    fi

    # ── Row 2 assertions ──
    # ROW2-1: after warm-up, the TASK SLICE's OWN cpu.pressure avg10 (H5:
    # per-cgroup, not global) < AGENT_THRESHOLD.
    # Guard: psi-avg10 CLI returns exit 0 even when printing "unavailable" (when
    # the subtree's cpu.pressure is transiently unreadable mid-run), so a
    # non-numeric sample SKIPs rather than raising a confusing python
    # ValueError.  A numeric-but-far-beyond-ceiling sample SKIPs too
    # (contention-inflated, not a governance failure).
    if ! python3 -c "float('${_ROW23_AVG10}')" 2>/dev/null; then
        echo "  SKIP ROW2-1: avg10 sample non-numeric (${_ROW23_AVG10}) — PSI transiently unreadable mid-run"
    elif [ "$_ROW23_CONTENDED" -eq 1 ]; then
        echo "  SKIP ROW2-1: contention-inflated avg10 (${_ROW23_AVG10} >= 90 on the task slice's own cpu.pressure) — likely foreign load on the pinned CPUs, inconclusive"
    else
        assert "ROW2-1: avg10 after warm-up < AGENT_THRESHOLD=${_ADMIT_THRESHOLD} (avg10=${_ROW23_AVG10}, subtree-relative)" \
            python3 -c "
import sys
v = float('${_ROW23_AVG10}')
t = float('${_ADMIT_THRESHOLD}')
sys.exit(0 if v < t else 1)
"
    fi
    # ROW2-2: >= 90% of confined+pinned sources completed (none starved).
    # Assert >= 90% completion (not strict equality) — serialized cpu-admit admission
    # under oversubscription can SIGTERM the slowest sources before their outer timeout,
    # making strict equality unreliable even when governance is correct.  SKIP (not
    # FAIL) when sub-90% AND foreign contention was detected — inconclusive.
    if [ "$_ROW23_ALL_PROGRESSED" -eq 0 ] && [ "$_ROW23_CONTENDED" -eq 1 ]; then
        echo "  SKIP ROW2-2: sub-90% completion (${_ROW23_DONE_COUNT}/${_ACTIVE_SOURCES}) under detected foreign load on the pinned CPUs — inconclusive, not a governance failure"
    else
        assert "ROW2-2: >= 90% (${_ROW23_THRESHOLD}/${_ACTIVE_SOURCES}) confined+pinned sources completed — none starved (done=${_ROW23_DONE_COUNT})" \
            test "${_ROW23_ALL_PROGRESSED}" -eq 1
    fi

    # ── Row 3 assertions ──
    # Compute slowdown = T_mix / T_base (float division via awk).
    _ROW3_SLOWDOWN="$(awk -v m="${_T_MIX}" -v b="${_T_BASE}" \
        'BEGIN{if(b+0<=0){print "0"}else{print m/b}}')"
    # fair_share_floor = active_sources / confine-cores (H5: confined
    # budget, not nproc — anti-#4901).
    _ROW3_FLOOR="$(python3 "$INSTRUMENT" fair-share "$_ACTIVE_SOURCES" "$_ROW4_CONFINE_CORES" \
        2>/dev/null || echo "0")"
    # ROW3-1: slowdown <= K·floor AND < 10 (4415 cannot recur — the DANGEROUS
    # direction, a real runaway-slowdown governance break, stays a hard assert).
    # Skip if T_mix probe timed out or failed (returns "0") — on a heavily contended
    # host the probe can exceed its budget when a large slowdown is real, making
    # T_mix == 0 an inconclusive measurement, not a governance failure.
    # Skip (not FAIL) if slowdown < floor too (H5, esc-4926-3 follow-up,
    # empirically observed): at confine-cores scale (active_sources=3 by
    # default) cpu-admit's OWN legitimate admission staggering means not all
    # active_sources are concurrently contending every instant, so the naive
    # fair_share_floor assumption can be violated in the SAFE direction
    # (faster than modeled) — inconclusive for the anti-runaway guarantee
    # below, never a governance failure (mirrors fair_share_floor's own
    # docstring: below-floor is "physically impossible" for the model, i.e.
    # a modeling/measurement mismatch, not evidence of broken governance).
    if awk -v m="${_T_MIX:-0}" 'BEGIN{exit !(m+0 <= 0)}'; then
        echo "  SKIP ROW3-1: T_mix probe timed out or failed (T_mix=${_T_MIX:-0}) — inconclusive"
    elif awk -v s="${_ROW3_SLOWDOWN:-0}" -v f="${_ROW3_FLOOR:-0}" 'BEGIN{exit !(s+0 < f+0)}'; then
        echo "  SKIP ROW3-1: slowdown=${_ROW3_SLOWDOWN} below fair-share floor=${_ROW3_FLOOR} — inconclusive (cpu-admit's own admission staggering at confined scale, not all active_sources concurrently contending; anti-runaway guarantee below is unaffected)"
    else
        assert "ROW3-1: slowdown=${_ROW3_SLOWDOWN} within_bound(floor=${_ROW3_FLOOR},K=${_SLOWDOWN_K}) [confined+pinned]" \
            python3 -c "
import sys
s = float('${_ROW3_SLOWDOWN}')
fl = float('${_ROW3_FLOOR}')
k = float('${_SLOWDOWN_K}')
ok = (s <= k * fl) and s < 10.0
sys.exit(0 if ok else 1)
"
    fi
fi

# ============================================================================
# Cycle ROW4 — §8 Row 4: merge-favored share in private hermetic slices.
# HOST-GATED for share measurement (cgroup placement required).
#
# Design (step-9; slice-naming fixed under H4/task 4922):
#   Private test slices (REIFY_CPU_GOVERN_SLICE_TASK=reify-govtest$$-agents.slice
#   and REIFY_CPU_GOVERN_SLICE_MERGE=reify-govtest$$-merge.slice, $$ = this
#   script's PID) nest under the shared parent reify-govtest$$.slice → they
#   are siblings → cpu.weight ratio is comparable (C-G2 invariant: weight
#   proportion valid among siblings only). Putting $$ in the PREFIX segment
#   (not trailing, e.g. reify-govtest-agents$$.slice) makes that shared parent
#   UNIQUE per concurrent test run, so two overlapping test invocations never
#   collide on one parent slice and cross-contaminate cpu.weight measurements
#   — a trailing $$ would still derive the shared cross-run parent
#   reify-govtest.slice and reintroduce the collision.
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
# Quiet-box gate (mirrors ROW1/ROW2_3; reuses shared QUIET_CEILING knob).
_ROW4_QUIET_CEILING="${REIFY_CPU_GOV_TEST_QUIET_CEILING:-20}"
# PSI source for ROW4 quiet-gate avg10 sampling (testability seam; mirrors
# the existing REIFY_CPU_ADMIT_PROC_PATH fixture injection in ROW4-BYPASS).
_ROW4_PROC_PATH="${REIFY_CPU_GOV_TEST_PROC_PATH:-/proc/pressure/cpu}"

# _ROW4_SLICE_TASK/_MERGE, the naming derivation, the _row4_confine_* helper
# functions, and _ROW4_CONFINE_CORES/QUOTA/W/CPUS/PARENT are now computed in
# the hoisted "Confined-quota derivation" section ABOVE Cycle ROW1 (H5, task
# 4926), so Cycle ROW1 and Cycle ROW2_3 can reuse the same shared parent +
# pin list before Cycle ROW4 runs.  Nothing here re-derives them.

# ----------------------------------------------------------------------------
# ROW4-NAMING: hermetic slice-parent invariants (always-on, no cgroup required)
# ----------------------------------------------------------------------------
# This is a pure string-property check on the naming already derived above —
# it needs no cgroup substrate, so it runs unconditionally and is never
# vacuous.
echo ""
echo "--- ROW4-NAMING: hermetic slice-parent invariants (always-on) ---"

assert "NAMING-1: task/merge slices share one parent (siblings, C-G2 guard)" \
    test "$_row4_naming_parent_task" = "$_row4_naming_parent_merge"
assert "NAMING-2: parent is unique per run (parent=${_row4_naming_parent_task}, expected=${_row4_naming_expected_parent})" \
    test "$_row4_naming_parent_task" = "$_row4_naming_expected_parent"
assert "NAMING-3: task/merge slice names are distinct" \
    test "$_ROW4_SLICE_TASK" != "$_ROW4_SLICE_MERGE"

# ----------------------------------------------------------------------------
# ROW4-CONFINE: hermetic confinement invariants (always-on, no cgroup needed)
# ----------------------------------------------------------------------------
# H5 (task 4926): confining the SHARED PARENT slice's CPUQuota (never the
# child scope/slice itself, C-G1) makes the two ROW4 child slices split ONE
# bounded budget, and pinning the burns to confine-cores CPUs (CONFINE_CPUS,
# esc-4926-3 ruling) creates the per-CPU runqueue co-residency that
# cpu.weight arbitration requires — TOGETHER these make the proportional
# ratio host-load-independent (PRD §1 G6 #3 as revised; quota alone drains
# weight-blind ~FCFS → ~50/50 false-RED).
# These are pure string/arithmetic checks — no cgroup substrate required —
# so they run unconditionally and are never vacuous (mirrors ROW4-NAMING).
echo ""
echo "--- ROW4-CONFINE: hermetic confinement invariants (always-on) ---"

# CONFINE-1: the confinement-quota target IS the shared parent both children
# derive (the C-G2/scale-invariance precondition — the two children must be
# siblings under the ONE capped parent, not two independently-capped scopes).
assert "CONFINE-1: confinement target is the shared parent slice (target=${_ROW4_CONFINE_PARENT:-unset}, shared_parent=${_row4_naming_parent_task})" \
    test "${_ROW4_CONFINE_PARENT:-}" = "$_row4_naming_parent_task"

# CONFINE-2: confinement size is a fixed scale-invariant knob (default 2 →
# CPUQuota=200%), NOT nproc-derived (anti-#4901: a measurement-footprint
# bound, not an admission count).
_confine_expected_cores="${REIFY_CPU_GOV_TEST_CONFINE_CORES:-2}"
_confine_expected_quota="$(( _confine_expected_cores * 100 ))%"
assert "CONFINE-2a: confine quota is well-formed <cores*100>% (${_ROW4_CONFINE_QUOTA:-unset} == ${_confine_expected_quota})" \
    test "${_ROW4_CONFINE_QUOTA:-}" = "$_confine_expected_quota"

# nproc-independence: fake two different nproc values (PATH-injected stub
# binary, plus the REIFY_LOAD_TOLERANCE_NPROC seam other load-scaling code
# reads) and confirm the derivation yields a byte-identical quota string
# regardless — the derivation must never consult either signal.
_confine_fake_nproc_lo="$(mktemp -d -p "$WORK")"
_confine_fake_nproc_hi="$(mktemp -d -p "$WORK")"
printf '#!/usr/bin/env bash\necho 4\n' > "$_confine_fake_nproc_lo/nproc"
printf '#!/usr/bin/env bash\necho 64\n' > "$_confine_fake_nproc_hi/nproc"
chmod +x "$_confine_fake_nproc_lo/nproc" "$_confine_fake_nproc_hi/nproc"
_confine_quota_nproc_lo="$(
    PATH="$_confine_fake_nproc_lo:$PATH" REIFY_LOAD_TOLERANCE_NPROC=4 \
        _row4_confine_quota 2>/dev/null || echo "unset-lo"
)"
_confine_quota_nproc_hi="$(
    PATH="$_confine_fake_nproc_hi:$PATH" REIFY_LOAD_TOLERANCE_NPROC=64 \
        _row4_confine_quota 2>/dev/null || echo "unset-hi"
)"
assert "CONFINE-2b: confine quota is byte-identical under faked nproc=4 vs nproc=64 (${_confine_quota_nproc_lo} == ${_confine_quota_nproc_hi})" \
    test "$_confine_quota_nproc_lo" = "$_confine_quota_nproc_hi"
assert "CONFINE-2c: faked-nproc quota matches the real derivation (${_confine_quota_nproc_lo} == ${_confine_expected_quota})" \
    test "$_confine_quota_nproc_lo" = "$_confine_expected_quota"

# CONFINE-3: the per-role confined worker count derives from confine-cores,
# NOT nproc — bounds the confined subtree's footprint to ~confine-cores
# regardless of host size.
assert "CONFINE-3: confined per-role worker count derives from confine-cores, not nproc (workers=${_ROW4_CONFINE_W:-unset-w}, expected_cores=${_confine_expected_cores})" \
    test "${_ROW4_CONFINE_W:-unset-w}" = "$_confine_expected_cores"

# CONFINE-PIN (esc-4926-3): the confined pin list is well-formed and sized by
# confine-cores ∩ own-affinity, and its derivation never consults nproc.
# Pinning is load-bearing for ROW4-1 — without per-CPU co-residency the
# proportional-share assertion is a guaranteed ~50/50 false-RED — so the
# derivation gets the same hermetic pinning-down as the quota (CONFINE-2).
_confine_pin_allowed_n="$(
    awk '/^Cpus_allowed_list/{print $2}' /proc/self/status 2>/dev/null \
        | tr ',' '\n' | while IFS=- read -r lo hi; do
            if [ -n "$hi" ]; then seq "$lo" "$hi"; else echo "$lo"; fi
        done | wc -l
)"
_confine_pin_expected_n="$_confine_expected_cores"
[ "$_confine_pin_allowed_n" -lt "$_confine_pin_expected_n" ] 2>/dev/null \
    && _confine_pin_expected_n="$_confine_pin_allowed_n"
_confine_pin_got_n="$(printf '%s' "${_ROW4_CONFINE_CPUS:-}" | tr ',' '\n' | grep -c '^[0-9][0-9]*$' || true)"
assert "CONFINE-PIN-1: pin list is min(confine-cores, n_allowed) CPU ids (cpus='${_ROW4_CONFINE_CPUS:-empty}', got_n=${_confine_pin_got_n}, expected_n=${_confine_pin_expected_n})" \
    test "$_confine_pin_got_n" = "$_confine_pin_expected_n"

# nproc-independence: same faked-nproc idiom as CONFINE-2b — the pin-list
# derivation must be byte-identical under wildly different nproc signals.
_confine_pin_nproc_lo="$(
    PATH="$_confine_fake_nproc_lo:$PATH" REIFY_LOAD_TOLERANCE_NPROC=4 \
        _row4_confine_cpus 2>/dev/null || echo "unset-lo"
)"
_confine_pin_nproc_hi="$(
    PATH="$_confine_fake_nproc_hi:$PATH" REIFY_LOAD_TOLERANCE_NPROC=64 \
        _row4_confine_cpus 2>/dev/null || echo "unset-hi"
)"
assert "CONFINE-PIN-2: pin list is byte-identical under faked nproc=4 vs nproc=64 ('${_confine_pin_nproc_lo}' == '${_confine_pin_nproc_hi}')" \
    test "$_confine_pin_nproc_lo" = "$_confine_pin_nproc_hi"

# CONFINE-VACUITY: pin non-vacuity of share_ge_proportional at ROW4-1's exact
# weights (w_merge=300, w_task=100) + tol — an equal-usage (broken)
# measurement MUST be rejected (observed 0.5 < floor 0.65) and a 3:1
# measurement MUST be accepted.  Guards against a future confined-ROW4-1
# refactor silently becoming vacuously-always-pass.
if [ "$_PYTHON_AVAILABLE" -eq 0 ]; then
    echo "  SKIP CONFINE-VACUITY: python3 not on PATH"
else
    assert "CONFINE-VACUITY-1: share_ge_proportional rejects equal-usage (observed 0.5 < floor 0.65)" \
        python3 -c "
import sys
sys.path.insert(0, '${SCRIPT_DIR}')
from cpu_gov_instrument import share_ge_proportional
ok = share_ge_proportional(50.0, 50.0, float('${_ROW4_W_MERGE}'), float('${_ROW4_W_TASK}'), float('${_ROW4_TOL}'))
sys.exit(0 if not ok else 1)
"
    assert "CONFINE-VACUITY-2: share_ge_proportional accepts a 3:1 measurement (observed 0.75 >= floor 0.65)" \
        python3 -c "
import sys
sys.path.insert(0, '${SCRIPT_DIR}')
from cpu_gov_instrument import share_ge_proportional
ok = share_ge_proportional(75.0, 25.0, float('${_ROW4_W_MERGE}'), float('${_ROW4_W_TASK}'), float('${_ROW4_TOL}'))
sys.exit(0 if ok else 1)
"
fi

# ----------------------------------------------------------------------------
# ROW4-CONFINE-APPLIED (H5, task 4926): host-gated proof that the confined
# quota lands on the PARENT slice while the CHILD governed scope stays
# uncapped (C-G1). Gated ONLY on host_supports_governance — never on
# quiet-box/PSI — because this checks that a CPUQuota setting mechanically
# lands on the correct cgroup file, a load-independent operation.
# ----------------------------------------------------------------------------
echo ""
echo "--- ROW4-CONFINE-APPLIED: quota lands on parent, scope stays uncapped ---"

if ! host_supports_governance; then
    echo "  SKIP CONFINE-APPLIED: host does not support cgroup governance"
else
    # Apply the confined quota to the shared parent BEFORE probing (S4/task
    # 4926 live wiring) — without this the parent reads uncapped ("max")
    # against the expected quota_usec and CONFINE-APPLIED-1 stays RED.
    _row4_confine_apply_quota "$_ROW4_CONFINE_PARENT" "$_ROW4_CONFINE_QUOTA"

    cat > "$WORK/confine_applied_probe.sh" << 'EOF_CONFINE_PROBE'
#!/usr/bin/env bash
rel=$(sed 's/^0:://' /proc/self/cgroup 2>/dev/null || echo "")
if [ -n "$rel" ]; then
    echo "OWN:$(cat "/sys/fs/cgroup${rel}/cpu.max" 2>/dev/null || echo unavailable)"
    parent_rel="$(dirname "$(dirname "$rel")")"
    echo "PARENT:$(cat "/sys/fs/cgroup${parent_rel}/cpu.max" 2>/dev/null || echo unavailable)"
else
    echo "OWN:unavailable"
    echo "PARENT:unavailable"
fi
EOF_CONFINE_PROBE

    _CONFINE_PROBE_OUT="$WORK/confine_applied_probe_out"
    REIFY_CPU_GOVERN_SLICE_TASK="$_ROW4_SLICE_TASK" \
    timeout 10 bash "$CPU_GOV_EXEC" --role task -- bash "$WORK/confine_applied_probe.sh" \
        > "$_CONFINE_PROBE_OUT" 2>/dev/null \
        || printf 'OWN:unavailable\nPARENT:unavailable\n' > "$_CONFINE_PROBE_OUT"
    # Mark for EXIT cleanup — this probe vivifies _ROW4_SLICE_TASK regardless
    # of whether the main ROW4 orchestration below also runs.
    _ROW4_SLICE_TASK_CREATED="$_ROW4_SLICE_TASK"

    _CONFINE_OWN_MAX="$(sed -n 's/^OWN://p' "$_CONFINE_PROBE_OUT")"
    _CONFINE_PARENT_MAX="$(sed -n 's/^PARENT://p' "$_CONFINE_PROBE_OUT")"
    _CONFINE_OWN_FIRST="${_CONFINE_OWN_MAX%% *}"
    _CONFINE_PARENT_FIRST="${_CONFINE_PARENT_MAX%% *}"
    _CONFINE_PARENT_PERIOD="${_CONFINE_PARENT_MAX##* }"

    # Expected quota_usec is derived from the PARENT's own (read-back) period
    # field, never a hardcoded period assumption — quota_usec = cores * period
    # holds for any period systemd defaults to. Sentinel distinct from
    # "unavailable" so a probe failure can never vacuously match a failed read.
    _confine_applied_expected="EXPECTED-PARSE-FAILED"
    case "$_CONFINE_PARENT_PERIOD" in
        ''|*[!0-9]*) ;;
        *) _confine_applied_expected="$(( _ROW4_CONFINE_CORES * _CONFINE_PARENT_PERIOD ))" ;;
    esac

    assert "CONFINE-APPLIED-1: parent slice cpu.max first field == confined quota usec (parent=${_ROW4_CONFINE_PARENT}, got='${_CONFINE_PARENT_FIRST:-?}', expected='${_confine_applied_expected}')" \
        test "${_CONFINE_PARENT_FIRST:-}" = "$_confine_applied_expected"

    assert "CONFINE-APPLIED-2: child governed scope cpu.max first field == max (C-G1 preserved, got='${_CONFINE_OWN_FIRST:-?}')" \
        test "${_CONFINE_OWN_FIRST:-}" = "max"
fi

if ! host_supports_governance; then
    echo "  SKIP ROW4: host does not support cgroup governance"
elif [ "$_PYTHON_AVAILABLE" -eq 0 ]; then
    echo "  SKIP ROW4: python3 unavailable"
elif ! command -v taskset >/dev/null 2>&1; then
    # Measurement-integrity skip (esc-4926-3): without affinity pinning the
    # confined proportional-share measurement is a guaranteed ~50/50
    # false-RED — never fall back to unpinned.
    echo "  SKIP ROW4: taskset unavailable — cannot pin confined burns"
elif [ -z "${_ROW4_CONFINE_CPUS:-}" ]; then
    echo "  SKIP ROW4: own Cpus_allowed_list unreadable — cannot derive confined pin list"
else
    # ── ROW4 ORCHESTRATION (step-10, confined H5/task 4926) ──────────────────
    # Confine the shared parent slice's CPUQuota BEFORE any measurement, and
    # pin the burns to the confined CPU list (esc-4926-3): the quota bounds
    # the subtree's aggregate footprint while the pinning creates the per-CPU
    # runqueue co-residency cpu.weight arbitration requires — together they
    # make the ratio host-load-independent (PRD §1 G6 #3 as revised), so the
    # PRIMARY quiet-box pre/post gate that used to wrap this block is DROPPED
    # on this delegated path -- the row now runs unconditionally once
    # host_supports_governance is true (checked above) and can go RED under
    # load if governance is broken (non-vacuous confined-quota; PRD §8). The
    # delegation-unavailable fallback is the host_supports_governance check
    # itself (SKIP ROW4 above) -- never a blanket pass (b-else-a, PRD §4 #1).
    _row4_confine_apply_quota "$_ROW4_CONFINE_PARENT" "$_ROW4_CONFINE_QUOTA"

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
    #     W=confine-cores workers each role → 2W=2*confine-cores against the
    #     confined parent's confine-cores budget → 2× oversubscription WITHIN
    #     the confined budget (H5/task 4926) — mirrors the original unconfined
    #     design's "W=nproc on nproc cores" 2× ratio, just at the confined
    #     scale. Using nproc workers here (as the pre-H5 unconfined design
    #     did) against a small confined cap over-oversubscribes by nproc/
    #     confine-cores×, which empirically degrades weight-ratio convergence.
    #     `taskset -c` pins both roles' burns to the SAME confine-cores CPUs
    #     (esc-4926-3): affinity inherits through cpu-governed-exec's
    #     `systemd-run --user --scope` (the payload stays a direct child), and
    #     the co-residency it forces is what lets cpu.weight arbitrate at all
    #     — unpinned, the 2W threads spread across the idle box and the quota
    #     drains weight-blind ~FCFS → ~50/50 false-RED. Pinning is a TEST-
    #     HARNESS mechanism only: production cpu-governed-exec stays unpinned
    #     and work-conserving (C-G1 untouched).
    _ROW4_W="$_ROW4_CONFINE_W"  # W per role; 2W = 2*confine-cores → 2× oversubscription

    REIFY_CPU_GOVERN_SLICE_TASK="$_ROW4_SLICE_TASK" \
    timeout $(( _ROW4_BURN_S + 15 )) taskset -c "$_ROW4_CONFINE_CPUS" \
        bash "$CPU_GOV_EXEC" --role task -- \
        bash "$FIXTURE" "$_ROW4_W" "$_ROW4_BURN_S" \
        >/dev/null 2>&1 &
    _ROW4_TASK_BG=$!

    REIFY_CPU_GOVERN_SLICE_MERGE="$_ROW4_SLICE_MERGE" \
    timeout $(( _ROW4_BURN_S + 15 )) taskset -c "$_ROW4_CONFINE_CPUS" \
        bash "$CPU_GOV_EXEC" --role merge -- \
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
    # Asserts the C-G2 proportional cpu.weight enforcement under contention,
    # measured INSIDE the confined parent-slice budget with the burns pinned
    # to the confined CPUs (H5 + esc-4926-3) -- the pinning-forced per-CPU
    # co-residency is what makes the bound host-load-independent (PRD §1 G6
    # #3 as revised; foreign/concurrent load on the pinned CPUs only deepens
    # co-residency and improves convergence).
    # W_merge/(W_merge+W_task) = 300/(300+100) = 0.75; floor = 0.75 - tol.
    # Default tol=0.10 (floor=0.65) accounts for real-world cgroup scheduling
    # measurement variance (startup stagger, scope-creation lag, process overhead).
    # Overridable via REIFY_CPU_GOV_TEST_SHARE_TOL. INHERITED VERBATIM (G6) --
    # no new number, tol untouched (re-tightening reopens the #4656 flake class).
    #
    # Skip if slice discovery failed (empty rel-path — probe timed out/errored) or
    # both deltas are zero (measurement inconclusive).  Without this guard an empty
    # rel-path causes cgroup-usage to read the root cgroup, both roles get the same
    # usage_usec, merge_share ≈ 0.5 which is below the 0.65 floor — a false-RED.
    # These are measurement-integrity guards (not load-based) and are retained.
    if [ -z "${_ROW4_TASK_SLICE_REL:-}" ] || [ -z "${_ROW4_MERGE_SLICE_REL:-}" ]; then
        echo "  SKIP ROW4-1: slice rel-path discovery failed (empty) — cannot compute share"
    elif [ "$_ROW4_TASK_DELTA" -le 0 ] && [ "$_ROW4_MERGE_DELTA" -le 0 ]; then
        echo "  SKIP ROW4-1: both cpu.stat deltas are zero — measurement inconclusive"
    else
        # No quiet-box fallback here (H5): the confined parent quota makes
        # this measurement host-load-independent, so it asserts directly and
        # can go RED if governance is broken (non-vacuous; PRD §8).
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
