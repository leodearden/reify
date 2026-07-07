#!/usr/bin/env bash
# Infrastructure test for task 5125.
#
# Drift-guard (INV-5): the full tests/infra/run_all.sh pool suite must run at
# the MERGE tier ONLY; every per-task verify must run the cheap selective-infra
# subset instead. This encodes the "exactly-one" invariant — {full pool,
# selective infra} — never both, never neither — keyed on DF_VERIFY_ROLE.
#
# Root cause this guards against (task 5125 analysis): the merge seam
# (hooks/pre-merge-commit:39) runs `DF_VERIFY_ROLE=merge verify.sh all
# --profile both --scope all` WITHOUT --include-infra, while EVERY per-task
# lane passes --include-infra. Gating the wholesale run_all.sh line on
# INCLUDE_INFRA (not on role) meant the full 103-test suite ran on every task
# lane and NEVER at merge — M-way concurrent pool contention that starved the
# shared 16-slot semaphore (30m timeout -> exit 124 -> BLOCKED).
#
# Oracle: verify.sh --print-plan (hermetic, never runs cargo/npm) inside an
# isolated mktemp git fixture. make_branch_fixture mirrors
# tests/infra/test_verify_scope.sh:make_branch_fixture verbatim (reuse note).
#
# RED now: verify.sh still gates run_all.sh on INCLUDE_INFRA, so the MERGE
# capture (no --include-infra) LACKS run_all.sh, and the TASK capture (with
# --include-infra) CONTAINS it — the exact inversion this task fixes.
#
# Review follow-up: the plain MERGE scenario below captures a no-diff plan,
# so its "selective loop absent" assertion holds independent of the
# DF_VERIFY_ROLE!=merge injection-site gate (see the "MERGE tier
# (non-vacuous)" scenario's own header comment for the belt-and-braces
# analysis and why a real-diff capture is added alongside it).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

[ -f "$SCRIPT_DIR/plan_capture_lib.sh" ] || { echo "ERROR: plan_capture_lib.sh not found at $SCRIPT_DIR/plan_capture_lib.sh"; exit 1; }
source "$SCRIPT_DIR/plan_capture_lib.sh"

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

echo "=== run_all.sh tiering drift-guard tests (task 5125) ==="

# make_branch_fixture VARNAME — isolated throwaway git repo with a 'main'
# branch containing just the scripts verify.sh needs. Mirrors
# tests/infra/test_verify_scope.sh:make_branch_fixture verbatim.
make_branch_fixture() {
    local _var="$1" dir
    dir="$(mktemp -d)"
    _TMPDIRS+=("$dir")
    mkdir -p "$dir/scripts"
    cp "$REPO_ROOT/scripts/verify.sh" "$dir/scripts/verify.sh"
    cp "$REPO_ROOT/scripts/occt-scope-lib.sh" "$dir/scripts/occt-scope-lib.sh"
    cp "$REPO_ROOT/scripts/occt-touching-crates.txt" "$dir/scripts/occt-touching-crates.txt"
    cp "$REPO_ROOT/scripts/release-scope-lib.sh" "$dir/scripts/release-scope-lib.sh"
    cp "$REPO_ROOT/scripts/release-sensitive-crates.txt" "$dir/scripts/release-sensitive-crates.txt"
    cp "$REPO_ROOT/scripts/affected-crates-lib.sh" "$dir/scripts/affected-crates-lib.sh"
    cp "$REPO_ROOT/scripts/lib_test_semaphore.sh" "$dir/scripts/lib_test_semaphore.sh"
    cp "$REPO_ROOT/scripts/lib_slot_acquire.sh" "$dir/scripts/lib_slot_acquire.sh"
    cp "$REPO_ROOT/scripts/lib_clock_stop.sh"   "$dir/scripts/lib_clock_stop.sh"
    cp "$REPO_ROOT/scripts/cpu-admit.sh" "$dir/scripts/cpu-admit.sh"
    cp "$REPO_ROOT/scripts/lib_proc_reaper.sh" "$dir/scripts/lib_proc_reaper.sh"
    cp "$REPO_ROOT/scripts/gen-nextest-config.sh" "$dir/scripts/gen-nextest-config.sh"
    cp "$REPO_ROOT/scripts/heavy-test-filter-lib.sh" "$dir/scripts/heavy-test-filter-lib.sh"
    cp "$REPO_ROOT/scripts/verify-pipeline-infra-tests.txt" "$dir/scripts/verify-pipeline-infra-tests.txt"
    mkdir -p "$dir/.config"
    cp "$REPO_ROOT/.config/nextest.toml" "$dir/.config/nextest.toml"
    chmod +x "$dir/scripts/verify.sh"
    git -C "$dir" init -q
    git -C "$dir" config user.email "test@test.com"
    git -C "$dir" config user.name "Test"
    git -C "$dir" add scripts
    git -C "$dir" commit -q -m "base"
    git -C "$dir" branch -M main
    printf -v "$_var" '%s' "$dir"
}

FIX=""
make_branch_fixture FIX

