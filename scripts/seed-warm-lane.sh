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
#   --fresh-checkout: a non-empty <lane_dir>/target is REPLACED (mv to trash,
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

# Touch every file in LANE_DIR listed by `git diff --name-only <sha>`.
# Logs how many paths were touched; warns (non-fatal) if git diff fails so
# the caller can see a zero-touch run rather than silently keeping stale stamps.
_touch_git_delta() {
    local sha="$1"
    local count=0
    local diff_out
    local diff_rc=0
    diff_out="$(git -C "$LANE_DIR" diff --name-only "$sha" 2>/dev/null)" || diff_rc=$?
    if [ "$diff_rc" -ne 0 ]; then
        warn "git diff --name-only $sha failed (exit $diff_rc); delta touch skipped — sources keep 2020-01-01 stamp"
        return 0
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

    # Self-clobber guard (always active): refuse if LANE_TARGET or LANE_DIR
    # resolves to BASE_TARGET_DIR — that would rename the warm base to trash.
    if [ "$_rp_lane_target" = "$_rp_base_target" ] || \
       [ "$_rp_lane_dir" = "$_rp_base_target" ]; then
        err "Misuse guard: LANE_TARGET or LANE_DIR resolves to BASE_TARGET_DIR (self-clobber)"
        err "  LANE_TARGET: $_rp_lane_target"
        err "  LANE_DIR: $_rp_lane_dir"
        err "  BASE_TARGET_DIR: $_rp_base_target"
        err "  Renaming the base to trash and cloning onto it would destroy the warm base."
        exit 1
    fi

    # --fresh-checkout: replace-existing semantics (D10 always-re-seed-at-acquire).
    # If LANE_TARGET is non-empty, atomically rename it to a trash sidecar before
    # cloning.  Crash-safe ordering: rename-then-clone-then-rm ensures a crash
    # leaves a recoverable trash dir, never a half-seeded target.
    if [ -d "$LANE_TARGET" ] && [ -n "$(ls -A "$LANE_TARGET" 2>/dev/null)" ]; then
        RESEED_TRASH="$LANE_DIR/target.reseed-trash.$$"
        info "Renaming non-empty $LANE_TARGET → $(basename "$RESEED_TRASH") before re-seed ..."
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

# Remove the reseed trash after a SUCCESSFUL clone (cp failure leaves trash for recovery).
# Background by default (production: large lane rm must not block acquire).
# Foreground when REIFY_WARM_LANE_RESEED_TRASH_SYNC=1 (test-determinism knob).
if [ -n "$RESEED_TRASH" ] && [ -d "$RESEED_TRASH" ]; then
    info "Removing reseed trash: $(basename "$RESEED_TRASH") ..."
    if [ "${REIFY_WARM_LANE_RESEED_TRASH_SYNC:-}" = "1" ]; then
        rm -rf "$RESEED_TRASH"
    else
        rm -rf "$RESEED_TRASH" &
    fi
fi

# ── mtime normalization (D5) ──────────────────────────────────────────────────
if [ -n "$FRESH_CHECKOUT" ]; then
    # Bulk-stamp all sources to 2020-01-01T00:00:00, pruning target/ and .git/
    # so only the delta closure needs recompilation.
    info "Stamping sources to 2020-01-01 (pruning target/ and .git/) ..."
    # touch -h (no-dereference): a checked-out worktree may contain tracked
    # RELATIVE symlinks (e.g. config/usage-accounts.yaml -> ../../dark-factory/...)
    # that resolve from the repo root but dangle inside a lane at a different
    # depth.  Without -h, touch follows the link and fails ("No such file"),
    # aborting the whole seed -> cold fallback.  -h stamps the symlink itself.
    find "$LANE_DIR" -mindepth 1 \
        \( -path "$LANE_DIR/target" -o -path "$LANE_DIR/.git" \) -prune \
        -o -exec touch -h -d "2020-01-01T00:00:00" {} +

    # Touch the delta to now: explicit --touch paths first
    if [ "${#TOUCH_PATHS[@]}" -gt 0 ]; then
        info "Touching ${#TOUCH_PATHS[@]} explicit delta path(s) to now ..."
        touch "${TOUCH_PATHS[@]}"
    fi

    # Touch the delta from git diff --name-only when a base commit is known
    if [ -n "$BASE_COMMIT" ]; then
        info "Touching git diff --name-only $BASE_COMMIT paths to now ..."
        _touch_git_delta "$BASE_COMMIT"
    fi

    # If sidecar recorded a BASE_COMMIT and none was passed on CLI, use the sidecar one
    if [ -z "$BASE_COMMIT" ]; then
        RECORDED_BASE_COMMIT="$(_sidecar_read "$SIDECAR" "BASE_COMMIT")"
        if [ -n "$RECORDED_BASE_COMMIT" ]; then
            info "Using sidecar BASE_COMMIT=$RECORDED_BASE_COMMIT for git diff ..."
            _touch_git_delta "$RECORDED_BASE_COMMIT"
        fi
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
