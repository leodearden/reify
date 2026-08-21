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
# SLICE LIFECYCLE (task 6386). This file creates FIVE per-run systemd user
# units and owns them through tests/infra/govtest_slice_reaper_lib.sh — the
# same library test_cpu_load_governance.sh uses, parameterised on a `reify-test`
# profile. It supplies the naming, the UNCONDITIONAL EXIT-trap teardown, the
# startup sweep for predecessors killed by an uncatchable signal, and the
# one-off sweep for the legacy residue described next.
#
#   THE D7/D8 RENAME, AND THE LEAK IT CLOSES. D7/D8 were previously named
#   `reify-test-task-$$.slice` / `reify-test-merge-$$.slice`. systemd
#   dash-nesting means `a-b-c.slice` implies parents `a.slice` and `a-b.slice`,
#   vivified automatically and named by nothing — so every run silently
#   accreted `reify-test.slice`, `reify-test-task.slice` and
#   `reify-test-merge.slice`, which no teardown stopped and no pid-keyed sweep
#   could recognise. Measured on the host on 2026-08-21 all three were present,
#   `loaded active active`, and EMPTY (TasksCurrent=0). The two units are now
#   `reify-test$$-taskweight.slice` / `reify-test$$-mergeweight.slice`, so every
#   unit this run creates carries at most ONE dash segment after the pid and its
#   only implicit parents are `reify-test$$.slice` — which IS named, torn down,
#   and cascade-stops its children — and the shared `reify.slice`, which is
#   correctly never touched by anything here.
#
#   WHY reify.slice IS SAFE. Measured on the same host: `reify-test<pid>.slice`
#   parents to `reify.slice`, NOT to `reify-test.slice` (`reify-test1234` is ONE
#   dash segment, not `reify-test` + `1234`). `reify.slice` is also the shared
#   implicit root of the PRODUCTION hierarchy — `reify-governed.slice` and
#   `reify-governed-agents.slice` nest under it carrying live orchestrator agent
#   placement — so it is structurally unreachable from every stop path the
#   library exposes, by anchored grammar rather than by convention.
#
#   The five units must stay FIVE DISTINCT slices, merely re-parented:
#   cgroup_set_slice_weight sets the SLICE's own CPUWeight, so D5's
#   REIFY_CPU_GOVERN_W_TASK=250 leaves $D_TASK_SLICE at 250 and reusing it for
#   D7 would send that row's SLICE_WEIGHT==100 assertion red.
#
# Knobs:
#   REIFY_GOVTEST_TEST_MODE             ARMING KEY for the seam below (tasks
#                                       5930/6386). It is refused unless this
#                                       is 1. Nothing in the repo sets it except
#                                       test_govtest_slice_reaper.sh, so the
#                                       seam is inert on every real run no
#                                       matter what else is exported.
#   REIFY_CPU_GOVEXEC_TEST_LIFECYCLE_ONLY
#                                       with the arming key, exit 0 immediately
#                                       after the startup sweeps, with the EXIT
#                                       trap already installed so teardown still
#                                       fires. Exists so
#                                       test_govtest_slice_reaper.sh can prove
#                                       the slice lifecycle is WIRED by driving
#                                       this script for real without placing
#                                       five systemd-run scopes (which a stub
#                                       systemctl cannot intercept — the wrapper
#                                       shells out to systemd-run).
#                                       NEVER set in a normal run — it would
#                                       make this suite vacuously green, and
#                                       run_all.sh judges a member by exit code
#                                       alone. Set WITHOUT the arming key it is
#                                       refused loudly and the full suite runs.
#   REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS  with the arming key, replaces the
#                                       `kill -0` liveness oracle used by the
#                                       startup sweep (consumed in
#                                       govtest_slice_reaper_lib.sh).
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

# Slice lifecycle (task 6386): owns the naming, the unconditional teardown and
# the two startup sweeps for this run's private reify-test$$ slices. MANDATORY,
# the same missing-file-is-a-broken-checkout idiom as test_helpers.sh above and
# as test_cpu_load_governance.sh uses for this same library — without it this
# script would create five systemd user units it has no way to clean up, which
# is the leak the library exists to close.
GOVTEST_SLICE_REAPER_LIB="$SCRIPT_DIR/govtest_slice_reaper_lib.sh"
[ -f "$GOVTEST_SLICE_REAPER_LIB" ] || {
    echo "ERROR: govtest_slice_reaper_lib.sh not found at $GOVTEST_SLICE_REAPER_LIB" >&2
    exit 1
}
# shellcheck source=tests/infra/govtest_slice_reaper_lib.sh
source "$GOVTEST_SLICE_REAPER_LIB"

# This file's namespace. The library defaults to test_cpu_load_governance.sh's
# `reify-govtest` profile at source time, so the switch is explicit and local.
# The setter validates both the prefix and every suffix; a dash inside a suffix
# is refused precisely because it would re-create the implicit-parent leak the
# D7/D8 rename closes.
govtest_profile_set reify-test agents merge taskweight mergeweight

echo "=== cpu-governed-exec.sh tests (task 4632) — host-exclusive real-scope placement (D) ==="

# ---------------------------------------------------------------------------
# Hermetic workdir — cleaned up on exit.
# ---------------------------------------------------------------------------
WORK="$(mktemp -d)"

