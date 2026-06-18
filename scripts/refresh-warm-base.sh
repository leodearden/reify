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
#   --landed-commit SHA      (Required for normal refresh) Assert that the
#                            advancing target's git worktree HEAD == SHA.
#                            Provenance guard (inv.9): refuses WIP lanes
#                            (uncommitted tracked changes) and mismatched HEADs.
#                            Not used by --check-frag.
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
    --landed-commit SHA      (Required for refresh) Assert advancing worktree
                             HEAD == SHA; refuses WIP lanes (uncommitted tracked
                             changes) and head mismatches. Not used by --check-frag.
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
LANDED_COMMIT=""

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
        --landed-commit)
            [ $# -ge 2 ] || { err "--landed-commit requires a value"; exit 2; }
            LANDED_COMMIT="$2"; shift 2 ;;
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

# ── PROVENANCE GUARD (inv.9) ──────────────────────────────────────────────────
# Required for the normal refresh path (not --check-frag).
# Guard sequence (fail-closed — each refusal: actionable stderr, non-zero exit,
# NO swap, base untouched):
#   1. Resolve git worktree = dirname(advancing_target_dir)
#   2. Refuse if not inside a git worktree
#   3. Refuse if git status non-empty (WIP = uncommitted TRACKED changes;
#      --untracked-files=no ignores untracked target/ etc., matching the
#      orchestrator dirty-start semantics from CLAUDE.md)
#   4. Refuse if --landed-commit is absent
#   5. Refuse if git rev-parse HEAD != the asserted sha (head mismatch)
_prov_wt="$(dirname "$ADVANCING_DIR")"
if ! git -C "$_prov_wt" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    err "Provenance guard: <advancing_target_dir> is not inside a git worktree: $ADVANCING_DIR"
    err "  Worktree resolved as: $_prov_wt"
    err "The advancing target must be a subdirectory of a git worktree. Refusing to swap."
    exit 1
fi
_prov_status="$(git -C "$_prov_wt" status --porcelain --untracked-files=no 2>&1)"
if [ -n "$_prov_status" ]; then
    err "Provenance guard: advancing worktree has uncommitted tracked changes (WIP detected)."
    err "  Worktree: $_prov_wt"
    _prov_dirty_head="$(printf '%s\n' "$_prov_status" | head -5)"
    err "  Dirty tracked files (first 5):"
    printf '%s\n' "$_prov_dirty_head" | while IFS= read -r _line; do
        err "    $_line"
    done
    err "Refusing to promote a task lane with WIP. Commit or stash tracked changes first."
    exit 1
fi
if [ -z "$LANDED_COMMIT" ]; then
    err "Provenance guard: --landed-commit <sha> is required (provenance assertion missing)."
    err "  Pass the confirmed landed HEAD sha:"
    err "    --landed-commit \$(git -C <lane_worktree> rev-parse HEAD)"
    err "This assertion prevents task-lane WIP and ensures the advancing HEAD is known."
    err "Only the merge lane (at confirmed landed HEAD) may advance the base."
    exit 1
fi
_prov_head="$(git -C "$_prov_wt" rev-parse HEAD 2>&1)"
if [ "$_prov_head" != "$LANDED_COMMIT" ]; then
    err "Provenance guard: advancing worktree HEAD does not match --landed-commit assertion."
    err "  Expected HEAD (--landed-commit): $LANDED_COMMIT"
    err "  Actual HEAD (git rev-parse):     $_prov_head"
    err "  Worktree: $_prov_wt"
    err "HEAD mismatch: pass the correct landed commit sha via --landed-commit."
    exit 1
fi
info "Provenance guard: OK (worktree clean, HEAD=$_prov_head)"

# ── EXIT trap: clean up .gen.*.partial + restore prior base on failure ────────
# State variables set during the swap; used by the trap for targeted recovery.
_SWAP_PRIOR_LINK=""    # prior symlink target (if base was a symlink pre-swap)
_SWAP_BOOTSTRAP_DIR="" # if bootstrap renamed a real base dir to a gen dir

_cleanup_on_exit() {
    local exit_code=$?
    [ $exit_code -eq 0 ] && return
    if [ -n "${BASE_DIR:-}" ]; then
        # Restore prior base state on failure:
        if [ -n "${_SWAP_BOOTSTRAP_DIR:-}" ] \
           && [ -d "${_SWAP_BOOTSTRAP_DIR}" ] \
           && [ ! -e "${BASE_DIR}" ] && [ ! -L "${BASE_DIR}" ]; then
            # Bootstrap case: real dir was renamed to a gen dir but the swap
            # did not complete — restore the original real dir.
            mv "${_SWAP_BOOTSTRAP_DIR}" "${BASE_DIR}" 2>/dev/null || true
        elif [ -n "${_SWAP_PRIOR_LINK:-}" ] && [ ! -L "${BASE_DIR}" ]; then
            # Symlink case: base was a symlink before but ln -sfn failed mid-swap
            # (extremely unlikely) — restore the prior symlink target.
            ln -sfn "${_SWAP_PRIOR_LINK}" "${BASE_DIR}" 2>/dev/null || true
        fi
        # Clean up all .gen.*.partial staging dirs left by this run
        for _p in "${BASE_DIR}.gen."*.partial; do
            [ -d "$_p" ] && rm -rf "$_p" 2>/dev/null || true
        done
    fi
}
trap _cleanup_on_exit EXIT

# ── main refresh — D10 symlink-gen swap ────────────────────────────────────────
info "refresh-warm-base.sh: advancing=$ADVANCING_DIR  base=$BASE_DIR"

# Pre-clean stale .gen.*.partial dirs from a prior interrupted run (SIGKILL/power-loss).
for _stale_p in "${BASE_DIR}.gen."*.partial; do
    [ -d "$_stale_p" ] || continue
    info "Pre-clean: removing stale partial gen dir: $_stale_p"
    rm -rf "$_stale_p" 2>/dev/null || true
done

# Step 1: compute the next generation index N.
# Scan existing <base>.gen.<N> dirs (integer N only; skip .partial suffixes).
_gen_max=0
for _eg in "${BASE_DIR}.gen."*; do
    [ -d "$_eg" ] || continue
    _gn="${_eg##*.gen.}"
    case "$_gn" in *[!0-9]*) continue ;; esac
    [ "$_gn" -gt "$_gen_max" ] && _gen_max="$_gn"
