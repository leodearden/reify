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
#                    [--base-commit <sha>] [--touch <path>]...)
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
      [--base-commit <sha>] [--touch <path>]...
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

    # Remove the reseed trash AFTER all find walks of LANE_DIR are complete.
    # Deferring to here (rather than immediately after the cp clone) prevents the
    # concurrent find/rm race: the find above prunes target.reseed-trash.* so it
    # never descends into the trash, but rm still starts only once every find walk
    # of LANE_DIR has finished — eliminating even the residual lstat-on-trash-dir
    # race that the prune alone would leave open (task 4715 async-trash fix).
    # On cp failure RESEED_TRASH is unset (no rename happened), so this block is skipped.
    # Background by default (production: large lane rm must not block acquire).
    # Foreground when REIFY_WARM_LANE_RESEED_TRASH_SYNC=1 (test-determinism knob).
    if [ -n "$RESEED_TRASH" ] && [ -d "$RESEED_TRASH" ]; then
        info "Removing reseed trash: $(basename "$RESEED_TRASH") ..."
        if [ "${REIFY_WARM_LANE_RESEED_TRASH_SYNC:-}" = "1" ]; then
            rm -rf "$RESEED_TRASH"
        else
            { rm -rf "$RESEED_TRASH" || warn "reseed trash rm failed (leaked): $RESEED_TRASH"; } &
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
