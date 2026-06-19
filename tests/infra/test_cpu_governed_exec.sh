#!/usr/bin/env bash
# tests/infra/test_cpu_governed_exec.sh — integration tests for
# scripts/cpu-governed-exec.sh (cgroup-v2 cpu.weight placement, task 4632).
#
# Test coverage added incrementally (TDD steps):
#   A: arg-contract (EX_USAGE=64 on bad/missing args)               host-independent
#   B: lib_cgroup.sh detection/resolution                            host-independent (fixture)
#   C: fail-open / degrade execution                                 host-independent (fixture)
#   D: governed cgroup placement (scope, weight, cpu.max, exit code) host-gated
#
# Auto-discovered by tests/infra/run_all.sh (glob test_*.sh).
# No cargo/npm builds — pure shell/cgroup hermetic assertions.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
WRAPPER="$REPO_ROOT/scripts/cpu-governed-exec.sh"
LIB="$REPO_ROOT/scripts/lib_cgroup.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== cpu-governed-exec.sh tests (task 4632) ==="

# ---------------------------------------------------------------------------
# Hermetic workdir — cleaned up on exit.
# ---------------------------------------------------------------------------
WORK="$(mktemp -d)"
# Per-run unique slice names used for D7/D8 isolation (avoid cross-test
# races on the shared reify-governed-agents/merge slices — see D7 comment).
D7_TASK_SLICE="reify-test-task-$$.slice"
D8_MERGE_SLICE="reify-test-merge-$$.slice"
trap 'rm -rf "$WORK"; systemctl --user stop "$D7_TASK_SLICE" "$D8_MERGE_SLICE" 2>/dev/null || true' EXIT

# Degrade fixture: controllers file that lacks the 'cpu' token (simulates an
# undelegated host), used to force the degrade path deterministically.
echo "memory pids" > "$WORK/controllers_no_cpu"

# ---------------------------------------------------------------------------
# host_supports_governance — gate helper for the host-dependent green path.
# Sources lib_cgroup.sh and calls cgroup_governance_supported with no
# overrides (real detection against the live host).
# Returns 0 if the host can run governed placement, 1 otherwise.
# ---------------------------------------------------------------------------
host_supports_governance() {
    [ -f "$LIB" ] || return 1
    # Source in a subshell to avoid polluting global env.
    (
        # shellcheck source=scripts/lib_cgroup.sh
        source "$LIB"
        cgroup_governance_supported
    )
}

# ---------------------------------------------------------------------------
# A: arg-contract assertions — EX_USAGE=64, host-independent.
# (added in step-1)
# ---------------------------------------------------------------------------
echo ""
echo "--- A: arg-contract (EX_USAGE=64) ---"

# (a) No args at all.
assert "A1: no args → exit 64" \
    bash -c '
        out=$(bash "$1" 2>&1); rc=$?
        [ "$rc" -eq 64 ] && printf "%s\n" "$out" | grep -qi "usage"
    ' _ "$WRAPPER"

# (b) Missing --role entirely (only -- CMD given).
assert "A2: missing --role → exit 64" \
    bash -c '
        out=$(bash "$1" -- true 2>&1); rc=$?
        [ "$rc" -eq 64 ] && printf "%s\n" "$out" | grep -qi "usage"
    ' _ "$WRAPPER"

# (c) Invalid role value.
assert "A3: --role bogus → exit 64" \
    bash -c '
        out=$(bash "$1" --role bogus -- true 2>&1); rc=$?
        [ "$rc" -eq 64 ] && printf "%s\n" "$out" | grep -qi "usage"
    ' _ "$WRAPPER"

# (d) Missing -- separator (role given but no separator).
assert "A4: missing -- separator → exit 64" \
    bash -c '
        out=$(bash "$1" --role task true 2>&1); rc=$?
        [ "$rc" -eq 64 ] && printf "%s\n" "$out" | grep -qi "usage"
    ' _ "$WRAPPER"

