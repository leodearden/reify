#!/usr/bin/env bash
# scripts/lib_git_env_scrub.sh — strip the ambient GIT REPOSITORY ENVIRONMENT
# from an infra-test child process.
#
# Designed to be sourced, not executed directly.
#
# Usage:  source "$(dirname "${BASH_SOURCE[0]}")/lib_git_env_scrub.sh"
#   or:   source "$REPO_ROOT/scripts/lib_git_env_scrub.sh"
#
# ---------------------------------------------------------------------------
# THE MEASURED FAILURE (task #7106, git 2.43.0 — the version setup-dev.sh's
# Ubuntu 24.04 floor ships)
# ---------------------------------------------------------------------------
# Probed with an env-dumping hook in a throwaway repo: `pre-commit` and
# `pre-merge-commit` export exactly ONE repository variable, `GIT_INDEX_FILE`,
# and its value is RELATIVE — literally `.git/index`.  GIT_DIR, GIT_WORK_TREE
# and GIT_COMMON_DIR are NOT exported by either hook.
#
# A relative GIT_INDEX_FILE resolves against CWD.  hooks/project-checks execs
# `verify.sh all --profile debug --scope staged --include-infra` with
# CWD=REPO_ROOT, so `.git/index` names the REAL index.  The hermetic fixtures
# under tests/infra/ build throwaway repos with `git -C "$FIX" init` /
# `git -C "$FIX" add -A` and never `cd` — and GIT_INDEX_FILE OUTRANKS `git -C`.
# So an unscrubbed member test writes the FIXTURE's file list into the REAL
# index.  Measured on a real lane: the index went 480763 -> 4827 bytes and
# `git diff --cached --stat` then read "4117 files changed, 2010075 deletions".
# The visible symptom was 6 of 22 subtests failing in
# tests/infra/test_reify_audit_ptodo.sh; the silent one was the corruption.
#
# The scrub therefore belongs at TEST EXECUTION — verify.sh's selective-infra
# plan leaf, tests/infra/run_all.sh's member spawns, and run_all.sh's OWN
# repo-targeting `git -C` calls (its flaky-ledger branch read and its
# content-skip engine, the latter gated on role=merge, i.e. running ONLY under
# the hook environment that exports GIT_INDEX_FILE) — and NOT earlier: the
# scope-derivation phase legitimately reads the hook environment
# (CHANGED_FILES_RAW comes from `git diff --cached`).
#
# ---------------------------------------------------------------------------
# WHY THIS ENUMERATED SET, NOT A WHOLESALE `GIT_*` CLEAR
# ---------------------------------------------------------------------------
# See crates/reify-test-support/src/git_env.rs:55-95 — that doc is the
# workspace's ONE copy of the argument, living with the constant it justifies,
# and this list is the bash mirror of its REPO_REDIRECT_VARS.  It is
# deliberately NOT restated here.  tests/infra/test_infra_git_env_isolation.sh
# carries a one-way drift guard that re-derives REPO_REDIRECT_VARS from that
# Rust source on every infra run and fails if the Rust set gains a variable
# this list lacks.
#
# ---------------------------------------------------------------------------
# WHY `env -u`, NEVER THE `GIT_DIR= git ...` PREFIX FORM
# ---------------------------------------------------------------------------
# An EMPTY GIT_DIR is not an unset one: git 2.43 hard-fails it with
# `fatal: not a git repository: ''` (measured).  The prefix form would convert
# a working run into a total failure.  Same rationale, same measurement, as
# scripts/lib_main_checkout.sh:74-78.
#
# ---------------------------------------------------------------------------
# DELIBERATELY NO `set -euo pipefail`
# ---------------------------------------------------------------------------
# Absent from all sibling scripts/lib_*.sh: a sourced lib must not impose shell
# options on its caller.  Functions `return`, never `exit`.

# Source guard — prevent double-sourcing.
if [ "${_REIFY_LIB_GIT_ENV_SCRUB_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_LIB_GIT_ENV_SCRUB_SH_SOURCED=1

# REIFY_GIT_ENV_SCRUB_VARS — space-delimited list of variables to remove.
#
# STATED EXACTLY ONCE: both reify_git_env_scrub and reify_git_env_scrub_prefix
# below derive their `env -u` argv by iterating this string, so widening the
# list automatically reaches the runner helper AND the plan-prefix emitter.
#
# The bash mirror of crates/reify-test-support/src/git_env.rs's
# REPO_REDIRECT_VARS.  The first three are the MEASURED HOOK-EXPORTED FLOOR (git
# actually exports GIT_INDEX_FILE, and GIT_DIR/GIT_WORK_TREE are its documented
# siblings); the remaining five are the same class — each redirects part of
# "which repository / which objects", so each can silently override an explicit
# `git -C <root>`.  See the drift guard named in the header for the direction
# this list is allowed to move.
#
# DELIBERATELY NOT EXPORTED.  Both consumers below are shell FUNCTIONS resolved
# in the sourcing shell, and every real caller (scripts/verify.sh,
# tests/infra/run_all.sh, the isolation test's arm B) sources this file itself
# — so nothing needs child visibility.  Exporting it would inject a ninth
# ambient variable into all ~103 run_all.sh pool members, which
# tests/infra/run-all-ambient-vars.manifest declares itself "the single
# acknowledged ledger" of; that ledger's set-equality guard derives the live set
# from verify.sh's plan-line prefix and dark-factory's verify_env block, so a
# var injected from a sourced lib would be a SILENT hole in it rather than a
# detected drift.  Keeping the assignment shell-local is what keeps the ledger
# honest without an entry.
REIFY_GIT_ENV_SCRUB_VARS="GIT_DIR GIT_INDEX_FILE GIT_WORK_TREE GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_COMMON_DIR GIT_NAMESPACE GIT_PREFIX"

# reify_git_env_scrub <cmd> [args...] — run <cmd> with every
# REIFY_GIT_ENV_SCRUB_VARS entry REMOVED from its environment.
#
# Removed, not overwritten (see the `env -u` note in the header).  The exit
# status of <cmd> is propagated unchanged, so callers keep using their existing
# `|| rc=$?` idiom.
reify_git_env_scrub() {
    local -a _u=()
    local _v
    for _v in $REIFY_GIT_ENV_SCRUB_VARS; do
        _u+=(-u "$_v")
    done
    env "${_u[@]}" "$@"
}

# reify_git_env_scrub_prefix — print the bare `env -u VAR -u VAR ...` token
# string on stdout, with no trailing newline-sensitive framing.
#
# For interpolation into a plan leaf that verify.sh EMITS as text and executes
# later (the selective-infra loop), where a shell function is not reachable.
# Derived from the same single list, so the emitted prefix and the runner helper
# can never disagree.
reify_git_env_scrub_prefix() {
    local _out="env"
    local _v
    for _v in $REIFY_GIT_ENV_SCRUB_VARS; do
        _out="$_out -u $_v"
    done
    printf '%s' "$_out"
}
