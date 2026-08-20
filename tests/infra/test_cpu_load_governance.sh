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
#   Row 2  heavy mix → after warm-up avg10 < AGENT_THRESHOLD (in-band  host-gated
#          avg10 SKIPs as inconclusive or hard FAILs — three-band note below)
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
# ROW2-1 three-band decision (task 4970, esc-4959-53): the TASK SLICE's own
# avg10 sample (after warm-up) splits into three bands instead of a single
# threshold:
#   avg10 <  AGENT_THRESHOLD (50)   → PASS (governance holds).
#   AGENT_THRESHOLD <= avg10 < 90   → SKIP-inconclusive IFF the slice is
#                                     STARVED (usage_fraction <
#                                     REIFY_CPU_GOV_TEST_ROW1_SATURATION_FLOOR)
#                                     AND windowed-stall-contended
#                                     (_row1_stall_contended) — i.e. foreign
#                                     load co-resident on the pinned CPUs.
#                                     Otherwise the slice is SATURATING its
#                                     own budget → hard FAIL (genuine
#                                     quiet-box governance regression; the
#                                     non-vacuity crux — see
#                                     _row2_band_inconclusive below).
#   avg10 >= 90                     → SKIP contention-inflated (unchanged).
#   non-numeric sample              → SKIP (unchanged).
# The band discriminator deliberately reuses ROW1-1's usage-fraction
# saturation machinery and _row1_stall_contended rather than gating on a
# slice-relative stall signal alone: avg10 (some.pressure) and the windowed
# some.total stall-fraction are the SAME signal at the SAME cgroup scope, so
# gating purely on stall would make the ENTIRE band SKIP unconditionally
# (vacuous — forbidden). No new host-baked constant is introduced: the band
# edges (50/90) and the saturation floor (0.85) are the existing knobs,
# just read together.
#
# ROW3-1 measurement-usability + high-direction hedge (task 5999): the
# slowdown = T_mix/T_base ratio's live T_base capture used to collapse a
# timed-out/errored baseline probe to a literal "1" via `|| echo "1"`,
# manufacturing an inflated slowdown on a busy host and hard-FAILing what
# was really a passing measurement — the same flake class as #4656/#4967/
# #4970/#5998 (a real, transient host condition misread as a genuine
# governance break). Two structural fixes, both SKIP-direction only — neither
# ever converts a would-be PASS into a SKIP, nor loosens the anti-runaway
# (#4415) hard assert on a genuinely quiet box:
#   - _row3_probe_sample takes the probe's EXIT STATUS as an explicit
#     parameter, not just its stdout, because stdout ALONE cannot
#     discriminate an errored probe that still printed a plausible number
#     from a genuine 1-second measurement — the exit status is the only
#     honest discriminator. An unusable T_base or T_mix
#     (_row3_measurement_unusable) SKIPs rather than manufacturing a ratio
#     from noise.
#   - _row3_slowdown_inconclusive SKIPs a bound-breaching slowdown IFF
#     _row3_foreign_load reports a GENUINE foreign-load reading — avg10 >= 90
#     from an actual PSI read (not the "99" default an unreadable PSI file
#     produces) AND the slice is starved rather than self-inflicting the
#     load (review-amendment: raw avg10 >= 90 alone is neither immune to an
#     unreadable-PSI false positive nor independent of the over-admission
#     failure this row exists to catch). The bound-breach + contended
#     conjunction itself is the same shape ROW2-2 already uses, not a new
#     policy. A breach on an UNCONTENDED (quiet) slice still hard-FAILs, so
#     #4415 cannot recur.
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
#   REIFY_CPU_GOV_TEST_QUIET_CEILING    host-wide PSI avg10 ceiling above which the
#                                       box counts as HOT (default 20). READ AGAIN by
#                                       ROW4-1 as of task 5998 (this entry previously
#                                       declared it UNUSED on the H5/#4926 premise
#                                       that confined-quota + pinning made ROW4-1
#                                       host-load-independent by construction —
#                                       measurement falsified that: the quota bounds
#                                       the AGGREGATE, not the SPLIT between the two
#                                       weighted children, so foreign load on the
#                                       pinned CPUs still dilutes proportionality.
#                                       See the ROW4-1-QUIET-VACUITY block below).
#                                       Raising it cannot make ROW4-1 unfailable:
#                                       it gates ONLY the corridor between the
#                                       weights-ignored fair share (widened by
#                                       _ROW4_TOL) and the proportional floor —
#                                       (0.60, 0.65) at the defaults — so a share
#                                       at or below fair share + tol still goes RED
#                                       at any ceiling (ROW4-1-CORRIDOR-VACUITY-1,
#                                       and -3/-3b pin that edge to one microsecond).
#                                       ROW1/ROW2_3 do not read it
#   REIFY_CPU_GOV_TEST_ROW1_WARMUP_S    ROW1-1 steady-state ramp before sampling
#                                       (default 1)
#   REIFY_CPU_GOV_TEST_ROW1_MEASURE_S   ROW1-1 steady-state delta window (default 3)
#   REIFY_CPU_GOV_TEST_ROW1_SATURATION_FLOOR  ROW1-1 saturation floor as a fraction
#                                       of the confine-cores*measure_s budget
#                                       (default 0.85; empirically calibrated, H5).
#                                       Reused (read inverted) as the ROW2-1
#                                       band's starvation floor (task 4970):
#                                       usage_fraction < floor => starved.
#   REIFY_CPU_GOV_TEST_ROW1_STALL_SKIP_FRACTION  ROW1-1 measurement-integrity SKIP
#                                       threshold (default 0.5): a windowed delta of
#                                       the task slice's OWN cpu.pressure `some.total`
#                                       (cumulative stall usec), bracketed over the
#                                       SAME measure window as the usage_usec delta,
#                                       as a fraction of that window. Supersedes a
#                                       single post-hoc avg10>=10 read — avg10 is a
#                                       10s-decayed moving average that systematically
#                                       under-reports contention accrued in the ~3s
#                                       measure window (esc-4031-154 / task 4967); the
#                                       windowed some.total delta is exact and
#                                       non-decaying instead. Reused as-is (via
#                                       _row1_stall_contended, no duplication) by
#                                       ROW2-1's band decision (task 4970).
#   REIFY_CPU_GOV_TEST_ROW1_INACTIVE_FRACTION  ROW1-1 measurement-integrity SKIP
#                                       threshold (default 0.02): fires when BOTH the
#                                       usage fraction and the windowed stall fraction
#                                       fall below this floor -- the confined scope
#                                       shows neither meaningful usage NOR meaningful
#                                       stall, meaning the burn never joined its
#                                       governed scope within warmup+measure at all
#                                       (systemd-run/DBus scope-creation and fork/exec
#                                       latency can exceed the window under extreme
#                                       load). Distinct from the contention SKIP above:
#                                       a source that is running-and-contended shows
#                                       HIGH stall; a source that never joined the
#                                       cgroup shows near-zero stall too, since a task
#                                       that does not exist yet is neither running nor
#                                       runnable-but-stalled (task 4967 follow-up,
#                                       esc-4031-154 residual: Δusage=2081usec on a
#                                       ~200+ load-avg host, stall_fraction=0.001).
#   REIFY_CPU_GOV_TEST_PROC_PATH        synthetic-PSI injection seam (testability seam —
#                                       mirrors REIFY_CPU_ADMIT_PROC_PATH used in ROW4-BYPASS)
#   REIFY_CPU_GOV_TEST_BURN_S           per-fixture burn duration seconds (default 4;
#                                       ROW4 default warmup+measure+4 if unset)
#   REIFY_CPU_GOV_TEST_ROW4_WARMUP_S    ROW4 steady-state ramp before sampling (default 3)
#   REIFY_CPU_GOV_TEST_ROW4_MEASURE_S   ROW4 steady-state delta window (default 8)
#   REIFY_CPU_GOV_TEST_ROW4_BYPASS_SLOW_S  ROW4-BYPASS-VACUITY-2 slow-but-
#                                       correct stub sleep seconds (default 6;
#                                       strictly ABOVE the retired 5 s bound,
#                                       proving a bypass merely slow to START
#                                       does not flip the verdict; task 6000).
#                                       Must stay strictly below
#                                       REIFY_CPU_GOV_TEST_ROW4_BYPASS_TIMEOUT_S
#                                       (below) or ROW4-BYPASS-VACUITY-2a fails
#                                       by construction. This default is
#                                       coupled to the inline rationale at its
#                                       resolution site (search
#                                       _ROW4_BYPASS_SLOW_S below) -- move
#                                       both together if it ever changes.
#   REIFY_CPU_GOV_TEST_ROW4_BYPASS_TIMEOUT_S  ROW4-2/ROW4-3 anti-hang guard,
#                                       never a discriminator (default 120;
#                                       task 6000 T-treatment). The
#                                       ROW4-BYPASS-VACUITY-2d >=60s floor
#                                       check applies ONLY to this built-in
#                                       default: override it (e.g. a tighter
#                                       CI budget) and 2d SKIPs rather than
#                                       FAILs, since the floor regression-pins
#                                       the default, not your override. Gated
#                                       by _row4_bypass_floor_applies (grep
#                                       doc -> code).
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
CLASSIFICATION_LIB="$SCRIPT_DIR/run-all-classification-lib.sh"
LOAD_TOLERANCE_LIB="$SCRIPT_DIR/load_tolerance_lib.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

# quiet_box_met (task 5998): ROW4-1's quiet-box escape hatch delegates its
# hot/quiet decision to this shared helper rather than re-deriving one — see
# _row4_share_inconclusive below.  The helper's own docstring has claimed ROW4
# as its consumer since #4656; this source makes that true again.
#
# MANDATORY, not degradable — same idiom as test_helpers.sh directly above,
# deliberately NOT the optional LIB_CGROUP shape.  A degrading `else` arm was
# tried and removed: there is no stub that both keeps the ROW4-1-QUIET-VACUITY
# asserts honest AND lets the file finish green, because those asserts exist
# precisely to pin quiet_box_met's real fail-open semantics.  A stub that
# always reports quiet sends three of them RED; one that reports hot inverts
# the guard into fail-MASKING.  So the lib's absence is reported ONCE, clearly,
# instead of as three misleading corridor-predicate failures.  (The lib is a
# tracked sibling in this directory and an implicit closure member of every
# pool run — run-all-skip-closures.manifest #5 — so this is a broken-checkout
# assertion, not a runtime-degradation path.)
[ -f "$LOAD_TOLERANCE_LIB" ] || {
    echo "ERROR: load_tolerance_lib.sh not found at $LOAD_TOLERANCE_LIB" >&2
    exit 1
}
# shellcheck source=tests/infra/load_tolerance_lib.sh
source "$LOAD_TOLERANCE_LIB"

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

    # SELF-6 (task 4967 / esc-4031-154): psi-some-total parses the `some`-line
    # `total=` field (cumulative CPU-stall usec) from a PSI-formatted file —
    # the windowed-delta counter ROW1-1 samples before/after its measure
    # window, replacing the single post-hoc avg10 read that under-reported
    # short-window contention. Mirrors SELF-4's synthetic-fixture idiom.
    _SELF6_STALL_FIXTURE="$(mktemp -p "$WORK" self6-stall.XXXXXX)"
    printf 'some avg10=0.00 avg60=0.00 avg300=0.00 total=123456\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
        > "$_SELF6_STALL_FIXTURE"
    assert "SELF-6a: cpu_gov_instrument.py psi-some-total <synthetic-fixture> == 123456" \
        bash -c '
            out=$(python3 "$1" psi-some-total "$2" 2>/dev/null)
            [ "$out" = "123456" ]
        ' _ "$INSTRUMENT" "$_SELF6_STALL_FIXTURE"

    _SELF6_MALFORMED_FIXTURE="$(mktemp -p "$WORK" self6-malformed.XXXXXX)"
    printf 'some avg10=0.00 avg60=0.00 avg300=0.00\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
        > "$_SELF6_MALFORMED_FIXTURE"
    assert "SELF-6b: cpu_gov_instrument.py psi-some-total <malformed/missing-total fixture> == unavailable" \
        bash -c '
            out=$(python3 "$1" psi-some-total "$2" 2>/dev/null)
            [ "$out" = "unavailable" ]
        ' _ "$INSTRUMENT" "$_SELF6_MALFORMED_FIXTURE"
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
# Measurement-inactive floor (task 4967 follow-up / esc-4031-154 residual):
# derived from the observed never-joined-scope residual (~0.001-0.003) plus
# margin — see _row1_measurement_inactive below and the header knob doc.
_ROW1_INACTIVE_FRACTION="${REIFY_CPU_GOV_TEST_ROW1_INACTIVE_FRACTION:-0.02}"

# _row1_stall_contended <total_before> <total_after> <window_us>
#   Windowed-stall measurement-integrity predicate (task 4967 / esc-4031-154):
#   delegates to cpu_gov_instrument.py's stall_fraction_contended, binding the
#   REIFY_CPU_GOV_TEST_ROW1_STALL_SKIP_FRACTION knob (default 0.5) read fresh
#   at call time (mirrors the _row4_confine_* knob-reading idiom, so a
#   per-call env override — e.g. a future faked-knob check in the style of
#   CONFINE-2b — takes effect deterministically). Returns 0 (contended ->
#   caller should SKIP) iff the windowed some.total delta as a fraction of
#   window_us is >= the threshold; 1 (not contended) otherwise. A
#   non-integer/"unavailable" sample (e.g. cpu.pressure unreadable) returns 1
#   without invoking python3 — parity with the old avg10-unavailable branch,
#   so an unreadable-pressure edge never masks a genuine governance break.
_row1_stall_contended() {
    local before="$1" after="$2" window_us="$3"
    local threshold="${REIFY_CPU_GOV_TEST_ROW1_STALL_SKIP_FRACTION:-0.5}"
    case "$before" in ''|*[!0-9]*) return 1 ;; esac
    case "$after" in ''|*[!0-9]*) return 1 ;; esac
    python3 -c "
import sys
sys.path.insert(0, '${SCRIPT_DIR}')
from cpu_gov_instrument import stall_fraction_contended
ok = stall_fraction_contended(${before}, ${after}, ${window_us}, ${threshold})
sys.exit(0 if ok else 1)
" 2>/dev/null
}

# _row1_measurement_inactive <usage_fraction> <stall_fraction> <floor>
#   Measurement-integrity predicate distinct from _row1_stall_contended above
#   (task 4967 follow-up / esc-4031-154 residual): detects a confined scope
#   that shows NEITHER meaningful usage NOR meaningful stall during the
#   measurement window, i.e. the burn never joined its governed scope at all
#   within warmup+measure. cpu-governed-exec.sh's own scope-creation
#   (two systemd-run/DBus round-trips) plus the fixture's fork/exec can, under
#   sufficiently extreme host load, take longer than the whole warmup+measure
#   window — the confined workers are still stuck in an ANCESTOR cgroup's
#   scheduling queue when BEFORE/AFTER are sampled, so the target slice's own
#   usage_usec and cpu.pressure some.total both stay near-zero. This is the
#   opposite failure shape from genuine contention (_row1_stall_contended):
#   a task that is running-and-contended accrues HIGH stall; a task that does
#   not exist yet in this cgroup accrues NEAR-ZERO stall too, because it is
#   neither running nor runnable-but-stalled. Pure awk, no I/O, mirrors the
#   ROW1-1-VACUITY-1/2 inline-awk idiom (simple threshold comparison, unlike
#   the windowed-delta/counter-wrap arithmetic that justified a python
#   analyzer for _row1_stall_contended). Returns 0 (inactive -> caller should
#   SKIP) iff BOTH fractions are numeric and strictly below floor; 1 (not
#   inactive -> proceed) if stall_fraction is "unavailable" or either
#   fraction is >= floor — an unreadable sample never masks a genuine break,
#   parity with _row1_stall_contended's unavailable handling.
_row1_measurement_inactive() {
    local usage_fraction="$1" stall_fraction="$2" floor="$3"
    case "$stall_fraction" in unavailable) return 1 ;; esac
    awk -v u="$usage_fraction" -v s="$stall_fraction" -v f="$floor" \
        'BEGIN{ exit !((u+0) < (f+0) && (s+0) < (f+0)) }'
}

# _row2_band_inconclusive <avg10> <threshold> <usage_fraction> <stall_before>
#   <stall_after> <window_us>
#   ROW2-1 band-decision predicate (task 4970, esc-4959-53): decides whether
#   an in-band [threshold, 90) avg10 sample should SKIP as inconclusive
#   (foreign load co-resident on the pinned CPUs) rather than fall through to
#   the caller's existing hard FAIL. Reads the starvation floor internally
#   from REIFY_CPU_GOV_TEST_ROW1_SATURATION_FLOOR (default 0.85 — the same
#   knob ROW1-1 uses for its saturation check, read INVERTED here: a slice
#   usage_fraction below the floor means the slice is STARVED rather than
#   saturating its own confine-cores budget), mirroring how
#   _row1_stall_contended reads its own threshold knob fresh at call time.
#   Returns 0 (inconclusive -> caller SKIPs) IFF ALL hold: avg10 is numeric
#   AND threshold <= avg10 < 90 (in-band; the < 90 keeps this predicate
#   self-contained even though the caller's own >=90 branch already guards
#   it) AND usage_fraction is numeric AND usage_fraction < floor (slice
#   STARVED, not saturating its own budget — a genuine quiet-box regression
#   would instead saturate it, the non-vacuity crux) AND
#   _row1_stall_contended reports the windowed some.total stall as contended.
#   Returns 1 (NOT inconclusive -> caller's hard assert stays reachable)
#   otherwise, including any non-numeric/"unavailable" avg10 or
#   usage_fraction — fail-safe, parity with _row1_stall_contended's /
#   _row1_measurement_inactive's own unavailable handling: an unreadable
#   sample must never mask a genuine break.
_row2_band_inconclusive() {
    local avg10="$1" threshold="$2" usage_fraction="$3"
    local stall_before="$4" stall_after="$5" window_us="$6"
    local floor="${REIFY_CPU_GOV_TEST_ROW1_SATURATION_FLOOR:-0.85}"

    case "$avg10" in ''|*[!0-9.]*) return 1 ;; esac
    case "$usage_fraction" in ''|*[!0-9.]*) return 1 ;; esac

    awk -v a="$avg10" -v t="$threshold" \
        'BEGIN{ exit !((a+0) >= (t+0) && (a+0) < 90) }' || return 1

    awk -v u="$usage_fraction" -v f="$floor" \
        'BEGIN{ exit !((u+0) < (f+0)) }' || return 1

    _row1_stall_contended "$stall_before" "$stall_after" "$window_us"
}

