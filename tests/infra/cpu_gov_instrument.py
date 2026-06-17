#!/usr/bin/env python3
"""
cpu_gov_instrument.py — measurement instrument for the cpu-load-governance
integration-gate harness (task 4634, §8 rows 1-4).

Reuses the busy_fraction / _read_proc_stat / NPROC primitives from the
tasks-4519/4521 sibling instrument (scripts/jobserver-tuning-harness.py)
via importlib.util.spec_from_file_location — the Block-7 precedent established
in scripts/jobserver-acceptance.py:38-50 (hyphenated filename not importable
by name; module-level config runs on exec_module, no side effects).

Adds non-destructive "FIONREAD-style" samplers for the cpu-governance substrate:
  read_psi_avg10(proc_path)  — parse avg10 from /proc/pressure/cpu-formatted file
  read_cgroup_cpu_usage(path)— read usage_usec from cgroup cpu.stat

Pure analyzers (all hermetic, no I/O, always-on tests):
  fair_share_floor(active, cores)              → active / cores
  slowdown_within_bound(s, floor, k)           → floor <= s <= k*floor AND s < 10
  share_ge_proportional(merge, task, w_m, w_t, tol) → merge/(merge+task) >= w_m/(w_m+w_t) - tol
  psi_below_band(avg10, thresh)                → avg10 < thresh

CLI subcommands (used by the bash harness):
  selftest                      run all synthetic-fixture self-tests, exit 0/1
  busy-fraction <before> <after>  print "fraction busy_cores" from /proc/stat files
  psi-avg10 [proc_path]         print avg10 float or "unavailable"
  cgroup-usage <path>           print usage_usec integer from cpu.stat
  fair-share <active> <cores>   print fair_share_floor float
"""

import importlib.util
import os
import pathlib
import sys

# ──────────────────────────────────────────────────────────────────────────────
# Reuse ε primitives from scripts/jobserver-tuning-harness.py via importlib
# (Block-7 precedent; runs module-level config only — no heavy side effects).
# ──────────────────────────────────────────────────────────────────────────────

_SELF = pathlib.Path(__file__).resolve()
_REPO_ROOT = _SELF.parent.parent.parent  # tests/infra/../../.. → repo root
_HARNESS_PATH = _REPO_ROOT / "scripts" / "jobserver-tuning-harness.py"

_spec = importlib.util.spec_from_file_location("jth", str(_HARNESS_PATH))
_harness = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_harness)

# Re-export the primitives so callers can do:
#   from cpu_gov_instrument import busy_fraction, _read_proc_stat, NPROC
busy_fraction = _harness.busy_fraction
_read_proc_stat = _harness._read_proc_stat
NPROC = _harness.NPROC


# ──────────────────────────────────────────────────────────────────────────────
# Non-destructive samplers
# ──────────────────────────────────────────────────────────────────────────────


def read_psi_avg10(proc_path: str = "/proc/pressure/cpu") -> float | None:
    """Parse avg10 from a /proc/pressure/cpu-formatted file.

    Mirrors the awk parser in scripts/cpu-admit.sh:62-68:
        /^some/ { for each field: if starts with 'avg10=' strip prefix, print, exit }

    Returns the avg10 float on success, or None on any read/parse failure
    (fail-open mirrors cpu-admit.sh C-A4 behaviour).
    """
    try:
        with open(proc_path, "r") as f:
            for line in f:
                if line.startswith("some "):
                    for field in line.split():
                        if field.startswith("avg10="):
                            return float(field[len("avg10="):])
    except (OSError, ValueError):
        pass
    return None


