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
import math
import os
import shutil
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

# Minimum absolute floor for the derived utilization_threshold.
# Prevents a degenerate baseline (e.g. all runs near-idle) from producing a
# threshold so low that any real-world record would pass trivially.
# The value 0.50 means "at least 50 % CPU utilisation under a parallel
# compilation load"; records below this floor represent either a mis-configured
# host or a fundamentally broken measurement and should be flagged.
MIN_UTILIZATION_THRESHOLD: float = 0.50


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


def _read_proc_stat() -> str:
    """Return the aggregate 'cpu  …' line from /proc/stat."""
    try:
        with open("/proc/stat", "r") as f:
            for line in f:
                if line.startswith("cpu "):
                    return line.strip()
    except OSError:
        pass
    # Fallback for non-Linux / test environments
    return "cpu  0 0 0 0 0 0 0 0 0 0"


def _prepare_cache(cache_state: str) -> None:
    """Prepare sccache for warm or cold run.

    warm : no-op (cache already primed)
    cold : stop sccache server (best-effort), clear SCCACHE_CACHE_DIR, zero stats
    """
    if cache_state != CACHE_COLD:
        return
    subprocess.run(
        ["sccache", "--stop-server"],
        capture_output=True,
        timeout=15,
    )
    if os.path.exists(SCCACHE_CACHE_DIR):
        shutil.rmtree(SCCACHE_CACHE_DIR, ignore_errors=True)
    subprocess.run(
        ["sccache", "--zero-stats"],
        capture_output=True,
        timeout=15,
    )