# _row2_usage_fraction <usage_before> <usage_after> <budget_us>
#   Pure usage-fraction helper for ROW2-1's band decision (task 4970 review-
#   amendment, reviewer_comprehensive/test_masks_regression): computes the
#   task slice's own usage delta as a fraction of budget_us, OR propagates
#   the literal "unavailable" sentinel when EITHER bracket is unreadable —
#   mirrors the _ROW1_STALL_FRACTION unavailable-guard idiom above (lines
#   909-914). Extracted so the WIRED ROW2_3 caller can no longer collapse an
#   unreadable usage_usec bracket to a numeric "0.000000" (which reads as
#   maximally STARVED and, combined with an in-band avg10 + high windowed
#   stall, would wrongly SKIP a genuine over-admission governance
#   regression instead of hard-FAILing it — the review-found masking gap).
#   _row2_band_inconclusive's own non-numeric guard
#   (case ''|*[!0-9.]*) => return 1) then fails safe on the propagated
#   sentinel exactly as it already does on a literal "unavailable" input.
#   Guards counter-wrap (after < before -> delta 0) and non-positive budget
#   (-> "0"), same degenerate-guard discipline as the ROW1-1 usage-fraction
#   awk idiom (lines 897-903).
_row2_usage_fraction() {
    local before="$1" after="$2" budget="$3"
    case "$before" in unavailable) echo unavailable; return 0 ;; esac
    case "$after" in unavailable) echo unavailable; return 0 ;; esac
    local delta=$(( after - before ))
    [ "$delta" -lt 0 ] && delta=0  # guard counter wrap
    awk -v d="$delta" -v b="$budget" \
        'BEGIN{ if (b+0<=0) {print "0"} else {printf "%.6f", d/b} }'
}

# _row3_sample_usable <value>
#   Shared numeric-usability predicate (task 5999 review-amendment,
#   reviewer_comprehensive/duplication): a value is usable iff it is numeric
#   (float-permissive, file's `case ''|*[!0-9.]*)` idiom) AND strictly
#   positive by awk. Extracted so _row3_probe_sample and
#   _row3_measurement_unusable cannot silently drift apart on what counts as
#   a usable sample — two independently-editable copies of the identical
#   case+awk triad is exactly the G7 no-lockstep-duplication shape
#   _row3_within_bound was already extracted below to avoid. The literal
#   "unavailable" sentinel needs no separate case: it already fails the
#   numeric guard. Returns 0 (usable) / 1 (not usable).
_row3_sample_usable() {
    local v="$1"
    case "$v" in ''|*[!0-9.]*) return 1 ;; esac
    awk -v x="$v" 'BEGIN{ exit !((x+0) > 0) }'
}

# _row3_probe_sample <rc> <raw>
#   Probe-sample normalizer for ROW3-1 (task 5999, #5999 false-RED): the
#   live T_base capture used to collapse a timed-out (rc 124) or errored
#   (rc != 0) probe to a literal "1" via `|| echo "1"` plus a rescue
#   `[ -z "${_T_BASE}" ] || [ "${_T_BASE}" = "0" ] && _T_BASE="1"` — which
#   reads as a legitimate 1.000000-second baseline and, divided into a real
#   T_mix, manufactures an inflated slowdown ratio (the false RED this task
#   fixes). Mirrors _row2_usage_fraction's sentinel-propagation shape above:
#   an unreadable/untrustworthy sample must propagate the literal
#   "unavailable" sentinel rather than collapse to a numeric that reads as
#   legitimate. Despite the historical name, this normalizer is probe-
#   agnostic (review-amendment: the T_mix capture had the SAME
#   stdout-only-collapse defect, asymmetrically left unfixed — see the T_mix
#   live capture below), so it is called for BOTH T_base and T_mix.
#
#   Takes the probe's EXIT STATUS as an explicit first parameter because
#   stdout ALONE cannot discriminate an errored probe that still printed a
#   plausible number (e.g. a partial "1.000000") from a genuine measurement
#   — the exit status is the only honest discriminator.
#
#   Echoes "unavailable" when: rc is non-zero or non-numeric; OR raw fails
#   _row3_sample_usable (empty/non-numeric/<= 0 — a degenerate divisor or
#   probe-work-time can never be a real wall-clock measurement). Otherwise
#   echoes raw verbatim.
_row3_probe_sample() {
    local rc="$1" raw="$2"
    case "$rc" in ''|*[!0-9]*) echo unavailable; return 0 ;; esac
    [ "$rc" -ne 0 ] && { echo unavailable; return 0; }
    _row3_sample_usable "$raw" || { echo unavailable; return 0; }
    echo "$raw"
}

# _row3_measurement_unusable <t_base> <t_mix>
#   ROW3-1 measurement-usability predicate (task 5999): both T_base and
#   T_mix must be usable numeric samples (_row3_sample_usable) before the
#   slowdown ratio (T_mix/T_base) means anything. Subsumes the pre-existing
#   inline `awk -v m="${_T_MIX:-0}" 'BEGIN{exit !(m+0 <= 0)}'` T_mix-only
#   hatch and closes the missing T_base arm — the #5999 defect WAS this
#   asymmetry: T_mix got a guard (`|| echo "0"` plus this hatch), T_base got
#   an ungarded `|| echo "1"` and no hatch at all. Both arguments run
#   through the SAME loop AND the SAME usability predicate _row3_probe_sample
#   itself uses on its raw value, so the symmetry is structural on two axes
#   (T_base/T_mix, and capture-time normalization/here) rather than
#   conventional.
#
#   Returns 0 (unusable -> caller SKIPs) iff EITHER argument fails
#   _row3_sample_usable — which already covers the literal "unavailable"
#   sentinel (it fails that predicate's numeric guard), empty/non-numeric,
#   and <= 0 (a degenerate/negative divisor or probe-work-time can never be
#   real). Returns 1 (usable -> the hard assert stays reachable) only when
#   BOTH arguments pass.
#
#   Fails SAFE toward SKIP (rc 0) on a non-numeric/unavailable input — the
#   OPPOSITE direction from _row1_stall_contended/_row1_measurement_inactive/
#   _row2_band_inconclusive, which all fail toward "assert stays reachable"
#   because there an unreadable sample could otherwise MASK a genuine break.
#   Here T_base is the DIVISOR of the slowdown ratio: an unreadable divisor
#   cannot produce evidence of anything, only noise, so continuing to the
#   assert would manufacture a verdict rather than preserve one.
_row3_measurement_unusable() {
    local t_base="$1" t_mix="$2"
    local v
    for v in "$t_base" "$t_mix"; do
        _row3_sample_usable "$v" || return 0
    done
    return 1
}

# _row3_within_bound <slowdown> <k> <floor>
#   ROW3-1's bound predicate (task 5999): the SINGLE source of truth for "is
#   this slowdown acceptable", encoding exactly what the inline
#   `python3 -c` ROW3-1 assert used to evaluate: s <= k*floor AND s < 10.0.
#   Extracting it here — and reusing it from the step-9 high-direction
#   hedge — means the assert and the hedge cannot silently diverge the
#   moment either bound is retuned (G7 no-lockstep-duplication); mirrors
#   #5998's share_ge_proportional extraction (6b3c6c2879).
#
#   The 10.0 absolute ceiling is kept as a documented in-helper constant, NOT
#   a new env knob: it already existed as an inline literal in the ROW3-1
#   assert, so lifting it in here is a MOVE, not a new host-baked tunable
#   (the file header's G6 CRUX forbids the latter). It independently bounds
#   a runaway slowdown even when a large floor would otherwise satisfy the
#   proportional K*floor clause (ROW3-1-BOUND-VACUITY-4/5 non-vacuity cases).
#
#   Returns 0 (within bound) iff slowdown is numeric AND s <= k*floor AND
#   s < 10.0. Returns 1 (breached / NOT within) on a non-numeric slowdown —
#   fail-safe, so an unreadable measurement can never be laundered into a
#   PASS.
_row3_within_bound() {
    local slowdown="$1" k="$2" floor="$3"
    # Absolute anti-runaway ceiling (4415 cannot recur) — moved verbatim
    # from the old inline `python3 -c` copy, not a new tunable.
    local ceiling=10.0
    case "$slowdown" in ''|*[!0-9.]*) return 1 ;; esac
    awk -v s="$slowdown" -v k="$k" -v f="$floor" -v c="$ceiling" \
        'BEGIN{ exit !((s+0) <= (k+0)*(f+0) && (s+0) < (c+0)) }'
}

# _row3_slowdown_inconclusive <slowdown> <k> <floor> <contended>
#   ROW3-1's high-direction hedge (task 5999): the row currently has NO hatch
#   at all in the high-slowdown direction, which is exactly the direction a
#   busy host pushes it (the #5999 false RED). COMPOSED from
#   _row3_within_bound above — never a second copy of the bound — so the
#   hedge SKIPs in exactly the cases the assert would otherwise FAIL
#   (G7 no-lockstep-duplication; mirrors #5998's share_ge_proportional
#   extraction, 6b3c6c2879).
#
#   Returns 0 (inconclusive -> caller SKIPs) iff the slowdown is NOT within
#   _row3_within_bound's bound AND contended="1". Returns 1 (not
#   inconclusive -> the hard assert stays reachable) otherwise — including
#   when the slowdown IS within bound (a hedge must never suppress a green,
#   mirrors 2908c04db0 test(5998): ROW4-1-QUIET-VACUITY-3) and when the
#   slowdown breaches the bound but contended != "1" (a runaway slowdown on
#   a quiet box must still hard-FAIL, so the #4415 regression this row
#   exists to catch cannot recur).
#
#   The conjunction (breach AND contended) is not a new policy: ROW2-2 (see
#   the ROW2-2 assert below) already SKIPs on precisely "sub-90% completion
#   AND _ROW23_CONTENDED" — this hedge follows the same precedent. contended
#   is a plain 0/1 flag; the live caller passes _row3_foreign_load's
#   refined verdict below rather than _ROW23_CONTENDED directly (review-
#   amendment: raw _ROW23_CONTENDED alone is neither immune to an
#   unreadable-PSI false positive nor independent of this row's own failure
#   mode — see _row3_foreign_load's docstring) — no new sample, threshold,
#   or env knob is introduced beyond what _row3_foreign_load documents.
#
#   Fails SAFE toward "hard assert stays reachable" (rc 1) on a non-numeric
#   slowdown — the SAME direction as _row3_within_bound's own fail-safe
#   (deliberately the OPPOSITE direction from _row3_measurement_unusable,
#   which guards the slowdown ratio's divisor rather than the ratio itself).
#   Without this explicit guard, composing through _row3_within_bound alone
#   would flip an unreadable slowdown into a false "breached" signal and
#   this hedge would SKIP it — swallowing the very case that must surface as
#   a hard, visible FAIL at the assert below instead (never masked).
_row3_slowdown_inconclusive() {
    local slowdown="$1" k="$2" floor="$3" contended="$4"
    [ "$contended" = "1" ] || return 1
    case "$slowdown" in ''|*[!0-9.]*) return 1 ;; esac
    if _row3_within_bound "$slowdown" "$k" "$floor"; then
        return 1
    else
        return 0
    fi
}

# _row3_foreign_load <contended> <avg10_rc> <usage_fraction>
#   ROW3-1 review-amendment (task 5999, reviewer_comprehensive/robustness +
#   /test-quality): refines the raw _ROW23_CONTENDED signal before it may
#   license _row3_slowdown_inconclusive's SKIP. Reads the starvation floor
#   internally from REIFY_CPU_GOV_TEST_ROW1_SATURATION_FLOOR (default 0.85)
#   — the SAME knob _row2_band_inconclusive already reads for the identical
#   starved-vs-saturating distinction — fresh at call time.
#
#   Closes two gaps a genuine over-admission regression (#4415) could
#   otherwise exploit to launder itself into a green ROW3-1 run:
#     1. avg10_rc must be "0" (a genuine PSI read): _ROW23_CONTENDED is "1"
#        whenever avg10 >= 90, but the live capture defaults avg10 to "99"
#        when the PSI read itself FAILS (an unreadable cpu.pressure file,
#        not genuine contention) — the same "unreadable sample collapsing to
#        a numeric that reads as legitimate" defect class _row3_probe_sample
#        exists to eliminate, reintroduced on the contention axis. avg10_rc
#        carries whether the read actually succeeded, so an unreadable PSI
#        file can no longer manufacture contended=1.
#     2. the slice must be STARVED (usage_fraction < floor), mirroring
#        _row2_band_inconclusive's own starved-vs-saturating discriminator:
#        a genuine over-admission regression drives too many governed
#        sources into the SAME task slice, which self-inflicts high avg10 on
#        that slice WHILE the slice keeps consuming its own cpu.pressure
#        budget (NOT starved) — so avg10 alone cannot distinguish foreign
#        load from the exact failure this row exists to catch; a saturating
#        slice must still hard-FAIL.
#
#   Echoes "1" iff contended="1" AND avg10_rc="0" AND usage_fraction is
#   numeric AND usage_fraction < floor; "0" otherwise — including a
#   non-numeric usage_fraction (fail-safe: an unreadable STARVATION signal
#   must not license a SKIP either).
_row3_foreign_load() {
    local contended="$1" avg10_rc="$2" usage_fraction="$3"
    local floor="${REIFY_CPU_GOV_TEST_ROW1_SATURATION_FLOOR:-0.85}"
    [ "$contended" = "1" ] || { echo 0; return 0; }
    [ "$avg10_rc" = "0" ] || { echo 0; return 0; }
    case "$usage_fraction" in ''|*[!0-9.]*) echo 0; return 0 ;; esac
    awk -v u="$usage_fraction" -v f="$floor" 'BEGIN{ exit !((u+0) < (f+0)) }' || { echo 0; return 0; }
    echo 1
}

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

# Non-vacuity guard for the windowed stall-contention SKIP predicate
# (task 4967 / esc-4031-154): a single post-hoc cpu.pressure avg10 read is a
# 10s-decayed moving average sampled against a ~3s measure window, and
# systematically under-reports contention accrued in that short window — the
# exact gap that let esc-4031-154 false-RED (saturation 0.137 while avg10
# stayed < 10). _row1_stall_contended instead windows a `some.total`
# before/after delta over the SAME measure window. High measured stall must
# trigger SKIP; low measured stall must NOT (so a genuine quiet-box
# governance break stays reachable — the non-vacuity crux); a counter-wrap
# sample must fail safe to NOT-contended. Mirrors CONFINE-VACUITY-1/2's
# tight-boundary shape.
if [ "$_PYTHON_AVAILABLE" -eq 0 ]; then
    echo "  SKIP ROW1-1-STALL-VACUITY: python3 not on PATH"
else
    assert "ROW1-1-STALL-VACUITY-1: high stall (before=0 after=900000 window=1000000) => contended (SKIP)" \
        _row1_stall_contended 0 900000 1000000

    _row1_stall_vacuity2_rc=0
    _row1_stall_contended 0 100000 1000000 || _row1_stall_vacuity2_rc=$?
    assert "ROW1-1-STALL-VACUITY-2: low stall (before=0 after=100000 window=1000000) => NOT contended (assertion stays reachable)" \
        test "$_row1_stall_vacuity2_rc" -ne 0

    _row1_stall_vacuity3_rc=0
    _row1_stall_contended 500000 100000 1000000 || _row1_stall_vacuity3_rc=$?
    assert "ROW1-1-STALL-VACUITY-3: counter-wrap (before=500000 after=100000 window=1000000) => NOT contended (safe)" \
        test "$_row1_stall_vacuity3_rc" -ne 0
fi

# Non-vacuity guard for the second, distinct measurement-integrity predicate
# (task 4967 follow-up / esc-4031-154 residual): _row1_stall_contended above
# only catches a source that IS running and IS contended (high stall).
# Under sufficiently extreme host load the burn can instead fail to join its
# governed scope at all within warmup+measure, showing NEITHER meaningful
# usage NOR meaningful stall — the opposite shape, which the stall-SKIP is
# structurally incapable of catching. A synthetic near-zero usage+stall pair
# must be accepted as inactive (SKIP); a genuine sub-saturation governance
# break (meaningful usage, low stall) must NOT be masked (the non-vacuity
# crux); a contended source (low usage, high stall) is the stall-SKIP's job,
# not this one's; an unavailable stall sample must fail safe to NOT-inactive
# (parity with _row1_stall_contended's unavailable handling). Pure awk, no
# cgroup/python3 needed, so always-on — mirrors CONFINE-VACUITY-1/2's
# tight-boundary shape.
assert "ROW1-1-INACTIVE-VACUITY-1: near-zero usage+stall (u=0.001 s=0.001 f=0.02) => inactive (SKIP)" \
    _row1_measurement_inactive 0.001 0.001 0.02

_row1_inactive_vacuity2_rc=0
_row1_measurement_inactive 0.5 0.001 0.02 || _row1_inactive_vacuity2_rc=$?
assert "ROW1-1-INACTIVE-VACUITY-2: genuine sub-saturation (u=0.5 s=0.001 f=0.02) => NOT inactive (assertion stays reachable)" \
    test "$_row1_inactive_vacuity2_rc" -ne 0

