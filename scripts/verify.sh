#!/usr/bin/env bash
# scripts/verify.sh — unified verification entrypoint for Reify.
#
# Single source of truth shared by BOTH:
#   - orchestrator.yaml  (test_command / lint_command / type_check_command)
#   - hooks/project-checks + hooks/pre-merge-commit  (main-branch git gate)
# so the two can no longer drift apart.
#
# Usage:
#   verify.sh <test|lint|typecheck|all> [options]
#
# Options:
#   --profile debug|release|both   Which build profile(s) to TEST. Default: debug.
#                                  Ignored by lint/typecheck (single pass each).
#                                  'both' runs debug then release test passes.
#                                  When DF_VERIFY_ROLE=merge and no explicit --profile
#                                  is given, defaults to 'both' automatically so the
#                                  orchestrator merge path gets release coverage.
#   --scope   all|staged|branch    all     = verify everything (orchestrator / merges).
#                                  staged  = scope by `git diff --cached` (hook fast path).
#                                  branch  = scope by merge-base(main,HEAD) → working tree;
#                                            tracked changes only (committed, staged, unstaged
#                                            tracked modifications — untracked new files not
#                                            classified). Fails wide to all on error.
#                                  Default: all.
#   --narrow                       With --scope staged: narrow test/check/clippy passes to
#                                  the affected-crate set. No-op for --scope branch (already
#                                  narrowing) and --scope all (always full workspace, C1).
#   --include-infra                Also run the cheap static infra checks
#                                  (sync_comments / run_all on the test side;
#                                  pm-standardization / event-inventory on the lint side).
#   --print-plan                   Dry run: build the exact ordered command list and
#                                  print it (shell-quoted, one command per line, env as
#                                  '# ' comments), then exit 0 WITHOUT running anything.
#                                  This is a faithful oracle of what a real run executes:
#                                  the command list is built once and only the leaf
#                                  step (print vs eval) branches on --print-plan.
#   -h|--help                      Show usage.
#
# Environment baked in (mirrors orchestrator.yaml verify_env + .cargo/run-with-occt.sh):
#   - . ~/.cargo/env
#   - RUSTC_WRAPPER=sccache, CARGO_INCREMENTAL=0  (sccache cache shared across worktrees)
#   - CARGO_MAKEFLAGS=--jobserver-auth=fifo:<role-fifo>  ONLY when the role's FIFO exists
#     (else cargo uses its own per-process job pool). Role→FIFO selection:
#       merge → ${REIFY_JOBSERVER_MERGE_FIFO:-/tmp/reify-jobserver-merge}
#       task  → ${REIFY_JOBSERVER_TASK_FIFO:-/tmp/reify-jobserver-task}
#     Var-names and defaults match scripts/jobserver-balancer.py (α, task 4516).
#     This is a COMPILE-time concurrency control; TEST-execution concurrency is
#     bounded by a separate mechanism (the semaphore wrapper + --test-threads=1 below).
#   - OCCT LD_LIBRARY_PATH (snap + /opt/reify-deps). The .cargo/config.toml `runner`
#     remains the primary runtime-lib mechanism for `cargo test`/`cargo run`; this is
#     belt-and-braces for contexts the runner does not cover.
#
# PSI gate (inter-dispatch throttle for multi-worktree verify bursts):
#   REIFY_PSI_GATE_THRESHOLD    — CPU avg10 % ceiling; dispatch waits until below this
#                                  value. Default: 50.
#   REIFY_PSI_GATE_WINDOW       — minimum inter-dispatch spacing in seconds.  Default: 20.
#   REIFY_PSI_GATE_MAX_WAIT     — give-up timeout (seconds); exits 75 (EX_TEMPFAIL) so
#                                  the orchestrator retries.  Default: 1800.
#   REIFY_PSI_GATE_DISABLE      — set to 1 to bypass entirely (no wait, no dispatch touch).
#                                  Emergency break-glass; does not affect coordination state.
#   REIFY_PSI_GATE_POLL         — recheck interval in seconds.  Default: 5.
#                                  (testability knob; reduce in tests for faster runs)
#   REIFY_PSI_GATE_PROC_PATH    — PSI source; defaults to /proc/pressure/cpu.
#                                  (testability knob; override to inject fixture files)
#   REIFY_PSI_GATE_DISPATCH_FILE— shared coordination timestamp file.
#                                  Default: /tmp/reify-verify-last-dispatch.
#                                  (testability knob; isolate per test case)
#   psi-gate action             — `verify.sh psi-gate` runs only the gate and exits;
#                                  used as the first test-phase plan entry (test/all).
#
# Compile-phase admission gate (task 4618 — soft PSI backpressure for clippy/check):
#   REIFY_COMPILE_GATE_THRESHOLD — CPU avg10 % ceiling for compile admission.
#                                  Default: 85 (well above test gate's 50; a single
#                                  EXEMPT merge holding its reserved core fraction
#                                  does NOT by itself reach 85 — only sustained
#                                  multi-lane oversubscription does).
#                                  Host-portable: PSI avg10 is a kernel-normalized
#                                  stall-%, so no nproc-baked count is introduced.
#   REIFY_COMPILE_GATE_MAX_WAIT  — maximum seconds to wait before ADMITTING anyway
#                                  (fairness floor). Default: 300. On timeout the
#                                  gate returns 0 (admits + warning) — NEVER exit 75.
#                                  This is the fundamental difference from the test
#                                  gate: compile admission is soft backpressure; it
#                                  can delay/stagger a compile start but NEVER requeues.
#   REIFY_COMPILE_GATE_POLL      — recheck interval in seconds. Default: 5.
#                                  (testability knob; reduce in tests for faster runs)
#   REIFY_COMPILE_GATE_PROC_PATH — PSI source; defaults to /proc/pressure/cpu.
#                                  (testability knob; override to inject fixture files)
#   REIFY_COMPILE_GATE_DISABLE   — set to 1 to bypass entirely. Emergency break-glass.
#   compile-gate action          — `verify.sh compile-gate` runs only the compile gate
#                                  and exits; wired into build_plan() before cargo
#                                  check/clippy for lint/typecheck/all (not pure test).
#                                  DF_VERIFY_ROLE=merge → immediate bypass (CAVEAT 1).
#
# Host-relative compile timeout knobs (task 4621):
#   REIFY_VERIFY_TEST_TIMEOUT   — outer timeout for `cargo nextest run` passes.
#                                  Default 60m (workstation budget, η/4521 × 4.5).
#   REIFY_VERIFY_CLIPPY_TIMEOUT — outer timeout for `cargo clippy` and the
#                                  gui-feature `cargo check -p reify-gui` pass.
#                                  Default 45m.
#   REIFY_VERIFY_CHECK_TIMEOUT  — outer timeout for `cargo check --workspace --tests`.
#                                  Default 30m.
#   Values validated as ^[0-9]+[smhd]?$; invalid → default + stderr warning.
#   Unset → identical render on the workstation (no-op). The leo-laptop verify-only
#   host (16t) may widen these via its dispatch env for per-host-measured budgets.
#
# OCCT safety (task 4451):
#   OCCT C++ globals are PER-PROCESS; cross-process isolation is already provided by
#   cargo's per-test-binary process model (nextest). Intra-run concurrency is bounded
#   by the nextest `occt` test-group (max-threads = 4) in .config/nextest.toml; this
#   limits peak OCCT RSS to ≤4×~2GiB ≈ 8GiB, well within the 32GiB host headroom.
#   The OCCT-touching crate set is defined exactly once in scripts/occt-scope-lib.sh
#   and shared with the nextest.toml filter drift check.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Shared OCCT-scope logic (occt_declared_set / occt_touching_set).
if [ ! -f "$SCRIPT_DIR/occt-scope-lib.sh" ]; then
    echo "verify.sh: ERROR — scripts/occt-scope-lib.sh not found next to verify.sh" >&2
    exit 1
fi
# shellcheck source=scripts/occt-scope-lib.sh
source "$SCRIPT_DIR/occt-scope-lib.sh"

# Shared release-sensitivity scope logic (release_declared_set / release_sensitive_set).
if [ ! -f "$SCRIPT_DIR/release-scope-lib.sh" ]; then
    echo "verify.sh: ERROR — scripts/release-scope-lib.sh not found next to verify.sh" >&2
    exit 1
