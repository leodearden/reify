#!/usr/bin/env python3
"""
jobserver-tuning-harness.py — empirical tuning harness for the dual-FIFO
jobserver balancer constants (task ε, PRD docs/prds/jobserver-merge-priority-
balancer.md §9/§10).

Drives three cargo load regimes (just-task, just-merge, mixed) at warm and
cold sccache against BOTH a self-provisioned single-pool baseline (A) and the
deployed dual-pool balancer (B), collecting:
  - CPU busy-core fraction from /proc/stat
  - per-pool FIONREAD occupancy time series
  - wall-clock of the merge verify and the SLOWEST task verify
  - exit-124 (timeout) count

Derived outputs (the balancer constants this PRD ships):
  - baseline idle split (merge-favored, sums to nproc, task ≥ 1)
  - poll interval ≥ floor
  - ε give-back buffer ≥ 1
  - task_timeout = ceil(worst_case_cold_task × MARGIN)
  - merge_timeout = ceil(measured_merge × MARGIN)
  - utilization_threshold derived from the baseline capture

CLI modes
---------
  --measure <output.json>    Run the A/B campaign and write raw measurements
  --derive  <input.json>     Load measurements, print derived constants
  --report  <input.json>     Load measurements+derived, write tuning-report.md
  --check   <input.json>     Re-derive, assert floors, exit 0/1 (CI-safe)

Environment variables (all optional, with sensible defaults)
------------------------------------------------------------
  REIFY_JOBSERVER_MERGE_FIFO      Path of the merge-pool FIFO
  REIFY_JOBSERVER_TASK_FIFO       Path of the task-pool FIFO
  REIFY_JOBSERVER_TOKENS          Total token count (default: nproc)
  REIFY_TUNING_NPROC              Override nproc detection (default: os.sched_getaffinity)
  REIFY_TUNING_MARGIN             Timeout margin multiplier (default: 1.5)
  REIFY_TUNING_MAX_SANE_TIMEOUT   Maximum sane timeout in seconds (default: 7200)
  REIFY_TUNING_MIN_POLL_INTERVAL  Minimum poll interval in seconds (default: 0.05)
  REIFY_SCCACHE_CACHE_DIR         sccache cache directory to clear for cold runs
"""

import argparse
import fcntl
import os
import struct
import subprocess
import sys
import termios
import time

# ──────────────────────────────────────────────────────────────────────────────
# Configuration — read from environment, fall back to sensible defaults
# ──────────────────────────────────────────────────────────────────────────────

_nproc_raw: str = os.environ.get("REIFY_TUNING_NPROC", "")
if _nproc_raw:
    try:
        NPROC: int = int(_nproc_raw)
        if NPROC < 1:
            raise ValueError("must be >= 1")
    except ValueError as _exc:
        sys.stderr.write(
            f"ERROR: REIFY_TUNING_NPROC={_nproc_raw!r}: {_exc}\n"
            f"  Set to a positive integer\n"
        )
        sys.exit(1)
else:
    NPROC = len(os.sched_getaffinity(0))

_margin_raw: str = os.environ.get("REIFY_TUNING_MARGIN", "1.5")
try:
    MARGIN: float = float(_margin_raw)
    if MARGIN <= 1.0:
        raise ValueError("must be > 1.0")
except ValueError as _exc:
    sys.stderr.write(
        f"ERROR: REIFY_TUNING_MARGIN={_margin_raw!r}: {_exc}\n"
        f"  Set to a float > 1.0\n"
    )
    sys.exit(1)

_max_sane_raw: str = os.environ.get("REIFY_TUNING_MAX_SANE_TIMEOUT", "7200")
try:
    MAX_SANE_TIMEOUT: int = int(_max_sane_raw)
    if MAX_SANE_TIMEOUT < 1:
        raise ValueError("must be >= 1")
except ValueError as _exc:
    sys.stderr.write(
        f"ERROR: REIFY_TUNING_MAX_SANE_TIMEOUT={_max_sane_raw!r}: {_exc}\n"
        f"  Set to a positive integer\n"
    )
    sys.exit(1)

