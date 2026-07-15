#!/usr/bin/env bash
# scripts/seed-warm-lane.sh — CoW clone + warmth-transfer helper for warm-lane pool.
#
# D10 always-re-seed-at-acquire contract (PRD §9.3, 2026-06-18 amendment):
#   The seed primitive itself is UNCHANGED.  The acquire path (pool consumer, DF ζ)
#   MUST always pass --fresh-checkout so a staled lane is rescued to warm rather
#   than rebuilt near-cold via --reset-in-place.  --reset-in-place is retained only
#   as a control arm in the B13 re-seed warmth delta test.
#
#   Resolve convention (D8 seam): the caller MUST resolve <base>/target (a symlink
#   to a .gen.N dir) to its CONCRETE .gen.N path before passing it to this script.
#   cp -a --reflink=always copies the SYMLINK, not its target; passing the symlink
#   directly creates a broken-link clone.  Pin the concrete gen path AND hold
#   `flock -s <base>.gen.N.lock` for the duration of the cp walk (reader-refcount
#   D8 seam) so refresh-warm-base.sh GC defers rm until the clone completes.
#
# Usage (seed mode):
#   lane_target=$(scripts/seed-warm-lane.sh <base_target_dir> <lane_dir> \
#                    (--fresh-checkout|--reset-in-place) \
#                    [--base-commit <sha>] [--touch <path>]... [--lane-lock])
#
#   --lane-lock (opt-in, default OFF; PRD §9.5 inv.11): acquire an EXCLUSIVE
#     flock on the sibling-path ${LANE_DIR}.lock -- the same convention
#     thin-warm-lane.sh (T3) and warm-lane-gc.sh (live-consumer probe) use --
#     BEFORE any target mutation, held across the whole run. Non-blocking:
#     refuses with EX_TEMPFAIL (75) when a live consumer already holds it.
#     Existing callers that omit this flag are unaffected -- see
#     thin-warm-lane.sh --reseed, which already holds this same lock before
#     invoking this script.
#     REIFY_WARM_LANE_LANE_LOCK_WAIT (env, only with --lane-lock): 0 (default)
#     = non-blocking refuse; N>0 = queue up to N seconds (flock -w N) before
#     refusing; "unlimited" = block until acquired, never refuses. A
#     refused acquirer of a task lane can just try a different FREE lane, but
#     the SINGLETON _merge-verify lane has no alternate -- it QUEUEs instead.
#     FD 9 (fixed, matching thin-warm-lane.sh's T3 convention): callers that
#     pass --lane-lock MUST NOT themselves hold a load-bearing FD 9 open across
#     this invocation -- `exec 9>"$LANE_LOCK"` would silently reassign it.
#
# Usage (record-base mode):
#   sidecar=$(scripts/seed-warm-lane.sh --record-base <base_target_dir>)
#
# Stdout (seed mode):   resolved <lane_dir>/target path on success.
# Stdout (record mode): resolved sidecar path on success.
# Stderr:               all diagnostics, progress messages, and errors.
#
# Guards (seed mode, checked before any work):
#   D4/B5: ${RUSTFLAGS:-} must match the RUSTFLAGS recorded in the sidecar beside
#          base_target_dir ($(dirname base_target_dir)/.warm-base-meta). Missing
#          sidecar → defaults recorded value to "" (§9.2).
#   S1:    ${REIFY_WARM_LANE_INVOCATION:-} must match the recorded invocation
#          fingerprint.  Mismatch → actionable stderr, non-zero exit, nothing on
#          stdout, no cp invoked.
#
# Clone (S2):
#   cp -a --reflink=always <base_target_dir> <lane_dir>/target
#   A non-reflink FS is a hard error; there is no silent full-copy fallback.
#   --fresh-checkout: a non-empty <lane_dir>/target is REPLACED (mv to pool-level
#     trash sidecar at dirname(lane_dir)/.reseed-trash/basename(lane_dir).PID,
#     reflink-clone, rm trash).  Misuse refusals (checked first, cp never reached):
#     (a) REIFY_WARM_LANE_MOUNT set + LANE_TARGET not under it → exit 1; (b) LANE_TARGET
#     or LANE_DIR == BASE_TARGET_DIR (self-clobber of base) → exit 1.
#     Knobs: REIFY_WARM_LANE_RESEED_TRASH_SYNC (foreground rm, tests).
#   --reset-in-place: a non-empty <lane_dir>/target is still REFUSED (clobber guard).
#
# Mtime (D5):
#   --fresh-checkout: bulk-stamp sources to 2020-01-01 (find, pruning target/ & .git/)
#                     then touch delta (--touch paths + git diff --name-only <base_commit>) to now.
#   --reset-in-place: no bulk stamp (git clean -xfd -e target already moved changed mtimes).

set -euo pipefail

# ── log helpers (all write to stderr) ────────────────────────────────────────
info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*" >&2; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*" >&2; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*" >&2; }
err()   { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }

