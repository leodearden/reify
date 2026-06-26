#!/usr/bin/env bash
# scripts/warm-lane-gc-sweep.sh — Periodic GC backstop for the warm-lane CoW pool.
#
# Invokes warm-lane-gc.sh reclaim --mount <worktree_base> unconditionally on each
# run (no disk-guard gate — the point is to reclaim FREE divergent lanes proactively
# to PREVENT monotonic accretion BETWEEN acquires, before the disk floor is reached).
#
# Driven by deploy/systemd/reify-warm-lane-gc.timer (periodic oneshot).
# The on-demand dark-factory ε path (disk-guard check → reclaim → requeue on low disk)
# is separate; this wrapper is the BACKGROUND sweep that keeps the pool lean.
#
# Fail-open: if the warm-lane mount (worktree_base / --mount) does not exist,
# warn on stderr and exit 0 — the timer must degrade quietly on a cold host or
# before the loopback mount is provisioned (inv.6 cold-fallback philosophy).
#
# Part of PRD docs/prds/warm-lane-pool-space-safety.md §12 (δ GC trigger backstop).
#
# Usage:
#   scripts/warm-lane-gc-sweep.sh [--mount DIR] [--gc-script PATH]
#
# Options:
#   --mount DIR        Worktrees base dir passed to gc.sh --mount (default:
#                      /home/leo/src/warm-lanes/worktrees or
#                      $REIFY_WARM_LANE_GC_SWEEP_MOUNT).
#   --gc-script PATH   Path to warm-lane-gc.sh (default: sibling warm-lane-gc.sh;
#                      overridable for hermetic tests via $REIFY_WARM_LANE_GC_SWEEP_GC_SCRIPT).
#   -h, --help         Print this message and exit 0.
#
# Exit codes:
#   0  — Sweep attempted or skipped (fail-open).
#   1  — Runtime error propagated from gc.sh.
#   2  — Usage error: unknown flag.
#
# Env knobs:
#   REIFY_WARM_LANE_GC_SWEEP_MOUNT      — default --mount
#   REIFY_WARM_LANE_GC_SWEEP_GC_SCRIPT  — default --gc-script

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ── log helpers (all write to stderr) ──────────────────────────────────────────
_warn() { printf '[warm-lane-gc-sweep] WARN:  %s\n' "$*" >&2; }
_info() { printf '[warm-lane-gc-sweep] INFO:  %s\n' "$*" >&2; }

# ── usage ───────────────────────────────────────────────────────────────────────
_usage() {
    cat >&2 <<EOF
Usage: $(basename "$0") [--mount DIR] [--gc-script PATH]

  Periodic GC backstop for the warm-lane CoW pool.
  Invokes warm-lane-gc.sh reclaim --mount <DIR> unconditionally.
  Fail-open: if --mount DIR does not exist, warns and exits 0.

  Options:
    --mount DIR        Worktrees base dir (default: /home/leo/src/warm-lanes/worktrees).
    --gc-script PATH   Path to warm-lane-gc.sh (default: sibling warm-lane-gc.sh).
    -h, --help         Print this message and exit 0.

  Exit codes:
    0  — Sweep attempted or skipped (fail-open on missing mount).
    1  — Runtime error from gc.sh.
    2  — Usage error: unknown flag.
EOF
}

# ── defaults ───────────────────────────────────────────────────────────────────
MOUNT="${REIFY_WARM_LANE_GC_SWEEP_MOUNT:-/home/leo/src/warm-lanes/worktrees}"
GC_SCRIPT="${REIFY_WARM_LANE_GC_SWEEP_GC_SCRIPT:-$SCRIPT_DIR/warm-lane-gc.sh}"

# ── arg parsing ────────────────────────────────────────────────────────────────
while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            _usage; exit 0 ;;
        --mount)
            [ $# -ge 2 ] || { _warn "--mount requires a value"; exit 2; }
            MOUNT="$2"; shift 2 ;;
        --gc-script)
            [ $# -ge 2 ] || { _warn "--gc-script requires a value"; exit 2; }
            GC_SCRIPT="$2"; shift 2 ;;
        -*)
            _warn "Unknown flag: $1"
            _warn "Run '$(basename "$0") --help' for usage."
            exit 2 ;;
        *)
            _warn "Unexpected argument: $1"
            _warn "Run '$(basename "$0") --help' for usage."
            exit 2 ;;
    esac
done

# ── fail-open: skip if mount dir does not exist ────────────────────────────────
if [ ! -d "$MOUNT" ]; then
    _warn "warm-lane mount dir does not exist: $MOUNT — skipping GC sweep (fail-open)"
    exit 0
fi

# ── invoke gc.sh reclaim --mount <MOUNT> ──────────────────────────────────────
_info "running: $GC_SCRIPT reclaim --mount $MOUNT"
exec "$GC_SCRIPT" reclaim --mount "$MOUNT"