# (e) --role given and -- present but no command after --.
assert "A5: --role task -- (no cmd) → exit 64" \
    bash -c '
        out=$(bash "$1" --role task -- 2>&1); rc=$?
        [ "$rc" -eq 64 ] && printf "%s\n" "$out" | grep -qi "usage"
    ' _ "$WRAPPER"

# ---------------------------------------------------------------------------
# B: lib_cgroup.sh detection/resolution assertions.
# (added in step-3)
# ---------------------------------------------------------------------------
echo ""
echo "--- B: lib_cgroup.sh detection/resolution ---"

# B1: cgroup_role_weight defaults.
assert "B1a: cgroup_role_weight task == 100 (default)" \
    bash -c '
        source "$1"
        w=$(cgroup_role_weight task)
        [ "$w" = "100" ]
    ' _ "$LIB"

assert "B1b: cgroup_role_weight merge == 300 (default)" \
    bash -c '
        source "$1"
        w=$(cgroup_role_weight merge)
        [ "$w" = "300" ]
    ' _ "$LIB"

# B2: weight env overrides.
assert "B2a: REIFY_CPU_GOVERN_W_TASK=250 overrides task weight" \
    bash -c '
        source "$1"
        w=$(REIFY_CPU_GOVERN_W_TASK=250 cgroup_role_weight task)
        [ "$w" = "250" ]
    ' _ "$LIB"

assert "B2b: REIFY_CPU_GOVERN_W_MERGE=400 overrides merge weight" \
    bash -c '
        source "$1"
        w=$(REIFY_CPU_GOVERN_W_MERGE=400 cgroup_role_weight merge)
        [ "$w" = "400" ]
    ' _ "$LIB"

# B3: non-integer weight rejected with non-zero exit.
assert "B3: non-integer REIFY_CPU_GOVERN_W_TASK rejected (non-zero)" \
    bash -c '
        source "$1"
        ! (REIFY_CPU_GOVERN_W_TASK=abc cgroup_role_weight task 2>/dev/null)
    ' _ "$LIB"

# B4: cgroup_role_slice defaults.
assert "B4a: cgroup_role_slice task == reify-governed-agents.slice" \
    bash -c '
        source "$1"
        s=$(cgroup_role_slice task)
        [ "$s" = "reify-governed-agents.slice" ]
    ' _ "$LIB"

assert "B4b: cgroup_role_slice merge == reify-governed-merge.slice" \
    bash -c '
        source "$1"
        s=$(cgroup_role_slice merge)
        [ "$s" = "reify-governed-merge.slice" ]
    ' _ "$LIB"

# B5: slice env overrides.
assert "B5a: REIFY_CPU_GOVERN_SLICE_TASK override honored" \
    bash -c '
        source "$1"
        s=$(REIFY_CPU_GOVERN_SLICE_TASK=custom-task.slice cgroup_role_slice task)
        [ "$s" = "custom-task.slice" ]
    ' _ "$LIB"

assert "B5b: REIFY_CPU_GOVERN_SLICE_MERGE override honored" \
    bash -c '
        source "$1"
        s=$(REIFY_CPU_GOVERN_SLICE_MERGE=custom-merge.slice cgroup_role_slice merge)
        [ "$s" = "custom-merge.slice" ]
    ' _ "$LIB"

# B6: cgroup_governance_supported returns non-zero when DISABLE=1.
assert "B6: REIFY_CPU_GOVERN_DISABLE=1 → governance unsupported (non-zero)" \
    bash -c '
        source "$1"
        ! (REIFY_CPU_GOVERN_DISABLE=1 cgroup_governance_supported 2>/dev/null)
    ' _ "$LIB"

# B7: cgroup_governance_supported returns non-zero when controllers file has no cpu.
assert "B7: no-cpu controllers fixture → governance unsupported" \
    bash -c '
        source "$1"
        ! (REIFY_CPU_GOVERN_CONTROLLERS_PATH="$2" cgroup_governance_supported 2>/dev/null)
    ' _ "$LIB" "$WORK/controllers_no_cpu"

# B8: (host-gated) governance supported on the real delegated host.
if host_supports_governance; then
    assert "B8: real host → cgroup_governance_supported returns 0" \
        bash -c '
            source "$1"
            cgroup_governance_supported
        ' _ "$LIB"