_row1_inactive_vacuity3_rc=0
_row1_measurement_inactive 0.001 0.9 0.02 || _row1_inactive_vacuity3_rc=$?
assert "ROW1-1-INACTIVE-VACUITY-3: contended source (u=0.001 s=0.9 f=0.02) => NOT inactive (stall-SKIP's job, not this one's)" \
    test "$_row1_inactive_vacuity3_rc" -ne 0

_row1_inactive_vacuity4_rc=0
_row1_measurement_inactive 0.001 unavailable 0.02 || _row1_inactive_vacuity4_rc=$?
assert "ROW1-1-INACTIVE-VACUITY-4: unavailable stall sample (u=0.001 s=unavailable f=0.02) => NOT inactive (fails safe)" \
    test "$_row1_inactive_vacuity4_rc" -ne 0

# Non-vacuity guard for the NEW ROW2-1 band-decision predicate (task 4970,
# esc-4959-53/esc-4959-57): closes the 50-90 avg10 gap in Cycle ROW2_3's
# ROW2-1 assertion, where an in-band [AGENT_THRESHOLD, 90) avg10 sample falls
# through to a hard FAIL today even when it is caused by foreign load
# co-resident on the pinned CPUs. _row2_band_inconclusive
# <avg10> <threshold> <usage_fraction> <stall_before> <stall_after>
# <window_us> decides SKIP (rc 0, inconclusive) vs. NOT-inconclusive (rc != 0,
# the caller's existing hard assert stays reachable).
#
# A slice-relative stall signal ALONE cannot gate this without making ROW2-1
# vacuous: avg10 (some.pressure) and the windowed some.total stall-fraction
# are the SAME signal at the SAME cgroup scope (avg10 ~= 100*stall-fraction
# at steady state), so avg10>=50 implies windowed stall-fraction gtrsim 0.5
# implies _row1_stall_contended fires across the ENTIRE band -- gating on
# stall alone would make the FAIL branch unreachable. The discriminator that
# separates a genuine quiet-box regression from foreign load is the slice's
# OWN usage-fraction, reusing the existing ROW1-1 saturation floor (default
# 0.85) read inverted: usage_fraction < floor => starved => foreign;
# usage_fraction >= floor => saturating => genuine regression (case 2 below
# is the non-vacuity crux this guards). Gated on python3 (case 1 exercises
# _row1_stall_contended); pure synthetic inputs, no cgroup/PSI needed.
if [ "$_PYTHON_AVAILABLE" -eq 0 ]; then
    echo "  SKIP ROW2-1-BAND-VACUITY: python3 not on PATH"
else
    # (1) FOREIGN / esc-4959-53 replay: in-band avg10=57 (threshold=50),
    # slice STARVED (usage_fraction=0.30 < floor 0.85), windowed stall high
    # (before=0 after=900000 window=1000000) => INCONCLUSIVE (rc 0 => SKIP).
    assert "ROW2-1-BAND-VACUITY-1: FOREIGN replay (avg10=57 threshold=50 usage_fraction=0.30 starved, stall high) => inconclusive (SKIP)" \
        _row2_band_inconclusive 57 50 0.30 0 900000 1000000

    # (2) NON-VACUITY CRUX / quiet-box governance break: SAME in-band avg10
    # and SAME high stall as (1), but the slice's OWN usage is SATURATING
    # (0.95 >= floor 0.85) => a genuine governance regression, NOT foreign
    # load => NOT inconclusive (rc != 0 => the caller's hard assert/FAIL
    # stays reachable). Forbids gating the SKIP on stall/avg10 alone.
    _row2_band_vacuity2_rc=0
    _row2_band_inconclusive 57 50 0.95 0 900000 1000000 || _row2_band_vacuity2_rc=$?
    assert "ROW2-1-BAND-VACUITY-2: NON-VACUITY CRUX -- same in-band avg10+stall but SATURATING usage (0.95 >= floor) => NOT inconclusive (FAIL stays reachable)" \
        test "$_row2_band_vacuity2_rc" -ne 0

    # (3) BELOW BAND: avg10=30 < threshold=50 => not this predicate's job (the
    # caller's own < threshold pass-branch handles it) => NOT inconclusive.
    _row2_band_vacuity3_rc=0
    _row2_band_inconclusive 30 50 0.30 0 900000 1000000 || _row2_band_vacuity3_rc=$?
    assert "ROW2-1-BAND-VACUITY-3: below band (avg10=30 < threshold=50) => NOT inconclusive" \
        test "$_row2_band_vacuity3_rc" -ne 0

    # (4) AT/ABOVE 90: avg10=95 => the existing _ROW23_CONTENDED (>=90) branch's
    # job, not this predicate's => NOT inconclusive. Keeps the predicate
    # self-contained/correct even called without the caller's own >=90 guard.
    _row2_band_vacuity4_rc=0
    _row2_band_inconclusive 95 50 0.30 0 900000 1000000 || _row2_band_vacuity4_rc=$?
    assert "ROW2-1-BAND-VACUITY-4: at/above 90 (avg10=95) => NOT inconclusive (other branch's job)" \
        test "$_row2_band_vacuity4_rc" -ne 0

    # (5) FAIL-SAFE: usage_fraction unavailable (in-band avg10=57) => never
    # mask a genuine break => NOT inconclusive, parity with
    # _row1_stall_contended's/_row1_measurement_inactive's own unavailable
    # handling.
    _row2_band_vacuity5_rc=0
    _row2_band_inconclusive 57 50 unavailable 0 900000 1000000 || _row2_band_vacuity5_rc=$?
    assert "ROW2-1-BAND-VACUITY-5: FAIL-SAFE -- usage_fraction unavailable (avg10=57) => NOT inconclusive (never mask a genuine break)" \
        test "$_row2_band_vacuity5_rc" -ne 0
fi

# Non-vacuity guard for the NEW ROW2-1 usage-fraction helper (task 4970
# review-amendment, reviewer_comprehensive/test_masks_regression): the WIRED
# ROW2_3 caller below used to collapse an unreadable usage_usec bracket to
# _ROW23_USAGE_DELTA=0 => fraction "0.000000", which reads as maximally
# STARVED and, combined with an in-band avg10 + high windowed stall, wrongly
# SKIPped a genuine over-admission governance regression instead of
# hard-FAILing it (ROW2-1-BAND-VACUITY-5 above only proved the PREDICATE
# fails safe on a literal "unavailable" -- it never exercised the WIRED
# computation that used to produce "0.000000" instead). _row2_usage_fraction
# <usage_before> <usage_after> <budget_us> fixes this by PROPAGATING the
# literal "unavailable" sentinel whenever either bracket is unreadable,
# mirroring the _ROW1_STALL_FRACTION unavailable-guard idiom (lines 909-914)
# so _row2_band_inconclusive's own non-numeric guard
# (case ''|*[!0-9.]*) => return 1) fires on the WIRED path too.
#
# Pure shell+awk, NO python needed -- even case (5) below short-circuits
# inside _row2_band_inconclusive's non-numeric guard before it would ever
# reach _row1_stall_contended -- so this block is strictly always-on, wider
# coverage than ROW2-1-BAND-VACUITY above (which is python-gated only
# because ITS case 1 needs a real contended verdict).
#
# Capture idiom: "$(_row2_usage_fraction ... || true)" -- MUST be `|| true`,
# NOT `|| echo unavailable`: an echo-unavailable fallback would make case (1)
# pass even while the helper is undefined and would destroy the RED signal.

# (1) REGRESSION CRUX: before-bracket unavailable => propagate the sentinel,
# never collapse to a numeric (maximally-STARVED-looking) fraction -- the
# exact reviewer-found masking bug.
_row2_uf1="$(_row2_usage_fraction unavailable 500000 1000000 2>/dev/null || true)"
assert "ROW2-1-USAGE-FRACTION-VACUITY-1: before-bracket unavailable => propagates 'unavailable' (never collapses to 0.000000)" \
    test "$_row2_uf1" = "unavailable"

# (2) Symmetric: after-bracket unavailable => propagate.
_row2_uf2="$(_row2_usage_fraction 1000 unavailable 1000000 2>/dev/null || true)"
assert "ROW2-1-USAGE-FRACTION-VACUITY-2: after-bracket unavailable => propagates 'unavailable'" \
    test "$_row2_uf2" = "unavailable"

# (3) Both brackets numeric => a real fraction (Δ=300000/1000000 ≈ 0.30),
# asserted numerically (locale-proof), not a string comparison.
_row2_uf3="$(_row2_usage_fraction 100000 400000 1000000 2>/dev/null || true)"
assert "ROW2-1-USAGE-FRACTION-VACUITY-3: both brackets numeric (Δ=300000/1000000) => ~0.30 (got ${_row2_uf3})" \
    awk -v u="$_row2_uf3" 'BEGIN{ exit !((u+0) > 0.29 && (u+0) < 0.31) }'

# (4) Counter-wrap (after < before) => guarded to 0, not a negative fraction.
_row2_uf4="$(_row2_usage_fraction 400000 100000 1000000 2>/dev/null || true)"
assert "ROW2-1-USAGE-FRACTION-VACUITY-4: counter-wrap (after < before) => 0 (got ${_row2_uf4})" \
    awk -v u="$_row2_uf4" 'BEGIN{ exit !((u+0) == 0) }'

# (5) END-TO-END FAIL-SAFE: drive the propagated 'unavailable' output through
# _row2_band_inconclusive with an in-band avg10 + high windowed stall (the
# exact esc-4959-53 masking shape) and prove the OUTCOME is NOT inconclusive
# -- ROW2-1's hard assert stays reachable instead of being silently SKIPped.
_row2_uf5="$(_row2_usage_fraction unavailable 500000 1000000 2>/dev/null || true)"
_row2_uf_vacuity5_rc=0
_row2_band_inconclusive 57 50 "$_row2_uf5" 0 900000 1000000 || _row2_uf_vacuity5_rc=$?
assert "ROW2-1-USAGE-FRACTION-VACUITY-5: END-TO-END FAIL-SAFE -- propagated 'unavailable' usage_fraction (avg10=57 in-band, stall high) => NOT inconclusive (hard assert stays reachable, never masks a genuine regression)" \
    test "$_row2_uf_vacuity5_rc" -ne 0

# Non-vacuity guard for the NEW ROW3-1 baseline-sample normalizer (task 5999,
# #5999 false-RED): the live T_base capture used to collapse a timed-out or
# errored probe to a literal "1" (`|| echo "1"` plus a
# `[ -z ] || [ = "0" ]` rescue), which reads as a legitimate 1.000000-second
# baseline and, divided into a real T_mix, manufactures an inflated slowdown
# ratio -- the false RED. _row3_probe_sample <rc> <raw> takes the probe's EXIT
# STATUS as an explicit parameter because stdout alone cannot discriminate an
# errored probe that still printed a plausible number from a genuine
# measurement (case 2 below). Mirrors _row2_usage_fraction's
# "unavailable"-sentinel-propagation shape above -- same class of
# defect (an unreadable/untrustworthy sample collapsing to a numeric that
# reads as legitimate), same remedy.
#
# Pure shell+awk, NO python3 needed, so always-on -- same wider-coverage
# rationale as ROW2-1-USAGE-FRACTION-VACUITY above.
#
# Capture idiom: "$(_row3_probe_sample ... || true)" -- MUST be `|| true`, NOT
# `|| echo unavailable`: an echo-unavailable fallback would make every case
# below pass even while the helper is undefined and would destroy the RED
# signal (same prohibition as _row2_usage_fraction's Capture idiom above).

# (1) REGRESSION CRUX / #5999 replay: probe timed out (rc=124, no stdout) =>
# propagate "unavailable", never the old collapse to "1".
_row3_bsv1="$(_row3_probe_sample 124 "" 2>/dev/null || true)"
assert "ROW3-1-BASE-SAMPLE-VACUITY-1: REGRESSION CRUX -- timed-out probe (rc=124, raw='') => 'unavailable' (never the old '1')" \
    test "$_row3_bsv1" = "unavailable"

# (2) Probe errored but still printed a plausible number -- stdout alone
# cannot discriminate this from a genuine 1-second baseline; the EXIT STATUS
# is the only honest discriminator, so rc=1 must still sentinel even though
# raw looks legitimate.
_row3_bsv2="$(_row3_probe_sample 1 "1.000000" 2>/dev/null || true)"
assert "ROW3-1-BASE-SAMPLE-VACUITY-2: errored probe (rc=1) with plausible stdout ('1.000000') => 'unavailable' (a failed probe must never be laundered into a legitimate-looking baseline)" \
    test "$_row3_bsv2" = "unavailable"

# (3) rc=0 but empty stdout => unavailable.
_row3_bsv3="$(_row3_probe_sample 0 "" 2>/dev/null || true)"
assert "ROW3-1-BASE-SAMPLE-VACUITY-3: rc=0 with empty stdout => 'unavailable'" \
    test "$_row3_bsv3" = "unavailable"

# (4) rc=0, degenerate divisor "0.000000" => unavailable.
_row3_bsv4="$(_row3_probe_sample 0 "0.000000" 2>/dev/null || true)"
assert "ROW3-1-BASE-SAMPLE-VACUITY-4: rc=0 with degenerate divisor ('0.000000') => 'unavailable'" \
    test "$_row3_bsv4" = "unavailable"

# (5) rc=0, non-numeric stdout => unavailable.
_row3_bsv5="$(_row3_probe_sample 0 "abc" 2>/dev/null || true)"
assert "ROW3-1-BASE-SAMPLE-VACUITY-5: rc=0 with non-numeric stdout ('abc') => 'unavailable'" \
    test "$_row3_bsv5" = "unavailable"

# (6) NON-VACUITY CRUX: rc=0 with a genuine positive baseline passes through
# verbatim, asserted NUMERICALLY via awk (locale-proof, mirrors
# ROW2-1-USAGE-FRACTION-VACUITY-3) -- sentinelling a GOOD baseline would make
# ROW3-1 SKIP unconditionally, which is vacuous.
_row3_bsv6="$(_row3_probe_sample 0 "2.750000" 2>/dev/null || true)"
assert "ROW3-1-BASE-SAMPLE-VACUITY-6: NON-VACUITY CRUX -- rc=0 with genuine baseline ('2.750000') passes through (got ${_row3_bsv6})" \
    awk -v v="$_row3_bsv6" 'BEGIN{ exit !((v+0) > 2.74 && (v+0) < 2.76) }'

# (7)/(8) task 5999 review-amendment (reviewer_comprehensive/test-coverage):
# the rc guard (case ''|*[!0-9]*)) is not reachable from the only production
# call site, which always assigns rc from a literal "0" or a captured "$?"
# (always numeric 0-255) -- but it was also previously unverified by any
# case here. Pin it directly rather than dropping it: dropping would leave
# `[ "$rc" -ne 0 ]` to run on a raw empty/non-numeric rc, which errors under
# `set -e` instead of returning false.
_row3_bsv7="$(_row3_probe_sample "" "2.0" 2>/dev/null || true)"
assert "ROW3-1-BASE-SAMPLE-VACUITY-7: rc='' (empty, non-numeric) => 'unavailable' even with a plausible raw value" \
    test "$_row3_bsv7" = "unavailable"

_row3_bsv8="$(_row3_probe_sample "abc" "2.0" 2>/dev/null || true)"
assert "ROW3-1-BASE-SAMPLE-VACUITY-8: rc='abc' (non-numeric) => 'unavailable' even with a plausible raw value" \
    test "$_row3_bsv8" = "unavailable"

# Non-vacuity guard for the NEW ROW3-1 measurement-usability predicate (task
# 5999): both T_base and T_mix must be usable numeric samples before ROW3-1's
# slowdown ratio (T_mix/T_base) means anything.
# _row3_measurement_unusable <t_base> <t_mix> subsumes today's inline
# `awk -v m="${_T_MIX:-0}" 'BEGIN{exit !(m+0 <= 0)}'` T_mix-only hatch (case 4
# below is byte-equivalent to it) and closes the missing T_base arm the #5999
# false-RED exploited -- the #5999 defect IS the asymmetry: T_mix got a
# hatch, T_base did not. Treating both probes through ONE predicate makes the
# symmetry structural rather than conventional.
#
# Predicate-rc contract (rc 0 = unusable/inconclusive -> caller SKIPs; rc != 0
# = usable -> the hard assert stays reachable), same convention as
# _row2_band_inconclusive/_row1_stall_contended/_row1_measurement_inactive.
# Fails SAFE toward SKIP (rc 0) on a non-numeric input -- the OPPOSITE
# direction from those predicates' "fail toward assert stays reachable": this
# predicate guards the DIVISOR of the slowdown ratio, and an unreadable
# divisor can only manufacture noise, never evidence of a governance break,
# so continuing to the assert would manufacture a verdict rather than
# preserve one.
#
# Pure shell+awk, NO python3 needed, so always-on -- same wider-coverage
# rationale as ROW2-1-USAGE-FRACTION-VACUITY / ROW3-1-BASE-SAMPLE-VACUITY
# above.

# (1) THE GAP: t_base unavailable, t_mix usable => unusable (SKIP). No guard
# at all exists for this direction today -- the #5999 defect.
assert "ROW3-1-UNUSABLE-VACUITY-1: THE GAP -- t_base=unavailable, t_mix=3.0 => unusable (SKIP; no guard exists for this direction today)" \
    _row3_measurement_unusable unavailable 3.0

# (2) t_base non-numeric.
assert "ROW3-1-UNUSABLE-VACUITY-2: t_base=abc (non-numeric), t_mix=3.0 => unusable (SKIP)" \
    _row3_measurement_unusable abc 3.0

# (3) t_base degenerate divisor.
assert "ROW3-1-UNUSABLE-VACUITY-3: t_base=0 (degenerate divisor), t_mix=3.0 => unusable (SKIP)" \
    _row3_measurement_unusable 0 3.0