fi
# shellcheck source=scripts/release-scope-lib.sh
source "$SCRIPT_DIR/release-scope-lib.sh"

# Affected-crate reverse-closure (Phase-2 narrowing: maps changed files → workspace crates).
if [ ! -f "$SCRIPT_DIR/affected-crates-lib.sh" ]; then
    echo "verify.sh: ERROR — scripts/affected-crates-lib.sh not found next to verify.sh" >&2
    exit 1
fi
# shellcheck source=scripts/affected-crates-lib.sh
source "$SCRIPT_DIR/affected-crates-lib.sh"

# Test-run counting semaphore (PRD test-run-concurrency-semaphore §3A/§5 D2/D5/D6).
# Holds one slot (FD 9) across ALL test passes via @@SEMAPHORE_ACQUIRE@@/@@SEMAPHORE_RELEASE@@
# sentinels in the PLAN array (see add_test_passes / executor below).
# Bypassed on DF_VERIFY_ROLE=merge or REIFY_TEST_SEMAPHORE_DISABLE=1; knob
# REIFY_TEST_SEMAPHORE_CONCURRENCY controls the slot count (default 1).
if [ ! -f "$SCRIPT_DIR/lib_test_semaphore.sh" ]; then
    echo "verify.sh: ERROR — scripts/lib_test_semaphore.sh not found next to verify.sh" >&2
    exit 1
fi
# shellcheck source=scripts/lib_test_semaphore.sh
source "$SCRIPT_DIR/lib_test_semaphore.sh"

# Shared PSI-admission core (psi_gate / compile_gate thin wrappers; agent shim β).
if [ ! -f "$SCRIPT_DIR/cpu-admit.sh" ]; then
    echo "verify.sh: ERROR — scripts/cpu-admit.sh not found next to verify.sh" >&2
    exit 1
fi
# shellcheck source=scripts/cpu-admit.sh
source "$SCRIPT_DIR/cpu-admit.sh"

# ---------------------------------------------------------------------------
# Host-relative compile timeout resolver (task 4621)
# ---------------------------------------------------------------------------

# _resolve_timeout_knob <env_var_name> <default>
# Validates that the env var value matches ^[0-9]+[smhd]?$ (digits + optional
# single unit suffix: s/m/h/d).  Returns the env value verbatim if valid, else
# returns the default and emits a stderr warning (non-empty invalid only).
# Mirrors the strict-digit idiom in gen-nextest-config.sh / parse_debug_port;
# adapted to duration values with an optional unit suffix.
_resolve_timeout_knob() {
    local _name="$1" _default="$2"
    local _val="${!_name:-}"
    # Strip exactly one trailing unit character (if present) to isolate the
    # digit part.  After stripping, the remainder must be purely digits and
    # non-empty to be a valid duration.
    local _core
    case "$_val" in
        (*[smhd]) _core="${_val%[smhd]}" ;;  # strip one trailing unit
        (*)       _core="$_val" ;;
    esac
    case "$_core" in
        (''|*[!0-9]*)
            [ -n "$_val" ] && printf 'verify.sh: WARNING: invalid %s=%s; using default %s\n' \
                "$_name" "$_val" "$_default" >&2
            printf '%s' "$_default"
            ;;
        (*) printf '%s' "$_val" ;;
    esac
}

# Resolve three compile-budget tiers once at startup.  Defaults match the
# workstation-measured budgets (unset → identical render, no-op on workstation).
_VERIFY_TEST_TIMEOUT="$(_resolve_timeout_knob REIFY_VERIFY_TEST_TIMEOUT 60m)"
_VERIFY_CLIPPY_TIMEOUT="$(_resolve_timeout_knob REIFY_VERIFY_CLIPPY_TIMEOUT 45m)"
_VERIFY_CHECK_TIMEOUT="$(_resolve_timeout_knob REIFY_VERIFY_CHECK_TIMEOUT 30m)"

usage() {
    sed -n '2,51p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

# ---------------------------------------------------------------------------
# PSI gate — throttle per-task test phases under multi-worktree verify bursts
# ---------------------------------------------------------------------------

# psi_gate() — thin wrapper over cpu_admit requeue (scripts/cpu-admit.sh).
# Called directly via `verify.sh psi-gate` (testable entry point) and wired
# as the first test-phase plan entry by add_test_passes().
#
# Environment knobs (see header comment block for full doc):
#   REIFY_PSI_GATE_THRESHOLD    — avg10 ceiling to allow dispatch (default 50)
#   REIFY_PSI_GATE_WINDOW       — min seconds between dispatches (default 20)
#   REIFY_PSI_GATE_MAX_WAIT     — give-up timeout in seconds (default 1800)
#   REIFY_PSI_GATE_POLL         — recheck interval in seconds (default 5)
#   REIFY_PSI_GATE_PROC_PATH    — PSI source path (default /proc/pressure/cpu)
#   REIFY_PSI_GATE_DISPATCH_FILE— coordination timestamp file
#   REIFY_PSI_GATE_DISABLE      — set to 1 to bypass entirely (no touch)
psi_gate() {
    # DF_VERIFY_ROLE=merge bypass (and all other admission logic) is enforced
    # in cpu_admit; this wrapper just maps REIFY_PSI_GATE_* → _ca_* and delegates.
    local _ca_threshold="${REIFY_PSI_GATE_THRESHOLD:-50}"
    local _ca_window="${REIFY_PSI_GATE_WINDOW:-20}"
    local _ca_max_wait="${REIFY_PSI_GATE_MAX_WAIT:-1800}"
    local _ca_poll="${REIFY_PSI_GATE_POLL:-5}"
    local _ca_proc_path="${REIFY_PSI_GATE_PROC_PATH:-/proc/pressure/cpu}"
    local _ca_dispatch="${REIFY_PSI_GATE_DISPATCH_FILE:-/tmp/reify-verify-last-dispatch}"
    local _ca_disable="${REIFY_PSI_GATE_DISABLE:-}"
    local _ca_log_prefix="verify.sh"
    local _ca_gate_name="PSI gate"
    local _ca_failopen_txt="PSI gate disabled"
    cpu_admit requeue
}

# compile_gate() — thin wrapper over cpu_admit admit (scripts/cpu-admit.sh).
# Called directly via `verify.sh compile-gate` (testable entry point) and wired
# as a plan entry in build_plan() before cargo check/clippy (lint/typecheck/all).
#
# Key differences from psi_gate() (preserved via cpu_admit admit mode):
#   - Higher default threshold (85 vs 50): treats a lone exempt merge's core
#     reservation as expected-high-pressure baseline — only sustained multi-lane
#     oversubscription trips it.
#   - Admit-on-timeout (cpu_admit admit returns 0 + warning) — NEVER exit 75.
#     Compile admission is soft backpressure; it can delay/stagger a compile start
#     but can NEVER requeue a task (storm-proof, CAVEAT 2).
#   - No WINDOW/dispatch-file/flock: compiles run concurrently under the jobserver.
#
# Environment knobs (see header comment block for full doc):
#   REIFY_COMPILE_GATE_THRESHOLD  — avg10 ceiling (default 85)
#   REIFY_COMPILE_GATE_MAX_WAIT   — admit-on-timeout seconds (default 300)
#   REIFY_COMPILE_GATE_POLL       — recheck interval in seconds (default 5)
#   REIFY_COMPILE_GATE_PROC_PATH  — PSI source path (default /proc/pressure/cpu)
#   REIFY_COMPILE_GATE_DISABLE    — set to 1 to bypass entirely
compile_gate() {
    # DF_VERIFY_ROLE=merge bypass (CAVEAT 1) and all other admission logic is
    # enforced in cpu_admit; this wrapper maps REIFY_COMPILE_GATE_* → _ca_* and
    # delegates.  No _ca_window / _ca_dispatch: compiles run concurrently under
    # the jobserver (serializing would recreate the throttling it already owns).
    local _ca_threshold="${REIFY_COMPILE_GATE_THRESHOLD:-85}"
    local _ca_max_wait="${REIFY_COMPILE_GATE_MAX_WAIT:-300}"
    local _ca_poll="${REIFY_COMPILE_GATE_POLL:-5}"
    local _ca_proc_path="${REIFY_COMPILE_GATE_PROC_PATH:-/proc/pressure/cpu}"
    local _ca_disable="${REIFY_COMPILE_GATE_DISABLE:-}"
    local _ca_window=""
    local _ca_dispatch=""
    local _ca_log_prefix="verify.sh"
    local _ca_gate_name="compile-gate"
    local _ca_failopen_txt="compile-gate fail-open"
    cpu_admit admit
}

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------
ACTION=""
PROFILE="debug"
PROFILE_EXPLICIT=0   # set to 1 if --profile was given explicitly; keeps explicit authoritative
SCOPE="all"
NARROW=0             # --narrow: opt-in to affected-crate narrowing for --scope staged
INCLUDE_INFRA=0
PRINT_PLAN=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        test|lint|typecheck|all|psi-gate|compile-gate)
            if [ -n "$ACTION" ]; then
                echo "verify.sh: ERROR — action already set to '$ACTION', got '$1'" >&2
                exit 64
            fi
            ACTION="$1"; shift ;;
        --profile)
            PROFILE="${2:?--profile requires an argument}"; PROFILE_EXPLICIT=1; shift 2 ;;
        --profile=*)
            PROFILE="${1#*=}"; PROFILE_EXPLICIT=1; shift ;;
        --scope)
            SCOPE="${2:?--scope requires an argument}"; shift 2 ;;
        --scope=*)
            SCOPE="${1#*=}"; shift ;;
        --narrow)
            NARROW=1; shift ;;
        --include-infra)
            INCLUDE_INFRA=1; shift ;;
        --print-plan)
            PRINT_PLAN=1; shift ;;
        -h|--help)
            usage; exit 0 ;;
        *)
            echo "verify.sh: ERROR — unknown argument '$1'" >&2
            usage >&2
            exit 64 ;;
    esac
