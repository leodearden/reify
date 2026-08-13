#!/usr/bin/env bash
# scripts/git-rerere-guard.sh — Keep git rerere disabled repo-wide, and detect
# the stuck-lane signature it leaves behind when it is not.
#
# WHY: `.git/rr-cache` is a git COMMON path — `git rev-parse --git-path rr-cache`
# resolves to the COMMON git dir from every linked worktree, while `MERGE_RR` and
# `index` resolve per-worktree.  Reify's ~238 warm lanes therefore share ONE
# rerere resolution cache, and git takes its only rerere lock on the PER-WORKTREE
# MERGE_RR — so that lock provides zero cross-worktree mutual exclusion over the
# shared payload directory.  Git exposes no knob to relocate rr-cache, so per-lane
# cache isolation is impossible by construction; the only sound fix is to disable
# rerere entirely.  Two hazards follow from leaving it on:
#
#   1. Silent cross-lane resolution bleed.  Lane A (task X) resolves a conflict
#      its way; lane B (unrelated task Y) later merges and git prints
#      `Staged '<f>' using previous resolution.` — lane B's tree now holds task
#      X's resolution, ALREADY STAGED under rerere.autoupdate=true, with no
#      conflict markers and a clean `git status`.
#   2. A failed rr-cache preimage write leaves a stale zero-byte MERGE_RR.lock,
#      after which every `git commit` in that lane exits 128 — while the commit
#      object is still written and the ref still moves.
#
# See docs/notes/git-rerere-shared-worktree-hazard.md (task 5870, esc-5785-5).
#
# Usage:
#   scripts/git-rerere-guard.sh <subcommand> [target_dir]
#
#   check       Report whether rerere is effectively ARMED for the target store.
#               Read-only: never writes config anywhere.
#               Exit 0 = safe, 1 = armed (the machine-readable signal).
#   arm         Idempotently write rerere.enabled=false and rerere.autoupdate=false
#               to the SHARED local config, then re-verify via `check`.
#               NEVER deletes or prunes rr-cache — see below.
#   scan-locks  Read-only census of stale MERGE_RR.lock files across the store's
#               linked worktrees.  Prints the remediation command; never runs it.
#               Exit 0 = clean, 1 = at least one lock found.
#
#   target_dir  Optional path to a git work tree inside the store to operate on.
#               Defaults to the repo root (one level up from this script).
#               Any worktree of the store resolves to the same shared config and
#               the same rr-cache, so the main checkout and any linked lane are
#               interchangeable here.
#
# Idempotent.  All diagnostics go to stderr; nothing is written to stdout.
#
# THE GOTCHA that makes a one-time `git config` write insufficient: git's default
# for `rerere.enabled` is -1, meaning "enabled iff rr-cache/ exists".  With the
# key UNSET and the residual rr-cache/ still on disk, rerere is silently ON for
# the whole fleet.  So the explicit `false` must be present, not merely absent —
# `git config --unset rerere.enabled` is a silent RE-ARM, not a no-op.  That is
# why this ships as a re-runnable guard with a `check` mode rather than a
# one-shot write, and why `arm` never prunes rr-cache: the explicit false
# neutralises the residual cache in place, while deleting an rr-cache entry that
# another live worktree still holds is precisely the operation that reproduces
# the segfault + stale-lock signature.

set -euo pipefail

_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() {
    cat >&2 <<'USAGE'
Usage: git-rerere-guard.sh <subcommand> [target_dir]

Subcommands:
  check       Report whether git rerere is effectively armed for the target
              store (read-only).  Exit 0 = safe, 1 = armed.
  arm         Idempotently disable rerere in the shared local config, then
              re-verify.  Never deletes rr-cache.
  scan-locks  Read-only census of stale MERGE_RR.lock files across the store's
              linked worktrees.  Exit 0 = clean, 1 = lock(s) found.

  target_dir  Optional git work tree inside the store; defaults to the repo
              root (one level up from this script).

All diagnostics go to stderr.  See
docs/notes/git-rerere-shared-worktree-hazard.md for the mechanism and the
recovery procedure.
USAGE
}

# ── argument parsing ──────────────────────────────────────────────────────────

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
    usage
    exit 0
fi

SUBCOMMAND="${1:-}"

if [ -z "$SUBCOMMAND" ]; then
    echo "ERROR: no subcommand given." >&2
    usage
    exit 1
fi

case "$SUBCOMMAND" in
    check|arm|scan-locks) ;;
    *)
        echo "ERROR: unknown subcommand: $SUBCOMMAND" >&2
        usage
        exit 1
        ;;