# (4) PRESERVES THE EXISTING HATCH: t_mix<=0, byte-equivalent to today's
# inline `awk -v m="${_T_MIX:-0}" 'BEGIN{exit !(m+0 <= 0)}'` T_mix-only hatch.
assert "ROW3-1-UNUSABLE-VACUITY-4: PRESERVES THE EXISTING HATCH -- t_base=2.0, t_mix=0 => unusable (SKIP; byte-equivalent to today's inline T_mix<=0 hatch)" \
    _row3_measurement_unusable 2.0 0

# (5) Symmetric counterpart: t_mix unavailable.
assert "ROW3-1-UNUSABLE-VACUITY-5: t_base=2.0, t_mix=unavailable => unusable (SKIP; symmetric treatment of both probes)" \
    _row3_measurement_unusable 2.0 unavailable

# (6) NON-VACUITY CRUX: both usable => NOT unusable, so ROW3-1's hard assert
# stays reachable.
_row3_muv6_rc=0
_row3_measurement_unusable 3.0 9.0 || _row3_muv6_rc=$?
assert "ROW3-1-UNUSABLE-VACUITY-6: NON-VACUITY CRUX -- t_base=3.0, t_mix=9.0 (both usable) => NOT unusable (ROW3-1's hard assert stays reachable)" \
    test "$_row3_muv6_rc" -ne 0

# (7) END-TO-END COMPOSE / #5999 false-RED replay: feed a timed-out probe
# through the step-2 helper, then into this predicate. The OLD collapse
# produced t_base=1 (usable) => slowdown 9.0/1=9.0 > K*floor=6.0 (K=4,
# floor=1.5 defaults) => the false RED this task fixes; the fix instead SKIPs
# as inconclusive.
_row3_muv7_tb="$(_row3_probe_sample 124 "" 2>/dev/null || true)"
assert "ROW3-1-UNUSABLE-VACUITY-7: END-TO-END COMPOSE / #5999 false-RED replay -- timed-out probe -> _row3_probe_sample -> t_base=${_row3_muv7_tb}, t_mix=9.000000 => unusable (SKIP; OLD collapse made t_base=1 => usable => slowdown 9.0/1=9.0 > K*floor=6.0 => the false RED)" \
    _row3_measurement_unusable "$_row3_muv7_tb" "9.000000"

# (8) COMPOSE CRUX counterpart: a genuine baseline through the step-2 helper
# must NOT blanket-SKIP ROW3-1 -- real slowdown 9.0/3.0=3.0 is inside the
# bound and must still be evaluated.
_row3_muv8_tb="$(_row3_probe_sample 0 "3.000000" 2>/dev/null || true)"
_row3_muv8_rc=0
_row3_measurement_unusable "$_row3_muv8_tb" "9.000000" || _row3_muv8_rc=$?
assert "ROW3-1-UNUSABLE-VACUITY-8: COMPOSE CRUX counterpart -- genuine probe -> _row3_probe_sample -> t_base=${_row3_muv8_tb}, t_mix=9.000000 => NOT unusable (the fix must not blanket-SKIP; real slowdown 9.0/3.0=3.0 must still be evaluated)" \
    test "$_row3_muv8_rc" -ne 0

# Non-vacuity guard for the NEW ROW3-1 bound predicate (task 5999): the
# SINGLE source of truth for ROW3-1's bound, encoding EXACTLY what the
# inline `python3 -c` ROW3-1 assert evaluates today: `s <= k*floor AND
# s < 10.0`. _row3_within_bound <slowdown> <k> <floor> (rc 0 = within the
# bound, rc != 0 = breached) will replace that inline copy in step-7 so the
# bound has one definition instead of two independently-driftable ones
# (G7 no-lockstep-duplication) — the new high-direction hedge (step-9) must
# skip in EXACTLY the cases the assert would fail, so both consult this same
# predicate rather than restating the comparison.
#
# Pure awk, NO python3 needed, so always-on.

# (1) s=3.0 k=4 floor=1.5 (bound=6.0) => within.
assert "ROW3-1-BOUND-VACUITY-1: s=3.0 k=4 floor=1.5 (bound=6.0) => within_bound" \
    _row3_within_bound 3.0 4 1.5

# (2) BOUNDARY: s exactly AT K*floor => within, pinning the <= inclusivity.
assert "ROW3-1-BOUND-VACUITY-2: BOUNDARY -- s=6.0 k=4 floor=1.5, exactly K*floor => within_bound (pins <= inclusivity)" \
    _row3_within_bound 6.0 4 1.5

# (3) s beyond K*floor => NOT within.
_row3_bndv3_rc=0
_row3_within_bound 7.0 4 1.5 || _row3_bndv3_rc=$?
assert "ROW3-1-BOUND-VACUITY-3: s=7.0 k=4 floor=1.5 (bound=6.0) => NOT within_bound" \
    test "$_row3_bndv3_rc" -ne 0

# (4) HARD-CEILING NON-VACUITY: the proportional clause is satisfied
# (K*floor=160 >> s) but the absolute < 10.0 ceiling still breaches => NOT
# within. Proves the ceiling clause is independently load-bearing and cannot
# be dissolved by a large floor.
_row3_bndv4_rc=0
_row3_within_bound 10.5 4 40 || _row3_bndv4_rc=$?
assert "ROW3-1-BOUND-VACUITY-4: HARD-CEILING NON-VACUITY -- s=10.5 k=4 floor=40 (K*floor=160, proportional clause satisfied) => NOT within_bound (absolute ceiling independently load-bearing)" \
    test "$_row3_bndv4_rc" -ne 0

# (5) CEILING BOUNDARY: s exactly AT the absolute ceiling => NOT within,
# pinning the strict < (not <=).
_row3_bndv5_rc=0
_row3_within_bound 10.0 4 40 || _row3_bndv5_rc=$?
assert "ROW3-1-BOUND-VACUITY-5: CEILING BOUNDARY -- s=10.0 k=4 floor=40, exactly the absolute ceiling => NOT within_bound (pins strict <)" \
    test "$_row3_bndv5_rc" -ne 0

# (6) FAIL-SAFE: non-numeric slowdown => NOT within, so an unreadable
# measurement can never be laundered into a PASS.
_row3_bndv6_rc=0
_row3_within_bound abc 4 1.5 || _row3_bndv6_rc=$?
assert "ROW3-1-BOUND-VACUITY-6: FAIL-SAFE -- s=abc (non-numeric) k=4 floor=1.5 => NOT within_bound (never laundered into a PASS)" \
    test "$_row3_bndv6_rc" -ne 0

# Non-vacuity guard for the NEW ROW3-1 high-direction hedge (task 5999):
# _row3_slowdown_inconclusive <slowdown> <k> <floor> <contended> (rc 0 =
# inconclusive -> caller SKIPs; rc != 0 = the hard assert stays reachable) is
# the high-direction hedge ROW3-1 currently lacks entirely -- composed from
# _row3_within_bound (step-9's definition), never a second copy of the bound.
# SKIPs only on the conjunction NOT within_bound AND _ROW23_CONTENDED==1,
# mirroring ROW2-2's existing "sub-90% completion AND _ROW23_CONTENDED =>
# SKIP" conjunction (see the ROW2-2 assert below) -- a precedent-following
# hedge, not new policy.
#
# Pure shell, NO python3 needed, so always-on.

# (1) FALSE-RED REPLAY: s=9.0 breaches the 6.0 bound (k=4 floor=1.5) on a
# slice whose own avg10 >= 90 => inconclusive (SKIP). The exact #5999 shape.
assert "ROW3-1-CONTENDED-VACUITY-1: FALSE-RED REPLAY -- s=9.0 k=4 floor=1.5 (breaches bound=6.0) contended=1 => inconclusive" \
    _row3_slowdown_inconclusive 9.0 4 1.5 1

# (2) NON-VACUITY CRUX: same breach, but UNCONTENDED => NOT inconclusive, so
# the hard assert stays reachable -- a runaway slowdown on a quiet box must
# still hard-FAIL (the #4415 regression this row exists to catch cannot
# recur).
_row3_civ2_rc=0
_row3_slowdown_inconclusive 9.0 4 1.5 0 || _row3_civ2_rc=$?
assert "ROW3-1-CONTENDED-VACUITY-2: NON-VACUITY CRUX -- s=9.0 k=4 floor=1.5 (breaches bound) contended=0 => NOT inconclusive (hard assert stays reachable, #4415 cannot recur)" \
    test "$_row3_civ2_rc" -ne 0

# (3) FORBIDS SUPPRESSING A GREEN: s=3.0 is well inside the bound; even
# contended, a hedge must never convert a green into a SKIP (mirrors
# 2908c04db0 test(5998): ROW4-1-QUIET-VACUITY-3 forbids suppressing a green).
_row3_civ3_rc=0
_row3_slowdown_inconclusive 3.0 4 1.5 1 || _row3_civ3_rc=$?
assert "ROW3-1-CONTENDED-VACUITY-3: FORBIDS SUPPRESSING A GREEN -- s=3.0 k=4 floor=1.5 (well within bound=6.0) contended=1 => NOT inconclusive (assert still runs and PASSes)" \
    test "$_row3_civ3_rc" -ne 0

# (4) CEILING DIRECTION: the absolute-ceiling breach (not just the
# proportional K*floor clause) is covered too.
assert "ROW3-1-CONTENDED-VACUITY-4: CEILING DIRECTION -- s=10.5 k=4 floor=40 (K*floor=160 satisfied, absolute ceiling breached) contended=1 => inconclusive" \
    _row3_slowdown_inconclusive 10.5 4 40 1

# (5) same ceiling breach, UNCONTENDED => NOT inconclusive.
_row3_civ5_rc=0
_row3_slowdown_inconclusive 10.5 4 40 0 || _row3_civ5_rc=$?
assert "ROW3-1-CONTENDED-VACUITY-5: s=10.5 k=4 floor=40 (ceiling breach) contended=0 => NOT inconclusive (hard assert stays reachable)" \
    test "$_row3_civ5_rc" -ne 0

# (6) FAIL-SAFE: unreadable slowdown is never masked by the hedge.
_row3_civ6_rc=0
_row3_slowdown_inconclusive abc 4 1.5 1 || _row3_civ6_rc=$?
assert "ROW3-1-CONTENDED-VACUITY-6: FAIL-SAFE -- s=abc (non-numeric) k=4 floor=1.5 contended=1 => NOT inconclusive (unreadable measurement never masked)" \
    test "$_row3_civ6_rc" -ne 0

# Non-vacuity guard for the NEW ROW3-1 contention-refinement predicate (task
# 5999 review-amendment, reviewer_comprehensive/robustness + /test-quality):
# _row3_foreign_load <contended> <avg10_rc> <usage_fraction> is what the live
# caller now feeds into _row3_slowdown_inconclusive's <contended> argument
# instead of raw _ROW23_CONTENDED, closing two gaps: (1) an unreadable PSI
# read defaults avg10 to "99" (>= 90), which would otherwise manufacture
# contended=1 with zero genuine evidence; (2) avg10 >= 90 alone is not
# independent of the failure ROW3-1 exists to catch, since a genuine
# over-admission regression self-inflicts pressure on its OWN slice -- so a
# starved-vs-saturating check (mirroring _row2_band_inconclusive) is
# required too.
#
# Pure shell+awk, NO python3 needed, so always-on.

# (1) FOREIGN LOAD CONFIRMED: contended, a genuine PSI read (avg10_rc=0),
# and the slice is STARVED (0.5 < default floor 0.85) => foreign load.
_row3_flv1="$(_row3_foreign_load 1 0 0.5 2>/dev/null || true)"
assert "ROW3-1-FOREIGN-LOAD-VACUITY-1: contended=1 avg10_rc=0 usage_fraction=0.5 (starved) => '1' (got '${_row3_flv1}')" \
    test "$_row3_flv1" = "1"

# (2) NON-VACUITY CRUX (self-inflicted / SATURATING): same contended+genuine
# signal, but usage_fraction=0.95 is AT/ABOVE the floor -- the slice is
# saturating its OWN budget, exactly the over-admission regression this row
# exists to catch, so foreign load must NOT be reported.
_row3_flv2="$(_row3_foreign_load 1 0 0.95 2>/dev/null || true)"
assert "ROW3-1-FOREIGN-LOAD-VACUITY-2: NON-VACUITY CRUX -- contended=1 avg10_rc=0 usage_fraction=0.95 (saturating, not starved) => '0' (got '${_row3_flv2}')" \
    test "$_row3_flv2" = "0"

# (3) UNREADABLE PSI: avg10_rc != 0 means _ROW23_AVG10 was the manufactured
# "99" fallback, not a genuine reading -- must not license a SKIP.
_row3_flv3="$(_row3_foreign_load 1 1 0.5 2>/dev/null || true)"
assert "ROW3-1-FOREIGN-LOAD-VACUITY-3: contended=1 avg10_rc=1 (PSI read failed) usage_fraction=0.5 => '0' (got '${_row3_flv3}')" \
    test "$_row3_flv3" = "0"

# (4) NOT CONTENDED at all (avg10 genuinely < 90).
_row3_flv4="$(_row3_foreign_load 0 0 0.5 2>/dev/null || true)"
assert "ROW3-1-FOREIGN-LOAD-VACUITY-4: contended=0 avg10_rc=0 usage_fraction=0.5 => '0' (got '${_row3_flv4}')" \
    test "$_row3_flv4" = "0"

# (5) FAIL-SAFE: non-numeric/unavailable usage_fraction must not license a
# SKIP either -- an unreadable starvation signal is not evidence of foreign
# load.
_row3_flv5="$(_row3_foreign_load 1 0 unavailable 2>/dev/null || true)"
assert "ROW3-1-FOREIGN-LOAD-VACUITY-5: FAIL-SAFE -- contended=1 avg10_rc=0 usage_fraction=unavailable => '0' (got '${_row3_flv5}')" \
    test "$_row3_flv5" = "0"

