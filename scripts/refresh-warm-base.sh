#!/usr/bin/env bash
# scripts/refresh-warm-base.sh — Atomically refresh the warm-lane CoW pool base
# from an advancing target directory using XFS reflinks.
#
# Usage:
#   scripts/refresh-warm-base.sh <advancing_target_dir> <base_dir> [OPTIONS]
#   scripts/refresh-warm-base.sh --check-frag <base_dir> [--frag-threshold N]
#
# Positional (normal mode):
#   <advancing_target_dir>   Source dir to reflink-copy from (e.g. Cargo target/).
#   <base_dir>               Destination base dir (e.g. /warm-lanes/base/target).
#
# Options:
#   --check-frag             Read-only defrag check: print verdict token (ok |
#                            reseed-due) + max per-file extent count to stdout;
#                            performs NO refresh. Stdout token contract: "ok N" or
#                            "reseed-due N" where N is the max extent count seen.
#                            exit 0 on a successful check; non-zero on hard errors
#                            (xfs_bmap missing, base_dir missing).
#   --frag-threshold N       Extent threshold for --check-frag (default: 64).
#   --rustflags VALUE        RUSTFLAGS stamp to write after swap (default: ${RUSTFLAGS:-}).
#   --invocation FP          Invocation fingerprint stamp to write after swap (default: '').
#   -h, --help               Print this message and exit.
#
# Stdout:  empty on the refresh path; "ok N" or "reseed-due N" on --check-frag.
#          All other output goes to stderr.
# Stderr:  all progress messages and errors.
#
# Refresh mechanics:
#   1. cp -a --reflink=always <advancing_target_dir> <base_dir>.new
#      (fail-closed: --reflink=always, never auto; non-reflink host → non-zero exit)
#   2. Atomic rename:
#      - if <base_dir> exists:  mv -T <base_dir> <base_dir>.old
#                               mv -T <base_dir>.new <base_dir>
#                               rm -rf <base_dir>.old
#      - if <base_dir> absent:  mv -T <base_dir>.new <base_dir>
#   3. Write self-description stamps: <base_dir>.rustflags, <base_dir>.invocation
#
# In-flight clone independence (B6): XFS extent refcounting means in-flight clones
# already taken are fully independent of the base swap. No drain protocol needed.
#
# Sidecar stamp convention: <base_dir>.rustflags and <base_dir>.invocation are
# adjacent to the base dir (sibling files, NOT inside the dir). warm-lane-preflight.sh
# reads them; seed-warm-lane.sh (sibling β) reads the same paths.

set -euo pipefail

# ── log helpers (all write to stderr) ─────────────────────────────────────────
info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*" >&2; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*" >&2; }
err()   { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }

# ── usage ──────────────────────────────────────────────────────────────────────
_usage() {
    cat >&2 <<EOF
Usage: $(basename "$0") <advancing_target_dir> <base_dir> [OPTIONS]
       $(basename "$0") --check-frag <base_dir> [--frag-threshold N]

  Atomically refresh the warm-lane CoW pool base from an advancing target
  directory using XFS reflinks (cp --reflink=always, never auto).

  Positional (normal mode):
    <advancing_target_dir>   Source directory to copy from (e.g. Cargo target/).
    <base_dir>               Destination base directory (e.g. /warm-lanes/base/target).

  Options:
    --check-frag             Read-only: print "ok N" or "reseed-due N" (extent count).
    --frag-threshold N       Extent threshold for --check-frag (default: 64).
    --rustflags VALUE        RUSTFLAGS stamp written after swap (default: \${RUSTFLAGS:-}).
    --invocation FP          Invocation fingerprint stamp written after swap (default: '').
    -h, --help               Print this message and exit.

  Stdout:  empty (refresh path) or "ok N" / "reseed-due N" (--check-frag).
  Stderr:  all diagnostics.
EOF
}

