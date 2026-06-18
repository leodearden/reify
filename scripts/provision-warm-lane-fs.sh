#!/usr/bin/env bash
# scripts/provision-warm-lane-fs.sh — Provision the XFS-reflink loopback volume
# used as the warm-lane CoW pool substrate.
#
# Usage:
#   mount=$(scripts/provision-warm-lane-fs.sh [--size-gib N] [--img PATH] [--mount DIR])
#   export REIFY_WARM_LANE_MOUNT=$mount
#
# Stdout:  ONLY the resolved mount directory (bare path, no trailing newline
#          beyond the echo).  Mirrors setup-worktree-debug-port.sh: the stdout
#          value is safe to capture with $(...).
# Stderr:  All diagnostics, progress messages, and errors.
#
# Invariants:
#   P1 (never reformat a populated image): if the image file exists with XFS
#      magic (blkid TYPE==xfs) it is NEVER reformatted — only re-attached and
#      remounted.  mkfs.xfs fires only when the image is absent or has no XFS
#      magic.  A currently-mounted image is an idempotent no-op (B1).
#   P2 (probe mandatory, fail-closed): after every mount or mount-verify step a
#      `cp --reflink=always` probe is run inside the mount.  Any failure prints
#      an actionable error to stderr and exits non-zero with NOTHING on stdout.
#      There is no silent cold-copy fallback.
#
# Defaults:
#   --size-gib  600           (overridable; PRD §9.1 / §13 Q1)
#   --img       /var/lib/reify-warm-lanes.img
#   --mount     ${REIFY_WARM_LANE_MOUNT:-<worktree_base>/warm-lanes}
#               (<worktree_base> is derived from REPO_ROOT's parent, ascending
#                past a `worktrees/` directory so that the default mount lives
#                next to the worktrees tree, not inside one worktree.)
#
# Privileged operations (fallocate into /var/lib, mkfs, losetup, mount, chown)
# are routed through $SUDO:
#   sudo       when EUID != 0
#   ''         when root
#   $REIFY_WARM_LANE_SUDO  override (set '' in tests to bypass sudo entirely)

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
_default_mount() {
    local parent
    parent="$(dirname "$REPO_ROOT")"
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
Usage: $(basename "$0") [--size-gib N] [--img PATH] [--mount DIR]

  Provision (or idempotently verify) the XFS-reflink loopback volume used as
  the warm-lane CoW pool substrate.

  Options:
    --size-gib N    Image size in GiB (default: 600)
    --img PATH      Image file path (default: /var/lib/reify-warm-lanes.img)
    --mount DIR     Mount point (default: \${REIFY_WARM_LANE_MOUNT:-$(_default_mount)})
    -h, --help      Print this message and exit

  Stdout:  the resolved mount directory (bare path)
  Stderr:  all diagnostics

  Invariants:
    P1 — never reformat a populated image (guard on existing XFS magic)
    P2 — reflink probe mandatory and fail-closed (cp --reflink=always)

  \$SUDO override: set REIFY_WARM_LANE_SUDO='' to bypass sudo (for tests).
EOF
}

# ── arg parsing ────────────────────────────────────────────────────────────────
SIZE_GIB=600
IMG="/var/lib/reify-warm-lanes.img"
MOUNT="${REIFY_WARM_LANE_MOUNT:-$(_default_mount)}"

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            _usage
            exit 0
            ;;
        --size-gib)
            [ $# -ge 2 ] || { err "--size-gib requires a value"; exit 2; }
            SIZE_GIB="$2"
            shift 2
            ;;
        --img)
            [ $# -ge 2 ] || { err "--img requires a value"; exit 2; }
            IMG="$2"
            shift 2
            ;;
        --mount)
            [ $# -ge 2 ] || { err "--mount requires a value"; exit 2; }
            MOUNT="$2"
            shift 2
            ;;
        *)
            err "Unknown flag: $1"
            err "Run '$(basename "$0") --help' for usage."
            exit 2
            ;;
    esac