# (6) BOUNDARY: usage_fraction exactly AT the default floor (0.85) is NOT
# starved -- pins the strict '<', same convention _row2_band_inconclusive
# uses for the identical starvation check.
_row3_flv6="$(_row3_foreign_load 1 0 0.85 2>/dev/null || true)"
assert "ROW3-1-FOREIGN-LOAD-VACUITY-6: BOUNDARY -- contended=1 avg10_rc=0 usage_fraction=0.85 (== floor) => '0' (got '${_row3_flv6}')" \
    test "$_row3_flv6" = "0"

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

    # Contention probe (task 4967 / esc-4031-154): the task slice's OWN
    # cpu.pressure `some.total` cumulative stall counter, sampled at the SAME
    # two bracket points as the usage_usec delta below — a windowed delta,
    # not a single post-hoc decayed avg10 read, so it cannot under-report
    # contention accrued mid-window.  A lone source pinned to its own
    # dedicated CPUs should show ~0 stall; a large delta indicates a FOREIGN
    # process (e.g. a concurrent pool run sharing the same
    # deterministically-derived pin list, esc-4926-3) co-resides on the
    # pinned CPUs, diluting this measurement.
    _ROW1_STALL_BEFORE="unavailable"
    if [ -n "${_ROW1_TASK_SLICE_REL:-}" ]; then
        _ROW1_STALL_BEFORE="$(python3 "$INSTRUMENT" psi-some-total \
            "/sys/fs/cgroup${_ROW1_TASK_SLICE_REL}/cpu.pressure" 2>/dev/null || echo "unavailable")"
    fi

    sleep "$_ROW1_CONFINE_MEASURE_S"

    _ROW1_USAGE_AFTER="$(python3 "$INSTRUMENT" cgroup-usage "$_ROW1_TASK_SLICE_REL" \
        2>/dev/null || echo "unavailable")"

    _ROW1_STALL_AFTER="unavailable"
    if [ -n "${_ROW1_TASK_SLICE_REL:-}" ]; then
        _ROW1_STALL_AFTER="$(python3 "$INSTRUMENT" psi-some-total \
            "/sys/fs/cgroup${_ROW1_TASK_SLICE_REL}/cpu.pressure" 2>/dev/null || echo "unavailable")"
    fi
    _ROW1_STALL_WINDOW_US=$(( _ROW1_CONFINE_MEASURE_S * 1000000 ))

    wait "$_ROW1_CONFINE_BG" 2>/dev/null || true

    _ROW1_USAGE_DELTA=0
    if [ "$_ROW1_USAGE_BEFORE" != "unavailable" ] && \
       [ "$_ROW1_USAGE_AFTER" != "unavailable" ]; then
        _ROW1_USAGE_DELTA=$(( _ROW1_USAGE_AFTER - _ROW1_USAGE_BEFORE ))
        [ "$_ROW1_USAGE_DELTA" -lt 0 ] && _ROW1_USAGE_DELTA=0  # guard counter wrap
    fi
    _ROW1_USAGE_BUDGET=$(( _ROW4_CONFINE_CORES * _ROW1_CONFINE_MEASURE_S * 1000000 ))

    # Usage fraction (task 4967 follow-up / esc-4031-154 residual): feeds the
    # INACTIVE SKIP predicate below, alongside the stall fraction — same
    # degenerate-guard discipline as the final saturation calc (non-positive
    # budget prints "0").
    _ROW1_USAGE_FRACTION="$(awk -v d="$_ROW1_USAGE_DELTA" -v b="$_ROW1_USAGE_BUDGET" \
        'BEGIN{ if (b+0<=0) {print "0"} else {printf "%.6f", d/b} }')"

    # Windowed stall fraction (display-only; the decision itself is made by
    # _row1_stall_contended below) — same degenerate-guard discipline as the
    # usage delta above: non-positive window or counter-wrap prints "0".
    _ROW1_STALL_FRACTION="unavailable"
    if [ "$_ROW1_STALL_BEFORE" != "unavailable" ] && \
       [ "$_ROW1_STALL_AFTER" != "unavailable" ]; then
        _ROW1_STALL_FRACTION="$(awk -v b="$_ROW1_STALL_BEFORE" -v a="$_ROW1_STALL_AFTER" -v w="$_ROW1_STALL_WINDOW_US" \
            'BEGIN{ if (w+0<=0 || (a+0)<(b+0)) {print "0"} else {printf "%.6f", (a-b)/w} }')"
    fi

    _ROW1_CONTENDED=0
    if _row1_stall_contended "$_ROW1_STALL_BEFORE" "$_ROW1_STALL_AFTER" "$_ROW1_STALL_WINDOW_US"; then
        _ROW1_CONTENDED=1
    fi

    # Measurement-integrity SKIP (never a false-RED under concurrent load):
    # empty slice rel-path, non-positive delta, detected foreign contention
    # on the pinned CPUs (esc-4926-3, a WINDOWED some.total stall-fraction
    # delta bracketed over the measure window rather than a single post-hoc
    # decayed avg10 read), or a never-joined-scope measurement artifact
    # (esc-4031-154 residual, task 4967 follow-up: under extreme load the
    # burn can fail to join its governed scope within warmup+measure at all,
    # showing neither meaningful usage nor meaningful stall — the opposite
    # shape from contention, so distinct from the stall-SKIP above).
    if [ -z "${_ROW1_TASK_SLICE_REL:-}" ]; then
        echo "  SKIP ROW1-1: slice rel-path discovery failed (empty) — cannot compute saturation"
    elif [ "$_ROW1_USAGE_DELTA" -le 0 ]; then
        echo "  SKIP ROW1-1: cpu.stat usage_usec delta is zero — measurement inconclusive"
    elif [ "$_ROW1_CONTENDED" -eq 1 ]; then
        echo "  SKIP ROW1-1: foreign contention detected on pinned CPUs (task-slice windowed stall_fraction=${_ROW1_STALL_FRACTION} >= threshold=${REIFY_CPU_GOV_TEST_ROW1_STALL_SKIP_FRACTION:-0.5}) — inconclusive, not a governance failure"
    elif _row1_measurement_inactive "$_ROW1_USAGE_FRACTION" "$_ROW1_STALL_FRACTION" "$_ROW1_INACTIVE_FRACTION"; then
        echo "  SKIP ROW1-1: measurement inactive — confined scope shows neither meaningful usage nor stall (usage_fraction=${_ROW1_USAGE_FRACTION}, stall_fraction=${_ROW1_STALL_FRACTION}, both < floor=${_ROW1_INACTIVE_FRACTION}) — burn likely never joined its governed scope within warmup+measure, not a governance failure"
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
# §8 Row 3 assertion (ROW3-1, task 5999 — see the ROW3-1 note above): the
#   slowdown = T_mix/T_base decision is a four-branch chain, in order:
#     1. either probe unusable (_row3_measurement_unusable)         → SKIP
#     2. slowdown < fair_share_floor(active, confine-cores)         → SKIP
#     3. slowdown breaches [.., K·floor] AND < 10 (_row3_within_bound)
#        AND the task slice has a GENUINE foreign-load reading
#        (_row3_foreign_load: avg10 >= 90 from an actual PSI read, AND
#        the slice is starved rather than self-inflicting the load)  → SKIP
#     4. otherwise → hard assert: slowdown within [fair_share_floor,
#        K·floor] AND < 10 (the anti-runaway guarantee; #4415 cannot recur).
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
    #     set). Captures the probe's EXIT STATUS explicitly (task 5999,
    #     #5999 false-RED) instead of masking it behind `|| echo "1"` — no
    #     `|| echo <sentinel>` fallback appears in this capture at all, which
    #     is strictly stronger than swapping the old numeric fallback for a
    #     string one and sidesteps the fallback class this file prohibits
    #     (see _row2_usage_fraction's Capture idiom comment above).
    #     _row3_probe_sample turns (rc, raw) into either the literal
    #     "unavailable" or the genuine measurement — never a manufactured
    #     "1" that reads as a legitimate 1-second baseline.
    _T_BASE_RC=0
    _T_BASE_RAW="$(
        REIFY_CPU_GOVERN_SLICE_TASK="$_ROW4_SLICE_TASK" \
        timeout 30 taskset -c "$_ROW4_CONFINE_CPUS" bash "$CPU_GOV_EXEC" --role task -- \
        python3 "$WORK/row23_probe.py" "$_PROBE_ITERS" 2>/dev/null
    )" || _T_BASE_RC=$?
    _T_BASE="$(_row3_probe_sample "$_T_BASE_RC" "$_T_BASE_RAW")"

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

    # ROW2-1 band-decision brackets (task 4970, esc-4959-53): sample the task
    # slice's OWN usage_usec and cpu.pressure some.total BEFORE the warm-up
    # sleep, bracketing the SAME window the avg10 read below uses — reuses
    # the ROW1-1 usage-fraction sampling idiom (cgroup-usage / psi-some-total,
    # guarded with "unavailable" on failure) so ROW2-1's in-band SKIP branch
    # can distinguish a starved (foreign-load) slice from a saturating
    # (genuine-regression) one.
    _ROW23_USAGE_BEFORE="$(python3 "$INSTRUMENT" cgroup-usage "$_ROW23_TASK_SLICE_REL" \
        2>/dev/null || echo "unavailable")"
    _ROW23_STALL_BEFORE="$(python3 "$INSTRUMENT" psi-some-total "$_ROW23_TASK_PRESSURE_PATH" \
        2>/dev/null || echo "unavailable")"

    # (c) Warm-up window then sample the TASK SLICE's OWN cpu.pressure
    #     avg10 (Row 2 PSI measurement — per-cgroup, H5).
    sleep "$_ROW23_WARMUP_S"
    # Captures the read's exit status alongside the existing "99" fallback
    # (task 5999 review-amendment, reviewer_comprehensive/robustness):
    # _ROW23_AVG10 keeps its EXACT pre-existing value/semantics in every case
    # (ROW2-1/ROW2-2 below are unaffected), but _ROW23_AVG10_RC now lets the
    # ROW3-1 hedge (_row3_foreign_load below) tell a genuine 99+ reading
    # apart from a defaulted-because-unreadable one.
    _ROW23_AVG10_RC=0
    _ROW23_AVG10="$(python3 "$INSTRUMENT" psi-avg10 "$_ROW23_TASK_PRESSURE_PATH" 2>/dev/null)" || _ROW23_AVG10_RC=$?
    # Rescue on RC, not emptiness: a read that fails AFTER printing a
    # plausible partial value must not keep that value, same "stdout alone
    # cannot discriminate" principle as the T_base capture above.
    [ "$_ROW23_AVG10_RC" -ne 0 ] && _ROW23_AVG10="99"

    # AFTER bracket, same point the avg10 sample above is taken.
    _ROW23_USAGE_AFTER="$(python3 "$INSTRUMENT" cgroup-usage "$_ROW23_TASK_SLICE_REL" \
        2>/dev/null || echo "unavailable")"
    _ROW23_STALL_AFTER="$(python3 "$INSTRUMENT" psi-some-total "$_ROW23_TASK_PRESSURE_PATH" \
        2>/dev/null || echo "unavailable")"
    _ROW23_STALL_WINDOW_US=$(( _ROW23_WARMUP_S * 1000000 ))

    # Usage fraction over the warm-up window (task 4970 review-amendment):
    # delegates to _row2_usage_fraction, which PROPAGATES the "unavailable"
    # sentinel when either bracket is unreadable instead of collapsing to a
    # numeric "0.000000" — an unreadable bracket must never read as
    # maximally STARVED and mask a genuine over-admission regression.
    _ROW23_USAGE_BUDGET=$(( _ROW4_CONFINE_CORES * _ROW23_WARMUP_S * 1000000 ))
    _ROW23_USAGE_FRACTION="$(_row2_usage_fraction "$_ROW23_USAGE_BEFORE" "$_ROW23_USAGE_AFTER" "$_ROW23_USAGE_BUDGET")"

    # (d) Timed work-based probe under the mix, CONFINED+PINNED → T_mix
    #     (Row 3 slowdown). Mirrors the T_base capture above (task 5999
    #     review-amendment, reviewer_comprehensive/correctness-symmetry): the
    #     previous `|| echo "0"` + `[ -z ] && _T_MIX="0"` pair discarded the
    #     probe's exit status exactly like the old T_base collapse did — an
    #     errored probe (rc!=0) that still printed a partial/plausible
    #     elapsed value would be accepted as a genuine measurement. Routes
    #     through the SAME _row3_probe_sample normalizer so both probes share
    #     one usability rule; _row3_measurement_unusable already handles the
    #     "unavailable" sentinel on either arm, so no other change is needed.
    _T_MIX_RC=0
    _T_MIX_RAW="$(
        REIFY_CPU_GOVERN_SLICE_TASK="$_ROW4_SLICE_TASK" \
        timeout 60 taskset -c "$_ROW4_CONFINE_CPUS" bash "$CPU_GOV_EXEC" --role task -- \
        python3 "$WORK/row23_probe.py" "$_PROBE_ITERS" 2>/dev/null
    )" || _T_MIX_RC=$?
    _T_MIX="$(_row3_probe_sample "$_T_MIX_RC" "$_T_MIX_RAW")"

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

    # Refined foreign-load verdict for ROW3-1's hedge ONLY (task 5999
    # review-amendment, reviewer_comprehensive/robustness + /test-quality;
    # see _row3_foreign_load's docstring above) — ROW2-1/ROW2-2 below keep
    # consuming raw _ROW23_CONTENDED unchanged.
    _ROW23_FOREIGN_LOAD="$(_row3_foreign_load "${_ROW23_CONTENDED}" "${_ROW23_AVG10_RC}" "${_ROW23_USAGE_FRACTION}")"

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
    elif _row2_band_inconclusive "$_ROW23_AVG10" "$_ADMIT_THRESHOLD" "$_ROW23_USAGE_FRACTION" "$_ROW23_STALL_BEFORE" "$_ROW23_STALL_AFTER" "$_ROW23_STALL_WINDOW_US"; then
        echo "  SKIP ROW2-1: inconclusive — foreign load on the pinned CPUs (avg10=${_ROW23_AVG10} in the [${_ADMIT_THRESHOLD},90) band, slice starved: usage_fraction=${_ROW23_USAGE_FRACTION} < floor=${REIFY_CPU_GOV_TEST_ROW1_SATURATION_FLOOR:-0.85} + windowed stall contended) — not a governance failure"
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
    # Skip if either probe is unusable (task 5999: _row3_measurement_unusable
    # subsumes the former T_mix-only hatch and closes the matching T_base
    # arm — a timed-out/errored baseline probe used to collapse to a literal
    # "1" and manufacture an inflated slowdown, the #5999 false-RED) — on a
    # heavily contended host a probe can exceed its budget when a large
    # slowdown is real, making an unusable T_base/T_mix an inconclusive
    # measurement, not a governance failure.
    # Skip (not FAIL) if slowdown < floor too (H5, esc-4926-3 follow-up,
    # empirically observed): at confine-cores scale (active_sources=3 by
    # default) cpu-admit's OWN legitimate admission staggering means not all
    # active_sources are concurrently contending every instant, so the naive
    # fair_share_floor assumption can be violated in the SAFE direction
    # (faster than modeled) — inconclusive for the anti-runaway guarantee
    # below, never a governance failure (mirrors fair_share_floor's own
    # docstring: below-floor is "physically impossible" for the model, i.e.
    # a modeling/measurement mismatch, not evidence of broken governance).
    if _row3_measurement_unusable "${_T_BASE}" "${_T_MIX}"; then
        echo "  SKIP ROW3-1: baseline or mix probe unusable (T_base=${_T_BASE}, T_mix=${_T_MIX}) — inconclusive"
    elif awk -v s="${_ROW3_SLOWDOWN:-0}" -v f="${_ROW3_FLOOR:-0}" 'BEGIN{exit !(s+0 < f+0)}'; then
        echo "  SKIP ROW3-1: slowdown=${_ROW3_SLOWDOWN} below fair-share floor=${_ROW3_FLOOR} — inconclusive (cpu-admit's own admission staggering at confined scale, not all active_sources concurrently contending; anti-runaway guarantee below is unaffected)"
    elif _row3_slowdown_inconclusive "${_ROW3_SLOWDOWN}" "${_SLOWDOWN_K}" "${_ROW3_FLOOR}" "${_ROW23_FOREIGN_LOAD}"; then
        echo "  SKIP ROW3-1: slowdown=${_ROW3_SLOWDOWN} exceeds bound(K=${_SLOWDOWN_K},floor=${_ROW3_FLOOR}) but the task slice's own avg10 (${_ROW23_AVG10}) >= 90 with a genuine PSI read and usage_fraction=${_ROW23_USAGE_FRACTION} < floor (starved, not self-inflicted) — foreign load on the pinned CPUs inflated it, inconclusive"
    else
        assert "ROW3-1: slowdown=${_ROW3_SLOWDOWN} within_bound(floor=${_ROW3_FLOOR},K=${_SLOWDOWN_K}) [confined+pinned]" \
            _row3_within_bound "${_ROW3_SLOWDOWN}" "${_SLOWDOWN_K}" "${_ROW3_FLOOR}"
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
#   fast AND marks its stderr with the structural 'bypass (role=merge)'
#   marker -- rc (ROW4-2) + marker (ROW4-3) are the verdict; the wall-clock
#   is a generous anti-hang guard only (task 6000 T-treatment). Hermetic
#   (synthetic PSI fixture), always-on, no cgroup required.
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

# _row4_share_inconclusive <merge_delta> <task_delta> <w_merge> <w_task> <tol>
#                          <host_avg10> <ceiling>
#   ROW4-1 quiet-box escape-hatch predicate (task 5998): decides whether a
#   measured merge_share that misses the proportional floor should SKIP as
#   inconclusive (foreign load co-resident on the pinned CPUs diluting
#   cpu.weight arbitration) rather than fall through to ROW4-1's hard FAIL.
#   Returns 0 (inconclusive -> caller SKIPs); 1 (NOT inconclusive -> ROW4-1's
#   hard assert stays reachable and can still go RED).
#
#   The hot/quiet decision is DELEGATED to quiet_box_met (load_tolerance_lib.sh)
#   — never re-implemented here and never given a second ceiling constant. That
#   helper is the one #4656 landed for exactly this gate, its fail-open
#   semantics are already unit-tested (test_load_tolerance_lib.sh:168-194), and
#   reusing it is what makes its own ROW4-naming docstring true again.
#
#   Deterministic on its arguments — it reads NO ambient host state, so it
#   stays hermetically testable with synthetic literals like
#   _row2_band_inconclusive and _row1_measurement_inactive above.  But unlike
#   those two (which are awk-only) it is NOT I/O-free: it forks python3 and
#   reads the global $SCRIPT_DIR to import share_ge_proportional.  That is
#   deliberate — importing the live assertion's own floor function is what
#   keeps the corridor bounds from drifting from the floor they bracket — and
#   it means this helper must not be called where a process spawn is unsafe
#   (a signal handler, a tight loop) or where $SCRIPT_DIR is unset.
_row4_share_inconclusive() {
    local merge_delta="$1" task_delta="$2"
    local w_merge="$3" w_task="$4" tol="$5"
    local host_avg10="$6" ceiling="$7"

    # FAIL-SAFE on an unusable cpu.stat delta (pinned by
    # ROW4-1-QUIET-VACUITY-6/7): "unavailable" is what cgroup-usage emits when
    # a slice read fails, and ROW4's bracket arithmetic propagates it verbatim
    # rather than coercing it to 0 — see the sentinel note at that bracket for
    # why the distinction matters. This MUST come before any python3/float()
    # conversion — a raw string reaching
    # float() raises ValueError, and a non-zero python exit is
    # byte-indistinguishable from an honest below-floor verdict, so the
    # predicate would wrongly conclude "sub-floor AND hot" and SKIP away a
    # measurement it never actually evaluated. Returns 1 (NOT inconclusive) so
    # ROW4-1's hard assert stays reachable. Uses the file's established awk
    # numeric-validity idiom, same discipline as _row1_stall_contended's
    # non-integer guard (which likewise short-circuits without invoking
    # python3).
    local _valid
    _valid="$(awk -v m="$merge_delta" -v t="$task_delta" \
        'BEGIN{ print (m+0 == m && m != "" && t+0 == t && t != "") ? "ok" : "bad" }' \
        2>/dev/null || true)"
    if [ "${_valid:-bad}" != "ok" ]; then
        return 1
    fi

    # Box temperature: quiet_box_met returns 0 = quiet/proceed, 1 = hot.
    # Inconclusive requires HOT, so a quiet box falls through to NOT
    # inconclusive and ROW4-1's hard assert stays reachable.
    #
    # LOAD-BEARING (task 5998, pinned by ROW4-1-QUIET-VACUITY-4/5): this
    # branch inherits quiet_box_met's FAIL-OPEN — an empty, "unavailable" or
    # non-numeric avg10 returns rc 0 (quiet), NOT rc 1 (hot). That is what
    # makes "cannot measure the box" compose to "box was quiet" and therefore
    # NOT inconclusive, so an unreadable /proc/pressure/cpu can never turn a
    # genuine governance regression into a silent SKIP. Do NOT "harden" this
    # by treating an unmeasurable sample as hot, and do not add a
    # pre-validation guard on host_avg10 here — either inverts the fail-safe
    # direction. Measured semantics (load_tolerance_lib.sh:146):
    #   'unavailable' -> rc 0   'nan-ish' -> rc 0   '' -> rc 0
    #   '5.0'         -> rc 0   '64.92'   -> rc 1
    if quiet_box_met "$host_avg10" "$ceiling"; then
        return 1
    fi

    # Share: the inconclusive quadrant is the OPEN corridor
    #     fair_share + tol  <  merge_share  <  proportional_floor - tol
    # measured on a hot box.  Both ends are computed by share_ge_proportional
    # — the SAME function the live ROW4-1 assertion below calls — so neither
    # bound can drift from the assertion's floor.
    #
    # UPPER bound (share >= floor): the measurement PASSES, so it is NOT
    # inconclusive no matter how hot the box was — a green is never
    # suppressed.
    #
    # LOWER bound (share <= fair share + tol): bounding above ALONE would make
    # every sub-floor share on a hot box a SKIP, including a TOTAL governance
    # failure — and since an orchestrator host is essentially never quiet (see
    # the escape-hatch comment at the live branch), that is the branch that
    # actually runs, so ROW4-1 would collapse to "PASS or SKIP, never RED".
    # Fair share is what this measures when cpu.weight arbitration is absent
    # or ignored: two sibling slices under one parent split evenly at ~0.50,
    # and a starved merge slice reads ~0.00.  Neither is dilution — every
    # dilution observation on record is far above (0.6355, plus
    # 0.6435/0.6471/0.639 at :1378) — so both fall through to the hard assert.
    #
    # The lower bound is a BAND, not the point share == fair share.  A
    # weights-ignored regression does not land on 0.5000000 exactly: it is an
    # even split measured over ~16M usec of counters with noise in BOTH
    # directions, so an exact-equality bound would be conclusive with
    # probability ~0 and roughly half of such regressions would measure
    # 0.50xx, land inside the corridor and SKIP instead of going RED — on the
    # hot host where this row actually runs.  Widening it by tol makes the
    # whole weights-ignored neighbourhood conclusive.
    # G6 (no new number): the bound is share_ge_proportional with the two
    # weights held EQUAL (w_task twice — the literal reading of "weights
    # ignored") and the SAME tol the live assertion uses, evaluated on the
    # SWAPPED pair so the comparison is task_share >= 0.5 - tol, i.e.
    # merge_share <= 0.5 + tol.  Swapping rather than negating is what makes
    # the bound STRICT: a share at fair share + tol is conclusive, while one
    # microsecond above it is still inside the corridor
    # (ROW4-1-CORRIDOR-VACUITY-3).  At the default tol=0.10 the corridor is
    # (0.60, 0.65), which still contains every recorded dilution observation
    # above, so widening the band re-arms the RED without re-opening #4656.
    #
    # Verdict is read from STDOUT, not from python3's exit status: a failed
    # import or a missing interpreter also exits non-zero, which would be
    # byte-indistinguishable from an honest below-floor verdict and would make
    # a broken python mask a regression.  Defaulting the empty capture to
    # "conclusive" keeps that failure in the fail-safe direction.  Same
    # discipline as quiet_box_met's own `|| echo quiet` capture.
    local _corridor
    _corridor="$(python3 -c "
import sys
sys.path.insert(0, '${SCRIPT_DIR}')
from cpu_gov_instrument import share_ge_proportional
m = float('${merge_delta}')
t = float('${task_delta}')
w_merge = float('${w_merge}')
w_task = float('${w_task}')
tol = float('${tol}')
if m + t <= 0:
    print('conclusive')          # nothing ran; not evidence of dilution
elif share_ge_proportional(m, t, w_merge, w_task, tol):
    print('conclusive')          # at/above the floor -> ROW4-1 PASSES
elif share_ge_proportional(t, m, w_task, w_task, tol):
    print('conclusive')          # merge_share <= fair share + tol -> BROKEN
else:
    print('corridor')            # strictly inside the dilution corridor
" 2>/dev/null || true)"
    [ "${_corridor:-conclusive}" = "corridor" ] || return 1

    # Share strictly inside the dilution corridor AND a hot box — the only
    # inconclusive quadrant.
    return 0
}