# ── usage ─────────────────────────────────────────────────────────────────────
_usage() {
    cat >&2 <<'EOF'
Usage:
  seed-warm-lane.sh <base_target_dir> <lane_dir> (--fresh-checkout|--reset-in-place) \
      [--base-commit <sha>] [--touch <path>]... [--lane-lock]
  seed-warm-lane.sh --record-base <base_target_dir>

Seed mode: CoW-clone a warm base target/ into a pool lane.
  <base_target_dir>   Path to the warm base target/ directory to clone.
  <lane_dir>          Path to the new pool lane directory.
  --fresh-checkout    Replace non-empty <lane_dir>/target (mv to trash, reflink-clone,
                      rm trash); then bulk-stamp sources to 2020-01-01 and touch
                      changed files to now (D5).
  --reset-in-place    Refuse a non-empty <lane_dir>/target (B13 control arm only;
                      production acquires always use --fresh-checkout).  No bulk stamp.
  --base-commit sha   Git commit the base was built from; drives git diff --name-only.
  --touch path        Additional path to touch to now after bulk stamp (repeatable).
  --lane-lock         Opt-in (default OFF; PRD §9.5 inv.11): hold an exclusive flock
                      on the sibling ${LANE_DIR}.lock across the whole run, BEFORE any
                      target mutation. Refuses (EX_TEMPFAIL 75) if a live consumer
                      already holds it (inv.2 one-consumer-per-lane-at-a-time).
                      REIFY_WARM_LANE_LANE_LOCK_WAIT (env, only with --lane-lock):
                      0 (default) = non-blocking refuse (flock -n); N>0 = queue up
                      to N seconds before refusing (flock -w N); "unlimited"
                      (case-insensitive) = block until acquired, never refuses
                      (flock). Anything else is a usage error (exit 64).
                      Uses a fixed FD 9 (matching thin-warm-lane.sh's T3):
                      callers passing --lane-lock must not themselves hold a
                      load-bearing FD 9 open across this invocation.

Record-base mode: stamp provenance beside the base target dir.
  --record-base dir   Write sidecar at $(dirname dir)/.warm-base-meta; print path on stdout.

Options:
  -h, --help          Print this message and exit (0).

Stdout:  resolved <lane_dir>/target (seed mode) or sidecar path (record-base mode).
Stderr:  all diagnostics.

Guards (seed mode, fail-closed before any work):
  B5/D4: ${RUSTFLAGS:-} must equal recorded RUSTFLAGS (default "").
  S1:    ${REIFY_WARM_LANE_INVOCATION:-} must equal recorded invocation (default "").
  S2:    clone uses cp --reflink=always; non-reflink FS is a hard error.
         --fresh-checkout: non-empty <lane_dir>/target is replaced (mv+cp+rm).
         Trash sidecar: dirname(lane_dir)/.reseed-trash/basename(lane_dir).PID
           (pool-level sibling — same XFS mount → atomic mv; dot-prefixed → invisible
           to any walker rooted at the lane: DF git clean, find, cargo; #4896).
         Misuse refusals (checked before any rename; --fresh-checkout only):
           REIFY_WARM_LANE_MOUNT set + LANE_TARGET not under mount → exit 1.
           LANE_TARGET or LANE_DIR == BASE_TARGET_DIR (self-clobber) → exit 1.
         REIFY_WARM_LANE_RESEED_TRASH_SYNC=1 forces synchronous trash rm (tests).
EOF
}

# ── arg parsing ───────────────────────────────────────────────────────────────
MODE=""             # set to "seed" or "record-base" after validation
FRESH_CHECKOUT=""
RESET_IN_PLACE=""
BASE_COMMIT=""
TOUCH_PATHS=()
RECORD_BASE_DIR=""
LANE_LOCK_OPT=""
_POSITIONALS=()

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            _usage
            exit 0
            ;;
        --fresh-checkout)
            FRESH_CHECKOUT=1
            shift
            ;;
        --reset-in-place)
            RESET_IN_PLACE=1
            shift
            ;;
        --lane-lock)
            LANE_LOCK_OPT=1
            shift
            ;;
        --base-commit)
            [ $# -ge 2 ] || { err "--base-commit requires a value"; exit 2; }
            BASE_COMMIT="$2"
            shift 2
            ;;
        --touch)
            [ $# -ge 2 ] || { err "--touch requires a value"; exit 2; }
            TOUCH_PATHS+=("$2")
            shift 2
            ;;
        --record-base)
            [ $# -ge 2 ] || { err "--record-base requires a value"; exit 2; }
            RECORD_BASE_DIR="$2"
            MODE="record-base"
            shift 2
            ;;
        -*)
            err "Unknown flag: $1"
            err "Run '$(basename "$0") --help' for usage."
            exit 2
            ;;
        *)
            _POSITIONALS+=("$1")
            shift
            ;;
    esac
done

# ── validate mode + args ──────────────────────────────────────────────────────
if [ "$MODE" = "record-base" ]; then
    # record-base mode: no positionals or mode flags allowed
    if [ "${#_POSITIONALS[@]}" -gt 0 ]; then
        err "--record-base mode: unexpected positional arguments: ${_POSITIONALS[*]}"
        exit 2
    fi
    if [ -n "$FRESH_CHECKOUT" ] || [ -n "$RESET_IN_PLACE" ]; then
        err "--record-base mode: --fresh-checkout/--reset-in-place are invalid here"
        exit 2
    fi
else
    # seed mode: require exactly 2 positionals + exactly one of the mode flags
    MODE="seed"
    if [ "${#_POSITIONALS[@]}" -lt 2 ]; then
        err "seed mode requires <base_target_dir> and <lane_dir> as positional arguments"
        err "Run '$(basename "$0") --help' for usage."
        exit 2
    fi
    if [ "${#_POSITIONALS[@]}" -gt 2 ]; then
        err "seed mode: unexpected extra positional arguments: ${_POSITIONALS[*]:2}"
        exit 2
    fi
    if [ -n "$FRESH_CHECKOUT" ] && [ -n "$RESET_IN_PLACE" ]; then
        err "Specify exactly one of --fresh-checkout or --reset-in-place, not both."
        exit 2
    fi
    if [ -z "$FRESH_CHECKOUT" ] && [ -z "$RESET_IN_PLACE" ]; then
        err "Specify exactly one of --fresh-checkout or --reset-in-place."
        err "Run '$(basename "$0") --help' for usage."
        exit 2
    fi

    BASE_TARGET_DIR="${_POSITIONALS[0]}"
    LANE_DIR="${_POSITIONALS[1]}"
fi

# ── sidecar helpers ───────────────────────────────────────────────────────────
# Sidecar lives BESIDE the base target dir: $(dirname base_target_dir)/.warm-base-meta
_sidecar_path() {
    local base_target_dir="$1"
    echo "$(dirname "$base_target_dir")/.warm-base-meta"
}

# Read a KEY from the sidecar; print "" if sidecar absent or key missing.
_sidecar_read() {
    local sidecar="$1"
    local key="$2"
    if [ ! -f "$sidecar" ]; then
        echo ""
        return
    fi
    local val
    # Match "KEY=<rest of line>" (key names are UPPER_SNAKE_CASE)
    val="$(grep -m1 "^${key}=" "$sidecar" 2>/dev/null || true)"
    # Strip the KEY= prefix
    echo "${val#${key}=}"
}

# Read the authoritative per-gen landed-commit stamp written by refresh-warm-base.sh.
# The stamp lives as a sibling of the concrete gen dir:
#   ${base_target_dir}.basecommit
# Per the D8 resolve convention the caller resolves the base symlink to its
# concrete gen path before passing base_target_dir here, so this is a direct file
# read with no symlink traversal.  Returns "" if the stamp is absent (pre-fix base
# or any mode where refresh has not yet run the Step 4b write).
# Consumed by the EFFECTIVE_BASE_COMMIT resolution below with higher priority than
# the drift-prone .warm-base-meta BASE_COMMIT (see esc-3468-75 and design decisions).
_read_basecommit_stamp() {
    local base_target_dir="$1"
    local stamp="${base_target_dir}.basecommit"
    if [ -f "$stamp" ]; then
        cat "$stamp"
    else
        echo ""
    fi
}

# Read the per-gen build-worktree stamp written by refresh-warm-base.sh (Step 4b):
#   ${base_target_dir}.buildroot
# Mirrors _read_basecommit_stamp above: per the D8 resolve convention the caller
# resolves the base symlink to its concrete gen path before passing
# base_target_dir here, so this is a direct file read with no symlink traversal.
# Returns "" if the stamp is absent (pre-fix base, or refresh has not yet run
# the Step 4b write) — the caller treats an absent stamp as a fail-safe
# "relink" case (see the env!()-baked-path relink gate below, task 4983).
_read_buildroot_stamp() {
    local base_target_dir="$1"
    local stamp="${base_target_dir}.buildroot"
    if [ -f "$stamp" ]; then
        cat "$stamp"
    else
        echo ""
    fi
}

# Escape <string> (anything outside [a-zA-Z0-9_]) for safe interpolation as a
# LITERAL into a `sed -E` pattern or replacement (task 5126). Backslash-escaping
# every non-word byte defeats ERE metacharacters (. * + ? | { } ( ) [ ] ^ $ \)
# on the pattern side and the &/\ specials on the replacement side. ERE (-E),
# not BRE: under plain BRE, \+ \? \| \{ \} \( \) are GNU *extensions* that turn
# ON special meaning, which would invert this escaping and corrupt any baked
# path containing those bytes.
_sed_escape() {
    printf '%s' "$1" | sed -e 's/[^a-zA-Z0-9_]/\\&/g'
}

