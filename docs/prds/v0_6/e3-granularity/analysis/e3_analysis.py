#!/usr/bin/env python3
"""
e3_analysis.py — the pre-registered E3 §7 analysis (commissioned by the §10
"Addendum 2026-10-10", item (d), of docs/prds/v0_6/e3-decomposition-granularity-ab.md).

Usage:
    python3 docs/prds/v0_6/e3-granularity/analysis/e3_analysis.py [--as-of ISO]
        [--tasks-snapshot PATH] [--runs-db PATH] [--repo PATH] [--ref REF]
        [--fused-memory-url URL] [--project-root PATH] [--out-dir DIR]
        [--rule-i-instant ISO]

Data sources, all read-only:
- the fused-memory task store through its MCP (get_statuses, then get_task),
  or a committed tasks-snapshot.json replayed with --tasks-snapshot;
- data/orchestrator/runs.db: attempts, every role's invocation cost, and
  blocking escalations;
- git main: each task's first-parent `Merge task/<id> into main` and its LOC.

The task store records no cancellation instant, so a cancelled close leaf
leaves stop rule (i) undetermined; --rule-i-instant is the operator's fix for
that state and is refused in any other.

Outputs, under analysis/results/<as-of>/ unless --out-dir names another
directory: report.txt (also printed), endpoints.json, and tasks-snapshot.json
when the task store was read live. The record is written into a staging
directory and renamed into place, and an existing record is never
overwritten.

Exit codes: 0 ok; 1 a pre-registered invariant is violated; 2 usage; 3 a
data source is unreadable; 4 the record could not be written; 5 a defect in
this script, i.e. any other failure (its traceback is printed). Nothing is
left behind unless the exit code is 0.

The legacy reify_task_size_analysis.py beside this file is the parent
investigation's record, not the E3 script.
"""

import argparse
import json
import os
import shutil
import sqlite3
import sys
import traceback
from dataclasses import dataclass, fields, is_dataclass
from datetime import datetime, timedelta
from enum import Enum
from fractions import Fraction
from pathlib import Path

import e3_model as model
from e3_protocol import E3, UTC, Arm, ProtocolViolation, format_ids
from e3_sources import (
    SourceError, fetch_task_snapshot, fused_memory_url, open_fused_memory, read_runs_db,
    read_task_merges, read_task_snapshot, utc_timestamp, write_task_snapshot,
)

ANALYSIS_DIR = Path(__file__).resolve().parent
SCRIPTS_DIR = ANALYSIS_DIR.parents[4] / "scripts"
REPORT_NAME = "report.txt"
ENDPOINTS_NAME = "endpoints.json"
SNAPSHOT_NAME = "tasks-snapshot.json"
ENDPOINTS_SCHEMA = "e3-endpoints/1"
EXIT_OK, EXIT_PROTOCOL, EXIT_USAGE, EXIT_SOURCE, EXIT_RECORD, EXIT_DEFECT = 0, 1, 2, 3, 4, 5
SOURCE_PARSE_FAILURES = (OSError, ValueError, KeyError, TypeError, sqlite3.Error)
DEFECT_ESCAPE_WINDOW = timedelta(days=30)


class UsageError(Exception):
    """The invocation asks for something the analysis refuses to do."""


class RecordNotWritten(Exception):
    """The record could not be written; nothing was left behind."""


def default_out_dir(as_of):
    return ANALYSIS_DIR / "results" / as_of.astimezone(UTC).strftime("%Y%m%dT%H%M%SZ")


@dataclass(frozen=True)
class Analysis:
    """Everything the report and endpoints.json render, computed once."""

    protocol: object
    as_of: datetime
    snapshot: object
    arm_tasks: tuple
    freeze_set: frozenset
    outcomes: tuple
    probe: object
    verdict: object
    at_trigger: tuple | None
    current: tuple
    descriptives: object
    era_cells: tuple
    censoring: tuple
    dropped: tuple
    sensitivity: tuple
    sensitivity_descriptives: object
    carve_outs: tuple


