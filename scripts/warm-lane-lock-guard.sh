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
# ONE inode with THREE dark-factory acquirers on THREE different waits, so a
# lease held for the length of a verify (1–2h) starves a 30s waiter every time.
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

# The would-block exit status asked of flock via -E. `flock -n` returns a bare 1
# on contention, which is indistinguishable from "flock itself failed" — and
# reading a tool fault as contention is exactly the false BUSY this guard must
# never emit. A distinct code separates the two. 124 mirrors dark-factory's own
# conflict-status value: git_ops.py's _seed_warm_lane assembles its flock
# invocation from _SEED_WARM_LANE_LOCK_WAIT_SECS / _SEED_WARM_LANE_LOCK_TIMEOUT_RC
# (currently 30 / 124) rather than a literal string, so the two repos speak one
# dialect about this lock via those constants' current values.
FLOCK_CONFLICT_RC=124

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
# Opens an EXISTING lock file READ-ONLY and attempts a non-blocking SHARED
# flock on that read-only fd: acquired => no exclusive holder => IDLE (released
# immediately); would-block => a live consumer holds it exclusively => BUSY.
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
#
# DELIBERATE DIVERGENCE FROM _probe_live, on the one question the two disagree
# about. Audit uses a bare `flock -n -s` and reads EVERY non-zero as LIVE — it
# conflates a broken or missing flock with contention, i.e. it fails CLOSED.
# This guard asks for `-E 124` and fails OPEN on anything that is not that exact
# status. Neither is a bug: audit's output is advisory prose a human reads, and
# over-reporting LIVE there merely looks conservative, whereas this script's
# exit 3 gates dispatch, and a false BUSY would wedge the serial merge queue.
# The consequence to know is that on a host with a degraded flock the two WILL
# disagree about one inode — audit says LIVE, this guard says IDLE. Two probes
# of one inode with no shared code path is a real seam; unifying them behind a
# tri-state helper (IDLE / BUSY / UNMEASURABLE, each caller applying its own
# fail direction) is filed as follow-up work, since warm-lane-audit.sh is
# outside task 5608's scope. Seam doc §3 carries the same note.
#
# FAIL-OPEN (see `--help`): BUSY is reachable from exactly ONE place below — a
# completed flock that reported would-block. Every other outcome warns and
# leaves the verdict IDLE.
_fail_open() {
    err "Lock probe could not be completed: $*"
    hint "FAIL-OPEN: reporting IDLE (exit 0), sentinel withheld. A false BUSY would defer"
    hint "merge dispatch indefinitely and wedge the serial merge queue; a false IDLE only"
    hint "restores today's behaviour, since dark-factory's own bounded-wait flock remains"
    hint "the real serialization and this guard is advisory backpressure, never a lock."
}

# _probe — sets PROBE_RESULT. Always returns 0: every failure it can encounter
# is a fail-open degradation, so a non-zero return here would abort the script
# under `set -e` instead of degrading. Probe statuses are captured with the
# `rc=0; cmd || rc=$?` idiom rather than by disabling errexit.
PROBE_RESULT='IDLE'
_probe() {
    local rc=0

    # Tool missing or not executable — a wiring/environment fault, not evidence
    # about the lane.
    if ! command -v "$FLOCK_BIN" >/dev/null 2>&1; then
        _fail_open "flock is missing or not executable (REIFY_WARM_LANE_LOCK_GUARD_FLOCK='$FLOCK_BIN')."
        return 0
    fi

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

    # An ABSENT lock file is not a degradation: it positively means no consumer
    # has ever taken this lane's lock. IDLE, silently, and never created.
    [ -e "$LOCK" ] || return 0

    # The read-only open is a SCOPED block redirect, deliberately NOT
    # `exec 7<"$LOCK" 2>/dev/null`. A redirection attached to a command-less
    # `exec` is PERMANENT for the shell, so that form would silently discard
    # every diagnostic emitted after this point — the BUSY message and every
    # fail-open warning alike, leaving an operator with no way to learn the
    # measurement had stopped working. warm-lane-audit.sh's _probe_live can use
    # the `exec` form safely only because it is always called inside a `$( )`
    # subshell, which contains the permanence; this script probes in the main
    # shell. The block form also closes fd 7 automatically on every path.
    #
    # `2>/dev/null` is ordered BEFORE `7<"$LOCK"` so it is already in effect if
    # the open itself fails: redirections apply left to right, and bash reports
    # a failed one on whatever stderr is current at that moment.
    #
    # `probed` distinguishes "the body ran" from "the redirect failed" — a
    # failed redirect skips the body entirely, leaving rc at 0, which would
    # otherwise be indistinguishable from a successfully acquired lock.
    local probed=0
    {
        probed=1
        "$FLOCK_BIN" -n -s -E "$FLOCK_CONFLICT_RC" 7 || rc=$?
        if [ "$rc" -eq 0 ]; then
            # Acquired, so nobody holds it exclusively. Release at once rather
            # than relying on the fd close alone.
            "$FLOCK_BIN" -u 7 || true
        fi
    } 2>/dev/null 7<"$LOCK" || true

    if [ "$probed" -ne 1 ]; then
        _fail_open "cannot open the lock file for reading: $LOCK."
        return 0
    fi

    case "$rc" in
        0)
            ;;
        "$FLOCK_CONFLICT_RC")
            # The ONLY path to BUSY: flock ran to completion and reported that
            # the shared request would block, which only an exclusive holder can
            # cause.
            PROBE_RESULT='BUSY'
            ;;
        *)
            _fail_open "flock exited $rc, which is neither acquired (0) nor would-block ($FLOCK_CONFLICT_RC)."
            ;;
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
