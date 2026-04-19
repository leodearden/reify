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
#   REIFY_OCCT_LOCK  Override the lock file path (default: /tmp/reify-occt.lock).
#                    Use a unique per-test path in test harnesses to avoid
#                    interference with real OCCT runs.

set -euo pipefail

if ! command -v flock >/dev/null 2>&1; then
    echo "ERROR: cargo-test-occt-gated.sh requires flock (util-linux) but it was not found on PATH." >&2
    echo "       Install util-linux or ensure /usr/bin/flock is accessible." >&2
    exit 1
fi

LOCK="${REIFY_OCCT_LOCK:-/tmp/reify-occt.lock}"

exec flock -x "$LOCK" -- "$@"