done

if [ -z "$ACTION" ]; then
    echo "verify.sh: ERROR — missing action (test|lint|typecheck|all)" >&2
    usage >&2
    exit 64
fi
case "$PROFILE" in debug|release|both) ;; *)
    echo "verify.sh: ERROR — invalid --profile '$PROFILE' (want debug|release|both)" >&2; exit 64 ;;
esac
case "$SCOPE" in all|staged|branch) ;; *)
    echo "verify.sh: ERROR — invalid --scope '$SCOPE' (want all|staged|branch)" >&2; exit 64 ;;
esac
DF_VERIFY_ROLE="${DF_VERIFY_ROLE:-task}"
# Role-based PROFILE default: when no explicit --profile was given and the
# orchestrator merge path stamps DF_VERIFY_ROLE=merge, default to 'both' so
# release-only tests are exercised on every merge (matching the local
# hooks/pre-merge-commit gate which also runs --profile both).
# Explicit --profile always wins; task/unset roles keep debug (fast feedback).
if [ "$PROFILE_EXPLICIT" -eq 0 ] && [ "$DF_VERIFY_ROLE" = "merge" ]; then
    PROFILE="both"
fi
# Probe scheduling-tool availability once; degrade gracefully on non-Linux hosts
# where util-linux may not be installed.
_HAS_NICE=0; _HAS_IONICE=0
command -v nice   >/dev/null 2>&1 && _HAS_NICE=1
command -v ionice >/dev/null 2>&1 && _HAS_IONICE=1
case "$DF_VERIFY_ROLE" in
    task)
        if   [ "$_HAS_NICE" -eq 1 ] && [ "$_HAS_IONICE" -eq 1 ]; then
            CARGO_PRIO="nice -n 15 ionice -c 2 -n 7 "
        elif [ "$_HAS_NICE" -eq 1 ]; then
            echo "verify.sh: WARNING — ionice not found; task role using nice only (no IO throttle)" >&2
            CARGO_PRIO="nice -n 15 "
        else
            echo "verify.sh: WARNING — nice/ionice not found; task role running at normal priority" >&2
            CARGO_PRIO=""
        fi ;;
    merge)
        if [ "$_HAS_NICE" -eq 1 ]; then
            CARGO_PRIO="nice -n 5 "
        else
            echo "verify.sh: WARNING — nice not found; merge role running at normal priority" >&2
            CARGO_PRIO=""
        fi ;;
    *)  echo "verify.sh: ERROR — unknown DF_VERIFY_ROLE '$DF_VERIFY_ROLE' (want task|merge)" >&2; exit 64 ;;
esac

# psi-gate is dispatched EARLY — before MERGE_HEAD check / cd / apply_env —
# so the integration test can drive it without triggering the cargo pipeline.
# Note: psi-gate is execute-only; --print-plan is intentionally ignored here.
# The parent test/all invocation prints the psi-gate command as a normal plan
# line; the psi-gate subprocess itself always executes the gate regardless of
# how it was invoked.
if [ "$ACTION" = "psi-gate" ]; then
    psi_gate
    exit $?
fi

# compile-gate is dispatched EARLY — same idiom as psi-gate: execute-only,
# hermetic, testable in isolation without triggering the cargo pipeline.
# DF_VERIFY_ROLE is already resolved above so the merge bypass works correctly.
if [ "$ACTION" = "compile-gate" ]; then
    compile_gate
    exit $?
fi

# A merge in progress cannot trust `git diff --cached` (the index reflects the
# merge result, not a curated stage), so force a full verification. Detected via
# the git-dir-relative MERGE_HEAD so it works correctly inside linked worktrees.
_MERGE_HEAD="$(git -C "$REPO_ROOT" rev-parse --git-path MERGE_HEAD 2>/dev/null || echo '')"
if [ -n "$_MERGE_HEAD" ] && [ -f "$_MERGE_HEAD" ] && [ "$SCOPE" != "all" ]; then
    echo "verify.sh: MERGE_HEAD present — forcing --scope all (merge in progress)" >&2
    SCOPE="all"
fi

# Defensive belt-and-braces (contract C2): the merge gate never narrows. The
# dark-factory orchestrator's post-merge verify stamps DF_VERIFY_ROLE=merge;
# force --scope all so a future caller cannot hand the merge gate a narrowing
# scope (branch/staged). Independent of the role-driven --profile default above
# and of the affected-crate machinery. Mirrors the MERGE_HEAD force.
if [ "$DF_VERIFY_ROLE" = "merge" ] && [ "$SCOPE" != "all" ]; then
    echo "verify.sh: DF_VERIFY_ROLE=merge — forcing --scope all (merge gate never narrows, contract C2)" >&2
    SCOPE="all"
fi

# Run all relative-path commands from the repo root, matching how both the
# orchestrator (project_root) and the git hook ($ROOT) invoke verification.
cd "$REPO_ROOT"

# --scope branch: resolve merge-base(main, HEAD) -> working tree diff.
# Fail WIDE (contract C5): detached HEAD / missing local 'main' ref / any
# git failure forces SCOPE=all (full plan) — under-verify ships breakage,
# over-verify just wastes CPU. Assignment inside `if` test keeps set -e clean.
_MERGE_BASE=""
if [ "$SCOPE" = "branch" ]; then
    if _MERGE_BASE="$(git -C "$REPO_ROOT" merge-base main HEAD 2>/dev/null)" && [ -n "$_MERGE_BASE" ]; then
        :
    else
        echo "verify.sh: WARNING — --scope branch could not resolve 'git merge-base main HEAD' (detached HEAD / missing local main ref / merge-base failure) — failing WIDE to --scope all (contract C5)" >&2
        SCOPE="all"
    fi
fi

# Action → which check families run.
case "$ACTION" in
    test)      DO_TEST=1; DO_LINT=0; DO_TYPECHECK=0 ;;
    lint)      DO_TEST=0; DO_LINT=1; DO_TYPECHECK=0 ;;
    typecheck) DO_TEST=0; DO_LINT=0; DO_TYPECHECK=1 ;;
    all)       DO_TEST=1; DO_LINT=1; DO_TYPECHECK=1 ;;
esac

# Profiles to TEST.
case "$PROFILE" in
    debug)   PROFILES=(debug) ;;
    release) PROFILES=(release) ;;
    both)    PROFILES=(debug release) ;;
esac

