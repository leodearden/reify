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
#               Exit 0 = safe, 1 = armed (the machine-readable signal).
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

set -euo pipefail

_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() {
    cat >&2 <<'USAGE'
Usage: git-rerere-guard.sh <subcommand> [target_dir]

Subcommands:
  check       Report whether git rerere is effectively armed for the target
              store (read-only).  Exit 0 = safe, 1 = armed.
  arm         Idempotently disable rerere in the shared local config, then
              re-verify.  Never deletes rr-cache.  Exit 0 = disarmed,
              2 = pinned but an out-of-reach override survives, any other
              non-zero = this run failed.  Branch on 0 | 2 | *.
  scan-locks  Read-only census of stale MERGE_RR.lock files across the whole
              store — the main checkout and every linked worktree.
              Exit 0 = clean, 1 = lock(s) found.

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

    return "$armed"
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
_check_shared_default() {
    local key="$1" shared

    if ! git -C "$TARGET" config --local --get "$key" >/dev/null 2>&1; then
        # Unset in the shared config.  Only rerere.enabled has a non-false
        # default: -1 = "enabled iff rr-cache/ exists".  rerere.autoupdate
        # defaults to false, so an unset shared autoupdate is genuinely safe.
        if [ "$key" = "rerere.enabled" ] && [ -d "$RR_CACHE" ]; then
            echo "ARMED: the SHARED config leaves rerere.enabled UNSET and $RR_CACHE exists." >&2
            echo "  $TARGET reads disarmed only because its own config overrides the shared" >&2
            echo "  default; every lane WITHOUT such an override inherits git's -1 default" >&2
            echo "  ('enabled iff rr-cache/ exists') and is armed.  Run 'arm' to pin it." >&2
            return 1
        fi
        return 0
    fi

    shared="$(git -C "$TARGET" config --local --bool --get "$key" 2>/dev/null || true)"
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
_sweep_worktree_configs() {
    local armed=0 wt_config wt_name mentions key value

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
    # 'include' is in the pattern alongside 'rerere' because `git config --file`
    # HONOURS include.path, so a rerere.* key can reach a config.worktree by
    # indirection without the string 'rerere' appearing in its bytes.  Matching
    # the indirection keyword too keeps the prefilter a pure cost optimisation
    # rather than a second, weaker parser.
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
            wt_name="${wt_config%/config.worktree}"
            wt_name="${wt_name##*/}"
        fi

        # One unreadable config.worktree must not abort the sweep and mask every
        # other lane — report it and keep going.
        if [ ! -r "$wt_config" ]; then
            echo "WARNING: cannot read $wt_config — skipping worktree '$wt_name'." >&2
            echo "  This lane's rerere state is UNKNOWN, not verified safe." >&2
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
        while read -r key value; do
            [ "$value" = "true" ] || continue
            echo "ARMED: worktree '$wt_name' overrides $key=true in its config.worktree." >&2
            echo "  Path: $wt_config" >&2
            echo "  git reads config.worktree FIRST, so this beats the shared .git/config." >&2
            armed=1
        done <<EOF
$(git config --file "$wt_config" --bool --get-regexp '^rerere\.(enabled|autoupdate)$' 2>/dev/null || true)
EOF
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
    local changed=0 key before

    for key in rerere.enabled rerere.autoupdate; do
        # Compare against the current LOCAL value and skip a redundant write, so
        # a re-run is a byte-level no-op on .git/config.  setup-dev.sh invokes
        # this every time; a rewrite would churn the shared file for nothing.
        before="$(git -C "$TARGET" config --local --get "$key" 2>/dev/null || true)"
        if [ "$before" = "false" ]; then
            continue
        fi
        # Guarded, not bare.  Under `set -euo pipefail` a bare write that loses a
        # race on .git/config.lock (200+ lanes share one shared config), hits a
        # read-only store, or trips over a multi-valued key would abort the script
        # with git's own status — never the documented 1 — and a consumer
        # branching on `1 => fatal` would read that as success.
        if ! git -C "$TARGET" config --local "$key" false; then
            echo "ERROR: failed to write $key=false to $COMMON_DIR/config." >&2
            echo "  The shared config was NOT pinned, so rerere may still be armed fleet-wide." >&2
            echo "  Common causes: a lost race on $COMMON_DIR/config.lock (every lane of the" >&2
            echo "  store writes that one file), a read-only store, or a multi-valued $key." >&2
            return 1
        fi
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
        echo "WARNING: rerere is STILL armed after writing the shared config." >&2
        echo "  The shared write above SUCCEEDED — what survives is an override this run" >&2
        echo "  cannot reach: another lane's config.worktree, or the user's global gitconfig." >&2
        echo "  See the ARMED lines above for which, and clear it at that scope." >&2
        # Exit 2, NOT 1.  setup-dev.sh runs `arm` under `set -e` on every
        # developer setup, and this branch is dominated by a FOREIGN lane's
        # config.worktree — something `arm` (a --local writer) has no ability to
        # fix.  One self-armed lane must not abort everything after
        # this point in setup.  So: 2 = "shared config pinned, an out-of-reach
        # override survives" (advisory, actionable by an operator); 1 stays
        # reserved for a genuine failure of this run.
        return 2
    fi

    return 0
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