def analyse(snapshot, arm_tasks, runs, merges, as_of, protocol, operator_rule_i_instant=None):
    """§7 over the parsed sources: freeze check, outcomes at as_of, the stop
    rule, the one look frozen at the trigger, then the descriptive figures."""
    freeze_set = model.freeze_window_set(arm_tasks, runs.attempts, protocol)

    def outcomes_at(instant):
        return model.task_outcomes(arm_tasks, snapshot, runs, merges, freeze_set, instant,
                                   protocol)

    outcomes = outcomes_at(as_of)
    probe = model.stop_rule_probe(outcomes, protocol, operator_rule_i_instant)
    at_trigger = (model.prd_endpoints(outcomes_at(probe.trigger_instant), protocol)
                  if probe.trigger_instant is not None else None)
    current = model.prd_endpoints(outcomes, protocol)
    arm_ids = {task.id for task in arm_tasks}
    dropped = tuple(a for a in protocol.amendments if a.task_id in arm_ids)
    sensitivity = model.prd_endpoints(outcomes, protocol,
                                      drop=frozenset(a.task_id for a in dropped))
    return Analysis(
        protocol=protocol, as_of=as_of, snapshot=snapshot, arm_tasks=arm_tasks,
        freeze_set=freeze_set, outcomes=outcomes, probe=probe,
        verdict=model.confirmatory_verdict(probe, at_trigger, protocol),
        at_trigger=at_trigger, current=current,
        descriptives=model.descriptives(current, protocol),
        era_cells=model.era_split(outcomes),
        censoring=model.arm_censoring_counts(outcomes),
        dropped=dropped, sensitivity=sensitivity,
        sensitivity_descriptives=model.descriptives(sensitivity, protocol),
        carve_outs=model.carve_out_ledgers(snapshot, runs, merges, as_of, protocol),
    )


def _parser(protocol):
    parser = argparse.ArgumentParser(
        prog="e3_analysis.py",
        description="Pre-registered E3 §7 analysis: stop-rule probe, censoring, endpoints.")
    parser.add_argument("--project-root", default=protocol.project_root,
                        help="the checkout whose runs.db and repo are the defaults; a live "
                             "task-store read admits only the registered project root")
    parser.add_argument("--runs-db", type=Path,
                        help="default: <project-root>/data/orchestrator/runs.db")
    parser.add_argument("--repo", type=Path, help="default: <project-root>")
    parser.add_argument("--ref", default="main")
    parser.add_argument("--fused-memory-url",
                        help="default: mcpServers['fused-memory'].url of <repo>/.mcp.json")
    parser.add_argument("--tasks-snapshot", type=Path,
                        help="replay a committed snapshot instead of reading the task store")
    parser.add_argument("--as-of", help="ISO 8601 instant with offset (default: now, UTC)")
    parser.add_argument("--out-dir", type=Path,
                        help="default: analysis/results/<as-of %%Y%%m%%dT%%H%%M%%SZ>")
    parser.add_argument("--rule-i-instant",
                        help="ISO 8601 instant, operator-fixed, at which stop rule (i) was met; "
                             "admitted only while cancelled close leaves leave it undetermined")
    return parser


def _instant(option, text):
    try:
        return utc_timestamp(text)
    except (ValueError, SourceError) as error:
        raise UsageError(f"{option} needs an ISO 8601 instant with a UTC offset: {error}") \
            from error


def _as_of(text):
    if text is None:
        return datetime.now(UTC).replace(microsecond=0)
    return _instant("--as-of", text)


def _operator_rule_i_instant(text, as_of):
    if text is None:
        return None
    instant = _instant("--rule-i-instant", text)
    if instant > as_of:
        raise UsageError(f"--rule-i-instant {_ts(instant)} is later than --as-of {_ts(as_of)}")
    return instant


