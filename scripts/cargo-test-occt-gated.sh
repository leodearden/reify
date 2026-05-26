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
# Intended usage (two-pass pattern, from scripts/verify.sh):
#
#   # Pass 1 — gated: only OCCT-touching crates, bounded via this wrapper.
#   REIFY_OCCT_TEST_TIMEOUT=2700 \
#     ./scripts/cargo-test-occt-gated.sh cargo test -p reify-kernel-occt \
#       -p reify-eval -p reify-cli -p reify-config -- --test-threads=1
#
#   # Pass 2 — ungated: all other workspace crates, runs in parallel across
#   # worktrees (no semaphore needed because they don't touch OCCT).
#   timeout --kill-after=60 30m cargo test --workspace \
#     --exclude reify-kernel-occt --exclude reify-eval --exclude reify-cli \
#     --exclude reify-config -- --test-threads=1
#
# The authoritative list of OCCT-touching crates lives in:
#   scripts/occt-touching-crates.txt
# The infra test that validates this wrapper's scope and verify.sh consistency:
#   tests/infra/test_occt_gated_scope.sh
#
# SEMAPHORE MECHANISM
# ===================
# N slot files are derived from the REIFY_OCCT_LOCK base path:
#   ${LOCK}.slot-1, ${LOCK}.slot-2, ..., ${LOCK}.slot-N
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
#   REIFY_OCCT_CONCURRENCY    Explicit slot count N (overrides auto-detect).
#                             When set, REIFY_OCCT_MAX_CONCURRENCY is ignored.
#
#   REIFY_OCCT_MAX_CONCURRENCY  Hard cap on auto-detected N.
#                             Default: 4.  Conservative for memory-heavy OCCT
#                             release builds; raise via env in CI/benchmark.
#                             Has no effect when REIFY_OCCT_CONCURRENCY is set.
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

# Slot count N.  REIFY_OCCT_CONCURRENCY pins it explicitly (override); if
# unset, auto-detect: N = clamp(nproc - load_1m_int, 1, MAX_CAP).
#
# Auto-detect rationale:
#   nproc    — available logical CPUs on this host.
#   load_int — 1-minute load average (integer truncation via awk), read from
#              /proc/loadavg.  Fallback 0 on non-Linux or unreadable file.
#   MAX_CAP  — hard ceiling, default 4.  Conservative for memory-heavy OCCT
#              release builds; raise via REIFY_OCCT_MAX_CONCURRENCY in
#              benchmark/CI contexts.
#   N        — max(1, min(MAX_CAP, nproc - load_int)).
#
# The intent: on an idle box with 32 CPUs and load 0, N = min(4, 32) = 4.
# On a stressed box (load 30 on 32 CPUs), N = max(1, 32-30) = 2 — still
# allows some parallelism rather than full serialization.

_MAX_CAP="${REIFY_OCCT_MAX_CONCURRENCY:-4}"

if [ -n "${REIFY_OCCT_CONCURRENCY:-}" ]; then
    _N="${REIFY_OCCT_CONCURRENCY}"
else
    _NPROC=2
    if command -v nproc >/dev/null 2>&1; then
        _NPROC="$(nproc)"
    fi
    # _REIFY_OCCT_NPROC_OVERRIDE: test-only env var (underscore prefix = private).
    # Overrides the nproc value in the auto-detect formula so tests can simulate
    # any CPU count without depending on actual machine capacity.
    if [ -n "${_REIFY_OCCT_NPROC_OVERRIDE:-}" ]; then
        _NPROC="${_REIFY_OCCT_NPROC_OVERRIDE}"
    fi
    _LOAD_INT=0
    if [ -r /proc/loadavg ]; then
        _LOAD_INT="$(awk '{printf "%d", $1}' /proc/loadavg)"
    fi
    # _REIFY_OCCT_LOAD_OVERRIDE: test-only env var (underscore prefix = private).
    # Overrides the 1-minute load average used in auto-detect so tests are
    # load-independent regardless of actual host utilization.
    if [ -n "${_REIFY_OCCT_LOAD_OVERRIDE:-}" ]; then
        _LOAD_INT="${_REIFY_OCCT_LOAD_OVERRIDE}"
    fi
    _N=$(( _NPROC - _LOAD_INT ))
    if [ "$_N" -lt 1 ]; then _N=1; fi
    if [ "$_N" -gt "$_MAX_CAP" ]; then _N="$_MAX_CAP"; fi
    echo "INFO: cargo-test-occt-gated.sh: auto-detect N=${_N} (nproc=${_NPROC}, load=${_LOAD_INT}, cap=${_MAX_CAP})" >&2
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
