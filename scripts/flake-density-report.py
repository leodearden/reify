#!/usr/bin/env python3
"""
flake-density-report.py — per-member accounting of tests/infra/run_all.sh's
durable FLAKY ledger (task #5142).

Reads the JSON Lines ledger whose records carry the 6-field schema emitted at
run_all.sh:711-715 — {ts, test, role, task, branch, run_id} — and reports, per
test member: flake count, distinct recorded runs, share of recorded flakes, and
a per-role breakdown. Members are ranked by flake count descending, ties broken
by name so the ranking is stable across runs.

WHAT THIS CANNOT TELL YOU. The ledger is append-only and a run with zero flaky
members writes NO line, by design (run_all.sh:729-731). The file therefore
describes only the runs that already flaked: counts here are per RECORDED-FLAKY
run, and a true flakes-per-gate density needs a total-run count that exists
nowhere in the JSONL. Supply it with --total-runs and the density is computed;
without it, none is reported rather than one divided by the wrong denominator.

`task` and `branch` are deliberately NOT grouping keys. Both are "unknown" and
"HEAD" in 167/167 records at the time of writing, because the merge-verify lane
runs detached and run_all.sh:699-705's `task/*` case never matches — grouping by
either yields one meaningless bucket. `role` is the secondary key that does
discriminate.

WHICH LEDGER THIS IS. This JSONL is the legacy/upstream source. dark-factory's
plans/flake-ledger-prd.md intends to supersede it with a SQLite ledger in
runs.db; the two must not be conflated, and this tool reads the older one.

Usage:
    python3 scripts/flake-density-report.py [--ledger PATH] [--top N]
                                            [--since ISO8601] [--total-runs N]
                                            [--json]
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from collections import Counter
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_LEDGER = REPO_ROOT / "data" / "verify-logs" / "flaky-ledger.jsonl"
LEDGER_ENV_VAR = "REIFY_RUN_ALL_FLAKY_LEDGER"

DENOMINATOR_CAVEAT = (
    "the ledger records only runs that already flaked (a clean run writes no "
    "line), so counts are per recorded-flaky run; pass --total-runs N for a "
    "true flakes-per-gate density"
)


# ---------------------------------------------------------------------------
# Ingest
# ---------------------------------------------------------------------------

@dataclass(frozen=True)
class LedgerRead:
    """Rows that parsed, plus one warning per row that did not."""
    rows: list[dict] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)


def resolve_ledger_path(explicit: str | None, env_value: str | None) -> Path:
    """--ledger beats $REIFY_RUN_ALL_FLAKY_LEDGER beats the repo default.

    The env-then-default tail is the producer's own precedence
    (run_all.sh:1236), so the tool and the writer agree on one source.
    """
    if explicit:
        return Path(explicit)
    if env_value:
        return Path(env_value)
    return DEFAULT_LEDGER


def read_ledger(path: Path) -> LedgerRead:
    """Tolerant JSON Lines read: a bad record is skipped with a warning.

    Mirrors dark-factory's read_flaky_ledger (chronic_flake.py:129, skips at
    :150 and :153) so the two readers of this same file agree on what a
    malformed record means. A row with no `test` field cannot be attributed to
    a member and is skipped on the same terms.
    """
    read = LedgerRead(rows=[], warnings=[])
    for lineno, line in enumerate(path.read_text().splitlines(), start=1):
        line = line.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            read.warnings.append(f"line {lineno}: not JSON: {line[:120]}")
            continue
        if not isinstance(row, dict):
            read.warnings.append(f"line {lineno}: not a JSON object: {line[:120]}")
            continue
        if not row.get("test"):
            read.warnings.append(f"line {lineno}: no `test` field: {line[:120]}")
            continue
        read.rows.append(row)
    return read


def _parse_ts(value: Any) -> datetime | None:
    """Parse the producer's `date -u +%Y-%m-%dT%H:%M:%SZ` stamp, tz-aware."""
    if not isinstance(value, str) or not value:
        return None
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None
    return parsed if parsed.tzinfo else parsed.replace(tzinfo=timezone.utc)


# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------

@dataclass(frozen=True)
class MemberReport:
    test: str
    flakes: int
    distinct_runs: int
    share: float
    roles: dict[str, int]
    density: float | None


@dataclass(frozen=True)
class Report:
    ledger_path: str
    records: int
    distinct_runs: int
    malformed: int
    undated_excluded: int
    since: str | None
    total_runs: int | None
    density: float | None
    truncated_to: int | None
    members_total: int
    members: list[MemberReport]

    def to_dict(self) -> dict:
        payload = {k: v for k, v in vars(self).items() if k != "members"}
        payload["caveat"] = DENOMINATOR_CAVEAT
        payload["members"] = [vars(m) for m in self.members]
        return payload


