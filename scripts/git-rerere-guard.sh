#!/usr/bin/env bash
# scripts/git-rerere-guard.sh — Keep git rerere disabled repo-wide, and detect
# the stuck-lane signature it leaves behind when it is not.
#
# WHY: `.git/rr-cache` is a git COMMON path — `git rev-parse --git-path rr-cache`
# resolves to the COMMON git dir from every linked worktree, while `MERGE_RR` and
# `index` resolve per-worktree.  Every warm lane of reify's shared store therefore
# shares ONE rerere resolution cache, and git takes its only rerere lock on the
# PER-WORKTREE MERGE_RR — so that lock provides zero cross-worktree mutual
# exclusion over the shared payload directory.  Git exposes no knob to relocate
# rr-cache, so per-lane cache isolation is impossible by construction; the only
# sound fix is to disable rerere entirely.  Two hazards follow from leaving it on:
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
#   check       Report whether rerere is effectively ARMED for the target store —
#               the effective rerere.enabled/rerere.autoupdate, plus a sweep of
#               every config.worktree in the store (the main checkout's own
#               included, since git dir == common dir there).
#               Read-only: never writes config anywhere.
#               Exit 0 = safe AND every scope was actually verified;
#                    1 = armed (the machine-readable signal);
#                    3 = UNVERIFIABLE — no armed scope was found, but at least
#                        one worktree's rerere state could not be determined at
#                        all (an unreadable config.worktree, an include.path
#                        chain git cannot resolve, or an unreadable `gitdir`
#                        that leaves the entry's liveness unknown).  This is
#                        deliberately NOT 0:
#                        a periodic probe that read an unverifiable store as
#                        clean would report the fleet healthy while a lane it
#                        could not read was armed.  It is deliberately NOT 1
#                        either — `arm` writes --local and cannot fix a lane the
#                        guard merely fails to read, so folding UNKNOWN into
#                        ARMED would strand the store on a permanent failure.
#               ARMED WINS over UNVERIFIABLE: a store that is both exits 1,
#               because that is the half an operator can act on.  Treat any
#               non-zero as "not verified safe"; only 1 means "armed".
#   arm         Idempotently write rerere.enabled=false and rerere.autoupdate=false
#               to the SHARED local config, then re-verify via `check`.
#               NEVER deletes or prunes rr-cache — see below.
#               Exit 0 = disarmed and verified;
#                    2 = shared config pinned, but an override this run cannot
#                        reach still wins (another lane's config.worktree, or
#                        the user's global gitconfig) — advisory, not fatal;
#                    ANY OTHER NON-ZERO = a failure of this run.  Normally 1,
#                        but the script runs under `set -euo pipefail`, so a git
#                        invocation that aborts outside a guarded `if` (a lost
#                        race on .git/config.lock, a read-only store) propagates
#                        git's own status instead.  Branch on `0 | 2 | *`, never
#                        on a closed set {0,1,2}.
#   scan-locks  Read-only census of stale MERGE_RR.lock files across the WHOLE
#               store — the main checkout's own git dir (where git dir == common
#               dir, so its lock lands at <common>/MERGE_RR.lock rather than
#               under worktrees/) AND every linked worktree.  Prints the
#               remediation command; never runs it.
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
#
# CALLERS — two cadences, both invoking `arm`:
#
#   scripts/setup-dev.sh                      DEVELOPER cadence.  Pins the store
#     when a developer runs setup.  Aborts setup on an unexpected non-zero,
#     because a developer can act on it immediately.
#
#   scripts/seed-warm-lane.sh --fresh-checkout   LANE cadence (task 6889).  Pins
#     the store on EVERY warm-lane ACQUIRE.  It exists because setup-dev's
#     cadence leaves the shared config unpinned for as long as a developer goes
#     without re-running setup — measured on the live store, the config was found
#     ARMED twice on a single day.  Fail-OPEN: an acquire must never fail because
#     the shared store could not be pinned, so seed warns and continues on any
#     non-zero.  Its gating rules (mode gate, existence gate) and its
#     REIFY_WARM_LANE_RERERE_ARM=0 escape hatch live THERE, not here — this guard
#     stays a caller-agnostic primitive.
#
# Lane cadence NARROWS the re-arm window; it does not close it.  The re-armer is
# agent behaviour, and no `arm` cadence outruns an agent re-running the write —
# see docs/notes/git-rerere-shared-worktree-hazard.md §7.

set -euo pipefail

