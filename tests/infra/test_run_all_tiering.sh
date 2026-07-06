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

assert "MERGE: plan LACKS the selective test_verify_*.sh loop (exactly-one: full pool present, selective absent)" \
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