def build_report(rows: list[dict], *, top: int | None = None,
                 since: str | None = None, total_runs: int | None = None,
                 ledger_path: str = "", malformed: int = 0) -> Report:
    """Rank members of `rows`, optionally windowed by `since` and truncated to
    `top`. `total_runs` is the external denominator; without it no density is
    reported at all — see DENOMINATOR_CAVEAT."""
    since_dt = _parse_ts(since) if since else None
    if since and since_dt is None:
        raise ValueError(f"--since is not an ISO 8601 timestamp: {since!r}")

    undated_excluded = 0
    if since_dt is not None:
        windowed = []
        for row in rows:
            row_dt = _parse_ts(row.get("ts"))
            if row_dt is None:
                undated_excluded += 1
            elif row_dt >= since_dt:
                windowed.append(row)
        rows = windowed

    flakes: Counter[str] = Counter()
    runs_by_test: dict[str, set] = {}
    roles_by_test: dict[str, Counter[str]] = {}
    for row in rows:
        test = row["test"]
        flakes[test] += 1
        runs_by_test.setdefault(test, set()).add(row.get("run_id"))
        roles_by_test.setdefault(test, Counter())[row.get("role") or "unknown"] += 1

    records = len(rows)
    members = [
        MemberReport(
            test=test,
            flakes=count,
            distinct_runs=len(runs_by_test[test]),
            share=count / records,
            roles=dict(sorted(roles_by_test[test].items())),
            density=(count / total_runs) if total_runs else None,
        )
        for test, count in sorted(flakes.items(), key=lambda kv: (-kv[1], kv[0]))
    ]

    return Report(
        ledger_path=ledger_path,
        records=records,
        distinct_runs=len({row.get("run_id") for row in rows}),
        malformed=malformed,
        undated_excluded=undated_excluded,
        since=since,
        total_runs=total_runs,
        density=(records / total_runs) if total_runs else None,
        truncated_to=top,
        members_total=len(members),
        members=members[:top] if top else members,
    )


def format_text(report: Report) -> str:
    lines = ["=== flaky-ledger density report ==="]
    if report.ledger_path:
        lines.append(f"ledger:          {report.ledger_path}")
    lines.append(f"records:         {report.records}   (rows, not runs)")
    lines.append(f"distinct runs:   {report.distinct_runs}")
    if report.malformed:
        lines.append(f"malformed:       {report.malformed} line(s) skipped")
    if report.since:
        lines.append(f"since:           {report.since}"
                     f"   ({report.undated_excluded} undated record(s) excluded)")
    if report.total_runs:
        lines.append(f"total runs:      {report.total_runs} (supplied)")
        lines.append(f"density:         {report.density:.4f} flakes per gate run")
    lines.append(f"NOTE: {DENOMINATOR_CAVEAT}.")
    lines.append("")

    if not report.members:
        lines.append("(no flaky members recorded)")
        return "\n".join(lines) + "\n"

    if report.truncated_to and report.truncated_to < report.members_total:
        lines.append(f"top {report.truncated_to} of {report.members_total} "
                     f"members shown")
    width = max(len(m.test) for m in report.members)
    header = f"  {'#':<4}{'member':<{width}}  {'flakes':>6}  {'runs':>5}  {'share':>6}"
    if report.total_runs:
        header += f"  {'density':>8}"
    lines.append(header)
    lines.append("  " + "-" * (len(header) - 2))
    for rank, member in enumerate(report.members, start=1):
        roles = " ".join(f"{role}={n}" for role, n in member.roles.items())
        row = (f"  #{rank:<3}{member.test:<{width}}  {member.flakes:>6}"
               f"  {member.distinct_runs:>5}  {member.share:>6.1%}")
        if report.total_runs:
            row += f"  {member.density:>8.4f}"
        lines.append(f"{row}  [{roles}]")
    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def _positive_int(value: str) -> int:
    parsed = int(value)
    if parsed <= 0:
        raise argparse.ArgumentTypeError(f"must be a positive integer: {value}")
    return parsed


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=("Per-member accounting of run_all.sh's durable FLAKY "
                     "ledger (task #5142)."),
        epilog=f"NOTE: {DENOMINATOR_CAVEAT}.",
    )
    parser.add_argument(
        "--ledger", metavar="PATH",
        help=(f"Ledger to read. Default: ${LEDGER_ENV_VAR}, else "
              f"data/verify-logs/flaky-ledger.jsonl."),
    )
    parser.add_argument(
        "--top", type=_positive_int, metavar="N",
        help="Show only the N most flaky members.",
    )
    parser.add_argument(
        "--since", metavar="ISO8601",
        help="Ignore records stamped before this time (undated ones too).",
    )
    parser.add_argument(
        "--total-runs", type=_positive_int, metavar="N", dest="total_runs",
        help=("External count of gate runs in the same window, the denominator "
              "the ledger cannot supply. Without it, no density is reported."),
    )
    parser.add_argument(
        "--json", action="store_true", dest="emit_json",
        help="Emit the report as JSON instead of human-readable text.",
    )
    args = parser.parse_args(argv)

    path = resolve_ledger_path(args.ledger, os.environ.get(LEDGER_ENV_VAR))
    if not path.is_file():
        print(f"ERROR: flaky ledger not found: {path}", file=sys.stderr)
        print("       Pass --ledger PATH, or set "
              f"${LEDGER_ENV_VAR}.", file=sys.stderr)
        return 2

    read = read_ledger(path)
    for warning in read.warnings:
        print(f"WARNING: skipping malformed ledger record — {warning}",
              file=sys.stderr)

    try:
        report = build_report(
            read.rows, top=args.top, since=args.since,
            total_runs=args.total_runs, ledger_path=str(path),
            malformed=len(read.warnings),
        )
    except ValueError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2

    if args.emit_json:
        print(json.dumps(report.to_dict(), indent=2))
    else:
        print(format_text(report), end="")
    return 0


if __name__ == "__main__":
    sys.exit(main())