# _row4_sample_host_avg10 <proc_path>
#   Echo the `some` line's avg10 field from a PSI-formatted file, or the
#   literal "unavailable" when the path is missing/unreadable/malformed
#   (task 5998).  Delegates to the instrument's psi-avg10 subcommand — the
#   SAME sampler ROW2_3 uses at its own bracket — which already prints
#   "unavailable" (exit 0) on any read/parse failure; the `|| echo` guard
#   covers python3 itself failing to launch.
#
#   "unavailable" is the FAIL-SAFE direction, not a degenerate one: it feeds
#   quiet_box_met's fail-open, so an unreadable PSI source reads as a quiet
#   box and leaves ROW4-1's hard assert reachable.  Never substitute 0 here —
#   a numeric 0 reads as a maximally quiet box for the same reason, but does
#   so by ASSERTING quiet rather than by declining to measure, which is the
#   distinction ROW4-1-SAMPLE-VACUITY-2 pins.
_row4_sample_host_avg10() {
    local proc_path="$1"
    python3 "$INSTRUMENT" psi-avg10 "$proc_path" 2>/dev/null \
        || echo "unavailable"
}

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

# ----------------------------------------------------------------------------
# ROW4-1-QUIET-VACUITY (task 5998): non-vacuity guard for the RESTORED ROW4-1
# quiet-box escape hatch.
#
# ROW4-1 below carried the design claim that "the confined parent quota makes
# this measurement host-load-independent, so it asserts directly" — which is
# why it was, uniquely in this file, the ONLY live-measurement assertion with
# no load/contention escape hatch (ROW1-1 has four SKIP branches, ROW2-1
# three, ROW2-2 and ROW3-1 partial ones). MEASUREMENT FALSIFIES THAT PREMISE:
# on tip 6f234cf98d at loadavg ~190/32, four back-to-back runs of this file
# produced one FAIL —
#     Δmerge=10367459, Δtask=5945387 => merge_share 0.6355 < floor 0.65
# — against a passing run minutes later on the same tip
#     Δmerge=11214247, Δtask=4615047 => merge_share 0.7084.
# The quota bounds the AGGREGATE (both runs saturated it: usage fractions
# 1.019 and 0.989 against the 2-core × 8 s budget) but NOT the SPLIT between
# the two weighted children. The burns are taskset-pinned to the confine
# CPUs, and pinning does not exclude FOREIGN processes from those CPUs
# (task 4967); under heavy oversubscription that foreign traffic perturbs the
# interleaving and cpu.weight proportionality — only asymptotically enforced —
# degrades from 0.75 toward 0.50. Every observation of this shape sits in that
# corridor: 0.6355, plus the 0.6435/0.6471/0.639 already recorded in this
# file's own history (:1378). This is a recurrence of the #4656 flake class,
# whose quiet-box gate H5/#4926 removed on the now-falsified premise.
#
# The fix restores that gate as a POST-measurement guard, NOT a pre-gate, and
# loosens no threshold (_ROW4_TOL stays byte-identical; the ceiling reused is
# the existing shared REIFY_CPU_GOV_TEST_QUIET_CEILING, default 20 — G6, no
# new number). _row4_share_inconclusive returns rc 0 (inconclusive -> caller
# SKIPs) ONLY when the measured share sits strictly inside the OPEN dilution
# corridor (fair + tol, floor) = (0.60, 0.65) at the defaults AND the box was
# hot, which keeps all four outcomes maximally informative:
#   share >= floor                -> assert runs and PASSES, even on a hot box
#                                    (no green is ever suppressed);
#   share <= fair + tol           -> assert runs (a weights-ignored or starved
#                                    split is a governance break, not dilution,
#                                    and goes RED at any ceiling);
#   in corridor, box QUIET        -> assert runs and goes RED (non-vacuous — a
#                                    genuine governance regression still fails);
#   in corridor, box HOT          -> SKIP-inconclusive (the false-RED case).
# Mirrors ROW2-2's existing post-measurement idiom in this same file.
#
# Pure synthetic literals, no cgroup/live measurement — deterministic at any
# host load. Python-gated because the predicate consults share_ge_proportional
# (the SAME function the live ROW4-1 assertion calls, so the guard's floor can
# never drift from the assertion's floor).
# ----------------------------------------------------------------------------
if [ "$_PYTHON_AVAILABLE" -eq 0 ]; then
    echo "  SKIP ROW4-1-QUIET-VACUITY: python3 not on PATH"
