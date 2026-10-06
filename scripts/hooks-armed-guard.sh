#!/usr/bin/env bash
# scripts/hooks-armed-guard.sh — Does git actually run hooks/reference-transaction's
# refs/stash arm (task 5981) in a given worktree?
#
# WHY: that arm only protects a worktree whose EFFECTIVE core.hooksPath reaches
# reify's hooks/ dir.  Claude Code's worktree feature rewrites the SHARED
# core.hooksPath to git's inert .git/hooks samples on every worktree enter, which
# silently darkens the guard in every worktree that relies on the shared value.
# Nothing fails when that happens; the guard just stops firing (task 6059).
#
# ARMED = LIVE and PINNED:
#   LIVE    git's own resolution of the worktree's hooks dir
#           (`rev-parse --git-path hooks`) holds an executable
#           reference-transaction that, run the way git runs it for a
#           `git stash push` (state `prepared`, a refs/stash create on stdin)
#           with REIFY_STASH_GUARD_ENFORCE=1 and REIFY_STASH_GUARD_BYPASS=0,
#           REFUSES.  A file that merely exists is not enough: a pre-5981 hook,
#           or one whose lib failed to source, exits 0 and reads as protection.
#           The probe runs inside a throwaway scratch repo, so the hook's audit
#           write never reaches the store's shared main-gate log, and under a
#           fixed timeout; a timeout counts as not live.
#   PINNED  extensions.worktreeConfig is on and core.hooksPath is set in THIS
#           worktree's config.worktree.  The shared value is exactly what the
#           clobber rewrites, so a worktree resting on it can go dark mid-session;
#           a per-worktree pin outranks it for the life of the worktree.
#
# Usage:
#   scripts/hooks-armed-guard.sh <subcommand> [target_dir]
#
#   check       Report whether the target worktree is ARMED.  Read-only: writes
#               no config and no log anywhere.
#   arm         If not armed, pin core.hooksPath for the target worktree by
#               delegating to scripts/setup-main-gate-worktree-config.sh, then
#               re-verify.  Writes nothing when already armed.
#
#   target_dir  Optional path inside the git work tree to operate on.  Defaults
#               to the repo root (one level up from this script).
#
# EXIT CONTRACT (normative; stated once, here):
#   check  0 = armed
#          1 = not armed (stderr names the toplevel, the resolved hooks dir,
#              each unmet condition and the remediation)
#          3 = could not check (usage error, the target is not a git work
#              tree, or the liveness probe could not run — e.g. mktemp or the
#              `git init` of its scratch repo failed; stderr names the step)
#   arm    0 = armed (already, or re-armed by this run)
#          2 = the pin cannot fix it (pinned, but the hook itself does not gate)
#          ANY OTHER NON-ZERO = this run failed.  Branch on `0 | 2 | *`, never on
#          a closed set.
#
# CALLERS:
#   scripts/seed-warm-lane.sh --fresh-checkout   LANE cadence: `arm` on every
#     warm-lane acquire, fail-open.  Its gating rules live there.
#   Operators: `check` to confirm a lane before trusting the guard in it.
#
# COVERAGE LIMIT: "armed" means a `git stash push` in that worktree fires the
# guard.  `git stash pop` and a non-last `git stash drop` fire no
# reference-transaction hook at all, so they are never covered — the hook's own
# header and tests/infra/test_stash_guard.sh are authoritative on that.  And
# "live" is not "refusing": whether a fired guard warns or refuses is the
# separate warn/enforce rollout switch in hooks/main-gate-lib.sh.
#
# All diagnostics go to stderr; stdout stays empty, so the exit code is the
# machine-readable signal.

set -euo pipefail

_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# target_dir is the authority: an inherited GIT_DIR / GIT_INDEX_FILE would
# otherwise outrank `git -C`.
# shellcheck source=scripts/lib_git_env_scrub.sh
. "$_SCRIPT_DIR/lib_git_env_scrub.sh"
for _var in $REIFY_GIT_ENV_SCRUB_VARS; do
    unset "$_var"
done

readonly PROBE_TIMEOUT_SECS=10
readonly PROBE_KILL_AFTER_SECS=2
readonly ZERO_OID=0000000000000000000000000000000000000000
readonly PROBE_OID=1111111111111111111111111111111111111111

