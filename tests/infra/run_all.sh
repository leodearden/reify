#!/usr/bin/env bash
# tests/infra/run_all.sh — discovers and runs all test_*.sh files.
#
# Usage: run_all.sh [--scope host-infra] [INFRA_DIR]
#
#   INFRA_DIR  Directory to search for test_*.sh files.
#              Defaults to the directory containing this script.
#   --scope host-infra
#              Run EXACTLY the declared `host-exclusive` bucket (H1 manifest)
#              -- the INVERSE of the REIFY_RUN_ALL_EXCLUDE_HOST_INFRA
#              exclusion knob documented below -- serially in the foreground,
#              under the H8 Lane-X single-flight flock
#              (scripts/lib_lane_x_flock.sh). This is task H9
#              (docs/prds/run-all-host-infra-partition.md): the off-hot-path
#              executor of the host-exclusive set. Deliberately IGNORES
#              REIFY_RUN_ALL_EXCLUDE_HOST_INFRA -- the knob that moves
#              host-exclusive off the hot path must not also suppress the
#              runner meant to catch it off-path. Any other --scope value
#              (or a --scope flag with no value) is a usage error (exit 64).
#              With no --scope, behavior is unchanged (pool/legacy below).
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
# Discovery/partition diagnostic guard (task #5123, esc-5080-9): a scoped ERR
# trap wraps the pool path's discovery -> partition -> workdir-setup block
# (before Phase 1 spawns anything) so a failure there prints an actionable
# `ERROR: run_all.sh pool: ...` diagnostic -- failing command, exit code,
# loadavg, PSI avg10 -- instead of dying silently right after the `INFO:
# run_all.sh pool: N=` line, then re-raises the same exit code. See
# _ra_discovery_diag below.
#
# Knobs:
#   REIFY_RUN_ALL_POOL_CONCURRENCY  N (default: max(1, nproc/2); never a
#                                   frozen host-baked count -- resolved at
#                                   runtime; never load-reduced, esc-4000-39)
#   REIFY_RUN_ALL_POOL_LOCK         semaphore lock base path
#                                   (default ${TMPDIR:-/tmp}/reify-run-all-pool-$(id -u).lock)
#   REIFY_RUN_ALL_POOL_WAIT         slot_acquire deadline seconds (default
#                                   1800; soft -- admits unslotted on timeout).
#                                   Must be a positive integer; slot_acquire's
#                                   own "unlimited" no-deadline sentinel is
#                                   REJECTED here (exit 64) -- honoring it
#                                   would let a wedged/over-subscribed pool
#                                   semaphore block run_all.sh indefinitely,
#                                   defeating the pool's never-hang contract.
#   REIFY_RUN_ALL_POOL_PSI_PROC_PATH   PSI source (default /proc/pressure/cpu)
#   REIFY_RUN_ALL_POOL_PSI_THRESHOLD   avg10 ceiling (default 85; soft)
#   REIFY_RUN_ALL_POOL_PSI_ATTEMPTS    load_tolerant_attempts base (default 3)
#   REIFY_RUN_ALL_POOL_PSI_DISABLE     set to 1 to disable the PSI gate
#   REIFY_RUN_ALL_POOL_DISABLE         set to 1 to force the legacy all-serial
#                                      fallback (break-glass)
#   REIFY_RUN_ALL_EXCLUDE_HOST_INFRA   set to EXACTLY "1" to exclude the
#                                      `host-exclusive` bucket (H1 manifest)
#                                      from discovery entirely (H3 flip-seam,
#                                      task 4925). Any other value (unset/
#                                      empty/"0"/garbage) runs the full
#                                      discovered set unchanged -- default 0,
#                                      strictly additive on landing.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Arg parsing: an optional `--scope <val>` / `--scope=<val>` flag plus one
# optional positional INFRA_DIR (order-independent). An unrecognized flag
# exits 64 (usage error) rather than being silently mis-parsed as INFRA_DIR,
# mirroring the REIFY_RUN_ALL_POOL_* validation idiom used below.
SCOPE=""
INFRA_DIR=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --scope=*)
            SCOPE="${1#--scope=}"
            shift
            ;;
        --scope)
            if [ "$#" -lt 2 ]; then
                echo "ERROR: run_all.sh: --scope requires a value" >&2
                exit 64
            fi
            SCOPE="$2"
            shift 2
            ;;
        --*)
            echo "ERROR: run_all.sh: unknown option '$1'" >&2
            exit 64
            ;;
        *)
            INFRA_DIR="$1"
            shift
            ;;
    esac
