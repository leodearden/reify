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
# Report renderer (step-08)
# ──────────────────────────────────────────────────────────────────────────────


def render_acceptance_report(measurements: dict, verdicts: dict) -> str:
    """Render a markdown acceptance report from A/B measurements and verdicts.

    Returns a markdown string containing:
      - Title + PRD/leaf-η citation
      - A/B comparison section (single-pool baseline vs dual-pool, same instrument)
      - Per-run table: service | regime | cache_state | busy_fraction |
                       merge_wall_s | slowest_task_wall_s | exit_124
      - Four-criteria verdict block: (a)–(d) with PASS/FAIL
      - ζ′/4520 budget floor section: merge wall + slowest task wall as the
        authoritative floor for timeout re-derivation

    This is the ζ′/4520 floor document.  Numbers here must be REAL
    verify.sh walls, never synthetic stubs.
    """
    nproc = measurements["nproc"]
    threshold = measurements["utilization_threshold"]
    baseline = measurements["baseline"]
    dual = measurements["dual_pool"]

    def _slowest(run: dict) -> float:
        walls = run.get("task_walls", [])
        return max(walls) if walls else 0.0

    lines: list = []

    # ── Title ─────────────────────────────────────────────────────────────────
    lines.append("# Jobserver Balancer η Acceptance Report")
    lines.append("")
    lines.append(
        "PRD: `docs/prds/jobserver-merge-priority-balancer.md` §9 leaf η  "
    )
    lines.append(f"nproc: **{nproc}**  ")
    lines.append(f"utilization_threshold: **{threshold}**  ")
    overall = "PASS" if all(v == "PASS" for v in verdicts.values()) else "FAIL"
    lines.append(f"Overall: **{overall}**")
    lines.append("")

    # ── A/B Comparison section ─────────────────────────────────────────────
    lines.append("## A/B Comparison: single-pool baseline vs dual-pool (same instrument)")
    lines.append("")
    lines.append(
        "**Baseline (A):** single-pool service, one merge + N task verifies "
        "concurrent under the standing 60m/75m verify.sh budgets.  "
    )
    lines.append(
        "**Dual-pool (B):** same concurrent load on the deployed "
        "`jobserver-balancer.py` dual-pool service.  "
    )
    lines.append(
        "esc-4520-22 (disposition D): numbers here are REAL verify.sh "
        "walls — NOT ε's synthetic ~2s CPU-burn stub."
    )
    lines.append("")

    # ── Per-run table ──────────────────────────────────────────────────────
    lines.append("## Per-Run Measurements")
    lines.append("")
    lines.append(
        "| service | regime | cache_state | busy_fraction | "
        "merge_wall_s | slowest_task_wall_s | exit_124 |"
    )
    lines.append(
        "|---------|--------|-------------|---------------|"
        "-------------|---------------------|----------|"
    )
    for run in (baseline, dual):
        svc = run.get("service", "—")
        regime = run.get("regime", "mixed")
        cache = run.get("cache_state", "—")
        bf = run.get("busy_fraction", 0.0)
        mw = run.get("merge_wall", 0.0)
        st = _slowest(run)
        e124 = run.get("exit_124_count", 0)
        lines.append(
            f"| {svc} | {regime} | {cache} | {bf:.3f} | {mw:.1f} | {st:.1f} | {e124} |"
        )
    lines.append("")

    # ── Verdict block ──────────────────────────────────────────────────────
    lines.append("## Acceptance Criteria Verdicts")
    lines.append("")
    dual_bf = dual.get("busy_fraction", 0.0)
    dual_mw = dual.get("merge_wall", 0.0)
    base_mw = baseline.get("merge_wall", 0.0)
    e124 = dual.get("exit_124_count", 0)
    lines.append(
        f"| criterion | description | value | verdict |"
    )
    lines.append(
        f"|-----------|-------------|-------|---------|"
    )
    lines.append(
        f"| (a) | box utilisation ≈ nproc (dual-pool mixed run) "
        f"| busy_fraction={dual_bf:.3f} >= {threshold} "
        f"| **{verdicts.get('a', '—')}** |"
    )
    lines.append(
        f"| (b) | merge wall-clock improved vs baseline "
        f"| dual={dual_mw:.1f}s < baseline={base_mw:.1f}s "
        f"| **{verdicts.get('b', '—')}** |"
    )
    lines.append(
        f"| (c) | no task verify exits 124 under standing budgets "
        f"| exit_124_count={e124} "
        f"| **{verdicts.get('c', '—')}** |"
    )
    series = dual.get("occupancy", [])
    max_merge = max((s.get("merge", 0) for s in series), default=0)
    lines.append(
        f"| (d) | merge pool reached full allocation under contention "
        f"| max_merge={max_merge} vs nproc={nproc} "
        f"| **{verdicts.get('d', '—')}** |"
    )
    lines.append("")

    # ── ζ′/4520 budget floor section ──────────────────────────────────────
    lines.append("## ζ′/4520 Budget Floor")
    lines.append("")
    lines.append(
        "This section records the authoritative timing floor that "
        "task ζ′/4520 consumes to re-derive the verify.sh outer-timeout "
        "budgets.  Numbers are REAL verify.sh walls from the dual-pool mixed "
        "run (baseline numbers provided for same-instrument A/B context)."
    )
    lines.append("")
    lines.append("| run | merge_wall_s | slowest_task_wall_s | cache_state |")
    lines.append("|-----|-------------|---------------------|-------------|")
    lines.append(
        f"| baseline (single-pool) | {base_mw:.1f} | {_slowest(baseline):.1f} "
        f"| {baseline.get('cache_state', '—')} |"
    )
    lines.append(
        f"| dual-pool (accepted) | {dual_mw:.1f} | {_slowest(dual):.1f} "
        f"| {dual.get('cache_state', '—')} |"
    )
    lines.append("")
    lines.append(
        f"**ζ′ floor (dual-pool):** merge_wall={dual_mw:.1f}s, "
        f"slowest_task_wall={_slowest(dual):.1f}s"
    )
    lines.append("")

    return "\n".join(lines)


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
