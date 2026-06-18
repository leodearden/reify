#!/usr/bin/env bash
# scripts/seed-warm-lane.sh — CoW clone + warmth-transfer helper for warm-lane pool.
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
#   A pre-existing non-empty <lane_dir>/target is refused (clobber guard).
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

# ── locate script dir / repo root ─────────────────────────────────────────────
_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$_SCRIPT_DIR/.." && pwd)"

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
  --fresh-checkout    Bulk-stamp sources to 2020-01-01, touch changed files to now (D5).
  --reset-in-place    No bulk stamp; git clean already moved changed mtimes.
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
    dirname "$base_target_dir" | xargs -I{} echo "{}/.warm-base-meta"
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

# ── main: record-base mode ────────────────────────────────────────────────────
if [ "$MODE" = "record-base" ]; then
    # Implemented in step-14 (GREEN for Block G)
    err "--record-base not yet implemented"
    exit 1
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
# Implemented in step-12 (GREEN for Block F)

# ── clobber guard + reflink clone (S2) ───────────────────────────────────────
LANE_TARGET="$LANE_DIR/target"

# Clobber guard: refuse a pre-existing non-empty lane target
# (Fully hardened in step-6 / Block C; here: basic check)
if [ -d "$LANE_TARGET" ] && [ -n "$(ls -A "$LANE_TARGET" 2>/dev/null)" ]; then
    err "Clobber guard: <lane_dir>/target already exists and is non-empty: $LANE_TARGET"
    err "seed-warm-lane.sh only seeds cold/empty lanes. Remove the lane first."
    exit 1
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
    # Bulk-stamp all sources to 2020-01-01T00:00:00, pruning target/ and .git/
    # so only the delta closure needs recompilation.
    info "Stamping sources to 2020-01-01 (pruning target/ and .git/) ..."
    find "$LANE_DIR" \
        -mindepth 1 \
        -not -path "$LANE_DIR/target" \
        -not -path "$LANE_DIR/target/*" \
        -not -path "$LANE_DIR/.git" \
        -not -path "$LANE_DIR/.git/*" \
        -exec touch -d "2020-01-01T00:00:00" {} +

    # Touch the delta to now: explicit --touch paths first
    if [ "${#TOUCH_PATHS[@]}" -gt 0 ]; then
        info "Touching ${#TOUCH_PATHS[@]} explicit delta path(s) to now ..."
        touch "${TOUCH_PATHS[@]}"
    fi

    # Touch the delta from git diff --name-only when a base commit is known
    if [ -n "$BASE_COMMIT" ]; then
        info "Touching git diff --name-only $BASE_COMMIT paths to now ..."
        while IFS= read -r rel_path; do
            [ -z "$rel_path" ] && continue
            abs_path="$LANE_DIR/$rel_path"
            [ -e "$abs_path" ] && touch "$abs_path"
        done < <(git -C "$LANE_DIR" diff --name-only "$BASE_COMMIT" 2>/dev/null || true)
    fi

    # If sidecar recorded a BASE_COMMIT and none was passed on CLI, use the sidecar one
    if [ -z "$BASE_COMMIT" ]; then
        RECORDED_BASE_COMMIT="$(_sidecar_read "$SIDECAR" "BASE_COMMIT")"
        if [ -n "$RECORDED_BASE_COMMIT" ]; then
            info "Using sidecar BASE_COMMIT=$RECORDED_BASE_COMMIT for git diff ..."
            while IFS= read -r rel_path; do
                [ -z "$rel_path" ] && continue
                abs_path="$LANE_DIR/$rel_path"
                [ -e "$abs_path" ] && touch "$abs_path"
            done < <(git -C "$LANE_DIR" diff --name-only "$RECORDED_BASE_COMMIT" 2>/dev/null || true)
        fi
    fi
fi
# --reset-in-place: no bulk stamp (git clean -xfd -e target already moved changed mtimes)

ok "Warm lane seeded at $LANE_TARGET"
echo "$LANE_TARGET"
