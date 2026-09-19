#!/usr/bin/env bash
# scripts/warm-lane-lock-guard.sh — Read-only availability oracle for a
# warm-lane lock. The LOCK-axis sibling of scripts/warm-lane-disk-guard.sh's
# DISK axis: same `check` shape, same exit-3 throttle vocabulary, a different
# measurement — and, deliberately, the opposite fail direction (A3 below).
#
# CLI, options, and the exit-code table: run `--help`. That table is the
# contract dark-factory branches on, and _usage() is its ONE rendering — a
# second copy here would be a second contract the moment either drifted.
#
# WHY, the full mechanism, the invariant rationale, and the dark-factory
# consumer recipe all live in ONE place:
#
#     docs/design/merge-verify-lane-dispatch-seam.md
#
# In brief (task 5608; escalation esc-5363-5): `<worktree_base>/<lane>.lock` is
# ONE inode per lane, per host, and dark-factory has FIVE acquirers of that
# inode family, each on its own independently-tuned wait (seam doc §1) — four
# in-orchestrator sites running on the workstation and sharing its one literal
# inode (three that can target `_merge-verify`, plus a task-lane consumer-hold
# that never does), and a fifth outside the in-process orchestrator on a
# physically separate host, taking that host's own same-named inode. So a
# dispatch that lands while a verify-length lease (1–2h) is held burns the full
# bounded wait before deferring. On an idle lane it costs nothing: the value
# this guard adds is in the contended case, not on every dispatch.
# How dark-factory CLASSIFIES that timeout is DF-owned and changes as DF
# changes; it is stated once, in the seam doc's §1 acquirer table. Do not
# restate it here — this header carried a copy that went stale when DF task
# 3003 reclassified the reset path, which is the argument for the pointer.
#
# Reify's half is unaffected by that classification either way: this script is
# an oracle DF can consult BEFORE dispatching into its own bounded wait, so a
# contended lane becomes a deferred dispatch that never burns the wait, nor the
# requeue and re-dispatch cycle that follows it.
#
# stdout contract: stdout carries the BUSY sentinel line and NOTHING else, on
# every path. All diagnostics — including --help — go to stderr, so a caller
# can parse stdout without filtering.
#
# Invariants (one line each; the normative statement and the reasoning behind
# each are in seam doc §3, which is what to amend if one of these changes):
#   A1 — NON-MUTATING. Read-only open on an EXISTING path. Never creates,
#        truncates, or changes the lock file or the mount: no `>`-open, no
#        `>>`-open, no `touch`, no `mkdir`, and deliberately NOT the
#        `flock <file> <cmd>` convenience form.
#   A2 — SHARED, non-blocking, released at once. Detects an exclusive holder;
#        never contends with another oracle. A point-in-time sample by nature.
#   A3 — FAIL-OPEN. Any probe-infrastructure failure yields exit 0 with a
#        stderr warning and NO sentinel, never exit 3. (Opposite of
#        disk-guard's fail-CLOSED calculus — see A3 in the seam doc for why the
#        asymmetry runs the other way here.)
#   A4 — ADVISORY BACKPRESSURE ONLY. Never requeues, never escalates, and is
#        NOT the correctness mechanism for lane exclusivity; dark-factory's own
#        bounded-wait flock remains that.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# The probe itself — shared with scripts/warm-lane-audit.sh, which applies the
# OPPOSITE fail direction to the same measurement. Guarded existence check so a
# mislocated lib surfaces a directed error, not a cryptic `source: No such file`.
if [ ! -f "$SCRIPT_DIR/lib_lane_lock.sh" ]; then
    echo "warm-lane-lock-guard.sh: required lib not found next to script: $SCRIPT_DIR/lib_lane_lock.sh" >&2
    exit 1
fi
# shellcheck source=scripts/lib_lane_lock.sh
source "$SCRIPT_DIR/lib_lane_lock.sh"

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
                      <mount>/<lane>.lock derivation. Makes --mount
                      unnecessary, and is unaffected by a stale one.
                      (default: \$REIFY_WARM_LANE_LOCK_GUARD_LOCK_PATH)
    -h, --help        Print this message and exit.

  Test seam (env only):
    REIFY_WARM_LANE_LOCK_GUARD_FLOCK   flock command override (default: flock)

  Exit codes:
    0   — IDLE: no exclusive holder observed. Stdout is EMPTY.
    3   — BUSY: an exclusive holder was POSITIVELY observed. Stdout carries
          exactly one line:
            @@REIFY_WARM_LANE_LOCK_BUSY@@ lane=<n> lock=<p>
          A throttle-not-requeue signal (the same cross-repo code
          warm-lane-disk-guard.sh --soft and fleet-load-detector.sh emit):
          dark-factory should DEFER this dispatch rather than enter its
          own bounded wait on the same inode — a wait it would burn in
          full, then pay for again with a requeue and re-dispatch cycle.
          What DF does with that timeout is DF's contract: seam doc §1.
    2   — Usage error: unknown flag, missing flag value, missing/unknown
          subcommand, or no mount when one is required. A wiring bug, not a
          verdict — never read it as BUSY.
EOF
}

