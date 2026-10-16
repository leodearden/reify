#!/usr/bin/env python3
"""
verify_events.py — the reader for the REIFY_VERIFY_EVENT_LOG verify event
channel, a JSON-lines file of v1 records. The contract, including the C3
consumer rules this module applies, is PRD
docs/prds/v0_6/run-all-runner-infra-harness.md §4; the schema table is in
tests/infra/README.md "Verify event channel".

Stdlib only, and safe under `python3 -I`: it imports nothing from tests/infra.
It is an importable module (read_events, summarize, select, wait_for) and a
CLI whose output is `key=value` scalars or JSON lines, so a bash caller reads
scalars and never parses JSON:

    verify_events.py summary PATH
    verify_events.py query PATH [--event E] [--where K=V ...] [--count]
    verify_events.py wait PATH [--event E] [--where K=V ...] [--count N] --timeout S

Exit codes: 0 success, 1 a `wait` that expired, 2 a usage error.
"""

import argparse
import json
import sys
import time
from dataclasses import dataclass
from pathlib import Path

SCHEMA_VERSION = 1

_SUMMARY_ORDER = (
    "channel",
    "runs",
    "discovered",
    "failures",
    "failed",
    "flaky",
    "skipped",
    "interrupted",
    "slot_timeout_fatal",
    "slot_timeout_soft",
    "skipped_lines",
)


@dataclass(frozen=True)
class Events:
    records: tuple
    skipped_lines: int

    @property
    def channel_present(self):
        return any(r.get("event") == "run_start" for r in self.records)


def _parse_line(raw):
    try:
        record = json.loads(raw.decode("utf-8", "surrogateescape"))
    except ValueError:
        return None
    if isinstance(record, dict) and record.get("v") == SCHEMA_VERSION:
        return record
    return None


def read_events(path):
    """Every v1 record in the file, in order; every other non-empty line is counted."""
    try:
        data = Path(path).read_bytes()
    except FileNotFoundError:
        return Events((), 0)
    records = []
    skipped = 0
    for raw in data.split(b"\n"):
        if not raw:
            continue
        record = _parse_line(raw)
        if record is None:
            skipped += 1
        else:
            records.append(record)
    return Events(tuple(records), skipped)


def _matches(record, event, where):
    if event is not None and record.get("event") != event:
        return False
    for field, wanted in (where or {}).items():
        if field not in record:
            return False
        value = record[field]
        if (value if isinstance(value, str) else json.dumps(value)) != wanted:
            return False
    return True


def select(events, event=None, where=None):
    """Records of `event` (any when None) whose fields equal `where`'s strings.

    A string field compares raw; any other value compares by its JSON
    spelling, so rc=1 matches "1" and interrupted=true matches "true".
    """
    return [r for r in events.records if _matches(r, event, where)]


def summarize(events):
    run_finishes = select(events, event="run_finish")
    timeouts = select(events, event="slot_timeout")

    def concatenated(field):
        return [name for r in run_finishes for name in (r.get(field) or [])]

    def total(field):
        return sum(r.get(field) or 0 for r in run_finishes)

    return {
        "channel": "present" if events.channel_present else "absent",
        "runs": len(select(events, event="run_start")),
        "discovered": total("discovered"),
        "failures": total("failures"),
        "failed": concatenated("failed"),
        "flaky": concatenated("flaky"),
        "skipped": concatenated("skipped"),
        "interrupted": any(r.get("interrupted") is True for r in run_finishes),
        "slot_timeout_fatal": sum(1 for r in timeouts if r.get("disposition") == "fatal"),
        "slot_timeout_soft": sum(1 for r in timeouts if r.get("disposition") == "soft"),
        "skipped_lines": events.skipped_lines,
    }


def _matching_count(path, event, where):
    return len(select(read_events(path), event=event, where=where))


def wait_for(path, *, event=None, where=None, count=1, deadline_s, poll_s=0.05):
    """Re-read `path` until `count` records match or `deadline_s` passes."""
    deadline = time.monotonic() + deadline_s
    while True:
        if _matching_count(path, event, where) >= count:
            return True
        if time.monotonic() >= deadline:
            return False
        time.sleep(poll_s)


def _scalar(value):
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, list):
        return " ".join(value)
    return str(value)


def _where_clause(text):
    field, sep, value = text.partition("=")
    if not sep or not field:
        raise argparse.ArgumentTypeError(f"expected FIELD=VALUE, got {text!r}")
    return field, value


def _parser():
    parser = argparse.ArgumentParser(prog="verify_events.py", description=__doc__.split("\n\n")[0])
    commands = parser.add_subparsers(dest="command", required=True)

    summary = commands.add_parser("summary", help="aggregate key=value scalars")
    summary.add_argument("path")

    def add_filters(sub):
        sub.add_argument("path")
        sub.add_argument("--event")
        sub.add_argument("--where", type=_where_clause, action="append", default=[])

    query = commands.add_parser("query", help="matching records as JSON lines")
    add_filters(query)
    query.add_argument("--count", action="store_true", help="print only the number")

    wait = commands.add_parser("wait", help="wait until enough records match")
    add_filters(wait)
    wait.add_argument("--count", type=int, default=1)
    wait.add_argument("--timeout", type=float, required=True)
    return parser


def main(argv=None):
    args = _parser().parse_args(argv)
    sys.stdout.reconfigure(errors="surrogateescape")
    if args.command == "summary":
        summary = summarize(read_events(args.path))
        for key in _SUMMARY_ORDER:
            print(f"{key}={_scalar(summary[key])}")
        return 0
    where = dict(args.where)
    if args.command == "query":
        matches = select(read_events(args.path), event=args.event, where=where)
        if args.count:
            print(len(matches))
        else:
            for record in matches:
                print(json.dumps(record))
        return 0
    if wait_for(args.path, event=args.event, where=where, count=args.count, deadline_s=args.timeout):
        return 0
    found = _matching_count(args.path, args.event, where)
    print(
        f"verify_events: wait: {found}/{args.count} matching records after {args.timeout:g}s",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