def _refuse_existing(out_dir):
    if out_dir.exists() and (not out_dir.is_dir() or any(out_dir.iterdir())):
        raise UsageError(f"{out_dir} already exists and is not an empty directory; "
                         "a committed record is never overwritten")


def _read_source(name, read):
    """read(), with any failure to read or parse the source raised as a
    SourceError naming it, so that exit 1 stays reserved for the protocol."""
    try:
        return read()
    except SOURCE_PARSE_FAILURES as error:
        raise SourceError(f"{name}: {error}") from error


def _load_snapshot(args, repo, protocol, open_task_store):
    """(snapshot, fetched live?)."""
    if args.tasks_snapshot is not None:
        return _read_source(f"task snapshot {args.tasks_snapshot}",
                            lambda: read_task_snapshot(args.tasks_snapshot)), False
    if args.project_root != protocol.project_root:
        raise UsageError(f"--project-root {args.project_root} is not the registered project "
                         f"root {protocol.project_root}, the one a live read keys the task "
                         "store by")
    url = args.fused_memory_url or _read_source(
        "fused-memory url", lambda: fused_memory_url(repo / ".mcp.json"))
    fetched_at = datetime.now(UTC).replace(microsecond=0)
    return _read_source(f"fused-memory task store {url}", lambda: fetch_task_snapshot(
        open_task_store(url), protocol.project_root, protocol.snapshot_candidate_ids(),
        fetched_at)), True


def _refuse_late_as_of(as_of, snapshot):
    if as_of > snapshot.fetched_at:
        raise UsageError(f"--as-of {_ts(as_of)} is later than the task snapshot "
                         f"(fetched {_ts(snapshot.fetched_at)}); its statuses cannot vouch "
                         "for that state")


def _compute(args, protocol, open_task_store):
    """(analysis, the live snapshot to persist or None, out_dir)."""
    as_of = _as_of(args.as_of)
    operator_rule_i_instant = _operator_rule_i_instant(args.rule_i_instant, as_of)
    out_dir = args.out_dir or default_out_dir(as_of)
    _refuse_existing(out_dir)
    project_root = Path(args.project_root)
    repo = args.repo or project_root
    snapshot, live = _load_snapshot(args, repo, protocol, open_task_store)
    _refuse_late_as_of(as_of, snapshot)
    arm_tasks = model.attribute(snapshot, protocol)
    task_ids = {task.id for task in arm_tasks} | protocol.carve_outs
    runs_db = args.runs_db or project_root / "data" / "orchestrator" / "runs.db"
    runs = _read_source(f"runs.db {runs_db}", lambda: read_runs_db(runs_db, task_ids))
    merges = _read_source(f"git {repo} {args.ref}",
                          lambda: read_task_merges(repo, args.ref, task_ids))
    try:
        analysis = analyse(snapshot, arm_tasks, runs, merges, as_of, protocol,
                           operator_rule_i_instant)
    except model.OperatorInstantRefused as refusal:
        raise UsageError(f"--rule-i-instant refused: {refusal}") from refusal
    return analysis, (snapshot if live else None), out_dir


def main(argv=None, protocol=E3, open_task_store=open_fused_memory):
    """open_task_store(url) -> call_tool is the live task-store seam; the
    default opens a fused-memory MCP session."""
    args = _parser(protocol).parse_args(argv)
    try:
        report = _run(args, protocol, open_task_store)
    except UsageError as error:
        print(f"e3_analysis: {error}", file=sys.stderr)
        return EXIT_USAGE
    except ProtocolViolation as violation:
        print(f"E3 PROTOCOL VIOLATION: {violation}", file=sys.stderr)
        return EXIT_PROTOCOL
    except SourceError as error:
        print(f"E3 DATA SOURCE UNREADABLE: {error}", file=sys.stderr)
        return EXIT_SOURCE
    except RecordNotWritten as error:
        print(f"E3 RECORD NOT WRITTEN: {error}", file=sys.stderr)
        return EXIT_RECORD
    except Exception:
        print("E3 ANALYSIS DEFECT: a failure outside every documented one, so a bug in this "
              "script, not a protocol finding:", file=sys.stderr)
        traceback.print_exc()
        return EXIT_DEFECT
    print(report, end="")
    return EXIT_OK


