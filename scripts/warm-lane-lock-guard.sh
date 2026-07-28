#!/usr/bin/env bash
# scripts/warm-lane-lock-guard.sh — Read-only availability oracle for a
# warm-lane lock. The LOCK-axis sibling of scripts/warm-lane-disk-guard.sh's
# DISK axis: same `check` shape, same exit-3 throttle vocabulary, a different
# measurement — and, deliberately, the opposite fail direction (see Exit codes).
#
# WHY (task 5608; escalation esc-5363-5). `<worktree_base>/<lane>.lock` is ONE
# inode with THREE dark-factory acquirers on THREE different waits:
#   · merge_verify_lease              — waits 300s, then HOLDS the lock for the
#                                       whole verify (1-2h). Its timeout is
#                                       classified RETRYABLE (requeue).
#   · reset_persistent_merge_worktree — waits only 30s
#                                       (_SEED_WARM_LANE_LOCK_WAIT_SECS, a
#                                       hardcoded DF module constant) and its
#                                       timeout is classified 'merge_error' —
#                                       TERMINAL, escalated, never requeued.
#   · _seed_warm_lane                 — `flock -x -w 30 -E 124`.
# A long-held lease therefore starves a 30s waiter into a spurious terminal
# merge_error. That ASYMMETRY on one inode is the defect; the behavioural fix
# is dark-factory's. This script is reify's half: an oracle DF can consult
# BEFORE dispatching into its own bounded wait, so contention becomes a
# deferred dispatch instead of a failed one.
# Seam contract: docs/design/merge-verify-lane-dispatch-seam.md.
#
# Usage:
#   scripts/warm-lane-lock-guard.sh check [--mount DIR] [--lane NAME] [--lock-path PATH]
#
# Subcommands:
#   check   Probe one warm-lane lock and report IDLE (0) or BUSY (3).
#
# Options (env defaults shown):
#   --mount DIR       Warm-lane mount point — the worktrees dir dark-factory
#                     passes to every warm-lane script
#                     (env: REIFY_WARM_LANE_MOUNT)
#   --lane NAME       Lane whose lock to probe
#                     (env: REIFY_WARM_LANE_LOCK_GUARD_LANE; default: _merge-verify)
#   --lock-path PATH  Probe this lock file directly, bypassing the
#                     <mount>/<lane>.lock derivation
#                     (env: REIFY_WARM_LANE_LOCK_GUARD_LOCK_PATH)
#   -h, --help        Print this message and exit.
#
# Exit codes:      [KEEP IN STEP with the same section inside _usage() below —
#                   one wording, two renderings, differing only in the leading
#                   comment prefix. This is the contract dark-factory branches
#                   on; two drifting copies of it would be two contracts.]
#   0   — IDLE: no exclusive holder observed. Stdout is EMPTY.
#   3   — BUSY: an exclusive holder was POSITIVELY observed. Stdout carries
#         exactly one line:
#           @@REIFY_WARM_LANE_LOCK_BUSY@@ lane=<n> lock=<p>
#         A throttle-not-requeue signal (the same cross-repo code
#         warm-lane-disk-guard.sh --soft and fleet-load-detector.sh emit):
#         dark-factory should DEFER this dispatch rather than enter its own
#         30s bounded wait, whose timeout is classified merge_error.
#   2   — Usage error: unknown flag, missing flag value, missing/unknown
#         subcommand, or no mount when one is required. A wiring bug, not a
#         verdict — never read it as BUSY.
#
# stdout contract: stdout carries the BUSY sentinel line and NOTHING else, on
# every path. All diagnostics — including --help — go to stderr, so a caller
# can parse stdout without filtering.

set -euo pipefail

# ── log helpers (all write to stderr) ─────────────────────────────────────────
info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*" >&2; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*" >&2; }
err()   { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }
hint()  { err "Hint:  $*"; }

# ── usage ──────────────────────────────────────────────────────────────────────
_usage() {
    cat >&2 <<EOF
Usage: $(basename "$0") check [--mount DIR] [--lane NAME] [--lock-path PATH]

  Read-only availability oracle for a warm-lane lock. Takes a non-blocking
  SHARED flock on an EXISTING <mount>/<lane>.lock and reports whether an
  exclusive holder occupies the lane. Never creates, truncates, or otherwise
  mutates anything.

  The LOCK-axis sibling of scripts/warm-lane-disk-guard.sh's DISK axis:
  pre-dispatch backpressure for dark-factory, never a correctness mechanism —
  DF's own bounded-wait flock remains the real serialization.

  Subcommands:
    check   Probe one warm-lane lock and report IDLE (0) or BUSY (3).

  Options:
    --mount DIR       Warm-lane mount point, i.e. the worktrees dir
                      (default: \$REIFY_WARM_LANE_MOUNT)
    --lane NAME       Lane whose lock to probe
                      (default: \$REIFY_WARM_LANE_LOCK_GUARD_LANE or _merge-verify)
    --lock-path PATH  Probe this lock file directly, bypassing the
                      <mount>/<lane>.lock derivation
                      (default: \$REIFY_WARM_LANE_LOCK_GUARD_LOCK_PATH)
    -h, --help        Print this message and exit.

  Exit codes:
    0   — IDLE: no exclusive holder observed. Stdout is EMPTY.
    3   — BUSY: an exclusive holder was POSITIVELY observed. Stdout carries
          exactly one line:
            @@REIFY_WARM_LANE_LOCK_BUSY@@ lane=<n> lock=<p>
          A throttle-not-requeue signal (the same cross-repo code
          warm-lane-disk-guard.sh --soft and fleet-load-detector.sh emit):
          dark-factory should DEFER this dispatch rather than enter its own
          30s bounded wait, whose timeout is classified merge_error.
    2   — Usage error: unknown flag, missing flag value, missing/unknown
          subcommand, or no mount when one is required. A wiring bug, not a
          verdict — never read it as BUSY.
EOF
}

