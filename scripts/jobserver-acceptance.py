#!/usr/bin/env python3
"""
jobserver-acceptance.py — end-to-end mixed-load acceptance gate for the
dual-FIFO jobserver priority balancer (task η, PRD §9 leaf,
docs/prds/jobserver-merge-priority-balancer.md §9/§10).

Proves four user-observable signals on the DEPLOYED dual-pool host under a
REAL merge verify concurrent with N task verifies, under the STANDING
verify.sh outer-timeout budgets (debug 60m / release 75m):

  (a) box ≈ fully utilised (busy-core fraction ≈ nproc from /proc/stat)
  (b) merge wall-clock IMPROVED vs the single-pool baseline (same instrument)
  (c) NO task verify spuriously exits 124 under the standing budgets
  (d) under contention the merge pool reaches full token allocation
      (FIONREAD merge → nproc while contested)

CLI modes
---------
  --run      <out.json>          Capture REAL same-instrument A/B measurements
  --evaluate <in.json>           Evaluate gate; exit 0 = PASS, exit 1 = FAIL
  --report   <in.json> [-o out]  Render markdown acceptance report

Pure primitives imported from the sibling ε tuning harness
(scripts/jobserver-tuning-harness.py) via importlib spec_from_file_location
(Block-7 precedent in test_jobserver_balancer.sh:540-558).
"""

import argparse
import importlib.util
import os
import pathlib
import sys

# ──────────────────────────────────────────────────────────────────────────────
# Import ε primitives via importlib (hyphenated filename not importable by name)
# ──────────────────────────────────────────────────────────────────────────────

_HARNESS_PATH = pathlib.Path(__file__).parent / "jobserver-tuning-harness.py"

_spec = importlib.util.spec_from_file_location("jth", str(_HARNESS_PATH))
_harness = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_harness)  # runs module-level config only, not main()

# Re-export the primitives this module uses so callers can import them from here
# and tests can load this module without needing the harness on sys.path.
busy_fraction = _harness.busy_fraction
sample_pool_occupancy = _harness.sample_pool_occupancy
is_timeout = _harness.is_timeout
_provision_service = _harness._provision_service
_teardown_service = _harness._teardown_service


# ──────────────────────────────────────────────────────────────────────────────
# Pure analyzers (step-04)
# ──────────────────────────────────────────────────────────────────────────────


def merge_reached_full_allocation(series: list, nproc: int) -> bool:
    """Return True if any sample in series has merge == nproc.

    Scans the during-overlap FIONREAD occupancy time-series produced by
    run_mixed_concurrent's background sampler.  Each sample is a dict with at
    least a "merge" key holding the merge-pool token count.  Returns True when
    the merge pool reached full token allocation (== nproc) while under
    contention with the task pool — criterion (d) of the acceptance gate.

    Returns False for an empty series (no sample → cannot assert full
    allocation).
    """
    for sample in series:
        if sample.get("merge", 0) >= nproc:
            return True
    return False


def utilization_ok(busy_frac: float, threshold: float) -> bool:
    """Return True when busy_frac >= threshold.

    busy_frac is the busy-core fraction derived from ε's busy_fraction() over
    two /proc/stat snapshots straddling a mixed run.  threshold is the
    operator-configured or derived utilisation floor (criterion (a)).
    """
    return busy_frac >= threshold


# ──────────────────────────────────────────────────────────────────────────────
# Acceptance gate evaluator (step-06)
# ──────────────────────────────────────────────────────────────────────────────


