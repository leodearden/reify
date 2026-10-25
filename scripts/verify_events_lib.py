"""
verify_events_lib.py — decode a C2 verify-event JSON-lines file into its v1
records, counting every other non-empty line (C3 rule 1).

The contract is PRD docs/prds/v0_6/run-all-runner-infra-harness.md §4; the
schema table is tests/infra/README.md "Verify event channel". Stdlib only and
safe under `python3 -I`. Its readers are scripts/verify-ledger.py and
tests/infra/verify_events.py, so it imports nothing from tests/infra.
"""

import json
from dataclasses import dataclass
from pathlib import Path

SCHEMA_VERSION = 1


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
    if isinstance(record, dict) and type(record.get("v")) is int and record["v"] == SCHEMA_VERSION:
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