_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() {
    cat >&2 <<'USAGE'
Usage: git-rerere-guard.sh <subcommand> [target_dir]

Subcommands:
  check       Report whether git rerere is effectively armed for the target
              store.  Read-only.        [0 safe+verified | 1 armed | 3 UNKNOWN]
  arm         Idempotently disable rerere in the shared local config, then
              re-verify.  Never deletes rr-cache.
                                        [0 disarmed | 2 advisory | * failed]
  scan-locks  Read-only census of stale MERGE_RR.lock files across the whole
              store — the main checkout and every linked worktree.
                                        [0 clean | 1 lock(s) found]

  target_dir  Optional git work tree inside the store; defaults to the repo
              root (one level up from this script).

All diagnostics go to stderr; stdout stays empty, so the exit code is the
machine-readable signal.

CALLER CONTRACT — what each code means and how to branch on it (notably: treat
any non-zero from `check` as "not verified safe", and branch `arm` on 0 | 2 | *,
never on a closed set) is NORMATIVE in the header comment block of this file,
scripts/git-rerere-guard.sh.  It is stated once, there, on purpose.

Mechanism, recovery procedure and incident history:
docs/notes/git-rerere-shared-worktree-hazard.md
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
# from the main checkout or from any lane of the store.

COMMON_DIR="$(git -C "$TARGET" rev-parse --git-common-dir)"
case "$COMMON_DIR" in
    /*) ;;
    *)  COMMON_DIR="$(cd "$TARGET" && cd "$COMMON_DIR" && pwd)" ;;
esac

RR_CACHE="$COMMON_DIR/rr-cache"
WORKTREES_DIR="$COMMON_DIR/worktrees"

# A literal newline, for the sweep's fork-free prefilter membership test.
LF=$'\n'

# Count of worktrees whose rerere state the sweep could not determine at all.
# Set by _sweep_worktree_configs, read by cmd_check, which turns a non-zero
# count into exit 3 (UNVERIFIABLE) rather than letting it pass as 0 (safe).
# Script-level, not local: the sweep returns 0/1 for the ARMED verdict, and
# UNKNOWN is a third, orthogonal state that must not be smuggled into that
# boolean.  Initialised here because the script runs under `set -u`.
_SWEEP_UNKNOWN=0

# ── subcommand implementations ────────────────────────────────────────────────

# cmd_check — report whether rerere is effectively armed for the target store.
#
# Reads the EFFECTIVE value via `git config --bool --get <key>` (no --local), so
# the full precedence chain is honoured: a value inherited from the user's global
# gitconfig, or set in a worktree's config.worktree, is just as armed as one in
# the shared .git/config, and reading only --local would silently miss it.
#
# The effective read ALONE is not sufficient, though, and the gap is the exact
# mirror image of the main-checkout sweep blind spot below.  $TARGET's own
# config.worktree beats the shared config, so a lane that disarms ITSELF reads
# clean while every other lane of the store still inherits an armed shared
# default.  So the shared (--local) value is read explicitly too, and reported
# whenever the target's own config merely masks it — that shared value is the
# fleet-wide default this guard exists to pin, and `arm` (a --local writer) can
# always clear it, so reporting it is both actionable and self-healing.
#
# Exit code is the machine-readable signal (0 = safe and fully verified,
# 1 = armed, 3 = at least one lane UNVERIFIABLE); stdout stays empty so a caller
# can use this in a pipeline without parsing prose.
cmd_check() {
    local armed=0 value

    # Reset before every run: cmd_arm calls cmd_check a second time to
    # re-verify, and a count carried over from the first call would make the
    # re-verify report lanes it never re-examined.
    _SWEEP_UNKNOWN=0

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
        elif ! _check_shared_default rerere.enabled; then
            # Effectively false HERE, but only because $TARGET's own config
            # masks an armed shared default that every other lane still gets.
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
    elif ! _check_shared_default rerere.autoupdate; then
        armed=1
    fi

    # Per-worktree overrides.  Reading the effective value above only sees
    # $TARGET's own config.worktree, so a DIFFERENT lane's override would be
    # invisible from here — and it is precisely a foreign lane's armed rerere
    # that bleeds a resolution into this one.  Sweep them all.
    if ! _sweep_worktree_configs; then
        armed=1
    fi

    # ARMED WINS over UNVERIFIABLE.  A store that is both is reported as armed:
    # that is the half an operator can act on, and `arm` needs the 1 to know its
    # re-verify found a surviving override.
    if [ "$armed" -ne 0 ]; then
        return 1
    fi

    # UNVERIFIABLE is a THIRD state, not a flavour of safe.  Every UNKNOWN lane
    # already printed a WARNING above, but stderr prose is not the contract —
    # the exit code is, and it was the ONLY machine-readable channel.  Reporting
    # 0 here made `check` fail-open for exactly the use the runbook proposes for
    # it (a periodic probe for the unidentified re-armer): a store with one lane
    # the guard could not read at all would answer "safe" while that lane was
    # armed through an include chain.  3, not 1: see the header — `arm` cannot
    # clear a lane it cannot read, so 1 would strand the store on a permanent
    # failure.
    if [ "$_SWEEP_UNKNOWN" -gt 0 ]; then
        echo "UNVERIFIABLE: $_SWEEP_UNKNOWN worktree(s) could not be checked — see the WARNINGs above." >&2
        echo "  No armed scope was found, but this store is NOT verified safe: exit 3, distinct" >&2
        echo "  from 0 so a probe cannot read an unverifiable store as clean." >&2
        return 3
    fi

    return 0
}

# _inherited_value KEY — the value a lane with NO --local and NO per-worktree
# override actually inherits: the user's --global gitconfig if it sets KEY, else
# --system.  Prints `true`, `false`, or NOTHING when neither scope sets it — the
# three answers _check_shared_default's unset branch has to tell apart.
#
# LAST-WINS within a scope, resolved the way git itself resolves a multi-valued
# key, for the same reason the sweep does it: --get-all lists every value a
# scope holds while git honours only the final one.  --includes EXPLICITLY,
# because naming a scope turns include-following off.  Both reads are guarded
# with `|| true`: under `set -euo pipefail` an unreadable ~/.gitconfig, or
# --system under GIT_CONFIG_NOSYSTEM, must degrade to "this scope says nothing"
# rather than abort a read-only probe.
_inherited_value() {
    local key="$1" scope out
    for scope in --global --system; do
        out="$(git -C "$TARGET" config "$scope" --includes --bool --get-all "$key" 2>/dev/null || true)"
        if [ -n "$out" ]; then
            printf '%s\n' "${out##*$LF}"
            return 0
        fi
    done
    return 0
}

# _check_shared_default KEY — report an armed SHARED (--local) value that
# $TARGET's own config.worktree masks.  Returns 1 if the fleet-wide default is
# armed, 0 otherwise.  Called ONLY from the effective-read's not-armed branch, so
# a store that is armed outright is never reported twice for the same key.
#
# --local, deliberately: this asks "what does a lane with NO local override
# inherit?", which is precisely what the shared write in `arm` controls.  A false
# verdict here is harmless in the way that matters — `arm` writes --local, so it
# can always clear whatever this reports, unlike the inert-config.worktree case
# the sweep below has to gate against.
#
# --includes EXPLICITLY on both reads: git turns include-following OFF whenever a
# specific scope is named and ON only for an effective read, so without the flag
# a shared config that reaches rerere.enabled=true through an `include.path`
# reads as UNSET here while git's own resolution honours it (measured, git
# 2.43.0: `--local --get` exit 1, `--local --includes --get` -> true).
_check_shared_default() {
    local key="$1" shared inherited

    if ! git -C "$TARGET" config --local --includes --get "$key" >/dev/null 2>&1; then
        # UNSET in the shared config is NOT the same as "git falls back to its
        # built-in default".  Precedence is system < global < local < worktree,
        # so what a lane with no local pin ACTUALLY inherits is the user's
        # ~/.gitconfig or /etc/gitconfig value when either sets the key, and the
        # -1 fallback is never reached.  Reading only --local here got that
        # backwards in both directions (measured, git 2.43.0).
        inherited="$(_inherited_value "$key")"

        if [ "$inherited" = "true" ]; then
            # Reported EVEN THOUGH the effective read at $TARGET came back not
            # armed: only $TARGET's own config.worktree masks it, and every
            # other lane of the store inherits the true.  `arm` fixes it — a
            # --local write beats global and system.
            echo "ARMED: $key=true is inherited from the user's global/system gitconfig." >&2
            echo "  The SHARED config leaves it unset, so every lane of $COMMON_DIR without" >&2
            echo "  its own override reads true; $TARGET reads disarmed only because its own" >&2
            echo "  config.worktree masks it.  Run 'arm' — a --local pin beats global." >&2
            return 1
        fi

        if [ "$inherited" = "false" ]; then
            # GENUINELY SAFE — git never reaches the -1 fallback, so no lane of
            # this store is armed and an exit 1 here would be a false positive
            # with a diagnostic naming a scope that has nothing to do with it
            # (the old text asserted it was $TARGET's OWN config).  But the
            # store is UNPINNED: the disarm lives in a file outside it, so it
            # does not travel with the repo and one `git config --global
            # --unset` re-arms the whole fleet.  NOTE + exit 0, the same
            # safe-but-recommend shape cmd_check uses for "unset and no
            # rr-cache yet".  Only for rerere.enabled: an inherited
            # autoupdate=false is byte-for-byte git's own default, so saying
            # anything about it would be pure noise.
            if [ "$key" = "rerere.enabled" ]; then
                echo "NOTE: the SHARED config leaves rerere.enabled unset; this store is disarmed" >&2
                echo "  only by an inherited global/system gitconfig.  Safe right now, but that" >&2
                echo "  file is outside the store, so the disarm does not travel with it and a" >&2
                echo "  single 'git config --global --unset' re-arms every lane.  Run 'arm' to pin" >&2
                echo "  rerere.enabled=false locally." >&2
            fi
            return 0
        fi

        # Unset at EVERY scope.  Only rerere.enabled has a non-false default:
        # -1 = "enabled iff rr-cache/ exists".  rerere.autoupdate defaults to
        # false, so an unset shared autoupdate is genuinely safe.
        if [ "$key" = "rerere.enabled" ] && [ -d "$RR_CACHE" ]; then
            echo "ARMED: the SHARED config leaves rerere.enabled UNSET and $RR_CACHE exists." >&2
            echo "  $TARGET reads disarmed only because its own config overrides the shared" >&2
            echo "  default; every lane WITHOUT such an override inherits git's -1 default" >&2
            echo "  ('enabled iff rr-cache/ exists') and is armed.  Run 'arm' to pin it." >&2
            return 1
        fi
        return 0
    fi

    shared="$(git -C "$TARGET" config --local --includes --bool --get "$key" 2>/dev/null || true)"
    if [ "$shared" = "true" ]; then
        echo "ARMED: the SHARED config sets $key=true in $COMMON_DIR/config." >&2
        echo "  $TARGET reads disarmed only because its own config.worktree masks it — every" >&2
        echo "  other worktree of this store still inherits the armed shared default." >&2
        return 1
    fi

    return 0
}

# _sweep_worktree_configs — report any worktree whose config.worktree sets
# rerere.enabled or rerere.autoupdate to true.  Returns 1 if any is armed.
#
# Sweeps the MAIN CHECKOUT as well as every linked worktree.  For the main
# checkout git dir == common dir, so its per-worktree config lands at
# <common>/config.worktree — beside the shared config, NEVER under worktrees/ —
# and a glob over the linked worktrees alone is blind to it.  That blind spot is
# doubly silent, because the effective-value read in cmd_check only ever sees
# $TARGET's OWN config.worktree: from any lane, a main-checkout self-arm would be
# invisible to both paths at once, and `arm` would report "disarmed and verified"
# while main stayed armed.  The main checkout is also an ACTIVE merge site — the
# sanctioned landing path (scripts/land.sh) runs a real `git merge --no-ff` there
# — which is the same reasoning that makes cmd_scan_locks cover
# <common>/MERGE_RR.lock.
#
# Values are read with `git config --file ... --bool --get-regexp`, never
# grepped: comments, whitespace variations, valueless keys and non-canonical
# booleans (`yes`, `on`, `1`) would all produce a false verdict from a grep, in
# either direction.  grep is used only as a PREFILTER — one fork for the whole
# store, deciding which files are worth parsing, never deciding a value.
_report_armed_worktree() {
    local wt_name="$1" key="$2" wt_config="$3"

    echo "ARMED: worktree '$wt_name' overrides $key=true in its config.worktree." >&2
    echo "  Path: $wt_config" >&2
    echo "  git reads config.worktree FIRST, so this beats the shared .git/config." >&2
}

# _linked_worktree_is_live WT_DIR — TRI-STATE, not a boolean:
#   0  the admin dir still backs a working tree that exists on disk (LIVE);
#   1  the entry is STALE (git's own term: prunable) — inert, skip silently;
#   2  UNVERIFIABLE — the `gitdir` file EXISTS but could not be read, so
#      liveness is unknown and the caller must count the lane UNKNOWN.
#
# 1 and 2 were once the same answer, and that made the sweep fail OPEN on its
# only machine-readable channel: an unreadable `gitdir` in a live, armed lane
# made the caller `continue` silently, the lane vanished from the sweep, and
# `check` answered 0 = "safe AND fully verified".  Measured on git 2.43.0 —
# with a lane's config.worktree setting rerere.enabled=true, `check` exited 1
# and named it; after `chmod 000` on that lane's `gitdir`, `check` exited 0
# printing nothing while `git -C <lane> config --get rerere.enabled` still
# answered true.  ABSENT is genuinely prunable and stays a silent skip;
# UNREADABLE is a permissions pathology and is now UNKNOWN, the same verdict an
# unreadable config.worktree and an unresolvable include chain already get.
#
# WHY: the sweep walks <common>/worktrees/*/config.worktree straight off disk,
# and a stale entry keeps its config.worktree long after the working tree it
# describes was deleted — `git worktree prune` has to run before the admin dir
# goes away, and reify's pool creates and destroys lanes continuously, so a
# prunable entry is a realistic transient rather than a pathology.  git never
# reads that file again for any live worktree, so reporting it is an INERT-CONFIG
# false positive of exactly the class the extensions.worktreeConfig gate already
# guards against — and the same uniquely damaging shape, because `arm` writes
# --local and can never clear a per-worktree file: the store would park on the
# advisory exit 2 and setup-dev.sh would warn on every developer setup until an
# operator pruned by hand.  Measured on git 2.43.0: deleting a lane's working
# directory leaves worktrees/<name>/config.worktree intact, `git worktree list
# --porcelain` marks the entry `prunable gitdir file points to non-existent
# location`, and the guard reported `ARMED: worktree '<name>'` for it.
#
# The test is the same one git's own prune uses — does the path named by the
# entry's `gitdir` file still exist — read with a bash redirect rather than
# `git worktree list --porcelain`, which would cost a fork per sweep and then
# need a reverse mapping from worktree path back to admin-dir name (they are not
# 1:1: git de-duplicates names).
#
# DELIBERATELY NOT gated on `locked`: git refuses to prune a locked entry, but
# lockedness does not make an absent working tree readable, and this asks "can
# any git command run there and honour this file?", not "will prune reclaim it?".
# A locked worktree on a detached removable device is therefore skipped while it
# is absent and reported again once it is back — `check` is re-run, not one-shot.
_linked_worktree_is_live() {
    local wt_dir="$1" pointer=""

    # ABSENT gitdir file — git calls this prunable too ("gitdir file does not
    # exist").  The routine lane-churn transient: silent skip.
    [ -e "$wt_dir/gitdir" ] || return 1

    # PRESENT BUT UNREADABLE is a different animal, and must NOT collapse into
    # the line above: nothing about a permission bit makes the entry prunable,
    # so this may well be a live lane whose config.worktree is armed.  UNKNOWN.
    [ -r "$wt_dir/gitdir" ] || return 2

    # The open can still fail on a shape `-r` accepts (a dangling symlink, an
    # I/O error), so the read's status is captured rather than discarded with
    # `|| true`, and 2>/dev/null keeps bash's own redirect diagnostic out of the
    # guard's stderr.  `read` also exits non-zero at EOF on a file with no
    # trailing newline, so the verdict is taken from $pointer, not the status —
    # and an empty $pointer is disambiguated by -s: no bytes at all is git's
    # other prunable shape (stale, 1), bytes we could not get is UNKNOWN (2).
    if ! IFS= read -r pointer < "$wt_dir/gitdir" 2>/dev/null && [ -z "$pointer" ]; then
        [ -s "$wt_dir/gitdir" ] && return 2
        return 1
    fi
    [ -n "$pointer" ] || return 1

    # The file names the worktree's own .git FILE, e.g. /path/to/lane/.git, and
    # git always writes it ABSOLUTE (strbuf_realpath).  A relative pointer is a
    # shape git does not produce, so rather than resolve it against an arbitrary
    # CWD, fail SAFE and call the entry live: skipping is the only direction of
    # this gate that can hide an armed lane, and a spurious ARMED report is
    # merely advisory where a spurious skip is silent.
    case "$pointer" in
        /*) ;;
        *)  return 0 ;;
    esac

    [ -e "$pointer" ] || return 1

    return 0
}

_sweep_worktree_configs() {
    local armed=0 wt_config wt_dir wt_name mentions key value
    local last_enabled last_autoupdate read_out read_status live_status

    # config.worktree is DEAD BYTES unless extensions.worktreeConfig is true —
    # git does not read those files at all, so a rerere.enabled=true sitting in
    # one is inert and reporting it would be a FALSE armed verdict.  A uniquely
    # damaging one, too: `arm` writes --local only and cannot clear a
    # per-worktree file, so the false verdict would make `arm` fail permanently
    # on a store where nothing is actually wrong.  Reify's store has the
    # extension on (setup-main-gate-worktree-config.sh enables it), but the
    # guard must not silently depend on a precondition it never checks.
    if [ "$(git -C "$TARGET" config --bool --get extensions.worktreeConfig 2>/dev/null || true)" != "true" ]; then
        return 0
    fi

    # COST.  This runs over every config.worktree in the store — 200+ files on the
    # live pool — so a `git config` fork per key per file was ~450 forks and a
    # measured 5.4s for a read-only probe.  One `grep -lis rerere` over the whole
    # set collapses that: on a healthy store NO config.worktree mentions rerere at
    # all, so the per-file parse below never runs and the sweep costs a single
    # fork.  The prefilter can only ever cause a file to be SKIPPED when the
    # string 'rerere' is absent from its bytes — in which case no `git config`
    # read of a rerere.* key could have returned anything either.  Unreadable
    # files are still walked and WARNed about below, precisely because grep cannot
    # see them.
    #
    # 'include' is in the pattern alongside 'rerere' because a rerere.* key can
    # reach a config.worktree by `include.path` indirection, without the string
    # 'rerere' appearing anywhere in its own bytes.  Matching the indirection
    # keyword too keeps the prefilter a pure cost optimisation rather than a
    # second, weaker parser.  Case-insensitively, it also matches `includeIf`.
    #
    # That term is load-bearing ONLY because the read below now passes
    # --includes EXPLICITLY.  git turns include-following OFF whenever a specific
    # file or scope is named (--file, --local, --worktree, --global, --system,
    # --blob) and ON only for an effective read — measured on git 2.43.0, the
    # exact opposite of what this comment used to assert — so before that flag
    # was added the term guarded a path the reader never traversed.
    mentions="$(grep -lisE 'rerere|include' -- "$COMMON_DIR/config.worktree" "$WORKTREES_DIR"/*/config.worktree 2>/dev/null || true)"

    # The main checkout's own config.worktree is PREPENDED to the linked-worktree
    # glob rather than handled by a duplicate block, so the readability check, the
    # `git config --file` read and the reporting below are shared by both.
    #
    # No `[ -d "$WORKTREES_DIR" ]` guard: a repo with no linked worktrees has no
    # worktrees/ dir at all — normal, not an error — and an early return there
    # would skip the main-dir entry too.  The unmatched glob is already handled by
    # the per-entry existence check.
    for wt_config in "$COMMON_DIR/config.worktree" "$WORKTREES_DIR"/*/config.worktree; do
        # Absent main-dir config.worktree, or an unmatched glob under a store with
        # no worktrees/ dir or no config.worktree files in it.
        [ -e "$wt_config" ] || continue

        # The main-dir label is STATED, not derived: basename(dirname) of
        # <common>/config.worktree is the useless '.git'.  '<main checkout>' is the
        # same label _classify_lock already uses for that dir in scan-locks.
        #
        # The lane label is derived with parameter expansion, NOT basename/dirname:
        # those were two forks per file, and on the live pool they — not the
        # `git config` reads — were the dominant cost of the whole sweep (measured:
        # ~4-5s of a 5.4s `check`, with the reads' own prefilter already in place).
        if [ "$wt_config" = "$COMMON_DIR/config.worktree" ]; then
            wt_name="<main checkout>"
        else
            wt_dir="${wt_config%/config.worktree}"
            wt_name="${wt_dir##*/}"

            # The liveness gate is TRI-STATE; treating it as a boolean is what
            # made the sweep fail open.  The main checkout is never stale and
            # has no `gitdir` file at all, hence the else-branch placement.
            live_status=0
            _linked_worktree_is_live "$wt_dir" || live_status=$?

            # STALE ENTRY (1): the admin dir outlived the working tree it
            # describes and `git worktree prune` has not reclaimed it yet.  git
            # will never read this config.worktree again, so it is inert — skip
            # SILENTLY, and specifically do NOT count it as UNKNOWN: it is
            # verified irrelevant, not unverifiable, and a routine transient on
            # a pool that churns lanes would otherwise pin `check` at exit 3
            # forever.
            if [ "$live_status" -eq 1 ]; then
                continue
            fi

            # ANYTHING ELSE non-zero (2): the entry EXISTS but its liveness
            # could not be determined, so the lane is neither verified live nor
            # verified inert — and if it is live, its config.worktree may be
            # armed.  UNKNOWN, in the same vocabulary as an unreadable
            # config.worktree below: cmd_check turns the count into exit 3, so a
            # store the guard could not fully read never answers 0 = safe.
            # Unconditional rather than `-eq 2`, so a future code can never fall
            # through into the sweep as though the lane were verified live.
            if [ "$live_status" -ne 0 ]; then
                echo "WARNING: cannot read $wt_dir/gitdir — cannot tell whether worktree '$wt_name' is live." >&2
                echo "  An unreadable gitdir does NOT make the entry prunable, so this lane's" >&2
                echo "  config.worktree may still be the one git reads.  Its rerere state is" >&2
                echo "  UNKNOWN, not verified safe." >&2
                _SWEEP_UNKNOWN=$((_SWEEP_UNKNOWN + 1))
                continue
            fi
        fi

        # One unreadable config.worktree must not abort the sweep and mask every
        # other lane — report it and keep going.  UNKNOWN, counted separately
        # from armed: cmd_check turns a non-zero count into exit 3, so a store
        # the guard could not fully read never answers 0 = safe.
        if [ ! -r "$wt_config" ]; then
            echo "WARNING: cannot read $wt_config — skipping worktree '$wt_name'." >&2
            echo "  This lane's rerere state is UNKNOWN, not verified safe." >&2
            _SWEEP_UNKNOWN=$((_SWEEP_UNKNOWN + 1))
            continue
        fi

        # Prefilter membership test — a pure-bash substring match on the
        # newline-delimited grep output, no fork.  Anchored on both sides with
        # newlines so '/a/config.worktree' cannot match '/xx/a/config.worktree'.
        case "$LF$mentions$LF" in
            *"$LF$wt_config$LF"*) ;;
            *) continue ;;
        esac

        # ONE fork for BOTH keys.  --bool applies to --get-regexp output, so
        # valueless keys and `yes`/`on`/`1` still normalise to true — the whole
        # reason a grep cannot be trusted with the value.
        #
        # --includes is passed EXPLICITLY: with --file naming a specific file git
        # defaults include-following OFF, so a config.worktree that is nothing
        # but `[include] path = extra.cfg` returned exit 1 and no output here
        # while git's own effective read honoured the chain and armed the lane
        # (measured, git 2.43.0).
        #
        # The reduction is LAST-VALUE-PER-KEY, not any-value: --get-regexp
        # reports EVERY value a file sets for a key, while git resolves a
        # multi-valued key to the LAST one, so the two must not be conflated.
        # Flagging any emitted `true` reported a config.worktree whose final word
        # is `false` as ARMED (measured: `enabled = true` then `enabled = false`
        # resolves to false, yet was reported) — and that false verdict is one
        # `arm` can NEVER clear, because it writes --local and cannot touch a
        # per-worktree file, so the store would park on the advisory exit 2
        # forever.  Accumulate here, decide after the loop.
        #
        # The read's STATUS is captured, never discarded with `|| true`.
        # --includes lets it fail on a file this loop never opens — a circular
        # chain and an unreadable include target both exit 128 with NO stdout —
        # and `|| true` would launder that into empty output, i.e. into "no
        # rerere keys here, clean": the exact silent-false-clean the guard
        # exists to prevent.  A command substitution swallows the status, so the
        # output is assigned to a variable under a guarded `if` first and the
        # loop is fed from that variable.  Still exactly ONE fork per file.
        read_status=0
        read_out="$(git config --file "$wt_config" --includes --bool \
            --get-regexp '^rerere\.(enabled|autoupdate)$' 2>/dev/null)" || read_status=$?

        # Exit 1 is git's ordinary "no matching key" answer — CLEAN.  Anything
        # else means this file's value could not be resolved at all, which is
        # UNKNOWN, not ARMED: `arm` writes --local and cannot fix a lane the
        # guard merely fails to read, so folding it into `armed` would strand the
        # store on a permanent failure — the same trap the extensions.
        # worktreeConfig gate avoids.  Warn and CONTINUE, so one broken lane
        # cannot mask the rest.
        if [ "$read_status" -ne 0 ] && [ "$read_status" -ne 1 ]; then
            echo "WARNING: cannot read $wt_config through its include.path chain — skipping worktree '$wt_name'." >&2
            echo "  git config exited $read_status; a circular chain and an unreadable include" >&2
            echo "  target both report 128 with no output, which is indistinguishable from clean." >&2
            echo "  This lane's rerere state is UNKNOWN, not verified safe." >&2
            _SWEEP_UNKNOWN=$((_SWEEP_UNKNOWN + 1))
            continue
        fi

        last_enabled=""
        last_autoupdate=""
        while read -r key value; do
            case "$key" in
                rerere.enabled)    last_enabled="$value" ;;
                rerere.autoupdate) last_autoupdate="$value" ;;
            esac
        done <<EOF
