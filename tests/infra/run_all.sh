#!/usr/bin/env bash
# tests/infra/run_all.sh — discovers and runs all test_*.sh files.
#
# Usage: run_all.sh [INFRA_DIR]
#
#   INFRA_DIR  Directory to search for test_*.sh files.
#              Defaults to the directory containing this script.
#
# Auto-discovery: all files matching test_*.sh in INFRA_DIR are discovered
# and run as subshell invocations. test_helpers.sh is excluded by name
# (it is a shared library, not a test runner).
#
# Output: After the "=== Summary: N discovered, M failed ===" line, if any
# tests failed, two lines are emitted:
#   "=== FAILED: <space-separated names> ===" — human-readable failure summary
#     so the tail of captured merge-gate output is attributable without re-running.
#   "FAILED <space-separated names>" — bare classifier marker that matches the
#     dark-factory verify.py `^FAILED\s` -> test_failure regex (pattern #7b),
#     checked before pattern #10 `tree-sitter generate` -> tree_sitter_generate_error.
#     Without this marker a pure infra-suite failure falls through to
#     tree_sitter_generate_error, triggering a thrash-escalating L1 merge
#     escalation; with it the failure is reclassified as test_failure and handled
#     by the normal debugger path.
#
# Exits 0 if all discovered tests pass (or none are found), 1 if any fail.
#
# Concurrent hermetic pool (H2, task 4924): the `pool`-bucket tests (as
# classified by tests/infra/run-all-classification.manifest, H1 task 4921)
# run concurrently under a host-global counting semaphore + a soft PSI gate.
# The `intra-run-serial` and `host-exclusive` buckets, and any UNCLASSIFIED
# test (fail-safe), run serially in a disjoint phase -- they never overlap
# the concurrent pool within a single run_all.sh invocation. Per-test output
# is buffered and replayed in discovered (sorted) order so the exact output
# contract above (headers/RESULT/Summary/FAILED) is unaffected by which tests
# finish first. The pool activates only when its substrate (classification
# lib, slot-acquire lib, cpu-admit lib, load-tolerance lib, flock) is present;
# otherwise run_all.sh falls back to the legacy fully-serial for-loop.
#
# Knobs:
#   REIFY_RUN_ALL_POOL_CONCURRENCY  N (default: max(1, nproc/2); never a
#                                   frozen host-baked count -- resolved at
#                                   runtime; never load-reduced, esc-4000-39)
#   REIFY_RUN_ALL_POOL_LOCK         semaphore lock base path
#                                   (default ${TMPDIR:-/tmp}/reify-run-all-pool-$(id -u).lock)
#   REIFY_RUN_ALL_POOL_WAIT         slot_acquire deadline seconds (default
#                                   1800; soft -- admits unslotted on timeout)
#   REIFY_RUN_ALL_POOL_PSI_PROC_PATH   PSI source (default /proc/pressure/cpu)
#   REIFY_RUN_ALL_POOL_PSI_THRESHOLD   avg10 ceiling (default 85; soft)
#   REIFY_RUN_ALL_POOL_PSI_ATTEMPTS    load_tolerant_attempts base (default 3)
#   REIFY_RUN_ALL_POOL_PSI_DISABLE     set to 1 to disable the PSI gate
#   REIFY_RUN_ALL_POOL_DISABLE         set to 1 to force the legacy all-serial
#                                      fallback (break-glass)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INFRA_DIR="${1:-$SCRIPT_DIR}"

# Hermetic-harness isolation: normalize DF_VERIFY_ROLE to 'task' for the whole
# suite run. The dark-factory post-merge gate stamps DF_VERIFY_ROLE=merge and
# runs the infra suites as one of its plan lines (verify.sh: bash run_all.sh),
# so without this every child inherits role=merge. Several suites
# (test_verify_throughput.sh, test_verify_scope.sh, test_scope_boundary.sh,
# test_verify_gui_feature_check.sh) are meta-tests that drive their own hermetic
# `verify.sh --scope branch/staged --print-plan` fixtures to assert narrowing
# behavior; under an inherited role=merge, verify.sh's contract-C2 guard
# ("merge gate never narrows") force-rewrites their scope to 'all', collapsing
# every scope=branch assertion (observed: throughput 24/14, scope 72/33).
# Pinning role=task here makes the meta-tests hermetic. Suites that deliberately
# exercise merge-role behavior set `DF_VERIFY_ROLE=merge` inline per command,
# and that per-command assignment still overrides this exported default.
export DF_VERIFY_ROLE=task

failures=0
discovered=0
failed_names=()