done
INFRA_DIR="${INFRA_DIR:-$SCRIPT_DIR}"

# --scope value validation: empty (default) selects the unchanged pool/legacy
# behavior below; "host-infra" selects the H9 branch; any other non-empty
# value is a usage error (exit 64), mirroring the REIFY_RUN_ALL_POOL_*
# validation style elsewhere in this script rather than silently falling
# through to the default full run.
case "$SCOPE" in
    ''|host-infra) ;;
    *)
        echo "ERROR: run_all.sh: --scope must be 'host-infra' (got '${SCOPE}')" >&2
        exit 64
        ;;
esac

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

# ---------------------------------------------------------------------------
# Clock-marker sanitizer for re-emitted infra-test output (task 4998, esc-4791-52).
#
# dark-factory's clock-stop verify-timeout parser (verify.py _match_clock_marker,
# dark_factory:1916) scans the STREAMED verify output for the
# @@REIFY_CLOCK_{STOP,HEARTBEAT,START}@@ markers to pause/resume its wall-clock
# budget and to run its 180s heartbeat-idle backstop. run_all.sh is a TEST RUNNER:
# it performs NO real admission wait, so it must never surface a LIVE clock marker.
# But the infra tests it drives (test_cpu_admit.sh, test_psi_gate.sh,
# test_clock_stop.sh, test_verify_semaphore_e2e.sh, ...) legitimately QUOTE these
# tokens in PASS/FAIL assertion text ("... stderr contains @@REIFY_CLOCK_STOP@@ ...")
# and emit a few as fixtures. Because run_all captures each test's output and
# re-emits it verbatim, those tokens reached DF's stream, where its (historically
# substring-based) matcher misread them as REAL STOP/START transitions — leaving
# DF's state machine wrongly STOPPED going into the heavy `cargo nextest run
# --workspace` compile. A >180s silent native-kernel (OCCT/gmsh/manifold/openvdb)
# link gap then tripped the heartbeat-idle backstop and false-killed a healthy,
# code-complete compile (esc-4791-52 / 4655 / 4994, all AFFECTED=ALL; run_all
# ~181s + 180s idle == the reported 361s kill).
#
# This is the SYSTEMIC fix for a class the per-source stderr-isolation patches
# (tasks 4802/4887/4931) could not close — those caught bare nested-subprocess
# leaks but never the assertion-text quotes (the test's own legitimate stdout).
# Rewriting the shared `@@REIFY_CLOCK_` prefix to `@@REIFY_QUOTED_CLOCK_` keeps the
# text human-readable while breaking BOTH substring and line-anchored matching.
# (Layer 2, dark_factory:1916, additionally anchors _match_clock_marker to
# line-start so quoted-in-prose tokens never match from any project — belt-and-
# braces; either layer alone closes the hole.)
#
# Scope: applied to the concurrent-pool re-emission sites below — the path every
# per-task/merge verify actually drives (`verify.sh test ... --include-infra` ->
# plain `bash run_all.sh` -> pool). The `--scope host-infra` (H9) and legacy
# all-serial paths are not the DF clock-stop-parsed verify stream (H9 is the
# off-hot-path Lane-X runner; legacy only runs when the pool substrate is absent),
# and Layer 2's anchored matcher covers their quoted-marker case regardless.
_RA_CLOCK_SANITIZE='s/@@REIFY_CLOCK_/@@REIFY_QUOTED_CLOCK_/g'