else
    # (1) The escape hatch FIRES on the real measured false-RED: sub-floor
    # share measured on a hot box (host avg10 64.92 was the live
    # /proc/pressure/cpu reading at loadavg 138/32 while reproducing this).
    assert "ROW4-1-QUIET-VACUITY-1: measured false-RED replay (Δmerge=10367459 Δtask=5945387 => share 0.6355 < floor 0.65, host avg10=64.92 >= ceiling=20) => inconclusive (SKIP)" \
        _row4_share_inconclusive 10367459 5945387 300 100 0.10 64.92 20

    # (2) NON-VACUITY CRUX: the SAME sub-floor deltas as (1), but measured on
    # a QUIET box — the "others quiet" precondition PRD §8 row 4 actually
    # assumes. A sub-floor share there is a genuine cpu.weight governance
    # regression, not foreign dilution, so the predicate must report NOT
    # inconclusive and leave ROW4-1's hard assert reachable to go RED.
    # Forbids degenerating the guard into an unconditional SKIP.
    _row4_quiet_vacuity2_rc=0
    _row4_share_inconclusive 10367459 5945387 300 100 0.10 5.0 20 \
        || _row4_quiet_vacuity2_rc=$?
    assert "ROW4-1-QUIET-VACUITY-2: NON-VACUITY CRUX -- same sub-floor share but QUIET box (avg10=5.0 < ceiling=20) => NOT inconclusive (hard assert stays reachable; a real governance regression still goes RED)" \
        test "$_row4_quiet_vacuity2_rc" -ne 0

    # (3) A SATURATING share on a HOT box must still be NOT inconclusive, so
    # the guard never suppresses a green: the assert runs and PASSES. Replays
    # the real measured PASSING run from the same reproduction session (same
    # tip, same box, minutes from the failing run replayed in (1)) — this is
    # what forbids short-circuiting on box temperature alone, which is
    # precisely what the implementation does today.
    _row4_quiet_vacuity3_rc=0
    _row4_share_inconclusive 11214247 4615047 300 100 0.10 64.92 20 \
        || _row4_quiet_vacuity3_rc=$?
    assert "ROW4-1-QUIET-VACUITY-3: hot box but SATURATING share (Δmerge=11214247 Δtask=4615047 => 0.7084 >= floor 0.65) => NOT inconclusive (a passing measurement still PASSES; the guard never suppresses a green)" \
        test "$_row4_quiet_vacuity3_rc" -ne 0

    # (4) FAIL-SAFE on an unmeasurable box: sub-floor share, but the host PSI
    # sample is the literal "unavailable" sentinel (/proc/pressure/cpu
    # unreadable). quiet_box_met deliberately fails OPEN on that input, so
    # "cannot measure the box" must read as "box was quiet" => NOT
    # inconclusive. An unmeasurable box must never mask a genuine governance
    # regression behind a SKIP. Mirrors ROW2-1-BAND-VACUITY-5 and
    # _row1_measurement_inactive's unavailable fail-safe convention.
    _row4_quiet_vacuity4_rc=0
    _row4_share_inconclusive 10367459 5945387 300 100 0.10 unavailable 20 \
        || _row4_quiet_vacuity4_rc=$?
    assert "ROW4-1-QUIET-VACUITY-4: FAIL-SAFE -- sub-floor share but host PSI unreadable (avg10=unavailable) => NOT inconclusive (an unmeasurable box never masks a genuine break)" \
        test "$_row4_quiet_vacuity4_rc" -ne 0

    # (5) Same fail-safe for a non-numeric (garbage) PSI sample — a truncated
    # or malformed /proc read must take the same never-mask path as an absent
    # one, not fall through to a SKIP.
    _row4_quiet_vacuity5_rc=0
    _row4_share_inconclusive 10367459 5945387 300 100 0.10 nan-ish 20 \
        || _row4_quiet_vacuity5_rc=$?
    assert "ROW4-1-QUIET-VACUITY-5: FAIL-SAFE -- sub-floor share but non-numeric PSI sample (avg10='nan-ish') => NOT inconclusive (malformed read takes the same never-mask path as an absent one)" \
        test "$_row4_quiet_vacuity5_rc" -ne 0

    # (6)/(7) FAIL-SAFE on an unusable cpu.stat delta. "unavailable" is the
    # exact shape `cpu_gov_instrument.py cgroup-usage` emits when a slice read
    # fails, and the ROW4 bracket below propagates it verbatim (it used to
    # collapse it to 0, which made a read failure indistinguishable from a
    # starved slice; the sentinel note at that bracket has the argument, and
    # CORRIDOR-VACUITY-2 pins the starved shape it is now distinct from). A
    # delta that cannot be read is NOT evidence of dilution, so even on a HOT
    # box it must be NOT inconclusive — otherwise an unreadable measurement
    # silently becomes a SKIP that hides a regression. The bracket's own
    # guard chain SKIPs the sentinel before the predicate ever sees it; this
    # is the second line of defence. Mirrors _row1_stall_contended's
    # numeric-validity guard, which returns rc 1 on non-integer input WITHOUT
    # invoking python3 (a raw string reaching float() raises ValueError, which
    # is indistinguishable from an honest below-floor verdict).
    _row4_quiet_vacuity6_rc=0
    _row4_share_inconclusive unavailable 5945387 300 100 0.10 64.92 20 \
        || _row4_quiet_vacuity6_rc=$?
    assert "ROW4-1-QUIET-VACUITY-6: FAIL-SAFE -- merge delta unreadable (Δmerge=unavailable, hot box) => NOT inconclusive (an unreadable measurement never becomes a SKIP)" \
        test "$_row4_quiet_vacuity6_rc" -ne 0

    _row4_quiet_vacuity7_rc=0
    _row4_share_inconclusive 10367459 unavailable 300 100 0.10 64.92 20 \
        || _row4_quiet_vacuity7_rc=$?
    assert "ROW4-1-QUIET-VACUITY-7: FAIL-SAFE -- task delta unreadable (Δtask=unavailable, hot box) => NOT inconclusive (symmetric with -6)" \
        test "$_row4_quiet_vacuity7_rc" -ne 0

    # ── ROW4-1-CORRIDOR-VACUITY: the inconclusive quadrant's LOWER bound ────
    # The QUIET family above bounds the inconclusive quadrant ABOVE (share <
    # floor). Bounding it above ALONE is not enough: it would make every
    # sub-floor share on a hot box a SKIP, including a TOTAL governance
    # failure. That matters because this row's own escape-hatch comment
    # concedes an orchestrator host "is essentially never quiet" — so the hot
    # branch is the branch that actually runs here, and if it swallowed
    # everything below the floor, ROW4-1 could only PASS or SKIP in practice
    # and VACUITY-2's quiet-box crux would pin a case that never occurs.
    #
    # The lower bound is the WEIGHTS-IGNORED FAIR SHARE, WIDENED BY tol: with
    # two sibling slices under one parent, cpu.weight arbitration being absent
    # or broken yields ~0.50 (equal split), and a starved merge slice yields
    # ~0.00. Neither is dilution. Every dilution observation on record sits
    # well ABOVE that: 0.6355 (the #5998 reproduction) plus 0.6435/0.6471/0.639
    # from this file's history (:1378). So the corridor is
    # (fair + tol, floor) — (0.60, 0.65) at the default tol — open at BOTH
    # ends, and a share at or below fair share + tol falls through to ROW4-1's
    # hard assert no matter how hot the box was.
    #
    # Why a BAND and not the point 0.50: a weights-ignored regression is an
    # even split measured over ~16M usec of counters, noisy in both
    # directions, so it lands on 0.5000000 exactly with probability ~0. An
    # exact-equality bound would therefore be conclusive almost never and
    # would SKIP roughly half of real weights-ignored regressions on the hot
    # host where this row runs — the exact failure mode ROW4-1 exists to
    # catch. Widening by tol makes the whole neighbourhood conclusive while
    # leaving every recorded dilution observation inside the corridor.
    #
    # G6 (no new number): the bound is not a tuned constant. It is
    # share_ge_proportional applied with the two weights held EQUAL — the
    # literal meaning of "weights ignored" — reusing _ROW4_W_TASK and the SAME
    # _ROW4_TOL the live assertion uses, so it can no more drift from the
    # assertion than the upper bound can.
    #
    # (1) pins a REALISTICALLY NOISY near-fair split rather than an exact tie:
    # 0.500110 is the shape the wired path actually produces when cpu.weight
    # is ignored, and it is the case an exact-equality bound would have missed.
    _row4_corridor_vacuity1_rc=0
    _row4_share_inconclusive 5948000 5945387 300 100 0.10 64.92 20 \
        || _row4_corridor_vacuity1_rc=$?
    assert "ROW4-1-CORRIDOR-VACUITY-1: LOWER-BOUND CRUX -- noisy weights-ignored split (Δmerge=5948000 Δtask=5945387 => 0.500110, just off an exact tie) on a HOT box => NOT inconclusive (forbids 'hot box => never RED'; broken cpu.weight still goes RED where the row actually runs, even though it never measures 0.50 exactly)" \
        test "$_row4_corridor_vacuity1_rc" -ne 0

    # (2) A STARVED merge slice (share 0.00) on a hot box is the other end of
    # the same argument, and it is a shape the WIRED path really produces: the
    # merge slice reads fine, its usage_usec simply never advances. That is a
    # governance break of the most total kind and must reach the hard assert.
    # It is deliberately NOT the same as an unreadable slice — the bracket
    # below propagates "unavailable" for that and SKIPs it — which is the
    # whole point of keeping the two apart (VACUITY-6/7 guard the sentinel).
    _row4_corridor_vacuity2_rc=0
    _row4_share_inconclusive 0 5945387 300 100 0.10 64.92 20 \
        || _row4_corridor_vacuity2_rc=$?
    assert "ROW4-1-CORRIDOR-VACUITY-2: starved merge slice (Δmerge=0 => share 0.00, hot box) => NOT inconclusive (a slice that reads fine but never advances is a total governance break, not dilution)" \
        test "$_row4_corridor_vacuity2_rc" -ne 0

    # (3) NON-VACUITY for the bound itself: the lower bound must be STRICT, or
    # it would swallow the corridor it exists to preserve and re-open the
    # #4656 false-RED this task closes. Pinned to one microsecond above the
    # widened bound fair + tol = 0.60 — the tightest statement that the hatch
    # still fires just inside the corridor. (The measured false-RED at 0.6355
    # sits far above this.) Its one-microsecond-lower neighbour is the
    # conclusive side of the same edge, pinned by (3b).
    assert "ROW4-1-CORRIDOR-VACUITY-3: one microsecond ABOVE fair share + tol (Δmerge=8918081 Δtask=5945387 => 0.60000001 > 0.60) on a HOT box => inconclusive (the lower bound is strict and does not swallow the dilution corridor)" \
        _row4_share_inconclusive 8918081 5945387 300 100 0.10 64.92 20

    # (3b) The other side of that same microsecond: one usec LOWER is at/below
    # fair + tol and must be conclusive. Together with (3) this pins the bound
    # to a single microsecond, so neither a silently-widened corridor (which
    # would re-swallow weights-ignored regressions) nor a silently-narrowed
    # one (which would re-open the #5998 false-RED) can pass unnoticed.
    _row4_corridor_vacuity3b_rc=0
    _row4_share_inconclusive 8918080 5945387 300 100 0.10 64.92 20 \
        || _row4_corridor_vacuity3b_rc=$?
    assert "ROW4-1-CORRIDOR-VACUITY-3b: one microsecond BELOW the same edge (Δmerge=8918080 Δtask=5945387 => 0.59999999 <= 0.60) on a HOT box => NOT inconclusive (the conclusive side of the strict bound)" \
        test "$_row4_corridor_vacuity3b_rc" -ne 0

    # (4) No usage at all is not evidence of dilution either. The live branch
    # already guards both-zero ahead of the predicate; this keeps the
    # predicate itself total, so a future caller cannot reach a 0/0 SKIP.
    _row4_corridor_vacuity4_rc=0
    _row4_share_inconclusive 0 0 300 100 0.10 64.92 20 \
        || _row4_corridor_vacuity4_rc=$?
    assert "ROW4-1-CORRIDOR-VACUITY-4: FAIL-SAFE -- no usage at all (Δmerge=Δtask=0, hot box) => NOT inconclusive (an empty measurement is not evidence of dilution)" \
        test "$_row4_corridor_vacuity4_rc" -ne 0

    # ── ROW4-1-SAMPLE-VACUITY: the host-PSI sampler feeding the guard ───────
    # _row4_sample_host_avg10 <proc_path> echoes the `some` line's avg10 field,
    # or the literal "unavailable" when the path is missing/unreadable/
    # malformed — the sentinel VACUITY-4 above already proves composes to a
    # fail-safe verdict. Exercised against a synthetic fixture written in the
    # _MEM_PSI_QUIET / _SELF_PSI_QUIET / ROW4-BYPASS style, so the sampler is
    # hermetic and needs no particular host PSI state.
    #
    # Capture idiom: "$(... || true)" — MUST NOT be `|| echo unavailable`,
    # which would make case (b) pass even while the sampler is undefined and
    # destroy the RED signal (:867-869).
    _row4_psi_hot_fixture="$(mktemp -p "$WORK" row4-psi-hot.XXXXXX)"
    printf 'some avg10=99.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
        > "$_row4_psi_hot_fixture"

    # (a) Parses the `some` line's avg10 out of a synthetic HOT fixture.
    # Compared numerically (locale-proof), mirroring
    # ROW2-1-USAGE-FRACTION-VACUITY-3 rather than a string equality.
    _row4_sampled_hot="$(_row4_sample_host_avg10 "$_row4_psi_hot_fixture" 2>/dev/null || true)"
    assert "ROW4-1-SAMPLE-VACUITY-1: synthetic HOT fixture (some avg10=99.00) => 99 (got '${_row4_sampled_hot}')" \
        awk -v a="$_row4_sampled_hot" 'BEGIN{ exit !(a+0 == 99) }'

    # (b) A missing path yields the literal sentinel, never an empty string or
    # a bare 0 — a 0 would read as a maximally QUIET box and flip the guard
    # from fail-safe to fail-masking.
    _row4_sampled_missing="$(_row4_sample_host_avg10 "$WORK/row4-psi-does-not-exist" 2>/dev/null || true)"
    assert "ROW4-1-SAMPLE-VACUITY-2: nonexistent PSI path => 'unavailable' (never '' or 0, which would read as a maximally QUIET box) (got '${_row4_sampled_missing}')" \
        test "$_row4_sampled_missing" = "unavailable"

    # (c) END-TO-END: drive the sampled-from-fixture value straight into the
    # predicate with the real measured false-RED deltas, proving sampler and
    # predicate compose through the REIFY_CPU_GOV_TEST_PROC_PATH seam that
    # step-12 wires into the live ROW4-1 branch. Mirrors
    # ROW2-1-USAGE-FRACTION-VACUITY-5's END-TO-END idiom.
    assert "ROW4-1-QUIET-VACUITY-8: END-TO-END -- avg10 sampled from a HOT fixture ('${_row4_sampled_hot}' >= ceiling=20) + the measured false-RED deltas => inconclusive (sampler and predicate compose)" \
        _row4_share_inconclusive 10367459 5945387 300 100 0.10 "$_row4_sampled_hot" 20
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

    # (g2) Sample HOST-WIDE PSI at the same instant the window closes, before
    #      the reap below — as close to the bracketed interval as possible, so
    #      the sample describes the interval that was actually measured rather
    #      than teardown.  Host-wide (not slice-relative) is deliberate: the
    #      ROW4 parent is quota-capped, and a cgroup's `some` pressure counts
    #      runnable-but-not-running time, which INCLUDES quota throttling — so
    #      a quota-capped slice always shows high stall and slice-relative PSI
    #      is useless as a foreign-load discriminator here.  What actually
    #      breaks this measurement is other work contending for the pinned
    #      CPUs, which is exactly what host-wide PSI measures.
    _ROW4_HOST_AVG10="$(_row4_sample_host_avg10 "$_ROW4_PROC_PATH")"

    # (h) Reap both burns (natural completion or timeout) before cleanup.
    wait "$_ROW4_TASK_BG" 2>/dev/null || true
    wait "$_ROW4_MERGE_BG" 2>/dev/null || true

    # An unreadable endpoint stays OUT of the numeric coercion (task 5998
    # amendment).  Initializing the deltas to 0 instead made a FAILED cpu.stat
    # read byte-indistinguishable from a genuinely STARVED merge slice: both
    # arrive as Δmerge=0, but the first is a measurement failure that must SKIP
    # and the second is a real governance break that must go RED.  Propagating
    # the sentinel — the same shape _row2_usage_fraction adopted for the same
    # reason — lets the guard chain below tell them apart, and makes
    # _row4_share_inconclusive's string guard (ROW4-1-QUIET-VACUITY-6/7) a real
    # second line of defence on the wired path rather than a contract on an
    # input this bracket could never actually produce.
    _ROW4_TASK_DELTA="unavailable"
    _ROW4_MERGE_DELTA="unavailable"
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
    # to the confined CPUs (H5 + esc-4926-3).
    #
    # SCOPE OF THE CONFINEMENT (task 5998 — corrects the earlier claim that
    # pinning makes this bound host-load-independent, and the "foreign load
    # only deepens co-residency and improves convergence" gloss): the quota
    # bounds the AGGREGATE, not the SPLIT. Both runs of the 5998 reproduction
    # saturated the aggregate — usage fractions 1.019 (failing) and 0.989
    # (passing) against the 2-core × 8 s budget — while their merge_share
    # differed by 0.07. Pinning does not exclude FOREIGN processes from the
    # pinned CPUs (task 4967); under heavy oversubscription that foreign
    # traffic perturbs the interleaving and cpu.weight proportionality, which
    # is only asymptotically enforced, degrades from 0.75 toward 0.50. The
    # measured false-RED was 0.6355 vs the 0.65 floor (#4656 flake class; this
    # file's own history at :1378 records 0.639 of the same shape). The
    # quiet-box escape hatch below closes it WITHOUT loosening any threshold.
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
    elif [ "$_ROW4_TASK_DELTA" = "unavailable" ] || \
         [ "$_ROW4_MERGE_DELTA" = "unavailable" ]; then
        # A cpu.stat endpoint could not be read, so there is no measurement to
        # judge.  Deliberately distinct from the starved-slice case below: a
        # starved merge slice READS fine and simply does not advance, which
        # still reaches ROW4-1's hard assert and goes RED.  Only an actual read
        # failure lands here.  Measurement-integrity guard, not load-based.
        echo "  SKIP ROW4-1: cpu.stat read failed (Δmerge=${_ROW4_MERGE_DELTA},Δtask=${_ROW4_TASK_DELTA}) — no measurement to judge"
    elif [ "$_ROW4_TASK_DELTA" -le 0 ] && [ "$_ROW4_MERGE_DELTA" -le 0 ]; then
        echo "  SKIP ROW4-1: both cpu.stat deltas are zero — measurement inconclusive"
    elif _row4_share_inconclusive "$_ROW4_MERGE_DELTA" "$_ROW4_TASK_DELTA" \
            "$_ROW4_W_MERGE" "$_ROW4_W_TASK" "$_ROW4_TOL" \
            "$_ROW4_HOST_AVG10" "$_ROW4_QUIET_CEILING"; then
        # Quiet-box escape hatch (task 5998, restoring the gate H5/#4926
        # removed). Ordered AFTER the measurement, never as a pre-gate: a
        # pre-gate would discard every ROW4-1 measurement on an orchestrator
        # host, which is essentially never quiet, making the assertion
        # vacuous in practice. Here it fires in exactly one quadrant — a share
        # strictly inside the dilution corridor (fair + tol, floor) AND a hot
        # box — so a share at/above the floor still PASSES even on a hot box,
        # a corridor share on a QUIET box still goes RED
        # (ROW4-1-QUIET-VACUITY-2 is the non-vacuity crux pinning that), and a
        # share at or below fair + tol — the weights-ignored shape this row
        # exists to catch — goes RED at ANY ceiling, hot box included
        # (ROW4-1-CORRIDOR-VACUITY-1). No threshold is loosened: _ROW4_TOL is
        # untouched and the ceiling is the existing shared
        # REIFY_CPU_GOV_TEST_QUIET_CEILING.
        echo "  SKIP ROW4-1: inconclusive — sub-floor merge_share (Δmerge=${_ROW4_MERGE_DELTA},Δtask=${_ROW4_TASK_DELTA},W=${_ROW4_W_MERGE}/${_ROW4_W_TASK},tol=${_ROW4_TOL}) measured on a NON-QUIET box (host avg10=${_ROW4_HOST_AVG10} >= ceiling=${_ROW4_QUIET_CEILING}) — foreign load on the pinned CPUs dilutes cpu.weight proportionality, not a governance failure"
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

# _row4_bypass_probe <admit_script> <psi_fixture> <timeout_s> <stderr_file>
#   Runs cpu-admit's `admit` subcommand under DF_VERIFY_ROLE=merge against a
#   synthetic PSI fixture, captures the child's stderr into <stderr_file>,
#   and RETURNS the child's rc (task 6000). Keeps the exact env set the live
#   ROW4-2/ROW4-3 checks below have always used (REIFY_CPU_ADMIT_PROC_PATH/
#   MAX_WAIT/POLL) — factored out so both the live cpu-admit.sh path and the
#   hermetic ROW4-BYPASS-VACUITY-1/2 stub scripts drive the identical
#   invocation shape.
#
#   <timeout_s> is an ANTI-HANG guard ONLY (T-treatment,
#   docs/prds/infra-test-wallclock-deflake.md:33) — it must never be sized to
#   discriminate pass/fail. The verdict is the rc (ROW4-2) plus the
#   structural stderr marker (ROW4-3); the timeout exists solely so a
#   genuinely hung admit cannot hang this suite forever. A caller that needs
#   to prove the guard still fires on a real hang (ROW4-BYPASS-VACUITY-2c)
#   passes a deliberately tiny explicit value here rather than mutating the
#   live budget.
#
#   rc is captured via `|| _rc=$?`, never `$(...)` command substitution: a
#   bare substitution discards the child's exit status, and a caller relying
#   on a genuine 124 (ROW4-BYPASS-VACUITY-2c) would silently see a false 0.
#
#   Explicitly neutralizes REIFY_CPU_ADMIT_DISABLE (forces it empty) instead
#   of merely inheriting whatever the ambient environment happens to hold:
#   cpu-admit.sh checks the break-glass disable branch (scripts/cpu-admit.sh,
#   "(1) Break-glass bypass") BEFORE the merge-bypass branch ("(2) Merge
#   bypass"), so an operator who has exported REIFY_CPU_ADMIT_DISABLE=1 --
#   plausibly exactly when a host is heavily loaded, which is when this test
#   is most likely to run -- would otherwise still see rc=0 (ROW4-2 passes)
#   but stderr reading "disabled" instead of "bypass (role=merge)", a false
#   RED on the ROW4-3 structural marker despite nothing being wrong.
_row4_bypass_probe() {
    local admit_script="$1" psi_fixture="$2" timeout_s="$3" stderr_file="$4"
    local _rc=0
    timeout "$timeout_s" \
        env DF_VERIFY_ROLE=merge \
            REIFY_CPU_ADMIT_PROC_PATH="$psi_fixture" \
            REIFY_CPU_ADMIT_MAX_WAIT=1 \
            REIFY_CPU_ADMIT_POLL=1 \
            REIFY_CPU_ADMIT_DISABLE= \
        bash "$admit_script" admit \
        >/dev/null 2>"$stderr_file" || _rc=$?
    return "$_rc"
}

# _row4_bypass_floor_applies <raw_knob_value>
#   Returns 0 (the >=60s floor APPLIES) iff <raw_knob_value> is empty --
#   i.e. the operator has NOT overridden
#   REIFY_CPU_GOV_TEST_ROW4_BYPASS_TIMEOUT_S and the live budget is
#   therefore the BUILT-IN DEFAULT the floor pins (task 6000 review fix).
#   Returns exactly 1 (floor SKIPs) for any non-empty value, whatever its
#   magnitude: the discriminator is "was it overridden", never "is it big"
#   (ROW4-BYPASS-VACUITY-3c). Unset and explicitly-empty are deliberately
#   identical -- the call site's `${KNOB:-}` and the budget's `${KNOB:-120}`
#   both resolve either to the default.
#   MUST be called only in `if` / `|| _rc=$?` context: under this file's
#   `set -euo pipefail` a bare call on the skip verdict (1) aborts the suite.
_row4_bypass_floor_applies() {
    [ -z "${1:-}" ]
}

# ============================================================================
# Cycle ROW4-BYPASS — §8 row-9 merge-bypass smoke (always-on, hermetic).
# DF_VERIFY_ROLE=merge + high-PSI fixture → cpu-admit.sh admit exits 0 fast
# AND marks its stderr with the structural 'bypass (role=merge)' marker.
# ROW4-2 (rc) + ROW4-3 (marker) are the verdict; the wall-clock is an
# anti-hang guard only -- T-treatment, docs/prds/infra-test-wallclock-
# deflake.md:33.
# Uses a synthetic /proc/pressure/cpu fixture (no real PSI needed).
# ============================================================================
echo ""
echo "--- Cycle ROW4-BYPASS: merge-bypass smoke (cpu-admit.sh, §8 row 9) ---"

# Create synthetic high-PSI fixture: avg10=99 would block non-merge admits.
_ROW4_PSI_FIXTURE="$WORK/row4_psi_fixture"
printf 'some avg10=99.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
    > "$_ROW4_PSI_FIXTURE"

# ROW4-2/ROW4-3 anti-hang guard ONLY (T-treatment,
# docs/prds/infra-test-wallclock-deflake.md:33) -- NEVER a discriminator. The
# verdict is the rc (ROW4-2 below) plus the structural stderr marker
# (ROW4-3); this timeout exists solely so a genuinely hung admit cannot hang
# this suite forever. ROW4-2 is the same merge-bypass case #4844
# (test_cpu_admit.sh Cycle D) converted to this treatment there -- this row
# was missed at the time and is what task 6000 closes.
#
# 120s is generous, not a tuned guess: the real merge bypass was MEASURED at
# 713 ms on a loadavg-~190/32-core box, so 120s is ~170x the observed cost.
# It also mirrors the budget tests/infra/test_cpu_load_governance_deflake.sh:115
# already grants to an ENTIRE run of this SUT. ROW4-BYPASS-VACUITY-2d pins
# that this floor stays generous enough to never discriminate; -2c pins that
# the guard still fires on a genuine hang regardless of how generous it is.
_ROW4_BYPASS_TIMEOUT_S="${REIFY_CPU_GOV_TEST_ROW4_BYPASS_TIMEOUT_S:-120}"

# ROW4-2: DF_VERIFY_ROLE=merge bypasses PSI → cpu-admit admit exits 0 fast.
_ROW4_BYPASS_STDERR="$(mktemp -p "$WORK" row4-bypass-stderr.XXXXXX)"
_ROW4_BYPASS_START=$(date +%s)
_ROW4_BYPASS_RC=0
_row4_bypass_probe "$CPU_ADMIT" "$_ROW4_PSI_FIXTURE" "$_ROW4_BYPASS_TIMEOUT_S" "$_ROW4_BYPASS_STDERR" \
    || _ROW4_BYPASS_RC=$?
_ROW4_BYPASS_END=$(date +%s)
# DIAGNOSTIC-ONLY: kept in the assert description below purely for
# debuggability. MUST NOT become a discriminator -- the two verdicts are the
# rc (this assert) and the ROW4-3 structural marker below.
_ROW4_BYPASS_ELAPSED=$(( _ROW4_BYPASS_END - _ROW4_BYPASS_START ))
assert "ROW4-2: DF_VERIFY_ROLE=merge + avg10=99 PSI → cpu-admit admit exits 0 fast (rc=${_ROW4_BYPASS_RC}, elapsed=${_ROW4_BYPASS_ELAPSED}s)" \
    test "${_ROW4_BYPASS_RC}" -eq 0

