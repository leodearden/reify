#!/usr/bin/env bash
# scripts/lib_main_checkout.sh — resolve the STABLE MAIN CHECKOUT root, for
# pinning host-global systemd --user unit ExecStart paths.
#
# Designed to be sourced, not executed directly.
#
# Usage:  source "$(dirname "${BASH_SOURCE[0]}")/lib_main_checkout.sh"
#   or:   source "$REPO_ROOT/scripts/lib_main_checkout.sh"
#
# FUNCTIONS (defined when sourced):
#   reify_main_checkout [ANCHOR_DIR]
#       Prints the absolute path of the MAIN worktree root on stdout and
#       returns 0; on failure prints NOTHING and returns non-zero.
#       ANCHOR_DIR defaults to this lib's own directory.
#
# KNOBS (environment variables):
#   REIFY_MAIN_CHECKOUT   if set and non-empty, used verbatim as the answer,
#                         short-circuiting the git derivation. Same override
#                         name (and same role) as in
#                         scripts/setup-agent-cache-redirect.sh:549-560.
#                         default: unset (derive from git)
#
# WHY THIS EXISTS
#   A host-global unit under $HOME/.config/systemd/user outlives whatever
#   checkout installed it. reify is developed across ~235 linked worktrees
#   (the warm-lane pool), and an installer that derives ExecStart from
#   ${BASH_SOURCE[0]} pins the unit at a lane path that vanishes the moment
#   that lane is reclaimed and re-seeded. Every subsequent start then fails
#   with status=203/EXEC, and because nothing Requires= these units the
#   failure is completely silent (task 5888; the identical defect class is
#   described in scripts/setup-agent-cache-redirect.sh:498-507).
#
#   The house convention is therefore that a host-global unit names the stable
#   MAIN-checkout absolute path — see the inline notes on
#   deploy/systemd/reify-warm-lane.service:15-18 and
#   deploy/systemd/reify-warm-lane-gc.service:5-8 (#4720). This lib is the
#   shared way to COMPUTE that path for a NEW pinning site.
#
# NOT YET THE ONLY WAY TO COMPUTE IT — a known, deliberate divergence.
#   scripts/setup-agent-cache-redirect.sh:554 still resolves the same concept
#   independently, as `"${REIFY_MAIN_CHECKOUT:-/home/leo/src/reify}"` — a
#   HARDCODED host default, where this lib DERIVES the answer from git. The two
#   agree on the reify host (the derivation answers /home/leo/src/reify there)
#   and on any host that exports REIFY_MAIN_CHECKOUT, but they disagree
#   SILENTLY on a contributor clone or a relocated tree: the redirect script
#   would pin its boot unit at a nonexistent /home/leo/src/reify and fall back
#   to $self, while this lib names the real main checkout.
#
#   That script was NOT migrated here because it is outside task 5888's module
#   scope (see the task's scope discipline), not because the divergence is
#   correct. Migrating install_boot_unit() to source this lib — keeping its
#   existing -x guard and its $self fallback verbatim — is tracked follow-up
#   work. Until it lands, do not describe this lib as the single source of
#   truth for the main-checkout path; it is the single source of truth for the
#   sites that source it.
#
#   PER-WORKTREE work must NOT use this: hooks wiring, the debug port, and
#   .cargo config are all correctly worktree-relative. Only host-global
#   artifacts get pinned.
#
# DELIBERATELY NO `set -euo pipefail`
#   Absent from all sibling scripts/lib_*.sh: a sourced lib must not impose
#   shell options on its caller. Functions `return`, never `exit`.