# ---------------------------------------------------------------------------
# Environment (process-level; inherited by every command in the plan)
# ---------------------------------------------------------------------------
ENV_LINES=()
apply_env() {
    if [ -f "$HOME/.cargo/env" ]; then
        # shellcheck disable=SC1091
        . "$HOME/.cargo/env"
        ENV_LINES+=(". $HOME/.cargo/env")
    else
        ENV_LINES+=("# ~/.cargo/env not found — relying on ambient PATH for cargo")
    fi

    export RUSTC_WRAPPER=sccache
    ENV_LINES+=("export RUSTC_WRAPPER=sccache")
    export CARGO_INCREMENTAL=0
    ENV_LINES+=("export CARGO_INCREMENTAL=0")

    # Inherit the shared global jobserver ONLY when the role's FIFO exists; otherwise
    # leave CARGO_MAKEFLAGS unset so cargo manages its own job pool. Exporting a
    # stale fifo path when reify-jobserver.service is down would wedge cargo.
    # Role→FIFO selection: merge → REIFY_JOBSERVER_MERGE_FIFO (default /tmp/reify-jobserver-merge)
    #                       task  → REIFY_JOBSERVER_TASK_FIFO  (default /tmp/reify-jobserver-task)
    # Defaults/var-names match scripts/jobserver-balancer.py (α, task 4516).
    local _jb_fifo
    if [ "$DF_VERIFY_ROLE" = "merge" ]; then
        _jb_fifo="${REIFY_JOBSERVER_MERGE_FIFO:-/tmp/reify-jobserver-merge}"
    else
        _jb_fifo="${REIFY_JOBSERVER_TASK_FIFO:-/tmp/reify-jobserver-task}"
    fi
    if [ -p "$_jb_fifo" ]; then
        export CARGO_MAKEFLAGS="--jobserver-auth=fifo:$_jb_fifo"
        ENV_LINES+=("export CARGO_MAKEFLAGS=--jobserver-auth=fifo:$_jb_fifo")
    else
        ENV_LINES+=("# CARGO_MAKEFLAGS left unset (no $_jb_fifo FIFO) — cargo uses its own job pool")
    fi

    # OCCT shared-library search path (mirrors .cargo/run-with-occt.sh).
    local snap_lib="/snap/freecad/current/usr/lib"
    if [ -d "$snap_lib" ]; then
        export LD_LIBRARY_PATH="$snap_lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    fi
    local deps_lib="/opt/reify-deps/lib"
    if [ -d "$deps_lib" ] && ls "$deps_lib"/libTKernel.so* >/dev/null 2>&1; then
        export LD_LIBRARY_PATH="$deps_lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    fi
    ENV_LINES+=("export LD_LIBRARY_PATH=${LD_LIBRARY_PATH:-}")
}
apply_env

# ---------------------------------------------------------------------------
# Scope decision: RUN_RUST / RUN_GUI / RUN_OCCT_GATE
# ---------------------------------------------------------------------------
RUN_RUST=0
RUN_GUI=0
# RUN_OCCT_GATE: diagnostic-only after task 4451 folded OCCT into the nextest pool.
# Still computed (gate=1 when OCCT-touching files change) and printed in the
# --print-plan header for observability; it no longer gates any test emission.
RUN_OCCT_GATE=0
CHANGED_FILES_RAW=""   # post-.task/ filtered file list; set by decide_scope for branch/staged

# is_occt_crate <crate-name> — true iff the crate is in the declared OCCT set.
_OCCT_DECLARED="$(occt_declared_set)"
is_occt_crate() {
    grep -qxF "$1" <<<"$_OCCT_DECLARED"
}

