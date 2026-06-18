#!/usr/bin/env bash
# scripts/warm-lane-preflight.sh — Fail-closed preflight guard for the warm-lane
# CoW pool. Mirrors scripts/check-manifold-deps.sh: fast, filesystem-only checks,
# exit 0 on all-pass / non-zero with an actionable hint() naming the remediation.
#
# Usage:
#   scripts/warm-lane-preflight.sh [--mount DIR] [--base-dir DIR] [--invocation FP]
#
# Options (env defaults shown):
#   --mount DIR         Warm-lane mount point  (env: REIFY_WARM_LANE_MOUNT)
#   --base-dir DIR      Base directory to check (env: REIFY_WARM_LANE_BASE;
#                       default: <mount>/base/target)
#   --invocation FP     Expected invocation fingerprint (env: REIFY_WARM_LANE_INVOCATION;
#                       default: '' — empty stamp is acceptable)
#   -h, --help          Print this message and exit.
#
# Checks (in order):
#   1. Volume mounted   (mountpoint -q <mount>)
#   2. Reflink-capable  (cp --reflink=always probe inside the mount)
#   3. Base present     (<base_dir> exists and is non-empty)
#   4. Invocation match (<base_dir>.invocation stamp == expected)
#   5. RUSTFLAGS match  (<base_dir>.rustflags stamp == ${RUSTFLAGS:-})
#
# On any failure: non-zero exit + actionable stderr naming the remediation script.
# Sidecar stamp convention: <base_dir>.rustflags and <base_dir>.invocation are
# siblings of <base_dir> (NOT inside it), written by refresh-warm-base.sh.

set -euo pipefail

# ── log helpers (all write to stderr) ─────────────────────────────────────────
info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*" >&2; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*" >&2; }
err()   { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }

hint()  { err "Run:  $*"; }

# ── usage ──────────────────────────────────────────────────────────────────────
_usage() {
    cat >&2 <<EOF
Usage: $(basename "$0") [--mount DIR] [--base-dir DIR] [--invocation FP]

  Fail-closed preflight guard for the warm-lane CoW pool.
  Runs 5 fast filesystem checks; exits 0 on all-pass, non-zero with an
  actionable message on any failure.

  Options:
    --mount DIR       Warm-lane mount point (default: \$REIFY_WARM_LANE_MOUNT)
    --base-dir DIR    Base directory (default: \$REIFY_WARM_LANE_BASE or <mount>/base/target)
    --invocation FP   Expected invocation fingerprint (default: \$REIFY_WARM_LANE_INVOCATION)
    -h, --help        Print this message and exit.

  Checks:
    1. Volume mounted     (mountpoint -q <mount>)
    2. Reflink-capable    (cp --reflink=always probe)
    3. Base present       (<base_dir> exists and is non-empty)
    4. Invocation match   (<base_dir>.invocation stamp == expected)
    5. RUSTFLAGS match    (<base_dir>.rustflags stamp == \${RUSTFLAGS:-})
EOF
}

# ── arg parsing ────────────────────────────────────────────────────────────────
MOUNT="${REIFY_WARM_LANE_MOUNT:-}"
BASE_DIR="${REIFY_WARM_LANE_BASE:-}"
INVOCATION="${REIFY_WARM_LANE_INVOCATION:-}"

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            _usage; exit 0 ;;
        --mount)
            [ $# -ge 2 ] || { err "--mount requires a value"; exit 2; }
            MOUNT="$2"; shift 2 ;;
        --base-dir)
            [ $# -ge 2 ] || { err "--base-dir requires a value"; exit 2; }
            BASE_DIR="$2"; shift 2 ;;
        --invocation)
            [ $# -ge 2 ] || { err "--invocation requires a value"; exit 2; }
            INVOCATION="$2"; shift 2 ;;
        *)
            err "Unknown flag: $1"
            err "Run '$(basename "$0") --help' for usage."
            exit 2 ;;
    esac
done

# Resolve defaults
if [ -z "$MOUNT" ]; then
    err "Warm-lane mount not specified. Set REIFY_WARM_LANE_MOUNT or pass --mount DIR."
    err "Run '$(basename "$0") --help' for usage."
    exit 2
fi
if [ -z "$BASE_DIR" ]; then
    BASE_DIR="$MOUNT/base/target"
