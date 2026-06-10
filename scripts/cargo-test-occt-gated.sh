#!/usr/bin/env bash
# cargo-test-occt-gated.sh — cross-worktree OCCT concurrency gate
#
# OCCT's C++ globals (allocators, shape naming tables, Standard_Failure state)
# are PER-PROCESS. cargo's natural test-binary parallelism (one process per
# test binary) already provides isolation within a single invocation. This
# wrapper bounds inter-worktree concurrency via an N-slot counting semaphore
# so total host load (FDs, memory — especially in release builds) stays within
# headroom even when multiple worktrees run OCCT tests concurrently.
#
# Background: 2026-04-19 — three concurrent release runs wedged at 0% CPU for
# ~10h. Root cause: resource exhaustion (FD leak + memory pressure), not OCCT
# shared state. 2026-04-20 — fd-9-inheritance by sccache pinned the flock;
# fixed by the `9<&-` invariant preserved here.
#
# Standalone/manual OCCT runner (task 4451):
#   scripts/verify.sh no longer invokes this wrapper. Task 4451 folds all OCCT
#   crates into the single nextest pool (via the `occt` test-group, max-threads=4
#   in .config/nextest.toml). This script is retained as a manual runner and for
#   its 23 mechanism tests in tests/infra/test_occt_flock_gate.sh.
#
#   Manual usage example (standalone OCCT run, not part of verify.sh):
#   REIFY_OCCT_TEST_TIMEOUT=2700 \
#     ./scripts/cargo-test-occt-gated.sh cargo test -p reify-kernel-occt \
#       -p reify-eval -p reify-cli -p reify-config -- --test-threads=1
#
# The authoritative list of OCCT-touching crates lives in:
#   scripts/occt-touching-crates.txt
# The infra test that validates this wrapper's mechanism:
#   tests/infra/test_occt_flock_gate.sh (Tests 1-9, 14-23)
#
# SEMAPHORE MECHANISM
# ===================
# N slot files are derived from the REIFY_OCCT_LOCK base path:
#   ${LOCK}.slot-1, ${LOCK}.slot-2, ..., ${LOCK}.slot-N
# Slot files persist across invocations — they hold no state, only an inode for
# flock. They are safe to remove when no wrappers are running:
#   rm -f "${REIFY_OCCT_LOCK:-/tmp/reify-occt-$(id -u).lock}.slot-"*
# The acquire loop shuffles 1..N order (thundering-herd avoidance), opens each
# slot on FD 9, tries `flock -xn 9` (non-blocking exclusive), and breaks on
# the first success. On full contention it closes FD 9, sleeps 0.5s, and
# retries — exiting 75 (EX_TEMPFAIL) when LOCK_WAIT deadline passes.
#
# FD 9 must NOT be inherited by the child process. cargo spawns sccache
# (via RUSTC_WRAPPER) as a detached background daemon that outlives cargo;
# an inherited FD 9 would pin the open file description, keeping the slot
# held forever after this wrapper exits — wedging the OCCT gate host-wide.
# On 2026-04-20 this bug wedged the orchestrator merge queue: the exclusive
# flock was held by a dead PID via a still-live sccache daemon that had
# inherited FD 9.
#
# Invariant: the slot fd is held by THIS shell process only.  The child
# (timeout → cargo → rustc → sccache, etc.) runs with FD 9 closed via
# "9<&-", so no descendant can leak the slot lock beyond this wrapper's
# lifetime.
#
# Environment:
#   REIFY_OCCT_LOCK           Override the lock BASE path.
#                             Default: ${TMPDIR:-/tmp}/reify-occt-$(id -u).lock
#                             Slot files are: ${LOCK}.slot-1 .. ${LOCK}.slot-N.
#                             The default is user-scoped so each OS account on a
#                             shared host gets its own slot files.  Use a unique
#                             per-test base path in test harnesses to avoid
#                             interference with real OCCT runs.
#
#   REIFY_OCCT_CONCURRENCY    Explicit slot count N.  When set,
#                             REIFY_OCCT_MAX_CONCURRENCY is ignored.
#
#   REIFY_OCCT_MAX_CONCURRENCY  Slot count N when REIFY_OCCT_CONCURRENCY is unset.
#                             Default: 32.  Sized to be effectively unlimiting on
#                             typical dev hosts — the orchestrator's
#                             max_concurrent_tasks is the real per-host cap, and
#                             kernel scheduling (nice/ionice, applied by the
#                             orchestrator spawn path) handles cross-workload
#                             priority.  OCCT is required intra-process serial
#                             (--test-threads=1); cross-process OCCT is treated as
#                             safe — any genuine cross-process unsafety surfaced
#                             by concurrency is a test bug to file at root, not a
#                             reason to throttle here.
#
#   REIFY_OCCT_LOCK_WAIT      Maximum seconds to wait for a slot.
#                             Default: 1800 (30 minutes).  If no slot can be
#                             acquired within this budget, the wrapper exits 75
#                             (EX_TEMPFAIL) with an error message on stderr — the
#                             command is NOT executed.  A caller that sees exit 75
#                             should interpret it as transient contention ("try
#                             again, nothing ran") rather than a test failure.
#
#   REIFY_OCCT_TEST_TIMEOUT   Maximum seconds the command may run AFTER a slot is
#                             acquired.  Default: 2700 (45 minutes).  The budget
#                             starts at slot-acquisition time, not at wrapper
#                             start — lock-wait time does not consume the test
#                             budget.  On expiry the command is sent SIGTERM; if
#                             still running after 60s it is sent SIGKILL
#                             (--kill-after=60 convention used project-wide).
#                             Exit code 124 signals SIGTERM; 137 signals SIGKILL.

