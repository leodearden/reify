#!/usr/bin/env python3
"""
prd-capability-check.py — executable capability probe runner (PRD §4 D1 / §9 contract).

Committed-probe-set format (JSON):
    {
        "probes": [
            {
                "capability": "<human name for the capability being probed>",
                "probe_kind": "grammar" | "check" | "ir",
                "fixture": "<repo-relative path to the .ri fixture file>",
                "expected": {
                    "observation": "present" | "absent",
                    "match": {
                        "exit_code": <int>,          // optional
                        "stderr_contains": "<str>",  // optional
                        "stdout_contains": "<str>"   // optional
                    }
                }
            },
            ...
        ]
    }

Probe kinds and dispatch:
    grammar  — `tree-sitter parse --quiet <fixture>` (CWD = tree-sitter-reify/)
               exit 0 → PRESENT (no parse errors); exit 1 with ERROR node → ABSENT
               exit 1 with "Failed to load language" → HARNESS ERROR (not a real absent)
    check    — `reify check <fixture>`
               match predicate satisfied → PRESENT; not satisfied → ABSENT
    ir       — `reify eval <fixture>` (eval-error proxy for IR shape)
               exit 0 clean → ABSENT (sound by determinism §6 G6(b))
               exit ≠ 0 WITH asserted signature in stderr → PRESENT
               exit ≠ 0 WITHOUT asserted signature → INDETERMINATE → UNPROVABLE

Verdicts:
    PASS       — observed matches expected
    FAIL       — observed contradicts expected
    UNPROVABLE — observation is INDETERMINATE (only possible for ir kind)

Harness exit codes:
    0   all PASS
    1   ≥1 FAIL
    2   ≥1 UNPROVABLE, 0 FAIL
    64  usage / argument error (sysexits EX_USAGE)
    70  tool / runtime error (sysexits EX_SOFTWARE) — missing binary, grammar load failure, etc.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional


# ---------------------------------------------------------------------------
# Valid constants
# ---------------------------------------------------------------------------

_VALID_PROBE_KINDS = frozenset({"grammar", "check", "ir"})
_VALID_OBSERVATIONS = frozenset({"present", "absent"})


# ---------------------------------------------------------------------------
# Data types
# ---------------------------------------------------------------------------

@dataclass
class Probe:
    """A single capability probe record from the committed probe-set JSON."""
    capability: str
    probe_kind: str                   # "grammar" | "check" | "ir"
    fixture: str                      # repo-relative path to the .ri fixture
    expected: Dict[str, Any]          # {observation: str, match: dict}


# ---------------------------------------------------------------------------
# Probe-set serialization
# ---------------------------------------------------------------------------

def load_probe_set(text: str) -> List[Probe]:
    """Parse a committed-probe-set JSON string into a list of Probe objects.

    Raises ValueError if the structure is invalid:
    - missing top-level 'probes' key
    - missing required fields (capability, probe_kind, fixture, expected)
    - unknown probe_kind (must be grammar|check|ir)
    - unknown observation value (must be present|absent)
    """
    try:
        obj = json.loads(text)
    except json.JSONDecodeError as e:
        raise ValueError(f"probe set is not valid JSON: {e}") from e

    if not isinstance(obj, dict) or "probes" not in obj:
        raise ValueError("probe set JSON must be an object with a top-level 'probes' key")

    probes: List[Probe] = []
    for i, raw in enumerate(obj["probes"]):
        # Required fields
        for field_name in ("capability", "probe_kind", "fixture", "expected"):
            if field_name not in raw:
                raise ValueError(
                    f"probe[{i}] is missing required field '{field_name}'"
                )

        probe_kind = raw["probe_kind"]
        if probe_kind not in _VALID_PROBE_KINDS:
            raise ValueError(
                f"probe[{i}] has unknown probe_kind '{probe_kind}'; "
                f"must be one of {sorted(_VALID_PROBE_KINDS)}"
            )

        expected = raw["expected"]
        if not isinstance(expected, dict) or "observation" not in expected:
            raise ValueError(
                f"probe[{i}] expected must be an object with an 'observation' field"
            )
        observation = expected["observation"]
        if observation not in _VALID_OBSERVATIONS:
            raise ValueError(
                f"probe[{i}] has unknown observation '{observation}'; "
                f"must be one of {sorted(_VALID_OBSERVATIONS)}"
            )
        # Ensure 'match' key exists (default to empty dict if absent)
        if "match" not in expected:
            expected = dict(expected, match={})

        probes.append(Probe(
            capability=raw["capability"],
            probe_kind=probe_kind,
            fixture=raw["fixture"],
            expected=expected,
        ))

    return probes


def dump_probe_set(probes: List[Probe]) -> str:
    """Serialize a list of Probe objects to a committed-probe-set JSON string.

    Uses stable key order and 4-space indentation for diff-friendliness.
    Round-trips with load_probe_set: load_probe_set(dump_probe_set(p)) == p.
    """
    def probe_to_dict(p: Probe) -> Dict[str, Any]:
        return {
            "capability": p.capability,
            "probe_kind": p.probe_kind,
            "fixture": p.fixture,
            "expected": {
                "observation": p.expected["observation"],
                "match": p.expected.get("match", {}),
            },
        }

    obj = {"probes": [probe_to_dict(p) for p in probes]}
    return json.dumps(obj, indent=4)


def main(argv: List[str]) -> int:
    """CLI entry-point. Returns an exit code (0/1/2/64/70)."""
    # Stub: usage error until implemented in step-14.
    parser = argparse.ArgumentParser(
        description="Run capability probes from a committed probe-set JSON file.",
        prog="prd-capability-check.py",
    )
    parser.add_argument(
        "probe_set",
        metavar="PROBE_SET_JSON",
        help="Path to the committed probe-set JSON file.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        dest="emit_json",
        help="Emit machine-readable JSON results to stdout.",
    )
    try:
        parser.parse_args(argv)
    except SystemExit as e:
        # argparse exits 0 for --help/--version, non-zero (usually 2) for bad args
        code = e.code if isinstance(e.code, int) else 64
        if code == 0:
            return 0
        return 64  # map any argparse error to EX_USAGE

    # Not yet implemented — return usage error.
    sys.stderr.write("error: probe runner not yet implemented\n")
    return 64


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