usage() {
    cat >&2 <<'USAGE'
Usage: hooks-armed-guard.sh <subcommand> [target_dir]

Subcommands:
  check       Report whether git runs hooks/reference-transaction's refs/stash
              guard in the target worktree, pinned per-worktree.  Read-only.
                                        [0 armed | 1 not armed | 3 could not check]
  arm         If not armed, pin core.hooksPath for the target worktree (via
              scripts/setup-main-gate-worktree-config.sh), then re-verify.
              Writes nothing when already armed.
                                        [0 armed | 2 the pin cannot fix it | * failed]

  target_dir  Optional path inside a git work tree; defaults to the repo root
              (one level up from this script).

All diagnostics go to stderr; stdout stays empty.  The CALLER CONTRACT is
NORMATIVE in the header comment block of scripts/hooks-armed-guard.sh.
USAGE
}

# _toplevel <target> — the absolute, existing toplevel of target's work tree.
_toplevel() {
    local top
    top="$(git -C "$1" rev-parse --path-format=absolute --show-toplevel 2>/dev/null)" || return 1
    case "$top" in
        /*) ;;
        *) return 1 ;;
    esac
    [ -d "$top" ] || return 1
    printf '%s\n' "$top"
}

# _probe_refuses <hook> — does <hook>, run as git runs it for a refs/stash
# create, refuse under ENFORCE?  Runs in a scratch repo, removed on every path.
# Returns 0 refuses | 1 does not refuse (exits 0, or times out) | 2 the probe
# could not run (stderr names the step), which says nothing about the hook.
_probe_refuses() {
    local hook="$1" scratch rc=0
    scratch="$(mktemp -d "${TMPDIR:-/tmp}/hooks-armed-probe.XXXXXX")" || {
        echo "[error] hooks-armed-guard: cannot run the liveness probe: mktemp -d under ${TMPDIR:-/tmp} failed" >&2
        return 2
    }
    if ! git init -q "$scratch" >/dev/null 2>&1; then
        echo "[error] hooks-armed-guard: cannot run the liveness probe: git init of its scratch repo $scratch failed" >&2
        rm -rf "$scratch"
        return 2
    fi
    ( cd "$scratch" \
        && REIFY_STASH_GUARD_ENFORCE=1 REIFY_STASH_GUARD_BYPASS=0 \
           timeout -k "$PROBE_KILL_AFTER_SECS" "$PROBE_TIMEOUT_SECS" "$hook" prepared \
           <<<"$ZERO_OID $PROBE_OID refs/stash" >/dev/null 2>&1 ) || rc=$?
    rm -rf "$scratch"
    case "$rc" in
        0|124|137) return 1 ;;
        *) return 0 ;;
    esac
}

# _pinned — is core.hooksPath set in $TOP's own config.worktree?  The extension
# is read first: `config --worktree` aborts when it is off and the store has
# several worktrees.
_pinned() {
    local ext pin
    ext="$(git -C "$TOP" config --local --bool --get extensions.worktreeConfig 2>/dev/null || true)"
    [ "$ext" = "true" ] || return 1
    pin="$(git -C "$TOP" config --worktree --get core.hooksPath 2>/dev/null || true)"
    [ -n "$pin" ]
}

_unmet() {
    UNMET+=("$1")
}

# _assess — resolve HOOKS_DIR for $TOP and collect every unmet ARMED condition
# into UNMET.  Returns 0 armed | 1 not armed | 3 could not check (the hooks dir
# does not resolve, or the liveness probe could not run).
_assess() {
    local hook probe_rc=0
    UNMET=()
    HOOKS_DIR="$(git -C "$TOP" rev-parse --path-format=absolute --git-path hooks 2>/dev/null)" || {
        echo "[error] hooks-armed-guard: cannot resolve the hooks dir of $TOP" >&2
        return 3
    }
    hook="$HOOKS_DIR/reference-transaction"
    if [ ! -d "$HOOKS_DIR" ]; then
        _unmet "the effective hooks dir does not exist"
    elif [ ! -f "$hook" ]; then
        _unmet "the effective hooks dir holds no reference-transaction hook"
    elif [ ! -x "$hook" ]; then
        _unmet "reference-transaction is not executable, so git skips it"
    else
        _probe_refuses "$hook" || probe_rc=$?
        case "$probe_rc" in
            0) ;;
            1) _unmet "reference-transaction does not refuse a refs/stash push under REIFY_STASH_GUARD_ENFORCE=1" ;;
            *) return 3 ;;
        esac
    fi
    _pinned || _unmet "core.hooksPath is not pinned in this worktree's config.worktree, so it rests on the shared value Claude Code's worktree feature rewrites"
    [ "${#UNMET[@]}" -eq 0 ]
}

# _verdict — _assess, reported on stderr.  Returns _assess's code.
_verdict() {
    local rc=0 reason
    _assess || rc=$?
    case "$rc" in
        0)
            echo "[ok] hooks-armed-guard: stash guard ARMED in $TOP (hooks dir $HOOKS_DIR)" >&2
            ;;
        1)
            for reason in "${UNMET[@]}"; do
                echo "[warn] hooks-armed-guard: NOT armed in $TOP (hooks dir $HOOKS_DIR): $reason" >&2
            done
            ;;
    esac
    return "$rc"
}

# _liveness_log MSG — append a `stash-guard: liveness:` line to the target
# store's shared main-gate audit log.  The cwd is $TOP so main_gate_log resolves
# THAT store's common dir.  Degrades to a stderr line if the lib is absent.
_liveness_log() {
    local msg="stash-guard: liveness: $1"
    local lib="$_SCRIPT_DIR/../hooks/main-gate-lib.sh"
    if [ -r "$lib" ]; then
        # shellcheck source=hooks/main-gate-lib.sh
        ( cd "$TOP" && . "$lib" && main_gate_log "$msg" ) \
            || echo "[warn] hooks-armed-guard: $msg (main-gate log write failed)" >&2
    else
        echo "[warn] hooks-armed-guard: $msg (hooks/main-gate-lib.sh absent; not logged)" >&2
    fi
}

cmd_check() {
    local rc=0
    _verdict || rc=$?
    if [ "$rc" -eq 1 ]; then
        echo "[warn] hooks-armed-guard: remediation: scripts/hooks-armed-guard.sh arm $TOP" \
             "(re-pins core.hooksPath; a hook that fails the probe must be restored in $HOOKS_DIR)" >&2
    fi
    return "$rc"
}

cmd_arm() {
    local rc=0
    _verdict || rc=$?
    case "$rc" in
        0) return 0 ;;
        1) ;;
        *) return "$rc" ;;
    esac

    if ! "$_SCRIPT_DIR/setup-main-gate-worktree-config.sh" "$TOP" >&2; then
        echo "[error] hooks-armed-guard: setup-main-gate-worktree-config.sh failed to pin $TOP" >&2
        return 1
    fi

    rc=0
    _verdict || rc=$?
    case "$rc" in
        0)
            _liveness_log "re-armed $TOP (hooks dir $HOOKS_DIR)"
            return 0
            ;;
        1)
            _liveness_log "still DARK in $TOP after arm: ${UNMET[0]}"
            echo "[warn] hooks-armed-guard: the pin cannot fix this; restore" \
                 "reference-transaction in $HOOKS_DIR" >&2
            return 2
            ;;
        *)
            return "$rc"
            ;;
    esac
}

# ── argument parsing ──────────────────────────────────────────────────────────

case "${1:-}" in
    -h|--help)
        usage
        exit 0
        ;;
    check|arm)
        ;;
    *)
        echo "[error] hooks-armed-guard: unknown or missing subcommand: ${1:-<none>}" >&2
        usage
        exit 3
        ;;
esac
SUBCOMMAND="$1"
shift

if [ $# -gt 1 ]; then
    echo "[error] hooks-armed-guard: too many arguments." >&2
    usage
    exit 3
fi

TARGET="${1:-"$(cd "$_SCRIPT_DIR/.." && pwd)"}"
TOP="$(_toplevel "$TARGET")" || {
    echo "[error] hooks-armed-guard: not a git work tree: $TARGET" >&2
    exit 3
}
HOOKS_DIR=""
UNMET=()

case "$SUBCOMMAND" in
    check) cmd_check ;;
    arm)   cmd_arm ;;
esac
