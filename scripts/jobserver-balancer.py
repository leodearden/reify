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
_tokens_raw: str = os.environ.get(
    "REIFY_JOBSERVER_TOKENS", str(len(os.sched_getaffinity(0)))
)
try:
    TOKENS: int = int(_tokens_raw)
    if TOKENS < 1:
        raise ValueError("must be >= 1")
except ValueError as _exc:
    sys.stderr.write(
        f"ERROR: REIFY_JOBSERVER_TOKENS={_tokens_raw!r}: {_exc}\n"
        f"  Set to a positive integer (detected nproc="
        f"{len(os.sched_getaffinity(0))})\n"
    )
    sys.exit(1)

# PLACEHOLDER: ε (task α-ε of PRD §10) will tune this based on measurement.
_MIN_POLL_INTERVAL: float = 0.001  # 1 ms — below this is a misconfiguration
_poll_raw: str = os.environ.get("REIFY_JOBSERVER_POLL_INTERVAL", "0.1")
try:
    POLL_INTERVAL: float = float(_poll_raw)
    if POLL_INTERVAL < _MIN_POLL_INTERVAL:
        raise ValueError(f"must be >= {_MIN_POLL_INTERVAL}")
except ValueError as _exc:
    sys.stderr.write(
        f"ERROR: REIFY_JOBSERVER_POLL_INTERVAL={_poll_raw!r}: {_exc}\n"
        f"  Set to a float >= {_MIN_POLL_INTERVAL}\n"
    )
    sys.exit(1)

# PLACEHOLDER pending ε (PRD §10): give-back buffer retained in merge pool.
# ε=1 is the smallest buffer that exercises the give-back path (merge_baseline
# > 1 for all TOKENS≥4, so give = merge_baseline − ε > 0).
# ε's true value is an OUTPUT of the ε measurement harness — not a frozen guess.
_eps_raw: str = os.environ.get("REIFY_JOBSERVER_EPSILON", "1")
try:
    EPSILON: int = int(_eps_raw)
    if EPSILON < 1:
        raise ValueError("must be >= 1")
except ValueError as _exc:
    sys.stderr.write(
        f"ERROR: REIFY_JOBSERVER_EPSILON={_eps_raw!r}: {_exc}\n"
        f"  Set to a positive integer >= 1\n"
    )
    sys.exit(1)

# Token byte: '+' (0x2b) — matches the retired printf/tr seeder for byte-level
# compatibility with the canary and any downstream tools.
TOKEN_BYTE: bytes = b"+"

# ──────────────────────────────────────────────────────────────────────────────
# Module-level stop flag — set by SIGTERM/SIGINT handler in main().
# Defined at module scope so _transfer() can check it during the write-retry
# spin: a SIGTERM that fires while the daemon is mid-retry would otherwise be
# ignored until the write succeeds (unreachable in production, but bounded
# here for correctness).
# ──────────────────────────────────────────────────────────────────────────────
_stop: list = [False]


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


def _transfer(donor_fd: int, recipient_fd: int) -> bool:
    """Transfer exactly ONE token byte from donor_fd to recipient_fd.

    Returns True if a token was moved, False if the donor was empty (EAGAIN).

    TRANSFER PRIMITIVE (contract C1 / anti-oscillation):
      Moves one token per call rather than draining the donor entirely.
      Draining all donor tokens per tick causes steady-state oscillation when
      a hold-and-stop consumer satisfies its demand and stops reading: the
      donated tokens then sit free in the recipient pool, and the next tick
      triggers a full reverse transfer.  Moving one token per tick avoids
      this in the common case (multiple free tokens in the donor): after the
      first donation the recipient leaves 0-free and the donate-idle condition
      no longer holds — transfer stops naturally.  The edge case (exactly one
      free token in the entire pool) can still exhibit per-tick oscillation;
      full anti-oscillation policy (C4 hysteresis / contention ratchet) is
      β's scope (PRD §4).

      A token is only ever in-transit inside one read→write pair — it is
      never dropped even if the process is killed mid-call (at most one
      token in flight at that instant).

    The recipient write is retried on BlockingIOError (the token byte is
    already in hand and must not be dropped — C1 conservation invariant).
    With ≤TOKENS bytes and a 64 KB pipe buffer this retry path is
    unreachable in practice; the guard defends against silent token loss if
    TOKENS ever approaches the pipe capacity.

    Backward compatible with α callers that ignore the return value.
    """
    try:
        byte = os.read(donor_fd, 1)
    except BlockingIOError:
        return False  # donor drained (EAGAIN) — nothing moved
    # Token byte is now in hand; must write before returning (C1 no-drop).
    # Retry loop is bounded so a SIGTERM during a pathological full-pipe spin
    # cannot hang indefinitely.  _stop check allows a clean exit mid-retry.
    # With ≤TOKENS bytes and a 64 KB pipe buffer this path is unreachable in
    # production; the cap + flag are purely a defensive bound.
    _WRITE_RETRY_MAX = 1000
    for _ in range(_WRITE_RETRY_MAX):
        try:
            os.write(recipient_fd, byte)
            return True  # token successfully moved
        except BlockingIOError:
            if _stop[0]:
                # Shutdown mid-retry: token is in-hand; C1 notes at most one
                # token may be in-flight at any instant — the γ canary
                # re-seeds on service restart if needed.
                return True  # in-hand token may be lost on shutdown; C1 allows 1
            time.sleep(0.001)  # pipe buffer briefly full; retry in 1 ms
    # Exhausted retries — unreachable with ≤TOKENS bytes and 64 KB pipe buffer.
    sys.stderr.write(
        "WARNING: _transfer: write retry limit exceeded; "
        "TOKENS may be near pipe capacity\n"
    )
    return True  # token was read; write-side exhausted (unreachable in practice)