else
    echo "  SKIP B8: host does not support cgroup governance (fail-open host)"
fi

# ---------------------------------------------------------------------------
# C: fail-open / degrade assertions — host-independent.
# (added in step-5)
# ---------------------------------------------------------------------------
echo ""
echo "--- C: fail-open / degrade (host-independent) ---"

# C1: DISABLE=1 → command executes (SENTINEL in stdout), warning on stderr.
assert "C1: REIFY_CPU_GOVERN_DISABLE=1 → execs command (fail-open)" \
    bash -c '
        out=$(REIFY_CPU_GOVERN_DISABLE=1 bash "$1" --role task -- bash -c "echo SENTINEL" 2>/dev/null)
        printf "%s\n" "$out" | grep -q "SENTINEL"
    ' _ "$WRAPPER"

assert "C2: REIFY_CPU_GOVERN_DISABLE=1 → emits degrade/bypass warning to stderr" \
    bash -c '
        err=$(REIFY_CPU_GOVERN_DISABLE=1 bash "$1" --role task -- bash -c "echo SENTINEL" 2>&1 >/dev/null)
        printf "%s\n" "$err" | grep -qiE "(degrad|bypass|warn)"
    ' _ "$WRAPPER"

assert "C3: REIFY_CPU_GOVERN_DISABLE=1 → exits 0" \
    bash -c '
        REIFY_CPU_GOVERN_DISABLE=1 bash "$1" --role task -- bash -c "echo SENTINEL" >/dev/null 2>&1
    ' _ "$WRAPPER"

# C4: controllers-fixture absent cpu → fail-open exec.
assert "C4: no-cpu controllers fixture → execs command (fail-open)" \
    bash -c '
        out=$(REIFY_CPU_GOVERN_CONTROLLERS_PATH="$2" bash "$1" --role task -- bash -c "echo SENTINEL" 2>/dev/null)
        printf "%s\n" "$out" | grep -q "SENTINEL"
    ' _ "$WRAPPER" "$WORK/controllers_no_cpu"

# C5: exit-code preservation through degrade path.
assert "C5: degrade path propagates exit 7" \
    bash -c '
        REIFY_CPU_GOVERN_DISABLE=1 bash "$1" --role task -- bash -c "exit 7" >/dev/null 2>&1
        rc=$?
        [ "$rc" -eq 7 ]
    ' _ "$WRAPPER"

# ---------------------------------------------------------------------------
# D: governed cgroup-placement assertions — host-gated.
# (added in step-7)
# ---------------------------------------------------------------------------
echo ""
echo "--- D: governed cgroup placement (host-gated) ---"

if ! host_supports_governance; then
    echo "  SKIP D: host does not support cgroup governance — skipping placement assertions"
else
    # Probe command: writes cgroup path, scope cpu.weight, cpu.max, and the
    # parent SLICE cpu.weight to a file.  SLICE_WEIGHT reads the parent cgroup
    # (the role slice), which is the cross-role lever driving proportional
    # sharing between lanes (C-G2) — distinct from the scope weight (C-G3).
    PROBE='