# Per-run unique PRIVATE slice hierarchy (task 4919), now derived from the
# library so the names, the teardown list and the sweep grammar cannot drift
# apart — this file spells no slice literal of its own.
#
# D1-D6 use a shared-parent $$-scoped pair so cpu-governed-exec.sh's
# slice-weight-set + scope placement never touch the live production
# reify-governed-*.slice hierarchy (which the running orchestrator's governance
# depends on). systemd dash-nesting (a-b-c.slice -> a.slice/a-b.slice/
# a-b-c.slice) gives task+merge a COMMON parent (D_PARENT_SLICE), faithfully
# mirroring the production reify-governed.slice/reify-governed-<agents|merge>
# .slice two-level hierarchy that D1a/D3a assert.
#
# D7/D8 use two MORE $$-scoped children of that same parent (task 6386, the
# rename: they were reify-test-task-$$.slice / reify-test-merge-$$.slice, whose
# extra dash segment vivified the pidless implicit parents this file now
# sweeps). They stay DISTINCT from D_TASK_SLICE/D_MERGE_SLICE rather than
# reusing them: cgroup_set_slice_weight sets the slice's own CPUWeight, so D5's
# REIFY_CPU_GOVERN_W_TASK=250 leaves D_TASK_SLICE at 250 and D7's
# SLICE_WEIGHT==100 assertion would go red against it.
D_PARENT_SLICE="$(govtest_slice_name "$$")"
D_TASK_SLICE="$(govtest_slice_name "$$" agents)"
D_MERGE_SLICE="$(govtest_slice_name "$$" merge)"
D7_TASK_SLICE="$(govtest_slice_name "$$" taskweight)"
D8_MERGE_SLICE="$(govtest_slice_name "$$" mergeweight)"
# grep-BRE dot-escaped forms (a literal "." in a slice name must not match
# "any char" when used inside a grep pattern). Only the three D1-D6 names need
# one today — D7/D8 assert on SLICE_WEIGHT= lines, not on unit names — so the
# other two are deliberately not minted rather than left unread; apply the same
# "${NAME//./\\.}" idiom at the point of use if that ever changes.
D_PARENT_SLICE_RE="${D_PARENT_SLICE//./\\.}"
D_TASK_SLICE_RE="${D_TASK_SLICE//./\\.}"
D_MERGE_SLICE_RE="${D_MERGE_SLICE//./\\.}"

# UNCONDITIONAL teardown (task 6386). This previously stopped a hand-written
# list of five names, which had to be kept in step with five separate
# assignments; govtest_slice_teardown derives all five from `$$` alone, so
# there is nothing left to keep in step and no future call site that can forget
# to. `systemctl --user stop` on a never-started slice is a harmless no-op
# already swallowed by the library, so it needs no record of what was created.
_cleanup_all() {
    govtest_slice_teardown "$$"
    rm -rf "$WORK"
}
trap '_cleanup_all' EXIT

# Startup sweeps — installed AFTER the trap above (so the sweeps themselves are
# covered) and BEFORE anything is created.
#
# (1) Predecessor runs stranded by an UNCATCHABLE exit. Measured in task 5930:
#     a bash script blocked in a foreground command DOES run its EXIT trap on
#     SIGTERM, SIGINT and SIGHUP, so only SIGKILL — a verify timeout, a harness
#     reap, an OOM kill — can strand units, and that residue is what this
#     clears. It never touches a live run's slices, which matters because
#     run_all.sh schedules many lanes against ONE shared per-user systemd
#     session.
govtest_reap_stale "$$"

# (2) The LEGACY pidless residue of the pre-rename naming, listed HERE rather
#     than in the library because a naming history belongs to the file that
#     made it. Each is stopped only once the fresh enumeration shows it has no
#     dash-child left to cascade into, so this converges in two runs and then
#     goes permanently silent — nothing in the repo produces these names any
#     more. See govtest_legacy_stale for why the rule is "blocked by ANY
#     dash-child" rather than deepest-first-in-one-pass.
govtest_reap_legacy reify-test-task.slice reify-test-merge.slice reify-test.slice

# Lifecycle-only seam (task 6386): exit HERE, after the trap is installed and
# both sweeps have run, so tests/infra/test_govtest_slice_reaper.sh can prove
# the slice lifecycle is wired by driving this script for real. Without it that
# proof would place five REAL systemd-run --user scopes, which the stub
# systemctl cannot intercept because the wrapper shells out to a different
# binary.
#
# TWO KEYS, NOT A COMMENT. This is the only knob in this file that can make a
# host-exclusive gate exit GREEN having run zero placement rows, and run_all.sh
# judges a member by EXIT CODE alone — it parses no "Results:" line — so a
# single stray export (a verify_env entry, an operator shell, a future harness
# forwarding REIFY_* wholesale) would silently turn this gate into a no-op that
# still reports success. So it is armed only in conjunction with
# REIFY_GOVTEST_TEST_MODE=1, a marker nothing in the repo sets outside that
# test's drive. Disarmed, the request is refused LOUDLY and the full suite runs
# — the failure mode of a stray var is then a noisy real run, never a quiet
# vacuous pass.
if [ "${REIFY_CPU_GOVEXEC_TEST_LIFECYCLE_ONLY:-0}" = "1" ]; then
    if [ "${REIFY_GOVTEST_TEST_MODE:-0}" = "1" ]; then
        echo "REIFY_CPU_GOVEXEC_TEST_LIFECYCLE_ONLY=1 — exiting after startup sweeps (slice-lifecycle drive only)"
        exit 0
    fi
    echo "WARNING: REIFY_CPU_GOVEXEC_TEST_LIFECYCLE_ONLY=1 IGNORED — REIFY_GOVTEST_TEST_MODE=1 is not set." >&2
    echo "WARNING: that seam would exit 0 having run zero placement rows, so it is refused unless" >&2
    echo "WARNING: explicitly armed.  Running the FULL suite.  If this is a real run, unset the var." >&2
fi

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