set -euo pipefail

if ! command -v flock >/dev/null 2>&1; then
    echo "ERROR: cargo-test-occt-gated.sh requires flock (util-linux) but it was not found on PATH." >&2
    echo "       Install util-linux or ensure /usr/bin/flock is accessible." >&2
    exit 1
fi
if ! command -v timeout >/dev/null 2>&1; then
    echo "ERROR: cargo-test-occt-gated.sh requires timeout (GNU coreutils) but it was not found on PATH." >&2
    echo "       Install coreutils or ensure /usr/bin/timeout is accessible." >&2
    exit 1
fi

LOCK="${REIFY_OCCT_LOCK:-${TMPDIR:-/tmp}/reify-occt-$(id -u).lock}"
LOCK_WAIT="${REIFY_OCCT_LOCK_WAIT:-1800}"
TEST_TIMEOUT="${REIFY_OCCT_TEST_TIMEOUT:-2700}"

# Slot count N.  REIFY_OCCT_CONCURRENCY pins it explicitly; otherwise N falls
# back to REIFY_OCCT_MAX_CONCURRENCY (default 32).  No load-based reduction:
# a prior version computed N = clamp(nproc - load_1m_int, 1, MAX_CAP), which
# created positive-feedback collapse to N=1 under high sibling-worktree load
# (the throttle measured run-queue length, which is fed by the work being
# throttled — driving every concurrent wrapper to contend for slot-1 alone
# even when slots 2..MAX were idle).  See esc-4000-39 (2026-05-28).

_MAX_CAP="${REIFY_OCCT_MAX_CONCURRENCY:-32}"
case "$_MAX_CAP" in
    ''|*[!0-9]*) echo "ERROR: cargo-test-occt-gated.sh: REIFY_OCCT_MAX_CONCURRENCY must be a positive integer (got '${_MAX_CAP}')" >&2; exit 64 ;;
esac
[ "$_MAX_CAP" -ge 1 ] || { echo "ERROR: cargo-test-occt-gated.sh: REIFY_OCCT_MAX_CONCURRENCY must be >= 1 (got '${_MAX_CAP}')" >&2; exit 64; }

if [ -n "${REIFY_OCCT_CONCURRENCY:-}" ]; then
    _N="${REIFY_OCCT_CONCURRENCY}"
    # Validate _N is a positive integer — a non-integer or 0 would cause
    # shuf/seq to produce no output, making the wrapper spin until LOCK_WAIT
    # elapses (~30 min default) without ever acquiring a slot.
    case "$_N" in
        ''|*[!0-9]*) echo "ERROR: cargo-test-occt-gated.sh: REIFY_OCCT_CONCURRENCY must be a positive integer (got '${_N}')" >&2; exit 64 ;;
    esac
    [ "$_N" -ge 1 ] || { echo "ERROR: cargo-test-occt-gated.sh: REIFY_OCCT_CONCURRENCY must be >= 1 (got '${_N}')" >&2; exit 64; }
else
    _N="$_MAX_CAP"
    echo "INFO: cargo-test-occt-gated.sh: N=${_N} (REIFY_OCCT_MAX_CONCURRENCY)" >&2
fi

if [ "$#" -eq 0 ]; then
    echo "ERROR: cargo-test-occt-gated.sh: no command provided" >&2
    exit 64