def evaluate_acceptance_gate(
    measurements: dict,
) -> "tuple[bool, dict, list]":
    """Evaluate the four η acceptance criteria against A/B measurements.

    Parameters
    ----------
    measurements : dict
        Same-instrument A/B capture produced by --run.  Required keys:
          nproc                 int    — core count
          utilization_threshold float  — busy-core fraction floor for (a)
          baseline              dict   — single-pool mixed-run record
          dual_pool             dict   — dual-pool mixed-run record

        Each run record must carry:
          busy_fraction  float — busy-core fraction (from ε busy_fraction)
          merge_wall     float — merge verify wall-clock seconds
          exit_124_count int   — number of timeout exits (exit code 124)
          occupancy      list  — FIONREAD time-series from background sampler

    Returns
    -------
    (ok, verdicts, findings)
        ok       : bool — True only when ALL four criteria pass
        verdicts : dict — {"a": "PASS"|"FAIL", "b": ..., "c": ..., "d": ...}
        findings : list[str] — human-readable failure descriptions (empty on pass)
    """
    nproc = measurements["nproc"]
    threshold = measurements["utilization_threshold"]
    baseline = measurements["baseline"]
    dual = measurements["dual_pool"]

    verdicts: dict = {}
    findings: list = []

    # ── (a) box ≈ fully utilised ──────────────────────────────────────────
    busy_frac = dual["busy_fraction"]
    if utilization_ok(busy_frac, threshold):
        verdicts["a"] = "PASS"
    else:
        verdicts["a"] = "FAIL"
        findings.append(
            f"(a) utilization FAIL: dual-pool busy_fraction={busy_frac:.3f} "
            f"< threshold={threshold:.3f}"
        )

    # ── (b) merge wall-clock improved vs single-pool baseline ────────────
    dual_wall = dual["merge_wall"]
    base_wall = baseline["merge_wall"]
    if dual_wall < base_wall:
        verdicts["b"] = "PASS"
    else:
        verdicts["b"] = "FAIL"
        findings.append(
            f"(b) merge wall NOT improved: dual-pool={dual_wall:.1f}s "
            f">= baseline={base_wall:.1f}s"
        )

    # ── (c) no task verify spuriously exits 124 ──────────────────────────
    exit124 = dual["exit_124_count"]
    if exit124 == 0:
        verdicts["c"] = "PASS"
    else:
        verdicts["c"] = "FAIL"
        findings.append(
            f"(c) spurious timeout(s): exit_124_count={exit124} > 0.  "
            f"§10.4 escape-valve: task could not fit even at implicit-only "
            f"priority — candidate to revisit absolute priority allocation."
        )

    # ── (d) merge pool reached full allocation under contention ──────────
    series = dual["occupancy"]
    if merge_reached_full_allocation(series, nproc):
        verdicts["d"] = "PASS"
    else:
        verdicts["d"] = "FAIL"
        findings.append(
            f"(d) merge pool never reached full allocation (nproc={nproc}) "
            f"during contention; max merge observed="
            f"{max((s.get('merge', 0) for s in series), default=0)}"
        )

    ok = all(v == "PASS" for v in verdicts.values())
    return ok, verdicts, findings


# ──────────────────────────────────────────────────────────────────────────────
# Report renderer stub (implemented in steps 07-08)
# ──────────────────────────────────────────────────────────────────────────────


# def render_acceptance_report(measurements, verdicts): ...


# ──────────────────────────────────────────────────────────────────────────────
# Concurrent mixed-load driver stub (implemented in steps 09-10)
# ──────────────────────────────────────────────────────────────────────────────


# def run_mixed_concurrent(merge_cmd, task_cmds, fifos, sampler_interval): ...


# ──────────────────────────────────────────────────────────────────────────────
# CLI entry point — mode stubs (implemented in step-12)
# ──────────────────────────────────────────────────────────────────────────────


def main() -> None:
    """CLI entry point for the three acceptance-gate modes."""
    parser = argparse.ArgumentParser(
        description=(
            "jobserver-acceptance — end-to-end mixed-load acceptance gate "
            "for the dual-FIFO priority balancer (PRD §9 leaf η)."
        )
    )
    sub = parser.add_subparsers(dest="mode")

    p_run = sub.add_parser(
        "run",
        help="Capture REAL same-instrument A/B measurements and write JSON",
    )
    p_run.add_argument("output", help="Output measurements JSON path")

    p_evaluate = sub.add_parser(
        "evaluate",
        help="Evaluate acceptance gate from measurements JSON; exit 0 = PASS",
    )
    p_evaluate.add_argument("input", help="Input measurements JSON path")

    p_report = sub.add_parser(
        "report",
        help="Render markdown acceptance report from measurements JSON",
    )
    p_report.add_argument("input", help="Input measurements JSON path")
    p_report.add_argument(
        "-o", "--output", dest="output", default=None,
        help="Output report path (default: stdout)",
    )

    # Support both subcommand and '--flag' forms, e.g. '--run out.json'.
    argv = sys.argv[1:]
    if argv and argv[0] in ("--run", "--evaluate", "--report"):
        argv[0] = argv[0].lstrip("-")

    args = parser.parse_args(argv)

    if args.mode is None:
        parser.print_help()
        sys.exit(0)

    # Stubs — each mode will be wired in step-12.
    sys.stderr.write(
        f"ERROR: --{args.mode} mode not yet implemented\n"
        f"  (will be wired in step-12 of task η/4521)\n"
    )
    sys.exit(2)


if __name__ == "__main__":
    main()
