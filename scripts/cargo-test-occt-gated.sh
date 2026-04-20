#!/usr/bin/env bash
# cargo-test-occt-gated.sh — cross-worktree OCCT serialization gate
#
# OCCT's C++ kernel shares hidden global state (memory allocators, shape naming
# tables, Standard_Failure exception state) across processes. When multiple
# worktrees each run `cargo test --workspace` concurrently, they can deadlock
# on OCCT's global state — observed 2026-04-19 when three release test runs sat
# at 0% CPU for ~10h. See feedback_occt_thread_safety.md for background.
#
# This wrapper acquires an exclusive flock before executing the given command,
# ensuring at most one OCCT-touching test process runs on the host at a time.
#
# Usage:
#   ./scripts/cargo-test-occt-gated.sh cargo test --workspace -- --test-threads=1
#
# Environment:
#   REIFY_OCCT_LOCK       Override the lock file path.
#                         Default: ${TMPDIR:-/tmp}/reify-occt-$(id -u).lock
#                         The default is user-scoped so each OS account on a
#                         shared host gets its own lock file.  Cross-user
#                         serialization (rare) requires setting
#                         REIFY_OCCT_LOCK to a shared path.  Use a unique
#                         per-test path in test harnesses to avoid
#                         interference with real OCCT runs.
#
#   REIFY_OCCT_LOCK_WAIT  Maximum seconds to wait for the exclusive lock.
#                         Default: 1800 (30 minutes).  If the lock cannot be
#                         acquired within this budget, the wrapper exits 75
#                         (EX_TEMPFAIL) with an error message on stderr — the
#                         command is NOT executed.  A caller that sees exit 75
#                         should interpret it as transient contention ("try
#                         again, nothing ran") rather than a test failure.
#
#   REIFY_OCCT_TEST_TIMEOUT  Maximum seconds the command may run AFTER the lock
#                            is acquired.  Default: 2700 (45 minutes).  The
#                            budget starts at lock-acquisition time, not at
#                            wrapper start — lock-wait time does not consume
#                            the test budget.  On expiry the command is sent
#                            SIGTERM; if still running after 60s it is sent
#                            SIGKILL (--kill-after=60 convention used
#                            project-wide in orchestrator.yaml).  Exit code
#                            124 signals the command was killed by this
#                            timeout (GNU timeout convention), distinct from
#                            exit 75 which means the lock was never acquired.

set -euo pipefail

if ! command -v flock >/dev/null 2>&1; then
    echo "ERROR: cargo-test-occt-gated.sh requires flock (util-linux) but it was not found on PATH." >&2
    echo "       Install util-linux or ensure /usr/bin/flock is accessible." >&2
    exit 1
fi

LOCK="${REIFY_OCCT_LOCK:-${TMPDIR:-/tmp}/reify-occt-$(id -u).lock}"
LOCK_WAIT="${REIFY_OCCT_LOCK_WAIT:-1800}"
TEST_TIMEOUT="${REIFY_OCCT_TEST_TIMEOUT:-2700}"

if [ "$#" -eq 0 ]; then
    echo "ERROR: cargo-test-occt-gated.sh: no command provided" >&2
    exit 64
fi

# FD-mode flock: open the lock file on FD 9 and acquire an exclusive lock with
# a bounded wait.  Using FD-mode (rather than command-mode) lets us interleave
# logic between lock acquisition and exec — specifically, the elapsed-time log
# line and the internal post-lock timeout applied in the next step.  The lock
# stays held for the lifetime of FD 9, which is inherited by exec'd children,
# so the serialization guarantee is preserved.  We use ">>" (append) rather
# than ">" to avoid truncating the lock file on every acquisition.
exec 9>>"$LOCK"
_FLOCK_START="$(date +%s)"
if ! flock -x -w "$LOCK_WAIT" 9; then
    echo "ERROR: cargo-test-occt-gated.sh: failed to acquire OCCT lock within ${LOCK_WAIT}s (LOCK=$LOCK)" >&2
    exit 75
fi
_ELAPSED=$(( $(date +%s) - _FLOCK_START ))
echo "INFO: cargo-test-occt-gated.sh: acquired OCCT lock after ${_ELAPSED}s (LOCK=$LOCK)" >&2

exec timeout --kill-after=60 "$TEST_TIMEOUT" "$@"
