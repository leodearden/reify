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
# Worktree-removal interruption (task #5261, W4e): if this suite's own
# INFRA_DIR is removed out from under it mid-run (e.g. the _merge-verify
# worktree gutted by a restart), run_all.sh emits a SINGLE line-anchored
# "=== INTERRUPTED (worktree removed) ===" marker to stdout and exits 99
# instead of letting each already-discovered member fail individually with
# "No such file or directory" (rc 127). 99 is distinct from every other
# exit code this script/its envelope uses (0 pass, 1 fail, 64 usage, 75 H9
# flock contention, 124 timeout, 127 not-found, 128+n signals) so
# dark-factory's ENV_TRANSIENT classifier can key off it directly instead of
# misreading a gutted worktree as N test failures. This self-check is
# strictly additive: it is only "armed" (able to fire at all) when
# INFRA_DIR held this script's own run_all.sh at STARTUP -- true only for a
# real no-arg `bash run_all.sh` invocation, never for a test fixture whose
# INFRA_DIR points at a synthetic temp dir. See _ra_interrupt_if_worktree_gone
# below.
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
#                                   (default /tmp/reify-run-all-pool-$(id -u).lock)
#                                   -- a FIXED host-global path, independent of
#                                   TMPDIR, so a caller-private TMPDIR (a common
#                                   per-lane isolation pattern) cannot fork the
#                                   lock namespace (task 5145).
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
#   REIFY_RUN_ALL_MEMBER_SUBSET        Space-delimited test_*.sh basenames
#                                      (task #5288; PRD verify-retry-failed-
#                                      only.md task beta, Sec 4.2/8). When
#                                      non-empty, run_all.sh runs ONLY the
#                                      named members -- a dedicated serial
#                                      branch reusing the Phase-2.5 per-
#                                      member serial-foreground-captured
#                                      invoke shape, single attempt each (no
#                                      internal deflake retry: the subset run
#                                      IS dark-factory's retry). Every OTHER
#                                      discovered member is reported skipped
#                                      ("--- Skipped (member-subset): <n> ---"
#                                      / "RESULT: SKIP (<n>)") -- acknowledged,
#                                      never invoked, never counted as a
#                                      failure; the skipped count surfaces on
#                                      the Summary line as "N member-subset-
#                                      skipped". A name in the knob not found
#                                      in INFRA_DIR emits a loud stderr
#                                      WARNING and is ignored (not a
#                                      failure). Unset/empty/whitespace-only
#                                      runs the FULL discovered set unchanged
#                                      (additive default, mirrors
#                                      EXCLUDE_HOST_INFRA's DA1 ethos) -- a
#                                      malformed knob must never silently run
#                                      zero tests. Set by dark-factory on a
#                                      merge-gate retry (D2), never by
#                                      default.
#   REIFY_RUN_ALL_POOL_FORK_FAIL_MEMBER   fault-injection seam (task #5129):
#                                      when set to a pool-bucket member's
#                                      basename, that ONE member is degraded
#                                      to a failure instead of spawned (see
#                                      _h2_degrade_member) -- the suite still
#                                      completes with the normal Summary/
#                                      FAILED contract. Empirically, a real
#                                      fork() EAGAIN is uncatchable/fatal in
#                                      bash (exit 254 even under `set +e`), so
#                                      this seam is how the graceful-degrade
#                                      path is exercised deterministically;
#                                      the `wait -n` worker-pool throttle
#                                      above is the actual fork-storm defense.
#                                      Unset by default -- test-only, never
#                                      set in normal operation.
#   REIFY_RUN_ALL_PROGRESS_SECS        Phase-1 single-writer progress-heartbeat
#                                      cadence in seconds (task #5130; default
#                                      30, mirrors REIFY_CLOCK_HEARTBEAT_SECS).
#                                      A background printer emits `INFO:
#                                      run_all.sh pool progress: X/Y complete,
#                                      elapsed Ns` on stderr at this cadence
#                                      (sleeps-FIRST -- a pool finishing under
#                                      one interval stays silent) so a
#                                      contended/slow pool is attributable on
#                                      the live stream instead of a fully
#                                      buffered black box. Pure observability,
#                                      strictly additive (INV-4): a malformed
#                                      value fails OPEN to 30 rather than
#                                      exiting, unlike the load-bearing knobs
#                                      above.
#   REIFY_RUN_ALL_FLAKY_LEDGER         Path to the durable append-only JSONL
#                                      FLAKY ledger (task #5142; default
#                                      $_H2_REPO_ROOT/data/verify-logs/
#                                      flaky-ledger.jsonl, git-ignored).
#                                      Every `=== FLAKY (passed on serial
#                                      retry) ===` emission (Phase 2.5
#                                      deflake above) appends one JSON line
#                                      {ts,test,role,task,branch,run_id} here,
#                                      crash-safely (flock). Pure
#                                      observability, strictly additive
#                                      (INV-4): a missing jq/flock or an
#                                      unwritable path silently no-ops rather
#                                      than exiting -- see
#                                      _ra_flaky_ledger_append.
#   REIFY_RUN_ALL_FLAKY_LEDGER_DISABLE Set to 1 to disable the FLAKY ledger
#                                      entirely (break-glass; default unset).
#   REIFY_RUN_ALL_FLAKY_CHRONIC_N      Chronic-offender WARNING threshold
#                                      (task #5142; default 3). Once a pool
#                                      member appears in >=N of the last
#                                      REIFY_RUN_ALL_FLAKY_CHRONIC_M distinct
#                                      recorded runs in the FLAKY ledger, a
#                                      loud `WARNING: chronic flaky member
#                                      ...` line fires on stderr (see
#                                      _ra_flaky_chronic_check). Pure
#                                      observability, strictly additive
#                                      (INV-4): does NOT hard-fail the run --
#                                      that policy is deliberately deferred
#                                      to Leo (2026-07-08 flakiness survey).
#                                      A malformed value fails OPEN to 3.
#   REIFY_RUN_ALL_FLAKY_CHRONIC_M      Chronic-offender scan window: the
#                                      number of most-recent DISTINCT
#                                      recorded run_ids to consider (task
#                                      #5142; default 20). A malformed value
#                                      fails OPEN to 20.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Per-run identifier for the FLAKY ledger (task #5142): stamps every ledger
# line so the chronic-offender scan (_ra_flaky_chronic_check) can window
# over "the last M DISTINCT runs" exactly, even though a single run can
# flag multiple members (one ledger line each). EPOCHSECONDS (bash 5.0+)
# falls back to `date +%s` for portability; $$ disambiguates two runs that
# start in the same second.
_RA_RUN_ID="${EPOCHSECONDS:-$(date +%s)}-$$"

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
#
# FLAKY ledger (task #5142): snapshot the INBOUND role before normalizing it
# below -- from this line on DF_VERIFY_ROLE is always "task", so the
# meaningful merge-vs-task/branch signal for ledger triage only exists here.
_RA_INBOUND_ROLE="${DF_VERIFY_ROLE:-unknown}"
export DF_VERIFY_ROLE=task