done
_next_gen=$(( _gen_max + 1 ))
info "Next generation: ${_next_gen} (max existing: ${_gen_max})"

# Step 2: bootstrap — if <base> is a pre-existing REAL dir (not a symlink),
# rename it to a retired gen dir BEFORE building the new gen.
# INVARIANT: NEVER rename over a populated dir (ENOTEMPTY); the next-gen index
# is computed first, so the target name does not exist yet.
# (A dir→new-name rename is always safe regardless of the dir's content.)
if [ -d "$BASE_DIR" ] && [ ! -L "$BASE_DIR" ]; then
    _retire_gen_dir="${BASE_DIR}.gen.${_next_gen}"
    info "Bootstrap: renaming pre-existing base to retired gen ${_next_gen} ..."
    info "  $BASE_DIR -> $_retire_gen_dir"
    mv "$BASE_DIR" "$_retire_gen_dir"
    _SWAP_BOOTSTRAP_DIR="$_retire_gen_dir"
    _next_gen=$(( _next_gen + 1 ))
    info "New generation index after bootstrap: ${_next_gen}"
elif [ -L "$BASE_DIR" ]; then
    # Record prior symlink target for recovery in the EXIT trap
    _SWAP_PRIOR_LINK="$(readlink "$BASE_DIR")"
fi

# Step 3: reflink-copy advancing → <base>.gen.<N>.partial (staging dir).
# fail-closed: --reflink=always, never auto (invariant P2).
_new_gen_dir="${BASE_DIR}.gen.${_next_gen}"
_new_gen_partial="${_new_gen_dir}.partial"
info "Copying $ADVANCING_DIR -> $_new_gen_partial (--reflink=always) ..."
if ! cp -a --reflink=always "$ADVANCING_DIR" "$_new_gen_partial"; then
    err "cp --reflink=always failed — the target filesystem may not support reflinks."
    err "Refusing to fall back to a non-reflink copy (invariant P2)."
    exit 1
fi
ok "Reflink copy complete (gen ${_next_gen})."

# Step 4: rename staging dir to the final gen dir (dir→new-name rename, safe).
info "Finalizing: $_new_gen_partial -> $_new_gen_dir"
mv "$_new_gen_partial" "$_new_gen_dir"

# Step 5: atomic whole-tree symlink swap.
# ln -sfn is atomic on Linux: symlink(2) to temp + rename(2) replaces the link.
# No compiled renameat2 helper needed — shell-only, FS-agnostic default.
info "Atomically re-pointing base symlink -> $_new_gen_dir"
ln -sfn "$_new_gen_dir" "$BASE_DIR"
ok "Base symlink updated: $BASE_DIR -> $(readlink "$BASE_DIR")"

# Step 6: reader-refcount GC — sweep retired gens and rm those with no reader.
#
# Convention (D8 seam): a consuming clone MUST hold `flock -s <base>.gen.<N>.lock`
# for the duration of its `cp -a --reflink` walk of that pinned gen dir.
# This separates two distinct refcounts:
#   - Reader-refcount (dir-entry governs): WHEN the dir ENTRY may be rm'd.
#     Removing an entry a live clone has not yet openat'd would ENOENT the clone
#     mid-walk. flock -s holds this refcount open; we try flock -n -x here.
#   - XFS extent-refcount (kernel governs): frees CoW extents on last file close.
#     This is automatic and orthogonal to when we rm the dir entry.
# reify ships the GC (rm side); DF ζ holds the shared lock during its clone walk
# (D8 'reify ships primitives, DF wires consumers' seam).
_gc_live_gen="$(readlink "$BASE_DIR")"
for _gc_gen in "${BASE_DIR}.gen."*; do
    [ -d "$_gc_gen" ] || continue
    _gc_n="${_gc_gen##*.gen.}"
    case "$_gc_n" in *[!0-9]*) continue ;; esac  # skip .partial and other suffixes
    # Skip the live (current) gen — never GC the gen the symlink points to
    [ "$_gc_gen" = "$_gc_live_gen" ] && continue
    # Try to acquire exclusive lock (non-blocking) on the per-gen lock file.
    # If a clone holds flock -s, flock -n -x fails → skip, reap on next refresh.
    _gc_lock="${_gc_gen}.lock"
    touch "$_gc_lock" 2>/dev/null || true
    if flock -n -x "$_gc_lock" true 2>/dev/null; then
        info "GC: reaping retired gen (no active reader): $_gc_gen"
        rm -rf "$_gc_gen" "$_gc_lock" 2>/dev/null || true
    else
        info "GC: skipping retired gen (reader in-flight): $_gc_gen"
    fi
done

# Step 7: self-description stamps (sibling files adjacent to the symlink, as before)
printf '%s' "$RUSTFLAGS_VAL" > "$BASE_DIR.rustflags"
printf '%s' "$INVOCATION_VAL" > "$BASE_DIR.invocation"

ok "Base refreshed at $BASE_DIR (gen ${_next_gen})"