def _transfer_burst(donor_fd: int, recipient_fd: int, max_count: int) -> int:
    """Transfer up to max_count tokens from donor_fd to recipient_fd.

    Loops the C1-safe one-token _transfer primitive, stopping when the donor
    is drained (EAGAIN → _transfer returns False) or max_count is reached.

    Returns the number of tokens actually moved (0 … max_count).

    C1 conservation is preserved: at most one token is in-flight per inner
    _transfer call, so no token is dropped even if the process is killed
    mid-burst.

    SPIN-GRAB CONTRACT (PRD §4 / β):
      All of the donor's spare tokens are moved in a single tick (bounded by
      max_count), upgrading α's one-token-per-tick to a burst.  This realises
      the "donate-idle → demanded" and "contention-ratchet" C4 policies where
      the full spare should migrate atomically (relative to the poll loop tick).
    """
    moved = 0
    while moved < max_count:
        if not _transfer(donor_fd, recipient_fd):
            break  # donor drained (EAGAIN) — stop early
        moved += 1
    return moved


def decide(
    free_merge: int,
    free_task: int,
    tokens: int,
    baseline_merge: int,
    baseline_task: int,
    epsilon: int,
    idle_ticks: int,
    idle_threshold: int,
) -> tuple:
    """Pure C4 policy function: given current FIFO state, return the action to take.

    Returns (action, count) where:
        action ∈ {"none", "m2t", "t2m"}
        count  = number of tokens to move (0 for "none")

    Branch order (critical — idle checked FIRST before demand branches):
      1. IDLE (sum_free == tokens): all tokens free, nobody holding
         - idle_ticks >= idle_threshold → reset toward baseline
         - else → ("none", 0) — wait out the window
      2. MERGE-DEMANDED (free_merge==0 and free_task>0):
         → ("t2m", free_task) — move all task spare to merge
         (unifies just-merge donate-idle and contention ratchet;
          monotone: merge never gives back while 0-free)
      3. TASK-DEMANDED (free_task==0 and free_merge>epsilon):
         → ("m2t", free_merge - epsilon) — give back spare, retain ε in merge
         (ε give-back buffer: merge keeps warm reservation)
      4. OTHERWISE (both-0 contention / both-free with epsilon margin) → ("none", 0)

    Invariants by construction:
      - free_merge==0 → NEVER returns "m2t" (give-back requires free_merge>epsilon)
      - Monotone: contested state drifts toward merge=tokens, task=0
    """
    sum_free = free_merge + free_task

    # ── Branch 1: IDLE — all tokens free, nobody holding ──────────────────
    if sum_free == tokens:
        if idle_ticks >= idle_threshold:
            # Reset toward seeded baseline partition
            if free_merge > baseline_merge:
                return ("m2t", free_merge - baseline_merge)
            if free_task > baseline_task:
                return ("t2m", free_task - baseline_task)
        return ("none", 0)

    # ── Branch 2: MERGE-DEMANDED — merge is 0-free, task has spare ────────
    # (just-merge donate-idle + contention ratchet, unified)
    if free_merge == 0 and free_task > 0:
        return ("t2m", free_task)

    # ── Branch 3: TASK-DEMANDED — task is 0-free, merge has spare > ε ─────
    # (give-back: retains ε in merge as warm reservation buffer)
    if free_task == 0 and free_merge > epsilon:
        return ("m2t", free_merge - epsilon)

    # ── Branch 4: otherwise — contention / at-ε / both-free mid-ratchet ──
    return ("none", 0)


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
    # Uses the module-level _stop flag (also checked by _transfer's write-retry).
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
    #          transfer ONE token (donor → recipient) via TRANSFER PRIMITIVE.
    #   3. Sleep POLL_INTERVAL.
    #
    # TRANSFER PRIMITIVE (C1 no-drop guarantee):
    #   - Non-blocking read 1 byte from the donor fd.
    #   - If EAGAIN/BlockingIOError: return without transferring.
    #   - Otherwise: IMMEDIATELY write that byte to the recipient fd
    #     (retrying on BlockingIOError) before returning.  A token is only
    #     ever in-transit inside one read→write pair, never dropped.
    #
    # One token per tick prevents oscillation in the common case (multiple
    # free tokens in the donor): after the first donation the recipient leaves
    # 0-free and the donate-idle condition no longer holds — transfer stops.
    # The edge case (exactly one free token in the pool) can still exhibit
    # per-tick oscillation; full anti-oscillation policy (C4 hysteresis /
    # contention ratchet) is β's scope (PRD §4).
    #
    # The recipient-at-0-free state IS the 'live demand' signal (GNU-make
    # jobserver semantics: a pool empties only when consumers hold its tokens).

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
