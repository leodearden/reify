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
#   - CARGO_MAKEFLAGS=--jobserver-auth=fifo:/tmp/reify-jobserver  ONLY when that FIFO
#     exists (else cargo uses its own per-process job pool). This is a COMPILE-time
#     concurrency control; OCCT TEST-execution concurrency is bounded by a separate
#     mechanism (the semaphore wrapper + --test-threads=1 below).
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
# OCCT safety:
#   OCCT C++ globals are PER-PROCESS; cross-process isolation is already provided by
#   cargo's test-binary parallelism. Cross-WORKTREE contention (concurrent worktrees)
#   is bounded by an N-slot counting semaphore in scripts/cargo-test-occt-gated.sh;
#   intra-process contention is bounded by `-- --test-threads=1`. The OCCT-touching
#   crate set is
#   defined exactly once in scripts/occt-scope-lib.sh and shared with the drift test.

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

usage() {
    sed -n '2,48p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

# ---------------------------------------------------------------------------
# PSI gate — throttle per-task test phases under multi-worktree verify bursts
# ---------------------------------------------------------------------------

# _psi_should_pass() — helper for psi_gate().
# Returns 0 if both PSI and window conditions are satisfied (safe to dispatch
# now), or 1 otherwise.  Reads PROC_PATH, THRESHOLD, WINDOW, DISPATCH from
# psi_gate()'s dynamic scope (bash locals are visible to callees via dynamic
# scoping, not lexical scoping).  $1 = current timestamp (integer seconds).
# Called from both the flock subshell and the lock-free fallback path.
_psi_should_pass() {
    local _ts="$1" _mtime _age _avg10
    _mtime=$(stat -c %Y "$DISPATCH" 2>/dev/null || echo 0)
    _age=$(( _ts - _mtime ))
    _avg10=$(awk '/^some/ {
        for (i=1; i<=NF; i++) {
            if ($i ~ /^avg10=/) { v=$i; sub(/^avg10=/, "", v); print v; exit }
        }
    }' "$PROC_PATH" 2>/dev/null || echo "")
    [ -n "$_avg10" ] && \
        awk -v p="$_avg10" -v t="$THRESHOLD" 'BEGIN{exit !(p<t)}' && \
        [ "$_age" -ge "$WINDOW" ]
}

# psi_gate() — wait for CPU headroom before dispatching the test phase.
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
    local THRESHOLD="${REIFY_PSI_GATE_THRESHOLD:-50}"
    local WINDOW="${REIFY_PSI_GATE_WINDOW:-20}"
    local MAX_WAIT="${REIFY_PSI_GATE_MAX_WAIT:-1800}"
    local POLL="${REIFY_PSI_GATE_POLL:-5}"
    local PROC_PATH="${REIFY_PSI_GATE_PROC_PATH:-/proc/pressure/cpu}"
    local DISPATCH="${REIFY_PSI_GATE_DISPATCH_FILE:-/tmp/reify-verify-last-dispatch}"

    # (1) Break-glass bypass — total bypass: no PSI read, no touch, no wait
    if [ "${REIFY_PSI_GATE_DISABLE:-}" = "1" ]; then
        echo "verify.sh: psi-gate disabled (REIFY_PSI_GATE_DISABLE=1)" >&2
        return 0
    fi

    # (2) Merge bypass: skip wait + bump timestamp so the next task backs off
    if [ "${DF_VERIFY_ROLE:-task}" = "merge" ]; then
        touch "$DISPATCH"
        echo "verify.sh: psi-gate bypass (role=merge) — timestamp bumped" >&2
        return 0
    fi

    # (3) Fail-open on missing/unreadable PSI source (older kernels / non-Linux hosts).
    # Touch the dispatch file so cross-process coordination stays consistent;
    # proceed without blocking the build.
    if [ ! -r "$PROC_PATH" ]; then
        echo "verify.sh: WARNING — PSI gate disabled — kernel lacks ${PROC_PATH}" >&2
        touch "$DISPATCH"
        return 0
    fi

    # (4) Task poll loop: wait for avg10 < THRESHOLD AND age >= WINDOW.
    # The read-mtime / compare / touch critical section is wrapped in a flock
    # so concurrent waiters pass one-at-a-time and each pass re-touches —
    # guaranteeing consecutive passes are >= WINDOW apart.
    local deadline
    deadline=$(( $(date +%s) + MAX_WAIT ))

    while true; do
        local now _flock_rc
        now=$(date +%s)
        _flock_rc=10  # not-yet (default: condition not met)

        if command -v flock >/dev/null 2>&1; then
            # Atomic check-and-touch inside a flock subshell.
            # Exit codes: 0=pass, 9=lock-timeout, 10=not-yet.
            # The subshell exits immediately so the FD is not inherited by
            # long-lived children (no cargo/sccache FD-9-inheritance hazard).
            # Use "|| _flock_rc=$?" to capture the non-zero exit without
            # triggering set -e in the outer function.
            _flock_rc=0
            (
                flock -w 5 9 || exit 9
                _ts=$(date +%s)
                if _psi_should_pass "$_ts"; then
                    touch "$DISPATCH"
                    exit 0
                fi
                exit 10
            ) 9>"${DISPATCH}.lock" || _flock_rc=$?
            # ${DISPATCH}.lock is a single fixed-name file in /tmp — one lockfile per
            # coordination point, does not accumulate.  Intentionally left in place
            # across runs (O_CREAT via '>' redirect; harmless stale presence).
        else
            # lock-free best-effort fallback (flock not available)
            local _ts
            _ts=$(date +%s)
            if _psi_should_pass "$_ts"; then
                touch "$DISPATCH"
                _flock_rc=0
            fi
        fi

        if [ "$_flock_rc" -eq 0 ]; then
            return 0
        fi

        # Re-sample now: the flock attempt above may have blocked up to 5s,
        # so the value captured at the top of the loop can be stale.
        now=$(date +%s)
        # Give up if we've waited too long
        if [ "$now" -ge "$deadline" ]; then
            echo "verify.sh: PSI gate gave up after ${MAX_WAIT}s waiting for CPU headroom" >&2
            return 75
        fi

        sleep "$POLL"
    done
}

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------
ACTION=""
PROFILE="debug"
PROFILE_EXPLICIT=0   # set to 1 if --profile was given explicitly; keeps explicit authoritative
SCOPE="all"
INCLUDE_INFRA=0
PRINT_PLAN=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        test|lint|typecheck|all|psi-gate)
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