$read_out
EOF

        if [ "$last_enabled" = "true" ]; then
            _report_armed_worktree "$wt_name" rerere.enabled "$wt_config"
            armed=1
        fi
        if [ "$last_autoupdate" = "true" ]; then
            _report_armed_worktree "$wt_name" rerere.autoupdate "$wt_config"
            armed=1
        fi
    done

    return "$armed"
}

# cmd_arm — idempotently pin rerere off in the SHARED local config, then
# re-verify via the full check logic.
#
# --local, NEVER --worktree: the entire point is that every lane of the store
# inherits one shared default with zero per-lane wiring.  A --worktree write would
# leave every other lane armed while this one reads clean.
#
# MUST NOT delete or prune rr-cache.  Deleting an entry another live worktree
# still holds is precisely the operation that reproduces the segfault + stale
# MERGE_RR.lock signature, so the cure would re-cause the disease across the
# fleet.  The explicit `false` neutralises the residual cache in place — that is
# measured behaviour, not an assumption (see the suite's behavioural oracles).
cmd_arm() {
    local changed=0 key before before_display check_rc

    for key in rerere.enabled rerere.autoupdate; do
        # Compare against the current LOCAL value and skip a redundant write, so
        # a re-run is a byte-level no-op on .git/config.  setup-dev.sh invokes
        # this every time; a rewrite would churn the shared file for nothing.
        #
        # DELIBERATELY NO --includes here, in intentional asymmetry with the two
        # reads in _check_shared_default and the one in _sweep_worktree_configs.
        # Those ask "what does git resolve?"; this one asks the different
        # question "is the literal shared FILE already pinned, so this write
        # would be a byte-level no-op?".  Following an include would answer a
        # question `arm` is not asking: a shared config whose INCLUDED file
        # happens to set false would make `arm` skip the write entirely, leaving
        # .git/config with no direct pin and re-armable the moment another lane
        # edits or removes that included file.  Do not "tidy" all four reads into
        # agreement.  (--includes is mechanically safe here — measured, it merely
        # widens which files are read — so the reason to omit it is semantic, not
        # mechanical.)
        #
        # --get-all, not --get: `--get` resolves a multi-valued key to its LAST
        # value, so a shared config holding `enabled = true` followed by
        # `enabled = false` would answer "false" and skip the write, leaving the
        # stale `true` line one deletion away from re-arming the fleet.  With
        # --get-all the answer is the literal set of values the file holds, and
        # the skip fires only when that set is exactly one `false` — which is
        # also what makes a re-run a byte-level no-op in the ordinary case.
        before="$(git -C "$TARGET" config --local --get-all "$key" 2>/dev/null || true)"
        if [ "$before" = "false" ]; then
            continue
        fi
        # A multi-valued key is rendered on one line so the SET diagnostic below
        # stays a single line an operator can grep.
        before_display="${before:-<unset>}"
        before_display="${before_display//$LF/, }"
        # --replace-all, and guarded rather than bare.
        #
        # --replace-all is a strict superset of a plain single-value write:
        # byte-identical output when the key is unset or single-valued (measured
        # on git 2.43.0), and it COLLAPSES a multi-valued key to one `false`
        # instead of failing.  A plain write aborts on that shape with
        # `error: cannot overwrite multiple values with a single value` and
        # git's own exit 5 — and `git config --add rerere.enabled true`, exactly
        # what an unidentified re-armer would leave behind (runbook §7), creates
        # it.  Without --replace-all the guard could not self-heal the very
        # shape it exists to fix, and the failure was not inert: setup-dev.sh's
        # `*` arm turns a non-zero `arm` into `err` + `exit 1`, killing every
        # later setup step for every developer while the fleet stayed armed.
        #
        # Still GUARDED: under `set -euo pipefail` a bare write that loses a race
        # on .git/config.lock (200+ lanes share one shared config) or hits a
        # read-only store would abort the script with git's own status — never
        # the documented 1 — and a consumer branching on `1 => fatal` would read
        # that as success.
        if ! git -C "$TARGET" config --local --replace-all "$key" false; then
            echo "ERROR: failed to write $key=false to $COMMON_DIR/config." >&2
            echo "  The shared config was NOT pinned, so rerere may still be armed fleet-wide." >&2
            echo "  Common causes: a lost race on $COMMON_DIR/config.lock (every lane of the" >&2
            echo "  store writes that one file), or a read-only store." >&2
            return 1
        fi
        echo "SET: $key=false (was $before_display) in $COMMON_DIR/config" >&2
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
    check_rc=0
    cmd_check || check_rc=$?

    if [ "$check_rc" -eq 0 ]; then
        return 0
    fi

    # check's exit 3 = at least one lane UNVERIFIABLE, none found armed.  The
    # shared write succeeded and nothing is known to be wrong, but the store was
    # not fully verified, so this is not the clean 0 either.  Advisory 2, the
    # same code as a surviving foreign override and for the same reason: what
    # remains is out of `arm`'s reach (a --local writer cannot repair a lane it
    # cannot read), and setup-dev.sh must warn rather than abort.
    if [ "$check_rc" -eq 3 ]; then
        echo "WARNING: the shared config is pinned, but $_SWEEP_UNKNOWN worktree(s) could not be verified." >&2
        echo "  No armed scope was found; those lanes' rerere state is UNKNOWN, not safe." >&2
        echo "  See the WARNINGs above for which, and make each config.worktree readable." >&2
        return 2
    fi

    # Any other non-zero: armed (1), or an exit this run does not recognise.
    # Unconditional rather than a further `if`, so a future `check` code can
    # never fall through to a clean 0 — an unverified store must stay non-zero.
    {
        echo "WARNING: rerere is STILL armed after writing the shared config." >&2
        echo "  The shared write above SUCCEEDED — what survives is an override this run" >&2
        echo "  cannot reach: another lane's config.worktree, the user's global gitconfig, or" >&2
        echo "  an include.path chain in the shared config resolved AFTER the section this run" >&2
        echo "  wrote — git rewrites an existing [rerere] section IN PLACE, so a later [include]" >&2
        echo "  still wins (measured), and the write can succeed while changing nothing." >&2
        echo "  See the ARMED lines above for which, and clear it at that scope." >&2
        # Exit 2, NOT 1.  setup-dev.sh runs `arm` under `set -e` on every
        # developer setup, and this branch is dominated by a FOREIGN lane's
        # config.worktree — something `arm` (a --local writer) has no ability to
        # fix.  One self-armed lane must not abort everything after
        # this point in setup.  So: 2 = "shared config pinned, an out-of-reach
        # override survives" (advisory, actionable by an operator); 1 stays
        # reserved for a genuine failure of this run.
        return 2
    }
}

