#!/usr/bin/env bash
# tests/infra/test_cpu_governed_exec_hostexcl.sh — host-exclusive real-scope-
# placement tests for scripts/cpu-governed-exec.sh (cgroup-v2 cpu.weight
# placement, task 4632).
#
# This file is the host-exclusive residue of the H6 split (task 4927; see
# docs/prds/run-all-host-infra-partition.md §11 decision (b)): D1-D8 place a
# REAL systemd-run --user scope under real cgroup delegation, so they must
# run on a host with cgroup governance and cannot run concurrently in the
# hermetic pool. See the sibling tests/infra/test_cpu_governed_exec.sh for
# the hermetic A (arg-contract) / B (lib_cgroup.sh detection) / C (fail-open)
# coverage — this file duplicates only what D-here needs (host_supports_
# governance(), $WORK, the #4919 $$-scoped slice isolation).
#
# Test coverage:
#   D: governed cgroup placement (scope, weight, cpu.max, exit code) host-gated
#
# Auto-discovered by tests/infra/run_all.sh (glob test_*.sh).
# No cargo/npm builds — pure shell/cgroup hermetic assertions (host-gated).

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

echo "=== cpu-governed-exec.sh tests (task 4632) — host-exclusive real-scope placement (D) ==="

# ---------------------------------------------------------------------------
# Hermetic workdir — cleaned up on exit.
# ---------------------------------------------------------------------------
WORK="$(mktemp -d)"
# Per-run unique slice names used for D7/D8 isolation (avoid cross-test
# races on the shared reify-governed-agents/merge slices — see D7 comment).
D7_TASK_SLICE="reify-test-task-$$.slice"
D8_MERGE_SLICE="reify-test-merge-$$.slice"

# Per-run unique PRIVATE slice hierarchy for D1-D6 isolation (task 4919):
# shared-parent $$-scoped slices so cpu-governed-exec.sh's slice-weight-set +
# scope placement never touch the live production reify-governed-*.slice
# hierarchy (which the running orchestrator's governance depends on).
# systemd dash-nesting (a-b-c.slice -> a.slice/a-b.slice/a-b-c.slice) gives
# task+merge a COMMON parent (D_PARENT_SLICE), faithfully mirroring the
# production reify-governed.slice/reify-governed-<agents|merge>.slice
# two-level hierarchy that D1a/D3a assert.
D_PARENT_SLICE="reify-test$$.slice"
D_TASK_SLICE="reify-test$$-agents.slice"
D_MERGE_SLICE="reify-test$$-merge.slice"
# grep-BRE dot-escaped forms (a literal "." in a slice name must not match
# "any char" when used inside a grep pattern).
D_PARENT_SLICE_RE="${D_PARENT_SLICE//./\\.}"
D_TASK_SLICE_RE="${D_TASK_SLICE//./\\.}"
D_MERGE_SLICE_RE="${D_MERGE_SLICE//./\\.}"

trap 'rm -rf "$WORK"; systemctl --user stop "$D7_TASK_SLICE" "$D8_MERGE_SLICE" "$D_TASK_SLICE" "$D_MERGE_SLICE" "$D_PARENT_SLICE" 2>/dev/null || true' EXIT

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
# D: governed cgroup-placement assertions — host-gated.
# (added in step-7 of task 4632; extracted to this host-exclusive sibling in
# task 4927/H6 — see docs/prds/run-all-host-infra-partition.md §11 decision (b))
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

    # D1: --role task → scope under a private, $$-scoped task slice (task 4919:
    # isolated from the production reify-governed-agents.slice).
    REIFY_CPU_GOVERN_SLICE_TASK="$D_TASK_SLICE" bash "$WRAPPER" --role task -- bash -c "$PROBE" > "$WORK/out_task" 2>/dev/null || true
    assert "D1a: --role task → cgroup under private $D_PARENT_SLICE/$D_TASK_SLICE" \
        bash -c '
            grep -q "CGROUP=.*$2/$3/" "$1"
        ' _ "$WORK/out_task" "$D_PARENT_SLICE_RE" "$D_TASK_SLICE_RE"
    assert "D1b: --role task → cgroup ends in .scope" \
        bash -c '
            grep -qE "CGROUP=.*\.scope$" "$1"
        ' _ "$WORK/out_task"

    # D2: --role task → WEIGHT == 100.
    assert "D2: --role task → WEIGHT==100" \
        bash -c '
            grep -q "^WEIGHT=100$" "$1"
        ' _ "$WORK/out_task"

    # D3: --role merge → scope under a private, $$-scoped merge slice (task 4919:
    # isolated from the production reify-governed-merge.slice) and WEIGHT==300.
    REIFY_CPU_GOVERN_SLICE_MERGE="$D_MERGE_SLICE" bash "$WRAPPER" --role merge -- bash -c "$PROBE" > "$WORK/out_merge" 2>/dev/null || true
    assert "D3a: --role merge → cgroup under private $D_PARENT_SLICE/$D_MERGE_SLICE" \
        bash -c '
            grep -q "CGROUP=.*$2/$3/" "$1"
        ' _ "$WORK/out_merge" "$D_PARENT_SLICE_RE" "$D_MERGE_SLICE_RE"
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
    REIFY_CPU_GOVERN_W_TASK=250 REIFY_CPU_GOVERN_SLICE_TASK="$D_TASK_SLICE" bash "$WRAPPER" --role task -- bash -c "$PROBE" > "$WORK/out_task_custom" 2>/dev/null || true
    assert "D5: REIFY_CPU_GOVERN_W_TASK=250 → WEIGHT==250 (role value, not default)" \
        bash -c '
            grep -q "^WEIGHT=250$" "$1"
        ' _ "$WORK/out_task_custom"

    # D6: exit code propagation through governed path.
    REIFY_CPU_GOVERN_SLICE_TASK="$D_TASK_SLICE" assert "D6: governed path propagates exit 7" \
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

    # D-guard: none of the captured probe outputs may reference a production
    # reify-governed-*.slice — deterministic proof that D1-D6 (and D7/D8) never
    # touch the live orchestrator's governance hierarchy (task 4919). A live
    # `systemctl --user list-units 'reify-governed*'` before/after diff would be
    # racy against a concurrently running orchestrator; grepping the captured
    # probe outputs is deterministic and stays in-process.
    for _f in out_task out_merge out_task_custom out_d7 out_d8; do
        assert "D-guard: $_f references no production reify-governed slice" \
            bash -c '! grep -q "reify-governed" "$1"' _ "$WORK/$_f"
    done
fi

test_summary