# Shared capture var + fork-free predicates (mirrors test_verify_scope.sh's
# PLAN_OUT/plan_has/plan_lacks convention — reassign PLAN_OUT per scenario).
PLAN_OUT=""
plan_has()    { plan_match "$PLAN_OUT" "$1"; }
plan_lacks()  { ! plan_match "$PLAN_OUT" "$1"; }

# ===========================================================================
# MERGE tier: DF_VERIFY_ROLE=merge, --scope all, NO --include-infra — mirrors
# hooks/pre-merge-commit:39 exactly (the real merge seam never passes
# --include-infra). The full run_all.sh pool suite must run here regardless.
# ===========================================================================
echo ""
echo "--- MERGE tier: role=merge --scope all (no --include-infra) -> full run_all.sh pool ---"

capture_print_plan PLAN_OUT "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
    bash -c 'cd "$1" && export DF_VERIFY_ROLE=merge && exec bash scripts/verify.sh all --profile both --scope all --print-plan' \
    _ "$FIX" || true

assert "MERGE: plan capture complete (structural markers present)" \
    plan_capture_complete "$PLAN_OUT"

assert "MERGE: plan CONTAINS tests/infra/run_all.sh (full pool suite, merge backstop, RED until step-3)" \
    plan_has 'tests/infra/run_all\.sh'

assert "MERGE: run_all.sh line carries REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1 (host-exclusive tests stay on cold lane, RED until step-3)" \
    bash -c 'printf "%s\n" "$1" | grep "run_all\.sh" | grep -q "REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1"' _ "$PLAN_OUT"

assert "MERGE: run_all.sh line carries REIFY_AUDIT_NO_COLD_BUILD=1 (budget-safe backstop, task #4624, RED until step-3)" \
    bash -c 'printf "%s\n" "$1" | grep "run_all\.sh" | grep -q "REIFY_AUDIT_NO_COLD_BUILD=1"' _ "$PLAN_OUT"

# Ambient-leak guard (task 5125): the run_all.sh line must NOT export
# REIFY_INFRA_SUITE_ACTIVE. Broadcasting the re-entrancy sentinel onto this line
# leaks it into all ~103 pool tests, suppressing run_all in the plans the
# plan-shape tests capture and tripping test_run_all_ambient_isolation.sh. The
# sentinel is set narrowly at the sole recursion source
# (test_verify_semaphore_e2e.sh Section B), never here.
assert "MERGE: run_all.sh line does NOT export REIFY_INFRA_SUITE_ACTIVE (no ambient leak into pool tests)" \
    bash -c '! { printf "%s\n" "$1" | grep "run_all\.sh" | grep -q "REIFY_INFRA_SUITE_ACTIVE"; }' _ "$PLAN_OUT"

assert "MERGE: plan LACKS the selective test_verify_*.sh loop (exactly-one: full pool present, selective absent)" \
    plan_lacks 'tests/infra/test_verify_\*\.sh'

# ---------------------------------------------------------------------------
# MERGE tier (non-vacuous drift-guard, review follow-up): the scenario above
# captures the plan with NO branch diff at all (FIX's "main" is untouched),
# so SELECTED_INFRA_GLOBS is empty independent of the DF_VERIFY_ROLE!=merge
# injection-site gate (verify.sh:~1388) — the "selective loop absent"
# assertion above would still pass even if that gate regressed. Close the
# gap for real: commit an actual diff to a mapped artifact (scripts/verify.sh,
# mirrors the TASK scenario below) on a branch ahead of main, and REQUEST
# --scope branch (an attempted narrow, NOT --scope all) so decide_scope()
# genuinely runs its branch-diff path instead of the test itself handing it
# an empty CHANGED_FILES_RAW via an explicit --scope all flag. The plan
# header's `scope=all` token is asserted directly, proving contract-C2
# forcing (verify.sh:583, "DF_VERIFY_ROLE=merge — forcing --scope all")
# actually fired for this real diff, rather than merely being assumed.
#
# Belt-and-braces limit (documented, not a gap this test can close): C2
# forcing runs BEFORE decide_scope() and is unconditional on the requested
# scope, so CHANGED_FILES_RAW/SELECTED_INFRA_GLOBS are empty at merge via
# scope-forcing ALONE — the injection-site role check is a second,
# structurally-redundant belt that can only independently matter if C2
# forcing is ALSO broken (verified empirically: with forcing intact,
# SELECTED_INFRA_GLOBS is unreachable-nonempty under role=merge no matter
# what diff exists). This scenario proves the composite guarantee holds
# end-to-end with a real diff in play, and goes RED the moment C2 forcing
# itself regresses — the only way the exactly-one invariant can actually
# break in practice — even though it cannot isolate the injection-site gate
# from forcing without patching verify.sh's own logic in the fixture.
# ---------------------------------------------------------------------------
echo ""
echo "--- MERGE tier (non-vacuous): role=merge --scope branch (attempted), verify.sh changed -> forced to scope=all, still no selective leak ---"

git -C "$FIX" checkout -q -b merge-diff-branch
echo "# task-5125 MERGE-tier verify.sh-change simulation sentinel" >> "$FIX/scripts/verify.sh"
git -C "$FIX" add scripts/verify.sh
git -C "$FIX" commit -q -m "merge-tier diff simulation"