def _run(args, protocol, open_task_store):
    """Compute, render and write the record; returns the report to print."""
    analysis, live_snapshot, out_dir = _compute(args, protocol, open_task_store)
    report = render_report(analysis)
    _write_outputs(out_dir, report, to_json(analysis), live_snapshot)
    return report


def _write_outputs(out_dir, report, document, live_snapshot):
    """Write the record, raising RecordNotWritten when the filesystem refuses."""
    try:
        _stage_then_rename(out_dir, report, document, live_snapshot)
    except OSError as error:
        raise RecordNotWritten(str(error)) from error


def _stage_then_rename(out_dir, report, document, live_snapshot):
    """Write the record into a staging sibling, then rename it into place
    (onto an absent or empty out_dir), so a failure leaves no partial record."""
    out_dir.parent.mkdir(parents=True, exist_ok=True)
    staging = out_dir.with_name(f".{out_dir.name}.staging-{os.getpid()}")
    staging.mkdir()
    try:
        (staging / REPORT_NAME).write_text(report, encoding="utf-8")
        (staging / ENDPOINTS_NAME).write_text(
            json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        if live_snapshot is not None:
            write_task_snapshot(live_snapshot, staging / SNAPSHOT_NAME)
        staging.rename(out_dir)
    except BaseException:
        shutil.rmtree(staging, ignore_errors=True)
        raise


# ── rendering ──────────────────────────────────────────────────────────────


def _ts(moment):
    return moment.astimezone(UTC).strftime("%Y-%m-%dT%H:%M:%SZ") if moment else "—"


def _num(value, digits=2):
    return "—" if value is None else f"{value:.{digits}f}"


def _name(member):
    return "—" if member is None else member.value


def _frac(value):
    return "—" if value is None else f"{value} ({float(value):.3f})"


def _current_kind(analysis):
    if analysis.probe.trigger_instant is None:
        return "interim descriptive"
    return "post-trigger descriptive update"


def render_report(analysis):
    sections = (_header, _probe_section, _verdict_section, _censoring_section,
                _endpoints_section, _medians_section, _stratum_b_section, _era_section,
                _sensitivity_section, _excluded_section, _unlanded_section,
                _endpoint5_section, _integrity_section)
    return "\n".join(line for render in sections for line in render(analysis)) + "\n"


def _header(analysis):
    snapshot, verdict = analysis.snapshot, analysis.verdict
    by_arm = {arm: sum(1 for t in analysis.arm_tasks if t.arm is arm) for arm in Arm}
    return [
        f"E3 §7 ANALYSIS — as of {_ts(analysis.as_of)}",
        f"LABEL: {verdict.label} — {verdict.reason}",
        "Protocol: docs/prds/v0_6/e3-decomposition-granularity-ab.md §7 + §10 addendum "
        "2026-10-10",
        f"Task snapshot: {snapshot.source}, fetched {_ts(snapshot.fetched_at)}, "
        f"project_root {snapshot.project_root}",
        f"Attribution: coarse {by_arm[Arm.COARSE]} tasks, standard {by_arm[Arm.STANDARD]} tasks",
        f"Freeze-window check: passed — derived {format_ids(analysis.freeze_set)} equals the "
        "pre-registered set",
        "",
    ]


def _probe_section(analysis):
    probe, k = analysis.probe, analysis.protocol.landing_floor_k
    lines = [f"== STOP-RULE PROBE (addendum (a); k={k}) =="]
    for floor in probe.per_prd:
        landings = ", ".join(f"#{task_id} ({_ts(at)})" for task_id, at in floor.qualifying)
        lines.append(f"  {floor.arm.value:<8} {floor.prd_slug:<40} "
                     f"{len(floor.qualifying)}/{k} qualifying  {landings or '—'}")
    lines += [
        f"  rule (ii) landing floor in every PRD: "
        f"{'met at ' + _ts(probe.rule_ii_instant) if probe.rule_ii_met else 'not met'}",
        f"  rule (i) every close leaf terminal: {_rule_i_state(probe.rule_i)}",
        "  rule (iii) human re-review 2027-01-09: an operator decision, not probed",
        f"  trigger instant: {_ts(probe.trigger_instant) if probe.trigger_instant else 'none'}",
        "",
    ]
    return lines


def _rule_i_state(rule_i):
    cancelled = format_ids(rule_i.cancelled)
    if rule_i.operator_fixed:
        return (f"met, instant {_ts(rule_i.instant)} (operator-fixed --rule-i-instant; "
                f"cancelled close leaf {cancelled} carries no recorded instant)")
    if rule_i.met:
        return f"met, instant {_ts(rule_i.instant)}"
    if rule_i.undetermined:
        return (f"undetermined — cancelled close leaf {cancelled} carries no recorded "
                "cancellation instant; an operator fixes it with --rule-i-instant")
    return "not met"


def _verdict_section(analysis):
    verdict, alpha = analysis.verdict, analysis.protocol.alpha
    lines = ["== CONFIRMATORY VERDICT (§7.3) ==", f"  label: {verdict.label}",
             f"  reason: {verdict.reason}"]
    if verdict.split is not None:
        lines.append(f"  contributing split: {verdict.split[0]}v{verdict.split[1]}, "
                     f"minimum achievable p {_frac(verdict.min_p)}, alpha {alpha}")
    if verdict.test is not None:
        lines.append(f"  exact one-sided U = {verdict.test.u}, p = {_frac(verdict.test.p)} "
                     f"over {verdict.test.arrangements} arrangements; rejects H0: "
                     f"{'yes' if verdict.rejects_null else 'no'}")
    return lines + [""]


def _censoring_section(analysis):
    lines = ["== CENSORING (addendum (b)3: three separate counts per arm) =="]
    for counts in analysis.censoring:
        lines.append(f"  {counts.arm.value:<8} freeze-window excluded "
                     f"{counts.freeze_window_excluded} tasks | right-censored "
                     f"{counts.right_censored} tasks | drain-censored attempts "
                     f"{counts.drain_censored_attempts}")
    return lines + [""]


ENDPOINT_HEADER = (f"  {'PRD':<38} {'arm':<8} {'land/cens/excl':>14} {'cost $':>9} {'LOC':>7} "
                   f"{'$/kLOC':>8} {'att/task':>8} {'att/kLOC':>8} {'blk/task':>8} "
                   f"{'blk/kLOC':>8}  scope share")


def _endpoint_row(e):
    counts = f"{e.n_landed}/{e.n_right_censored}/{e.n_freeze_excluded}"
    return (f"  {e.prd_slug:<38} {e.arm.value:<8} {counts:>14} {e.landed_cost:>9.2f} "
            f"{e.landed_loc:>7} {_num(e.usd_per_kloc):>8} {_num(e.attempts_per_task):>8} "
            f"{_num(e.attempts_per_kloc):>8} {_num(e.blocking_esc_per_task):>8} "
            f"{_num(e.blocking_esc_per_kloc):>8}  {_frac(e.landed_scope_share)}")


def _landed_task_row(outcome):
    ledger, amendment = outcome.ledger, outcome.task.amendment
    return (f"      #{ledger.task_id} landed {_ts(ledger.landed_at)}: cost ${ledger.cost:.2f}, "
            f"LOC {ledger.loc}, attempts {ledger.attempts_counted}, blocking esc "
            f"{ledger.blocking_escalations}, era {_name(outcome.era)}"
            + (f", amended {amendment.source}" if amendment else ""))


def _landed_task_rows(analysis, slug):
    return [_landed_task_row(o) for o in analysis.outcomes
            if o.task.prd_slug == slug and o.landed_eligible]


def _endpoints_section(analysis):
    lines = [f"== PER-PRD ENDPOINTS AS OF {_ts(analysis.as_of)} — "
             f"{_current_kind(analysis).upper()} ==", ENDPOINT_HEADER]
    for endpoint in analysis.current:
        lines.append(_endpoint_row(endpoint))
        lines += _landed_task_rows(analysis, endpoint.prd_slug)
    if analysis.at_trigger is not None:
        lines += [f"  at the trigger instant {_ts(analysis.probe.trigger_instant)} (frozen; the "
                  "confirmatory input):", ENDPOINT_HEADER]
        lines += [_endpoint_row(endpoint) for endpoint in analysis.at_trigger]
    return lines + [""]


def _summary_cell(summary):
    low, high = summary.interval or (None, None)
    return f"{_num(summary.median)} [{_num(low)}, {_num(high)}] n={summary.n_prds}"


def _median_rows(descriptives, metrics):
    return [f"  {metric:<22} coarse {_summary_cell(descriptives.summary(metric, Arm.COARSE)):<34}"
            f" standard {_summary_cell(descriptives.summary(metric, Arm.STANDARD))}"
            for metric in metrics]


def _medians_section(analysis):
    protocol = analysis.protocol
    return ([f"== ARM MEDIANS + BOOTSTRAP ({float(protocol.interval_level):.0%} interval, "
             f"{protocol.bootstrap_resamples} resamples by PRD; DESCRIPTIVE) =="]
            + _median_rows(analysis.descriptives, model.ENDPOINT_METRICS) + [""])


def _stratum_b_section(analysis):
    return (["== STRATUM-B PAIR (closest-matched; DESCRIPTIVE) ==", ENDPOINT_HEADER]
            + [_endpoint_row(e) for e in analysis.descriptives.stratum_b] + [""])


def _era_section(analysis):
    boundaries = ", ".join(f"{_ts(e.at)} ({e.label})" for e in analysis.protocol.era_events)
    lines = [f"== ERA SPLIT (§7.6 at {boundaries}; landed eligible tasks; DESCRIPTIVE) =="]
    for cell in analysis.era_cells:
        lines.append(f"  {cell.arm.value:<8} {cell.era.value:<9} n={cell.n_tasks:<3} "
                     f"$/kLOC {_num(cell.usd_per_kloc):>8}  att/task "
                     f"{_num(cell.attempts_per_task):>6}  blk/task "
                     f"{_num(cell.blocking_esc_per_task):>6}")
    return lines + [""]


def _sensitivity_section(analysis):
    dropped = ", ".join(f"#{a.task_id} ({a.source}, {a.date})" for a in analysis.dropped)
    return (["== SENSITIVITY (addendum (b)4: amended tasks dropped; DESCRIPTIVE) ==",
             f"  dropped: {dropped or 'none'}", ENDPOINT_HEADER]
            + [_endpoint_row(e) for e in analysis.sensitivity]
            + _median_rows(analysis.sensitivity_descriptives, ("usd_per_kloc",)) + [""])


def _ledger_row(ledger, prefix):
    """One excluded task's descriptive figures."""
    return (f"  #{ledger.task_id} {prefix}: status {ledger.status.value}, landed "
            f"{_ts(ledger.landed_at)}, cost ${ledger.cost:.2f}, LOC {ledger.loc}, attempts "
            f"counted {ledger.attempts_counted} / drain-censored "
            f"{ledger.attempts_drain_censored} / in-flight {ledger.attempts_in_flight}, "
            f"blocking esc {ledger.blocking_escalations}")


def _excluded_section(analysis):
    lines = ["== EXCLUDED TASKS (descriptive only; never in an endpoint) =="]
    lines += [_ledger_row(o.ledger, f"{o.task.arm.value} {o.task.prd_slug} {o.excluded.value}")
              for o in analysis.outcomes if not o.eligible]
    lines += [_ledger_row(ledger, "carve-out (§7.1)") for ledger in analysis.carve_outs]
    return lines + [""]


def _unlanded_section(analysis):
    lines = ["== UNLANDED (addendum (b)1: right-censored; never in a per-kLOC figure) =="]
    lines += [f"  {e.prd_slug:<38} {e.arm.value:<8} right-censored {e.n_right_censored} tasks, "
              f"attempts {e.unlanded_attempts}, blocking esc {e.unlanded_blocking_escalations}"
              for e in analysis.current]
    return lines + [""]


def _endpoint5_section(analysis):
    by_id = {o.task.id: o.ledger for o in analysis.outcomes}
    lines = ["== ENDPOINT 5 (defect escape: PRD close + 30 days) =="]
    for prd in analysis.protocol.prds:
        closed_at = by_id[prd.close_leaf].landed_at
        state = (f"closed {_ts(closed_at)}; window ends {_ts(closed_at + DEFECT_ESCAPE_WINDOW)};"
                 " not computed by this script" if closed_at else
                 "not closed — not yet available")
        lines.append(f"  {prd.slug:<38} close leaf #{prd.close_leaf}: {state}")
    return lines + [""]


def _integrity_section(analysis):
    anomalies = [a for o in analysis.outcomes for a in o.ledger.anomalies]
    anomalies += [a for ledger in analysis.carve_outs for a in ledger.anomalies]
    return (["== DATA INTEGRITY =="]
            + ([f"  {a.kind}: {a.detail}" for a in anomalies] or ["  none"]))


# ── endpoints.json ─────────────────────────────────────────────────────────


def _jsonable(value):
    if isinstance(value, datetime):
        return _ts(value)
    if isinstance(value, Fraction):
        return {"exact": str(value), "float": float(value)}
    if isinstance(value, Enum):
        return value.value
    if is_dataclass(value):
        return {f.name: _jsonable(getattr(value, f.name)) for f in fields(value)}
    if isinstance(value, dict):
        return {str(key): _jsonable(item) for key, item in value.items()}
    if isinstance(value, (set, frozenset)):
        return sorted(_jsonable(item) for item in value)
    if isinstance(value, (list, tuple)):
        return [_jsonable(item) for item in value]
    return value


def to_json(analysis):
    protocol, snapshot = analysis.protocol, analysis.snapshot
    return _jsonable({
        "schema": ENDPOINTS_SCHEMA,
        "as_of": analysis.as_of,
        "protocol": {
            "landing_floor_k": protocol.landing_floor_k, "alpha": protocol.alpha,
            "freeze_cutoff": protocol.freeze_cutoff,
            "drain_window_seconds": protocol.drain_window.total_seconds(),
            "bootstrap_seed": protocol.bootstrap_seed,
            "bootstrap_resamples": protocol.bootstrap_resamples,
            "interval_level": protocol.interval_level,
        },
        "task_snapshot": {"source": snapshot.source, "fetched_at": snapshot.fetched_at,
                          "project_root": snapshot.project_root},
        "attribution": {arm.value: [t.id for t in analysis.arm_tasks if t.arm is arm]
                        for arm in Arm},
        "freeze_window_set": analysis.freeze_set,
        "stop_rule_probe": analysis.probe,
        "verdict": analysis.verdict,
        "at_trigger": analysis.at_trigger,
        "current": {
            "kind": _current_kind(analysis), "label": model.LABEL_DESCRIPTIVE,
            "prd_endpoints": analysis.current, "descriptives": analysis.descriptives,
            "era_split": analysis.era_cells, "censoring": analysis.censoring,
            "tasks": analysis.outcomes,
        },
        "sensitivity": {
            "label": model.LABEL_DESCRIPTIVE, "dropped": analysis.dropped,
            "prd_endpoints": analysis.sensitivity,
            "descriptives": analysis.sensitivity_descriptives,
        },
        "carve_outs": analysis.carve_outs,
    })


if __name__ == "__main__":
    sys.path.insert(0, str(SCRIPTS_DIR))
    sys.exit(main())