echo "=== Running all infra tests in: $INFRA_DIR ==="

# ---------------------------------------------------------------------------
# H2 concurrent-pool substrate detection. The pool activates only when every
# lib below is present AND flock is on PATH AND REIFY_RUN_ALL_POOL_DISABLE is
# not set; otherwise run_all.sh falls back to the legacy all-serial for-loop
# (byte-identical output/contract) so a missing/lagging substrate never
# breaks discovery or execution.
# ---------------------------------------------------------------------------
_H2_REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
_H2_CLASSIFICATION_LIB="$SCRIPT_DIR/run-all-classification-lib.sh"
_H2_SLOT_ACQUIRE_LIB="$_H2_REPO_ROOT/scripts/lib_slot_acquire.sh"
_H2_CPU_ADMIT_LIB="$_H2_REPO_ROOT/scripts/cpu-admit.sh"
_H2_LOAD_TOLERANCE_LIB="$SCRIPT_DIR/load_tolerance_lib.sh"

_H2_POOL_ACTIVE=1
if [ "${REIFY_RUN_ALL_POOL_DISABLE:-}" = "1" ]; then
    _H2_POOL_ACTIVE=0
fi
if [ "$_H2_POOL_ACTIVE" -eq 1 ]; then
    if [ ! -f "$_H2_CLASSIFICATION_LIB" ] || [ ! -f "$_H2_SLOT_ACQUIRE_LIB" ] || \
       [ ! -f "$_H2_CPU_ADMIT_LIB" ] || [ ! -f "$_H2_LOAD_TOLERANCE_LIB" ] || \
       ! command -v flock >/dev/null 2>&1; then
        _H2_POOL_ACTIVE=0
    fi
fi

if [ "$_H2_POOL_ACTIVE" -eq 1 ]; then
    # shellcheck disable=SC1090
    source "$_H2_CLASSIFICATION_LIB"
    # shellcheck disable=SC1090
    source "$_H2_SLOT_ACQUIRE_LIB"
    # shellcheck disable=SC1090
    source "$_H2_CPU_ADMIT_LIB"
    # shellcheck disable=SC1090
    source "$_H2_LOAD_TOLERANCE_LIB"
fi

