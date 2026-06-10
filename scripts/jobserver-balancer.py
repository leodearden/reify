#!/usr/bin/env python3
"""
jobserver-balancer.py — dual-FIFO custodian daemon for the Reify cargo jobserver.

Replaces the single 32-token FIFO seeder (reify-jobserver.service) with a
daemon that seeds TWO FIFOs (merge + task) to a merge-favored baseline
partition of nproc, holds both O_RDWR for its lifetime (contract C5), and
runs a single-threaded control loop implementing the TRANSFER PRIMITIVE and
minimal DONATE-IDLE tick (PRD docs/prds/jobserver-merge-priority-balancer.md).

Environment variables (all optional, with sensible defaults):
  REIFY_JOBSERVER_MERGE_FIFO   Path of the merge-pool FIFO
                                (default: /tmp/reify-jobserver-merge)
  REIFY_JOBSERVER_TASK_FIFO    Path of the task-pool FIFO
                                (default: /tmp/reify-jobserver-task)
  REIFY_JOBSERVER_TOKENS       Total token count (default: nproc via
                                len(os.sched_getaffinity(0)))
  REIFY_JOBSERVER_POLL_INTERVAL Control-loop tick period in seconds
                                (default: 0.1; ε will tune this)
"""

import fcntl
import os
import signal
import stat
import struct
import sys
import termios
import time

# ──────────────────────────────────────────────────────────────────────────────
# Configuration — read from environment, fall back to sensible defaults
# ──────────────────────────────────────────────────────────────────────────────

MERGE_FIFO: str = os.environ.get(
    "REIFY_JOBSERVER_MERGE_FIFO", "/tmp/reify-jobserver-merge"
)
TASK_FIFO: str = os.environ.get(
    "REIFY_JOBSERVER_TASK_FIFO", "/tmp/reify-jobserver-task"
)
TOKENS: int = int(
    os.environ.get("REIFY_JOBSERVER_TOKENS", str(len(os.sched_getaffinity(0))))
)
# PLACEHOLDER: ε (task α-ε of PRD §10) will tune this based on measurement.
POLL_INTERVAL: float = float(
    os.environ.get("REIFY_JOBSERVER_POLL_INTERVAL", "0.1")
)

# Token byte: '+' (0x2b) — matches the retired printf/tr seeder for byte-level
# compatibility with the canary and any downstream tools.
TOKEN_BYTE: bytes = b"+"


def fionread(fd: int) -> int:
    """Return the number of readable bytes buffered on fd (non-destructive)."""
    buf = struct.pack("i", 0)
    result = fcntl.ioctl(fd, termios.FIONREAD, buf)
    return struct.unpack("i", result)[0]


def make_fifo(path: str) -> None:
    """Remove any stale file/FIFO at path, then create a fresh FIFO."""
    try:
        os.remove(path)
    except FileNotFoundError:
        pass
    os.mkfifo(path)


def open_rdwr(path: str) -> int:
    """Open a FIFO O_RDWR|O_NONBLOCK.  Returns the fd (kept open for lifetime)."""
    return os.open(path, os.O_RDWR | os.O_NONBLOCK)


def seed_fifo(fd: int, count: int) -> None:
    """Write `count` token bytes to fd (O_RDWR FIFO, so they buffer immediately)."""
    os.write(fd, TOKEN_BYTE * count)


def main() -> None:
    """Daemon entry point: create/seed/hold FIFOs, run control loop."""

    # ── Compute baseline partition (impl-3 refines this) ─────────────────────
    # PLACEHOLDER: ε will tune the exact split (PRD §10).  For α any valid
    # merge-favored partition works: task ~1/4, merge ~3/4, both >= 1.
    task_baseline = max(1, TOKENS // 4)
    merge_baseline = TOKENS - task_baseline  # sum == TOKENS by construction (C1)

    # ── FIFO lifecycle ────────────────────────────────────────────────────────
    make_fifo(MERGE_FIFO)
    make_fifo(TASK_FIFO)

    # Open both O_RDWR to keep the FIFO buffers alive for the process lifetime.
    # Without an O_RDWR holder the buffered tokens evaporate when the last
    # reader/writer closes — this is the C5 custodian contract.
    merge_fd = open_rdwr(MERGE_FIFO)
    task_fd  = open_rdwr(TASK_FIFO)

    # ── Seed the pools ───────────────────────────────────────────────────────
    seed_fifo(merge_fd, merge_baseline)
    seed_fifo(task_fd,  task_baseline)

    # ── Signal handling: clean exit on SIGTERM/SIGINT ─────────────────────────
    _stop = [False]

    def _handler(signum: int, frame: object) -> None:  # noqa: ANN001
        _stop[0] = True

    signal.signal(signal.SIGTERM, _handler)
    signal.signal(signal.SIGINT,  _handler)

    # ── Control loop (impl-4 fills in the transfer/donate-idle logic) ─────────
    while not _stop[0]:
        time.sleep(POLL_INTERVAL)

    # Clean exit — fds are closed by the OS when the process exits.


if __name__ == "__main__":
    main()