fi

# ---------------------------------------------------------------------------
# N-slot semaphore acquire
# ---------------------------------------------------------------------------
# Slot files: ${LOCK}.slot-1, ${LOCK}.slot-2, ..., ${LOCK}.slot-N.
# We shuffle 1..N each pass to spread pressure across slots (thundering-herd
# avoidance).  `shuf` comes from GNU coreutils; fall back to `seq` (ordered,
# still correct).
#
# For each slot attempt: open the slot file on FD 9 (append), try `flock -xn`
# (non-blocking exclusive).  On failure close FD 9 immediately and continue.
# On success hold FD 9 for the wrapper's lifetime.
#
# The child runs with "9<&-" so no descendant inherits the slot FD.

# Validate the lock parent directory is accessible before entering the acquire
# loop.  Under set -e, a failed `exec 9>>slot-file` (ENOENT, EACCES, ENOSPC)
# would terminate the script with no diagnostic.  Checking once up front gives
# a clear operator-facing error and avoids a confusing silent exit.
_LOCK_PARENT="$(dirname "$LOCK")"
if [ ! -d "$_LOCK_PARENT" ]; then
    echo "ERROR: cargo-test-occt-gated.sh: lock parent directory '${_LOCK_PARENT}' does not exist (REIFY_OCCT_LOCK='${LOCK}')" >&2
    exit 1
fi
if [ ! -w "$_LOCK_PARENT" ]; then
    echo "ERROR: cargo-test-occt-gated.sh: lock parent directory '${_LOCK_PARENT}' is not writable (REIFY_OCCT_LOCK='${LOCK}')" >&2
    exit 1
fi

_FLOCK_START="$(date +%s)"
_DEADLINE=$(( _FLOCK_START + LOCK_WAIT ))
_ACQUIRED_SLOT=""

while true; do
    # IMPORTANT: re-evaluate the shuffled order on EVERY retry pass, not just
    # the first.  Caching the shuffle would cause all waiters to hammer the
    # same slot on each retry, defeating thundering-herd avoidance.  A fresh
    # shuffle each pass spreads load across slots as slots become free.
    if command -v shuf >/dev/null 2>&1; then
        _ORDER="$(shuf -i "1-${_N}")"
    else
        _ORDER="$(seq 1 "${_N}")"
    fi

    for _SLOT in $_ORDER; do
        _SLOT_FILE="${LOCK}.slot-${_SLOT}"
        # Open slot file on FD 9 (append — no truncation of shared lock file).
        # INVARIANT: at most one FD 9 is open at any time.  Successful
        # acquisition leaves FD 9 open (the held slot).  Failed acquisition
        # closes FD 9 immediately (exec 9>&-) so the file description is fully
        # released before the next attempt — no "stale" slot FD leaks across
        # iterations.  The child invocation uses "9<&-" (see below) so no
        # descendant inherits the acquired slot's FD either.
        exec 9>>"$_SLOT_FILE"
        if flock -xn 9; then
            _ACQUIRED_SLOT="$_SLOT"
            break
        fi
        # Acquisition failed — close FD 9 so the file description is freed
        # before we attempt the next slot.
        exec 9>&-
    done

    if [ -n "$_ACQUIRED_SLOT" ]; then
        break
    fi

    # All N slots busy.  Check deadline BEFORE sleeping (not after) so the
    # wrapper exits within at most one retry-pass overhead (~0.5s) of the
    # deadline, regardless of N.  This guarantees a 1s LOCK_WAIT exits in
    # ≤ ~1.5s even when all N slots are externally held (full contention).
    _NOW="$(date +%s)"
    if [ "$_NOW" -ge "$_DEADLINE" ]; then
        echo "ERROR: cargo-test-occt-gated.sh: failed to acquire OCCT slot within ${LOCK_WAIT}s (LOCK=${LOCK}, N=${_N})" >&2
        exit 75
    fi
    sleep 0.5
done

_ELAPSED=$(( $(date +%s) - _FLOCK_START ))
echo "INFO: cargo-test-occt-gated.sh: acquired OCCT lock (slot ${_ACQUIRED_SLOT}/${_N}) after ${_ELAPSED}s (LOCK=${LOCK})" >&2

# Run the child with FD 9 closed (9<&-).  set -e + bash's implicit
# last-command exit-status propagation preserves the command's exit code
# (including 124 for SIGTERM-on-timeout and 137 for SIGKILL escalation).
timeout --kill-after=60 "$TEST_TIMEOUT" "$@" 9<&-
