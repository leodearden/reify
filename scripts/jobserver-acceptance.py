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
# Concurrent mixed-load driver (step-10)
# ──────────────────────────────────────────────────────────────────────────────

import subprocess
import threading
import time


def run_mixed_concurrent(
    merge_cmd: list,
    task_cmds: list,
    fifos: dict,
    sampler_interval: float = 0.5,
    service: str = "dual-pool",
    cache_state: str = "warm",
) -> dict:
    """Run one merge command and N task commands truly concurrently.

    Starts all N+1 processes simultaneously via subprocess.Popen, launches a
    background sampler thread that calls ε's sample_pool_occupancy every
    *sampler_interval* seconds during the overlap, then waits for all
    processes to finish.

    This is the η concurrent driver — NOT ε's run_regime MIXED which runs
    merge then tasks SEQUENTIALLY (jobserver-tuning-harness.py:548-556).
    Genuine concurrency is required so signal (a) full-utilisation and
    signal (d) merge-FIONREAD→nproc-WHILE-CONTESTED can be observed.

    Parameters
    ----------
    merge_cmd       : list[str] — command for the merge process
    task_cmds       : list[list[str]] — commands for N task processes
    fifos           : dict — {"merge_fifo": str, "task_fifo": str}
    sampler_interval: float — seconds between FIONREAD samples (default 0.5s)
    service         : str — label for the returned record ("dual-pool" or
                            "single-pool" for the baseline run)
    cache_state     : str — "warm" or "cold" (operator-supplied)

    Returns
    -------
    dict with:
        service          : str
        regime           : "mixed"
        cache_state      : str
        merge_wall       : float  — merge process wall-clock seconds
        task_walls       : list[float]
        exit_codes       : list[int]  — [merge_exit, task0_exit, ...]
        exit_124_count   : int
        occupancy        : list[dict] — FIONREAD time-series (merge/task/timestamp)
        busy_fraction    : float  — from ε busy_fraction over /proc/stat snapshots
    """
    merge_fifo = fifos["merge_fifo"]
    task_fifo = fifos["task_fifo"]

    # /proc/stat snapshot before launch (for criterion-a busy_fraction)
    stat_before = _harness._read_proc_stat()

    occupancy: list = []
    _stop_sampler = threading.Event()

    def _sampler():
        while not _stop_sampler.is_set():
            try:
                sample = sample_pool_occupancy(merge_fifo, task_fifo)
                # sample_pool_occupancy returns {"merge", "task", "sum", "timestamp"}
                # Rename "timestamp" to "ts" for consistency with fixture shape.
                occupancy.append({
                    "merge": sample["merge"],
                    "task": sample["task"],
                    "timestamp": sample["timestamp"],
                })
            except OSError:
                pass  # FIFO may not yet be open — skip sample
            _stop_sampler.wait(sampler_interval)

    sampler_thread = threading.Thread(target=_sampler, daemon=True)

    # Launch all processes simultaneously
    t_merge_start = time.monotonic()
    merge_proc = subprocess.Popen(merge_cmd)
    t_tasks_start = [time.monotonic() for _ in task_cmds]
    task_procs = [subprocess.Popen(cmd) for cmd in task_cmds]

    # Start the background sampler once all processes are Popen'd
    sampler_thread.start()

    # Wait for merge
    merge_proc.wait()
    merge_wall = time.monotonic() - t_merge_start

    # Wait for all tasks
    task_walls: list = []
    for i, proc in enumerate(task_procs):
        proc.wait()
        task_walls.append(time.monotonic() - t_tasks_start[i])

    # Stop the sampler
    _stop_sampler.set()
    sampler_thread.join(timeout=sampler_interval * 2 + 1.0)

    # /proc/stat snapshot after — for criterion-a busy_fraction
    stat_after = _harness._read_proc_stat()
    nproc_env = _harness.NPROC
    bf_frac, _bf_cores = busy_fraction(stat_before, stat_after, nproc_env)

    exit_codes = [merge_proc.returncode] + [p.returncode for p in task_procs]
    exit_124_count = sum(1 for rc in exit_codes if is_timeout(rc))

    return {
        "service": service,
        "regime": "mixed",
        "cache_state": cache_state,
        "merge_wall": float(merge_wall),
        "task_walls": [float(w) for w in task_walls],
        "exit_codes": exit_codes,
        "exit_124_count": exit_124_count,
        "occupancy": occupancy,
        "busy_fraction": float(bf_frac),
    }