# A merge in progress cannot trust `git diff --cached` (the index reflects the
# merge result, not a curated stage), so force a full verification. Detected via
# the git-dir-relative MERGE_HEAD so it works correctly inside linked worktrees.
_MERGE_HEAD="$(git -C "$REPO_ROOT" rev-parse --git-path MERGE_HEAD 2>/dev/null || echo '')"
if [ -n "$_MERGE_HEAD" ] && [ -f "$_MERGE_HEAD" ] && [ "$SCOPE" != "all" ]; then
    echo "verify.sh: MERGE_HEAD present — forcing --scope all (merge in progress)" >&2
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

    # Inherit the shared global jobserver ONLY when its FIFO exists; otherwise
    # leave CARGO_MAKEFLAGS unset so cargo manages its own job pool. Exporting a
    # stale fifo path when reify-jobserver.service is down would wedge cargo.
    if [ -p /tmp/reify-jobserver ]; then
        export CARGO_MAKEFLAGS="--jobserver-auth=fifo:/tmp/reify-jobserver"
        ENV_LINES+=("export CARGO_MAKEFLAGS=--jobserver-auth=fifo:/tmp/reify-jobserver")
    else
        ENV_LINES+=("# CARGO_MAKEFLAGS left unset (no /tmp/reify-jobserver FIFO) — cargo uses its own job pool")
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
# Phase-2 narrowing: map changed files → affected crate set → -p flag strings.
#
# Eligible when: scope=branch AND RUN_RUST=1.
# (scope=staged + --narrow eligibility added in a later step.)
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
AFFECTED_OCCT_FLAGS=""
AFFECTED_UNGATED_FLAGS=""