# Seed-time post-condition: assert no file listed by `git diff --name-only <sha>`
# still carries the 2020-01-01 bulk-stamp epoch after the delta-touch.
# This is defense-in-depth against any future regression of _touch_git_delta
# (path with spaces, symlink quirk, partial touch) — the exact failure class
# that produced esc-3468-75.
#
# Implementation:
#   - The bulk-stamp epoch is computed via `date -d 2020-01-01T00:00:00 +%s`,
#     matching the `touch -d 2020-01-01T00:00:00` interpretation (TZ-robust;
#     avoids hardcoding 1577836800 which is only correct under TZ=UTC).
#   - Re-run `git diff --name-only <sha>` (fail-closed on non-zero, mirroring
#     _touch_git_delta) and stat each existing path in the lane.
#   - Any path whose mtime equals the stale epoch → err naming the path + return 1.
#     Under set -e, return 1 aborts the seed before `echo "$LANE_TARGET"`,
#     leaving stdout empty → cold-fallback rebuild.
#
# Gated inside --fresh-checkout (the only mode that bulk-stamps) and on a
# non-empty sha (same gate as the _touch_git_delta caller).
_assert_no_stale_delta_stamp() {
    local sha="$1"
    local stale_epoch
    stale_epoch="$(date -d '2020-01-01T00:00:00' +%s)"
    local diff_out
    local diff_rc=0
    diff_out="$(git -C "$LANE_DIR" diff --name-only "$sha" 2>/dev/null)" || diff_rc=$?
    if [ "$diff_rc" -ne 0 ]; then
        err "_assert_no_stale_delta_stamp: git diff --name-only $sha failed (exit $diff_rc); failing closed"
        return 1
    fi
    local violations=0
    if [ -n "$diff_out" ]; then
        while IFS= read -r rel_path; do
            [ -z "$rel_path" ] && continue
            local abs_path="$LANE_DIR/$rel_path"
            [ -e "$abs_path" ] || continue
            local mtime
            mtime="$(stat -c '%Y' "$abs_path" 2>/dev/null || echo 0)"
            if [ "$mtime" -eq "$stale_epoch" ]; then
                err "Stale 2020-01-01 stamp detected on delta file after touch: $rel_path (esc-3468-75 regression)"
                violations=$((violations + 1))
            fi
        done <<< "$diff_out"
    fi
    if [ "$violations" -gt 0 ]; then
        err "_assert_no_stale_delta_stamp: $violations delta file(s) retain the 2020-01-01 stamp after delta-touch — seed aborted (cold rebuild forced)"
        return 1
    fi
    info "Post-condition OK: no stale 2020-01-01 stamp on delta file(s) from $sha"
}

# Touch every file in LANE_DIR listed by `git diff --name-only <sha>`.
# Fail-closed: a non-zero git diff exit aborts the seed (err + return 1 →
# set -e propagates → stdout stays empty → caller falls back to cold rebuild).
# An empty diff output is a legitimate zero-change result, NOT a failure.
_touch_git_delta() {
    local sha="$1"
    local count=0
    local diff_out
    local diff_rc=0
    diff_out="$(git -C "$LANE_DIR" diff --name-only "$sha" 2>/dev/null)" || diff_rc=$?
    if [ "$diff_rc" -ne 0 ]; then
        err "git diff --name-only $sha failed (exit $diff_rc); failing closed so the lane is rebuilt cold rather than seeded with a global 2020-stamp staleness (esc-3468-75)"
        return 1
    fi
    if [ -n "$diff_out" ]; then
        while IFS= read -r rel_path; do
            [ -z "$rel_path" ] && continue
            local abs_path="$LANE_DIR/$rel_path"
            if [ -e "$abs_path" ]; then
                touch "$abs_path"
                count=$((count + 1))
            fi
        done <<< "$diff_out"
    fi
    info "Touched $count git delta path(s) from $sha"
}

# ── main: record-base mode ────────────────────────────────────────────────────
if [ "$MODE" = "record-base" ]; then
    SIDECAR="$(_sidecar_path "$RECORD_BASE_DIR")"
    info "Recording base provenance at $SIDECAR ..."

    # Resolve base commit: prefer CLI --base-commit, else git rev-parse HEAD
    RESOLVED_BASE_COMMIT="${BASE_COMMIT:-}"
    if [ -z "$RESOLVED_BASE_COMMIT" ]; then
        RESOLVED_BASE_COMMIT="$(git -C "$RECORD_BASE_DIR" rev-parse HEAD 2>/dev/null || true)"
    fi

    # Write sidecar atomically (write to tmp, then move into place)
    SIDECAR_TMP="${SIDECAR}.tmp.$$"
    {
        printf 'RUSTFLAGS=%s\n' "${RUSTFLAGS:-}"
        printf 'INVOCATION=%s\n' "${REIFY_WARM_LANE_INVOCATION:-}"
        [ -n "$RESOLVED_BASE_COMMIT" ] && printf 'BASE_COMMIT=%s\n' "$RESOLVED_BASE_COMMIT"
    } > "$SIDECAR_TMP"
    mv "$SIDECAR_TMP" "$SIDECAR"

    ok "Base provenance recorded at $SIDECAR"
    # STDOUT contract: print sidecar path on success
    echo "$SIDECAR"
    exit 0
fi

# ── main: seed mode ───────────────────────────────────────────────────────────

info "seed-warm-lane.sh: base=$BASE_TARGET_DIR  lane=$LANE_DIR"

# ── Base-absent guard (distinct from reflink-unsupported; esc-triaged 2026-07-03) ──
# A MISSING CoW base (absent dir / removed-or-dangling base symlink / non-dir) is
# NOT a reflink-capability fault. Fail with a distinct code (76) and an accurate
# diagnostic BEFORE the clone so operators + DF's BASE_ABSENT discriminant are not
# sent down a reflink/filesystem dead-end (the actual cp error would be
# "cannot stat <base>/target: No such file or directory").
#
# MUST run before the sidecar read / RUSTFLAGS / invocation guards below: on a
# full teardown (base parent dir — and its sidecar — gone entirely, not just the
# target dir), _sidecar_read would see a missing sidecar and default both
# recorded values to "". Under a typical non-empty env RUSTFLAGS, that would
# make the RUSTFLAGS guard fire first with a misleading "RUSTFLAGS mismatch"
# message instead of the true "base is missing" cause — exactly the wrong signal
# for DF's BASE_ABSENT discriminant this guard exists to serve.
if [ ! -d "$BASE_TARGET_DIR" ]; then
    err "Warm base target dir absent/unresolvable: $BASE_TARGET_DIR"
    err "The warm base is missing — run scripts/refresh-warm-base.sh (or scripts/seed-warm-base-initial.sh for first standup) to (re)establish it."
    err "NOT a reflink-capability fault; the CoW base source does not exist."
    exit 76
fi

