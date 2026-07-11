#!/usr/bin/env bash
# scripts/warm-lane-audit.sh — Standalone, timer-friendly audit/telemetry
# report for the warm-lane CoW pool. Read-only observability: never mutates a
# lane and never gates dispatch/reclaim/merge (PRD §9.5 inv.12).
#
# Part of PRD docs/prds/warm-lane-pool-sizing-lifecycle.md §9.1, §10 B1/B2.
# Consumer: reify ζ (integration gate), dark-factory θ (soft-floor dispatch
# throttle), the operator, and a health timer (§13 open-Q 3).
#
# Usage:
#   scripts/warm-lane-audit.sh [--mount WORKTREES_DIR] [--format table|json] \
#       [--status-cmd CMD] [--stale-age-min N] [--main-ref REF] [--safety N]
#
# For each resident worktree under --mount (lanes _lane-*/_spec-* and orphan
# git-worktree dirs), emits: lane · role · ASSIGNED|FREE · branch ·
# backing-task-status (terminal|non-terminal|unknown) · recoverable
# (LANDED|PUSHED|ORPHAN) · dirty (clean|residue-only|wip) · divergent_gib ·
# age_min · classification (LIVE|RECLAIMABLE|LEAKED|PRESERVED-OK). A trailing
# HEADROOM line summarizes: resident/assigned/free/reclaimable/leaked counts
# + divergent_gib/free_gib/budget_gib.
#
# Options (env defaults shown):
#   --mount DIR           Warm-lane worktrees dir (env: REIFY_WARM_LANE_MOUNT;
#                         shared with warm-lane-preflight.sh / warm-lane-gc.sh).
#                         A nonexistent/empty mount reports resident=0 (not
#                         an error — this script is advisory-only).
#   --format table|json  Output format (default: table).
#   --status-cmd CMD     Backing-task status oracle: invoked as `<cmd> <id>`,
#                         expected to print a status (e.g. done/cancelled/
#                         pending) to stdout. Empty output or a non-zero exit
#                         is treated as unknown. Default: REIFY_LANE_LEAK_STATUS_CMD
#                         (the same oracle warm-lane-preflight.sh Check 6 /
#                         warm-lane-gc.sh consume — D6, no new status plumbing).
#   --stale-age-min N     Minutes; a LEAKED candidate is stale when
#                         age_min >= N (env: REIFY_WARM_LANE_AUDIT_STALE_AGE_MIN;
#                         default: 60).
#   --main-ref REF        Git ref for "main" (env: REIFY_WARM_LANE_AUDIT_MAIN_REF;
#                         default: main).
#   --safety N            Dimensionless divisor for budget_gib = floor(free_gib
#                         / safety) (env: REIFY_WARM_LANE_AUDIT_SAFETY; default:
#                         1.5, mirroring the illustrative safety factor in PRD
#                         §9.2's worked example). Must be > 0.
#   -h, --help            Print this message and exit.
#
# Additional env knobs (no dedicated CLI flag):
#   REIFY_WARM_LANE_AUDIT_DF             df command override (default: df),
#                                         mirrors REIFY_WARM_LANE_DISK_GUARD_DF.
#   REIFY_WARM_LANE_AUDIT_RESIDUE_GLOB   Glob (or comma-separated globs)
#                                         matching "residue" dirty paths that
#                                         don't count as unrecoverable WIP
#                                         (default: data/queue/*.db*, the
#                                         §2/D1 write_queue.db residue).
#
# Exit codes:
#   0  — Always, on every valid invocation (advisory/observability — this
#        script must never gate anything; PRD §9.5 inv.12). A status-lookup
#        failure, a df/du measurement failure, or a nonexistent/empty mount
#        all degrade gracefully rather than aborting.
#   2  — Usage error: unknown flag, missing flag value, invalid --format/
#        --stale-age-min/--safety.
#
# Invariants:
#   A1 — read-only: never mutates a lane (no reset/rm/reclaim); the ASSIGNED/
#        FREE probe opens an EXISTING <dir>.lock read-only and never creates
#        a missing one.
#   A2 — the flock -n -x probe is non-blocking and released immediately.
#   A3 — a status-lookup failure degrades that lane to `unknown` (never
#        aborts, never reclassifies as reclaimable/leaked).
#   A4 — `stale` is always the relation age_min >= stale_age_min against the
#        declared knob — never an inline/undeclared literal.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck source=scripts/lib_portable.sh
source "$SCRIPT_DIR/lib_portable.sh"

# ── log helpers (all write to stderr) ─────────────────────────────────────────
info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*" >&2; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*" >&2; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*" >&2; }
err()   { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }

# ── usage ──────────────────────────────────────────────────────────────────────
_usage() {
    cat >&2 <<EOF
Usage: $(basename "$0") [--mount DIR] [--format table|json] [--status-cmd CMD]
                       [--stale-age-min N] [--main-ref REF] [--safety N]

  Standalone, timer-friendly audit/telemetry report for the warm-lane CoW
  pool. Read-only: never mutates a lane; never gates dispatch/reclaim/merge.

  Options:
    --mount DIR           Warm-lane worktrees dir (default: \$REIFY_WARM_LANE_MOUNT).
    --format table|json   Output format (default: table).
    --status-cmd CMD      Backing-task status oracle (default:
                          \$REIFY_LANE_LEAK_STATUS_CMD).
    --stale-age-min N     Minutes before a candidate is stale (default:
                          \$REIFY_WARM_LANE_AUDIT_STALE_AGE_MIN or 60).
    --main-ref REF        Git ref for "main" (default: \$REIFY_WARM_LANE_AUDIT_MAIN_REF
                          or main).
    --safety N            Divisor for budget_gib = floor(free_gib / safety)
                          (default: \$REIFY_WARM_LANE_AUDIT_SAFETY or 1.5).
    -h, --help            Print this message and exit.

  Exit codes:
    0  — Always, on every valid invocation (advisory-only; never gates).
    2  — Usage error.
EOF
}

# ── defaults ───────────────────────────────────────────────────────────────────
MOUNT="${REIFY_WARM_LANE_MOUNT:-}"
FORMAT="table"
STATUS_CMD="${REIFY_LANE_LEAK_STATUS_CMD:-}"
STALE_AGE_MIN="${REIFY_WARM_LANE_AUDIT_STALE_AGE_MIN:-60}"
MAIN_REF="${REIFY_WARM_LANE_AUDIT_MAIN_REF:-main}"
SAFETY="${REIFY_WARM_LANE_AUDIT_SAFETY:-1.5}"
DF="${REIFY_WARM_LANE_AUDIT_DF:-df}"
RESIDUE_GLOB="${REIFY_WARM_LANE_AUDIT_RESIDUE_GLOB:-data/queue/*.db*}"

# ── arg parsing ────────────────────────────────────────────────────────────────
while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            _usage; exit 0 ;;
        --mount)
            [ $# -ge 2 ] || { err "--mount requires a value"; exit 2; }
            MOUNT="$2"; shift 2 ;;
        --format)
            [ $# -ge 2 ] || { err "--format requires a value"; exit 2; }
            FORMAT="$2"; shift 2 ;;
        --status-cmd)
            [ $# -ge 2 ] || { err "--status-cmd requires a value"; exit 2; }
            STATUS_CMD="$2"; shift 2 ;;
        --stale-age-min)
            [ $# -ge 2 ] || { err "--stale-age-min requires a value"; exit 2; }
            STALE_AGE_MIN="$2"; shift 2 ;;
        --main-ref)
            [ $# -ge 2 ] || { err "--main-ref requires a value"; exit 2; }
            MAIN_REF="$2"; shift 2 ;;
        --safety)
            [ $# -ge 2 ] || { err "--safety requires a value"; exit 2; }
            SAFETY="$2"; shift 2 ;;
        -*)
            err "Unknown flag: $1"
            err "Run '$(basename "$0") --help' for usage."
            exit 2 ;;
        *)
            err "Unexpected argument: $1"
            err "Run '$(basename "$0") --help' for usage."
            exit 2 ;;
    esac
done

# ── validate ───────────────────────────────────────────────────────────────────
case "$FORMAT" in
    table|json) : ;;
    *)
        err "Unknown --format: '$FORMAT' (expected table or json)."
        err "Run '$(basename "$0") --help' for usage."
        exit 2 ;;
esac

if ! printf '%s\n' "$STALE_AGE_MIN" | grep -qE '^[0-9]+$'; then
    err "--stale-age-min (or REIFY_WARM_LANE_AUDIT_STALE_AGE_MIN) is not a valid non-negative integer: '$STALE_AGE_MIN'."
    err "Run '$(basename "$0") --help' for usage."
    exit 2
fi

if ! printf '%s\n' "$SAFETY" | grep -qE '^[0-9]+(\.[0-9]+)?$' || ! awk -v s="$SAFETY" 'BEGIN{exit !(s>0)}'; then
    err "--safety (or REIFY_WARM_LANE_AUDIT_SAFETY) is not a valid positive number: '$SAFETY'."
    err "Run '$(basename "$0") --help' for usage."
    exit 2
fi

info "warm-lane-audit.sh: mount=$MOUNT format=$FORMAT stale_age_min=$STALE_AGE_MIN main_ref=$MAIN_REF safety=$SAFETY"

exit 0