_min_poll_raw: str = os.environ.get("REIFY_TUNING_MIN_POLL_INTERVAL", "0.05")
try:
    MIN_POLL_INTERVAL: float = float(_min_poll_raw)
    if MIN_POLL_INTERVAL <= 0:
        raise ValueError("must be > 0")
except ValueError as _exc:
    sys.stderr.write(
        f"ERROR: REIFY_TUNING_MIN_POLL_INTERVAL={_min_poll_raw!r}: {_exc}\n"
        f"  Set to a positive float\n"
    )
    sys.exit(1)

# FIFO paths (for campaign runs — harness also accepts overrides)
MERGE_FIFO: str = os.environ.get(
    "REIFY_JOBSERVER_MERGE_FIFO", "/tmp/reify-jobserver-merge"
)
TASK_FIFO: str = os.environ.get(
    "REIFY_JOBSERVER_TASK_FIFO", "/tmp/reify-jobserver-task"
)

# sccache cache directory (cleared to simulate cold-cache runs)
SCCACHE_CACHE_DIR: str = os.environ.get(
    "REIFY_SCCACHE_CACHE_DIR",
    os.path.expanduser("~/.cache/sccache"),
)

# Regime names (constants so test code can import them)
REGIME_JUST_TASK = "just-task"
REGIME_JUST_MERGE = "just-merge"
REGIME_MIXED = "mixed"
REGIMES = [REGIME_JUST_TASK, REGIME_JUST_MERGE, REGIME_MIXED]

# Service labels
SERVICE_SINGLE_POOL = "single-pool"
SERVICE_DUAL_POOL = "dual-pool"

# Cache states
CACHE_WARM = "warm"
CACHE_COLD = "cold"


# ──────────────────────────────────────────────────────────────────────────────
# Pure functions
# ──────────────────────────────────────────────────────────────────────────────


def busy_fraction(stat_before: str, stat_after: str, nproc: int):
    """Compute CPU busy-core fraction from two /proc/stat 'cpu …' snapshots.

    Parses the aggregate 'cpu' line from each snapshot string.  The line
    format is (fields after the 'cpu' label, 0-based):
        user  nice  system  idle  iowait  irq  softirq  steal  guest  guest_nice

    busy  = Σ(user + nice + system + irq + softirq + steal) delta
    idle  = Σ(idle + iowait) delta
    total = busy + idle

    Returns
    -------
    (fraction, busy_cores) where
      fraction   : float in [0.0, 1.0] — busy / total (0.0 if total == 0)
      busy_cores : float — fraction × nproc
    """

    def _parse(line: str):
        """Return a list of ints from a /proc/stat cpu line."""
        parts = line.split()
        # Skip the leading 'cpu' label; handle both 'cpu' and 'cpu0' etc.
        idx = 0
        while idx < len(parts) and not parts[idx][0].isdigit():
            idx += 1
        return [int(p) for p in parts[idx:]]

    b = _parse(stat_before)
    a = _parse(stat_after)

    # Field indices (0-based within the numeric section):
    # 0=user 1=nice 2=system 3=idle 4=iowait 5=irq 6=softirq 7=steal
    BUSY_FIELDS = (0, 1, 2, 5, 6, 7)   # user, nice, system, irq, softirq, steal
    IDLE_FIELDS = (3, 4)               # idle, iowait

    busy_delta = sum(a[i] - b[i] for i in BUSY_FIELDS if i < len(a) and i < len(b))
    idle_delta = sum(a[i] - b[i] for i in IDLE_FIELDS if i < len(a) and i < len(b))
    total_delta = busy_delta + idle_delta

    if total_delta == 0:
        return 0.0, 0.0

    fraction = busy_delta / total_delta
    busy_cores = fraction * nproc
    return fraction, busy_cores