esac

shift

if [ $# -gt 1 ]; then
    echo "ERROR: too many arguments." >&2
    usage
    exit 1
fi

TARGET="${1:-"$(cd "$_SCRIPT_DIR/.." && pwd)"}"

if [ ! -d "$TARGET" ]; then
    echo "ERROR: target directory does not exist: $TARGET" >&2
    exit 1
fi

if ! git -C "$TARGET" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    echo "ERROR: not a git work tree: $TARGET" >&2
    exit 1
fi

# ── resolve the SHARED store ──────────────────────────────────────────────────
# --git-common-dir, not --git-dir: from a linked worktree --git-dir points at
# .git/worktrees/<name>/ (which holds the per-worktree MERGE_RR), while the
# shared config and rr-cache both live in the COMMON dir.  Resolving through the
# common dir is what makes this script behave identically whether it is invoked
# from the main checkout or from any of the ~238 lanes.

COMMON_DIR="$(git -C "$TARGET" rev-parse --git-common-dir)"
case "$COMMON_DIR" in
    /*) ;;
    *)  COMMON_DIR="$(cd "$TARGET" && cd "$COMMON_DIR" && pwd)" ;;
esac

RR_CACHE="$COMMON_DIR/rr-cache"
WORKTREES_DIR="$COMMON_DIR/worktrees"

# ── subcommand implementations ────────────────────────────────────────────────

# cmd_check — report whether rerere is effectively armed for the target store.
#
# Reads the EFFECTIVE value via `git config --bool --get <key>` (no --local), so
# the full precedence chain is honoured: a value inherited from the user's global
# gitconfig, or set in a worktree's config.worktree, is just as armed as one in
# the shared .git/config, and reading only --local would silently miss it.
#
# Exit code is the machine-readable signal (0 = safe, 1 = armed); stdout stays
# empty so a caller can use this in a pipeline without parsing prose.
cmd_check() {
    local armed=0 value

    # rerere.enabled — the master switch.
    #
    # "explicitly false" and "unset" must be distinguished by the EXIT STATUS of
    # `git config --get` (exit 1 = unset), not by the value: through --bool an
    # unset key and an explicit false both read as empty/false, yet they behave
    # oppositely.  git's default is -1 = "enabled iff rr-cache/ exists", so an
    # unset key with the residual cache on disk is ARMED.
    if ! git -C "$TARGET" config --get rerere.enabled >/dev/null 2>&1; then
        if [ -d "$RR_CACHE" ]; then
            echo "ARMED: rerere.enabled is UNSET and $RR_CACHE exists." >&2
            echo "  git's default for rerere.enabled is -1, meaning 'enabled iff rr-cache/ exists'," >&2
            echo "  so the residual rr-cache directory silently re-arms rerere for every worktree" >&2
            echo "  sharing this store.  An explicit 'false' is required; --unset is a re-arm." >&2
            armed=1
        else
            # Not armed today, but one conflicted merge in any lane creates
            # rr-cache/ and flips the -1 default.  Recommend, do not fail.
            echo "NOTE: rerere.enabled is UNSET and no rr-cache/ exists yet at $RR_CACHE." >&2
            echo "  Safe right now, but git's -1 default means the first conflicted merge in any" >&2
            echo "  worktree would create rr-cache/ and silently arm rerere.  Run 'arm' to pin it." >&2
        fi
    else
        value="$(git -C "$TARGET" config --bool --get rerere.enabled 2>/dev/null || true)"
        if [ "$value" = "true" ]; then
            echo "ARMED: rerere.enabled=true for $COMMON_DIR" >&2
            echo "  A resolution recorded by any lane can be replayed into an unrelated lane's merge." >&2
            armed=1
        fi
    fi

    # rerere.autoupdate — reported independently of rerere.enabled, not folded
    # into it.  autoupdate is what auto-STAGES a replayed resolution, turning a
    # visible conflict into a clean `git status` an agent will commit blind.
    value="$(git -C "$TARGET" config --bool --get rerere.autoupdate 2>/dev/null || true)"
    if [ "$value" = "true" ]; then
        echo "ARMED: rerere.autoupdate=true for $COMMON_DIR" >&2
        echo "  A replayed resolution is auto-staged, leaving no conflict markers and a clean git status." >&2
        armed=1
    fi

    # Per-worktree overrides.  Reading the effective value above only sees
    # $TARGET's own config.worktree, so a DIFFERENT lane's override would be
    # invisible from here — and it is precisely a foreign lane's armed rerere
    # that bleeds a resolution into this one.  Sweep them all.
    if ! _sweep_worktree_configs; then
        armed=1
    fi

    return "$armed"
}

# _sweep_worktree_configs — report any linked worktree whose config.worktree sets
# rerere.enabled or rerere.autoupdate to true.  Returns 1 if any is armed.
#
# Values are read with `git config --file ... --bool --get`, never grepped:
# comments, whitespace variations and section-header spellings would all produce
# a false verdict from a grep, in either direction.
_sweep_worktree_configs() {
    local armed=0 wt_config wt_name key value

    # A repo with no linked worktrees has no worktrees/ dir at all — normal, not
    # an error.
    [ -d "$WORKTREES_DIR" ] || return 0

    for wt_config in "$WORKTREES_DIR"/*/config.worktree; do
        # Unmatched glob under a worktrees/ dir with no config.worktree files.
        [ -e "$wt_config" ] || continue
        wt_name="$(basename "$(dirname "$wt_config")")"

        # One unreadable config.worktree must not abort the sweep and mask every
        # other lane — report it and keep going.
        if [ ! -r "$wt_config" ]; then
            echo "WARNING: cannot read $wt_config — skipping worktree '$wt_name'." >&2
            echo "  This lane's rerere state is UNKNOWN, not verified safe." >&2
            continue
        fi

        for key in rerere.enabled rerere.autoupdate; do
            value="$(git config --file "$wt_config" --bool --get "$key" 2>/dev/null || true)"
            if [ "$value" = "true" ]; then
                echo "ARMED: worktree '$wt_name' overrides $key=true in its config.worktree." >&2
                echo "  Path: $wt_config" >&2
                echo "  git reads config.worktree FIRST, so this beats the shared .git/config." >&2
                armed=1
            fi
        done
    done

    return "$armed"
}