# ──────────────────────────────────────────────────────────────────────────────
# Real A/B campaign driver (step-12 — wired in main()'s --run dispatch)
# ──────────────────────────────────────────────────────────────────────────────

import json as _json_module


# Production-faithful verify.sh invocations (actions + scope):
#   merge — `all --scope all`: matches the hooks/pre-merge-commit gate
#           (profile=both comes from DF_VERIFY_ROLE=merge, verify.sh role default).
#   task  — `test --scope all --include-infra`: orchestrator.yaml's task
#           test_command uses --scope branch, but the η branch touches no Rust,
#           so a branch-scope verify here would be near-empty and useless as
#           the ζ′ slowest-task-wall floor.  --scope all is the conservative
#           heavy-task instrument (≡ a task whose branch touched core crates).
MERGE_VERIFY_ARGS = ["all", "--scope", "all"]
TASK_VERIFY_ARGS = ["test", "--scope", "all", "--include-infra"]


def make_verify_cmd(
    role: str,
    action_args: list,
    timeout_min: int,
    merge_fifo: str,
    task_fifo: str,
    verify_sh: str,
) -> list:
    """Build one real verify.sh campaign command.

    Two things MUST be routed through the environment for the measurement to
    be valid:
      - DF_VERIFY_ROLE — selects profile default + psi/semaphore exemptions.
      - REIFY_JOBSERVER_{MERGE,TASK}_FIFO — verify.sh selects its jobserver
        FIFO from these (role→var, default = the LIVE /tmp dual-pool FIFOs).
        Without them the single-pool baseline run would silently draw from
        the live dual-pool service, turning the A/B into dual-vs-dual.

    The standing outer-timeout budget (debug 60m / release 75m) is applied
    via `timeout --kill-after=60 <timeout_min>m` so exit-124 is observable
    for criterion (c).
    """
    return [
        "env",
        f"DF_VERIFY_ROLE={role}",
        f"REIFY_JOBSERVER_MERGE_FIFO={merge_fifo}",
        f"REIFY_JOBSERVER_TASK_FIFO={task_fifo}",
        "timeout",
        "--kill-after=60",
        f"{timeout_min}m",
        "bash",
        verify_sh,
    ] + list(action_args)


