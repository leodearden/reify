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


def _transfer(donor_fd: int, recipient_fd: int) -> None:
    """Spin-grab all free tokens from donor_fd and write them to recipient_fd.

    TRANSFER PRIMITIVE (contract C1):
      Each non-blocking read of one byte is immediately paired with a write of
      that byte to recipient_fd before the next read.  A token is only ever
      in-transit inside one read→write pair — it is never lost even if the
      process is killed mid-loop (at most one token in flight at that instant).

    Stops when donor_fd raises BlockingIOError (no more free tokens / EAGAIN).
    With ≤32 tokens and a 64 KB pipe buffer the recipient write never blocks.
    """
    while True:
        try:
            byte = os.read(donor_fd, 1)
        except BlockingIOError:
            break
        os.write(recipient_fd, byte)


def main() -> None:
    """Daemon entry point: create/seed/hold FIFOs, run control loop."""

    # ── Compute baseline partition ────────────────────────────────────────────
    # PLACEHOLDER pending ε's measurement harness (PRD §4 C4 / §10):
    #   task_baseline  = max(1, TOKENS // 4)   (~1/4 of pool, minimum 1)
    #   merge_baseline = TOKENS - task_baseline (~3/4 of pool)
    #
    # Invariants guaranteed by construction:
    #   merge_baseline > task_baseline  (merge-favored, PRD §4 C4)
    #   task_baseline  >= 1              (non-starving; prevents idle thrash)
    #   merge + task   == TOKENS         (C1 token conservation)
    #
    # Tests assert the PARTITION PROPERTY, not a guessed numeric value, so
    # ε's retune will not break them.  For TOKENS=32: 24/8.  For TOKENS=4: 3/1.
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

    # ── Control loop: SENSE → DONATE-IDLE ────────────────────────────────────
    #
    # Each tick:
    #   1. SENSE both pools via FIONREAD (non-destructive).
    #   2. Apply the minimal symmetric DONATE-IDLE rule:
    #      if donor.free > 0 and recipient.free == 0:
    #          spin-grab donor's free tokens to recipient via TRANSFER PRIMITIVE.
    #   3. Sleep POLL_INTERVAL.
    #
    # TRANSFER PRIMITIVE (C1 no-drop guarantee):
    #   - Non-blocking read 1 byte from the donor fd.
    #   - If EAGAIN/BlockingIOError: stop (no more free tokens).
    #   - Otherwise: IMMEDIATELY write that byte to the recipient fd BEFORE
    #     reading the next.  A token is only ever in-transit inside one
    #     read→write pair, never dropped.
    #
    # With ≤32 tokens and a 64 KB pipe buffer the write never blocks.
    # The recipient-at-0-free state IS the 'live demand' signal (GNU-make
    # jobserver semantics: a pool empties only when consumers hold its tokens).
    # both_baseline ≥ 1 + a real consumer keeping the demanded pool at 0 ensure
    # the reverse condition never holds simultaneously → non-thrashing.

    while not _stop[0]:
        # SENSE
        free_merge = fionread(merge_fd)
        free_task  = fionread(task_fd)

        # DONATE-IDLE: transfer donor's free tokens to the demanding recipient
        if free_merge > 0 and free_task == 0:
            _transfer(merge_fd, task_fd)
        elif free_task > 0 and free_merge == 0:
            _transfer(task_fd, merge_fd)

        time.sleep(POLL_INTERVAL)

    # Clean exit — fds are closed by the OS when the process exits.


if __name__ == "__main__":
    main()