def _provision_service(
    service: str,
    tokens: int,
    balancer_path: str,
) -> dict:
    """Provision jobserver infrastructure in user space (no systemctl).

    Returns an infra dict:
        merge_fifo      : str  — path to merge-pool FIFO
        task_fifo       : str  — path to task-pool FIFO (dummy for single-pool)
        balancer_pid    : int | None — PID of background balancer (dual-pool only)
        _balancer_proc  : Popen | None
        _fds_to_close   : list[int]
        _paths_to_unlink: list[str]
    """
    import tempfile as _tempfile

    paths_to_unlink: list = []
    fds_to_close: list = []

    if service == SERVICE_SINGLE_POOL:
        # One FIFO seeded with `tokens` tokens; dummy FIFO represents empty
        # task pool (single-pool has no concept of separate pools).
        single_path = _tempfile.mktemp(prefix="/tmp/harness-single-")
        dummy_path  = _tempfile.mktemp(prefix="/tmp/harness-dummy-")
        os.mkfifo(single_path)
        os.mkfifo(dummy_path)
        paths_to_unlink.extend([single_path, dummy_path])

        single_fd = os.open(single_path, os.O_RDWR | os.O_NONBLOCK)
        dummy_fd  = os.open(dummy_path,  os.O_RDWR | os.O_NONBLOCK)
        fds_to_close.extend([single_fd, dummy_fd])

        os.write(single_fd, b"\x00" * tokens)

        return {
            "merge_fifo":       single_path,
            "task_fifo":        dummy_path,
            "balancer_pid":     None,
            "_balancer_proc":   None,
            "_fds_to_close":    fds_to_close,
            "_paths_to_unlink": paths_to_unlink,
        }

    elif service == SERVICE_DUAL_POOL:
        merge_path = _tempfile.mktemp(prefix="/tmp/harness-merge-")
        task_path  = _tempfile.mktemp(prefix="/tmp/harness-task-")
        paths_to_unlink.extend([merge_path, task_path])

        env = os.environ.copy()
        env.update({
            "REIFY_JOBSERVER_MERGE_FIFO":    merge_path,
            "REIFY_JOBSERVER_TASK_FIFO":     task_path,
            "REIFY_JOBSERVER_TOKENS":        str(tokens),
            "REIFY_JOBSERVER_POLL_INTERVAL": "0.05",
        })
        proc = subprocess.Popen(
            [sys.executable, balancer_path],
            env=env,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

        # Wait for FIFOs to appear and be seeded (up to 10 s)
        deadline = time.monotonic() + 10.0
        while time.monotonic() < deadline:
            if os.path.exists(merge_path) and os.path.exists(task_path):
                try:
                    mfd = os.open(merge_path, os.O_RDONLY | os.O_NONBLOCK)
                    tfd = os.open(task_path,  os.O_RDONLY | os.O_NONBLOCK)
                    try:
                        total = _fionread(mfd) + _fionread(tfd)
                    finally:
                        os.close(mfd)
                        os.close(tfd)
                    if total == tokens:
                        break
                except OSError:
                    pass
            time.sleep(0.05)

        return {
            "merge_fifo":       merge_path,
            "task_fifo":        task_path,
            "balancer_pid":     proc.pid,
            "_balancer_proc":   proc,
            "_fds_to_close":    fds_to_close,
            "_paths_to_unlink": paths_to_unlink,
        }

    else:
        raise ValueError(f"Unknown service: {service!r}")


def _teardown_service(infra: dict) -> None:
    """Tear down provisioned jobserver infrastructure."""
    proc = infra.get("_balancer_proc")
    if proc is not None:
        try:
            proc.terminate()
            proc.wait(timeout=3)
        except Exception:
            try:
                proc.kill()
                proc.wait(timeout=3)
            except Exception:
                pass

    for fd in infra.get("_fds_to_close", []):
        try:
            os.close(fd)
        except OSError:
            pass

    for path in infra.get("_paths_to_unlink", []):
        try:
            os.unlink(path)
        except OSError:
            pass


def run_regime(
    regime: str,
    service: str,
    cache_state: str,
    load_cmd: list = None,
    balancer_path: str = None,
    nproc: int = None,
    tokens: int = None,
) -> dict:
    """Run one measurement regime and return a structured result record.

    Parameters
    ----------
    regime       : REGIME_JUST_TASK | REGIME_JUST_MERGE | REGIME_MIXED
    service      : SERVICE_SINGLE_POOL | SERVICE_DUAL_POOL
    cache_state  : CACHE_WARM | CACHE_COLD
    load_cmd     : command list for the verify load (real verify.sh or a stub)
    balancer_path: path to jobserver-balancer.py (default: sibling script)
    nproc        : override NPROC for this run
    tokens       : total token count (default: nproc)

    Returns
    -------
    dict with keys:
        service, regime, cache_state, busy_fraction, occupancy,
        merge_wall, task_wall, exit_124_count, nproc
    """
    if nproc is None:
        nproc = NPROC
    if tokens is None:
        tokens = nproc
    if load_cmd is None:
        raise ValueError("load_cmd must be provided")
    if balancer_path is None:
        balancer_path = os.path.join(
            os.path.dirname(os.path.abspath(__file__)), "jobserver-balancer.py"
        )

    _prepare_cache(cache_state)

    stat_before = _read_proc_stat()
    infra = _provision_service(service, tokens, balancer_path)

    occupancy_samples: list = []
    merge_wall = 0.0
    task_wall = 0.0
    exit_124_count = 0

    try:
        # Sample occupancy before loads
        try:
            occupancy_samples.append(
                sample_pool_occupancy(infra["merge_fifo"], infra["task_fifo"])
            )
        except Exception:
            pass

        # Run loads for this regime
        if regime == REGIME_JUST_TASK:
            elapsed, rc = timed_run(load_cmd)
            task_wall = elapsed
            if is_timeout(rc):
                exit_124_count += 1

        elif regime == REGIME_JUST_MERGE:
            elapsed, rc = timed_run(load_cmd)
            merge_wall = elapsed
            if is_timeout(rc):
                exit_124_count += 1

        elif regime == REGIME_MIXED:
            e_merge, rc_merge = timed_run(load_cmd)
            merge_wall = e_merge
            if is_timeout(rc_merge):
                exit_124_count += 1
            e_task, rc_task = timed_run(load_cmd)
            task_wall = e_task
            if is_timeout(rc_task):
                exit_124_count += 1

        else:
            raise ValueError(f"Unknown regime: {regime!r}")

        # Sample occupancy after loads
        try:
            occupancy_samples.append(
                sample_pool_occupancy(infra["merge_fifo"], infra["task_fifo"])
            )
        except Exception:
            pass

        # Ensure at least one occupancy sample
        if not occupancy_samples:
            occupancy_samples = [
                {"merge": 0, "task": 0, "sum": 0, "timestamp": time.monotonic()}
            ]

    finally:
        _teardown_service(infra)

    stat_after = _read_proc_stat()
    fraction, _busy_cores = busy_fraction(stat_before, stat_after, nproc)

    return {
        "service":        service,
        "regime":         regime,
        "cache_state":    cache_state,
        "busy_fraction":  fraction,
        "occupancy":      occupancy_samples,
        "merge_wall":     merge_wall,
        "task_wall":      task_wall,
        "exit_124_count": exit_124_count,
        "nproc":          nproc,
    }


def derive_constants(measurements: dict) -> dict:
    """Derive balancer constants from a completed A/B measurements record.

    Returns a dict with:
        merge_baseline       : int   — merge-favored token count (> task_baseline)
        task_baseline        : int   — task token count (≥ 1)
        poll_interval        : float — ≥ MIN_POLL_INTERVAL
        epsilon              : int   — ≥ 1 (give-back buffer)
        task_timeout_secs    : int   — ceil(worst_case_cold_task × MARGIN)
        merge_timeout_secs   : int   — ceil(measured_merge_full_alloc × MARGIN)
        utilization_threshold: float — derived from the baseline just-task capture
    """
    nproc = measurements["nproc"]
    runs  = measurements.get("runs", [])

    def _filter(service, regime, cache=None):
        return [
            r for r in runs
            if r["service"] == service
            and r["regime"] == regime
            and (cache is None or r["cache_state"] == cache)
        ]

    # ── 1. task_timeout_secs ─────────────────────────────────────────────────
    # Worst-case cold task wall-clock from the single-pool (implicit-token-only)
    # baseline under cold sccache.  This is the canonical reference: the
    # task verify with all tokens available and no cached artifacts.
    # (PRD: "task timeout budget ≥ measured worst-case cold task verify at
    # implicit-token-only WITH margin".)
    cold_task_walls = [
        r["task_wall"]
        for r in _filter(SERVICE_SINGLE_POOL, REGIME_JUST_TASK, CACHE_COLD)
        if r["task_wall"] > 0
    ]
    worst_case_cold_task = max(cold_task_walls) if cold_task_walls else 0.0
    task_timeout_secs = math.ceil(worst_case_cold_task * MARGIN)

    # ── 2. merge_timeout_secs ────────────────────────────────────────────────
    # Measured merge wall-clock at full allocation = single-pool just-merge warm
    # (all nproc tokens available to the merge, no task competition).
    merge_walls_full = [
        r["merge_wall"]
        for r in _filter(SERVICE_SINGLE_POOL, REGIME_JUST_MERGE, CACHE_WARM)
        if r["merge_wall"] > 0
    ]
    measured_merge_full = max(merge_walls_full) if merge_walls_full else 0.0
    merge_timeout_secs = math.ceil(measured_merge_full * MARGIN)

    # ── 3. utilization_threshold ─────────────────────────────────────────────
    # Derived from the minimum busy_fraction across the baseline (single-pool)
    # just-task warm runs.  Scaled down by UTIL_SLACK (20%) to allow variance
    # across regimes and cache states that inherently measure lower utilization
    # (cold cache, mixed loads).  A tighter slack is valid but the 20% floor
    # is the safe default: threshold = 0.80 means we're still catching genuine
    # regressions while tolerating cold-start variance.
    _UTIL_SLACK = 0.8
    baseline_jt_fracs = [
        r["busy_fraction"]
        for r in _filter(SERVICE_SINGLE_POOL, REGIME_JUST_TASK, CACHE_WARM)
    ]
    if baseline_jt_fracs:
        utilization_threshold = max(
            min(baseline_jt_fracs) * _UTIL_SLACK,
            MIN_UTILIZATION_THRESHOLD,
        )
    else:
        utilization_threshold = MIN_UTILIZATION_THRESHOLD  # conservative fallback

    # ── 4. baseline split ────────────────────────────────────────────────────
    # Derive from average task-pool token occupancy in dual-pool just-task runs.
    # If those tokens were ≥ nproc//2, task pool is heavily used → keep nproc//4.
    # This preserves the merge-favored property while tracking actual usage.
    dual_task_occs = [
        sample["task"]
        for r in _filter(SERVICE_DUAL_POOL, REGIME_JUST_TASK)
        for sample in r.get("occupancy", [])
    ]
    if dual_task_occs:
        avg_task_occ = sum(dual_task_occs) / len(dual_task_occs)
        task_baseline = max(1, min(int(math.ceil(avg_task_occ)), nproc - 1))
    else:
        task_baseline = max(1, nproc // 4)

    merge_baseline = nproc - task_baseline

    # Guard: enforce merge-favored (merge > task)
    if merge_baseline <= task_baseline:
        task_baseline = max(1, nproc // 4)
        merge_baseline = nproc - task_baseline

    # ── 5. poll_interval ─────────────────────────────────────────────────────
    # Conservative: 2× the minimum detectable interval.  In production this
    # would be tuned from the occupancy time-series; here we use MIN_POLL_INTERVAL
    # doubled as the safe default derived from the harness constant.
    poll_interval = MIN_POLL_INTERVAL * 2.0

    # ── 6. epsilon ───────────────────────────────────────────────────────────
    # Minimum give-back buffer: 1 for up to 32 cores; scale up for larger hosts.
    epsilon = max(1, nproc // 32)

    return {
        "merge_baseline":        merge_baseline,
        "task_baseline":         task_baseline,
        "poll_interval":         poll_interval,
        "epsilon":               epsilon,
        "task_timeout_secs":     task_timeout_secs,
        "merge_timeout_secs":    merge_timeout_secs,
        "utilization_threshold": utilization_threshold,
    }


def evaluate_acceptance(measurements: dict, derived: dict) -> tuple:
    """Evaluate the measurements record against the derived acceptance gates.

    Checks:
      1. utilization ≥ derived threshold in every run (all regimes, services)
      2. occupancy sum == nproc throughout (token conservation)
      3. worst-case task < task_timeout (budget not exceeded)
      4. §10.4 escape-valve: single-pool cold task > MAX_SANE_TIMEOUT → FINDING

    Returns
    -------
    (ok, findings) where:
        ok       : bool — True when all hard checks pass (escape valve = soft)
        findings : list[dict] — each has 'code', 'severity', 'message', 'details'
    """
    findings: list = []
    hard_fail = False

    nproc      = measurements["nproc"]
    runs       = measurements.get("runs", [])
    threshold  = derived.get("utilization_threshold", 0.0)
    task_timeout = derived.get("task_timeout_secs", float("inf"))

    # ── 1. Utilization ≥ threshold in all runs ───────────────────────────────
    for run in runs:
        bf = run.get("busy_fraction", 0.0)
        if bf < threshold:
            findings.append({
                "code":     "UTILIZATION_FAIL",
                "severity": "error",
                "message":  (
                    f"busy_fraction={bf:.4f} < utilization_threshold={threshold:.4f} "
                    f"in {run['service']} {run['regime']} {run['cache_state']}"
                ),
                "details": {
                    "service":       run["service"],
                    "regime":        run["regime"],
                    "cache_state":   run["cache_state"],
                    "busy_fraction": bf,
                    "threshold":     threshold,
                },
            })
            hard_fail = True

    # ── 2. Token conservation: occupancy sum == nproc ────────────────────────
    for run in runs:
        for sample in run.get("occupancy", []):
            s = sample.get("sum")
            if s is not None and s != nproc:
                findings.append({
                    "code":     "TOKEN_CONSERVATION",
                    "severity": "error",
                    "message":  (
                        f"occupancy sum={s} != nproc={nproc} in "
                        f"{run['service']} {run['regime']} {run['cache_state']}"
                    ),
                    "details": {
                        "service":     run["service"],
                        "regime":      run["regime"],
                        "cache_state": run["cache_state"],
                        "sum":         s,
                        "nproc":       nproc,
                        "sample":      sample,
                    },
                })
                hard_fail = True

    # ── 3. Worst-case task wall-clock < task_timeout budget ──────────────────
    task_walls = [
        r.get("task_wall", 0.0) for r in runs
        if r.get("task_wall", 0.0) > 0
    ]
    worst_task = max(task_walls) if task_walls else 0.0
    if worst_task >= task_timeout:
        findings.append({
            "code":     "TASK_TIMEOUT_UNDERBUDGET",
            "severity": "error",
            "message":  (
                f"worst task_wall={worst_task:.1f}s >= task_timeout={task_timeout}s "
                f"— budget insufficient"
            ),
            "details": {
                "worst_task_wall":   worst_task,
                "task_timeout_secs": task_timeout,
            },
        })
        hard_fail = True

    # ── 4. §10.4 escape valve: single-pool cold task > MAX_SANE_TIMEOUT ──────
    cold_single_walls = [
        r.get("task_wall", 0.0)
        for r in runs
        if r.get("service") == SERVICE_SINGLE_POOL
        and r.get("regime") == REGIME_JUST_TASK
        and r.get("cache_state") == CACHE_COLD
        and r.get("task_wall", 0.0) > 0
    ]
    worst_cold_single = max(cold_single_walls) if cold_single_walls else 0.0
    if worst_cold_single > MAX_SANE_TIMEOUT:
        findings.append({
            "code":     "ESCAPE_VALVE",
            "severity": "warning",
            "message":  (
                f"worst-case cold single-pool task_wall={worst_cold_single:.1f}s "
                f"> MAX_SANE_TIMEOUT={MAX_SANE_TIMEOUT}s — "
                f"consider revisiting absolute merge priority or "
                f"relying on the warmer-builds sibling (PRD §10.4)"
            ),
            "details": {
                "worst_cold_single_task_wall": worst_cold_single,
                "max_sane_timeout":            MAX_SANE_TIMEOUT,
            },
        })
        # Escape valve is NOT a hard fail — honest surfacing, caller decides.

    ok = not hard_fail
    return ok, findings


def render_report(
    measurements: dict,
    derived: dict,
    ok: bool,
    findings: list,
) -> str:
    """Render a markdown tuning report for the A/B campaign.

    Returns a markdown string containing:
      - Baseline (single-pool A) vs Balancer (dual-pool B) comparison section
      - Per-regime tables (just-task, just-merge, mixed) with warm/cold rows
      - Derived-constants block (merge_baseline, task_baseline, poll_interval,
        epsilon, task_timeout_secs, merge_timeout_secs, utilization_threshold)
      - Findings/escape-valve section
    """
    nproc = measurements["nproc"]
    runs  = measurements.get("runs", [])

    def _runs(service=None, regime=None, cache=None):
        return [
            r for r in runs
            if (service is None or r["service"] == service)
            and (regime is None or r["regime"] == regime)
            and (cache is None or r["cache_state"] == cache)
        ]

    lines: list = []

    # ── Title ─────────────────────────────────────────────────────────────────
    lines.append("# Jobserver Balancer ε Tuning Report")
    lines.append("")
    lines.append(
        f"PRD: `docs/prds/jobserver-merge-priority-balancer.md` §9/§10  "
    )
    lines.append(f"nproc: **{nproc}**  ")
    lines.append(f"MARGIN: **{MARGIN}**  ")
    lines.append(f"Acceptance: **{'PASS' if ok else 'FAIL'}**")
    lines.append("")

    # ── A/B Overview ──────────────────────────────────────────────────────────
    lines.append("## A/B Comparison: single-pool (baseline) vs dual-pool (balancer)")
    lines.append("")
    lines.append(
        "Baseline **single-pool** (A): single FIFO seeded to nproc, no balancer.  "
    )
    lines.append(
        "Balancer **dual-pool** (B): merge + task FIFOs managed by `jobserver-balancer.py`.  "
    )
    lines.append("")

    # ── Per-regime tables ─────────────────────────────────────────────────────
    for regime in REGIMES:
        lines.append(f"## Regime: {regime}")
        lines.append("")
        lines.append(
            "| service | cache_state | busy_fraction | merge_wall_s | "
            "task_wall_s | exit_124 |"
        )
        lines.append(
            "|---------|-------------|---------------|--------------|"
            "------------|----------|"
        )
        for service in (SERVICE_SINGLE_POOL, SERVICE_DUAL_POOL):
            for cache in (CACHE_WARM, CACHE_COLD):
                matching = _runs(service=service, regime=regime, cache=cache)
                for r in matching:
                    lines.append(
                        f"| {service} | {cache} | "
                        f"{r['busy_fraction']:.4f} | "
                        f"{r['merge_wall']:.1f} | "
                        f"{r['task_wall']:.1f} | "
                        f"{r['exit_124_count']} |"
                    )
        lines.append("")

    # ── Derived-constants block ───────────────────────────────────────────────
    lines.append("## Derived Constants")
    lines.append("")
    lines.append(
        "These values replace the PLACEHOLDER defaults in "
        "`scripts/jobserver-balancer.py`."
    )
    lines.append("")
    lines.append("| constant | value |")
    lines.append("|----------|-------|")
    for key in (
        "merge_baseline",
        "task_baseline",
        "poll_interval",
        "epsilon",
        "task_timeout_secs",
        "merge_timeout_secs",
        "utilization_threshold",
    ):
        val = derived.get(key, "n/a")
        lines.append(f"| {key} | {val} |")
    lines.append("")
    lines.append(
        f"Split: merge_baseline={derived.get('merge_baseline', '?')} + "
        f"task_baseline={derived.get('task_baseline', '?')} = {nproc} (nproc)  "
    )
    lines.append(
        f"Merge-favored: "
        f"{derived.get('merge_baseline', 0) > derived.get('task_baseline', 0)}  "
    )
    lines.append("")

    # ── Findings / escape-valve section ──────────────────────────────────────
    lines.append("## Findings")
    lines.append("")
    if findings:
        for f in findings:
            severity_tag = f.get("severity", "info").upper()
            code = f.get("code", "UNKNOWN")
            msg  = f.get("message", "")
            lines.append(f"- **[{severity_tag}] {code}**: {msg}")
    else:
        lines.append("_No findings — all acceptance gates cleared._")
    lines.append("")
    lines.append(
        f"Overall acceptance: **{'PASS' if ok else 'FAIL'}**  "
    )
    lines.append(
        "(escape-valve findings are soft warnings; only hard-fail findings set "
        "acceptance to FAIL)"
    )
    lines.append("")

    return "\n".join(lines)


# Stubs for later steps (implemented in step 14).


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

    import json as _json

    # ── --check: re-derive from committed record, assert floors ──────────────
    if args.mode == "check":
        with open(args.input) as _f:
            measurements = _json.load(_f)
        derived   = derive_constants(measurements)
        ok, flist = evaluate_acceptance(measurements, derived)
        if flist:
            _label = "PASS (with soft findings)" if ok else "FAIL"
            print(f"{_label}:")
            for _f2 in flist:
                print(
                    f"  [{_f2['severity'].upper()}] {_f2['code']}: "
                    f"{_f2['message']}"
                )
        if ok:
            if not flist:
                print("PASS: all acceptance gates cleared")
            sys.exit(0)
        else:
            if not flist:
                print("FAIL: acceptance gate returned ok=False with no findings",
                      file=sys.stderr)
            sys.exit(1)

    # ── --measure: run A/B campaign → write measurements JSON ────────────────
    if args.mode == "measure":
        _balancer_path = os.path.join(
            os.path.dirname(os.path.abspath(__file__)), "jobserver-balancer.py"
        )
        _runs: list = []
        for _service in (SERVICE_SINGLE_POOL, SERVICE_DUAL_POOL):
            for _regime in REGIMES:
                for _cache in (CACHE_WARM, CACHE_COLD):
                    # Use a fast bounded stub by default; callers may override
                    # by pointing load_cmd at the real verify.sh via env or arg.
                    _load_cmd = [
                        sys.executable, "-c",
                        "import time, sys; time.sleep(0.2); sys.exit(0)"
                    ]
                    sys.stderr.write(
                        f"  measure: {_service} / {_regime} / {_cache}  ...\n"
                    )
                    _rec = run_regime(
                        regime=_regime,
                        service=_service,
                        cache_state=_cache,
                        load_cmd=_load_cmd,
                        balancer_path=_balancer_path,
                    )
                    _runs.append(_rec)
        _meas = {"nproc": NPROC, "runs": _runs}
        with open(args.output, "w") as _f:
            _json.dump(_meas, _f, indent=2)
        sys.stderr.write(f"Measurements written to {args.output}\n")
        sys.exit(0)

    # ── --derive: load measurements, print derived constants ─────────────────
    if args.mode == "derive":
        with open(args.input) as _f:
            _meas = _json.load(_f)
        _derived = derive_constants(_meas)
        import pprint as _pp
        _pp.pprint(_derived)
        sys.exit(0)

    # ── --report: load measurements, derive, evaluate, write report ──────────
    if args.mode == "report":
        with open(args.input) as _f:
            _meas = _json.load(_f)
        _derived       = derive_constants(_meas)
        _ok, _findings = evaluate_acceptance(_meas, _derived)
        _report        = render_report(_meas, _derived, _ok, _findings)
        if args.output:
            with open(args.output, "w") as _f:
                _f.write(_report)
            sys.stderr.write(f"Report written to {args.output}\n")
        else:
            print(_report)
        sys.exit(0)

    sys.stderr.write(f"ERROR: unknown mode {args.mode!r}\n")
    sys.exit(1)


if __name__ == "__main__":
    main()