# cmd_scan_locks — read-only census of MERGE_RR.lock across the WHOLE store —
# the main checkout's own git dir AND every linked worktree — classifying each
# hit.
#
# MATCHES ONLY THE .lock SUFFIX, never a bare MERGE_RR.  A bare MERGE_RR is
# ORDINARY rerere state — 41 of the live store's worktrees carry one right now —
# and reporting it would make this census read as "the corruption is fleet-wide"
# when nothing is wrong.  Only MERGE_RR.lock indicates the stuck condition.
#
# NEVER MUTATES.  Deletion stays manual and operator-driven: this script must not
# reach into another lane's git dir, and a lock is only safe to remove once it is
# established that no operation is in progress there — a judgement an unattended
# script should not act on.  It prints the exact command instead.
cmd_scan_locks() {
    local found=0 lock wt_dir

    # The MAIN checkout first.  For it, git dir == common dir, so its lock lands
    # at <common>/MERGE_RR.lock and NEVER under worktrees/ — a glob over the
    # linked worktrees alone is blind to it.  The main checkout is an ACTIVE
    # merge site: the sanctioned landing path (scripts/land.sh) runs a real
    # `git merge --no-ff` there, so it is a live candidate for this very
    # signature, and a census that skipped it would report "clean" while every
    # `git commit` on main exits 128.
    if [ -e "$COMMON_DIR/MERGE_RR.lock" ]; then
        found=1
        _classify_lock "$COMMON_DIR/MERGE_RR.lock" "$COMMON_DIR" "<main checkout>"
    fi

    # Then every linked worktree.  A store with no linked worktrees has no
    # worktrees/ dir at all — a normal shape, and NOT a reason to skip the
    # verdict below, which is why this is a plain guard rather than the early
    # return it used to be.
    if [ -d "$WORKTREES_DIR" ]; then
        for lock in "$WORKTREES_DIR"/*/MERGE_RR.lock; do
            [ -e "$lock" ] || continue
            found=1
            wt_dir="$(dirname "$lock")"
            _classify_lock "$lock" "$wt_dir" "$(basename "$wt_dir")"
        done
    fi

    if [ "$found" -eq 0 ]; then
        echo "clean: no MERGE_RR.lock in $COMMON_DIR or under $WORKTREES_DIR" >&2
        return 0
    fi

    return 1
}

# _classify_lock LOCK WT_DIR LABEL — report one MERGE_RR.lock hit as STALE or
# OPERATION-IN-PROGRESS.  Read-only; prints to stderr only.
_classify_lock() {
    local lock="$1" wt_dir="$2" label="$3" marker in_progress=""

    # An in-flight merge/rebase/cherry-pick/revert makes the lock LIVE, not
    # stale.  Removing it would destroy that operation's state.
    for marker in MERGE_HEAD MERGE_MSG CHERRY_PICK_HEAD REVERT_HEAD rebase-merge rebase-apply; do
        if [ -e "$wt_dir/$marker" ]; then
            in_progress="$marker"
            break
        fi
    done

    if [ -n "$in_progress" ]; then
        echo "OPERATION-IN-PROGRESS: $label holds MERGE_RR.lock alongside $in_progress." >&2
        echo "  Path: $lock" >&2
        echo "  NOT safe to clean — an operation is live in that worktree.  Let it finish or" >&2
        echo "  abort it from inside that worktree first, then re-run scan-locks." >&2
    else
        echo "STALE: $label holds a MERGE_RR.lock with no operation in progress." >&2
        echo "  Path: $lock" >&2
        echo "  Every git commit in that worktree exits 128 until it is removed — but the" >&2
        echo "  commit object is still written and the ref still moves, so ALWAYS run" >&2
        echo "  'git log --oneline -1' there before retrying, or you will double-commit." >&2
        echo "  Remediation (run manually, this script never deletes):" >&2
        echo "    rm -f $lock" >&2
    fi
}

case "$SUBCOMMAND" in
    check)      cmd_check ;;
    arm)        cmd_arm ;;
    scan-locks) cmd_scan_locks ;;
esac