# Source guard — prevent double-sourcing.
if [ "${_REIFY_LIB_MAIN_CHECKOUT_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_LIB_MAIN_CHECKOUT_SH_SOURCED=1

# _reify_main_checkout_git <git args...> — `git` with the ambient repository
# environment stripped (trap (d) below). Internal; not part of the lib's API.
#
# `env -u`, NOT the shorter `GIT_DIR= GIT_WORK_TREE= git ...` prefix form: an
# EMPTY GIT_DIR is not the same as an unset one, and git 2.43 hard-fails it with
# `fatal: not a git repository: ''` (measured). The prefix form would therefore
# convert a working resolution into a total failure — fail-closed, so the
# ExecStart stays safe, but the pin would be lost on every hook-driven run.
_reify_main_checkout_git() {
    env -u GIT_DIR -u GIT_WORK_TREE -u GIT_COMMON_DIR -u GIT_INDEX_FILE \
        git "$@"
}

# reify_main_checkout [ANCHOR_DIR]
#
# Layered resolution, most-explicit first:
#   1. $REIFY_MAIN_CHECKOUT, when set and non-empty.
#   2. dirname of the ANCHOR's absolute git COMMON dir, validated (below).
#   3. failure: empty stdout, non-zero return.
#
# FOUR MEASURED TRAPS THIS CLOSES, none of them obvious:
#
#   (a) --path-format=absolute is REQUIRED. Bare `--git-common-dir` prints a
#       path RELATIVE to CWD inside the main checkout (`.git`, or `../../.git`
#       from a subdir) and an absolute one only inside a LINKED worktree. So
#       the natural `dirname "$(git rev-parse --git-common-dir)"` answers `.`
#       — silently wrong in precisely the main checkout it is meant to name.
#       (git >= 2.31; setup-dev.sh hard-requires Ubuntu 24.04, which ships
#       2.43.)
#
#   (b) `git -C "$anchor"` ANCHORING is required. git rev-parse resolves
#       against CWD, and the primary consumer (setup-dev.sh) never `cd`s, so
#       CWD is whatever directory the operator happened to invoke from. An
#       un-anchored derivation would answer a different repository depending
#       on where the installer was run.
#
#   (c) dirname is VALIDATED, not trusted. Parent-of-common-dir is not a
#       worktree root for a bare repo or a `--separate-git-dir` layout, so the
#       candidate must round-trip through --show-toplevel before it is
#       returned. Emitting a guessed path is worse than failing: it would be
#       interpolated straight into an ExecStart and yield a silently dead unit.
#
#   (d) The AMBIENT GIT ENVIRONMENT must be neutralized, or (b) is a lie.
#       GIT_DIR, GIT_WORK_TREE and GIT_COMMON_DIR all OUTRANK `git -C`:
#       measured on git 2.43, with an anchor inside worktree M and
#       GIT_DIR/GIT_WORK_TREE (or GIT_COMMON_DIR alone) pointing at an
#       unrelated repo O, `git -C M rev-parse --git-common-dir` answers O.
#       This is not exotic — git EXPORTS GIT_DIR into every hook, and reify
#       runs its whole verify gate from hooks/pre-merge-commit, so an
#       un-neutralized resolver silently answers for a different repository in
#       precisely the context that matters most. Every git invocation below
#       therefore goes through _reify_main_checkout_git.
#
# Callers are expected to treat a non-zero return as "could not resolve" and
# fall back deliberately (see setup-dev.sh's install_build_services(), which
# additionally requires the resolved tree to actually hold the executables
# before it will pin a unit at it).
reify_main_checkout() {
    local anchor_dir="${1:-}"
    local common_dir cand top

    if [ -n "${REIFY_MAIN_CHECKOUT:-}" ]; then
        printf '%s\n' "$REIFY_MAIN_CHECKOUT"
        return 0
    fi

    if [ -z "$anchor_dir" ]; then
        # This lib's own directory. Command substitution rather than a bare
        # dirname so a relative ${BASH_SOURCE[0]} still yields an absolute
        # anchor; a failure here falls through to the guard below.
        anchor_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" 2>/dev/null && pwd -P)" || anchor_dir=""
    fi
    [ -n "$anchor_dir" ] || return 1
    [ -d "$anchor_dir" ] || return 1

    common_dir="$(_reify_main_checkout_git -C "$anchor_dir" \
        rev-parse --path-format=absolute --git-common-dir 2>/dev/null)" || return 1
    [ -n "$common_dir" ] || return 1

    cand="$(dirname "$common_dir")"
    [ -n "$cand" ] || return 1
    [ -d "$cand" ] || return 1

    # Trap (c): only accept the candidate if it really is a worktree ROOT.
    top="$(_reify_main_checkout_git -C "$cand" \
        rev-parse --path-format=absolute --show-toplevel 2>/dev/null)" || return 1
    [ "$top" = "$cand" ] || return 1

    printf '%s\n' "$cand"
    return 0
}