def read_cgroup_cpu_usage(cgroup_path: str) -> int | None:
    """Read usage_usec from a cgroup-v2 cpu.stat file.

    Accepts either:
      - an absolute path to a cpu.stat file, or
      - a cgroup relative path (e.g. "/user.slice/user-1000.slice/...") which
        is resolved to /sys/fs/cgroup<rel>/cpu.stat.

    Returns the usage_usec integer on success, or None on failure.
    """
    # Resolve path: if the given path is a cgroup relative (starts with '/' but
    # does not contain 'cpu.stat'), prefix /sys/fs/cgroup and append cpu.stat.
    path = cgroup_path
    if not path.endswith("cpu.stat"):
        # Strip a leading '0::' prefix (from /proc/self/cgroup format).
        if path.startswith("0::"):
            path = path[3:]
        path = "/sys/fs/cgroup" + path.rstrip("/") + "/cpu.stat"

    try:
        with open(path, "r") as f:
            for line in f:
                parts = line.split()
                if len(parts) == 2 and parts[0] == "usage_usec":
                    return int(parts[1])
    except (OSError, ValueError):
        pass
    return None


# ──────────────────────────────────────────────────────────────────────────────
# Pure analyzers — hermetic, no I/O, always-on in selftest
# ──────────────────────────────────────────────────────────────────────────────


def fair_share_floor(active: float, cores: float) -> float:
    """Compute the unavoidable slowdown floor under contention.

    Under perfect work-conserving scheduling with *active* equal-weight tasks
    sharing *cores* CPUs, each task runs at cores/active speed → slowdown ≥
    active/cores.  This is the physically unavoidable lower bound; an assertion
    should check slowdown ≥ this floor (not ≥ 0) to be honest about contention.

    Returns active / cores (or 0.0 if cores <= 0).
    """
    if cores <= 0:
        return 0.0
    return active / cores


def slowdown_within_bound(
    slowdown: float,
    floor: float,
    k: float,
) -> bool:
    """Return True if slowdown is within the acceptable band.

    Acceptable band: [floor, k * floor] AND slowdown < 10.

    The 10× ceiling is the "4415 cannot recur" line — any slowdown ≥ 10×
    indicates the oversubscription incident class has recurred.  The upper
    bound K * floor is a generous tolerance above fair share; the lower bound
    floor is the fair-share minimum (physically unavoidable under contention).

    Synthetic-fixture assertions (from step-1 spec):
      (1.6, 1.5, 4) → True   (1.07× fair share, within 4× band, < 10)
      (2.0, 1.5, 4) → True   (1.33× fair share, boundary at upper=6.0)
      (12.0, 1.5, 4) → False (≥ 10 — 4415-class oversubscription)
      (0.5, 1.5, 4) → False  (< floor — physically impossible, instrument error)
    """
    if slowdown < floor:
        return False
    if slowdown >= 10.0:
        return False
    if slowdown > k * floor:
        return False
    return True


def share_ge_proportional(
    merge_usage: float,
    task_usage: float,
    w_merge: float,
    w_task: float,
    tol: float = 0.05,
) -> bool:
    """Return True if the merge slice received at least its proportional share.

    Proportional floor = w_merge / (w_merge + w_task).
    Observed share = merge_usage / (merge_usage + task_usage).

    Returns True iff observed_share >= floor - tol.

    Synthetic-fixture assertions (from step-1 spec):
      (75, 25, 300, 100, 0.05) → True   (0.75 >= 0.75 - 0.05 = 0.70)
      (60, 40, 300, 100, 0.05) → False  (0.60 < 0.70)
    """
    total = merge_usage + task_usage
    if total <= 0:
        return False
    observed = merge_usage / total
    proportional_floor = w_merge / (w_merge + w_task)
    return observed >= (proportional_floor - tol)


def psi_below_band(avg10: float, thresh: float) -> bool:
    """Return True if the PSI avg10 value is below the admission threshold.

    Mirrors the admission check in scripts/cpu-admit.sh C-A1:
      avg10 < threshold → admit immediately.

    Synthetic-fixture assertions (from step-1 spec):
      (40, 50) → True   (40 < 50)
      (60, 50) → False  (60 >= 50)
    """
    return avg10 < thresh


# ──────────────────────────────────────────────────────────────────────────────
# Selftest — hermetic synthetic-fixture assertions
# ──────────────────────────────────────────────────────────────────────────────


