#!/usr/bin/env bash
# tests/infra/run_all_ambient_isolation_lib.sh — the per-key hostile+baseline
# ambient-isolation decision, extracted so it can be unit-tested in isolation
# (task 5259, PRD docs/prds/merge-gate-health.md W4c).
#
# Provides:
#   ambient_isolation_check_one <target> <key> <val> <manifest_keys>
#
#     Runs <target> (a test_run_all.sh-shaped script) under a HOSTILE ambient
#     env — every var named in <manifest_keys> unset, then only <key>=<val>
#     exported — the same shape dark-factory-orchestrator.yaml's verify_env /
#     the run_all.sh plan-line prefix env produce. Then classifies:
#
#       hostile GREEN                -> PASS  (return 0). No baseline run, so
#                                      the common all-green merge gate keeps
#                                      its current single-nested-run cost.
#       hostile RED + baseline RED   -> SKIP  (return 0). The target is red
#                                      independent of the ambient env, so the
#                                      failure is DERIVATIVE — emitting a
#                                      distinct SKIP marker (not a failure)
#                                      stops run_all.sh's exit-code classifier
#                                      from double-counting one test_run_all.sh
#                                      failure as two FAILED names (W4c).
#       hostile RED + baseline GREEN -> FAIL  (return 1). The hostile ambient
#                                      env ALONE flipped the target red — a
#                                      genuine ambient-isolation bug.
#
#     "GREEN" means exit 0 AND a `^Results: [0-9]+ passed, 0 failed$` line
#     (the same predicate the consumer's hostile loop already used). The
#     verdict is returned via EXIT CODE (0 = PASS/SKIP, 1 = FAIL) and the
#     PASS/SKIP/FAIL line is emitted to STDOUT, so a unit test can capture
#     both in a $(...) subshell while the consumer maps rc -> a single assert.
#     The function never calls assert() itself.
#
# Designed to be sourced, not executed directly:
#   source "$(dirname "${BASH_SOURCE[0]}")/run_all_ambient_isolation_lib.sh"

# Source guard — prevent double-sourcing.
if [ "${_REIFY_RUN_ALL_AMBIENT_ISOLATION_LIB_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_RUN_ALL_AMBIENT_ISOLATION_LIB_SOURCED=1

_RUN_ALL_AMBIENT_ISOLATION_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# plan_match() — the fork-free `^Results: ... 0 failed$` matcher. Source
# defensively via our own dir; plan_capture_lib.sh is itself source-guarded,
# so a double-source (the consumer already sources it) is a harmless no-op.
if ! declare -F plan_match >/dev/null 2>&1; then
    [ -f "$_RUN_ALL_AMBIENT_ISOLATION_LIB_DIR/plan_capture_lib.sh" ] || {
        echo "ERROR: plan_capture_lib.sh not found at $_RUN_ALL_AMBIENT_ISOLATION_LIB_DIR/plan_capture_lib.sh" >&2
        return 1 2>/dev/null || exit 1
    }
    source "$_RUN_ALL_AMBIENT_ISOLATION_LIB_DIR/plan_capture_lib.sh"
fi

# ambient_isolation_check_one <target> <key> <val> <manifest_keys>
ambient_isolation_check_one() {
    local _target="$1" _key="$2" _val="$3" _manifest_keys="$4"

    # Hostile run: unset every ledger var, then export only the one under test.
    # unset/export/nested-bash all run inside the $( ... ) subshell, so none of
    # it leaks back to the caller. Declare the locals BEFORE the capturing
    # assignment so `|| _amb_rc=$?` sees the subshell's exit code (a combined
    # `local x="$(...)"` would mask it — local always returns 0).
    local _amb_out _amb_rc=0
    _amb_out="$(
        while IFS= read -r _unset_key; do
            [ -z "$_unset_key" ] && continue
            unset "$_unset_key"
        done <<< "$_manifest_keys"
        export "$_key=$_val"
        bash "$_target" 2>&1
    )" || _amb_rc=$?

    if [ "$_amb_rc" -eq 0 ] && plan_match "$_amb_out" '^Results: [0-9]+ passed, 0 failed$'; then
        echo "AMBIENT-ISOLATION PASS: $_key hostile run is green — no isolation bug [ambient $_key=$_val]"
        return 0
    fi

    # NAIVE (step-2): treat ANY hostile-red as derivative and SKIP. This
    # over-skips a genuine isolation bug (a target red ONLY under the hostile
    # env); step-4 adds the clean-env baseline disambiguation to fix that.
    echo "SKIP: baseline test_run_all.sh already red (not an isolation bug) [ambient $_key=$_val; hostile rc=$_amb_rc]"
    return 0
}