# _ra_emit_sanitized <captured-output-file>
#   cat a captured per-test output file to stdout with clock-marker tokens
#   neutralized. A missing file is a silent no-op, preserving the prior
#   `cat "$f" 2>/dev/null || true` re-emit semantics exactly.
_ra_emit_sanitized() {
    [ -f "$1" ] || return 0
    sed "$_RA_CLOCK_SANITIZE" "$1" 2>/dev/null || true
}

# ---------------------------------------------------------------------------
# _ra_discovery_diag <rc> <lineno> <bash_command>  (task #5123, esc-5080-9)
#
# ERR-trap handler scoped around the H2 pool discovery/partition/pool-workdir
# block below. Under fleet-wide host overload that block was observed to die
# IMMEDIATELY after the "INFO: run_all.sh pool: N=" line with ZERO
# diagnostic -- an unguarded command inside it returned nonzero under
# `set -euo pipefail` and the script just exited, so the operator had no way
# to tell which command failed or why. The pool branch installs
# `trap '_ra_discovery_diag "$?" "$LINENO" "$BASH_COMMAND"' ERR` right after
# the INFO line and clears it (`trap - ERR`) right after the workdir/EXIT-trap
# setup, so this fires on the first unguarded failure in exactly that span
# (never the PSI-gate definition or Phases 1-3).
#
# Runs fail-open under `set +e` -- gathering diagnostic context must never
# itself become a new failure source or mask the real root cause -- reads
# loadavg and PSI avg10 best-effort, prints an actionable `ERROR: run_all.sh
# pool: ...` diagnostic to stderr, then RE-RAISES the exact captured exit
# code. Purely additive observability: no classifier/aggregation/exit-code
# behavior changes on either the failure or success path.
# ---------------------------------------------------------------------------
_ra_discovery_diag() {
    set +e
    local _rc="$1" _lineno="$2" _cmd="$3"
    local _la _avg10 _psi_proc

    _la="$( (cut -d' ' -f1-3 /proc/loadavg) 2>/dev/null )"
    [ -n "$_la" ] || _la="?"

    _psi_proc="${REIFY_RUN_ALL_POOL_PSI_PROC_PATH:-/proc/pressure/cpu}"
    if command -v cpu_admit_read_avg10 >/dev/null 2>&1; then
        _avg10="$(cpu_admit_read_avg10 "$_psi_proc" 2>/dev/null)"
    else
        _avg10=""
    fi
    [ -n "$_avg10" ] || _avg10="n/a"

    echo "ERROR: run_all.sh pool: discovery/partition stage failed (exit ${_rc}) at line ${_lineno}: command: ${_cmd}" >&2
    echo "ERROR: run_all.sh pool: host state at failure: loadavg=${_la} psi_cpu_avg10=${_avg10} (task #5123 discovery/partition guard)" >&2

    exit "$_rc"
}

failures=0
discovered=0
failed_names=()
flaky_names=()

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

# ---------------------------------------------------------------------------
# H3 flip-seam (task 4925): REIFY_RUN_ALL_EXCLUDE_HOST_INFRA exclusion set.
# Strict `= "1"` equality only -- unset/empty/"0"/garbage all leave
# _h3_exclude empty, so the FULL discovered set runs unchanged (DA1:
# strictly additive default; a malformed knob must never silently drop
# host-infra coverage). Computed ONCE here, before the pool-vs-legacy
# branch, so both paths below apply the identical exclusion set regardless
# of pool activation / REIFY_RUN_ALL_POOL_DISABLE.
# ---------------------------------------------------------------------------
declare -A _h3_exclude=()
if [ "${REIFY_RUN_ALL_EXCLUDE_HOST_INFRA:-}" = "1" ] && [ -f "$_H2_CLASSIFICATION_LIB" ]; then
    # shellcheck disable=SC1090
    source "$_H2_CLASSIFICATION_LIB"
    while IFS= read -r _h3_name; do
        [ -n "$_h3_name" ] && _h3_exclude["$_h3_name"]=1
    done < <(classification_bucket host-exclusive)