decide_scope() {
    if [ "$SCOPE" = "all" ]; then
        RUN_RUST=1; RUN_GUI=1; RUN_OCCT_GATE=1
        return
    fi

    # Classify the changed files for staged/branch scope, ignoring the agent
    # scratch dir (.task/). Source depends on scope:
    #   staged: git diff --cached (added/copied/modified/renamed index entries)
    #   branch: git diff "$_MERGE_BASE" (working tree vs merge-base(main,HEAD);
    #           tracked changes only — committed, staged, unstaged tracked
    #           modifications; untracked new files are not included)
    # Map each path to its impact:
    #   rust+gui+gate   workspace-global or OCCT-touching crate change
    #   rust+gui        a non-OCCT Rust crate / Tauri crate change (Rust ⊇ GUI)
    #   gui             frontend-only TS change (Rust ⊥ GUI)
    #   ignore          docs / markdown / yaml config
    #   conservative    anything unrecognised -> treat as rust+gui+gate
    local rust=0 gui=0 gate=0 f crate
    # Determine the changed-file list up front. For branch scope, check git diff's
    # exit status explicitly: if it fails after merge-base resolution (e.g. corrupt
    # object), fail WIDE rather than silently classifying nothing (contract C5).
    # The staged path keeps || true to absorb grep's harmless "no matches" exit-1.
    local _files="" _diff_out=""
    if [ "$SCOPE" = "branch" ]; then
        if ! _diff_out="$(git -C "$REPO_ROOT" diff --name-only --diff-filter=ACMR "$_MERGE_BASE")"; then
            echo "verify.sh: WARNING — --scope branch git diff failed — failing WIDE to --scope all (contract C5)" >&2
            RUN_RUST=1; RUN_GUI=1; RUN_OCCT_GATE=1
            return
        fi
        _files="$(grep -v '^\.task/' <<< "$_diff_out" || true)"
    else
        _files="$(git -C "$REPO_ROOT" diff --cached --name-only --diff-filter=ACMR | grep -v '^\.task/' || true)"
    fi
    while IFS= read -r f; do
        [ -z "$f" ] && continue
        case "$f" in
            crates/*)
                rust=1; gui=1
                crate="${f#crates/}"; crate="${crate%%/*}"
                if is_occt_crate "$crate"; then gate=1; fi
                ;;
            gui/src-tauri/*)
                # The Tauri Rust crate (reify-gui) is OCCT-clean by default features.
                rust=1; gui=1
                ;;
            Cargo.toml|Cargo.lock|.cargo/*)
                # Workspace-global: can affect any crate, including OCCT ones.
                rust=1; gui=1; gate=1
                ;;
            tree-sitter-reify/*)
                # Grammar drives the generated parser consumed by reify-eval (OCCT).
                rust=1; gui=1; gate=1
                ;;
            gui/*)
                # Any other GUI path (frontend src, sidecar, configs) — GUI only.
                gui=1
                ;;
            docs/*|*.md|*.yaml|*.yml)
                : # no heavy checks
                ;;
            *)
                # Unrecognised path: be conservative.
                rust=1; gui=1; gate=1
                ;;
        esac
    done <<< "$_files"

    # Capture for Phase-2 narrowing (after .task/ filter). scope=all returns early
    # above, leaving CHANGED_FILES_RAW="" (never narrowing-eligible).
    CHANGED_FILES_RAW="$_files"

    RUN_RUST=$rust
    # Any Rust change implies the (fast) GUI checks too.
    RUN_GUI=$(( rust | gui ))
    RUN_OCCT_GATE=$gate
}
decide_scope

# ---------------------------------------------------------------------------
# Selective infra test injection (task 4523).
#
# After decide_scope, read verify-pipeline-infra-tests.txt to derive
# SELECTED_INFRA_GLOBS: the set of infra-test globs whose artifact was changed
# on this branch/staged diff.  Empty under scope=all (CHANGED_FILES_RAW="").
#
# Design notes (see task 4523 decisions):
#   • Map is read inline (NOT via a sourced lib) so the throughput/gui_feature
#     auto-discovery greps don't flag it.  _VP_INFRA_MAP uses a variable
#     assignment; no 'source' directive for this map — fixture-check greps skip it.
#   • [ -f ] guard degrades gracefully in fixtures that omit the map.
#   • GLOB (not explicit names) so future test_verify_*.sh guards are
#     auto-covered without a map edit.
# ---------------------------------------------------------------------------
SELECTED_INFRA_GLOBS=""

select_infra_tests() {
    local _VP_INFRA_MAP="$SCRIPT_DIR/verify-pipeline-infra-tests.txt"
    # Graceful degradation: absent map or empty changed-file list -> empty.
    [ -f "$_VP_INFRA_MAP" ] || return 0
    [ -n "$CHANGED_FILES_RAW" ] || return 0
    local _artifact _glob _f _line
    while IFS= read -r _line; do
        # Each row: <artifact-path>  <infra-test-glob>
        read -r _artifact _glob <<< "$_line"
        [ -n "$_artifact" ] || continue
        [ -n "$_glob"     ] || continue
        while IFS= read -r _f; do
            [ -z "$_f" ] && continue
            if [ "$_f" = "$_artifact" ]; then
                # Append glob to selection if not already present (whole-token
                # dedup via space sentinels — prevents false dedup when one
                # glob is a substring of another, e.g. a specific path vs a
                # broader wildcard pattern).
                case " $SELECTED_INFRA_GLOBS " in
                    *" $_glob "*) : ;;
                    *) SELECTED_INFRA_GLOBS="${SELECTED_INFRA_GLOBS:+$SELECTED_INFRA_GLOBS }$_glob" ;;
                esac
                break
            fi
        done <<< "$CHANGED_FILES_RAW"
    done < <(grep -v '^\s*#' "$_VP_INFRA_MAP" | grep -v '^\s*$')
}
select_infra_tests

# ---------------------------------------------------------------------------
# Phase-2 narrowing: map changed files → affected crate set → -p flag strings.
#
# Eligible when: (scope=branch OR (scope=staged AND --narrow)) AND RUN_RUST=1.
# scope=all is structurally unreachable for narrowing (C1 — returns early in
# decide_scope, leaving CHANGED_FILES_RAW="", and the condition is never true).
# --narrow is a no-op for scope=branch (already narrowing) and scope=all
# (condition never true).
#
# REIFY_AFFECTED_CRATES_OVERRIDE — testability/operator knob (whitespace/newline-
# separated crate names). When set AND narrowing is eligible, used verbatim in
# place of calling affected_crates(). This mirrors the REIFY_PSI_GATE_PROC_PATH
# knob idiom and allows hermetic --print-plan assertions in the workspace-less
# fixture (where cargo metadata fails and affected_crates() always returns ALL).
# ---------------------------------------------------------------------------
AFFECTED=""
NARROW_ACTIVE=0
AFFECTED_ALL_FLAGS=""

_narrowing_eligible=0
if [ "$SCOPE" = "branch" ] && [ "$RUN_RUST" -eq 1 ]; then
    _narrowing_eligible=1
elif [ "$SCOPE" = "staged" ] && [ "$NARROW" -eq 1 ] && [ "$RUN_RUST" -eq 1 ]; then
    _narrowing_eligible=1
fi

if [ "$_narrowing_eligible" -eq 1 ]; then
    if [ -n "${REIFY_AFFECTED_CRATES_OVERRIDE:-}" ]; then
        # Operator/testability override: use verbatim crate list.
        AFFECTED="${REIFY_AFFECTED_CRATES_OVERRIDE}"
    elif [ -n "$CHANGED_FILES_RAW" ]; then
        # Real run: compute reverse-closure from the captured changed-file list.
        _af_args=()
        while IFS= read -r _af_f; do
            [ -n "$_af_f" ] && _af_args+=("$_af_f")
        done <<< "$CHANGED_FILES_RAW"
        if [ "${#_af_args[@]}" -gt 0 ]; then
            AFFECTED="$(affected_crates "${_af_args[@]}")"
        fi
    fi
    # NARROW_ACTIVE iff AFFECTED is non-empty and is NOT the sentinel "ALL".
    if [ -n "$AFFECTED" ] && [ "$AFFECTED" != "ALL" ]; then
        NARROW_ACTIVE=1
    fi
fi

if [ "$NARROW_ACTIVE" -eq 1 ]; then
    # Build the affected-crate -p flag string. Task 4451: no gated/ungated split;
    # all affected crates (including OCCT ones) go through the single nextest pass,
    # with the occt test-group (max-threads=24, env-driven) bounding their concurrency.
    # Word-split $AFFECTED (safe: Rust crate names never contain spaces).
    # shellcheck disable=SC2086
    for _nc in $AFFECTED; do
        [ -z "$_nc" ] && continue
        AFFECTED_ALL_FLAGS+=" -p $_nc"
    done
    AFFECTED_ALL_FLAGS="${AFFECTED_ALL_FLAGS# }"
    # Guard: a whitespace-only REIFY_AFFECTED_CRATES_OVERRIDE passes the non-empty check
    # above but word-splits to nothing, leaving all flag vars empty. Empty AFFECTED_ALL_FLAGS
    # with NARROW_ACTIVE=1 would cause narrowed cargo check/clippy to run with no -p selector
    # and narrowed test passes to emit zero commands (silent coverage gap). Fall back to
    # full-workspace to preserve the fail-wide invariant for a malformed knob value.
    if [ -z "$AFFECTED_ALL_FLAGS" ]; then
        NARROW_ACTIVE=0
    fi
fi

# ---------------------------------------------------------------------------
# Plan construction (built ONCE; print vs execute branches only at the leaves)
# ---------------------------------------------------------------------------
PLAN=()
add() { PLAN+=("$1"); }

# Release-sensitive crate flags: ALL release-sensitive crates in one nextest -p set.
# Task 4451: the gated/ungated split is gone; the nextest occt group (max-threads=24,
# env-driven) bounds intra-run concurrency for OCCT-touching release-sensitive crates (reify-eval).
# reify-kernel-occt, reify-cli, reify-config have zero release-sensitive tests and
# correctly drop out of the release pass; the debug full-workspace pass covers them.
_RELEASE_DECLARED="$(release_declared_set)"
_RELEASE_ALL_FLAGS=""
while IFS= read -r _rc; do
    [ -z "$_rc" ] && continue
    _RELEASE_ALL_FLAGS+=" -p $_rc"
done <<<"$_RELEASE_DECLARED"
_RELEASE_ALL_FLAGS="${_RELEASE_ALL_FLAGS# }"

# Test runner: prefer cargo-nextest (one global pool over ~hundreds of test
# binaries, OCCT concurrency bounded by the occt test-group) with a graceful
# fallback to plain `cargo test -- --test-threads=1` when nextest is not installed.
NEXTEST=0
if cargo nextest --version >/dev/null 2>&1; then
    NEXTEST=1
fi

# wrap_subshell <dir> <minutes> <inner> — "(cd DIR && timeout … INNER)", using
# `bash -c '…'` only when INNER is a compound (&&) so the timeout governs it.
wrap_subshell() {
    local dir="$1" mins="$2" inner="$3"
    case "$inner" in
        *"&&"*)
            printf '(cd %s && timeout --kill-after=60 %sm bash -c '\''%s'\'')' "$dir" "$mins" "$inner" ;;
        *)
            printf '(cd %s && timeout --kill-after=60 %sm %s)' "$dir" "$mins" "$inner" ;;
    esac
}

# Memoized temp nextest config path (populated on first NEXTEST=1 execute-mode pass in
# emit_nextest_pass).  scripts/gen-nextest-config.sh writes a full copy of
# .config/nextest.toml with the occt literal rewritten to the REIFY_OCCT_NEXTEST_MAX_THREADS
# value (default 24).  nextest --config overrides CARGO config only (NO-OP for test-groups
# on 0.9.136); --config-file is required to actually override the occt group max-threads.
# In --print-plan mode the variable stays empty (no subprocess, no temp file — print mode
# is a hermetic, side-effect-free oracle; execute mode generates the real file).
_NEXTEST_CONFIG_FILE=""

_verify_cleanup() {
    if [ -n "$_NEXTEST_CONFIG_FILE" ] && [ -f "$_NEXTEST_CONFIG_FILE" ]; then
        rm -f "$_NEXTEST_CONFIG_FILE"
    fi
}
trap '_verify_cleanup' EXIT

# emit_nextest_pass <selector> <rel> <outer_timeout>
# Emit a single nextest (or cargo-test fallback) pass.
# selector: "--workspace" (full-workspace) or "-p crate1 -p crate2 ..." (narrowed/release)
# rel: "" (debug) or " --release"
# outer_timeout: e.g. "60m" or "75m"
# Task 4451: replaces emit_gated_ungated; the flock-gated OCCT pass is dropped.
# Task 4503/γ: env-driven occt cap via REIFY_OCCT_NEXTEST_MAX_THREADS (default 24).
# scripts/gen-nextest-config.sh generates a temp nextest config (memoized in
# _NEXTEST_CONFIG_FILE) passed as --config-file; nextest --config overrides CARGO
# config only (NO-OP for test-groups on 0.9.136) so --config-file is required.
# In --print-plan mode a static placeholder path is emitted instead of a real temp
# path so --print-plan remains a pure, hermetic oracle (no subprocess, no temp file).
emit_nextest_pass() {
    local selector="$1" rel="$2" outer_timeout="$3"
    local cmd
    if [ "$NEXTEST" -eq 1 ]; then
        local _cfg_path
        if [ "$PRINT_PLAN" -eq 1 ]; then
            # Print mode: emit a representative placeholder so --print-plan is a
            # pure, hermetic oracle — no subprocess, no temp file created.
            # The placeholder preserves the 'reify-nextest-occt' prefix so plan-shape
            # assertions (tests/infra/test_occt_gated_scope.sh Test 9) can still
            # match the pattern without requiring a real file on disk.
            # This path is intentionally NOT re-runnable; only execute mode produces
            # a real config file (memoized in _NEXTEST_CONFIG_FILE).
            _cfg_path="${TMPDIR:-/tmp}/reify-nextest-occt.<print-plan-placeholder>"
        else
            # Execute mode: generate the nextest config once per process (memoized).
            # Produces a full copy of .config/nextest.toml with the occt cap rewritten
            # to the resolved env value; removed by _verify_cleanup on EXIT.
            if [ -z "$_NEXTEST_CONFIG_FILE" ]; then
                _NEXTEST_CONFIG_FILE="$("$SCRIPT_DIR/gen-nextest-config.sh")"
            fi
            _cfg_path="$_NEXTEST_CONFIG_FILE"
        fi
        cmd="timeout --kill-after=60 ${outer_timeout} ${CARGO_PRIO}cargo nextest run ${selector}${rel} --config-file ${_cfg_path}"
    else
        # Fallback: single-threaded (OCCT serialization via the nextest occt group is
        # unavailable without nextest; use --test-threads=1 as the whole-workspace guard).
        cmd="timeout --kill-after=60 ${outer_timeout} ${CARGO_PRIO}cargo test ${selector}${rel} -- --test-threads=1"
    fi
    # FD 9 is the held semaphore slot; close it for each gated child so daemon
    # processes (sccache/rustc) cannot inadvertently inherit the lock fd and
    # wedge the slot after the test pass exits (2026-04-20 wedge class).
    # Harmless no-op when the slot was not acquired (merge-exempt or disabled).
    add "$cmd 9<&-"
}

add_test_passes() {
    # PSI gate: must pass before any cargo test work starts.
    # In execute mode: eval runs this as a subprocess that inherits DF_VERIFY_ROLE
    # and REIFY_PSI_GATE_*; exit 75 (EX_TEMPFAIL) propagates → orchestrator retries.
    # In --print-plan mode: printed faithfully as a normal plan line.
    add "./scripts/verify.sh psi-gate"

    # Acquire the test-run semaphore slot AFTER psi-gate so the scarce held
    # slot is not occupied during a PSI pressure wait (PRD §5 D2).
    # The executor calls test_semaphore_acquire here; the printer emits a comment.
    add "@@SEMAPHORE_ACQUIRE@@"

    local profile rel outer_timeout
    # Outer timeout: single unified budget re-derived from η/4521's authoritative
    # real-load floor (task 4520/ζ′).
    # Floor: 798.9 s (worst-observed cold real-load, genuinely cold-cache, quiet box
    # with warm host sccache — see docs/prds/jobserver-merge-priority-balancer
    # .acceptance-report.md §"ζ′/4520 budget floor (authoritative)").
    # Derivation: ceil(798.9 × 4.5 production-weighted margin) = ceil(3595.05 s) =
    # 3596 s → rounded up to clean minute-granularity = 60m (3600 s).
    # Bound 3600 s > floor 798.9 s by construction. The 4.5× margin weights ambient
    # production contention on top of the quiet-box measurement (the η report endorses
    # the standing ≈4.5× headroom as appropriate). The debug --workspace pass (all
    # crates) is the HEAVIER compile and already clears 60m battle-tested (task 4453,
    # zero exit-124 under η's real-load gate); the lighter release-sensitive-subset
    # pass clears 60m a fortiori — the prior 75m release budget was load-inconsistent
    # band-aid lineage (esc-4178/esc-4180/#4447/#4453).
    # NOTE: both outer timeouts are asserted in tests/infra/test_occt_flock_gate.sh
    # (Test 17 — debug pass, Test 17b — release pass) — keep them in sync.
    for profile in "${PROFILES[@]}"; do
        if [ "$profile" = "release" ]; then
            rel=" --release"; outer_timeout="${_VERIFY_TEST_TIMEOUT}"
        else
            rel=""; outer_timeout="${_VERIFY_TEST_TIMEOUT}"
        fi

        if [ "$profile" = "release" ]; then
            # Release pass: ALL release-sensitive crates in one nextest pass (task 4451).
            # The nextest occt group (max-threads=24, env-driven) bounds concurrency for OCCT-touching
            # release-sensitive crates (e.g. reify-eval). Only crates with
            # debug_assertions/overflow-checks-dependent tests need to re-run in release;
            # the DEBUG full-workspace pass covers every other crate.
            # NARROW_ACTIVE is intentionally not applied here. The release pass is
            # scoped by release-sensitivity (task/4390), not the affected-crate set
            # (task/4060). Over-running the full release-sensitive set on a rare
            # --profile both --scope branch is safe (fail-wide), and avoids entangling
            # two orthogonal scoping axes — do not "fix" this by narrowing this pass.
            emit_nextest_pass "$_RELEASE_ALL_FLAGS" "$rel" "$outer_timeout"
        else
            # Debug pass.
            if [ "$NARROW_ACTIVE" -eq 1 ]; then
                # Narrowed debug pass: all affected crates (including OCCT) in one nextest
                # pass. Task 4451: no gated/ungated split; the nextest occt group bounds
                # OCCT concurrency (C3 completeness: an OCCT crate enters the affected set
                # as a reverse-dep of a changed non-OCCT crate even when RUN_OCCT_GATE=0).
                emit_nextest_pass "$AFFECTED_ALL_FLAGS" "$rel" "$outer_timeout"
            else
                # Full-workspace debug pass (scope=all and non-narrow branch/staged).
                # Task 4451: OCCT crates are now IN the pool (--workspace, no --exclude);
                # the nextest occt test-group (max-threads=24, env-driven) bounds their concurrency.
                emit_nextest_pass "--workspace" "$rel" "$outer_timeout"
            fi
        fi
    done

    # Release the semaphore slot after all passes complete.
    # The executor calls test_semaphore_release; the printer emits a comment.
    # The slot is also freed automatically on any verify.sh exit (FD 9 closes),
    # so the failure path needs no explicit release sentinel.
    add "@@SEMAPHORE_RELEASE@@"
}

build_plan() {
    # manifold prebuilt guard: fail fast (with a clear "run the deps script"
    # message) if the prebuilt manifold libs that .cargo/config.toml's
    # [target.*.manifold] override links are missing or version-drifted —
    # before any multi-minute compile turns that into a cryptic linker error.
    if [ "$RUN_RUST" -eq 1 ]; then
        add "./scripts/check-manifold-deps.sh"
    fi

    # tree-sitter parser regeneration is a Rust-build prerequisite.
    if [ "$RUN_RUST" -eq 1 ]; then
        add "./scripts/tree-sitter-generate.sh"
    fi

    # Compile-phase PSI admission gate (task 4618): soft backpressure backstop
    # for the jobserver's implicit-token leak (FIFO pool tokens + 1 implicit
    # token per concurrent cargo) and non-cargo load.  Emitted only when
    # cargo check/clippy will actually run (lint or typecheck side); pure
    # 'test' is not emitted here — the nextest compile is already inside the
    # existing test psi-gate + held-slot region (no double-gate).
    # DF_VERIFY_ROLE=merge is bypass at RUNTIME inside compile_gate() (CAVEAT 1);
    # the plan line is still emitted in merge plans so the plan shape is
    # role-invariant (mirrors the psi-gate idiom).
    if [ "$RUN_RUST" -eq 1 ] && { [ "$DO_LINT" -eq 1 ] || [ "$DO_TYPECHECK" -eq 1 ]; }; then
        add "./scripts/verify.sh compile-gate"
    fi

    # typecheck (cargo check) only when NOT also linting — clippy --all-targets
    # is a strict superset of `cargo check`, so running both would be redundant.
    if [ "$DO_TYPECHECK" -eq 1 ] && [ "$DO_LINT" -eq 0 ] && [ "$RUN_RUST" -eq 1 ]; then
        if [ "$NARROW_ACTIVE" -eq 1 ]; then
            add "timeout --kill-after=60 ${_VERIFY_CHECK_TIMEOUT} ${CARGO_PRIO}cargo check ${AFFECTED_ALL_FLAGS} --tests"
        else
            add "timeout --kill-after=60 ${_VERIFY_CHECK_TIMEOUT} ${CARGO_PRIO}cargo check --workspace --tests"
        fi
    fi

    # GUI ecosystem (npm). Rust changes imply these too; they are fast. Only
    # meaningful when there is a GUI check to run — the GUI has a test side
    # (npm test) and a typecheck (npm run typecheck) but no `cargo check`
    # analogue, so a pure typecheck action skips it entirely (verify.sh's own
    # `typecheck` action is cargo-check-only; the GUI ecosystem has no equivalent).
    #
    # The GUI typecheck (tsc --noEmit) now runs whenever this block runs — on the
    # TEST side as well as the lint side — not lint-only as before. Rationale: the
    # orchestrator's inner TDD loop runs `verify.sh test --scope branch` (npm test
    # = vitest), which never type-checks; a type-only break that renders fine at
    # runtime (e.g. a solid-js <Show> function-child rejected by the non-keyed
    # overload) therefore stayed invisible through development and only surfaced at
    # lint/merge time — by which point, since any Rust change forces RUN_GUI=1, it
    # blocks every task's branch verify on an inherited error. Putting tsc on the
    # test side catches this class in the cheap inner loop. The block is built ONCE
    # (not per-profile), so a single `&& npm run typecheck` means action=all runs it
    # exactly once — no double-run.
    #
    # FAIL-FAST: emitted BEFORE add_test_passes (the expensive pole) so a broken
    # gui tsc fails the plan in ~minutes, not after 85 min of Rust build+test.
    # (task #4448 / incident fix for #4446)
    #
    # BOUNDED node||cargo OVERLAP (task #4448, Leo's directive): when a rust
    # foreground cheap gate (clippy/gui-feature-check) is also emitted for this
    # action (DO_LINT=1 && RUN_RUST=1), background the node lane so it runs
    # concurrently with those gates. Node npm runs off the rustc jobserver →
    # zero jobserver contention. bg PID variable persists across plan entries
    # because the executor evals every entry in this shell (same-shell eval).
    # For action=test there is no rust foreground gate; the node lane stays plain
    # (pure fail-fast reorder, no overlap). For action=typecheck the node lane is
    # empty (gui block gated on test||lint) → unchanged.
    local _gui_cmd="" _sidecar_cmd="" _ts_cmd="" _node_lane=""
    if [ "$RUN_GUI" -eq 1 ] && { [ "$DO_TEST" -eq 1 ] || [ "$DO_LINT" -eq 1 ]; }; then
        # typecheck always (whenever the block runs, test OR lint); npm test only
        # on the test side.
        local gui_inner="npm ci && npm run typecheck"
        [ "$DO_TEST" -eq 1 ] && gui_inner+=" && npm test"
        _gui_cmd="if test -d gui; then $(wrap_subshell gui 15 "$gui_inner"); fi"

        # sidecar has no vitest side; both typecheck passes run whenever the block does.
        local sidecar_inner="npm ci && npm run typecheck && npm run typecheck:test"
        _sidecar_cmd="if test -f gui/sidecar/package-lock.json; then $(wrap_subshell gui/sidecar 10 "$sidecar_inner"); fi"

        _ts_cmd="if test -f tree-sitter-reify/package-lock.json; then $(wrap_subshell tree-sitter-reify 10 "npm ci"); fi"
        _node_lane="${_gui_cmd} && ${_sidecar_cmd} && ${_ts_cmd}"
    fi

    # Overlap path: background the node lane BEFORE the foreground rust cheap
    # gates (clippy + gui-feature-check) so they run concurrently. The bg PID
    # variable persists into the join entry below (same executor shell).
    #
    # Cleanup trap: registered in the same eval so it fires on any EXIT (success
    # or failure). If a foreground rust gate fails before the wait join, the
    # executor calls exit and the trap kills the still-running npm job instead of
    # orphaning it.
    #
    # The kill is wrapped in an `if ...; then :; fi` rather than a bare sequence.
    # On the happy path `wait` has already reaped the job before EXIT fires, so
    # the kill returns 1 (no such process). Under the script's `set -euo
    # pipefail`, a *bare* `kill ...; true` poisons the exit code: bash aborts the
    # trap body at the failing kill BEFORE reaching `true`, flipping a fully
    # passing run (rc=0 after "all checks passed") to rc=1 (regression from
    # commit 9b398f7a26; esc-3993-22, independently reproduced under bash 5.2 as
    # esc-4431-30). An `if` *condition* is exempt from set -e, so
    # `if kill ...; then :; fi` swallows the no-such-process failure without
    # aborting — and still reaps the job on the fail path (kill succeeds → `:`).
    # NOTE: "|| true" is intentionally avoided here — the npm ci hardening test
    # (test_npm_ci_hardening.sh Test 3) asserts that no plan line contains
    # "npm ci.*|| true", and the trap is on the same line as the npm ci call;
    # the `if`-guard achieves the same set -e safety without that token.
    if [ "$DO_LINT" -eq 1 ] && [ "$RUN_RUST" -eq 1 ] && [ -n "$_node_lane" ]; then
        add "{ ${_node_lane} ; } & _VERIFY_NODE_BG_PID=\$!; trap 'if kill \"\$_VERIFY_NODE_BG_PID\" 2>/dev/null; then :; fi; _verify_cleanup' EXIT"
    fi

    # lint: clippy over all targets, warnings-as-errors.
    if [ "$DO_LINT" -eq 1 ] && [ "$RUN_RUST" -eq 1 ]; then
        if [ "$NARROW_ACTIVE" -eq 1 ]; then
            add "timeout --kill-after=60 ${_VERIFY_CLIPPY_TIMEOUT} ${CARGO_PRIO}cargo clippy ${AFFECTED_ALL_FLAGS} --all-targets -- -D warnings"
        else
            add "timeout --kill-after=60 ${_VERIFY_CLIPPY_TIMEOUT} ${CARGO_PRIO}cargo clippy --workspace --all-targets -- -D warnings"
        fi
    fi

    # gui-feature compile-check: type-check reify-gui's #[cfg(feature="gui")] code
    # (engine.rs, main.rs, tests/*) which is never reached by the workspace-wide
    # cargo check / clippy / nextest passes (all run without --features gui).
    #
    # Placed on the LINT side (DO_LINT=1 && RUN_RUST=1) because:
    #   - It is a compile-check, semantically adjacent to clippy.
    #   - LINT is the only action that fires on EVERY merge path (orchestrator
    #     lint_command, pre-merge-commit `all`, hooks/project-checks `all`).
    #   - Gating under RUN_RUST (not RUN_GUI) keeps frontend-only/docs-only
    #     commits fast — only Rust changes can break gui-gated Rust.
    #
    # ensure-gui-sidecar-placeholder.sh runs first because tauri_build::build()
    # (in gui/src-tauri/build.rs) validates bundle.externalBin and panics if
    # gui/src-tauri/sidecar/reify-sidecar-<triple> is absent from disk; the stub
    # satisfies the existence check without clobbering a real built sidecar.
    if [ "$DO_LINT" -eq 1 ] && [ "$RUN_RUST" -eq 1 ]; then
        add "if test -f gui/src-tauri/Cargo.toml; then ./scripts/ensure-gui-sidecar-placeholder.sh && timeout --kill-after=60 ${_VERIFY_CLIPPY_TIMEOUT} ${CARGO_PRIO}cargo check -p reify-gui --features gui --tests; fi"
    fi

    # Overlap join: wait for the background node lane before infra checks / pole.
    # Maximises the concurrency window (join as late as possible while still
    # preceding the expensive pole and infra checks).
    if [ "$DO_LINT" -eq 1 ] && [ "$RUN_RUST" -eq 1 ] && [ -n "$_node_lane" ]; then
        add 'wait "$_VERIFY_NODE_BG_PID"'
    fi

    # Plain path: node lane as sequential lines (no foreground rust gate, e.g. action=test).
    if [ -n "$_node_lane" ] && { [ "$DO_LINT" -eq 0 ] || [ "$RUN_RUST" -eq 0 ]; }; then
        add "$_gui_cmd"
        add "$_sidecar_cmd"
        add "$_ts_cmd"
    fi

    # Cheap static infra checks (opt-in). Test-side and lint-side, mirroring the
    # historical orchestrator split. Tied to RUN_RUST (the heavy gate) so a
    # frontend-only or docs-only staged commit stays fast.
    #
    # FAIL-FAST: emitted BEFORE add_test_passes (task #4448).
    if [ "$INCLUDE_INFRA" -eq 1 ] && [ "$RUN_RUST" -eq 1 ]; then
        if [ "$DO_TEST" -eq 1 ]; then
            add "if test -f tests/sync_comments_test.sh; then timeout --kill-after=60 10m bash tests/sync_comments_test.sh; else echo 'WARNING: sync_comments_test.sh not found, skipping'; fi"
            # task #4624: pre-build reify-audit OUTSIDE the run_all.sh wall (30m).
            # By the time run_all.sh runs, target/release/{reify-audit,ptodo-baseline-gen}
            # are fresh so the in-wall freshness guard finds them fresh and skips the cold
            # build.  sccache (RUSTC_WRAPPER) makes this cheap when already cached.
            # Timeout is 10m (distinct from the run_all wall) so the plan-shape test can assert
            # the pre-step is not the walled run_all.sh line.
            #
            # ADMISSION CONTROLS: this pre-step runs OUTSIDE compile_gate()/psi_gate().
            # Rationale: (1) DF_VERIFY_ROLE=merge is exempt from all gates anyway;
            # (2) sccache makes this a no-op when warm; (3) this plan line emits in the
            # infra block — after all main Rust compile phases — so it does not race with
            # the compile-gate window that guards clippy/check; (4) the CLAUDE.md
            # admission-control invariant is for task×compile contention during the
            # main psi-gate/slot region, which this small pre-build does not enter.
            add "if test -f crates/reify-audit/Cargo.toml; then timeout --kill-after=60 10m ${CARGO_PRIO}cargo build --release -q -p reify-audit; fi"
            # Positive assertion: if the Cargo.toml exists but the pre-build did not
            # produce the binary, abort loudly rather than silently degrading to SKIP.
            # Guards against the pre-step being removed or reordered without updating
            # the REIFY_AUDIT_NO_COLD_BUILD backstop below.  Only fires if the
            # pre-step is present (Cargo.toml guard matches) but produces no output.
            add "if test -f crates/reify-audit/Cargo.toml && [ ! -f target/release/reify-audit ]; then echo 'ERROR(#4624): reify-audit binary missing after pre-build step — PTODO gate will silently SKIP; restore the pre-step above or remove this check deliberately' >&2; false; fi"
            # Arm the budget-safe backstop: REIFY_AUDIT_NO_COLD_BUILD=1 tells the
            # freshness guard to skip rather than cold-build if somehow the pre-step
            # above was bypassed or narrowed (defense-in-depth; maps to SKIP exit 0).
            # task #3810/esc-3810-4: bumped 20m -> 30m. The infra suite grew past
            # the 20m wall after the warm-lane CoW-pool tests landed (they auto-run
            # heavy cargo blocks when TMPDIR is XFS-reflink, i.e. on the merge worker),
            # tipping a suite already near its budget over the wall (exit 124). 30m
            # restores headroom for the full --scope all / merge gate.
            add "if test -f tests/infra/run_all.sh; then REIFY_AUDIT_NO_COLD_BUILD=1 timeout --kill-after=60 30m bash tests/infra/run_all.sh; fi"
        fi
        if [ "$DO_LINT" -eq 1 ]; then
            add "if test -f scripts/test_pm_standardization.sh; then timeout --kill-after=60 10m bash scripts/test_pm_standardization.sh; else echo 'WARNING: test_pm_standardization.sh not found, skipping'; fi"
            add "if test -f scripts/check_event_inventory.sh; then timeout --kill-after=60 5m bash scripts/check_event_inventory.sh; else echo 'WARNING: check_event_inventory.sh not found, skipping'; fi"
        fi
    fi

    # Selective infra injection (task 4523): task-level path runs the infra
    # drift-guards for any changed verify-pipeline artifact.  FAIL-FAST: emitted
    # BEFORE add_test_passes (the expensive long-pole).  One guarded for-loop
    # per glob — the glob literal is embedded in the emitted subshell command
    # and expands at EXECUTION time under CWD=REPO_ROOT.
    # set -f / set +f prevents the shell from pathname-expanding the token
    # during loop iteration here at build time, so the literal glob string
    # (e.g. tests/infra/test_verify_*.sh) always reaches the emitted plan.
    # Suppressed when INCLUDE_INFRA=1: run_all.sh already runs the full suite
    # (a superset), so the selective subset would double-run hermetic tests.
    if [ "$DO_TEST" -eq 1 ] && [ -n "$SELECTED_INFRA_GLOBS" ] && [ "$INCLUDE_INFRA" -eq 0 ]; then
        local _glob
        set -f  # disable pathname expansion: keep glob tokens as literals
        for _glob in $SELECTED_INFRA_GLOBS; do
            add "( for _vt in $_glob; do [ -f \"\$_vt\" ] || continue; timeout --kill-after=60 10m bash \"\$_vt\" || exit \$?; done )"
        done
        set +f
    fi

    # test: gated + ungated cargo passes, per profile.
    # Emitted LAST — this is the expensive long-pole (psi-gate + full cargo
    # nextest run + OCCT-gated passes). All cheap gates run before this.
    # (task #4448 fail-fast reorder)
    if [ "$DO_TEST" -eq 1 ] && [ "$RUN_RUST" -eq 1 ]; then
        add_test_passes
    fi
}
build_plan

# ---------------------------------------------------------------------------
# Emit: print the plan (oracle) or execute it (&& semantics)
# ---------------------------------------------------------------------------
if [ "$PRINT_PLAN" -eq 1 ]; then
    echo "# verify.sh plan — action=$ACTION profile=$PROFILE scope=$SCOPE include_infra=$INCLUDE_INFRA nextest=$NEXTEST role=$DF_VERIFY_ROLE"
    echo "# scope decision — RUN_RUST=$RUN_RUST RUN_GUI=$RUN_GUI RUN_OCCT_GATE=$RUN_OCCT_GATE"
    echo "# narrowing — NARROW_ACTIVE=$NARROW_ACTIVE affected=${AFFECTED:-}"
    echo "# --- environment (process-level; inherited by every command below) ---"
    for _e in "${ENV_LINES[@]}"; do echo "# $_e"; done
    echo "# --- commands (executed in order; '&&' semantics — stop on first failure) ---"
    if [ "${#PLAN[@]}" -eq 0 ]; then
        echo "# (no commands — nothing to verify for this action/scope)"
    fi
    for _cmd in "${PLAN[@]+"${PLAN[@]}"}"; do
        case "$_cmd" in
            '@@SEMAPHORE_ACQUIRE@@')
                printf '# >>> test-run semaphore: ACQUIRE held slot — TEST-EXECUTION gated region BEGINS (held in verify.sh, not a fire-and-return line)\n'
                ;;
            '@@SEMAPHORE_RELEASE@@')
                printf '# <<< test-run semaphore: RELEASE held slot — TEST-EXECUTION gated region ENDS\n'
                ;;
            *)
                printf '%s\n' "$_cmd"
                ;;
        esac
    done
    exit 0
fi

if [ "${#PLAN[@]}" -eq 0 ]; then
    echo "verify.sh: nothing to verify (action=$ACTION scope=$SCOPE) — no commands in plan." >&2
    exit 0
fi

for _cmd in "${PLAN[@]}"; do
    case "$_cmd" in
        '@@SEMAPHORE_ACQUIRE@@')
            test_semaphore_acquire || {
                _rc=$?
                echo "verify.sh: FAILED (exit $_rc): test-run semaphore acquire" >&2
                exit "$_rc"
            }
            continue
            ;;
        '@@SEMAPHORE_RELEASE@@')
            test_semaphore_release || true
            continue
            ;;
    esac
    echo "verify.sh: + $_cmd" >&2
    eval "$_cmd" || {
        _rc=$?
        echo "verify.sh: FAILED (exit $_rc): $_cmd" >&2
        exit "$_rc"
    }
done
echo "verify.sh: all checks passed (action=$ACTION profile=$PROFILE scope=$SCOPE)." >&2
