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
#   1. Provenance guard (inv.9): refuse if advancing worktree is dirty (uncommitted
#      tracked changes) or --landed-commit <sha> is absent / mismatched vs HEAD.
#      Fail-closed: no swap on refusal.
#   2. Symlink-gen staging: build <base_dir>.gen.<N>.partial via
#      cp -a --reflink=always (fail-closed — P2), rename to <base_dir>.gen.<N>.
#   2b.Prune superseded hash-generations: within <base_dir>.gen.<N>.partial's
#      debug/deps, group depth-1 regular files by cargo's hashed-artefact
#      STEM (stripping a trailing `-<16 hex>` extra-filename suffix) and
#      rank each stem's HASHES — never its (stem, ext) files independently —
#      newest-first by the max mtime over every extension that hash has;
#      keep the newest _PRUNE_KEEP_GENERATIONS (=2) hashes per stem and
#      delete every file of a losing hash, on every extension it has.
#      Ranking by hash (not per-extension) keeps a kept "fallback
#      generation" COMPLETE: rustc writes a hash's .rmeta (pipelining) and
#      .rlib (end of codegen) at different times, so per-extension ranking
#      could keep one extension of a hash while pruning another — a
#      fallback that is only partially present is not a fallback. N=2, not
#      N=1: N=1 reclaims more but leaves no fallback generation, so a lane
#      whose fingerprint misses the single survivor rebuilds cold — the
#      exact cost the warm base exists to avoid. mtime, not .fingerprint:
#      .fingerprint is a strict superset of deps (cargo GCs neither tree),
#      so a fingerprint-keyed filter cannot remove anything BY CONSTRUCTION
#      — do not reinstate that rule. LOAD-BEARING PRECONDITION: mtime is
#      only a valid ordering signal because nothing stamps artefact mtimes
#      today (seed-warm-lane.sh's bulk stamp targets sources, not target/;
#      this script stamps no artefacts either) — if that ever changes,
#      re-measure the live base's mtime spread before trusting this rule
#      again. debug/.fingerprint is NOT pruned in lockstep (116 MB against
#      204 GiB; pruning it restores no liveness signal). Sited BEFORE the
#      partial→gen rename below, so a failed prune costs only the
#      .partial, which the EXIT trap already sweeps. Scope: base only
#      (never a task lane, never _merge-verify); debug/deps only —
#      release/ and debug/build are untouched. The logged `victim_bytes=`
#      figure is the victims' APPARENT size, not disk actually reclaimed:
#      this staging copy is a reflink, so the same extents stay referenced
#      by the advancing source (and, refreshing over an existing base, by
#      the retired gen until Step 6's reader-refcount GC) — real reclaim
#      lags by at least one more refresh-and-reseed cycle plus that GC.
#   3. Bootstrap (first refresh): if <base_dir> is a pre-existing real dir,
#      rename it to a retired gen dir first (never rename-over-populated).
#   4. Write per-gen authoritative landed-commit stamp: <base_dir>.gen.<N>.basecommit
#      = the verified --landed-commit SHA (== $_prov_head).  Written AFTER the
#      partial→gen rename and BEFORE the atomic symlink swap so the stamp is present
#      the instant the symlink flips.  PER-GEN (sibling of the gen dir, NOT a single
#      shared file) so any clone pinning gen.N via `flock -s` reads gen.N's OWN
#      immutable commit — TOCTOU-free under concurrent refreshes (inv.8).
#      Consumed by seed-warm-lane.sh as the authoritative delta-touch base (priority
#      over the drift-prone legacy .warm-base-meta BASE_COMMIT, esc-3468-75).
#   4b.Write per-gen build-worktree stamp: <base_dir>.gen.<N>.buildroot = realpath of
#      the advancing worktree ROOT (dirname(advancing_target_dir), the same _prov_wt
#      resolved by the inv.9 provenance guard).  Written alongside .basecommit (same
#      timing/TOCTOU properties).  Consumed by seed-warm-lane.sh to detect when the
#      base's test binaries baked a CARGO_MANIFEST_DIR (or other env!() path macro)
#      pointing at a build worktree that differs from the consuming lane, so it can
#      relink the affected test binaries (task 4983, esc-4906-57).
#   5. Atomic whole-tree swap: ln -sfn <base_dir>.gen.<N> <base_dir>
#      (ln -sfn is atomic on Linux: symlink + rename under the hood).
#   6. Reader-refcount GC: sweep retired <base_dir>.gen.* dirs; rm each whose
#      per-gen flock (flock -n -x *.lock) is free, holding the exclusive lock
#      ACROSS the rm so no reader can sneak in mid-deletion. A consuming clone
#      MUST hold flock -s <base_dir>.gen.<N>.lock for the duration of its cp -a
#      walk of that gen — the flock defers the rm until the clone finishes.
#      (Separates dir-entry refcount from XFS extent-refcount, which are orthogonal.)
#      The .basecommit and .buildroot siblings are also removed when their gen is
#      reaped (no orphans).
#   7. Write self-description stamps: <base_dir>.rustflags, <base_dir>.invocation
#
# In-flight clone independence (B6 + D10): the atomic symlink swap means readers
# that have already resolved the symlink to a concrete gen dir remain coherent for
# that gen. The reader-refcount flock defers dir-entry removal until the clone walk
# completes — an rm while a reader holds flock -s would ENOENT the clone mid-walk.
#
# Sidecar stamp convention: <base_dir>.rustflags, <base_dir>.invocation, and
# per-gen <base_dir>.gen.<N>.basecommit / <base_dir>.gen.<N>.buildroot are adjacent
# to the base dir (sibling files, NOT inside the dir). warm-lane-preflight.sh reads
# .rustflags/.invocation; seed-warm-lane.sh reads .basecommit (authoritative) and
# the legacy .warm-base-meta for delta-touch provenance, and .buildroot to detect a
# build-worktree mismatch for the env!()-baked-path test relink (task 4983).

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
# Resolve BASE_DIR to an absolute path so symlink targets are always absolute.
# A relative BASE_DIR with a directory component (e.g. foo/bar) would produce a
# symlink target foo/bar.gen.N that the kernel resolves relative to bar's PARENT
# directory — yielding foo/foo/bar.gen.N (wrong). Using absolute paths avoids this.
# NB: the parent dir was just verified to exist, so 'cd <parent> && pwd' is safe.
BASE_DIR="$(cd "$(dirname "$BASE_DIR")" && pwd)/$(basename "$BASE_DIR")"

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
_prov_status="$(git -C "$_prov_wt" status --porcelain --untracked-files=no 2>/dev/null)"
if [ -n "$_prov_status" ]; then
    err "Provenance guard: advancing worktree has uncommitted tracked changes (WIP detected)."
    err "  Worktree: $_prov_wt"
    _prov_dirty_head="$(printf '%s\n' "$_prov_status" | head -5)"
    err "  Dirty tracked files (first 5):"
    printf '%s\n' "$_prov_dirty_head" | while IFS= read -r _line; do
        err "    $_line"
    done
    err "Refusing to promote a task lane with WIP. Commit or clean tracked changes first."
    # This refusal fires INSIDE a warm lane, so the remedy it names is read by
    # exactly the population that filled the shared WIP stack. Naming the reason
    # here (not just the remedy) is the point: that stack is ONE ref in the one
    # shared .git — it is not per-worktree — so parking WIP there hands it to
    # every other lane on the host (esc-5785-6). Wording deliberately avoids the
    # bare verb, which tests/infra/test_refresh_warm_base.sh Block I pins.
    err "  Do NOT park it on the shared LIFO WIP ref either: that ref lives in the one"
    err "  shared .git and is not per-worktree, so every lane on this host shares it."
    err "  Rule and reach: CLAUDE.md, \"Warm lanes\"."
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
_prov_head="$(git -C "$_prov_wt" rev-parse HEAD 2>/dev/null)"
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
_SWAP_PRIOR_LINK=""     # prior symlink target (if base was a symlink pre-swap)
_SWAP_BOOTSTRAP_DIR=""  # if bootstrap renamed a real base dir to a gen dir
_PRUNE_SUMMARY_FILE=""  # Step 3b's summary temp file, if one was created

