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
# Intended usage (two-pass pattern in orchestrator.yaml):
#
#   # Pass 1 — gated: only OCCT-touching crates, serialized via this wrapper.
#   REIFY_OCCT_TEST_TIMEOUT=2700 \
#     ./scripts/cargo-test-occt-gated.sh cargo test -p reify-kernel-occt \
#       -p reify-eval -p reify-cli -- --test-threads=1
#
#   # Pass 2 — ungated: all other workspace crates, runs in parallel across
#   # worktrees (no flock needed because they don't touch OCCT).
#   timeout --kill-after=60 30m cargo test --workspace \
#     --exclude reify-kernel-occt --exclude reify-eval --exclude reify-cli \
#     -- --test-threads=1
#
# The authoritative list of OCCT-touching crates lives in:
#   scripts/occt-touching-crates.txt
# The infra test that validates this wrapper's scope and orchestrator.yaml
# consistency is:
#   tests/infra/test_occt_gated_scope.sh
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
#                         Note: orchestrator.yaml currently treats exit 75
#                         identically to any other non-zero exit (no
#                         caller-side retry logic is implemented yet); this
#                         semantics is documented here for future
#                         differentiation.
#
#   REIFY_OCCT_TEST_TIMEOUT  Maximum seconds the command may run AFTER the lock
#                            is acquired.  Default: 2700 (45 minutes).  The
#                            budget starts at lock-acquisition time, not at
#                            wrapper start — lock-wait time does not consume
#                            the test budget.  On expiry the command is sent
#                            SIGTERM; if still running after 60s it is sent
#                            SIGKILL (--kill-after=60 convention used
#                            project-wide in orchestrator.yaml).  Exit code
#                            124 signals the command was killed via SIGTERM
#                            (GNU timeout convention); 137 (128+9) signals
#                            --kill-after=60 escalated to SIGKILL because the
#                            child ignored SIGTERM.  Either code indicates
#                            this wrapper's timeout fired, distinct from exit
#                            75 which means the lock was never acquired.

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

if [ "$#" -eq 0 ]; then
    echo "ERROR: cargo-test-occt-gated.sh: no command provided" >&2
    exit 64
fi

# FD-mode flock: open the lock file on FD 9 and acquire an exclusive lock with
# a bounded wait.  Using FD-mode (rather than command-mode) lets us interleave
# logic between lock acquisition and the child invocation — specifically, the
# elapsed-time log line and the internal post-lock timeout.  We use ">>"
# (append) rather than ">" to avoid truncating the lock file on every
# acquisition.
#
# FD 9 must NOT be inherited by the child process.  cargo spawns sccache
# (via RUSTC_WRAPPER) as a detached background daemon that outlives cargo;
# an inherited FD 9 would pin the open file description, keeping the flock
# held forever after this wrapper exits — wedging the OCCT gate host-wide.
# On 2026-04-20 this bug wedged the orchestrator merge queue: the exclusive
# flock was held by a dead PID via a still-live sccache daemon that had
# inherited FD 9.
#
# Invariant: the lock fd is held by THIS shell process only.  The child
# (timeout → cargo → rustc → sccache, etc.) runs with FD 9 closed via
# "9<&-", so no descendant can leak the lock beyond this wrapper's lifetime.
# Because this shell remains alive (we do not exec) waiting on the child,
# FD 9 stays open for the full duration of the cargo run — preserving
# cross-worktree serialization — and closes on wrapper exit, releasing the
# flock immediately.
exec 9>>"$LOCK"
# date +%s has 1-second resolution; acquisitions under 1s are reported as 0s.
_FLOCK_START="$(date +%s)"
if ! flock -x -w "$LOCK_WAIT" 9; then
    echo "ERROR: cargo-test-occt-gated.sh: failed to acquire OCCT lock within ${LOCK_WAIT}s (LOCK=$LOCK)" >&2
    exit 75
fi
_ELAPSED=$(( $(date +%s) - _FLOCK_START ))
echo "INFO: cargo-test-occt-gated.sh: acquired OCCT lock after ${_ELAPSED}s (LOCK=$LOCK)" >&2

# Run the child with FD 9 closed (9<&-).  set -e + bash's implicit
# last-command exit-status propagation preserves the command's exit code
# (including 124 for SIGTERM-on-timeout and 137 for SIGKILL escalation).
timeout --kill-after=60 "$TEST_TIMEOUT" "$@" 9<&-