def _run_campaign(
    output_path: str,
    ntasks: int = 1,
    cache_state: str = "warm",
    utilization_threshold: float = 0.85,
    sampler_interval: float = 5.0,
    task_repo: str = None,
) -> None:
    """Capture REAL same-instrument A/B measurements and write JSON.

    Runs two concurrent mixed-load sessions back-to-back:
      A (baseline): single-pool service provisioned via ε _provision_service;
                    no balancer in the path; merge and tasks share one FIFO.
      B (dual-pool): the DEPLOYED reify-jobserver.service (merge-pool +
                     task-pool managed by jobserver-balancer.py).

    Each session:  1 merge verify (DF_VERIFY_ROLE=merge, 75m budget) +
                   ntasks task verifies (DF_VERIFY_ROLE=task, 60m budget),
                   all run CONCURRENTLY via run_mixed_concurrent.

    Records sccache cache_state (operator-supplied), busy_fraction, per-process
    walls, exit-124 counts, and the FIONREAD occupancy time-series.

    Writes the measurements dict as JSON to output_path for --evaluate/--report.

    SAFETY NOTE: This function runs real verify.sh builds.  Run ONLY on the
    deployed host with the dual-pool reify-jobserver.service active.  The
    single-pool baseline is self-provisioned in a private tmpdir and torn down
    automatically; it does NOT touch the live service.

    task_repo is REQUIRED when ntasks > 0: the task verifies must run from a
    SEPARATE checkout.  Merge and task verify.sh both cd to their own
    REPO_ROOT; sharing one checkout means sharing one cargo target/ — the
    concurrent builds would serialize on cargo's build-dir lock and criteria
    (a)/(d) would measure lock-waiting, not jobserver contention.
    """
    repo_root = pathlib.Path(__file__).parent.parent.resolve()
    verify_sh = str(repo_root / "scripts" / "verify.sh")
    balancer_path = str(pathlib.Path(__file__).parent / "jobserver-balancer.py")

    if ntasks > 0:
        if not task_repo:
            raise RuntimeError(
                "--task-repo is required when ntasks > 0: task verifies must "
                "run from a separate checkout (own cargo target/), or the "
                "merge and task builds serialize on cargo's build-dir lock "
                "and the contention measurement is invalid."
            )
        task_verify_sh = str(
            pathlib.Path(task_repo).resolve() / "scripts" / "verify.sh"
        )
        if not os.path.exists(task_verify_sh):
            raise RuntimeError(f"--task-repo has no verify.sh: {task_verify_sh}")
    else:
        task_verify_sh = verify_sh

    nproc = _harness.NPROC

    def _cmds_for(merge_fifo: str, task_fifo: str) -> "tuple[list, list]":
        """Merge + task command lists with the run's FIFOs routed in."""
        merge_cmd = make_verify_cmd(
            "merge", MERGE_VERIFY_ARGS, 75, merge_fifo, task_fifo, verify_sh
        )
        task_cmds = [
            make_verify_cmd(
                "task", TASK_VERIFY_ARGS, 60, merge_fifo, task_fifo,
                task_verify_sh,
            )
            for _ in range(ntasks)
        ]
        return merge_cmd, task_cmds

    sys.stderr.write(
        f"[η --run] Starting A/B campaign: {ntasks} task(s), "
        f"cache={cache_state}, threshold={utilization_threshold}\n"
    )

    # ── Run A: single-pool baseline ────────────────────────────────────────
    sys.stderr.write("[η --run] Provisioning single-pool baseline…\n")
    infra_a = _provision_service(
        _harness.SERVICE_SINGLE_POOL,
        nproc,
        balancer_path,
    )
    fifos_a = {
        "merge_fifo": infra_a["merge_fifo"],
        "task_fifo": infra_a["task_fifo"],
    }
    try:
        sys.stderr.write("[η --run] Running baseline mixed concurrent…\n")
        # Pre-balancer single-pool world: ONE shared FIFO that every verify
        # (merge and task role alike) draws from — both role vars point at
        # the seeded single FIFO.  The dummy task FIFO exists only so the
        # sampler has a second path to watch (stays 0).
        single = infra_a["merge_fifo"]
        merge_cmd_a, task_cmds_a = _cmds_for(single, single)
        baseline_rec = run_mixed_concurrent(
            merge_cmd_a, task_cmds_a, fifos_a,
            sampler_interval=sampler_interval,
            service=_harness.SERVICE_SINGLE_POOL,
            cache_state=cache_state,
        )
    finally:
        _teardown_service(infra_a)
        sys.stderr.write("[η --run] Baseline service torn down.\n")

    # ── Run B: deployed dual-pool service ──────────────────────────────────
    merge_fifo_live = _harness.MERGE_FIFO
    task_fifo_live = _harness.TASK_FIFO
    if not os.path.exists(merge_fifo_live) or not os.path.exists(task_fifo_live):
        raise RuntimeError(
            f"Live dual-pool FIFOs not found: "
            f"{merge_fifo_live!r}, {task_fifo_live!r}.  "
            f"Is reify-jobserver.service running?  "
            f"Run: systemctl --user status reify-jobserver.service"
        )
    fifos_b = {"merge_fifo": merge_fifo_live, "task_fifo": task_fifo_live}
    sys.stderr.write("[η --run] Running dual-pool mixed concurrent…\n")
    merge_cmd_b, task_cmds_b = _cmds_for(merge_fifo_live, task_fifo_live)
    dualpool_rec = run_mixed_concurrent(
        merge_cmd_b, task_cmds_b, fifos_b,
        sampler_interval=sampler_interval,
        service=_harness.SERVICE_DUAL_POOL,
        cache_state=cache_state,
    )

    # ── Write measurements JSON ────────────────────────────────────────────
    measurements = {
        "nproc": nproc,
        "utilization_threshold": utilization_threshold,
        "baseline": baseline_rec,
        "dual_pool": dualpool_rec,
    }
    with open(output_path, "w") as _f:
        _json_module.dump(measurements, _f, indent=2)
    sys.stderr.write(f"[η --run] Measurements written to {output_path}\n")


