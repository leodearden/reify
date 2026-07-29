#!/usr/bin/env bash
# tests/infra/test_flock_detached_fork_guard.sh
#
# Repo-wide regression guard (task #5817):
#   Flags a tracked shell script that BOTH (a) acquires a flock on a numbered
#   FD **it opens itself**, AND (b) forks a DETACHED background child (`... &`)
#   downstream of that acquire, WITHOUT (c) an explicit `flock -u <fd>` release.
#
#   Zero offenders exist today.  The guard exists so the class cannot bite a
#   THIRD time (2026-04-20 sccache/FD-9 wedge; 2026-07-28 seed-warm-lane.sh,
#   fixed by #5705).
#
# The guard is a LOAD-INDEPENDENT static scan: the verdict is a function of
# script TEXT only — no filesystem stat, no network, no model call — so it is
# host-independent and deterministic (the scripts/lock-charter-guard.sh C-P3
# discipline).  That is what lets it sit in the hermetic `pool` bucket.
#
# ── The four line classes ────────────────────────────────────────────────────
#   OPEN(N)    — a depth-0, non-comment, non-continuation line that OPENS FD N
#                (`exec <ws> N` followed by `>`, `>>` or `<` whose next char is
#                not `&`).  A close (`exec N>&-`) and a dup (`exec N>&M`) are
#                deliberately NOT opens.
#   ACQUIRE(N) — a depth-0, non-comment, non-continuation line invoking `flock`
#                in command position with N as a bare, tail-anchored FD operand,
#                occurring after OPEN(N).
#   UNLOCK(N)  — a non-comment line running `flock` in command position with
#                `-u` and a bare, tail-anchored operand N.  FILE-LEVEL and not
#                order-sensitive: the sanctioned remedy is a `flock -u N` inside
#                a function registered as an EXIT trap, whose textual position
#                relative to the fork carries no meaning.
#   DETACH     — a non-comment line, at subshell depth 0 after its own paren
#                delta, whose FINAL EFFECTIVE TOKEN is a bare `&`.
#
#   FLAG iff there is an N with OPEN(N) < ACQUIRE(N) < DETACH and NO UNLOCK(N).
#
# ── The two false-positive exemptions are STRUCTURAL, not allow-listed ───────
#   FOREGROUND children are SAFE.  Only a line whose final effective token is a
#   bare `&` is a DETACH, so `"$@" 9<&-` (scripts/lib_test_semaphore.sh) can
#   never enter the candidate set.
#   INHERITED-FD paths are SAFE.  A candidate requires a LOCAL open, so a file
#   that merely inherits FD 9 from its caller is never considered — the guard is
#   structurally incapable of advising a `flock -u 9` that would release the
#   CALLER's lock.  (scripts/seed-warm-lane.sh documents that hazard at its
#   contract block and installs its trap INSIDE the acquire branch for the same
#   "make it impossible rather than conditionally avoided" reason.)
#
# ── SELF-MATCH SAFETY ────────────────────────────────────────────────────────
#   This guard scans every tracked shell file, including itself.  Fixture bodies
#   carrying the offending shape are therefore ASSEMBLED FROM SHELL VARIABLES at
#   runtime and written only into a `mktemp -d` dir — never emitted as literal
#   source lines (the test_no_new_wallclock_upper_bounds.sh /
#   test_reify_audit_ptodo.sh convention).  A dedicated assertion pins that this
#   file scans clean.
#
# ── HONEST LIMITATION (deliberate, not an oversight) ─────────────────────────
#   A file-local syntactic scan cannot follow FD provenance across `source`.
#   tests/infra/run_all.sh holds the Lane-X FD 9 opened inside the sourced
#   scripts/lib_lane_x_flock.sh and forks pool workers with `) &`; it contains no
#   local open, so it is out of criterion (a)'s stated shape.  It is
#   independently safe — each worker runs `bash ... 9<&-` and closes with
#   `exec 9>&-`.  Chasing provenance across `source` would need interprocedural
#   analysis and would forfeit the host-independent, no-stat property above.
#
# WHY an explicit unlock and not a close, plus the measured held-after-exit
# rates: the LANE-LOCK RELEASE CONTRACT block at the flock acquire in
# scripts/seed-warm-lane.sh is the single source of truth (G7 — not restated
# here).
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== flock-held-across-detached-fork regression guard ==="