fi

info "warm-lane-preflight.sh: mount=$MOUNT  base=$BASE_DIR"

# ── Check 1: volume mounted ────────────────────────────────────────────────────
info "Check 1: volume mounted at $MOUNT ..."
if ! mountpoint -q "$MOUNT"; then
    err "Warm-lane volume is not mounted at $MOUNT."
    err "The warm-lane CoW pool must be provisioned and mounted before use."
    hint "scripts/provision-warm-lane-fs.sh --mount $MOUNT"
    exit 1
fi
ok "Check 1: volume mounted."

# ── Check 2: reflink-capable ───────────────────────────────────────────────────
info "Check 2: reflink probe inside $MOUNT ..."
_probe_dir="$MOUNT/.preflight-probe-$$"
mkdir -p "$_probe_dir"
printf 'probe' > "$_probe_dir/src"
if ! cp --reflink=always "$_probe_dir/src" "$_probe_dir/dst" 2>/dev/null; then
    rm -rf "$_probe_dir" 2>/dev/null || true
    err "Reflink probe FAILED at $MOUNT — the filesystem does not support reflinks."
    err "Refusing to use a non-reflink mount (invariant P2 — no silent cold-copy fallback)."
    hint "scripts/provision-warm-lane-fs.sh --mount $MOUNT"
    exit 1
fi
rm -rf "$_probe_dir" 2>/dev/null || true
ok "Check 2: reflink-capable."

# ── Check 3: base present and non-empty ────────────────────────────────────────
info "Check 3: base present and non-empty at $BASE_DIR ..."
if [ ! -d "$BASE_DIR" ] || [ -z "$(ls -A "$BASE_DIR" 2>/dev/null)" ]; then
    err "Warm-lane base is missing or empty: $BASE_DIR"
    err "The base must be seeded before pooled verifies can use it."
    hint "scripts/refresh-warm-base.sh <advancing_target_dir> $BASE_DIR"
    exit 1
fi
ok "Check 3: base present."

# ── Check 4: invocation stamp match ───────────────────────────────────────────
info "Check 4: invocation stamp match ..."
_stamp_inv="$BASE_DIR.invocation"
if [ ! -f "$_stamp_inv" ]; then
    err "Invocation stamp missing: $_stamp_inv"
    err "Expected invocation: '$INVOCATION'"
    err "The base may have been seeded without an invocation stamp."
    hint "scripts/refresh-warm-base.sh <advancing_target_dir> $BASE_DIR --invocation '$INVOCATION'"
    exit 1
fi
_actual_inv="$(cat "$_stamp_inv")"
if [ "$_actual_inv" != "$INVOCATION" ]; then
    err "Invocation mismatch: base was built with '$_actual_inv', expected '$INVOCATION'."
    err "A mismatched invocation means the base may contain a different build configuration."
    hint "scripts/refresh-warm-base.sh <advancing_target_dir> $BASE_DIR --invocation '$INVOCATION'"
    exit 1
fi
ok "Check 4: invocation match."

# ── Check 5: RUSTFLAGS stamp match ────────────────────────────────────────────
info "Check 5: RUSTFLAGS stamp match ..."
_stamp_rf="$BASE_DIR.rustflags"
_expected_rf="${RUSTFLAGS:-}"
if [ ! -f "$_stamp_rf" ]; then
    err "RUSTFLAGS stamp missing: $_stamp_rf"
    err "Expected RUSTFLAGS: '$_expected_rf'"
    err "Using the base with a different RUSTFLAGS risks a cold rebuild of all crates."
    hint "scripts/refresh-warm-base.sh <advancing_target_dir> $BASE_DIR"
    exit 1
fi
_actual_rf="$(cat "$_stamp_rf")"
if [ "$_actual_rf" != "$_expected_rf" ]; then
    err "RUSTFLAGS mismatch: base was built with RUSTFLAGS='$_actual_rf',"
    err "  but the current environment has RUSTFLAGS='$_expected_rf'."
    err "Using the base with a different RUSTFLAGS risks a cold rebuild of all crates (D4)."
    hint "scripts/refresh-warm-base.sh <advancing_target_dir> $BASE_DIR"
    exit 1
fi
ok "Check 5: RUSTFLAGS match."

ok "warm-lane-preflight: all checks passed."