# ── read sidecar ──────────────────────────────────────────────────────────────
SIDECAR="$(_sidecar_path "$BASE_TARGET_DIR")"
RECORDED_RUSTFLAGS="$(_sidecar_read "$SIDECAR" "RUSTFLAGS")"
RECORDED_INVOCATION="$(_sidecar_read "$SIDECAR" "INVOCATION")"

# ── B5/D4: RUSTFLAGS guard (fail-closed, before any work) ────────────────────
ENV_RUSTFLAGS="${RUSTFLAGS:-}"
if [ "$ENV_RUSTFLAGS" != "$RECORDED_RUSTFLAGS" ]; then
    err "RUSTFLAGS mismatch: env RUSTFLAGS=${ENV_RUSTFLAGS@Q} but base recorded RUSTFLAGS=${RECORDED_RUSTFLAGS@Q}"
    err "The base artifact was built with different RUSTFLAGS — seeding would produce a cold rebuild."
    err "Re-build the warm base with matching RUSTFLAGS, or update the base sidecar via --record-base."
    exit 1
fi

# ── S1: invocation fingerprint guard (fail-closed, before any work) ──────────
ENV_INVOCATION="${REIFY_WARM_LANE_INVOCATION:-}"
if [ "$ENV_INVOCATION" != "$RECORDED_INVOCATION" ]; then
    err "Invocation mismatch: env REIFY_WARM_LANE_INVOCATION=${ENV_INVOCATION@Q} but base recorded INVOCATION=${RECORDED_INVOCATION@Q}"
    err "The base artifact was built with a different invocation fingerprint — seeding would produce a cold rebuild."
    err "Re-build the warm base with matching REIFY_WARM_LANE_INVOCATION, or update via --record-base."
    exit 1
fi

# ── --lane-lock (opt-in, default OFF): acquire-time lane-lock exclusivity ────
# PRD §9.5 inv.11. Acquires an EXCLUSIVE flock on the sibling-path
# ${LANE_DIR}.lock -- the SAME convention thin-warm-lane.sh's T3 (FD 9) and
# warm-lane-gc.sh's live-consumer probe (FD 8) already use -- BEFORE any
# target mutation (i.e. before the mode-split below / the fresh-checkout mv),
# and holds it across the whole run so the destructive replace+clone never
# races a live consumer (inv.2: one consumer per lane at a time). Runs before
# the mode-split so it guards BOTH --fresh-checkout and --reset-in-place.
#
# OPT-IN and default OFF: thin-warm-lane.sh --reseed ALREADY holds
# ${LANE_DIR}.lock on FD 9 before invoking this script, so an unconditional
# acquire here would self-refuse that caller. Every existing caller (thin
# --reseed, dark-factory acquire_lane pre-DF-wiring, tests) that does not pass
# --lane-lock is byte-for-byte unchanged.
if [ -n "$LANE_LOCK_OPT" ]; then
    LANE_LOCK="${LANE_DIR}.lock"
    # Mirrors thin-warm-lane.sh's breadcrumb: the pool's acquire/release
    # convention (inv.2) guarantees this lock file already exists for any
    # lane that went through acquire_lane; a missing lock here likely means
    # the lane never was.
    [ -e "$LANE_LOCK" ] || info "Lane lock does not exist yet, creating: $LANE_LOCK (lane may never have been acquired through the pool)"

    # REIFY_WARM_LANE_LANE_LOCK_WAIT (opt-in knob, default 0): a refused
    # acquirer of an ordinary task lane should just try a different FREE
    # lane (0 -> flock -n, non-blocking refuse) -- but the SINGLETON
    # _merge-verify lane has no alternate to fall back to, so it can QUEUE
    # instead: N>0 -> flock -w N (bounded queue, refuse on timeout);
    # "unlimited" (case-insensitive) -> flock (block until acquired, never
    # refuses). Validation mirrors lib_lane_x_flock.sh's
    # REIFY_LANE_X_FLOCK_WAIT gate (non-negative integer or "unlimited",
    # else exit 64/usage) and runs BEFORE the lock FD is even opened, so a
    # bad knob can never touch the target.
    LANE_LOCK_WAIT="${REIFY_WARM_LANE_LANE_LOCK_WAIT:-0}"
    _llw_unlimited=0
    case "$LANE_LOCK_WAIT" in
        [Uu][Nn][Ll][Ii][Mm][Ii][Tt][Ee][Dd]) _llw_unlimited=1 ;;
    esac
    if [ "$_llw_unlimited" -eq 0 ]; then
        # The '' alternative is defensive-only: LANE_LOCK_WAIT is assigned via
        # ${REIFY_WARM_LANE_LANE_LOCK_WAIT:-0}, and `:-` substitutes the
        # default for both unset AND empty, so LANE_LOCK_WAIT can never
        # actually be empty here -- kept in case that assignment ever changes.
        case "$LANE_LOCK_WAIT" in
            ''|*[!0-9]*)
                err "REIFY_WARM_LANE_LANE_LOCK_WAIT must be a non-negative integer or 'unlimited' (got '${LANE_LOCK_WAIT}')"
                exit 64
                ;;
        esac
    fi

    # Fixed FD 9 (matches thin-warm-lane.sh's T3 convention, not dynamically
    # allocated): this silently reassigns any FD 9 already open in this
    # process, so callers passing --lane-lock must not hold a load-bearing
    # FD 9 of their own across this invocation -- see the --lane-lock header
    # note above and thin-warm-lane.sh --reseed, the one caller that already
    # holds ${LANE_DIR}.lock on FD 9 itself and therefore omits --lane-lock.
    exec 9>"$LANE_LOCK"
    if [ "$_llw_unlimited" -eq 1 ]; then
        flock 9   # block until acquired -- never refuses, no exit-75 case
    elif [ "$LANE_LOCK_WAIT" = "0" ]; then
        if ! flock -n 9; then
            exec 9>&-
            err "Lane lock held by a live consumer (flock -n failed): $LANE_LOCK"
            err "Refusing to reseed an ASSIGNED lane (inv.2: one consumer per lane at a time)."
            exit 75
        fi
    else
        if ! flock -w "$LANE_LOCK_WAIT" 9; then
            exec 9>&-
            err "Lane lock still held by a live consumer after waiting ${LANE_LOCK_WAIT}s (flock -w timed out): $LANE_LOCK"
            err "Refusing to reseed an ASSIGNED lane (inv.2: one consumer per lane at a time)."
            exit 75
        fi
    fi
    unset _llw_unlimited
    # FD 9 stays open (lock held) for the rest of the run -- spanning the
    # mv+clone below with no check-then-act gap; bash releases it on exit.
fi

# ── mode-split: replace-existing (fresh-checkout) vs clobber-guard (reset-in-place) ──
LANE_TARGET="$LANE_DIR/target"
RESEED_TRASH=""