_cleanup_on_exit() {
    local exit_code=$?
    [ $exit_code -eq 0 ] && return
    [ -n "${_PRUNE_SUMMARY_FILE:-}" ] && rm -f "$_PRUNE_SUMMARY_FILE" 2>/dev/null || true
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

# Step 3b: prune superseded cargo hash-generations from the staging copy.
#
# Groups depth-1 files under debug/deps by cargo's hashed-artefact STEM
# (stripping a trailing `-<16 lowercase hex>` extra-filename suffix) and
# ranks each stem's HASHES — never its (stem, ext) files independently —
# newest-first by the max mtime over every extension that hash has; keeps
# the newest _PRUNE_KEEP_GENERATIONS and deletes every file of a losing
# hash, on every extension it has. Ranking per-hash (not per-extension) is
# what keeps a kept generation COMPLETE: rustc writes a hash's .rmeta
# (pipelining) and .rlib (end of codegen) at different times, so ranking
# each extension independently could keep hash B's .rlib while pruning
# hash B's .rmeta — a "fallback generation" only partially present, which
# defeats the point of keeping one. libfoo-<hash>.rlib and
# libfoo-<hash>.rmeta therefore rise and fall together; a split-debuginfo
# shard like axum-<hash>.axum.<hash>-cgu.09.rcgu.dwo never matches at all
# (its stem-before-last-dot does not end in `-<16hex>`) so it is never a
# candidate. Reclaims the bulk of the warm base's bytes (mostly
# extensionless test/bench binaries); N=2 rather than N=1 is a deliberate
# ruling that keeps a fallback generation so a lane whose fingerprint misses
# the single newest survivor does not rebuild cold — not a tunable, so no CLI
# flag or env override.
#
# Sited on the .partial STAGING dir (never the live base, never the final gen
# dir) — a failure here is covered for free by the existing EXIT trap's
# `.gen.*.partial` sweep above, with no new cleanup code needed.
#
# The `victim_bytes=` figure logged below is the victims' APPARENT size, not
# disk actually reclaimed: the staging copy is a reflink of the advancing
# source, so the same extents stay referenced by the advancing target (and,
# refreshing over an existing base, by the retired gen until Step 6's
# reader-refcount GC reaps it) — real reclaim lags by at least one more
# refresh-and-reseed cycle plus that GC. A still-uplifted cargo hardlink
# (debug/<bin>, debug/build/<pkg>-<hash>/build-script-build) overstates it
# further: cp -a preserves hardlinks, so unlinking the deps copy of a file
# still linked elsewhere frees nothing at all.
readonly _PRUNE_KEEP_GENERATIONS=2
# _prune_deps is the ONE place the prune's reach is written (SPOT) — every
# find/cd/rm below reads only this local, never a second path expression, so
# the scope of the sweep is a single line to audit.
_prune_deps="${_new_gen_partial}/debug/deps"
if [ -d "$_prune_deps" ]; then
    info "Pruning superseded hash-generations under $_prune_deps (keep newest ${_PRUNE_KEEP_GENERATIONS}) ..."
    # -maxdepth 1 -type f confines the sweep to regular files directly in
    # deps/, excluding the nested dir (e.g. deps/rustc*/), its contents, and
    # any symlink — never descended into, never followed.
    #
    # The victim-count/victim-bytes summary crosses from awk back into this
    # shell via a temp file (never stdout, which xargs below consumes as the
    # NUL-delimited deletion list) so it can be logged through the real
    # info() helper — reusing it rather than re-implementing its formatting
    # a second time inside the awk program (SPOT).
    _prune_summary_file="$(mktemp)"
    _PRUNE_SUMMARY_FILE="$_prune_summary_file"  # let the EXIT trap reclaim it on a mid-prune failure
    # The total order (mtime desc, then hash string desc as an explicit
    # tiebreak) is expressed ONCE, in the hash_newer() comparator below, and
    # is applied per HASH rather than per file — the one normative ranking
    # for the whole stage (SPOT). No shell `sort` stage feeds this: ranking
    # is keyed on hash, not on incoming line order, so pre-sorting the raw
    # `find` stream would establish an order this program never reads.
    find "$_prune_deps" -maxdepth 1 -type f -printf '%T@\t%s\t%f\n' \
        | awk -F'\t' -v keep="$_PRUNE_KEEP_GENERATIONS" -v summary_file="$_prune_summary_file" '
            function hash_newer(mt_a, h_a, mt_b, h_b) {
                # True when (mt_a, h_a) ranks strictly ahead of (mt_b, h_b):
                # mtime desc, then hash string desc as an explicit tiebreak.
                # Numeric mtime compare (never lexical: %T@ carries
                # sub-second precision, so "...9" must not sort before
                # "...10"). The hash alphabet is fixed to [0-9a-f] by the
                # grammar match below, so the string tiebreak is stable
                # across locales without needing LC_ALL here.
                if (mt_a != mt_b) return mt_a > mt_b
                return h_a > h_b
            }
            BEGIN { ORS = "\0" }
            {
                fmtime = $1 + 0
                fsize  = $2
                fname  = $3
                # Split on the FINAL dot only (last dot, never the first,
                # never a greedy multi-dot extension): ext = ".<suffix>"
                # when fname contains a dot, else "". This is the one
                # normative copy of the hashed-artefact grammar (SPOT) —
                # the stem must match ^(.+)-<16 lowercase hex>$ exactly,
                # anchored both ends, so a short/long/uppercase pseudo-hash
                # never matches. The hash is spelled out as 16 repeated
                # [0-9a-f] classes rather than a {16} interval expression:
                # interval expressions are unsupported by mawk < 1.3.4 and
                # busybox awk, under which {16} is literal text, no
                # filename would ever match, and this whole stage would
                # silently become a no-op (files=0) — no error, no failing
                # assertion, just a quietly disabled mechanism.
                base_no_ext = fname
                sub(/\.[^.]*$/, "", base_no_ext)
                ext = substr(fname, length(base_no_ext) + 1)
                if (base_no_ext !~ /^.+-[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]$/) next
                stem = substr(base_no_ext, 1, length(base_no_ext) - 17)
                hash = substr(base_no_ext, length(base_no_ext) - 15)

                n++
                rec_stem[n] = stem
                rec_hash[n] = hash
                rec_name[n] = fname
                rec_size[n] = fsize

                # Rank by HASH, not by (stem, ext) independently — see the
                # comment above this pipeline for why. The rank key for a
                # hash is the MAX mtime over every file of that hash,
                # regardless of extension, so a losing hash loses on every
                # extension it has, together.
                hkey = stem SUBSEP hash
                if (!(hkey in hash_mtime) || fmtime > hash_mtime[hkey]) hash_mtime[hkey] = fmtime
                if (!((stem, hash) in stem_hash_seen)) {
                    stem_hash_seen[stem, hash] = 1
                    stem_hash_count[stem]++
                    stem_hash_list[stem, stem_hash_count[stem]] = hash
                }
            }
            END {
                # Per stem, rank its distinct hashes with hash_newer() and
                # keep the newest `keep`. cargo hash-generation counts per
                # unit are small (mean ~1.9, max ~30 measured on the live
                # warm base), so a plain insertion sort is plenty — no
                # gawk-only asort()/asorti() needed.
                for (stem in stem_hash_count) {
                    cnt = stem_hash_count[stem]
                    for (i = 1; i <= cnt; i++) order[i] = stem_hash_list[stem, i]
                    for (i = 2; i <= cnt; i++) {
                        v = order[i]
                        j = i - 1
                        while (j >= 1 && hash_newer(hash_mtime[stem, v], v, hash_mtime[stem, order[j]], order[j])) {
                            order[j + 1] = order[j]
                            j--
                        }
                        order[j + 1] = v
                    }
                    for (i = 1; i <= cnt && i <= keep; i++) kept[stem, order[i]] = 1
                }
                total_files = 0
                total_bytes = 0
                for (i = 1; i <= n; i++) {
                    if ((rec_stem[i], rec_hash[i]) in kept) continue
                    print rec_name[i]
                    total_files++
                    total_bytes += rec_size[i]
                }
                printf "files=%d victim_bytes=%d\n", total_files, total_bytes > summary_file
            }
        ' \
        | (cd "$_prune_deps" && xargs -r -0 rm -f --)
    info "prune deps=$_prune_deps $(cat "$_prune_summary_file") (apparent victim size — real reclaim lags until the advancing source is reseeded and Step 6 reaps the retired gen)"
    rm -f "$_prune_summary_file"
else
    info "No debug/deps under staging copy — skipping hash-generation prune (files=0)."
fi

# Step 4: rename staging dir to the final gen dir (dir→new-name rename, safe).
info "Finalizing: $_new_gen_partial -> $_new_gen_dir"
mv "$_new_gen_partial" "$_new_gen_dir"

# Step 4b: write the authoritative per-gen landed-commit stamp.
# Written AFTER the partial→gen rename (gen dir is final) and BEFORE the symlink
# swap (Step 5), so the stamp is present the instant the symlink flips — no
# concurrent reader can observe the gen without its .basecommit sibling.
#
# PER-GEN (not a single overwritten file): a consuming clone resolves the symlink
# to gen.N, pins it with `flock -s`, and reads gen.N's OWN .basecommit — coherent
# with the exact artifacts it is cloning, even while a concurrent refresh advances
# to gen.N+1 (inv.8 base-coherence, TOCTOU-free).
#
# Consumed by seed-warm-lane.sh as the authoritative delta-touch base (priority
# over the legacy .warm-base-meta BASE_COMMIT, which is drift-prone).
# See design decision in .task/plan.json and esc-3468-75.
printf '%s' "$LANDED_COMMIT" > "${_new_gen_dir}.basecommit"

# Step 4b (cont.): write the per-gen build-worktree stamp alongside .basecommit.
# Value = realpath of _prov_wt (the advancing worktree ROOT, already resolved by
# the inv.9 provenance guard above) — the worktree under which this gen's test
# binaries were compiled, and thus the worktree path baked into any env!("CARGO_
# MANIFEST_DIR") (and allied CARGO_* path macro) call in their sources.
# Same per-gen / TOCTOU-free properties as .basecommit (written before the Step 5
# symlink swap, reaped alongside its gen in the Step 6 GC).
# Consumed by seed-warm-lane.sh to detect when the recorded build-worktree path
# differs from the consuming lane, so it can relink the affected env!()-baked-path
# test/bench binaries (task 4983, esc-4906-57).
printf '%s' "$(realpath -m "$_prov_wt")" > "${_new_gen_dir}.buildroot"

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
    # Try to acquire exclusive lock (non-blocking) on the per-gen lock file, and
    # hold it ACROSS the rm so no reader can acquire flock -s between lock release
    # and deletion (that window would allow a reader to openat the gen dir just
    # before we remove it, causing ENOENT mid-walk — the race flock is meant to
    # prevent). flock -n -x FILE sh -c 'rm -rf "$1"' _ DIR holds the lock for the
    # full duration of the rm; the lock file itself is removed afterwards.
    # If a clone holds flock -s, flock -n -x fails → skip, reap on next refresh.
    _gc_lock="${_gc_gen}.lock"
    touch "$_gc_lock" 2>/dev/null || true
    if flock -n -x "$_gc_lock" sh -c 'rm -rf "$1"' _ "$_gc_gen" 2>/dev/null; then
        rm -f "$_gc_lock" 2>/dev/null || true
        # Also reap the authoritative per-gen .basecommit sibling so it does not
        # accumulate as an orphan after the gen dir is removed.  The .basecommit
        # file is only meaningful while its gen dir exists; removing it here keeps
        # the pool directory clean as gens roll forward.
        rm -f "${_gc_gen}.basecommit" 2>/dev/null || true
        # Also reap the .buildroot sibling for the same reason (no orphans).
        rm -f "${_gc_gen}.buildroot" 2>/dev/null || true
        info "GC: reaping retired gen (no active reader): $_gc_gen"
    else
        info "GC: skipping retired gen (reader in-flight): $_gc_gen"
    fi
done

# Step 7: self-description stamps (sibling files adjacent to the symlink, as before)
printf '%s' "$RUSTFLAGS_VAL" > "$BASE_DIR.rustflags"
printf '%s' "$INVOCATION_VAL" > "$BASE_DIR.invocation"

ok "Base refreshed at $BASE_DIR (gen ${_next_gen})"