# Worktree-removal self-check "armed" gate (task #5261, W4e). The merge
# suite runs `bash run_all.sh` with NO positional arg, so INFRA_DIR ==
# SCRIPT_DIR and $INFRA_DIR/run_all.sh IS the running script -- present at
# startup, gone if the worktree is later removed mid-run. Capturing presence
# ONCE, here, makes the check strictly additive: every existing/other test
# fixture points INFRA_DIR at a temp dir that never contained run_all.sh, so
# those runs are NEVER armed and _ra_worktree_gone (below) stays completely
# inert for them -- only a run whose INFRA_DIR held run_all.sh at start and
# lost it mid-run can ever trip it.
_RA_SELFCHECK_SENTINEL="$INFRA_DIR/run_all.sh"
_RA_SELFCHECK_ARMED=0
[ -e "$_RA_SELFCHECK_SENTINEL" ] && _RA_SELFCHECK_ARMED=1
# Distinct from every exit code run_all.sh/its envelope already uses (0
# pass, 1 any-fail, 64 usage, 75 H9 flock contention, 124 GNU-timeout, 127
# command-not-found, 128+n signals incl. 143 SIGTERM) -- gives dark-factory's
# ENV_TRANSIENT classifier a clean numeric anchor alongside the marker line.
_RA_INTERRUPTED_EXIT_CODE=99

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

