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
# Env knobs:
#   REIFY_WARM_LANE_MOUNT                    warm-lane mount point (shared with
#                                            the other warm-lane scripts)
#   REIFY_WARM_LANE_LOCK_GUARD_LANE          lane to probe (default: _merge-verify)
#   REIFY_WARM_LANE_LOCK_GUARD_LOCK_PATH     explicit lock path override
#   REIFY_WARM_LANE_LOCK_GUARD_FLOCK         flock command override (default: flock).
#                                            The measurement seam: the hermetic
#                                            tests stub it here rather than on
#                                            PATH, so the fail-open branches can
#                                            be exercised against a real held lock.
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
#
# Invariants:
#   A1 — NON-MUTATING. Never creates, truncates, or otherwise changes the lock
#        file or the mount. The lock is opened READ-ONLY on an EXISTING path; a
#        missing lock file is IDLE and is NEVER brought into existence, and a
#        missing mount dir is never created. No `>`-open, no `>>`-open, no
#        `touch`, no `mkdir`, and deliberately NOT the `flock <file> <cmd>`
#        convenience form — all of which would make this reader a writer of the
#        very inode dark-factory serializes on.
#   A2 — the `flock -n -s` probe is SHARED and non-blocking, and is released
#        immediately. A shared request still detects a live consumer (their
#        exclusive flock blocks it), but two concurrent oracles never contend
#        with each other. The verdict is inherently a POINT-IN-TIME sample: the
#        lane can be taken the instant after IDLE is reported, and a writer's own
#        non-blocking attempt could be perturbed for the instant this fd is open.
#        Accepted, exactly as scripts/warm-lane-audit.sh documents for the same
#        probe — and harmless, because of A4.
#   A3 — FAIL-OPEN. Any probe-infrastructure failure (flock missing or broken,
#        lock unreadable, mount absent, unrecognised flock status) yields exit 0
#        with a stderr warning and NO sentinel — never exit 3. A false BUSY would
#        defer merge dispatch indefinitely and wedge the serial merge queue; a
#        false IDLE merely restores today's behaviour. That asymmetry is the
#        opposite of warm-lane-disk-guard.sh's fail-CLOSED calculus, where an
#        unmeasurable disk must be assumed full to avoid ENOSPC.
#   A4 — ADVISORY BACKPRESSURE ONLY. This guard never requeues, never escalates,
#        and is NOT the correctness mechanism for lane exclusivity — dark-factory's
#        own bounded-wait flock remains that. It exists so contention becomes a
#        deferred dispatch instead of a failed one.
#
# Usage by the dark-factory merge worker (cross-repo seam — wiring tracked
# separately; reify ships the oracle, dark-factory does the wiring):
#
#   # `exit_code=0; ... || exit_code=$?` — NOT a bare assignment followed by
#   # `exit_code=$?`. An assignment whose value comes from a command
#   # substitution IS a simple command for errexit purposes, so under `set -e`
#   # the caller's shell would abort AT THE ASSIGNMENT on exit 3 and never reach
#   # the branch this recipe exists to demonstrate. Placing it in an `||` list
#   # exempts it from errexit — the same `rc=0; cmd || rc=$?` idiom this script
#   # uses internally.
#   exit_code=0
#   busy=$(bash scripts/warm-lane-lock-guard.sh check --mount "$worktree_base" \
#                                                     --lane _merge-verify) || exit_code=$?
#   if [ "$exit_code" -eq 0 ]; then
#       # IDLE — dispatch the verify onto this lane.
#   elif [ "$exit_code" -eq 3 ]; then
#       # BUSY — DEFER this dispatch and retry later. "$busy" is the sentinel
#       # line, carrying lane= and lock= for logging. Do NOT dispatch: doing so
#       # enters DF's own 30s bounded wait on the same inode, whose timeout is
#       # classified merge_error (terminal) rather than requeued.
#   else
#       # 2 — a wiring bug on our side. Log it and treat as 0: this guard is
#       # advisory (A4), so a broken consult must never block the queue.
#   fi

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
FLOCK_BIN="${REIFY_WARM_LANE_LOCK_GUARD_FLOCK:-flock}"

# The would-block exit status asked of flock via -E. `flock -n` returns a bare 1
# on contention, which is indistinguishable from "flock itself failed" — and
# reading a tool fault as contention is exactly the false BUSY this guard must
# never emit. A distinct code separates the two. 124 mirrors dark-factory's own
# `flock -x -w 30 -E 124` in _seed_warm_lane, so the two repos speak one dialect
# about this lock.
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
# FAIL-OPEN (see Exit codes): BUSY is reachable from exactly ONE place below — a
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