def _selftest() -> bool:
    """Run all pure-analyzer assertions with synthetic fixtures.

    Returns True if all pass, False if any fail.  Prints PASS/FAIL per check
    to stdout so the caller can see exactly which assertion broke.
    """
    passed = 0
    failed = 0

    def check(desc: str, result: bool) -> None:
        nonlocal passed, failed
        if result:
            print(f"  PASS: {desc}")
            passed += 1
        else:
            print(f"  FAIL: {desc}")
            failed += 1

    # --- Import-reuse contract ---
    check(
        "importlib: busy_fraction is callable",
        callable(busy_fraction),
    )
    check(
        "importlib: _read_proc_stat is callable",
        callable(_read_proc_stat),
    )
    check(
        "importlib: NPROC is a positive int",
        isinstance(NPROC, int) and NPROC >= 1,
    )

    # --- busy_fraction with synthetic /proc/stat lines ---
    # Idle snapshot: all time in idle (field 3).
    stat_idle = "cpu  0 0 0 1000 0 0 0 0 0 0"
    # After 500 busy ticks (user) + 500 idle ticks.
    stat_after_half = "cpu  500 0 0 1500 0 0 0 0 0 0"
    frac, cores = busy_fraction(stat_idle, stat_after_half, 8)
    check(
        "busy_fraction: 50% busy → fraction ≈ 0.5",
        abs(frac - 0.5) < 0.01,
    )
    check(
        "busy_fraction: 50% busy, 8 cores → busy_cores ≈ 4.0",
        abs(cores - 4.0) < 0.01,
    )
    # Identical snapshots → fraction 0.0.
    frac0, cores0 = busy_fraction(stat_idle, stat_idle, 8)
    check(
        "busy_fraction: identical snapshots → fraction 0.0",
        frac0 == 0.0 and cores0 == 0.0,
    )

    # --- fair_share_floor ---
    check(
        "fair_share_floor(48, 32) = 1.5",
        abs(fair_share_floor(48, 32) - 1.5) < 1e-9,
    )
    check(
        "fair_share_floor(32, 32) = 1.0",
        abs(fair_share_floor(32, 32) - 1.0) < 1e-9,
    )
    check(
        "fair_share_floor(16, 32) = 0.5",
        abs(fair_share_floor(16, 32) - 0.5) < 1e-9,
    )
    check(
        "fair_share_floor(active, 0) = 0.0 (degenerate guard)",
        fair_share_floor(16, 0) == 0.0,
    )

    # --- slowdown_within_bound ---
    # (slowdown, floor, k) spec table from step-1:
    check(
        "slowdown_within_bound(1.6, 1.5, 4) → True (1.07× floor, within band)",
        slowdown_within_bound(1.6, 1.5, 4) is True,
    )
    check(
        "slowdown_within_bound(2.0, 1.5, 4) → True (boundary check, upper=6.0)",
        slowdown_within_bound(2.0, 1.5, 4) is True,
    )
    check(
        "slowdown_within_bound(12.0, 1.5, 4) → False (≥ 10 — 4415 line)",
        slowdown_within_bound(12.0, 1.5, 4) is False,
    )
    check(
        "slowdown_within_bound(0.5, 1.5, 4) → False (below floor — impossible)",
        slowdown_within_bound(0.5, 1.5, 4) is False,
    )
    # Additional: above k*floor but below 10.
    check(
        "slowdown_within_bound(7.0, 1.5, 4) → False (7.0 > 4×1.5=6.0)",
        slowdown_within_bound(7.0, 1.5, 4) is False,
    )
    # At exactly the k*floor boundary.
    check(
        "slowdown_within_bound(6.0, 1.5, 4) → True (exactly k*floor)",
        slowdown_within_bound(6.0, 1.5, 4) is True,
    )

    # --- share_ge_proportional ---
    check(
        "share_ge_proportional(75, 25, 300, 100, 0.05) → True (0.75 ≥ 0.70)",
        share_ge_proportional(75, 25, 300, 100, 0.05) is True,
    )
    check(
        "share_ge_proportional(60, 40, 300, 100, 0.05) → False (0.60 < 0.70)",
        share_ge_proportional(60, 40, 300, 100, 0.05) is False,
    )
    check(
        "share_ge_proportional(70, 30, 300, 100, 0.05) → True (exactly at floor−tol+?)",
        # observed=0.70, floor=0.75, tol=0.05 → 0.70 >= 0.70 → True
        share_ge_proportional(70, 30, 300, 100, 0.05) is True,
    )
    check(
        "share_ge_proportional: zero total → False",
        share_ge_proportional(0, 0, 300, 100, 0.05) is False,
    )

    # --- psi_below_band ---
    check(
        "psi_below_band(40, 50) → True",
        psi_below_band(40.0, 50.0) is True,
    )
    check(
        "psi_below_band(60, 50) → False",
        psi_below_band(60.0, 50.0) is False,
    )
    check(
        "psi_below_band(50, 50) → False (not strictly below)",
        psi_below_band(50.0, 50.0) is False,
    )
    check(
        "psi_below_band(0, 50) → True",
        psi_below_band(0.0, 50.0) is True,
    )

    print()
    print(f"selftest: {passed} passed, {failed} failed")
    return failed == 0