fi

if [ "$SCOPE" = "host-infra" ]; then
    # -------------------------------------------------------------------------
    # H9 host-infra branch (docs/prds/run-all-host-infra-partition.md task H9):
    # runs EXACTLY the declared host-exclusive set (the INVERSE of H3's
    # exclusion) under the H8 Lane-X single-flight flock, serially in the
    # foreground -- host-exclusive tests do real burn/cgroup/reflink work and
    # must never overlap, so no pool, no buffering. Deliberately IGNORES
    # _h3_exclude / REIFY_RUN_ALL_EXCLUDE_HOST_INFRA: this is the runner
    # meant to catch exactly the residue that knob moves off the hot path, so
    # the knob must never suppress it too. The flock lib is sourced/acquired
    # ONLY in this branch, so the default/pool/legacy hot paths below never
    # touch the Lane-X lock (H8's inert-on-hot-path safety property).
    # -------------------------------------------------------------------------
    _H9_LANE_X_FLOCK_LIB="$_H2_REPO_ROOT/scripts/lib_lane_x_flock.sh"

    if [ ! -f "$_H2_CLASSIFICATION_LIB" ]; then
        echo "ERROR: run_all.sh: --scope host-infra requires the classification lib at $_H2_CLASSIFICATION_LIB" >&2
        exit 1
    fi
    if [ ! -f "$_H9_LANE_X_FLOCK_LIB" ]; then
        echo "ERROR: run_all.sh: --scope host-infra requires the Lane-X flock lib at $_H9_LANE_X_FLOCK_LIB" >&2
        exit 1
    fi

    # shellcheck disable=SC1090
    source "$_H2_CLASSIFICATION_LIB"
    # shellcheck disable=SC1090
    source "$_H9_LANE_X_FLOCK_LIB"

    # Members = the declared host-exclusive bucket INTERSECT files present in
    # INFRA_DIR ([ -f ] guard mirrors run_all.sh's own discovery predicate),
    # sorted for deterministic discovered-order output.
    _h9_members=()
    while IFS= read -r _h9_name; do
        [ -n "$_h9_name" ] || continue
        [ -f "$INFRA_DIR/$_h9_name" ] || continue
        _h9_members+=("$_h9_name")
    done < <(classification_bucket host-exclusive | sort)

    # Acquire the Lane-X flock ONCE around the whole serial run (coarse
    # single-flight; PRD §4 #5). Capture the rc with `|| _h9_flock_rc=$?` --
    # NOT `if ! lane_x_flock_acquire; then rc=$?`, which would capture the
    # negation's 0 instead of the acquire failure code (75 contention / 64
    # bad WAIT / 1 missing-flock-or-bad-lock-parent).
    _h9_flock_rc=0
    lane_x_flock_acquire || _h9_flock_rc=$?
    if [ "$_h9_flock_rc" -ne 0 ]; then
        echo "ERROR: run_all.sh: --scope host-infra failed to acquire the Lane-X lock (rc=$_h9_flock_rc)" >&2
        exit "$_h9_flock_rc"
    fi
    # Safety-net release on any exit path (normal/INT/TERM/HUP); the explicit
    # release below is the primary path, this is a backstop.
    trap 'lane_x_flock_release' EXIT

    for _h9_name in "${_h9_members[@]+"${_h9_members[@]}"}"; do
        discovered=$((discovered + 1))
        echo ""
        echo "--- Running: $_h9_name ---"
        _h9_child_rc=0
        bash "$INFRA_DIR/$_h9_name" 9<&- || _h9_child_rc=$?
        if [ "$_h9_child_rc" -eq 0 ]; then
            echo "  RESULT: PASS ($_h9_name)"
        else
            echo "  RESULT: FAIL ($_h9_name)"
            failures=$((failures + 1))
            failed_names+=("$_h9_name")
        fi
    done

    lane_x_flock_release