# cmd_arm — idempotently pin rerere off in the SHARED local config, then
# re-verify via the full check logic.
#
# --local, NEVER --worktree: the entire point is that all ~238 lanes inherit one
# shared default with zero per-lane wiring.  A --worktree write would leave every
# other lane armed while this one reads clean.
#
# MUST NOT delete or prune rr-cache.  Deleting an entry another live worktree
# still holds is precisely the operation that reproduces the segfault + stale
# MERGE_RR.lock signature, so the cure would re-cause the disease across the
# fleet.  The explicit `false` neutralises the residual cache in place — that is
# measured behaviour, not an assumption (see the suite's behavioural oracles).
cmd_arm() {
    local changed=0 key before

    for key in rerere.enabled rerere.autoupdate; do
        # Compare against the current LOCAL value and skip a redundant write, so
        # a re-run is a byte-level no-op on .git/config.  setup-dev.sh invokes
        # this every time; a rewrite would churn the shared file for nothing.
        before="$(git -C "$TARGET" config --local --get "$key" 2>/dev/null || true)"
        if [ "$before" = "false" ]; then
            continue
        fi
        git -C "$TARGET" config --local "$key" false
        echo "SET: $key=false (was ${before:-<unset>}) in $COMMON_DIR/config" >&2
        changed=1
    done

    if [ "$changed" -eq 0 ]; then
        echo "already armed: rerere.enabled=false, rerere.autoupdate=false in $COMMON_DIR/config" >&2
    fi

    if [ -d "$RR_CACHE" ]; then
        echo "note: $RR_CACHE left intact by design — the explicit 'false' neutralises it in place." >&2
        echo "  Deleting an entry another live worktree still holds is what reproduces the" >&2
        echo "  segfault + stale MERGE_RR.lock signature; never prune it to 'clean up'." >&2
    fi

    # Re-verify through the SAME logic `check` uses, so `arm` can never report
    # success while a per-worktree override or an inherited global value still
    # wins over the shared write just made.
    if ! cmd_check; then
        echo "ERROR: rerere is STILL armed after writing the shared config." >&2
        echo "  A per-worktree config.worktree or the user's global gitconfig is overriding it;" >&2
        echo "  see the ARMED lines above for which." >&2
        return 1
    fi

    return 0
}

cmd_scan_locks() {
    echo "ERROR: scan-locks is not implemented yet" >&2
    return 1
}

case "$SUBCOMMAND" in
    check)      cmd_check ;;
    arm)        cmd_arm ;;
    scan-locks) cmd_scan_locks ;;
esac