TMPWORK="$(mktemp -d)"
trap 'rm -rf "$TMPWORK"' EXIT

# ---------------------------------------------------------------------------
# _write_fixture <basename> <line>...
#
# Writes a fixture script to "$TMPWORK/<basename>", one argument per line.
#
# SELF-MATCH SAFETY: callers MUST assemble any offending token from shell
# variables (adjacent-single-quote '' splitting) rather than writing it as a
# literal line here — see the header.  This helper exists so every fixture
# lands under the mktemp dir and never in the tracked corpus.
# ---------------------------------------------------------------------------
_write_fixture() {
    local name="$1"
    shift
    local path="$TMPWORK/$name"
    printf '%s\n' "$@" >"$path"
}

# ---------------------------------------------------------------------------
# Fixture tokens — SELF-MATCH SAFETY (see header).
#
# Every token that could make a line flaggable is assembled here from adjacent
# single-quoted fragments ('' splitting), so this source file contains no
# literal open / acquire / detach construct.  Fixture bodies built from these
# are written only into $TMPWORK by _write_fixture, never emitted as tracked
# source lines.
# ---------------------------------------------------------------------------
_FX_AMP='&'
_FX_OPEN='ex''ec 9>"$LOCK"'
_FX_ACQ_IF='if ! flo''ck -n 9; then'
_FX_DETACH='{ rm -rf "$TRASH"; } 9<&- '"$_FX_AMP"

# ---------------------------------------------------------------------------
# Harness — run the detector once, capture rc + stdout into globals, then
# assert over those globals (assert runs "$@" in THIS shell, so the globals
# survive).
# ---------------------------------------------------------------------------
OFFENDERS_RC=0
OFFENDERS_OUT=""

_run_offenders() {
    OFFENDERS_OUT=""
    OFFENDERS_RC=0
    OFFENDERS_OUT="$(_flock_fork_offenders "$@")" || OFFENDERS_RC=$?
}

_rc_is() { [ "$OFFENDERS_RC" = "$1" ]; }

_out_has() { case "$OFFENDERS_OUT" in *"$1"*) return 0 ;; *) return 1 ;; esac; }

# ---------------------------------------------------------------------------
# Cycle 1 — POSITIVE CONTROL.
#
# The real tree has zero offenders, so nothing in it keeps the matcher honest.
# This synthetic offender is the "a detector, not a constant failure" half:
# a locally-opened FD 9, a flock acquire on it, a detached fork downstream, and
# no unlock anywhere.  Its report must be ACTIONABLE (path + fd + line numbers),
# not a bare boolean.
# ---------------------------------------------------------------------------
echo "--- Cycle 1: positive control — a synthetic offender must be FLAGGED ---"

_write_fixture pos_offender.sh \
    '#!/usr/bin/env bash' \
    'set -euo pipefail' \
    'LOCK=/tmp/fixture.lock' \
    'TRASH=/tmp/fixture.trash' \
    "$_FX_OPEN" \
    "$_FX_ACQ_IF" \
    '    exit 75' \
    'fi' \
    'echo "holding the lane lock"' \
    "$_FX_DETACH" \
    'echo "done"'

_run_offenders "$TMPWORK/pos_offender.sh"

assert "positive control: synthetic offender is flagged (rc 1)" _rc_is 1
assert "positive control: report names the fixture path" \
    _out_has "$TMPWORK/pos_offender.sh"
assert "positive control: report names fd=9" _out_has "fd=9"
assert "positive control: report names the OPEN line (open=5)" _out_has "open=5"
assert "positive control: report names the ACQUIRE line (acq=6)" _out_has "acq=6"
assert "positive control: report names the DETACH line (detach=10)" \
    _out_has "detach=10"

test_summary