if [ "$_H2_POOL_ACTIVE" -eq 1 ]; then
    # -------------------------------------------------------------------------
    # Concurrent pool path.
    # -------------------------------------------------------------------------

    # Resolve the concurrency bound N. anti-#4901: nproc-derived AT RUNTIME,
    # never a frozen host-baked count. esc-4000-39: never load-reduced -- load
    # reactivity is handled softly by the separate PSI gate (step 6) instead.
    if [ -n "${REIFY_RUN_ALL_POOL_CONCURRENCY:-}" ]; then
        _H2_POOL_N="${REIFY_RUN_ALL_POOL_CONCURRENCY}"
        case "$_H2_POOL_N" in
            ''|*[!0-9]*)
                echo "ERROR: run_all.sh: REIFY_RUN_ALL_POOL_CONCURRENCY must be a positive integer (got '${_H2_POOL_N}')" >&2
                exit 64
                ;;
        esac
        [ "$_H2_POOL_N" -ge 1 ] || { echo "ERROR: run_all.sh: REIFY_RUN_ALL_POOL_CONCURRENCY must be >= 1 (got '${_H2_POOL_N}')" >&2; exit 64; }
    else
        _H2_NPROC="$(nproc 2>/dev/null || echo 2)"
        case "$_H2_NPROC" in
            ''|*[!0-9]*) _H2_NPROC=2 ;;
        esac
        _H2_POOL_N=$(( _H2_NPROC / 2 ))
        [ "$_H2_POOL_N" -ge 1 ] || _H2_POOL_N=1
    fi

    _H2_POOL_LOCK="${REIFY_RUN_ALL_POOL_LOCK:-${TMPDIR:-/tmp}/reify-run-all-pool-$(id -u).lock}"
    _H2_POOL_WAIT="${REIFY_RUN_ALL_POOL_WAIT:-1800}"

    # -- Discovery + partition (pool vs serial; unclassified fail-safes serial) --
    _h2_discovered_list=()
    while IFS= read -r _h2_name; do
        [ -n "$_h2_name" ] && _h2_discovered_list+=("$_h2_name")
    done < <(classification_discovered_set "$INFRA_DIR")

    declare -A _h2_is_pool=()
    while IFS= read -r _h2_name; do
        [ -n "$_h2_name" ] && _h2_is_pool["$_h2_name"]=1
    done < <(classification_bucket pool)

    _h2_pool_members=()
    _h2_serial_members=()
    for _h2_name in "${_h2_discovered_list[@]}"; do
        if [ "${_h2_is_pool[$_h2_name]:-0}" = "1" ]; then
            _h2_pool_members+=("$_h2_name")
        else
            _h2_serial_members+=("$_h2_name")
        fi
    done

    discovered="${#_h2_discovered_list[@]}"

    declare -A _h2_index_of=()
    _h2_idx=0
    for _h2_name in "${_h2_discovered_list[@]}"; do
        _h2_idx=$((_h2_idx + 1))
        _h2_index_of["$_h2_name"]=$_h2_idx
    done

    _H2_WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/reify-run-all-pool.XXXXXX")"
    trap 'rm -rf "$_H2_WORKDIR"' EXIT

    # -- Phase 1: pool (concurrent, bounded by the host-global semaphore) --------
    _h2_pids=()
    for _h2_name in "${_h2_pool_members[@]}"; do
        _h2_i="${_h2_index_of[$_h2_name]}"
        (
            _h2_child_rc=0
            _h2_slot_rc=0
            # Soft acquire: on deadline (75) proceed unslotted -- never skip a
            # test, never hang. slot_acquire itself already closes FD 9 on
            # every failed attempt, so no held-slot cleanup is needed here.
            slot_acquire "$_H2_POOL_LOCK" "$_H2_POOL_N" "$_H2_POOL_WAIT" || _h2_slot_rc=$?
            bash "$INFRA_DIR/$_h2_name" 9<&- > "$_H2_WORKDIR/${_h2_i}.out" 2>&1 || _h2_child_rc=$?
            if [ "$_h2_slot_rc" -eq 0 ]; then
                exec 9>&-
            fi
            echo "$_h2_child_rc" > "$_H2_WORKDIR/${_h2_i}.rc"
            exit 0
        ) &
        _h2_pids+=($!)
    done
    if [ "${#_h2_pids[@]}" -gt 0 ]; then
        wait "${_h2_pids[@]}" 2>/dev/null || true
    fi

    # -- Phase 2: serial (foreground, one at a time, discovered order) -----------
    for _h2_name in "${_h2_serial_members[@]}"; do
        _h2_i="${_h2_index_of[$_h2_name]}"
        _h2_rc=0
        bash "$INFRA_DIR/$_h2_name" > "$_H2_WORKDIR/${_h2_i}.out" 2>&1 || _h2_rc=$?
        echo "$_h2_rc" > "$_H2_WORKDIR/${_h2_i}.rc"
    done

    # -- Phase 3: emit (discovered/sorted order -- preserves the output contract) --
    for _h2_name in "${_h2_discovered_list[@]}"; do
        _h2_i="${_h2_index_of[$_h2_name]}"
        echo ""
        echo "--- Running: $_h2_name ---"
        cat "$_H2_WORKDIR/${_h2_i}.out" 2>/dev/null || true
        _h2_rc="$(cat "$_H2_WORKDIR/${_h2_i}.rc" 2>/dev/null || echo 1)"
        if [ "$_h2_rc" -eq 0 ]; then
            echo "  RESULT: PASS ($_h2_name)"
        else
            echo "  RESULT: FAIL ($_h2_name)"
            failures=$((failures + 1))
            failed_names+=("$_h2_name")
        fi
    done
else
    # -------------------------------------------------------------------------
    # Legacy all-serial fallback (byte-identical to the pre-H2 behavior).
    # -------------------------------------------------------------------------
    for test_file in "$INFRA_DIR"/test_*.sh; do
        # If glob matches nothing, the literal pattern string is returned — skip it.
        [ -f "$test_file" ] || continue

        # Exclude test_helpers.sh — shared library, not a test runner.
        basename="$(basename "$test_file")"
        if [ "$basename" = "test_helpers.sh" ]; then
            continue
        fi

        discovered=$((discovered + 1))
        echo ""
        echo "--- Running: $basename ---"
        if bash "$test_file"; then
            echo "  RESULT: PASS ($basename)"
        else
            echo "  RESULT: FAIL ($basename)"
            failures=$((failures + 1))
            failed_names+=("$basename")
        fi
    done
fi

echo ""
echo "=== Summary: $discovered discovered, $failures failed ==="
if [ "${#failed_names[@]}" -gt 0 ]; then
    echo "=== FAILED: ${failed_names[*]} ==="
    printf 'FAILED %s\n' "${failed_names[*]}"
fi

if [ "$failures" -eq 0 ]; then
    exit 0
else
    exit 1
fi