def _fionread(fd: int) -> int:
    """Return the number of bytes readable on *fd* via FIONREAD (non-destructive)."""
    buf = struct.pack("i", 0)
    return struct.unpack("i", fcntl.ioctl(fd, termios.FIONREAD, buf))[0]


def sample_pool_occupancy(merge_fifo: str, task_fifo: str) -> dict:
    """Sample token counts from both FIFO pools in a single process.

    Opens BOTH FIFOs with O_RDONLY | O_NONBLOCK and reads FIONREAD for each
    within the same Python process, minimising the race where a balancer
    transfer fires between two separate shell/subprocess calls.

    Returns a dict with:
        merge      : int — token count in the merge pool
        task       : int — token count in the task pool
        sum        : int — merge + task
        timestamp  : float — monotonic time at sampling (time.monotonic())
    """
    merge_fd = os.open(merge_fifo, os.O_RDONLY | os.O_NONBLOCK)
    try:
        task_fd = os.open(task_fifo, os.O_RDONLY | os.O_NONBLOCK)
        try:
            merge_count = _fionread(merge_fd)
            task_count  = _fionread(task_fd)
            ts = time.monotonic()
        finally:
            os.close(task_fd)
    finally:
        os.close(merge_fd)

    return {
        "merge":     merge_count,
        "task":      task_count,
        "sum":       merge_count + task_count,
        "timestamp": ts,
    }


def timed_run(cmd_list: list) -> tuple:
    """Run *cmd_list* via subprocess and return (elapsed_seconds, returncode).

    elapsed_seconds is the wall-clock duration measured with time.monotonic().
    returncode is the process exit code.
    """
    t0 = time.monotonic()
    result = subprocess.run(cmd_list, check=False)
    elapsed = time.monotonic() - t0
    return elapsed, result.returncode


def is_timeout(returncode: int) -> bool:
    """Return True iff *returncode* is 124 (the POSIX timeout(1) exit code).

    cargo/make propagates 124 through the jobserver timeout path, so this is
    the canonical 'this verify exceeded its time budget' signal.
    """
    return returncode == 124


# Stubs for later steps (implemented in steps 6, 8, 10, 12, 14).


def main() -> None:
    """CLI entry point for the four harness modes."""
    parser = argparse.ArgumentParser(
        description=(
            "jobserver-tuning-harness — empirical tuning for the dual-FIFO "
            "balancer constants (PRD §9/§10)."
        )
    )
    sub = parser.add_subparsers(dest="mode")

    p_measure = sub.add_parser(
        "measure",
        help="Run A/B campaign and write raw measurements JSON",
    )
    p_measure.add_argument("output", help="Output JSON path")

    p_derive = sub.add_parser(
        "derive",
        help="Load measurements JSON and print derived constants",
    )
    p_derive.add_argument("input", help="Input measurements JSON path")

    p_report = sub.add_parser(
        "report",
        help="Load measurements JSON and write tuning-report.md",
    )
    p_report.add_argument("input", help="Input measurements JSON path")
    p_report.add_argument("output", nargs="?", help="Output report path (default: stdout)")

    p_check = sub.add_parser(
        "check",
        help=(
            "Load committed measurements JSON, re-derive, assert floors. "
            "Exit 0 on pass or escape-valve finding; exit 1 on unexplained "
            "floor violation."
        ),
    )
    p_check.add_argument("input", help="Input measurements JSON path")

    # Support both 'mode' and '--mode' forms for ergonomic CLI use.
    # e.g. both '--check file.json' and 'check file.json' are accepted.
    argv = sys.argv[1:]
    if argv and argv[0] in ("--measure", "--derive", "--report", "--check"):
        argv[0] = argv[0].lstrip("-")

    args = parser.parse_args(argv)

    if args.mode is None:
        parser.print_help()
        sys.exit(0)

    sys.stderr.write(
        f"ERROR: mode '{args.mode}' not yet implemented\n"
        f"  This skeleton will be filled by the ε implementation steps.\n"
    )
    sys.exit(1)


if __name__ == "__main__":
    main()