# ──────────────────────────────────────────────────────────────────────────────
# CLI entry point (step-12)
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
    p_run.add_argument(
        "--ntasks", type=int, default=1, metavar="N",
        help="Number of concurrent task verifies (default: 1)",
    )
    p_run.add_argument(
        "--cache-state", choices=["warm", "cold"], default="warm",
        dest="cache_state",
        help="sccache state for this run (default: warm)",
    )
    p_run.add_argument(
        "--utilization-threshold", type=float, default=0.85,
        dest="utilization_threshold",
        help="Busy-core fraction floor for criterion (a) (default: 0.85)",
    )
    p_run.add_argument(
        "--sampler-interval", type=float, default=5.0,
        dest="sampler_interval",
        help="Seconds between FIONREAD samples (default: 5.0)",
    )
    p_run.add_argument(
        "--task-repo", dest="task_repo", default=None,
        help=(
            "Checkout the task verifies run from (REQUIRED when --ntasks > 0; "
            "must be a separate checkout with its own cargo target/, or the "
            "concurrent builds serialize on cargo's build-dir lock)"
        ),
    )

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

    import json as _json

    # ── --evaluate: load measurements, evaluate gate, exit 0/1 ───────────
    if args.mode == "evaluate":
        with open(args.input) as _f:
            measurements = _json.load(_f)
        ok, verdicts, findings = evaluate_acceptance_gate(measurements)
        overall = "PASS" if ok else "FAIL"
        print(f"Acceptance gate: {overall}")
        for crit in ("a", "b", "c", "d"):
            print(f"  ({crit}) {verdicts.get(crit, '—')}")
        if findings:
            print("\nFindings:")
            for finding in findings:
                print(f"  {finding}")
        sys.exit(0 if ok else 1)

    # ── --report: load measurements, evaluate, render, write/print ────────
    if args.mode == "report":
        with open(args.input) as _f:
            measurements = _json.load(_f)
        _ok, verdicts, _findings = evaluate_acceptance_gate(measurements)
        report = render_acceptance_report(measurements, verdicts)
        if args.output:
            with open(args.output, "w") as _f:
                _f.write(report)
            sys.stderr.write(f"Report written to {args.output}\n")
        else:
            print(report)
        sys.exit(0)

    # ── --run: capture REAL same-instrument A/B measurements ─────────────
    if args.mode == "run":
        _run_campaign(
            args.output,
            ntasks=args.ntasks,
            cache_state=args.cache_state,
            utilization_threshold=args.utilization_threshold,
            sampler_interval=args.sampler_interval,
            task_repo=args.task_repo,
        )
        sys.exit(0)


if __name__ == "__main__":
    main()
