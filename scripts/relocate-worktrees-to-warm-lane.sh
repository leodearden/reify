#!/usr/bin/env bash
# scripts/relocate-worktrees-to-warm-lane.sh — Relocate <repo>/.worktrees onto
# the XFS warm-lane mount via a path-stable symlink (DA2).
#
# Usage:
#   dest=$(scripts/relocate-worktrees-to-warm-lane.sh [--repo DIR] [--mount DIR] \
#                                                      [--worktree-dirname NAME])
#   # .worktrees is now a symlink → <mount>/worktrees
#
# Stdout:  ONLY the resolved <mount>/worktrees destination path.
#          Mirrors provision-warm-lane-fs.sh: safe to capture with $(...).
# Stderr:  All diagnostics, progress messages, and errors.
#
# What it does (DA2 path-stable symlink):
#   1. Probe the mount for reflink support (P2 invariant — fail-closed).
#   2. If <repo>/<worktree-dirname> is ABSENT       → create <mount>/worktrees
#      and `ln -s <mount>/worktrees <repo>/<name>`.
#   3. If it is a SYMLINK already pointing at DEST  → idempotent no-op (exit 0).
#   4. If it is a SYMLINK pointing elsewhere         → refuse (exit non-zero).
#   5. If it is a real DIRECTORY                     → mv each entry into
#      <mount>/worktrees/, rmdir, then create the symlink.
#
# git.worktree_dir (.worktrees) stays UNCHANGED in orchestrator.yaml — the
# symlink makes DF's `worktree_base = (project_root / worktree_dir).resolve()`
# follow it onto XFS (PRD DA2, task 4696).
#
# Pool stays OFF (git.warm_lane_pool absent → GitConfig default False).
# base/ seeding is R4's job; this script does NOT create base/.
#
# Defaults:
#   --repo              REPO_ROOT (directory containing this script's parent)
#   --mount             ${REIFY_WARM_LANE_MOUNT:-<_default_mount>}
#   --worktree-dirname  .worktrees
#
# Note: run this with no in-flight task worktrees (#4665 maintenance window).

set -euo pipefail

# ── log helpers (all write to stderr) ─────────────────────────────────────────
info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*" >&2; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*" >&2; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*" >&2; }
err()   { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }

# ── locate repo root ───────────────────────────────────────────────────────────
_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$_SCRIPT_DIR/.." && pwd)"

# ── default mount dir: ascend past worktrees/ if present ──────────────────────
# Mirrors provision-warm-lane-fs.sh _default_mount() so both scripts agree on
# the default mount path.  Accepts an optional first argument as the repo root
# (defaults to REPO_ROOT) so the default is computed relative to the RESOLVED
# --repo value, not the script's own location.
_default_mount() {
    local repo="${1:-$REPO_ROOT}"
    local parent
    parent="$(dirname "$repo")"
    # If the repo root is inside a worktrees/ directory, surface one level higher
    # so the warm-lanes dir lives beside the worktrees tree, not inside a worktree.
    if [ "$(basename "$parent")" = "worktrees" ]; then
        parent="$(dirname "$parent")"
    fi
    echo "$parent/warm-lanes"
}

# ── usage ──────────────────────────────────────────────────────────────────────
_usage() {
    cat >&2 <<EOF
Usage: $(basename "$0") [--repo DIR] [--mount DIR] [--worktree-dirname NAME]

  Relocate <repo>/<worktree-dirname> onto the XFS warm-lane mount via a
  path-stable symlink (DA2), so DF's worktree_base follows it onto XFS.

  Options:
    --repo DIR              Repo root (default: REPO_ROOT = ${REPO_ROOT})
    --mount DIR             Warm-lane mount point
                            (default: \${REIFY_WARM_LANE_MOUNT:-$(_default_mount)})
    --worktree-dirname NAME Worktree dirname inside repo (default: .worktrees)
    -h, --help              Print this message and exit

  Stdout:  the resolved <mount>/worktrees destination path
  Stderr:  all diagnostics

  Run with no in-flight task worktrees (#4665 maintenance window).
  The mount must be provisioned first (run scripts/provision-warm-lane-fs.sh).
EOF
}

# ── arg parsing ────────────────────────────────────────────────────────────────
REPO="${REPO_ROOT}"
MOUNT=""            # resolved AFTER arg parsing so it uses the --repo value
_MOUNT_SET=0        # tracks whether --mount was given explicitly
WORKTREE_DIRNAME=".worktrees"

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            _usage
            exit 0
            ;;
        --repo)
            [ $# -ge 2 ] || { err "--repo requires a value"; exit 2; }
            REPO="$2"
            shift 2
            ;;
        --mount)
            [ $# -ge 2 ] || { err "--mount requires a value"; exit 2; }
            MOUNT="$2"
            _MOUNT_SET=1
            shift 2
            ;;
        --worktree-dirname)
            [ $# -ge 2 ] || { err "--worktree-dirname requires a value"; exit 2; }
            WORKTREE_DIRNAME="$2"
            shift 2
            ;;
        *)
            err "Unknown flag: $1"
            err "Run '$(basename "$0") --help' for usage."
            exit 2
            ;;
    esac
done

# ── resolve MOUNT default (deferred: derived from resolved REPO, not REPO_ROOT) ─
# Using --repo X without --mount should compute the default relative to X.
if [ "$_MOUNT_SET" -eq 0 ]; then
    MOUNT="${REIFY_WARM_LANE_MOUNT:-$(_default_mount "$REPO")}"
fi

# ── validate mount exists ──────────────────────────────────────────────────────
if [ ! -d "$MOUNT" ]; then
    err "Mount directory does not exist: $MOUNT"
    err "Provision the XFS warm-lane volume first: scripts/provision-warm-lane-fs.sh"
    exit 1
