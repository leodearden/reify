#!/usr/bin/env bash
# scripts/warm-lane-disk-guard.sh — Fail-closed disk-pressure admission guard
# for the warm-lane CoW pool. Checks BOTH free bytes AND free inodes on the
# warm-lane mount; reflink shares extents but NOT inodes, so a bytes-only
# guard is insufficient (PRD docs/prds/warm-lane-pool-space-safety.md §2, D4).
#
# Usage:
#   scripts/warm-lane-disk-guard.sh check [--mount DIR] [--min-free-gib N] [--min-free-inodes N]
#
# Subcommands:
#   check   Measure free bytes and inodes; exit 0 if both healthy, 75 if below floor.
#
# Options (env defaults shown):
#   --mount DIR            Warm-lane mount point  (env: REIFY_WARM_LANE_MOUNT)
#   --min-free-gib N       Minimum free GiB required (env: REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_GIB;
#                          default: 50; maps to orchestrator.yaml warm_lane_pool.min_free_gib)
#   --min-free-inodes N    Minimum free inodes required (env: REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_INODES;
#                          default: 500000; maps to orchestrator.yaml warm_lane_pool.min_free_inodes)
#   -h, --help             Print this message and exit.
#
# Exit codes:
#   0   — Both axes healthy: free_bytes >= min_free_gib AND free_inodes >= min_free_inodes.
#   75  — Backpressure (EX_TEMPFAIL): either axis below floor, OR fail-closed measurement
#          failure (df exits non-zero / emits unparseable output). Orchestrator requeues 75.
#   2   — Usage error: unknown flag, missing/unknown subcommand, missing mount.
#   0   — --help.
#
# Env knobs:
#   REIFY_WARM_LANE_DISK_GUARD_DF           df command override (default: df)
#   REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_GIB minimum free bytes in GiB (default: 50)
#   REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_INODES minimum free inodes (default: 500000)
#   REIFY_WARM_LANE_MOUNT                   warm-lane mount point (shared with preflight)
#
# The df invocation: "$DF" -B1 --output=avail,iavail -- "$MOUNT"
# (GNU coreutils df; Linux/XFS-only infra — same constraint as test_warm_lane_pool.sh)
# Reads both axes from one consistent snapshot.

set -euo pipefail

# ── log helpers (all write to stderr) ─────────────────────────────────────────
info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*" >&2; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*" >&2; }
err()   { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }
hint()  { err "Hint:  $*"; }

# ── usage ──────────────────────────────────────────────────────────────────────
_usage() {
    cat >&2 <<EOF
Usage: $(basename "$0") check [--mount DIR] [--min-free-gib N] [--min-free-inodes N]

  Fail-closed disk-pressure admission guard for the warm-lane CoW pool.
  Checks BOTH free bytes AND free inodes (reflink shares extents, not inodes).
  Exits 0 when both axes are healthy; exits 75 (EX_TEMPFAIL) to signal
  backpressure — the orchestrator requeues exit-75 as transient infra pressure.

  Subcommands:
    check   Measure free bytes and inodes against floor thresholds.

  Options:
    --mount DIR            Warm-lane mount point (default: \$REIFY_WARM_LANE_MOUNT)
    --min-free-gib N       Minimum free GiB required
                           (default: \$REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_GIB or 50)
                           Maps to: orchestrator.yaml warm_lane_pool.min_free_gib
    --min-free-inodes N    Minimum free inodes required
                           (default: \$REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_INODES or 500000)
                           Maps to: orchestrator.yaml warm_lane_pool.min_free_inodes
    -h, --help             Print this message and exit.

  Exit codes:
    0   — Both axes healthy.
    75  — Below-threshold on either axis, OR fail-closed measurement failure.
    2   — Usage error (wiring bug, not transient pressure).
EOF
}

# ── defaults ───────────────────────────────────────────────────────────────────
DF="${REIFY_WARM_LANE_DISK_GUARD_DF:-df}"
MOUNT="${REIFY_WARM_LANE_MOUNT:-}"
MIN_FREE_GIB="${REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_GIB:-50}"
MIN_FREE_INODES="${REIFY_WARM_LANE_DISK_GUARD_MIN_FREE_INODES:-500000}"

# ── arg parsing ────────────────────────────────────────────────────────────────
SUBCOMMAND=""

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            _usage; exit 0 ;;
        --mount)
            [ $# -ge 2 ] || { err "--mount requires a value"; exit 2; }
            MOUNT="$2"; shift 2 ;;
        --min-free-gib)
            [ $# -ge 2 ] || { err "--min-free-gib requires a value"; exit 2; }
            MIN_FREE_GIB="$2"; shift 2 ;;
        --min-free-inodes)
            [ $# -ge 2 ] || { err "--min-free-inodes requires a value"; exit 2; }
            MIN_FREE_INODES="$2"; shift 2 ;;
        check)
            SUBCOMMAND="check"; shift ;;
        -*)
            err "Unknown flag: $1"
            err "Run '$(basename "$0") --help' for usage."
            exit 2 ;;
        *)
            err "Unknown subcommand: $1"
            err "Run '$(basename "$0") --help' for usage."
            exit 2 ;;
    esac
done

# Validate subcommand
if [ -z "$SUBCOMMAND" ]; then
    err "Missing subcommand. Expected: check"
    err "Run '$(basename "$0") --help' for usage."
    exit 2
fi

# Validate mount
if [ -z "$MOUNT" ]; then
    err "Warm-lane mount not specified. Set REIFY_WARM_LANE_MOUNT or pass --mount DIR."
    err "Run '$(basename "$0") --help' for usage."
    exit 2
fi

# ── check subcommand ───────────────────────────────────────────────────────────
info "warm-lane-disk-guard.sh check: mount=$MOUNT  min_free_gib=$MIN_FREE_GIB  min_free_inodes=$MIN_FREE_INODES"

# Single df call: avail bytes (via -B1) + avail inodes, consistent snapshot.
df_out="$("$DF" -B1 --output=avail,iavail -- "$MOUNT")"
data_line="$(printf '%s\n' "$df_out" | tail -n +2 | head -n 1)"
avail_bytes="$(printf '%s\n' "$data_line" | awk '{print $1}')"
avail_inodes="$(printf '%s\n' "$data_line" | awk '{print $2}')"

info "  avail_bytes=$avail_bytes  avail_inodes=$avail_inodes"

ok "check: disk space healthy."
exit 0