# ──────────────────────────────────────────────────────────────────────────────
# CLI dispatch
# ──────────────────────────────────────────────────────────────────────────────


def _usage() -> None:
    print(
        "Usage: cpu_gov_instrument.py <subcommand> [args...]\n"
        "\n"
        "Subcommands:\n"
        "  selftest                   run pure-analyzer self-tests, exit 0/1\n"
        "  busy-fraction <bf> <af>    print 'fraction busy_cores' from /proc/stat files\n"
        "  psi-avg10 [proc_path]      print avg10 float or 'unavailable'\n"
        "  cgroup-usage <path>        print usage_usec from cpu.stat (cgroup rel or abs)\n"
        "  fair-share <active> <cores>  print fair_share_floor float\n",
        file=sys.stderr,
    )
    sys.exit(64)


def main(argv: list[str]) -> int:
    if not argv:
        _usage()

    cmd = argv[0]
    args = argv[1:]

    if cmd == "selftest":
        ok = _selftest()
        return 0 if ok else 1

    elif cmd == "busy-fraction":
        if len(args) != 2:
            print("busy-fraction: need <before_file> <after_file>", file=sys.stderr)
            return 64
        before_file, after_file = args
        try:
            before_line = ""
            with open(before_file) as f:
                for line in f:
                    if line.startswith("cpu "):
                        before_line = line.strip()
                        break
            after_line = ""
            with open(after_file) as f:
                for line in f:
                    if line.startswith("cpu "):
                        after_line = line.strip()
                        break
        except OSError as e:
            print(f"busy-fraction: {e}", file=sys.stderr)
            return 1
        frac, cores = busy_fraction(before_line, after_line, NPROC)
        print(f"{frac} {cores}")
        return 0

    elif cmd == "psi-avg10":
        proc_path = args[0] if args else "/proc/pressure/cpu"
        val = read_psi_avg10(proc_path)
        if val is None:
            print("unavailable")
        else:
            print(val)
        return 0

    elif cmd == "cgroup-usage":
        if len(args) != 1:
            print("cgroup-usage: need <path>", file=sys.stderr)
            return 64
        val = read_cgroup_cpu_usage(args[0])
        if val is None:
            print("unavailable")
        else:
            print(val)
        return 0

    elif cmd == "fair-share":
        if len(args) != 2:
            print("fair-share: need <active> <cores>", file=sys.stderr)
            return 64
        try:
            active = float(args[0])
            cores = float(args[1])
        except ValueError as e:
            print(f"fair-share: {e}", file=sys.stderr)
            return 64
        result = fair_share_floor(active, cores)
        # Print with enough precision but strip trailing zeros.
        print(f"{result:g}")
        return 0

    else:
        print(f"unknown subcommand: {cmd!r}", file=sys.stderr)
        _usage()
        return 64


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