if [ "$SCOPE" = "branch" ] && [ "$RUN_RUST" -eq 1 ]; then
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
    # Split affected set into OCCT, non-OCCT, and all-crate -p flag strings.
    # Word-split $AFFECTED (safe: Rust crate names never contain spaces).
    # shellcheck disable=SC2086
    for _nc in $AFFECTED; do
        [ -z "$_nc" ] && continue
        AFFECTED_ALL_FLAGS+=" -p $_nc"
        if is_occt_crate "$_nc"; then
            AFFECTED_OCCT_FLAGS+=" -p $_nc"
        else
            AFFECTED_UNGATED_FLAGS+=" -p $_nc"
        fi
    done
    AFFECTED_ALL_FLAGS="${AFFECTED_ALL_FLAGS# }"
    AFFECTED_OCCT_FLAGS="${AFFECTED_OCCT_FLAGS# }"
    AFFECTED_UNGATED_FLAGS="${AFFECTED_UNGATED_FLAGS# }"
fi

# ---------------------------------------------------------------------------
# Plan construction (built ONCE; print vs execute branches only at the leaves)
# ---------------------------------------------------------------------------
PLAN=()
add() { PLAN+=("$1"); }

# OCCT crate flags, in occt-touching-crates.txt order.
_OCCT_CRATES=()
while IFS= read -r _c; do [ -n "$_c" ] && _OCCT_CRATES+=("$_c"); done <<<"$_OCCT_DECLARED"
P_FLAGS=""
EXCLUDE_FLAGS=""
for _c in "${_OCCT_CRATES[@]}"; do
    P_FLAGS+=" -p $_c"
    EXCLUDE_FLAGS+=" --exclude $_c"
done
P_FLAGS="${P_FLAGS# }"
EXCLUDE_FLAGS="${EXCLUDE_FLAGS# }"

# Release-sensitive crate flags: split by OCCT membership into gated and ungated.
# Gated: OCCT ∩ release-sensitive = reify-eval only (stays flock-gated in release).
# Ungated: release-sensitive ∖ OCCT = the non-OCCT crates (full nextest concurrency).
# reify-kernel-occt, reify-cli, reify-config have zero release-sensitive tests and
# correctly drop out of the release pass; the debug full-workspace pass covers them.
_RELEASE_DECLARED="$(release_declared_set)"
_RELEASE_GATED_FLAGS=""
_RELEASE_UNGATED_FLAGS=""
while IFS= read -r _rc; do
    [ -z "$_rc" ] && continue
    if is_occt_crate "$_rc"; then
        _RELEASE_GATED_FLAGS+=" -p $_rc"
    else
        _RELEASE_UNGATED_FLAGS+=" -p $_rc"
    fi
done <<<"$_RELEASE_DECLARED"
_RELEASE_GATED_FLAGS="${_RELEASE_GATED_FLAGS# }"
_RELEASE_UNGATED_FLAGS="${_RELEASE_UNGATED_FLAGS# }"

# Ungated tail runner: prefer cargo-nextest (one global pool over ~hundreds of
# test binaries) with a graceful fallback to plain `cargo test` when nextest is
# not installed. OCCT crates are excluded from this pass (they run gated).
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