done

# ── $SUDO indirection ──────────────────────────────────────────────────────────
# Override: REIFY_WARM_LANE_SUDO (set to '' in tests to bypass sudo entirely)
if [ -n "${REIFY_WARM_LANE_SUDO+x}" ]; then
    SUDO="${REIFY_WARM_LANE_SUDO}"
elif [ "$(id -u)" -ne 0 ]; then
    SUDO="sudo"
else
    SUDO=""
fi

# ── reflink probe (P2) ─────────────────────────────────────────────────────────
# Mandatory on every success path.  Uses cp --reflink=always (NOT auto) so a
# non-reflink mount fails loudly rather than silently falling back to a full copy.
_probe_reflink() {
    local mount_dir="$1"
    local probe_dir="$mount_dir/.reflink-probe-$$"
    mkdir -p "$probe_dir"
    echo "probe" > "$probe_dir/src"
    if ! cp --reflink=always "$probe_dir/src" "$probe_dir/dst" 2>&1; then
        rm -rf "$probe_dir" 2>/dev/null || true
        err "Reflink probe FAILED at $mount_dir — provisioning aborted."
        err "The filesystem at $mount_dir does not support reflinks."
        err "Refusing to fall back to cold copies (P2 invariant)."
        exit 1
    fi
    rm -rf "$probe_dir" 2>/dev/null || true
}

# ── main ──────────────────────────────────────────────────────────────────────

info "provision-warm-lane-fs.sh: img=$IMG  mount=$MOUNT  size=${SIZE_GIB}GiB"

# ── B1 / P1: idempotent no-op — already mounted and probe passes ───────────────
if [ -f "$IMG" ] && mountpoint -q "$MOUNT"; then
    info "Image $IMG is already mounted at $MOUNT; verifying reflink probe (P2)..."
    _probe_reflink "$MOUNT"
    ok "Warm-lane volume already provisioned and healthy at $MOUNT"
    echo "$MOUNT"
    exit 0
fi

# ── P1: existing image with XFS magic — re-attach and mount, never reformat ───
if [ -f "$IMG" ]; then
    _img_type="$($SUDO blkid -o value -s TYPE "$IMG" 2>/dev/null || true)"
    if [ "$_img_type" = "xfs" ]; then
        info "Image $IMG has XFS magic — re-attaching (P1: never reformat a populated image)..."
        LOOP="$($SUDO losetup --find --show "$IMG")"
        info "Attached $IMG to $LOOP"
        mkdir -p "$MOUNT"
        $SUDO mount "$LOOP" "$MOUNT"
        $SUDO chown "$(id -u):$(id -g)" "$MOUNT"
        info "Mounted $LOOP at $MOUNT"
        _probe_reflink "$MOUNT"
        ok "Warm-lane volume re-mounted at $MOUNT"
        echo "$MOUNT"
        exit 0
    fi
    # Image exists but has no XFS magic — fall through to provision from scratch
    warn "Image $IMG exists but has no XFS magic (type='$_img_type'); reprovisioning."
fi

# ── Fresh provision ────────────────────────────────────────────────────────────
info "Allocating ${SIZE_GIB} GiB image at $IMG ..."
$SUDO fallocate -l "${SIZE_GIB}GiB" "$IMG"

info "Formatting $IMG as XFS with reflink=1,bigtime=1 ..."
$SUDO mkfs.xfs -f -m reflink=1,bigtime=1 "$IMG"

info "Attaching $IMG to loop device ..."
LOOP="$($SUDO losetup --find --show "$IMG")"
info "Attached to $LOOP"

mkdir -p "$MOUNT"
$SUDO mount "$LOOP" "$MOUNT"
$SUDO chown "$(id -u):$(id -g)" "$MOUNT"
info "Mounted $LOOP at $MOUNT"

_probe_reflink "$MOUNT"

ok "Warm-lane volume provisioned at $MOUNT"

# STDOUT contract: ONLY this line goes to stdout.
echo "$MOUNT"