# ── defaults ───────────────────────────────────────────────────────────────────
MOUNT="${REIFY_WARM_LANE_MOUNT:-}"
LANE="${REIFY_WARM_LANE_LOCK_GUARD_LANE:-_merge-verify}"
LOCK_PATH="${REIFY_WARM_LANE_LOCK_GUARD_LOCK_PATH:-}"

# ── arg parsing ────────────────────────────────────────────────────────────────
SUBCOMMAND=""

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            _usage; exit 0 ;;
        --mount)
            [ $# -ge 2 ] || { err "--mount requires a value"; exit 2; }
            MOUNT="$2"; shift 2 ;;
        --lane)
            [ $# -ge 2 ] || { err "--lane requires a value"; exit 2; }
            LANE="$2"; shift 2 ;;
        --lock-path)
            [ $# -ge 2 ] || { err "--lock-path requires a value"; exit 2; }
            LOCK_PATH="$2"; shift 2 ;;
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

# ── post-parse validation ──────────────────────────────────────────────────────
# Kept separate from the loop above so a wiring bug is reported once, in one
# place, regardless of flag order.
if [ -z "$SUBCOMMAND" ]; then
    err "Missing subcommand. Expected: check"
    err "Run '$(basename "$0") --help' for usage."
    exit 2
fi

if [ -z "$MOUNT" ]; then
    err "Warm-lane mount not specified. Set REIFY_WARM_LANE_MOUNT or pass --mount DIR."
    err "Run '$(basename "$0") --help' for usage."
    exit 2
fi

if [ -z "$LANE" ]; then
    err "Lane name is empty. Set REIFY_WARM_LANE_LOCK_GUARD_LANE or pass --lane NAME."
    err "Run '$(basename "$0") --help' for usage."
    exit 2
fi

# ── lock-path derivation ───────────────────────────────────────────────────────
# The lock is a SIBLING of the lane dir — `<mount>/<lane>.lock`, NOT a file
# inside `<mount>/<lane>/`. This byte-matches dark-factory's own
# verify_cancel.py lane_lock_path(), which is
# `lane_dir.with_name(lane_dir.name + '.lock')`; probing anything else would
# report IDLE forever and silently defeat the guard.
#
# `--mount` is the WORKTREES DIR. That is the value dark-factory passes to every
# warm-lane script (str(self.worktree_base)) — the same convention
# scripts/warm-lane-gc.sh:120-136 documents for its own WORKTREES_DIR
# assignment. On the real host: --mount=/home/leo/src/warm-lanes/worktrees, so
# the _merge-verify lock is /home/leo/src/warm-lanes/worktrees/_merge-verify.lock.
LOCK="${LOCK_PATH:-$MOUNT/$LANE.lock}"

# ── check subcommand ───────────────────────────────────────────────────────────
info "warm-lane-lock-guard.sh check: mount=$MOUNT  lane=$LANE  lock=$LOCK"

# ── probe ──────────────────────────────────────────────────────────────────────
# Opens an EXISTING lock file READ-ONLY and attempts a non-blocking SHARED
# flock on that read-only fd: acquired => no exclusive holder => IDLE (released
# immediately); blocked => a live consumer holds it exclusively => BUSY.
#
# Technique copied from scripts/warm-lane-audit.sh's _probe_live, and the three
# avoidances are load-bearing:
#   · a MISSING lock file is IDLE and is NEVER created — no `>`-open, no
#     `>>`-open, no `touch`. Materializing the inode DF serializes on would make
#     this reader a writer.
#   · SHARED (-s), not exclusive: every real consumer holds an EXCLUSIVE flock
#     while live, so a shared request still detects them — but two concurrent
#     oracles never contend with each other.
#   · not the `flock <file> <cmd>` convenience form, which opens for writing and
#     creates the file.
PROBE_RESULT='IDLE'
if [ -e "$LOCK" ]; then
    if exec 7<"$LOCK" 2>/dev/null; then
        if flock -n -s 7 2>/dev/null; then
            flock -u 7 2>/dev/null || true
        else
            PROBE_RESULT='BUSY'
        fi
        exec 7<&- 2>/dev/null || true
    fi
fi

if [ "$PROBE_RESULT" = "BUSY" ]; then
    err "Lane '$LANE' is BUSY: an exclusive holder occupies $LOCK."
    hint "Dark-factory should DEFER this dispatch. Dispatching now would enter its own"
    hint "30s bounded wait on this same inode, whose timeout is classified merge_error"
    hint "(terminal) rather than requeued — the failure mode task 5608 exists to avoid."
    # The ONE line this script ever writes to stdout. Emitted last, after the
    # stderr prose, so a caller reading only stdout gets the verdict and nothing
    # else; `lane=` and `lock=` are everything a defer decision needs (holder
    # attribution is deliberately not carried — see the seam doc).
    printf '@@REIFY_WARM_LANE_LOCK_BUSY@@ lane=%s lock=%s\n' "$LANE" "$LOCK"
    exit 3
fi

ok "check: lane '$LANE' is IDLE (no exclusive holder observed)."
exit 0