# ── arg parsing ────────────────────────────────────────────────────────────────
ADVANCING_DIR=""
BASE_DIR=""
CHECK_FRAG=0
FRAG_THRESHOLD=64
RUSTFLAGS_VAL="${RUSTFLAGS:-}"
INVOCATION_VAL=""

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            _usage; exit 0 ;;
        --check-frag)
            CHECK_FRAG=1; shift ;;
        --frag-threshold)
            [ $# -ge 2 ] || { err "--frag-threshold requires a value"; exit 2; }
            FRAG_THRESHOLD="$2"; shift 2 ;;
        --rustflags)
            [ $# -ge 2 ] || { err "--rustflags requires a value"; exit 2; }
            RUSTFLAGS_VAL="$2"; shift 2 ;;
        --invocation)
            [ $# -ge 2 ] || { err "--invocation requires a value"; exit 2; }
            INVOCATION_VAL="$2"; shift 2 ;;
        -*)
            err "Unknown flag: $1"
            err "Run '$(basename "$0") --help' for usage."
            exit 2 ;;
        *)
            if [ -z "$ADVANCING_DIR" ]; then
                ADVANCING_DIR="$1"
            elif [ -z "$BASE_DIR" ]; then
                BASE_DIR="$1"
            else
                err "Unexpected positional argument: $1"
                err "Run '$(basename "$0") --help' for usage."
                exit 2
            fi
            shift ;;
    esac
done

# ── --check-frag mode: needs only <base_dir> ──────────────────────────────────
if [ "$CHECK_FRAG" = "1" ]; then
    # Accept either "script --check-frag <base_dir>" (one positional, ends in
    # ADVANCING_DIR slot) or with the second positional (BASE_DIR slot).
    if [ -n "$ADVANCING_DIR" ] && [ -z "$BASE_DIR" ]; then
        BASE_DIR="$ADVANCING_DIR"
        ADVANCING_DIR=""
    fi
    if [ -z "$BASE_DIR" ]; then
        err "Missing required argument: <base_dir>"
        err "Run '$(basename "$0") --help' for usage."
        exit 2
    fi
    if [ ! -d "$BASE_DIR" ]; then
        err "--check-frag: base_dir not found or not a directory: $BASE_DIR"
        exit 1
    fi
    # Count extents per regular file; track max.
    # xfs_bmap failure (not on PATH, not an XFS file, etc.) exits non-zero with
    # an actionable message — no silent swallowing (do NOT use || true here).
    max_extents=0
    while IFS= read -r -d '' f; do
        _bmap_out=""
        if ! _bmap_out=$(xfs_bmap "$f" 2>&1); then
            err "--check-frag: xfs_bmap failed on $f"
            err "$_bmap_out"
            err "Is xfsprogs installed? Is $BASE_DIR on an XFS filesystem?"
            err "Install xfsprogs or run on an XFS volume."
            exit 1
        fi
        n=$(printf '%s\n' "$_bmap_out" | grep -c '^\s*[0-9]*:' || true)
        [ "$n" -gt "$max_extents" ] && max_extents=$n
    done < <(find "$BASE_DIR" -type f -print0 2>/dev/null)
    if [ "$max_extents" -ge "$FRAG_THRESHOLD" ]; then
        printf 'reseed-due %d\n' "$max_extents"
    else
        printf 'ok %d\n' "$max_extents"
    fi
    exit 0
fi

# ── Normal refresh mode: validate positional args ─────────────────────────────
if [ -z "$ADVANCING_DIR" ]; then
    err "Missing required positional argument: <advancing_target_dir>"
    err "Run '$(basename "$0") --help' for usage."
    exit 2
fi
if [ -z "$BASE_DIR" ]; then
    err "Missing required positional argument: <base_dir>"
    err "Run '$(basename "$0") --help' for usage."
    exit 2
fi