# ── defaults ───────────────────────────────────────────────────────────────────
MOUNT="${REIFY_WARM_LANE_MOUNT:-}"
LANE="${REIFY_WARM_LANE_LOCK_GUARD_LANE:-_merge-verify}"
LOCK_PATH="${REIFY_WARM_LANE_LOCK_GUARD_LOCK_PATH:-}"
# The measurement seam. Every flock invocation below routes through this, so the
# hermetic tests can drive a broken or missing flock through the script's OWN
# env knob rather than by shadowing PATH — the house convention (see the df stub
# in tests/infra/test_warm_lane_disk_guard.sh). Stubbing here rather than on
# PATH is what lets Block D exercise the fail-open branches against a REAL held
# lock: the fixture's own holder keeps using the real flock.
FLOCK_BIN="${REIFY_WARM_LANE_LOCK_GUARD_FLOCK:-flock}"

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

# MOUNT is required only when there is something left to DERIVE. An explicit
# --lock-path names the inode outright, so demanding a mount alongside it would
# force a caller that knows the exact lock path to invent one.
if [ -z "$MOUNT" ] && [ -z "$LOCK_PATH" ]; then
    err "Warm-lane mount not specified. Set REIFY_WARM_LANE_MOUNT or pass --mount DIR."
    hint "Alternatively pass --lock-path PATH to name the lock file directly."
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
# The pre-probe line reports only the inputs that were actually consulted: under
# an explicit --lock-path neither MOUNT nor LANE reaches the derivation, and
# echoing an inherited-but-unused mount= would misdescribe what was measured.
if [ -n "$LOCK_PATH" ]; then
    info "warm-lane-lock-guard.sh check: lock=$LOCK  (explicit --lock-path; mount/lane not consulted)"
else
    info "warm-lane-lock-guard.sh check: mount=$MOUNT  lane=$LANE  lock=$LOCK"
fi

# ── probe ──────────────────────────────────────────────────────────────────────
# The MEASUREMENT is `lane_lock_probe` in scripts/lib_lane_lock.sh, shared with
# scripts/warm-lane-audit.sh: it answers IDLE / BUSY / UNMEASURABLE and carries
# no fail direction at all. This script's contribution is the mapping — it fails
# OPEN, sending UNMEASURABLE to IDLE with a warning and no sentinel, where the
# audit sends the same state to LIVE. Both are right for their own consumer, and
# the third state exists precisely so one measurement can serve both calculi.
# Reasoning: docs/design/merge-verify-lane-dispatch-seam.md §3.
#
# FAIL-OPEN (see `--help`): BUSY is reachable from exactly ONE place below — a
# probe that positively observed an exclusive holder. Every other outcome warns
# and leaves the verdict IDLE.
_fail_open() {
    err "Lock probe could not be completed: $*"
    hint "FAIL-OPEN: reporting IDLE (exit 0), sentinel withheld. A false BUSY would defer"
    hint "merge dispatch indefinitely and wedge the serial merge queue; a false IDLE only"
    hint "restores today's behaviour, since dark-factory's own bounded-wait flock remains"
    hint "the real serialization and this guard is advisory backpressure, never a lock."
}

# _probe — sets PROBE_RESULT. Always returns 0: every failure it can encounter
# is a fail-open degradation, so a non-zero return here would abort the script
# under `set -e` instead of degrading.
PROBE_RESULT='IDLE'
_probe() {
    # Mount absent: skip the probe entirely rather than deriving a path under a
    # directory that is not there. Nothing is created — not the mount, not the
    # lock.
    #
    # The `-z "$LOCK_PATH"` guard is load-bearing, not defensive noise. An
    # explicit --lock-path names the inode outright, so MOUNT is not consulted by
    # the derivation at all — and dark-factory exports REIFY_WARM_LANE_MOUNT
    # AMBIENTLY on real verify runs. Without this clause a --lock-path caller
    # inheriting a stale or absent ambient mount would fail-open to IDLE while
    # holding a perfectly readable lock path: a silent false IDLE on a genuinely
    # BUSY lane, which is the same silent-no-op failure Block E exists to
    # prevent, reached through a different door. Pinned by test E6.
    if [ -z "$LOCK_PATH" ] && [ -n "$MOUNT" ] && [ ! -d "$MOUNT" ]; then
        _fail_open "mount directory does not exist: $MOUNT."
        return 0
    fi

    lane_lock_probe "$LOCK" "$FLOCK_BIN"
    case "$LANE_LOCK_PROBE_STATE" in
        BUSY)         PROBE_RESULT='BUSY' ;;
        UNMEASURABLE) _fail_open "$LANE_LOCK_PROBE_DETAIL." ;;
        IDLE)         ;;
    esac
    return 0
}

_probe

if [ "$PROBE_RESULT" = "BUSY" ]; then
    err "Lane '$LANE' is BUSY: an exclusive holder occupies $LOCK."
    hint "Dark-factory should DEFER this dispatch. Dispatching now would enter its own"
    hint "bounded wait on this same inode — burned in full against a holder that outlasts"
    hint "it, then paid for again by the requeue and re-dispatch that follow. Deferring"
    hint "here avoids both; see docs/design/merge-verify-lane-dispatch-seam.md §1."
    # The ONE line this script ever writes to stdout. Emitted last, after the
    # stderr prose, so a caller reading only stdout gets the verdict and nothing
    # else; `lane=` and `lock=` are everything a defer decision needs (holder
    # attribution is deliberately not carried — see the seam doc).
    printf '@@REIFY_WARM_LANE_LOCK_BUSY@@ lane=%s lock=%s\n' "$LANE" "$LOCK"
    exit 3
fi

ok "check: lane '$LANE' is IDLE (no exclusive holder observed)."
exit 0