capture_print_plan PLAN_OUT "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
    bash -c 'cd "$1" && export DF_VERIFY_ROLE=merge && exec bash scripts/verify.sh all --profile both --scope branch --print-plan' \
    _ "$FIX" || true

git -C "$FIX" checkout -q main
git -C "$FIX" branch -q -D merge-diff-branch

assert "MERGE (non-vacuous): plan capture complete (structural markers present)" \
    plan_capture_complete "$PLAN_OUT"

assert "MERGE (non-vacuous): plan header shows scope=all (contract C2 forced the attempted --scope branch back to all, despite a real diff)" \
    plan_has 'scope=all'

assert "MERGE (non-vacuous): plan CONTAINS tests/infra/run_all.sh despite the attempted narrow scope" \
    plan_has 'tests/infra/run_all\.sh'

assert "MERGE (non-vacuous): plan LACKS the selective test_verify_*.sh loop even though scripts/verify.sh (a mapped artifact) genuinely changed on this branch" \
    plan_lacks 'tests/infra/test_verify_\*\.sh'

# ===========================================================================
# RE-ENTRANCY GUARD (task 5125): a DF_VERIFY_ROLE=merge verify running INSIDE
# an infra suite must NOT re-emit the run_all.sh line. run_all.sh keys the pool
# on the INHERITED env var DF_VERIFY_ROLE=merge, and
# tests/infra/test_verify_semaphore_e2e.sh Section B deliberately drives a real
# `DF_VERIFY_ROLE=merge verify.sh` to prove the semaphore bypass. Without the
# guard the merge gate recurses unboundedly (run_all -> semaphore-e2e ->
# merge-role verify -> run_all -> ...) until the 30m wall SIGKILLs it — the
# exact fork-bomb that blocked this task's own merge (post-merge verify
# "Terminated"). The guard: verify.sh exports REIFY_INFRA_SUITE_ACTIVE=1 onto
# the run_all.sh / selective plan lines (inherited by every descendant), and
# the run_all emit is gated on that sentinel being unset.
# Oracle: with the sentinel PRE-SET (simulating "already inside run_all"), the
# merge plan must LACK run_all.sh — RED before the guard, GREEN after.
# ===========================================================================
echo ""
echo "--- RE-ENTRANCY: role=merge --scope all with REIFY_INFRA_SUITE_ACTIVE=1 -> run_all.sh suppressed (no recursion) ---"

capture_print_plan PLAN_OUT "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
    bash -c 'cd "$1" && export DF_VERIFY_ROLE=merge REIFY_INFRA_SUITE_ACTIVE=1 && exec bash scripts/verify.sh all --profile both --scope all --print-plan' \
    _ "$FIX" || true

assert "RE-ENTRANCY: plan capture complete (structural markers present)" \
    plan_capture_complete "$PLAN_OUT"

assert "RE-ENTRANCY: plan LACKS tests/infra/run_all.sh when REIFY_INFRA_SUITE_ACTIVE=1 (nested verify does not re-launch the pool -> no fork-bomb, RED before the guard)" \
    plan_lacks 'tests/infra/run_all\.sh'

assert "RE-ENTRANCY: plan also LACKS the selective test_verify_*.sh loop (a nested merge verify runs neither infra suite -> exactly-neither when re-entrant)" \
    plan_lacks 'tests/infra/test_verify_\*\.sh'

# ===========================================================================
# TASK tier: role=task (default), --scope branch --include-infra, on a branch
# that changes a mapped verify-pipeline artifact (scripts/verify.sh itself).
# Mirrors every per-task lane (which always passes --include-infra). The
# wholesale run_all.sh pool must be ABSENT; the cheap selective subset for the
# changed artifact must be PRESENT instead.
# ===========================================================================
echo ""
echo "--- TASK tier: role=task --scope branch --include-infra, verify.sh changed -> selective subset, no full pool ---"

git -C "$FIX" checkout -q -b task-branch
echo "# task-5125 verify.sh-change simulation sentinel" >> "$FIX/scripts/verify.sh"
git -C "$FIX" add scripts/verify.sh
git -C "$FIX" commit -q -m "task changes"

capture_print_plan PLAN_OUT "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
    bash -c 'cd "$1" && exec bash scripts/verify.sh all --profile debug --scope branch --include-infra --print-plan' \
    _ "$FIX" || true

git -C "$FIX" checkout -q main
git -C "$FIX" branch -q -D task-branch

assert "TASK: plan capture complete (structural markers present)" \
    plan_capture_complete "$PLAN_OUT"

assert "TASK: plan LACKS tests/infra/run_all.sh (full pool suite moved to merge tier, RED until step-3)" \
    plan_lacks 'tests/infra/run_all\.sh'

assert "TASK: plan CONTAINS the selective test_verify_*.sh loop (fail-fast per-task subset)" \
    plan_has 'tests/infra/test_verify_\*\.sh'

assert "TASK: plan contains timeout+bash invocation for the selective infra loop" \
    plan_has 'test_verify.*timeout.*bash'

test_summary