# Validate advancing dir
if [ ! -d "$ADVANCING_DIR" ]; then
    err "<advancing_target_dir> not found or not a directory: $ADVANCING_DIR"
    exit 1
fi
# Validate base parent dir exists (we'll create base_dir itself via cp)
_base_parent="$(dirname "$BASE_DIR")"
if [ ! -d "$_base_parent" ]; then
    err "Parent of <base_dir> does not exist: $_base_parent"
    exit 1
fi

# ── EXIT trap: clean up partial .new / .old on failure ────────────────────────
# Mirrors provision-warm-lane-fs.sh's _cleanup_on_exit discipline.
# A failed reflink copy may have created a partial <base_dir>.new before the cp
# command exited non-zero. The trap ensures no partial tree is left behind and
# a pre-existing <base_dir> is never disturbed by a failed refresh.
_cleanup_on_exit() {
    local exit_code=$?
    [ $exit_code -eq 0 ] && return
    if [ -n "${BASE_DIR:-}" ]; then
        # Recovery: if the atomic swap was partially complete (base was moved to
        # base.old but base.new → base rename never finished), base.old holds the
        # ONLY surviving copy of the original base. Restore it instead of deleting.
        # This guards the window between "mv -T base base.old" succeeding and
        # "mv -T base.new base" failing/being killed.
        if [ ! -d "${BASE_DIR}" ] && [ -d "${BASE_DIR}.old" ]; then
            mv "${BASE_DIR}.old" "${BASE_DIR}" 2>/dev/null || true
        fi
        # Clean up partial intermediates; .old is already gone if restored above.
        rm -rf "${BASE_DIR}.new" 2>/dev/null || true
        rm -rf "${BASE_DIR}.old" 2>/dev/null || true
    fi
}
trap _cleanup_on_exit EXIT

# ── main refresh ───────────────────────────────────────────────────────────────
info "refresh-warm-base.sh: advancing=$ADVANCING_DIR  base=$BASE_DIR"

# Pre-clean stale intermediates from a prior interrupted run (SIGKILL/power-loss).
# The EXIT trap only fires for THIS run; a .new or .old left by a prior crash must
# be cleared before we start so that:
#   - a pre-existing <base>.new doesn't cause cp to copy the source INSIDE it
#     (cp -a with an existing destination dir copies INTO it, not over it), and
#   - a non-empty <base>.old doesn't cause "mv -T base base.old" to fail with
#     "Directory not empty" and abort the swap.
rm -rf "${BASE_DIR}.new" "${BASE_DIR}.old"

# Step 1: reflink-copy advancing -> base.new (fail-closed: --reflink=always)
info "Copying $ADVANCING_DIR -> $BASE_DIR.new (--reflink=always) ..."
if ! cp -a --reflink=always "$ADVANCING_DIR" "$BASE_DIR.new"; then
    err "cp --reflink=always failed — the target filesystem may not support reflinks."
    err "Refusing to fall back to a non-reflink copy (invariant P2)."
    exit 1
fi
ok "Reflink copy complete."

# Step 2: atomic rename swap
if [ -d "$BASE_DIR" ]; then
    info "Moving $BASE_DIR -> $BASE_DIR.old ..."
    mv -T "$BASE_DIR" "$BASE_DIR.old"
    info "Moving $BASE_DIR.new -> $BASE_DIR ..."
    mv -T "$BASE_DIR.new" "$BASE_DIR"
    info "Removing $BASE_DIR.old ..."
    rm -rf "$BASE_DIR.old"
else
    info "No prior base — moving $BASE_DIR.new -> $BASE_DIR ..."
    mv -T "$BASE_DIR.new" "$BASE_DIR"
fi

# Step 3: self-description stamps
printf '%s' "$RUSTFLAGS_VAL" > "$BASE_DIR.rustflags"
printf '%s' "$INVOCATION_VAL" > "$BASE_DIR.invocation"

ok "Base refreshed at $BASE_DIR"