rel=$(sed "s/^0:://" /proc/self/cgroup)
slice_rel="${rel%/*}"
echo CGROUP="$rel"
echo WEIGHT=$(cat /sys/fs/cgroup"$rel"/cpu.weight)
echo MAX=$(cat /sys/fs/cgroup"$rel"/cpu.max)
echo SLICE_WEIGHT=$(cat /sys/fs/cgroup"$slice_rel"/cpu.weight 2>/dev/null || echo MISSING)
'

    # D1: --role task → scope under reify-governed-agents.slice.
    bash "$WRAPPER" --role task -- bash -c "$PROBE" > "$WORK/out_task" 2>/dev/null || true
    assert "D1a: --role task → cgroup under reify-governed.slice/reify-governed-agents.slice" \
        bash -c '
            grep -q "CGROUP=.*reify-governed\.slice/reify-governed-agents\.slice/" "$1"
        ' _ "$WORK/out_task"
    assert "D1b: --role task → cgroup ends in .scope" \
        bash -c '
            grep -qE "CGROUP=.*\.scope$" "$1"
        ' _ "$WORK/out_task"

    # D2: --role task → WEIGHT == 100.
    assert "D2: --role task → WEIGHT==100" \
        bash -c '
            grep -q "^WEIGHT=100$" "$1"
        ' _ "$WORK/out_task"

    # D3: --role merge → scope under reify-governed-merge.slice and WEIGHT==300.
    bash "$WRAPPER" --role merge -- bash -c "$PROBE" > "$WORK/out_merge" 2>/dev/null || true
    assert "D3a: --role merge → cgroup under reify-governed.slice/reify-governed-merge.slice" \
        bash -c '
            grep -q "CGROUP=.*reify-governed\.slice/reify-governed-merge\.slice/" "$1"
        ' _ "$WORK/out_merge"
    assert "D3b: --role merge → WEIGHT==300" \
        bash -c '
            grep -q "^WEIGHT=300$" "$1"
        ' _ "$WORK/out_merge"

    # D4: cpu.max first field == "max" (work-conserving, C-G1).
    # Kernel renders "max 100000" — check first token only, NOT full-string.
    assert "D4: cpu.max first field == max (work-conserving, no quota)" \
        bash -c '
            max_line=$(grep "^MAX=" "$1" | cut -d= -f2-)
            first_field="${max_line%% *}"
            [ "$first_field" = "max" ]
        ' _ "$WORK/out_task"

    # D5: custom weight override (REIFY_CPU_GOVERN_W_TASK=250 → WEIGHT==250).
    REIFY_CPU_GOVERN_W_TASK=250 bash "$WRAPPER" --role task -- bash -c "$PROBE" > "$WORK/out_task_custom" 2>/dev/null || true
    assert "D5: REIFY_CPU_GOVERN_W_TASK=250 → WEIGHT==250 (role value, not default)" \
        bash -c '
            grep -q "^WEIGHT=250$" "$1"
        ' _ "$WORK/out_task_custom"

    # D6: exit code propagation through governed path.
    assert "D6: governed path propagates exit 7" \
        bash -c '
            bash "$1" --role task -- bash -c "exit 7" >/dev/null 2>&1
            rc=$?
            [ "$rc" -eq 7 ]
        ' _ "$WRAPPER"

    # D7: slice cpu.weight for task role — the C-G2 cross-role lever.
    # Uses a per-run unique isolated slice ($D7_TASK_SLICE) to avoid a
    # cross-test race: concurrent test runs share the default slice names and
    # can change their weights between cgroup_set_slice_weight and the PROBE
    # read, producing a transient false-negative.  A unique per-PID slice is
    # guaranteed cold on entry and cannot be touched by concurrent tests.
    # This still verifies the same property: that cpu-governed-exec.sh --role
    # task correctly pre-weights the role slice to 100 via cgroup_set_slice_weight.
    REIFY_CPU_GOVERN_SLICE_TASK="$D7_TASK_SLICE" bash "$WRAPPER" --role task -- bash -c "$PROBE" > "$WORK/out_d7" 2>/dev/null || true
    assert "D7: task slice (isolated) cpu.weight == 100" \
        bash -c '
            grep -q "^SLICE_WEIGHT=100$" "$1"
        ' _ "$WORK/out_d7"

    # D8: merge slice cpu.weight — same isolation rationale as D7.
    # Verifies cold-start 300 (not systemd default 100) thanks to the
    # start-then-set-property sequence in cgroup_set_slice_weight.
    REIFY_CPU_GOVERN_SLICE_MERGE="$D8_MERGE_SLICE" bash "$WRAPPER" --role merge -- bash -c "$PROBE" > "$WORK/out_d8" 2>/dev/null || true
    assert "D8: merge slice (isolated) cpu.weight == 300" \
        bash -c '
            grep -q "^SLICE_WEIGHT=300$" "$1"
        ' _ "$WORK/out_d8"
fi

test_summary