add_test_passes() {
    # PSI gate: must pass before any cargo test work starts.
    # In execute mode: eval runs this as a subprocess that inherits DF_VERIFY_ROLE
    # and REIFY_PSI_GATE_*; exit 75 (EX_TEMPFAIL) propagates → orchestrator retries.
    # In --print-plan mode: printed faithfully as a normal plan line.
    add "./scripts/verify.sh psi-gate"

    local profile rel gated_timeout outer_timeout ungated
    # Timeout budgets sized to absorb a COLD merge-worktree workspace compile.
    # Bumped 2026-06-03 (Leo) after recurring exit-124 cold-compile timeouts on the
    # merge gate (esc-4178 / esc-4180 cluster killed the debug nextest mid-compile at
    # the old 30m; the OCCT gate hit the old 2700s). sccache shares rustc output
    # across worktrees, so warm runs finish well inside these — the larger caps only
    # bite a genuinely cold cache. NOTE: the gated values are asserted in
    # tests/infra/test_occt_flock_gate.sh (Test 17) — keep them in sync.
    for profile in "${PROFILES[@]}"; do
        if [ "$profile" = "release" ]; then
            rel=" --release"; gated_timeout=4800; outer_timeout="75m"
        else
            rel=""; gated_timeout=3600; outer_timeout="60m"
        fi

        if [ "$profile" = "release" ]; then
            # Release pass: sensitivity-scoped to the release-sensitive crate set
            # (scripts/release-sensitive-crates.txt, guarded by
            # tests/infra/test_release_scoped_scope.sh). Only crates with
            # debug_assertions/overflow-checks-dependent tests need to re-run in
            # release; the DEBUG full-workspace pass covers every other crate, so
            # total merge-gate coverage is preserved.

            # Gated release: OCCT-touching release-sensitive crates only (reify-eval).
            # reify-kernel-occt, reify-cli, reify-config have zero release-sensitive
            # tests and drop out of the release pass entirely.
            if [ "$RUN_OCCT_GATE" -eq 1 ] && [ -n "$_RELEASE_GATED_FLAGS" ]; then
                add "REIFY_OCCT_TEST_TIMEOUT=${gated_timeout} ./scripts/cargo-test-occt-gated.sh ${CARGO_PRIO}cargo test ${_RELEASE_GATED_FLAGS}${rel} -- --test-threads=1"
            fi

            # Ungated release: non-OCCT release-sensitive crates, full concurrency.
            if [ -n "$_RELEASE_UNGATED_FLAGS" ]; then
                if [ "$NEXTEST" -eq 1 ]; then
                    ungated="timeout --kill-after=60 ${outer_timeout} ${CARGO_PRIO}cargo nextest run ${_RELEASE_UNGATED_FLAGS}${rel}"
                else
                    ungated="timeout --kill-after=60 ${outer_timeout} ${CARGO_PRIO}cargo test ${_RELEASE_UNGATED_FLAGS}${rel} -- --test-threads=1"
                fi
                add "$ungated"
            fi
        else
            # Debug pass.
            if [ "$NARROW_ACTIVE" -eq 1 ]; then
                # Narrowed debug pass: -p per affected crate, split by OCCT membership.
                # The gated-run condition keys on the affected∩OCCT intersection (NOT
                # RUN_OCCT_GATE): an OCCT crate can enter the affected set as a reverse-dep
                # of a changed non-OCCT crate where RUN_OCCT_GATE=0 (C3 completeness —
                # gating on RUN_OCCT_GATE would silently skip that OCCT dependent).
                if [ -n "$AFFECTED_OCCT_FLAGS" ]; then
                    add "REIFY_OCCT_TEST_TIMEOUT=${gated_timeout} ./scripts/cargo-test-occt-gated.sh ${CARGO_PRIO}cargo test ${AFFECTED_OCCT_FLAGS}${rel} -- --test-threads=1"
                fi
                if [ -n "$AFFECTED_UNGATED_FLAGS" ]; then
                    if [ "$NEXTEST" -eq 1 ]; then
                        ungated="timeout --kill-after=60 ${outer_timeout} ${CARGO_PRIO}cargo nextest run ${AFFECTED_UNGATED_FLAGS}${rel}"
                    else
                        ungated="timeout --kill-after=60 ${outer_timeout} ${CARGO_PRIO}cargo test ${AFFECTED_UNGATED_FLAGS}${rel} -- --test-threads=1"
                    fi
                    add "$ungated"
                fi
            else
                # Full-workspace debug pass (scope=all and non-narrow branch/staged).
                # Gated pass: OCCT-touching crates, bounded via the semaphore wrapper,
                # single-threaded. No outer timeout — the wrapper owns it via
                # REIFY_OCCT_TEST_TIMEOUT (lock-wait time does not consume the budget).
                if [ "$RUN_OCCT_GATE" -eq 1 ]; then
                    add "REIFY_OCCT_TEST_TIMEOUT=${gated_timeout} ./scripts/cargo-test-occt-gated.sh ${CARGO_PRIO}cargo test ${P_FLAGS}${rel} -- --test-threads=1"
                fi

                # Ungated tail: everything except the OCCT crates, full concurrency.
                if [ "$NEXTEST" -eq 1 ]; then
                    ungated="timeout --kill-after=60 ${outer_timeout} ${CARGO_PRIO}cargo nextest run --workspace ${EXCLUDE_FLAGS}${rel}"
                else
                    ungated="timeout --kill-after=60 ${outer_timeout} ${CARGO_PRIO}cargo test --workspace ${EXCLUDE_FLAGS}${rel} -- --test-threads=1"
                fi
                add "$ungated"
            fi
        fi
    done
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

    # typecheck (cargo check) only when NOT also linting — clippy --all-targets
    # is a strict superset of `cargo check`, so running both would be redundant.
    if [ "$DO_TYPECHECK" -eq 1 ] && [ "$DO_LINT" -eq 0 ] && [ "$RUN_RUST" -eq 1 ]; then
        if [ "$NARROW_ACTIVE" -eq 1 ]; then
            add "timeout --kill-after=60 30m ${CARGO_PRIO}cargo check ${AFFECTED_ALL_FLAGS} --tests"
        else
            add "timeout --kill-after=60 30m ${CARGO_PRIO}cargo check --workspace --tests"
        fi
    fi

    # lint: clippy over all targets, warnings-as-errors.
    if [ "$DO_LINT" -eq 1 ] && [ "$RUN_RUST" -eq 1 ]; then
        if [ "$NARROW_ACTIVE" -eq 1 ]; then
            add "timeout --kill-after=60 45m ${CARGO_PRIO}cargo clippy ${AFFECTED_ALL_FLAGS} --all-targets -- -D warnings"
        else
            add "timeout --kill-after=60 45m ${CARGO_PRIO}cargo clippy --workspace --all-targets -- -D warnings"
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
        add "if test -f gui/src-tauri/Cargo.toml; then ./scripts/ensure-gui-sidecar-placeholder.sh && timeout --kill-after=60 45m ${CARGO_PRIO}cargo check -p reify-gui --features gui --tests; fi"
    fi

    # test: gated + ungated cargo passes, per profile.
    if [ "$DO_TEST" -eq 1 ] && [ "$RUN_RUST" -eq 1 ]; then
        add_test_passes
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
    if [ "$RUN_GUI" -eq 1 ] && { [ "$DO_TEST" -eq 1 ] || [ "$DO_LINT" -eq 1 ]; }; then
        # typecheck always (whenever the block runs, test OR lint); npm test only
        # on the test side.
        local gui_inner="npm ci && npm run typecheck"
        [ "$DO_TEST" -eq 1 ] && gui_inner+=" && npm test"
        add "if test -d gui; then $(wrap_subshell gui 15 "$gui_inner"); fi"

        # sidecar has no vitest side; both typecheck passes run whenever the block does.
        local sidecar_inner="npm ci && npm run typecheck && npm run typecheck:test"
        add "if test -f gui/sidecar/package-lock.json; then $(wrap_subshell gui/sidecar 10 "$sidecar_inner"); fi"

        add "if test -f tree-sitter-reify/package-lock.json; then $(wrap_subshell tree-sitter-reify 10 "npm ci"); fi"
    fi

    # Cheap static infra checks (opt-in). Test-side and lint-side, mirroring the
    # historical orchestrator split. Tied to RUN_RUST (the heavy gate) so a
    # frontend-only or docs-only staged commit stays fast.
    if [ "$INCLUDE_INFRA" -eq 1 ] && [ "$RUN_RUST" -eq 1 ]; then
        if [ "$DO_TEST" -eq 1 ]; then
            add "if test -f tests/sync_comments_test.sh; then timeout --kill-after=60 10m bash tests/sync_comments_test.sh; else echo 'WARNING: sync_comments_test.sh not found, skipping'; fi"
            add "if test -f tests/infra/run_all.sh; then timeout --kill-after=60 20m bash tests/infra/run_all.sh; fi"
        fi
        if [ "$DO_LINT" -eq 1 ]; then
            add "if test -f scripts/test_pm_standardization.sh; then timeout --kill-after=60 10m bash scripts/test_pm_standardization.sh; else echo 'WARNING: test_pm_standardization.sh not found, skipping'; fi"
            add "if test -f scripts/check_event_inventory.sh; then timeout --kill-after=60 5m bash scripts/check_event_inventory.sh; else echo 'WARNING: check_event_inventory.sh not found, skipping'; fi"
        fi
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
        printf '%s\n' "$_cmd"
    done
    exit 0
fi

if [ "${#PLAN[@]}" -eq 0 ]; then
    echo "verify.sh: nothing to verify (action=$ACTION scope=$SCOPE) — no commands in plan." >&2
    exit 0
fi

for _cmd in "${PLAN[@]}"; do
    echo "verify.sh: + $_cmd" >&2
    eval "$_cmd" || {
        _rc=$?
        echo "verify.sh: FAILED (exit $_rc): $_cmd" >&2
        exit "$_rc"
    }
done
echo "verify.sh: all checks passed (action=$ACTION profile=$PROFILE scope=$SCOPE)." >&2