# ---------------------------------------------------------------------------
# _ra_flaky_ledger_append <name>  (task #5142)
#
# Persists one JSONL line to the durable FLAKY ledger (_RA_FLAKY_LEDGER)
# recording that <name> passed on the Phase-2.5 serial retry this run --
# otherwise the only trace of a "flaky" reclassification is the transient
# `=== FLAKY ...` stdout line, which nothing archives or counts across runs.
#
# Pure observability, strictly additive (INV-4): fails OPEN (silent no-op)
# when REIFY_RUN_ALL_FLAKY_LEDGER_DISABLE=1, jq or flock is missing, or the
# ledger path/parent is unwritable -- a pure-observability subsystem must
# never break the suite. Crash-safe append via the repo's subshell-flock
# idiom (mirrors scripts/lib_slot_acquire.sh / lib_lane_x_flock.sh) into a
# dedicated `.lock` sibling file, so lock acquisition never contends with a
# reader opening the ledger itself.
# ---------------------------------------------------------------------------
_ra_flaky_ledger_append() {
    local _name="$1"
    [ "${REIFY_RUN_ALL_FLAKY_LEDGER_DISABLE:-}" = "1" ] && return 0
    command -v jq >/dev/null 2>&1 || return 0
    command -v flock >/dev/null 2>&1 || return 0

    local _branch _task _line
    _branch="$(git -C "$INFRA_DIR" rev-parse --abbrev-ref HEAD 2>/dev/null || true)"
    [ -n "$_branch" ] || _branch="unknown"
    case "$_branch" in
        task/*) _task="${_branch#task/}" ;;
        *) _task="unknown" ;;
    esac

    _line="$(jq -cn \
        --arg ts "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
        --arg test "$_name" \
        --arg role "$_RA_INBOUND_ROLE" \
        --arg task "$_task" \
        --arg branch "$_branch" \
        --arg run_id "$_RA_RUN_ID" \
        '{ts:$ts,test:$test,role:$role,task:$task,branch:$branch,run_id:$run_id}' 2>/dev/null || true)"
    [ -n "$_line" ] || return 0

    mkdir -p "$(dirname "$_RA_FLAKY_LEDGER")" 2>/dev/null || true
    ( flock 9 || exit 0; printf '%s\n' "$_line" >> "$_RA_FLAKY_LEDGER" ) 9>>"${_RA_FLAKY_LEDGER}.lock" 2>/dev/null || true
    return 0
}

# ---------------------------------------------------------------------------
# _ra_flaky_chronic_check <name>  (task #5142)
#
# Scans the durable FLAKY ledger (_RA_FLAKY_LEDGER) and, when <name> appears
# in >=_ra_chronic_n of the last _ra_chronic_m DISTINCT recorded run_ids,
# emits a loud `WARNING: chronic flaky member ...` line to STDERR. "Last M
# runs" = the last M distinct run_ids present in the ledger file, which is
# append-only in chronological order -- a run with zero flaky members writes
# no ledger line by design, so M is necessarily measured over recorded/
# flaky runs only (see design_decisions in the task plan).
#
# Pure observability, strictly additive (INV-4): this NEVER touches the
# exit code or any stdout line -- hard-failing on repeat offenders is a
# policy explicitly deferred to Leo (2026-07-08 flakiness survey; task
# #5142). Fails OPEN (silent no-op) when REIFY_RUN_ALL_FLAKY_LEDGER_DISABLE=1,
# jq is missing, or the ledger file is absent/empty -- a pure-observability
# subsystem must never break the suite.
# ---------------------------------------------------------------------------
_ra_flaky_chronic_check() {
    local _name="$1"
    [ "${REIFY_RUN_ALL_FLAKY_LEDGER_DISABLE:-}" = "1" ] && return 0
    command -v jq >/dev/null 2>&1 || return 0
    [ -s "$_RA_FLAKY_LEDGER" ] || return 0

    local _count
    _count="$(jq -r '[.run_id,.test]|@tsv' "$_RA_FLAKY_LEDGER" 2>/dev/null | awk -F'\t' -v name="$_name" -v m="$_ra_chronic_m" '
        {
            if (!($1 in seen)) {
                order[++n] = $1
                seen[$1] = 1
            }
            if ($2 == name) {
                hit[$1] = 1
            }
        }
        END {
            start = n - m + 1
            if (start < 1) start = 1
            count = 0
            for (i = start; i <= n; i++) {
                if (hit[order[i]] == 1) count++
            }
            print count
        }
    ' 2>/dev/null || true)"
    case "$_count" in
        ''|*[!0-9]*) return 0 ;;
    esac
    [ "$_count" -ge "$_ra_chronic_n" ] || return 0

    echo "WARNING: chronic flaky member $_name (flaky in $_count of last $_ra_chronic_m recorded runs; N=$_ra_chronic_n) -- run_all.sh serial retry-once is masking it; observability only, not hard-failing (policy deferred, task #5142)" >&2
    return 0
}

# ---------------------------------------------------------------------------
# _ra_partial_failed_names  (task #5147)
#
# Best-effort scan of the H2 pool workdir's per-member `.rc` bookkeeping
# (see the `.rc` writes in the pool/serial/retry phases below) for members
# that have already recorded a nonzero exit code -- used by _ra_on_term to
# attribute a partial FAILED marker when the outer timeout SIGTERMs mid-run,
# long before the normal Phase 3 emit block would otherwise run. Prints one
# name per line; prints nothing (silently) on the legacy/host-infra paths,
# which never set _H2_WORKDIR.
#
# Fail-open: this runs from inside a signal handler, so an error here must
# never prevent _ra_on_term from emitting its classifier marker -- every
# dereference below is guarded so a missing/empty _H2_WORKDIR, an unreadable
# `.rc` file, or an out-of-range index just yields whatever was gathered so
# far, never an error under set -u/set -e.
# ---------------------------------------------------------------------------
_ra_partial_failed_names() {
    [ -n "${_H2_WORKDIR:-}" ] && [ -d "${_H2_WORKDIR:-}" ] || return 0
    local _f _base _idx _rc _name
    for _f in "$_H2_WORKDIR"/*.rc; do
        [ -f "$_f" ] || continue
        # Exclude serial-retry .rc files -- mirrors the Phase-1 progress
        # printer's `! -name '*.retry.rc'` exclusion above: a retry .rc
        # shares its base member's index and is reconciled separately by
        # Phase 2.5/3, not part of the first-attempt bookkeeping this scan
        # reads.
        case "$_f" in
            *.retry.rc) continue ;;
        esac
        _base="$(basename "$_f" .rc)"
        [[ "$_base" =~ ^[0-9]+$ ]] || continue
        _idx="$_base"
        _rc="$(cat "$_f" 2>/dev/null || echo 0)"
        [[ "$_rc" =~ ^[0-9]+$ ]] || _rc=0
        [ "$_rc" -eq 0 ] && continue
        _name="${_h2_discovered_list[$((_idx-1))]:-}"
        [ -n "$_name" ] && printf '%s\n' "$_name"
    done
    return 0
}

# ---------------------------------------------------------------------------
# _ra_on_term  (task #5147)
#
# TERM-signal handler: installed immediately below (before ANY phase work),
# so it covers both the pool and legacy-serial paths alike. The merge-tier
# envelope wraps run_all.sh in `timeout --kill-after=60 30m` (verify.sh) --
# timeout sends SIGTERM first (SIGKILL only after the kill-after grace
# period), so this handler gets a real window to reclassify a mid-run kill
# BEFORE dark-factory's classifier falls through to a tree_sitter_generate_error
# mislabel for want of any Summary/FAILED line (the normal-path emit block,
# lines ~988-997 below, never runs on a signal death).
#
# Emits, to STDOUT (the same stream the normal-path marker uses and DF's
# `^FAILED\s` scan reads):
#   - a distinct `=== Summary: INTERRUPTED ... ===` line, deliberately NOT
#     matching the byte-exact `=== Summary: N discovered, M failed ===`
#     substring the contract tests assert (T9a/T13b/T22e class).
#   - `=== FAILED: <names> (partial) ===` (human-readable) and the bare
#     `FAILED <names> (partial)` classifier line (matches DF's `^FAILED\s`
#     reclassifier -- pattern #7b, checked before the tree-sitter mislabel).
#
# Name attribution is best-effort and fail-open: the sorted-unique union of
# _ra_partial_failed_names (the .rc-file scan) and the existing failed_names
# array (covers the legacy path and any Phase-3 partial progress) -- the
# guaranteed deliverable is the classifier reclassification itself, so a
# gather error must never be able to suppress the marker. `set +e` covers
# the whole handler for exactly this reason.
#
# DELIBERATE even when the name union is empty (SIGTERM landed before any
# member recorded a nonzero exit -- e.g. all workers still in-flight): the
# bare `FAILED (partial)` classifier line is still emitted rather than a
# distinct not-yet-failed token, so an interrupted run with no confirmed
# member failure is still over-classified as a failure. A run that never
# reached a Summary line is abnormal regardless of attribution, and a
# distinct empty-names token (e.g. "INTERRUPTED (partial)") would only be
# safe once dark-factory's classifier is confirmed to recognize it -- a
# cross-repo change out of scope here (CLAUDE.md "Cross-repo seams": reify
# ships the primitive, dark-factory wires the invocation). Over-classifying
# a bare interrupt as a failure is the accepted tradeoff over risking a
# silent fall-through back to the tree_sitter_generate_error mislabel.
#
# Re-raises via `trap - TERM; kill -TERM $$` (rather than `exit`) so the
# process's recorded exit status reflects signal death (143), and so the
# EXISTING `_H2_WORKDIR` EXIT-trap cleanup (below) still runs AFTER this
# handler returns -- this handler's `.rc` scan above therefore sees the
# workdir before that cleanup removes it.
# ---------------------------------------------------------------------------
_ra_on_term() {
    set +e

    echo ""
    echo "=== Summary: INTERRUPTED (outer timeout; partial results) ==="

    local _names
    _names="$(
        { _ra_partial_failed_names; printf '%s\n' "${failed_names[@]:-}"; } 2>/dev/null \
            | sed '/^$/d' | sort -u | tr '\n' ' ' | sed 's/ $//'
    )"

    # Unconditional even when $_names is empty -- see "DELIBERATE" above.
    echo "=== FAILED: ${_names} (partial) ==="
    printf 'FAILED %s(partial)\n' "${_names:+$_names }"

    trap - TERM
    kill -TERM $$
}

# ---------------------------------------------------------------------------
# _ra_worktree_gone / _ra_interrupt_if_worktree_gone  (task #5261, W4e)
#
# _ra_worktree_gone: true (rc 0) only when the startup self-check was ARMED
# (_RA_SELFCHECK_ARMED=1) AND the sentinel is now missing -- see the "armed"
# gate comment above _RA_SELFCHECK_SENTINEL. Never true for any run that was
# not armed at startup (INV: strictly additive -- every existing fixture is
# unaffected).
#
# _ra_interrupt_if_worktree_gone: no-op unless _ra_worktree_gone; otherwise
# emits a line-anchored "=== INTERRUPTED (worktree removed) ===" marker to
# STDOUT (the same stream dark-factory's classifier reads) and exits
# _RA_INTERRUPTED_EXIT_CODE (99), bypassing the normal Summary/FAILED block
# entirely. The existing _H2_WORKDIR EXIT trap (pool path) still runs on
# this exit, cleaning up /tmp state and stopping the progress printer.
#
# MUST be called only from the MAIN shell (never inside a `( ) &` worker,
# where `exit` would only terminate the subshell, leaving the main shell
# none the wiser) -- see call sites below.
# ---------------------------------------------------------------------------
_ra_worktree_gone() {
    [ "$_RA_SELFCHECK_ARMED" = "1" ] || return 1
    [ -e "$_RA_SELFCHECK_SENTINEL" ] && return 1
    return 0
}

_ra_interrupt_if_worktree_gone() {
    _ra_worktree_gone || return 0
    echo ""
    echo "=== INTERRUPTED (worktree removed) ==="
    exit "${_RA_INTERRUPTED_EXIT_CODE:-99}"
}

failures=0
discovered=0
failed_names=()
flaky_names=()
member_subset_skipped=()

# Install the TERM trap BEFORE any phase work (pool or legacy), so a
# mid-run outer-timeout SIGTERM is reclassified regardless of which path is
# active (task #5147). See _ra_on_term above.
trap _ra_on_term TERM

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

# FLAKY ledger path (task #5142): overridable for test isolation; defaults
# under the repo's git-ignored data/verify-logs/ tree (.git/info/exclude),
# so the default file is never accidentally committed.
_RA_FLAKY_LEDGER="${REIFY_RUN_ALL_FLAKY_LEDGER:-$_H2_REPO_ROOT/data/verify-logs/flaky-ledger.jsonl}"

# Chronic-offender scan thresholds (task #5142): resolved unconditionally
# (common to both the pool and legacy-serial paths, mirroring _RA_FLAKY_LEDGER
# above) since the Summary block that consumes them runs on every path. Pure
# observability / strictly additive (INV-4), mirroring the
# REIFY_RUN_ALL_PROGRESS_SECS idiom below: a malformed value fails OPEN to
# the default rather than exiting -- see _ra_flaky_chronic_check.
_ra_chronic_n="${REIFY_RUN_ALL_FLAKY_CHRONIC_N:-3}"
case "$_ra_chronic_n" in
    ''|*[!0-9]*) _ra_chronic_n=3 ;;
esac
[ "$_ra_chronic_n" -ge 1 ] || _ra_chronic_n=3

_ra_chronic_m="${REIFY_RUN_ALL_FLAKY_CHRONIC_M:-20}"
case "$_ra_chronic_m" in
    ''|*[!0-9]*) _ra_chronic_m=20 ;;
esac
[ "$_ra_chronic_m" -ge 1 ] || _ra_chronic_m=20

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

# ---------------------------------------------------------------------------
# REIFY_RUN_ALL_MEMBER_SUBSET tokenization (task #5288): computed ONCE here,
# before the branch chain, mirroring _h3_exclude immediately above. Unquoted
# expansion is deliberate -- it word-splits the space-delimited knob value;
# empty tokens (leading/trailing/repeated whitespace) are skipped. An unset,
# empty, or whitespace-only value yields an EMPTY set, so the branch below
# (`elif [ "${#_ra_member_subset[@]}" -gt 0 ]`) never engages and the
# UNCHANGED pool/legacy path runs the full discovered set -- a malformed
# knob must never silently run zero tests (mirrors EXCLUDE_HOST_INFRA's
# additive-default/DA1 ethos).
# ---------------------------------------------------------------------------
declare -A _ra_member_subset=()
for _ra_subset_tok in ${REIFY_RUN_ALL_MEMBER_SUBSET:-}; do
    [ -n "$_ra_subset_tok" ] && _ra_member_subset["$_ra_subset_tok"]=1
done

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
elif [ "${#_ra_member_subset[@]}" -gt 0 ]; then
    # -------------------------------------------------------------------------
    # REIFY_RUN_ALL_MEMBER_SUBSET dedicated serial branch (task #5288; PRD
    # docs/prds/verify-retry-failed-only.md task beta, Sec 4.2/8): on a
    # merge-gate retry, dark-factory names the FAILED members from a prior
    # attempt and this branch runs ONLY those -- reusing the Phase-2.5
    # per-member serial-foreground-captured invoke shape -- reporting every
    # OTHER discovered member as skipped (acknowledged, never invoked, never
    # a failure). Mirrors the --scope host-infra branch's shape: a
    # self-contained serial loop that sets discovered/failures/failed_names
    # and falls through to the shared Summary/FAILED/exit tail below. Plain
    # glob discovery (test_*.sh minus test_helpers.sh, matching the legacy
    # branch's own predicate/order) -- no classification-substrate
    # dependency, so this branch never consults _h3_exclude/
    # REIFY_RUN_ALL_EXCLUDE_HOST_INFRA (subset mode is authoritative) nor the
    # pool/slot_acquire/PSI machinery. SINGLE attempt per named member: the
    # subset run IS dark-factory's retry, so an internal re-retry here would
    # mask a real retry-time failure.
    #
    # Clock-marker sanitization (task #5288 pair 2; task 4998/esc-4791-52
    # class): unlike --scope host-infra / legacy (off the DF-parsed verify
    # stream), a subset retry runs ON it -- so each named member's output is
    # captured to a reused temp file and re-emitted via _ra_emit_sanitized
    # (:274), exactly the concurrent-pool Phase-3 replay shape, instead of
    # streaming directly to stdout.
    # -------------------------------------------------------------------------
    _ra_subset_tmp="$(mktemp "${TMPDIR:-/tmp}/reify-run-all-subset.XXXXXX")"
    _ra_subset_matched=()
    for _ra_subset_file in "$INFRA_DIR"/test_*.sh; do
        # If glob matches nothing, the literal pattern string is returned — skip it.
        [ -f "$_ra_subset_file" ] || continue

        _ra_subset_base="$(basename "$_ra_subset_file")"
        [ "$_ra_subset_base" = "test_helpers.sh" ] && continue

        discovered=$((discovered + 1))
        if [ "${_ra_member_subset[$_ra_subset_base]:-0}" = "1" ]; then
            _ra_subset_matched+=("$_ra_subset_base")
            echo ""
            echo "--- Running: $_ra_subset_base ---"
            _ra_subset_rc=0
            # Hermetic-harness isolation, mirroring the DF_VERIFY_ROLE
            # normalization above: a named member must run as a clean,
            # normal, unfiltered run of ITSELF. Without `env -u`, a member
            # that itself shells out to run_all.sh internally for its own
            # fixture testing (e.g. this very file, or
            # test_run_all_clock_marker_sanitize.sh) would inherit THIS
            # invocation's REIFY_RUN_ALL_MEMBER_SUBSET value and misapply
            # the SAME name filter to its own unrelated nested fixture
            # dirs -- turning "run everything in my temp fixture" into
            # "skip everything, nothing here is named test_x.sh" and
            # corrupting that member's self-tests. The subset knob governs
            # only the outer/top-level discovery, never a member's own
            # nested invocations.
            env -u REIFY_RUN_ALL_MEMBER_SUBSET \
                bash "$INFRA_DIR/$_ra_subset_base" > "$_ra_subset_tmp" 2>&1 || _ra_subset_rc=$?
            _ra_emit_sanitized "$_ra_subset_tmp"
            if [ "$_ra_subset_rc" -eq 0 ]; then
                echo "  RESULT: PASS ($_ra_subset_base)"
            else
                echo "  RESULT: FAIL ($_ra_subset_base)"
                failures=$((failures + 1))
                failed_names+=("$_ra_subset_base")
            fi
        else
            echo ""
            echo "--- Skipped (member-subset): $_ra_subset_base ---"
            echo "  RESULT: SKIP ($_ra_subset_base)"
            member_subset_skipped+=("$_ra_subset_base")
        fi
    done
    rm -f "$_ra_subset_tmp"

    # Unmatched subset entries: named in the knob but not present in
    # INFRA_DIR (typo/stale retry name) -- a loud stderr WARNING, never a
    # failure; the run must not be sunk by a construction bug in the
    # allow-list itself.
    for _ra_subset_name in "${!_ra_member_subset[@]}"; do
        _ra_subset_found=0
        for _ra_subset_m in "${_ra_subset_matched[@]+"${_ra_subset_matched[@]}"}"; do
            if [ "$_ra_subset_m" = "$_ra_subset_name" ]; then
                _ra_subset_found=1
                break
            fi
        done
        if [ "$_ra_subset_found" -eq 0 ]; then
            echo "WARNING: run_all.sh: REIFY_RUN_ALL_MEMBER_SUBSET member '$_ra_subset_name' not found in $INFRA_DIR (ignored)" >&2
        fi
    done
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

    # Fixed /tmp base, independent of TMPDIR (task 5145) -- mirrors the
    # scripts/lib_test_semaphore.sh `_test_semaphore_default_lock` resolver's
    # identical policy. Keep both host-global lock defaults in sync if either
    # changes.
    _H2_POOL_LOCK="${REIFY_RUN_ALL_POOL_LOCK:-/tmp/reify-run-all-pool-$(id -u).lock}"
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

    # Clock-stop reason token for the pool worker's slot_acquire wait (task
    # #5147). Passed as slot_acquire's optional 4th REASON arg so a REAL
    # host-global pool-lock wait emits @@REIFY_CLOCK_{STOP,HEARTBEAT,START}@@
    # markers to stderr (scripts/lib_slot_acquire.sh / lib_clock_stop.sh) --
    # the exact span dark_factory:1916 reads to pause/resume its wall-clock
    # verify-timeout budget; without it a wait against this host-global lock
    # (up to REIFY_RUN_ALL_POOL_WAIT seconds) burns that budget invisibly.
    # `test_slot_starvation` is lib_clock_stop.sh's own REASON VOCABULARY
    # token for "held-slot semaphore wait (lib_slot_acquire.sh)" -- exactly
    # this call site -- reused rather than minting a new one so DF's
    # clock-stop parser already recognizes it. The marker rides the worker
    # subshell's inherited parent stderr (fd 2), NOT the per-member `.out`
    # capture (the `> .out 2>&1` redirect below is scoped to the member
    # `bash` command only), so it is never rewritten by
    # _RA_CLOCK_SANITIZE/_ra_emit_sanitized, which only touches the Phase-3
    # `.out` re-emission.
    _H2_POOL_CLOCK_REASON="test_slot_starvation"

    # Phase-1 single-writer progress-heartbeat cadence (task #5130). Pure
    # observability / strictly additive (INV-4): unlike the load-bearing
    # knobs above, a malformed value fails OPEN to 30 instead of exiting --
    # a pure-observability knob must never be able to break the suite.
    _h2_progress_secs="${REIFY_RUN_ALL_PROGRESS_SECS:-30}"
    case "$_h2_progress_secs" in
        ''|*[!0-9]*) _h2_progress_secs=30 ;;
    esac
    [ "$_h2_progress_secs" -ge 1 ] || _h2_progress_secs=30

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

    # W5b (task #5261): membership map for the `intra-run-serial` bucket,
    # mirroring the _h2_is_pool idiom above. Used to extend Phase 2.5's
    # deflake retry to intra-run-serial members (they already run serially
    # in Phase 2, so a same-mode serial re-run is cheap and sound) and to
    # pick Phase 3's truthful attempt-1 label -- see both below. Deliberately
    # NOT used to change the Phase 1/2 partition itself (still driven solely
    # by _h2_is_pool, unchanged).
    declare -A _h2_is_intra_serial=()
    while IFS= read -r _h2_name; do
        [ -n "$_h2_name" ] && _h2_is_intra_serial["$_h2_name"]=1
    done < <(classification_bucket intra-run-serial)

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
    # amend (task #5130): also stop the single-writer progress printer (once
    # started, see _h2_progress_pid below) on ANY exit path, not just the
    # normal one. `_h2_progress_pid` does not exist yet at this line, but the
    # trap body is single-quoted so expansion is deferred to fire-time, by
    # which point it is set if Phase 1 ever started the printer. The explicit
    # kill after the Phase-1 join (below) remains the primary stop path; this
    # is a backstop for a signal-driven exit (INT/TERM) that unwinds through
    # this trap first -- same "explicit path primary, EXIT trap backstop"
    # shape as lane_x_flock_release above (:384). A SIGKILL to the parent
    # still bypasses this (and every) trap; that residual case is left to the
    # orphaned-test-binary reaper, per docs/notes/orphaned-test-binary-reaper.md.
    trap '[ -n "${_h2_progress_pid:-}" ] && kill "$_h2_progress_pid" 2>/dev/null || true; rm -rf "$_H2_WORKDIR"' EXIT

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

    # -- Fork-EAGAIN degrade path (task #5129, esc-3848) -------------------------
    # A real async `( ) &` fork EAGAIN is uncatchable/fatal in bash (verified
    # empirically on this host, bash 5.2: the shell prints "fork: Resource
    # temporarily unavailable" and exits 254 regardless of `set +e`/
    # `|| rc=$?`) -- reproducing it requires exhausting the host's process
    # limit, which would also kill the test harness. The bounded `wait -n`
    # throttle above is therefore the REAL fork-storm defense; this helper
    # makes the graceful single-member degrade contract explicit and
    # deterministically testable via the REIFY_RUN_ALL_POOL_FORK_FAIL_MEMBER
    # fault-injection seam below (also the landing spot for any future
    # catchable spawn-failure signal). Routes the failure through the
    # EXISTING per-member .out/.rc protocol so Phases 2/2.5/3 (serial retry,
    # FLAKY ledger, discovered-order replay) consume it unchanged (INV-2).
    _h2_degrade_member() {
        local _name="$1" _idx="$2"
        echo "ERROR: run_all.sh pool: fork() EAGAIN for ${_name} under load -- degrading this member to a failure (esc-3848 class; suite continues)" >&2
        echo "run_all.sh pool: fork() EAGAIN for ${_name} -- degraded to a failure" > "$_H2_WORKDIR/${_idx}.out"
        echo 1 > "$_H2_WORKDIR/${_idx}.rc"
    }

    # -- Phase 1: pool (concurrent, bounded by the host-global semaphore) --------
    # Bounded worker-pool throttle (task #5129, esc-5029/3848 fork-storm): cap
    # the number of LIVE worker SHELLS at _H2_POOL_N (the same resolved pool
    # concurrency bound slot_acquire uses below -- no new constant). This
    # bounds the PARENT's own fork footprint; slot_acquire remains the
    # host-global concurrency gate inside the worker body, unchanged (INV-1).
    #
    # `wait -n` is given the explicit `_h2_pids` list (not called bare) so it
    # only ever reaps OUR worker shells. Argument-less `wait -n` waits for the
    # next job of ANY kind to change state -- including the parent's earlier
    # `< <(classification_discovered_set ...)` / `< <(classification_bucket
    # pool)` process-substitution subshells above, which bash keeps in the
    # same internal wait-list until explicitly reaped. If one of those were
    # still unreaped here, a bare `wait -n` could consume it instead of a
    # worker, silently letting the pool exceed _H2_POOL_N. Passing
    # "${_h2_pids[@]}" closes that gap.
    #
    # The live count is NOT derived by assuming "one wait -n call reaps
    # exactly one entry, so decrement by 1": _h2_pids accumulates every pid
    # ever spawned and is never pruned, so a throttled run hands `wait -n` a
    # growing mix of live and already-reaped pids. This repo pins no minimum
    # bash version (see :461), and whether `wait -n <ids>` tolerates
    # already-reaped ids in that list and keeps blocking for a live one, vs.
    # returning early the moment it hits an id it no longer recognizes, is
    # exactly the kind of edge case that has differed across bash releases --
    # trusting a fixed decrement would let the live count silently drift
    # under an untested bash, over-spawning past _H2_POOL_N without any
    # signal. Instead `wait -n` is used ONLY as a blocking wake-up; the live
    # set itself is always re-derived from the shell's own job table via
    # `jobs -rp` (running jobs' pids), which reflects bash's SIGCHLD-driven
    # Running/Done bookkeeping directly and needs no id-list tolerance at
    # all -- verified empirically on this host (bash 5.2) across repeated
    # stress runs (N in {1,3,5}, up to 50 members): peak never exceeded N.
    # Worst case on a bash where `wait -n` itself returns early on a stale
    # id, this spins re-checking `jobs -rp` rather than silently
    # over-spawning -- safe, just not maximally efficient.
    _h2_pids=()
    _h2_peak=0

    # Single-writer Phase-1 progress heartbeat (task #5130, PRD
    # run-all-pool-contention-tiering-fix.md §9 L2): exactly ONE background
    # printer, started only when there is at least one pool member, counting
    # completed members via the existing per-member .rc protocol. MUST be
    # disowned immediately -- the L1 throttle re-derives its live-worker set
    # from `jobs -rp` (below) and this phase ends with `wait "${_h2_pids[@]}"`;
    # a non-disowned forever-looping printer would inflate _h2_peak (breaking
    # Test 20's peak<=N) and would be FATALLY joined by that terminal `wait`,
    # hanging the whole suite. `disown` removes it from the shell job table
    # (invisible to both `jobs -rp` and `wait`) while it stays killable via
    # the saved PID (killed right after the Phase-1 join below). Sleeps-FIRST:
    # a pool finishing inside one interval is killed mid-sleep and never
    # prints (see the REIFY_RUN_ALL_PROGRESS_SECS Knobs doc above).
    if [ "${#_h2_pool_members[@]}" -gt 0 ]; then
        _h2_progress_start=$(date +%s)
        (
            set +e +o pipefail
            # 1s-sliced sleep (NOT a single `sleep "$_h2_progress_secs"`): a
            # killed shell does not kill its own in-flight `sleep` CHILD
            # process, which would otherwise linger as an orphan holding
            # this subshell's inherited stdout/stderr open for up to the
            # full interval -- and any caller reading run_all.sh's output
            # through a pipe (command substitution, `| tee`, dark-factory's
            # own subprocess capture) blocks on read-until-EOF until that
            # orphan finally exits. Slicing bounds that post-kill orphan
            # lifetime to ~1s regardless of how large the interval is
            # configured, instead of hanging the caller for the whole
            # interval (e.g. up to 3600s at the ceiling exercised by
            # Test 22's fast-pool fixture).
            _h2_slept=0
            while [ -d "$_H2_WORKDIR" ]; do
                sleep 1
                _h2_slept=$((_h2_slept + 1))
                if [ "$_h2_slept" -ge "$_h2_progress_secs" ]; then
                    _h2_slept=0
                    _done=$(find "$_H2_WORKDIR" -maxdepth 1 -name '*.rc' ! -name '*.retry.rc' 2>/dev/null | wc -l | tr -d ' ')
                    _elapsed=$(( $(date +%s) - _h2_progress_start ))
                    echo "INFO: run_all.sh pool progress: ${_done}/${#_h2_pool_members[@]} complete, elapsed ${_elapsed}s" >&2
                fi
            done
        ) &
        _h2_progress_pid=$!
        disown 2>/dev/null || true
    fi

    for _h2_name in "${_h2_pool_members[@]}"; do
        _h2_i="${_h2_index_of[$_h2_name]}"

        # Fault-injection seam (test-only, esc-3848 class): degrade this ONE
        # member instead of spawning it -- see _h2_degrade_member above.
        if [ -n "${REIFY_RUN_ALL_POOL_FORK_FAIL_MEMBER:-}" ] && [ "$_h2_name" = "$REIFY_RUN_ALL_POOL_FORK_FAIL_MEMBER" ]; then
            _h2_degrade_member "$_h2_name" "$_h2_i"
            continue
        fi

        while [ "${#_h2_pids[@]}" -ge "$_H2_POOL_N" ]; do
            wait -n "${_h2_pids[@]}" 2>/dev/null || true
            _h2_pids=()
            while IFS= read -r _h2_p; do
                [ -n "$_h2_p" ] && _h2_pids+=("$_h2_p")
            done < <(jobs -rp)
        done
        _h2_psi_gate
        (
            _h2_child_rc=0
            _h2_slot_rc=0
            # Soft acquire: on deadline (75) proceed unslotted -- never skip a
            # test, never hang. slot_acquire itself already closes FD 9 on
            # every failed attempt, so no held-slot cleanup is needed here.
            slot_acquire "$_H2_POOL_LOCK" "$_H2_POOL_N" "$_H2_POOL_WAIT" "$_H2_POOL_CLOCK_REASON" || _h2_slot_rc=$?
            bash "$INFRA_DIR/$_h2_name" 9<&- > "$_H2_WORKDIR/${_h2_i}.out" 2>&1 || _h2_child_rc=$?
            if [ "$_h2_slot_rc" -eq 0 ]; then
                exec 9>&-
            fi
            echo "$_h2_child_rc" > "$_H2_WORKDIR/${_h2_i}.rc"
            exit 0
        ) &
        _h2_pids+=($!)
        [ "${#_h2_pids[@]}" -gt "$_h2_peak" ] && _h2_peak="${#_h2_pids[@]}"
    done
    if [ "${#_h2_pids[@]}" -gt 0 ]; then
        wait "${_h2_pids[@]}" 2>/dev/null || true
    fi

    # W4e primary check point (task #5261): _H2_WORKDIR lives under TMPDIR
    # (/tmp), so it survives an INFRA_DIR deletion and the Phase-1 join above
    # always completes even when the worktree was gutted mid-pool -- this is
    # the first point after that join where we can tell. Checking here emits
    # ONE marker instead of letting Phase 2/2.5/3 emit N per-member FAILED
    # lines. See _ra_interrupt_if_worktree_gone above.
    _ra_interrupt_if_worktree_gone

    # Stop the single-writer progress printer now that Phase 1 has joined --
    # disowned above, so it is invisible to `jobs -rp`/`wait` and must be
    # stopped explicitly via its saved PID (task #5130). This is the primary
    # stop path; the _H2_WORKDIR EXIT trap above (:517) is the backstop for a
    # signal-driven exit that never reaches this line.
    [ -n "${_h2_progress_pid:-}" ] && kill "$_h2_progress_pid" 2>/dev/null || true
    if [ "${#_h2_pool_members[@]}" -gt 0 ]; then
        echo "INFO: run_all.sh pool: Phase-1 peak concurrent worker shells=${_h2_peak} (limit ${_H2_POOL_N})" >&2
    fi

    # -- Phase 2: serial (foreground, one at a time, discovered order) -----------
    for _h2_name in "${_h2_serial_members[@]}"; do
        _ra_interrupt_if_worktree_gone
        _h2_i="${_h2_index_of[$_h2_name]}"
        _h2_rc=0
        bash "$INFRA_DIR/$_h2_name" > "$_H2_WORKDIR/${_h2_i}.out" 2>&1 || _h2_rc=$?
        echo "$_h2_rc" > "$_H2_WORKDIR/${_h2_i}.rc"
    done

    # -- Phase 2.5: serial retry-once of failed pool/intra-run-serial members
    # (deflake) --------------------------------------------------------------
    # Re-run each FAILED pool-OR-intra-run-serial member ONCE, serially, in
    # the foreground, AFTER both the concurrent pool (Phase 1) and serial
    # (Phase 2) phases have finished -- the quietest point of the run for
    # host load. Pool members are hermetic by classification (H2/4924), so
    # re-running one is side-effect-free; intra-run-serial members already
    # run serially in Phase 2 (task #5261, W5b), so a same-mode serial
    # re-run is likewise cheap and side-effect-free by classification.
    # host-exclusive members (real burn/cgroup/reflink work, non-hermetic)
    # and any UNCLASSIFIED fail-safe-serial member are deliberately EXCLUDED
    # -- re-running either risks host side effects or has unknown re-run
    # safety, so they keep the single-attempt contract. A member that passes
    # on retry is NOT counted as a failure (flaky_names, not
    # failed_names/failures) -- see Phase 3 below; a member that fails twice
    # keeps the exact existing FAILED contract. Exactly ONE retry per failed
    # eligible member; no slot_acquire/PSI gate (already serial).
    declare -A _h2_retried=()
    declare -A _h2_retry_rc=()
    for _h2_name in "${_h2_discovered_list[@]}"; do
        if [ "${_h2_is_pool[$_h2_name]:-0}" != "1" ] && [ "${_h2_is_intra_serial[$_h2_name]:-0}" != "1" ]; then
            continue
        fi
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
        _ra_interrupt_if_worktree_gone
        _h2_i="${_h2_index_of[$_h2_name]}"
        echo ""
        echo "--- Running: $_h2_name ---"
        if [ "${_h2_retried[$_h2_name]:-0}" = "1" ]; then
            # Retried member: archive BOTH attempts under this SAME header,
            # using attempt-delimiter lines that do NOT match
            # `^--- Running: ` so the discovered-order header-list contract
            # (one header per discovered test) is unaffected. The attempt-1
            # label is conditional (task #5261, W5b): a retried
            # intra-run-serial member's FIRST attempt ran serially in
            # Phase 2, not in the concurrent pool, so "concurrent pool" would
            # be a lie for it -- key the label on _h2_is_pool instead.
            if [ "${_h2_is_pool[$_h2_name]:-0}" = "1" ]; then
                echo "--- attempt 1 (concurrent pool) ---"
            else
                echo "--- attempt 1 (serial) ---"
            fi
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
        # W4e between-members check (task #5261): this loop's iteration list
        # was captured ONCE at glob-expansion time, so a member removed
        # mid-loop by an earlier member just makes `[ -f ]` below silently
        # `continue` past every remaining entry (a silent false success, not
        # even a 127) -- checking here, before that guard, catches it instead.
        _ra_interrupt_if_worktree_gone

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
        _ra_legacy_rc=0
        bash "$test_file" || _ra_legacy_rc=$?
        if [ "$_ra_legacy_rc" -eq 0 ]; then
            echo "  RESULT: PASS ($basename)"
        else
            # W4e on-127 check (task #5261): a "No such file or directory"
            # exit from bash itself is the direct symptom of the worktree
            # having vanished out from under this member's own invocation.
            [ "$_ra_legacy_rc" -eq 127 ] && _ra_interrupt_if_worktree_gone
            echo "  RESULT: FAIL ($basename)"
            failures=$((failures + 1))
            failed_names+=("$basename")
        fi
    done
fi

# W4e final backstop (task #5261): covers a nuke on the very last member,
# which no subsequent loop iteration would otherwise catch (there is no
# "next" top-of-loop check to fire). Common to both the pool and legacy
# paths -- placed once, here, immediately before the Summary block.
_ra_interrupt_if_worktree_gone

echo ""
if [ "${#flaky_names[@]}" -gt 0 ]; then
    echo "=== Summary: $discovered discovered, $failures failed, ${#flaky_names[@]} flaky-retried ==="
elif [ "${#member_subset_skipped[@]}" -gt 0 ]; then
    echo "=== Summary: $discovered discovered, $failures failed, ${#member_subset_skipped[@]} member-subset-skipped ==="
else
    echo "=== Summary: $discovered discovered, $failures failed ==="
fi
if [ "${#failed_names[@]}" -gt 0 ]; then
    echo "=== FAILED: ${failed_names[*]} ==="
    printf 'FAILED %s\n' "${failed_names[*]}"
fi
if [ "${#flaky_names[@]}" -gt 0 ]; then
    echo "=== FLAKY (passed on serial retry): ${flaky_names[*]} ==="
    # FLAKY ledger (task #5142): persist each emission for durable
    # cross-run triage -- observability only, never touches exit code or
    # any stdout line above (_ra_flaky_ledger_append is fail-open).
    for _ra_flaky_name in "${flaky_names[@]}"; do
        _ra_flaky_ledger_append "$_ra_flaky_name"
    done
    # Chronic-offender scan (task #5142): append-all-THEN-scan-all so the
    # current run's just-appended lines are included in the window (a member
    # can cross the threshold on the very run that reports it). Stderr-only,
    # exit code untouched -- see _ra_flaky_chronic_check.
    for _ra_flaky_name in "${flaky_names[@]}"; do
        _ra_flaky_chronic_check "$_ra_flaky_name"
    done
fi

if [ "$failures" -eq 0 ]; then
    exit 0
else
    exit 1
fi