fi

# ── reflink probe (mirrors provision P2) ──────────────────────────────────────
# Fail-closed: refuse to relocate onto a non-reflink filesystem.
# Stubbable via PATH (REIFY_TEST_REFLINK_OK=1 → cp exits 0 in tests).
_probe_reflink() {
    local mount_dir="$1"
    local probe_dir="$mount_dir/.reflink-probe-$$"
    mkdir -p "$probe_dir"
    echo "probe" > "$probe_dir/src"
    if ! cp --reflink=always "$probe_dir/src" "$probe_dir/dst"; then
        rm -rf "$probe_dir" 2>/dev/null || true
        err "Reflink probe FAILED at $mount_dir — relocation aborted."
        err "The filesystem at $mount_dir does not support reflinks."
        err "Refusing to fall back to cold copies (P2 invariant)."
        err "Provision the XFS warm-lane volume first: scripts/provision-warm-lane-fs.sh"
        exit 1
    fi
    rm -rf "$probe_dir" 2>/dev/null || true
}

# ── main ──────────────────────────────────────────────────────────────────────
LINK="$REPO/$WORKTREE_DIRNAME"
DEST="$MOUNT/worktrees"

info "relocate-worktrees-to-warm-lane.sh: repo=$REPO  mount=$MOUNT  dirname=$WORKTREE_DIRNAME"
info "link=$LINK  dest=$DEST"

# P2: probe reflink capability before any destructive operation
_probe_reflink "$MOUNT"
ok "Reflink probe passed at $MOUNT"

# ── Branch: what is LINK currently? ───────────────────────────────────────────
# mkdir -p "$DEST" is deferred to the create/migrate branches only, so a
# wrong-target refuse leaves the filesystem completely untouched.

if [ -L "$LINK" ]; then
    # LINK is a symlink — check its target
    _existing_target="$(readlink -f "$LINK" 2>/dev/null || readlink "$LINK")"
    _dest_resolved="$(readlink -f "$DEST" 2>/dev/null || echo "$DEST")"
    if [ "$_existing_target" = "$_dest_resolved" ]; then
        # Idempotent: already pointing at DEST
        ok "Idempotent: $LINK already points to $DEST — no changes needed"
    else
        err "Refusing to clobber unexpected symlink target."
        err "  $LINK -> $_existing_target"
        err "  Expected: $_dest_resolved"
        err "Re-run with the correct --mount or remove the symlink manually."
        exit 1
    fi

elif [ -d "$LINK" ]; then
    # LINK is a real directory — migrate entries to DEST then replace with symlink
    warn "Relocating: $LINK is a real directory; moving entries into $DEST"
    warn "Run with no in-flight task worktrees (#4665 maintenance window)."

    # Create DEST only now (after all guards), so a refused run is a pure no-op.
    mkdir -p "$DEST"

    # Enable dotglob so .hidden entries (like _merge-verify) are included,
    # and nullglob so an empty directory doesn't fail.
    _entries=()
    _old_opts="$(shopt -p dotglob nullglob 2>/dev/null || true)"
    shopt -s dotglob nullglob
    for _entry in "$LINK"/*; do
        _entries+=("$_entry")
    done
    # Restore shell options
    eval "$_old_opts" 2>/dev/null || true

    for _src in "${_entries[@]+${_entries[@]}}"; do
        _name="$(basename "$_src")"
        _dst="$DEST/$_name"
        if [ -e "$_dst" ] || [ -L "$_dst" ]; then
            err "Collision: $DEST/$_name already exists — refusing to clobber."
            err "Remove or rename $DEST/$_name before re-running."
            exit 1
        fi
        info "Moving: $_src → $_dst"
        mv "$_src" "$_dst"
    done

    # Directory should now be empty; remove it
    rmdir "$LINK"
    info "Removed empty directory $LINK"

    # Create the symlink
    ln -s "$DEST" "$LINK"
    ok "Created symlink: $LINK -> $DEST"

else
    # LINK does not exist — create DEST and symlink
    mkdir -p "$DEST"
    ln -s "$DEST" "$LINK"
    ok "Created symlink: $LINK -> $DEST"
fi

# ── Post-creation verification ────────────────────────────────────────────────
_link_resolved="$(readlink -f "$LINK" 2>/dev/null || echo "")"
_dest_resolved="$(readlink -f "$DEST" 2>/dev/null || echo "$DEST")"
if [ "$_link_resolved" != "$_dest_resolved" ]; then
    err "Verification failed: $LINK does not resolve to $DEST"
    err "  Resolved: $_link_resolved"
    err "  Expected: $_dest_resolved"
    exit 1
fi

# ── Best-effort git worktree repair ───────────────────────────────────────────
# Empirically a no-op for correctness (mv+symlink preserves registration),
# but harmless defense against stale recorded paths.
if git -C "$REPO" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    git -C "$REPO" worktree repair >&2 2>/dev/null || true
    info "git worktree repair completed (best-effort)"
fi

# ── Soft post-check: warn if .gitignore won't cover the symlink ───────────────
if git -C "$REPO" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    if ! git -C "$REPO" check-ignore -q "$WORKTREE_DIRNAME" 2>/dev/null; then
        warn "Symlink '$WORKTREE_DIRNAME' is NOT covered by .gitignore."
        warn "Add a no-slash entry: echo '$WORKTREE_DIRNAME' >> .gitignore"
        warn "(Trailing-slash gitignore patterns do not match symlinks.)"
    fi
fi

ok "Relocation complete: $LINK -> $DEST"

# STDOUT contract: ONLY this line goes to stdout.
echo "$DEST"
