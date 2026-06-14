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
from dataclasses import dataclass
from typing import Any, Dict, List, Optional


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
        # argparse exits 0 for --help, non-zero for bad args
        code = e.code if isinstance(e.code, int) else 64
        return code if code != 0 else 0

    # Not yet implemented — return usage error.
    sys.stderr.write("error: probe runner not yet implemented\n")
    return 64


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