# ROW4-3: structural discriminator (task 6000, porting the #4844 treatment
# from test_cpu_admit.sh:255-258 Cycle D). rc alone cannot tell a genuine
# merge bypass apart from a cpu-admit that keeps returning 0 while silently
# no longer taking the bypass path (ROW4-BYPASS-VACUITY-1c pins this
# hermetically) -- the stderr marker is what actually discriminates.
# grep -qF (fixed-string): the marker contains literal parens, and matches
# BOTH cpu-admit.sh branches (bare, and the " — timestamp bumped" variant at
# cpu-admit.sh:226).
assert "ROW4-3: DF_VERIFY_ROLE=merge → cpu-admit stderr marks 'bypass (role=merge)' (structural discriminator; see test_cpu_admit.sh Cycle D / task 4844)" \
    bash -c 'grep -qF "bypass (role=merge)" "$1"' _ "$_ROW4_BYPASS_STDERR"

# ── ROW4-BYPASS-VACUITY-1: the stderr marker DISCRIMINATES (task 6000) ─────
# Drives `_row4_bypass_probe` (defined above) against two synthetic "admit"
# stub scripts (never the real cpu-admit.sh), so this block is hermetic and
# needs no host PSI/cgroup support. Proves the structural stderr marker that
# the live ROW4-3 assertion (above) checks actually DISCRIMINATES — i.e.
# that an rc-only check (ROW4-2 alone) is BLIND to a cpu-admit that keeps
# returning 0 but stops emitting the marker.
#
# Capture idiom: "_rc=0; _row4_bypass_probe ... || _rc=$?" — MUST NOT be a
# bare "_x=\"\$(_row4_bypass_probe ...)\"" at top level. Under this file's
# `set -euo pipefail` (line 176), a helper that ever went missing again
# (renamed/removed out from under this block) would surface as "command not
# found" (rc 127); inside an `||` arm that is a clean per-assert FAIL, but
# unguarded at top level it would abort the ENTIRE suite instead — same
# capture-idiom hazard flagged at the _row4_sample_host_avg10 comment above
# (~line 2590).
_row4_bypass_faithful_stub="$(mktemp -p "$WORK" row4-bypass-stub-faithful.XXXXXX)"
printf '#!/usr/bin/env bash\necho "cpu-admit: bypass (role=merge)" >&2\nexit 0\n' \
    > "$_row4_bypass_faithful_stub"

_row4_bypass_mutant_stub="$(mktemp -p "$WORK" row4-bypass-stub-mutant.XXXXXX)"
printf '#!/usr/bin/env bash\nexit 0\n' > "$_row4_bypass_mutant_stub"

_row4_bypass_vacuity1_stderr="$(mktemp -p "$WORK" row4-bypass-vacuity1.XXXXXX)"
_row4_bypass_vacuity1a_rc=0
_row4_bypass_probe "$_row4_bypass_faithful_stub" "$_ROW4_PSI_FIXTURE" 5 "$_row4_bypass_vacuity1_stderr" \
    || _row4_bypass_vacuity1a_rc=$?
assert "ROW4-BYPASS-VACUITY-1a: faithful stub (echoes marker, exits 0) => probe rc is 0 (got ${_row4_bypass_vacuity1a_rc})" \
    test "$_row4_bypass_vacuity1a_rc" -eq 0
assert "ROW4-BYPASS-VACUITY-1b: faithful stub => captured stderr matches 'bypass (role=merge)'" \
    bash -c 'grep -qF "bypass (role=merge)" "$1"' _ "$_row4_bypass_vacuity1_stderr"

# (1c) THE discriminator. Poison-seed the stderr file with the marker text
# BEFORE probing: _row4_bypass_probe's own `2>"$stderr_file"` redirection
# truncates it the instant the child is launched, so a correctly-behaving
# probe always overwrites the seed with the mutant stub's REAL (empty)
# stderr. The poison seed proves this rewrite actually happens: without it,
# a broken redirect that left the file untouched would still pass the "must
# NOT match" assert below vacuously against a pre-existing empty file, for
# the wrong reason entirely. Same fail-vacuously hazard as the sampler
# capture idiom above, just on the file side rather than the variable side.
_row4_bypass_vacuity1c_stderr="$(mktemp -p "$WORK" row4-bypass-vacuity1c.XXXXXX)"
printf 'bypass (role=merge)\n' > "$_row4_bypass_vacuity1c_stderr"
_row4_bypass_vacuity1c_rc=0
_row4_bypass_probe "$_row4_bypass_mutant_stub" "$_ROW4_PSI_FIXTURE" 5 "$_row4_bypass_vacuity1c_stderr" \
    || _row4_bypass_vacuity1c_rc=$?
assert "ROW4-BYPASS-VACUITY-1c: mutant stub (no marker, still exits 0) => probe rc is STILL 0 (got ${_row4_bypass_vacuity1c_rc}; rc alone cannot tell this apart from the faithful stub)" \
    test "$_row4_bypass_vacuity1c_rc" -eq 0
assert "ROW4-BYPASS-VACUITY-1c: mutant stub => captured stderr does NOT match 'bypass (role=merge)' (the discriminator the new live ROW4-3 assertion exists to catch)" \
    bash -c '! grep -qF "bypass (role=merge)" "$1"' _ "$_row4_bypass_vacuity1c_stderr"

# ── ROW4-BYPASS-VACUITY-2: the wall-clock T-treatment drivers (task 6000) ──
# THE task-6000 defect, now fixed: the RETIRED literal-5s budget was only
# ~7x the measured real cost (713 ms on a loadavg-~190/32-core box) -- a
# bypass that is merely slow to START (process-spawn latency under load)
# could blow that budget and FAIL a genuinely correct bypass. -2a/-2b pin
# that a slow-to-start-but-correct bypass now passes under the live,
# knob-backed budget (_ROW4_BYPASS_TIMEOUT_S, default 120); -2c proves the
# anti-hang guard this T-treatment keeps still fires on a real hang; -2d pins
# that the built-in default stays generous enough to never discriminate.
_ROW4_BYPASS_SLOW_S="${REIFY_CPU_GOV_TEST_ROW4_BYPASS_SLOW_S:-6}"
# Strictly greater than the RETIRED 5 s bound, so a bypass that is merely
# slow to start (process-spawn latency on an oversubscribed host) is proven
# not to flip the verdict. This default (6) is coupled to the knob-header
# doc above (REIFY_CPU_GOV_TEST_ROW4_BYPASS_SLOW_S) -- if it ever changes,
# move the header line and this rationale together, or the two silently
# drift apart again (task 6000 review-cycle-1 caught exactly that drift:
# the header briefly said "default 1" while this line stayed "6").
_row4_bypass_slow_stub="$(mktemp -p "$WORK" row4-bypass-stub-slow.XXXXXX)"
printf '#!/usr/bin/env bash\nsleep "%s"\necho "cpu-admit: bypass (role=merge)" >&2\nexit 0\n' \
    "$_ROW4_BYPASS_SLOW_S" > "$_row4_bypass_slow_stub"

_row4_bypass_hanging_stub="$(mktemp -p "$WORK" row4-bypass-stub-hang.XXXXXX)"
printf '#!/usr/bin/env bash\nsleep 30\n' > "$_row4_bypass_hanging_stub"

# (2a) THE task-6000 driver: the SLOW-but-correct stub probed under the LIVE
# budget. RED after step-2 (budget=5 < sleep=6 => timeout kills it => 124);
# GREEN once step-4 widens the budget past the sleep.
_row4_bypass_vacuity2_stderr="$(mktemp -p "$WORK" row4-bypass-vacuity2.XXXXXX)"
_row4_bypass_vacuity2_rc=0
_row4_bypass_probe "$_row4_bypass_slow_stub" "$_ROW4_PSI_FIXTURE" "$_ROW4_BYPASS_TIMEOUT_S" "$_row4_bypass_vacuity2_stderr" \
    || _row4_bypass_vacuity2_rc=$?
assert "ROW4-BYPASS-VACUITY-2a: slow-but-correct stub (sleeps ${_ROW4_BYPASS_SLOW_S}s) under the LIVE budget (_ROW4_BYPASS_TIMEOUT_S=${_ROW4_BYPASS_TIMEOUT_S}) => probe rc is 0 (got ${_row4_bypass_vacuity2_rc}; a too-tight budget false-REDs a merely-slow-to-start bypass)" \
    test "$_row4_bypass_vacuity2_rc" -eq 0

# (2b) Same run: once it is allowed to finish, its stderr still carries the
# marker -- the slowness was ONLY in starting, never in what it reports.
assert "ROW4-BYPASS-VACUITY-2b: slow-but-correct stub => captured stderr matches 'bypass (role=merge)' once given enough budget to finish" \
    bash -c 'grep -qF "bypass (role=merge)" "$1"' _ "$_row4_bypass_vacuity2_stderr"

# (2c) The anti-hang guard stays REAL: a genuinely hanging admit, probed with
# a deliberately tiny EXPLICIT timeout (never the live budget), still gets
# killed. Permanently green -- pins that widening the live budget (step-4)
# never disables the guard itself, per the T-treatment
# (docs/prds/infra-test-wallclock-deflake.md:33). Costs ~1s.
_row4_bypass_vacuity2c_stderr="$(mktemp -p "$WORK" row4-bypass-vacuity2c.XXXXXX)"
_row4_bypass_vacuity2c_rc=0
_row4_bypass_probe "$_row4_bypass_hanging_stub" "$_ROW4_PSI_FIXTURE" 1 "$_row4_bypass_vacuity2c_stderr" \
    || _row4_bypass_vacuity2c_rc=$?
assert "ROW4-BYPASS-VACUITY-2c: hanging stub (sleep 30) under an explicit tiny 1s timeout => probe rc is 124 (got ${_row4_bypass_vacuity2c_rc}; the anti-hang guard still fires on a real hang)" \
    test "$_row4_bypass_vacuity2c_rc" -eq 124

# (2d) A SOURCE-LEVEL regression pin on the built-in default constant --
# literal-vs-literal at runtime BY DESIGN, not a check of any SUT behaviour.
# The cycle's actual SUT-behaviour discriminators are ROW4-2 (rc), ROW4-3
# (structural stderr marker), and 2a/2b/2c (the slow-start / real-hang
# drivers above); this pin exists only so a future edit cannot silently
# retighten _ROW4_BYPASS_TIMEOUT_S's built-in default back toward a value
# that could discriminate. It must NOT fire against a deliberate operator
# override, so it is gated behind _row4_bypass_floor_applies (defined above,
# alongside _row4_bypass_probe) -- ROW4-BYPASS-VACUITY-3a..c hermetically
# pin that gate's SKIP-vs-ASSERT decision in both directions, which matters
# here because a single straight-line run can only ever execute ONE arm of
# this `if`.
if _row4_bypass_floor_applies "${REIFY_CPU_GOV_TEST_ROW4_BYPASS_TIMEOUT_S:-}"; then
    assert "ROW4-BYPASS-VACUITY-2d: live budget (_ROW4_BYPASS_TIMEOUT_S=${_ROW4_BYPASS_TIMEOUT_S}) is generously >= 60s -- >= 84x the measured 713ms real cost, so it cannot discriminate pass/fail" \
        test "$_ROW4_BYPASS_TIMEOUT_S" -ge 60
else
    echo "  SKIP ROW4-BYPASS-VACUITY-2d: budget overridden (${_ROW4_BYPASS_TIMEOUT_S}s) -- the >=60s floor regression-pins the built-in default, not an operator override"
fi

# ── ROW4-BYPASS-VACUITY-3: the 2d floor's SKIP-vs-ASSERT decision (task 6000
# review fix, blocking issue) ──────────────────────────────────────────────
# THE DEFECT: the knob header above (:172-180) documents that overriding
# REIFY_CPU_GOV_TEST_ROW4_BYPASS_TIMEOUT_S makes ROW4-BYPASS-VACUITY-2d SKIP
# rather than FAIL -- but the live 2d assert above is unconditional: it
# asserts >=60s against the RESOLVED value with no SKIP branch. A blessed
# override (e.g. REIFY_CPU_GOV_TEST_ROW4_BYPASS_TIMEOUT_S=30, a tighter CI
# budget) turns this ALWAYS-ON hermetic cycle RED in a merge-gate suite for a
# configuration the header explicitly blesses.
#
# Drives a not-yet-defined `_row4_bypass_floor_applies <raw_knob_value>`:
# returns 0 (floor APPLIES -> 2d asserts) iff <raw_knob_value> is empty --
# i.e. the operator has NOT overridden the budget knob and the live value is
# therefore the BUILT-IN DEFAULT the floor pins. Returns EXACTLY 1 (floor
# SKIPs) for any non-empty override, whatever its magnitude -- the
# discriminator is "was it overridden", never "is the value large".
#
# Capture idiom: "_rc=0; _row4_bypass_floor_applies ... || _rc=$?" -- MUST
# NOT be invoked bare at top level. Under this file's `set -euo pipefail`
# (line 193), a `1` verdict (floor SKIPs) would abort the ENTIRE suite
# instead of producing a clean per-assert FAIL -- same capture-idiom hazard
# flagged at the _row4_sample_host_avg10 (~line 2603) and _row4_bypass_probe
# (~line 2980) comments.
#
# 3b/3c assert the EXACT skip verdict `-eq 1`, never `-ne 0`: with the
# predicate still undefined here the rc is 127, which SATISFIES `-ne 0` --
# written that way 3b/3c would pass VACUOUSLY in this very RED step, losing
# the signal for exactly the two review-fix cases. Same fail-vacuously
# hazard the file already guards against with the VACUITY-1c poison seed
# above (~line 3004). 3a's `-eq 0` correctly FAILs at 127.

# (3a) The regression-pin arm stays REACHABLE on the default path: the new
# SKIP branch must not swallow the pin entirely. One input, two real
# scenarios collapse to it -- the knob unset AND an explicitly-empty
# override -- because the call site expands
# "${REIFY_CPU_GOV_TEST_ROW4_BYPASS_TIMEOUT_S:-}" and the budget expands
# "${...:-120}"; both map either scenario to the built-in default, which is
# exactly what the floor pins.
_row4_bypass_vacuity3a_rc=0
_row4_bypass_floor_applies "" || _row4_bypass_vacuity3a_rc=$?
assert "ROW4-BYPASS-VACUITY-3a: unset/empty override => floor APPLIES (got rc=${_row4_bypass_vacuity3a_rc}, want 0)" \
    test "$_row4_bypass_vacuity3a_rc" -eq 0

# (3b) THE review-fix driver: the documented, supported tighter-CI-budget
# override must no longer reach the floor assert.
_row4_bypass_vacuity3b_rc=0
_row4_bypass_floor_applies "30" || _row4_bypass_vacuity3b_rc=$?
assert "ROW4-BYPASS-VACUITY-3b: tight blessed override (30) => floor SKIPs (got rc=${_row4_bypass_vacuity3b_rc}, want EXACTLY 1)" \
    test "$_row4_bypass_vacuity3b_rc" -eq 1

# (3c) Pins that the discriminator is "was it overridden", never "is the
# value large" -- blocks a later "simplification" into a value comparison,
# which would silently re-open 3b's false RED while leaving 3a/3b green.
_row4_bypass_vacuity3c_rc=0
_row4_bypass_floor_applies "240" || _row4_bypass_vacuity3c_rc=$?
assert "ROW4-BYPASS-VACUITY-3c: generous override (240) => floor STILL SKIPs (got rc=${_row4_bypass_vacuity3c_rc}, want EXACTLY 1)" \
    test "$_row4_bypass_vacuity3c_rc" -eq 1

# ============================================================================
# Cycle CLASSIFY — run_all.sh classification-manifest self-check (H5, task
# 4926; always-on, hermetic — no host/PSI/cgroup precondition, pure file
# reads). Proves this file is declared `host-exclusive` (not `pool`) in
# run-all-classification.manifest — task 4997 (fa7fbc3481) reclassified it
# host-exclusive and superseded the earlier H5 pool-rescue (run-all-host-infra-
# partition.md S3); this self-check tracks that CURRENT intent. (Stale until
# task 5011: 4997 flipped the manifest bucket but left this block asserting
# `pool`, which failed on the leo-laptop merge host — where host-exclusive
# tests still run — while staying dormant on the hot path that sets
# REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1.) Whole-manifest drift (declared-union == discovered set, no
# overlap, every entry resolves) is enforced by the standalone
# test_run_all_classification.sh, which already runs as its own
# independent pool test — re-running it here would couple this file's
# health to unrelated manifest changes elsewhere in tests/infra/, so only
# this-file-specific bucket membership is asserted.
# ============================================================================
echo ""
echo "--- Cycle CLASSIFY: run_all.sh manifest self-classification (always-on) ---"

_CLASSIFY_SELF="$(basename "${BASH_SOURCE[0]}")"

if [ ! -f "$CLASSIFICATION_LIB" ]; then
    echo "  SKIP CLASSIFY: run-all-classification-lib.sh not found at $CLASSIFICATION_LIB"
else
    assert "CLASSIFY-1: ${_CLASSIFY_SELF} is declared host-exclusive in run-all-classification.manifest" \
        bash -c '
            source "$1"
            classification_bucket host-exclusive | grep -qxF -- "$2"
        ' _ "$CLASSIFICATION_LIB" "$_CLASSIFY_SELF"

    assert "CLASSIFY-2: ${_CLASSIFY_SELF} is NOT declared pool in run-all-classification.manifest" \
        bash -c '
            source "$1"
            ! classification_bucket pool | grep -qxF -- "$2"
        ' _ "$CLASSIFICATION_LIB" "$_CLASSIFY_SELF"
fi

# ---------------------------------------------------------------------------
# Final summary — PASS/FAIL count from test_helpers.sh.
# ---------------------------------------------------------------------------
test_summary
