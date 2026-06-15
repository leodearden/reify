#!/usr/bin/env python3
"""
jobserver-balancer.py — dual-FIFO custodian daemon for the Reify cargo jobserver.

Replaces the single 32-token FIFO seeder (reify-jobserver.service) with a
daemon that seeds TWO FIFOs (merge + task) to a merge-favored baseline
partition of nproc, holds both O_RDWR for its lifetime (contract C5), and
runs a single-threaded control loop implementing the full C4 policy (β):
  donate-idle + contention ratchet (absolute merge priority) + ε give-back
  + idle baseline-reset
(PRD docs/prds/jobserver-merge-priority-balancer.md §4 C4).

Environment variables (all optional, with sensible defaults):
  REIFY_JOBSERVER_MERGE_FIFO   Path of the merge-pool FIFO
                                (default: /tmp/reify-jobserver-merge)
  REIFY_JOBSERVER_TASK_FIFO    Path of the task-pool FIFO
                                (default: /tmp/reify-jobserver-task)
  REIFY_JOBSERVER_TOKENS       Total token count (default: nproc via
                                len(os.sched_getaffinity(0)))
  REIFY_JOBSERVER_POLL_INTERVAL Control-loop tick period in seconds
                                (default: 0.1; confirmed by ε tuning run,
                                see docs/prds/jobserver-merge-priority-balancer.tuning-measurements.json)
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

# ε-confirmed (PRD §10): poll interval of 0.1 s validated by the tuning harness
# (docs/prds/jobserver-merge-priority-balancer.tuning-measurements.json).
# Re-run `scripts/jobserver-tuning-harness.py --measure` at full scale to retune.
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

# ε-confirmed (PRD §10): give-back buffer of 1 token validated by the tuning
# harness (docs/prds/jobserver-merge-priority-balancer.tuning-measurements.json).
# ε=1 is the smallest buffer that exercises the give-back path (merge_baseline
# > 1 for all TOKENS≥4, so give = merge_baseline − ε > 0).
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

# Idle-reset window: consecutive idle ticks before redistributing back to the
# seeded baseline.  Value 10 is a conservative default; the ε harness does not
# derive this constant directly (idle-reset is internal policy, not an
# operator-visible tuning target), so 10 ticks × 0.1 s = 1.0 s idle dwell
# remains the production setting.
_idle_reset_raw: str = os.environ.get("REIFY_JOBSERVER_IDLE_RESET_TICKS", "10")
try:
    IDLE_RESET_TICKS: int = int(_idle_reset_raw)
    if IDLE_RESET_TICKS < 1:
        raise ValueError("must be >= 1")
except ValueError as _exc:
    sys.stderr.write(
        f"ERROR: REIFY_JOBSERVER_IDLE_RESET_TICKS={_idle_reset_raw!r}: {_exc}\n"
        f"  Set to a positive integer >= 1\n"
    )
    sys.exit(1)

# Path to the kernel PSI file (testability seam: override via env to point at a
# fixture file so tests can inject deterministic pressure values without root).
PSI_PROC_PATH: str = os.environ.get(
    "REIFY_JOBSERVER_PSI_PROC_PATH", "/proc/pressure/cpu"
)

# Pressure-reactive hold/release thresholds (hysteresis band).
# Defaults mirror verify.sh's REIFY_PSI_GATE_THRESHOLD (50 %) so the two
# admission controls agree on what "overloaded" means.
# See design decision in plan: "PRESSURE_HOLD_THRESHOLD defaults to 50.0"
_ph_raw: str = os.environ.get("REIFY_JOBSERVER_PRESSURE_HOLD_THRESHOLD", "50.0")
try:
    PRESSURE_HOLD_THRESHOLD: float = float(_ph_raw)
except ValueError as _exc:
    sys.stderr.write(
        f"ERROR: REIFY_JOBSERVER_PRESSURE_HOLD_THRESHOLD={_ph_raw!r}: {_exc}\n"
        f"  Set to a float (e.g., 50.0)\n"
    )
    sys.exit(1)

_pr_raw: str = os.environ.get("REIFY_JOBSERVER_PRESSURE_RELEASE_THRESHOLD", "40.0")
try:
    PRESSURE_RELEASE_THRESHOLD: float = float(_pr_raw)
except ValueError as _exc:
    sys.stderr.write(
        f"ERROR: REIFY_JOBSERVER_PRESSURE_RELEASE_THRESHOLD={_pr_raw!r}: {_exc}\n"
        f"  Set to a float < PRESSURE_HOLD_THRESHOLD={PRESSURE_HOLD_THRESHOLD}\n"
    )
    sys.exit(1)

if PRESSURE_RELEASE_THRESHOLD >= PRESSURE_HOLD_THRESHOLD:
    sys.stderr.write(
        f"ERROR: REIFY_JOBSERVER_PRESSURE_RELEASE_THRESHOLD must be < "
        f"PRESSURE_HOLD_THRESHOLD "
        f"(got release={PRESSURE_RELEASE_THRESHOLD}, "
        f"hold={PRESSURE_HOLD_THRESHOLD})\n"
        f"  Hysteresis band requires release < hold.\n"
    )
    sys.exit(1)

# Maximum tokens held in the pressure reservoir.
# Default: max(1, TOKENS//4) = the task_baseline (=8 at nproc=32), bounding
# the reservoir to the task pool's own allocation so merge's 24 tokens are
# never clawed.  Tunable for ε/η runs; set 0 to disable hold-back entirely.
_mhb_raw: str = os.environ.get(
    "REIFY_JOBSERVER_MAX_HELD_BACK", str(max(1, TOKENS // 4))
)
try:
    MAX_HELD_BACK: int = int(_mhb_raw)
    if MAX_HELD_BACK < 0:
        raise ValueError("must be >= 0")
except ValueError as _exc:
    sys.stderr.write(
        f"ERROR: REIFY_JOBSERVER_MAX_HELD_BACK={_mhb_raw!r}: {_exc}\n"
        f"  Set to a non-negative integer\n"
    )
    sys.exit(1)

# Break-glass: set REIFY_JOBSERVER_PRESSURE_DISABLE=1 to skip the entire
# pressure stage (mirrors REIFY_PSI_GATE_DISABLE for the verify-gate peer).
# Useful for ε/η acceptance runs that measure pure allocation without throttle.
PRESSURE_DISABLE: bool = (
    os.environ.get("REIFY_JOBSERVER_PRESSURE_DISABLE", "") == "1"
)

# State file: the daemon publishes its held_back count here on each change so
# the canary can distinguish "held back on purpose" from a real token leak.
# setup-dev.sh rm's this file in both ExecStartPre and ExecStopPost so a stale
# count from a crashed daemon can never mask a genuine leak on restart.
HELD_BACK_FILE: str = os.environ.get(
    "REIFY_JOBSERVER_HELD_BACK_FILE", "/tmp/reify-jobserver-held-back"
)

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


def read_pressure(proc_path: str):
    """Parse a /proc/pressure/cpu-format file and return avg10 as a float.

    Scans the file for the line starting with 'some', then extracts the
    'avg10=' field.  Returns None on any OSError or parse failure — fail-open:
    an unreadable PSI file must never wedge the build.  This mirrors
    verify.sh's _psi_should_pass() missing-PSI branch.

    Port of verify.sh's awk idiom: /^some/ → split on 'avg10=' token.

    Returns float (the avg10 value) or None (on any failure).
    """
    try:
        with open(proc_path) as _f:
            for _line in _f:
                if _line.startswith("some"):
                    for _token in _line.split():
                        if _token.startswith("avg10="):
                            return float(_token[len("avg10="):])
        return None  # 'some' line absent in file
    except (OSError, ValueError):
        return None  # unreadable file or malformed float — fail-open


def pressure_decide(
    avg10,
    hold_threshold: float,
    release_threshold: float,
    free_task: int,
    held_back: int,
    max_held_back: int,
) -> tuple:
    """Pure pressure-control policy: given avg10, return (action, count).

    action ∈ {"hold", "release", "none"}
    count  = tokens to grab into ("hold") or release from ("release") the
             reservoir.

    Hysteresis band (prevents threshold-boundary oscillation):
      avg10 >= hold_threshold   → ("hold", min(free_task, max_held_back - held_back))
      avg10 <  release_threshold → ("release", held_back)
      release ≤ avg10 < hold     → ("none", 0)
      avg10 is None (fail-open)  → treated as low pressure (release if held>0)

    Any computed count == 0 collapses to ("none", 0).

    MERGE-SAFE: no free_merge parameter — pressure only ever touches the TASK
    pool (enforced by this signature and the call site in main()).
    """
    if avg10 is None or avg10 < release_threshold:
        # fail-open (PSI unreadable) or below release threshold → release reservoir
        count = held_back
        if count > 0:
            return ("release", count)
        return ("none", 0)

    if avg10 >= hold_threshold:
        # above hold threshold → grab tokens into reservoir (bounded by headroom)
        headroom = max_held_back - held_back
        count = min(free_task, headroom)
        if count > 0:
            return ("hold", count)
        return ("none", 0)

    # Hysteresis band: release_threshold ≤ avg10 < hold_threshold → no action
    return ("none", 0)


def suppress_giveback(avg10, release_threshold: float, held_back: int) -> bool:
    """Return True when C4 merge→task give-back should be suppressed.

    Suppression is active when either condition holds:
      (1) avg10 >= release_threshold: pressure is still above the release edge,
          so the controller is in the hold or hysteresis-band phase.  Allowing
          give-back (m2t) here would refill the task pool from merge, and the
          pressure stage would immediately claw those tokens back into the
          reservoir — a back-door merge drain.
      (2) held_back > 0: the reservoir is non-empty.  Even if pressure just
          dropped below release_threshold, we must release the reservoir (via
          the "release" path in pressure_decide) BEFORE re-enabling give-back;
          otherwise the freshly-donated merge tokens are reclawed before they
          reach task consumers.

    Fail-open (avg10 is None → PSI unreadable): suppression fires only when the
    reservoir is non-empty (held_back > 0), matching the principle that an
    unreadable PSI file must never wedge the build.

    Pure function, no side effects.
    """
    return (avg10 is not None and avg10 >= release_threshold) or held_back > 0


def _grab_burst(donor_fd: int, max_count: int) -> int:
    """Non-blocking drain up to max_count tokens from donor_fd into the reservoir.

    Mirrors the read-half of _transfer_burst but WITHOUT a recipient FIFO:
    the bytes are consumed from the donor and their count returned — they are
    conserved in the caller's `held_back` counter and re-injected into the
    donor later via seed_fifo when pressure_decide returns "release".

    Stops on BlockingIOError (EAGAIN — donor empty) or max_count reached.
    Returns the number of tokens absorbed (0 … max_count).

    C1 conservation: `held_back += _grab_burst(...)` keeps the total
    `free_merge + free_task + held_by_rustc + held_back == TOKENS` invariant.
    """
    absorbed = 0
    while absorbed < max_count:
        try:
            os.read(donor_fd, 1)
            absorbed += 1
        except BlockingIOError:
            break  # donor drained (EAGAIN)
    return absorbed


# Write-on-change cache for write_held_back() (module-level sentinel).
_held_back_last: list = [None]


def write_held_back(path: str, n: int) -> None:
    """Atomically publish the held-back token count to path (write-on-change).

    Writes str(n) to a tmp file then renames atomically, so a concurrent
    canary reader always sees a complete integer, never a partial write.
    Skips the write when n equals the last written value (write-on-change)
    to avoid unnecessary filesystem churn on each control-loop tick.
    """
    if _held_back_last[0] == n:
        return  # no change — skip write
    _tmp = path + ".tmp"
    try:
        with open(_tmp, "w") as _f:
            _f.write(str(n))
        os.rename(_tmp, path)
        _held_back_last[0] = n
    except OSError as _exc:
        sys.stderr.write(f"WARNING: write_held_back({path!r}, {n}): {_exc}\n")


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

    Requires: baseline_merge + baseline_task == tokens.
    main() guarantees this via:
        task_baseline  = max(1, TOKENS // 4)
        merge_baseline = TOKENS - task_baseline  # sum == TOKENS by construction
    Violated baselines cause the IDLE branch to oscillate (the toward-baseline
    move overshoots, then the next tick undershoots, indefinitely).  An assert
    below guards this so a future caller cannot silently induce oscillation.
    """
    assert baseline_merge + baseline_task == tokens, (
        f"decide() precondition violated: "
        f"baseline_merge({baseline_merge}) + baseline_task({baseline_task}) "
        f"!= tokens({tokens}) — caller must ensure partition sums to tokens"
    )

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
    # ε-confirmed (PRD §4 C4 / §10; tuning-measurements.json):
    #   task_baseline  = max(1, TOKENS // 4)   (~1/4 of pool, minimum 1)
    #   merge_baseline = TOKENS - task_baseline (~3/4 of pool)
    #
    # For TOKENS=32 (nproc on the reference host): 24/8.  For TOKENS=4: 3/1.
    # The ε harness confirmed task_baseline=8 / merge_baseline=24 for TOKENS=32
    # (docs/prds/jobserver-merge-priority-balancer.tuning-measurements.json).
    #
    # Invariants guaranteed by construction:
    #   merge_baseline > task_baseline  (merge-favored, PRD §4 C4)
    #   task_baseline  >= 1              (non-starving; prevents idle thrash)
    #   merge + task   == TOKENS         (C1 token conservation)
    #
    # Tests assert the PARTITION PROPERTY, not a numeric value, so scale-up
    # retuning will not break them.
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

    # ── Control loop: SENSE → PRESSURE → idle_ticks → decide() → execute ────
    #
    # Each tick:
    #   1. SENSE both pools via FIONREAD (non-destructive).
    #   2. PRESSURE STAGE (guarded by PRESSURE_DISABLE break-glass):
    #        read avg10 from PSI_PROC_PATH → pressure_decide() → hold/release.
    #        "hold"    → _grab_burst(task_fd, n): absorb n tokens into held_back
    #        "release" → seed_fifo(task_fd, n): write held_back tokens back; held_back-=n
    #        re-SENSE free_task; publish held_back to HELD_BACK_FILE on change.
    #        MERGE-SAFE: only task_fd is touched; merge is protected by
    #        suppress_giveback() applied to the "m2t" output in step 5.
    #   3. Maintain idle_ticks counter (held_back included so a quiet box with
    #      a non-empty reservoir still counts as idle for baseline-reset):
    #        free_merge+free_task+held_back == TOKENS → increment idle_ticks
    #        else → reset to 0
    #   4. Call decide(free_merge, free_task, …) for the C4 policy action.
    #   5. Suppress m2t when pressure is active or reservoir non-empty:
    #        if action=="m2t" and suppress_giveback(avg10, …, held_back): → "none"
    #   6. Execute the (possibly suppressed) action via _transfer_burst:
    #        "t2m" → _transfer_burst(task_fd, merge_fd, count)
    #        "m2t" → _transfer_burst(merge_fd, task_fd, count)
    #        "none" → no-op
    #   7. Sleep POLL_INTERVAL.
    #
    # C4 policy summary (full details in decide() docstring):
    #   IDLE   → reset toward baseline after IDLE_RESET_TICKS idle ticks
    #   MERGE-DEMANDED (free_merge=0, task spare) → burst task→merge (monotone)
    #   TASK-DEMANDED  (free_task=0, merge>ε)     → burst merge→task, retain ε
    #                  (suppressed under pressure by step 5)
    #   otherwise → no-op
    #
    # C1 conservation: _transfer_burst wraps _transfer (one token in-flight per
    # inner call, never dropped); _grab_burst absorbs into held_back (not dropped);
    # held_back is re-injected via seed_fifo on release.  Total invariant:
    #   free_merge + free_task + held_by_rustc + held_back == TOKENS throughout.
    # GNU-jobserver demand signal: a pool reaching 0-free means consumers hold
    # all its tokens — the balancer observes this via FIONREAD (non-destructive).

    idle_ticks: int = 0
    held_back: int = 0
    avg10 = None  # initialise before loop (used by suppress_giveback on tick 1)
    write_held_back(HELD_BACK_FILE, 0)  # publish 0 at startup; clears stale state

    while not _stop[0]:
        # ── SENSE ──────────────────────────────────────────────────────────
        free_merge = fionread(merge_fd)
        free_task  = fionread(task_fd)

        # ── PRESSURE STAGE (runs before C4 decide()) ───────────────────────
        # Read CPU pressure (PSI avg10) and adjust the task-pool reservoir.
        # MERGE-SAFE: _grab_burst drains from task_fd only; the merge pool is
        # protected by the suppress_giveback guard applied to "m2t" below.
        if not PRESSURE_DISABLE:
            avg10 = read_pressure(PSI_PROC_PATH)
            p_action, p_count = pressure_decide(
                avg10,
                PRESSURE_HOLD_THRESHOLD,
                PRESSURE_RELEASE_THRESHOLD,
                free_task,
                held_back,
                MAX_HELD_BACK,
            )
            _prev_hb = held_back
            if p_action == "hold":
                held_back += _grab_burst(task_fd, p_count)
            elif p_action == "release":
                seed_fifo(task_fd, p_count)
                held_back -= p_count
            # Re-sense free_task: pressure stage may have moved tokens
            free_task = fionread(task_fd)
            if held_back != _prev_hb:
                write_held_back(HELD_BACK_FILE, held_back)

        # ── Maintain idle_ticks counter ────────────────────────────────────
        # Include held_back so a quiet box with a non-empty reservoir still
        # counts as globally idle for baseline-reset purposes (design §6).
        if free_merge + free_task + held_back == TOKENS:
            idle_ticks += 1
        else:
            idle_ticks = 0

        # ── Decide → Execute (C4 policy) ───────────────────────────────────
        action, count = decide(
            free_merge=free_merge,
            free_task=free_task,
            tokens=TOKENS,
            baseline_merge=merge_baseline,
            baseline_task=task_baseline,
            epsilon=EPSILON,
            idle_ticks=idle_ticks,
            idle_threshold=IDLE_RESET_TICKS,
        )

        # ── Suppress merge→task give-back under pressure ────────────────────
        # When pressure is active or the reservoir is non-empty, block C4's
        # "m2t": allowing it would refill task from merge, and the pressure
        # stage would immediately re-claw those tokens back into the reservoir
        # — a back-door drain of the merge pool.  suppress_giveback() closes
        # that back-door (design decision "Pressure hold-back targets TASK only").
        if action == "m2t" and suppress_giveback(
            avg10, PRESSURE_RELEASE_THRESHOLD, held_back
        ):
            action, count = "none", 0

        if action == "t2m":
            _transfer_burst(task_fd, merge_fd, count)
        elif action == "m2t":
            # ε retention is best-effort under concurrent merge consumption:
            # count = free_merge - epsilon was computed from the FIONREAD
            # snapshot; a concurrent merge consumer may drain merge to 0
            # before the burst completes.  C1 conservation is preserved
            # (no token is dropped); the state self-corrects on the next tick.
            _transfer_burst(merge_fd, task_fd, count)
        # "none" → no-op

        time.sleep(POLL_INTERVAL)

    # Clean exit — fds are closed by the OS when the process exits.


if __name__ == "__main__":
    main()