elif [ "$_H2_POOL_ACTIVE" -eq 1 ]; then
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

    # Reject non-integer values -- notably slot_acquire's own "unlimited"
    # sentinel (lib_slot_acquire.sh), which it would otherwise honor verbatim
    # as a no-deadline continuous wait. The pool is documented/designed as a
    # SOFT admission that never hangs (admits unslotted on deadline); letting
    # "unlimited" through would silently defeat that guarantee on a
    # wedged/over-subscribed host-global semaphore. Fail fast instead, before
    # any discovery/execution work, mirroring the REIFY_RUN_ALL_POOL_CONCURRENCY
    # validation style below.
    case "$_H2_POOL_WAIT" in
        ''|*[!0-9]*)
            echo "ERROR: run_all.sh: REIFY_RUN_ALL_POOL_WAIT must be a positive integer number of seconds (got '${_H2_POOL_WAIT}'); the slot_acquire 'unlimited' sentinel is not supported here -- it would defeat the pool's never-hang soft-admission guarantee" >&2
            exit 64
            ;;
    esac
    [ "$_H2_POOL_WAIT" -ge 1 ] || { echo "ERROR: run_all.sh: REIFY_RUN_ALL_POOL_WAIT must be >= 1 (got '${_H2_POOL_WAIT}')" >&2; exit 64; }

    # Observability: report the resolved bound before doing any discovery/
    # execution work, mirroring cargo-test-occt-gated.sh's `INFO: ... N=`
    # idiom. Only emitted on the pool path (not the all-serial fallback).
    echo "INFO: run_all.sh pool: N=${_H2_POOL_N} lock=${_H2_POOL_LOCK}" >&2

    # Discovery/partition/pool-workdir-setup diagnostic guard (task #5123,
    # esc-5080-9): scoped to exactly this block, cleared right after the
    # workdir/EXIT-trap setup below -- see _ra_discovery_diag for why.
    trap '_ra_discovery_diag "$?" "$LINENO" "$BASH_COMMAND"' ERR

    # -- Discovery + partition (pool vs serial; unclassified fail-safes serial) --
    _h2_discovered_list=()
    while IFS= read -r _h2_name; do
        [ -n "$_h2_name" ] && _h2_discovered_list+=("$_h2_name")
    done < <(classification_discovered_set "$INFRA_DIR")

    # H3: drop host-exclusive members from the discovered set when the
    # flip-seam knob is engaged (_h3_exclude, computed above). Filtering here
    # -- before the pool/serial partition -- makes `discovered` and the
    # "=== Summary: N discovered ===" line drop by exactly the excluded
    # count, with no other change to the partition/emit machinery below.
    if [ "${#_h3_exclude[@]}" -gt 0 ]; then
        _h3_kept=()
        for _h2_name in "${_h2_discovered_list[@]}"; do
            [ "${_h3_exclude[$_h2_name]:-0}" = "1" ] || _h3_kept+=("$_h2_name")
        done
        # Guard against expanding an empty array: "${_h3_kept[@]}" on a
        # zero-element array raises 'unbound variable' under set -u on
        # bash < 4.4 (fixed in bash 4.4+, but this script has no minimum
        # bash-version gate). Only hit when the ENTIRE discovered set is
        # host-exclusive; reassign explicitly to stay portable.
        _h2_discovered_list=()
        [ "${#_h3_kept[@]}" -gt 0 ] && _h2_discovered_list=("${_h3_kept[@]}")
    fi

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

    # Discovery/partition/pool-workdir-setup guard ends here -- clear before
    # the PSI-gate definition and Phases 1-3 so it never wraps them.
    trap - ERR

    # -- PSI soft-gate (parent-side, before each pool spawn) ---------------------
    # Paces pool spawns under sustained CPU pressure without ever reducing N,
    # skipping a test, or blocking indefinitely: bounded by
    # load_tolerant_attempts (auto-extends under host load), then admits
    # regardless (soft, mirrors compile_gate's admit-on-timeout fairness
    # floor). Empty/unreadable avg10 admits immediately (fail-open).
    _h2_psi_gate() {
        [ "${REIFY_RUN_ALL_POOL_PSI_DISABLE:-}" = "1" ] && return 0
        local _proc="${REIFY_RUN_ALL_POOL_PSI_PROC_PATH:-/proc/pressure/cpu}"
        local _threshold="${REIFY_RUN_ALL_POOL_PSI_THRESHOLD:-85}"
        local _attempts
        _attempts="$(load_tolerant_attempts "${REIFY_RUN_ALL_POOL_PSI_ATTEMPTS:-3}")"
        local _avg10 _i=0 _yielded=0
        _avg10="$(cpu_admit_read_avg10 "$_proc")"
        [ -z "$_avg10" ] && return 0
        while awk -v p="$_avg10" -v t="$_threshold" 'BEGIN{exit !(p>=t)}'; do
            [ "$_i" -ge "$_attempts" ] && break
            if [ "$_yielded" -eq 0 ]; then
                echo "INFO: run_all.sh pool: PSI backoff (avg10=${_avg10} >= ${_threshold}) -- yielding before next pool spawn" >&2
                _yielded=1
            fi
            sleep 0.2
            _i=$((_i + 1))
            _avg10="$(cpu_admit_read_avg10 "$_proc")"
            [ -z "$_avg10" ] && return 0
        done
        return 0
    }

    # -- Phase 1: pool (concurrent, bounded by the host-global semaphore) --------
    # Bounded worker-pool throttle (task #5129, esc-5029/3848 fork-storm): cap
    # the number of LIVE worker SHELLS at _H2_POOL_N (the same resolved pool
    # concurrency bound slot_acquire uses below -- no new constant). This
    # bounds the PARENT's own fork footprint; slot_acquire remains the
    # host-global concurrency gate inside the worker body, unchanged (INV-1).
    _h2_pids=()
    _h2_active=0
    _h2_peak=0
    for _h2_name in "${_h2_pool_members[@]}"; do
        while [ "$_h2_active" -ge "$_H2_POOL_N" ]; do
            wait -n 2>/dev/null || true
            _h2_active=$((_h2_active - 1))
        done
        _h2_psi_gate
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
        _h2_active=$((_h2_active + 1))
        [ "$_h2_active" -gt "$_h2_peak" ] && _h2_peak=$_h2_active
    done
    if [ "${#_h2_pids[@]}" -gt 0 ]; then
        wait "${_h2_pids[@]}" 2>/dev/null || true
    fi
    if [ "${#_h2_pool_members[@]}" -gt 0 ]; then
        echo "INFO: run_all.sh pool: Phase-1 peak concurrent worker shells=${_h2_peak} (limit ${_H2_POOL_N})" >&2
    fi

    # -- Phase 2: serial (foreground, one at a time, discovered order) -----------
    for _h2_name in "${_h2_serial_members[@]}"; do
        _h2_i="${_h2_index_of[$_h2_name]}"
        _h2_rc=0
        bash "$INFRA_DIR/$_h2_name" > "$_H2_WORKDIR/${_h2_i}.out" 2>&1 || _h2_rc=$?
        echo "$_h2_rc" > "$_H2_WORKDIR/${_h2_i}.rc"
    done

    # -- Phase 2.5: serial retry-once of failed pool members (deflake) -----------
    # Re-run each FAILED pool-bucket member ONCE, serially, in the foreground,
    # AFTER both the concurrent pool (Phase 1) and serial (Phase 2) phases have
    # finished -- the quietest point of the run for host load. Pool members are
    # hermetic by classification (H2/4924), so re-running one is side-effect-
    # free. A member that passes on retry is NOT counted as a failure
    # (flaky_names, not failed_names/failures) -- see Phase 3 below; a member
    # that fails twice keeps the exact existing FAILED contract. Exactly ONE
    # retry per failed pool member; no slot_acquire/PSI gate (already serial).
    declare -A _h2_retried=()
    declare -A _h2_retry_rc=()
    for _h2_name in "${_h2_pool_members[@]}"; do
        _h2_i="${_h2_index_of[$_h2_name]}"
        _h2_first_rc="$(cat "$_H2_WORKDIR/${_h2_i}.rc" 2>/dev/null || echo 1)"
        [ "$_h2_first_rc" -eq 0 ] && continue
        _h2_retry_rc_val=0
        bash "$INFRA_DIR/$_h2_name" > "$_H2_WORKDIR/${_h2_i}.retry.out" 2>&1 || _h2_retry_rc_val=$?
        echo "$_h2_retry_rc_val" > "$_H2_WORKDIR/${_h2_i}.retry.rc"
        _h2_retried["$_h2_name"]=1
        _h2_retry_rc["$_h2_name"]="$_h2_retry_rc_val"
    done

    # -- Phase 3: emit (discovered/sorted order -- preserves the output contract) --
    for _h2_name in "${_h2_discovered_list[@]}"; do
        _h2_i="${_h2_index_of[$_h2_name]}"
        echo ""
        echo "--- Running: $_h2_name ---"
        if [ "${_h2_retried[$_h2_name]:-0}" = "1" ]; then
            # Retried pool member: archive BOTH attempts under this SAME
            # header, using attempt-delimiter lines that do NOT match
            # `^--- Running: ` so the discovered-order header-list contract
            # (one header per discovered test) is unaffected.
            echo "--- attempt 1 (concurrent pool) ---"
            _ra_emit_sanitized "$_H2_WORKDIR/${_h2_i}.out"
            echo "--- attempt 2 (serial retry) ---"
            _ra_emit_sanitized "$_H2_WORKDIR/${_h2_i}.retry.out"
            _h2_rc="${_h2_retry_rc[$_h2_name]}"
            if [ "$_h2_rc" -eq 0 ]; then
                echo "  RESULT: PASS ($_h2_name) [flaky: passed on serial retry]"
                flaky_names+=("$_h2_name")
            else
                echo "  RESULT: FAIL ($_h2_name)"
                failures=$((failures + 1))
                failed_names+=("$_h2_name")
            fi
        else
            _ra_emit_sanitized "$_H2_WORKDIR/${_h2_i}.out"
            _h2_rc="$(cat "$_H2_WORKDIR/${_h2_i}.rc" 2>/dev/null || echo 1)"
            if [ "$_h2_rc" -eq 0 ]; then
                echo "  RESULT: PASS ($_h2_name)"
            else
                echo "  RESULT: FAIL ($_h2_name)"
                failures=$((failures + 1))
                failed_names+=("$_h2_name")
            fi
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

        # H3: skip host-exclusive members when the flip-seam knob is engaged
        # (_h3_exclude, computed above the pool-vs-legacy branch).
        [ "${_h3_exclude[$basename]:-0}" = "1" ] && continue

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
if [ "${#flaky_names[@]}" -gt 0 ]; then
    echo "=== Summary: $discovered discovered, $failures failed, ${#flaky_names[@]} flaky-retried ==="
else
    echo "=== Summary: $discovered discovered, $failures failed ==="
fi
if [ "${#failed_names[@]}" -gt 0 ]; then
    echo "=== FAILED: ${failed_names[*]} ==="
    printf 'FAILED %s\n' "${failed_names[*]}"
fi
if [ "${#flaky_names[@]}" -gt 0 ]; then
    echo "=== FLAKY (passed on serial retry): ${flaky_names[*]} ==="
fi

if [ "$failures" -eq 0 ]; then
    exit 0
else
    exit 1
fi