if [ -n "$FRESH_CHECKOUT" ]; then
    # ── Misuse guards (refuse BEFORE any rename; cp never reached on refusal) ──
    # Resolve paths once; used by both guard checks below.
    _rp_base_target="$(realpath -m "$BASE_TARGET_DIR")"
    _rp_lane_target="$(realpath -m "$LANE_TARGET")"
    _rp_lane_dir="$(realpath -m "$LANE_DIR")"

    # Under-mount guard: when REIFY_WARM_LANE_MOUNT is set, LANE_TARGET must be
    # under the mount root.  Trailing-slash prefix compare prevents a sibling path
    # like /mnt/warm-lanes-evil from falsely matching /mnt/warm-lanes.
    # Gated on the env being set so hermetic /tmp test fixtures are unaffected.
    if [ -n "${REIFY_WARM_LANE_MOUNT:-}" ]; then
        _rp_mount="$(realpath -m "$REIFY_WARM_LANE_MOUNT")"
        case "$_rp_lane_target/" in
            "$_rp_mount"/*) ;;
            *)
                err "Misuse guard: LANE_DIR/target is not under REIFY_WARM_LANE_MOUNT"
                err "  LANE_TARGET: $_rp_lane_target"
                err "  REIFY_WARM_LANE_MOUNT (canonicalized): $_rp_mount"
                err "  The --fresh-checkout replace path is restricted to the warm-lane mount."
                exit 1
                ;;
        esac
    fi

    # Self-clobber guard (unconditional within --fresh-checkout; not gated on
    # REIFY_WARM_LANE_MOUNT): refuse if LANE_TARGET or LANE_DIR resolves to
    # BASE_TARGET_DIR (exact equality), OR if either party is an ancestor/
    # descendant of the other (nesting relationship) — a nesting match means
    # `mv "$LANE_TARGET" "$RESEED_TRASH"` would relocate the live warm base
    # into trash and the subsequent rm -rf would destroy it.
    _self_clobber=0
    if [ "$_rp_lane_target" = "$_rp_base_target" ] || \
       [ "$_rp_lane_dir" = "$_rp_base_target" ]; then
        _self_clobber=1
    fi
    # Nesting: base is under LANE_TARGET (LANE_TARGET is a parent of base)
    case "$_rp_base_target/" in
        "$_rp_lane_target"/*) _self_clobber=1 ;;
    esac
    # Nesting: LANE_TARGET is under base (base is a parent of LANE_TARGET)
    case "$_rp_lane_target/" in
        "$_rp_base_target"/*) _self_clobber=1 ;;
    esac
    if [ "$_self_clobber" = "1" ]; then
        err "Misuse guard: LANE_TARGET or LANE_DIR resolves to or nests with BASE_TARGET_DIR (self-clobber)"
        err "  LANE_TARGET: $_rp_lane_target"
        err "  LANE_DIR: $_rp_lane_dir"
        err "  BASE_TARGET_DIR: $_rp_base_target"
        err "  Renaming the base to trash and cloning onto it would destroy the warm base."
        exit 1
    fi

    # --fresh-checkout: replace-existing semantics (D10 always-re-seed-at-acquire).
    # If LANE_TARGET is non-empty, atomically rename it to a pool-level trash sidecar
    # at dirname(LANE_DIR)/.reseed-trash/basename(LANE_DIR).$$ BEFORE cloning.
    #
    # Crash-safe ordering: rename-then-clone-then-rm ensures a crash leaves a
    # recoverable trash dir, never a half-seeded target.
    #
    # WHY THE SIBLING PATH (#4896, esc-4892-99):
    #   1. SAME XFS MOUNT — dirname(LANE_DIR) already holds the lane on the same
    #      filesystem, so `mv` stays a pure atomic rename (a cross-FS path would
    #      silently degrade mv to a slow non-atomic copy+delete).
    #   2. STRUCTURALLY INVISIBLE TO ALL LANE-ROOTED WALKERS — the trash is outside
    #      LANE_DIR, so DF's `git clean -xfd -e target`, our find bulk-stamp, and cargo
    #      never descend into it.  This generalises the per-walker task-4715 prune to
    #      structural invisibility and removes the cross-repo coupling whereby DF must
    #      know reify's trash naming.
    #   3. DOT-PREFIXED PARENT — warm-lane-gc.sh enumerates lanes via `$WORKTREES_DIR/*/`;
    #      bash `*/` does not match leading-dot entries, so `.reseed-trash/` is never
    #      mistaken for a lane or orphan candidate.
    if [ -d "$LANE_TARGET" ] && [ -n "$(ls -A "$LANE_TARGET" 2>/dev/null)" ]; then
        RESEED_TRASH_DIR="$(dirname "$LANE_DIR")/.reseed-trash"
        mkdir -p "$RESEED_TRASH_DIR"
        # Guard: RESEED_TRASH_DIR must be on the same device as LANE_DIR (#4896).
        # provision-warm-lane-fs.sh guarantees lanes are plain directories under the
        # XFS warm-lane mount — never separate mount points — so the device numbers
        # always match.  If they diverge (operator misconfiguration where LANE_DIR is
        # itself a mount point), `mv LANE_TARGET RESEED_TRASH` would silently degrade
        # to a slow non-atomic copy+delete, violating the crash-safe rename→clone→rm
        # ordering; abort instead so DF can cold-fallback or escalate.
        _rp_lane_dev="$(stat -c '%d' "$LANE_DIR" 2>/dev/null || true)"
        _rp_trash_dev="$(stat -c '%d' "$RESEED_TRASH_DIR" 2>/dev/null || true)"
        if [ -n "$_rp_lane_dev" ] && [ -n "$_rp_trash_dev" ] && \
           [ "$_rp_lane_dev" != "$_rp_trash_dev" ]; then
            err "same-FS check FAILED: LANE_DIR ($LANE_DIR, dev=$_rp_lane_dev) and"
            err "  RESEED_TRASH_DIR ($RESEED_TRASH_DIR, dev=$_rp_trash_dev) are on different filesystems."
            err "  mv LANE_TARGET→RESEED_TRASH cannot be atomic; aborting seed."
            err "  Resolution: ensure LANE_DIR is a plain dir, not a mount point (provision-warm-lane-fs.sh contract)."
            exit 1
        fi
        unset _rp_lane_dev _rp_trash_dev
        # Sweep orphaned trash entries for this lane left by a prior crash/SIGKILL.
        # Inv.2 (one consumer per lane at a time) guarantees any pre-existing
        # <lane>.<pid> entry under RESEED_TRASH_DIR cannot be from a concurrent live
        # seed — it is always a prior-crash orphan and is safe to reclaim now.
        # Background rm mirrors the main rm (large tree, must not block acquire).
        # 9<&-: close the (possibly held, --lane-lock) exclusive lane-lock FD
        # before backgrounding so a detached child never inherits it -- the
        # lock must release exactly when seed exits, not whenever this rm
        # happens to finish (lib_slot_acquire.sh daemon-FD-inheritance guard).
        # A no-op when FD 9 was never opened (--lane-lock not passed).
        while IFS= read -r -d '' _rp_orphan; do
            warn "Sweeping orphaned trash entry (prior-crash recovery): $_rp_orphan"
            { rm -rf "$_rp_orphan" || warn "orphan trash sweep rm failed (leaked): $_rp_orphan"; } 9<&- &
        done < <(find "$RESEED_TRASH_DIR" -maxdepth 1 -name "$(basename "$LANE_DIR").*" -print0 2>/dev/null)
        unset _rp_orphan
        RESEED_TRASH="$RESEED_TRASH_DIR/$(basename "$LANE_DIR").$$"
        info "Renaming non-empty $LANE_TARGET → $RESEED_TRASH before re-seed ..."
        mv "$LANE_TARGET" "$RESEED_TRASH"
    fi
else
    # --reset-in-place: keep existing clobber-refusal (B13 warmth-delta control arm).
    # reset-in-place is a test-only path; production acquires always use --fresh-checkout.
    if [ -d "$LANE_TARGET" ] && [ -n "$(ls -A "$LANE_TARGET" 2>/dev/null)" ]; then
        err "Clobber guard: <lane_dir>/target already exists and is non-empty: $LANE_TARGET"
        err "seed-warm-lane.sh --reset-in-place only seeds cold/empty lanes. Remove the lane first."
        exit 1
    fi
fi

# Remove an empty lane target/ if present (cp -a SRC DEST requires DEST to not exist
# to create DEST as a copy of SRC; otherwise it creates DEST/basename(SRC))
[ -d "$LANE_TARGET" ] && rmdir "$LANE_TARGET" 2>/dev/null || true

info "Cloning $BASE_TARGET_DIR → $LANE_TARGET (--reflink=always) ..."
if ! cp -a --reflink=always "$BASE_TARGET_DIR" "$LANE_TARGET"; then
    err "Reflink clone FAILED: cp -a --reflink=always $BASE_TARGET_DIR $LANE_TARGET"
    err "The filesystem does not support reflinks — seeding aborted (S2: no silent full-copy fallback)."
    exit 1
fi
info "Clone complete: $LANE_TARGET"

# ── mtime normalization (D5) ──────────────────────────────────────────────────
if [ -n "$FRESH_CHECKOUT" ]; then
    # Bulk-stamp all sources to 2020-01-01T00:00:00, pruning target/, .git/, and
    # target.reseed-trash.* so only the delta closure needs recompilation.
    info "Stamping sources to 2020-01-01 (pruning target/, .git/, and reseed trash) ..."
    # touch -h (no-dereference): a checked-out worktree may contain tracked
    # RELATIVE symlinks (e.g. config/usage-accounts.yaml -> ../../dark-factory/...)
    # that resolve from the repo root but dangle inside a lane at a different
    # depth.  Without -h, touch follows the link and fails ("No such file"),
    # aborting the whole seed -> cold fallback.  -h stamps the symlink itself.
    # target.reseed-trash.* is pruned as DEFENSE-IN-DEPTH (task 4715/4896).
    # PRIMARY protection (#4896): trash is now at the pool-level sibling
    #   dirname(LANE_DIR)/.reseed-trash/basename(LANE_DIR).PID, so it is
    #   structurally outside LANE_DIR and this prune matches nothing for new seeds.
    # LEGACY defense: the prune still guards against any pre-#4896 in-lane trash
    #   (target.reseed-trash.*) left by an older seed during the migration window,
    #   and against any future regression that re-introduces in-lane trash:
    #   (1) avoids wasteful stamping of the ~227 MB old-lane tree
    #   (2) avoids find descending into a tree concurrently deleted by `rm -rf &`;
    #       a touch/lstat on an rm-unlinked path exits non-zero under set -euo
    #       pipefail, aborting the seed → cold fallback (async-trash race, task 4715)
    find "$LANE_DIR" -mindepth 1 \
        \( -path "$LANE_DIR/target" \
           -o -path "$LANE_DIR/.git" \
           -o -path "$LANE_DIR/target.reseed-trash.*" \) -prune \
        -o -exec touch -h -d "2020-01-01T00:00:00" {} +

    # Touch the delta to now: explicit --touch paths first
    if [ "${#TOUCH_PATHS[@]}" -gt 0 ]; then
        info "Touching ${#TOUCH_PATHS[@]} explicit delta path(s) to now ..."
        touch "${TOUCH_PATHS[@]}"
    fi

    # Resolve the delta-touch base commit with 3-tier priority (esc-3468-75):
    #   1. CLI --base-commit (highest trust: caller is explicit)
    #   2. <base_target_dir>.basecommit (authoritative, refresh-written, gen-bound,
    #      TOCTOU-free; see refresh-warm-base.sh Step 4b)
    #   3. .warm-base-meta BASE_COMMIT (legacy fallback; drift-prone)
    # An empty result means no base is known → no delta-touch (Block D unchanged).
    EFFECTIVE_BASE_COMMIT=""
    if [ -n "$BASE_COMMIT" ]; then
        EFFECTIVE_BASE_COMMIT="$BASE_COMMIT"
        # Tier 1 (CLI --base-commit): source is self-evident; logged below.
    else
        EFFECTIVE_BASE_COMMIT="$(_read_basecommit_stamp "$BASE_TARGET_DIR")"
        if [ -n "$EFFECTIVE_BASE_COMMIT" ]; then
            # Tier 2: authoritative per-gen stamp (refresh-written, TOCTOU-free).
            info "delta-touch base from authoritative .basecommit: $EFFECTIVE_BASE_COMMIT"
        else
            EFFECTIVE_BASE_COMMIT="$(_sidecar_read "$SIDECAR" "BASE_COMMIT")"
            if [ -n "$EFFECTIVE_BASE_COMMIT" ]; then
                # Tier 3: legacy fallback.  Stamp absent means either a pre-fix base
                # (refresh has not yet written Step 4b) or the caller passed an
                # unresolved symlink instead of the concrete .gen.N path (D8 seam
                # contract violation).  Either way, this is diagnosable from logs.
                warn "delta-touch base from legacy .warm-base-meta BASE_COMMIT (authoritative stamp absent — caller may have passed an unresolved symlink): $EFFECTIVE_BASE_COMMIT"
            fi
        fi
    fi

    if [ -n "$EFFECTIVE_BASE_COMMIT" ]; then
        info "Touching git diff --name-only $EFFECTIVE_BASE_COMMIT paths to now ..."
        _touch_git_delta "$EFFECTIVE_BASE_COMMIT"
        # Seed-time post-condition (inv.9 defense-in-depth): after the delta-touch,
        # no tracked file listed by git diff may still carry the 2020-01-01 bulk-stamp
        # epoch. Violations abort the seed (fail-closed → stdout empty → cold rebuild).
        _assert_no_stale_delta_stamp "$EFFECTIVE_BASE_COMMIT"
    fi

    # ── non-relocatable build-script output-dir invalidation ──────────────────
    # tauri (links = "Tauri") bakes absolute paths into `links` metadata via
    # cargo:...PERMISSION_FILES_PATH=<abs>/out/tauri-core-*-permission-files.
    # Cargo turns these into DEP_TAURI_*_PERMISSION_FILES_PATH env vars that
    # reify-gui's tauri-build ACL codegen opens as files.  After a CoW clone
    # from _merge-verify, those paths still point at _merge-verify (which gets
    # refreshed/cleaned) → ENOENT in the lane, even though the .toml files
    # exist at the correct _lane-K path.
    #
    # Fix: delete only the build-script output dirs whose scripts bake such
    # non-relocatable absolute paths consumed by DEPENDENT build scripts.
    # This forces cargo to re-RUN their build scripts (cheap, seconds), re-
    # baking correct lane paths, while the expensive rlib compiles stay Fresh
    # (path-independent fingerprint, PRD spike §4/§6.1).
    #
    # Allow-list globs (tauri-* covers tauri core + tauri-plugin-* + tauri-runtime*).
    # MUST be single-quoted to defer pathname expansion to the glob site below;
    # without quotes, bash expands tauri-* / reify-gui-* against the CWD at
    # assignment time — silently replacing the literal patterns with any CWD
    # matches and invalidating 0 dirs (re-introducing the ENOENT bug, no error).
    #
    # MAINTENANCE: when a workspace crate gains `links = "..."` in Cargo.toml AND
    # its build script emits absolute paths into cargo metadata consumed by dependent
    # build scripts (e.g. `cargo:MY_KEY=/abs/path/to/out/file`), add its package-
    # name prefix glob here.  Omitting it lets the stale cross-lane absolute path
    # survive verbatim in the CoW-cloned `output` file; cargo treats the build
    # script as Fresh (path-independent fingerprint, PRD spike §4/§6.1) → ENOENT
    # in the lane once the base is refreshed/cleaned.
    _NONRELOCATABLE_BUILD_GLOBS=('tauri-*' 'reify-gui-*')
    _invalidated_count=0
    # -maxdepth 3: covers depth-2 profile build dirs (debug/build, release/build)
    # and depth-3 cross-compile dirs (<triple>/debug/build, <triple>/release/build).
    # Depths 4+ are nested build/ dirs inside build-script out/ subdirs — not
    # cargo profile build dirs — intentionally excluded (false-invalidation risk).
    while IFS= read -r -d '' _build_dir; do
        for _glob in "${_NONRELOCATABLE_BUILD_GLOBS[@]}"; do
            for _d in "$_build_dir"/$_glob; do
                # [ -e ] guard: if the glob matches nothing, the shell expands it
                # to the literal pattern string; skip instead of rm-ing a literal.
                [ -e "$_d" ] || continue
                rm -rf "$_d"
                _invalidated_count=$((_invalidated_count + 1))
            done
        done
    done < <(find "$LANE_TARGET" -maxdepth 3 -type d -name build -print0)
    info "Invalidated $_invalidated_count non-relocatable build-script output dir(s) so cargo re-bakes lane-correct paths"

    # Base build-worktree stamp (refresh-warm-base.sh Step 4b), shared by the
    # relocation sweep below and the env!()-relink gate further down.
    _recorded_buildroot="$(_read_buildroot_stamp "$BASE_TARGET_DIR")"
    _lane_rp="$(realpath -m "$LANE_DIR")"

    # ── non-relocatable links-metadata/OUT_DIR path relocation (task 5126) ────
    # cxx/reify-kernel-occt/reify-kernel-openvdb/libsqlite3-sys/zstd-sys/etc. sit
    # outside the _NONRELOCATABLE_BUILD_GLOBS allow-list above, so the foreign
    # buildroot baked into two build-script replay files survives the CoW seed
    # verbatim -> ENOENT once the base's own worktree is refreshed or cleaned:
    #   `output`      — links-metadata cargo emits, e.g. cxx's
    #                   cargo:CXXBRIDGE_DIR0=<foreign>/target/.../out/cxxbridge/include
    #   `root-output` — the build script's OUT_DIR, replayed so
    #                   include!(concat!(env!("OUT_DIR"), ...)) opens the right dir
    # Rewrite the foreign prefix to this lane's root in place rather than
    # deleting the build dir: the CoW copy already placed identical out/
    # content at the lane-relative path, so relocating keeps the compiled
    # rlib/.o/.a Fresh (path-independent fingerprint) instead of forcing a
    # multi-minute native/C++ rebuild.
    # Gate (distinct from the env!()-relink gate below): an ABSENT stamp means
    # the foreign prefix is UNKNOWN, so this skips with a warn rather than
    # relinking-on-uncertainty like the env!() case — an empty search prefix
    # would match every byte of every candidate file and corrupt it, whereas
    # the env!() relink's fail-safe action (a bare `touch`) has no such risk.
    #
    # SCOPE BOUNDARY (deliberate, not exhaustive): only files NAMED `output` or
    # `root-output` are candidates — a filename filter, not a content scan — so
    # a sibling `.d` depfile or a compiled `.o`/`.a`/`.rlib` that happens to
    # also contain the foreign prefix is NEVER touched, even though it exists
    # in the same build dir. `.d` depfiles are advisory (a stale entry forces
    # at most a localized recompile, never ENOENT); binaries are already
    # compiled and path-independent, and rewriting their bytes is a corruption
    # risk with no upside. This mirrors the _NONRELOCATABLE_BUILD_GLOBS
    # allow-list's philosophy of a narrow, explicit surface rather than a
    # broad one. MAINTENANCE: if cargo ever bakes a foreign-path-carrying
    # replay file under a NEW name (beyond `output`/`root-output`), add it to
    # the `-name` clause below rather than widening this into a content scan.
    if [ -z "$_recorded_buildroot" ]; then
        warn "buildroot stamp absent (${BASE_TARGET_DIR}.buildroot not found) — cannot relocate baked links-metadata/OUT_DIR path(s); an empty search prefix would match every byte and corrupt files, so skipping rather than guessing. Re-run scripts/refresh-warm-base.sh to (re)write the stamp."
    else
        _foreign_rp="$(realpath -m "$_recorded_buildroot")"
        if [ "$_foreign_rp" != "$_lane_rp" ]; then
            _relocate_search_esc="$(_sed_escape "$_foreign_rp")"
            _relocate_replace_esc="$(_sed_escape "$_lane_rp")"
            _relocate_candidate_count=0
            _relocated_count=0
            # -maxdepth 5: output/root-output live at target/<profile>/build/<pkg>-<hash>/{output,root-output}
            # (depth 4) or, for cross-compiled targets, target/<triple>/<profile>/build/<pkg>-<hash>/{output,root-output}
            # (depth 5) — mirrors the -maxdepth 3 bound on the tauri/reify-gui deletion sweep's directory
            # walk above, applied one filename-match level deeper here since this find locates leaf files
            # rather than `build` dirs. Bounding this avoids a full unbounded descent into every out/
            # subdir and incremental-fingerprint dir under the (potentially huge) warm target tree.
            while IFS= read -r -d '' _rl_file; do
                _relocate_candidate_count=$((_relocate_candidate_count + 1))
                if grep -qF "$_foreign_rp" "$_rl_file" 2>/dev/null; then
                    sed -E -i "s/${_relocate_search_esc}/${_relocate_replace_esc}/g" "$_rl_file"
                    _relocated_count=$((_relocated_count + 1))
                fi
            done < <(find "$LANE_TARGET" -maxdepth 5 -type f \( -name output -o -name root-output \) -print0)
            if [ "$_relocated_count" -eq 0 ] && [ "$_relocate_candidate_count" -gt 0 ]; then
                # Candidates exist (cargo DID write output/root-output files) yet NONE carried the
                # recorded foreign prefix, even though the buildroot gate says foreign != lane. That
                # combination is only expected if the .buildroot stamp and the bytes actually baked into
                # these files were canonicalized inconsistently (e.g. the base was built through a
                # symlinked worktree path while the stamp records the realpath, or vice-versa) — grep -qF
                # would then silently match nothing on every candidate, nothing gets rewritten, and the
                # foreign path(s) survive verbatim, reproducing the exact ENOENT this fix targets. Warn
                # instead of a bare "Relocated 0" so canonicalization drift surfaces instead of reading
                # as success.
                warn "Relocated 0 of $_relocate_candidate_count links-metadata/OUT_DIR candidate file(s) even though recorded buildroot ($_foreign_rp) differs from this lane ($_lane_rp) — none contained the expected foreign prefix. This may mean the .buildroot stamp and the baked paths were canonicalized inconsistently (e.g. symlinked vs realpath worktree root); if so, the foreign path(s) will survive uncorrected and the lane may hit ENOENT. Re-check the stamp written by refresh-warm-base.sh."
            else
                info "Relocated $_relocated_count of $_relocate_candidate_count links-metadata/OUT_DIR candidate file(s): foreign buildroot ($_foreign_rp) -> this lane ($_lane_rp)"
            fi
        else
            info "Skipping links-metadata/OUT_DIR relocation: recorded buildroot matches this lane ($_lane_rp)"
        fi
    fi

    # ── non-relocatable env!()-baked-path test/bench relink (task 4983) ───────
    # A TEST or BENCH source can bake an absolute worktree path at compile time
    # via a cargo-internal env!() macro (CARGO_MANIFEST_DIR, OUT_DIR,
    # CARGO_TARGET_TMPDIR, CARGO_BIN_EXE_*).  These macros are NOT part of
    # cargo's fingerprint, so after a CoW clone from a base built under a
    # DIFFERENT worktree, the frozen 2020-01-01 source mtime makes cargo treat
    # the test/bench binary as Fresh and it keeps serving the BASE's baked
    # path — a deterministic runtime NotFound (Cargo.toml missing) once that
    # worktree is deleted or holds different content (esc-4906-57; confirmed
    # manual fix: touching the offending sources forces cargo to recompile and
    # relink).
    #
    # Fix: touch (to now) only the seeded tests/ and benches/ .rs sources that
    # contain one of these macros, forcing cargo to recompile+relink ONLY those
    # integration/bench test binaries (each is its own compilation-unit root —
    # no lib rlib cascade).  src/ unit-test env!() usage is an accepted,
    # documented limitation (touching a src/ file would force a lib recompile
    # and cascade to every downstream dependent — expensive and unobserved).
    #
    # MAINTENANCE: extend this regex if a new cargo-internal path-baking env!()
    # macro is identified (mirrors the _NONRELOCATABLE_BUILD_GLOBS allow-list
    # convention above).  Currently covers the CARGO_* macros known to bake
    # absolute build-time paths into compiled test/bench binaries.
    _ENV_PATH_MACRO_RE='env!\("(CARGO_MANIFEST_DIR|OUT_DIR|CARGO_TARGET_TMPDIR|CARGO_BIN_EXE_[A-Za-z0-9_]+)"\)'

    # ── buildroot-match gate ───────────────────────────────────────────────
    # Compare the base's recorded build-worktree (refresh-warm-base.sh Step 4b
    # .buildroot stamp) against THIS consuming lane.  In production the base
    # is always built under _merge-verify while the consuming lane is
    # _lane-K, so these realpaths always DIFFER and the relink below fires on
    # every acquire — correctly, because the baked path is never the lane's
    # own, and trusting that the base's original build worktree still exists
    # with identical content is exactly the esc-4906-57 failure mode (that
    # worktree gets refreshed or deleted out from under the stale baked
    # path). ABSENT (no stamp — pre-fix base, or refresh has not yet run the
    # Step 4b write) fails safe and relinks too, since the baked path's
    # provenance is unknown. Only an EXACT match (the base was built under
    # this lane's own path) skips the relink as a genuine no-op.
    # (_recorded_buildroot/_lane_rp already computed above, shared with the
    # links-metadata/OUT_DIR relocation sweep.)
    if [ -z "$_recorded_buildroot" ] || [ "$(realpath -m "$_recorded_buildroot")" != "$_lane_rp" ]; then
        _relinked_count=0
        while IFS= read -r -d '' _rs_file; do
            # grep -q exits 1 on no-match; under set -euo pipefail a bare/&&-
            # chained call would abort the seed on the (common) no-match
            # case, so guard it in an `if` instead.
            if grep -qE "$_ENV_PATH_MACRO_RE" "$_rs_file" 2>/dev/null; then
                touch "$_rs_file"
                _relinked_count=$((_relinked_count + 1))
            fi
        done < <(find "$LANE_DIR" -type f -name '*.rs' \
                      \( -path '*/tests/*' -o -path '*/benches/*' \) \
                      -not -path "$LANE_DIR/target/*" -print0)
        info "Relinked $_relinked_count env!()-baked-path test/bench source(s) so cargo re-bakes the lane's own CARGO_MANIFEST_DIR (recorded buildroot=${_recorded_buildroot:-<absent>}, lane=$_lane_rp)"
    else
        info "Skipping env!()-baked-path relink: recorded buildroot matches this lane ($_lane_rp)"
    fi

    # Remove the reseed trash AFTER all find walks of LANE_DIR are complete.
    # Deferring to here (rather than immediately after the cp clone) prevents the
    # concurrent find/rm race: the find above prunes target.reseed-trash.* so it
    # never descends into the trash, but rm still starts only once every find walk
    # of LANE_DIR has finished — eliminating even the residual lstat-on-trash-dir
    # race that the prune alone would leave open (task 4715 async-trash fix).
    # On cp failure RESEED_TRASH is unset (no rename happened), so this block is skipped.
    # Background by default (production: large lane rm must not block acquire).
    # Foreground when REIFY_WARM_LANE_RESEED_TRASH_SYNC=1 (test-determinism knob).
    # 9<&-: close the (possibly held, --lane-lock) exclusive lane-lock FD
    # before backgrounding so a detached child never inherits it -- the lock
    # must release exactly when seed exits, not whenever this rm happens to
    # finish (lib_slot_acquire.sh daemon-FD-inheritance guard). No-op when
    # FD 9 was never opened (--lane-lock not passed); the SYNC (foreground)
    # branch needs no change -- it completes before seed exits either way.
    if [ -n "$RESEED_TRASH" ] && [ -d "$RESEED_TRASH" ]; then
        info "Removing reseed trash: $(basename "$RESEED_TRASH") ..."
        if [ "${REIFY_WARM_LANE_RESEED_TRASH_SYNC:-}" = "1" ]; then
            rm -rf "$RESEED_TRASH"
        else
            { rm -rf "$RESEED_TRASH" || warn "reseed trash rm failed (leaked): $RESEED_TRASH"; } 9<&- &
        fi
    fi
fi
# --reset-in-place: no bulk stamp AND no build-dir invalidation.
#   reset-in-place is a test-only control arm (B13 warmth-delta test) whose lane
#   was built at its own path — build dirs already hold correct lane-K paths.
#   Invalidating them would waste build-script re-runs for no benefit.
#   Per D10 always-re-seed-at-acquire: production acquires (task lanes AND
#   merge-spec slots) ALWAYS use --fresh-checkout, so the invalidation above
#   covers both lane classes without extra code.

ok "Warm lane seeded at $LANE_TARGET"
echo "$LANE_TARGET"
