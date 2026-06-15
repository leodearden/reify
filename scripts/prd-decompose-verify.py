#!/usr/bin/env python3
"""
prd-decompose-verify.py — deterministic harness library + CLI for γ decompose-phase
verification (PRD §4 D3 / §11 γ).

This module is the testable core of the Workflow-based decompose verification γ.
It handles:

  - Binding premise records to α probe-set dicts  (negative-assertion mandate)
  - Synthesizing α result records into a batch verdict
    (blocking on FAIL / UNPROVABLE / HARNESS_ERROR)

CLI subcommands:
    bind      <premises.json>     — emit probe-set JSON to stdout (exit 0)
    synthesize <results.json>     — emit BatchVerdict JSON;
                                    exit 0 (all pass) or 1 (blocks)

Reuses α (`prd-capability-check.py`) in-process via importlib — the same loader
pattern used by `test_prd_capability_check.py`.  α's file is NOT edited.

Design decisions:
  D1: negative-assertion mandate — a `rejection` premise binds probe_kind="check"
      with expected.observation="present" (NOT "absent").  The polarity is
      deterministic and unit-tested, NOT left to the LLM Enumerator.
  D2: `synthesize_batch` unions Prover + Adversary results; blocks on any
      FAIL / UNPROVABLE / HARNESS_ERROR; Adversary can only ADD blocking
      signals (net-positive recall, PRD decision 5).
  D3: reuse α's exact output shape via importlib — no re-implementation.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import sys
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional

# ---------------------------------------------------------------------------
# Load α (prd-capability-check.py) via importlib
# ---------------------------------------------------------------------------
# Mirror the loader pattern in test_prd_capability_check.py so α is reused
# in-process without editing the locked/merged file.

_SCRIPTS_DIR = os.path.dirname(os.path.abspath(__file__))
_ALPHA_PATH = os.path.join(_SCRIPTS_DIR, "prd-capability-check.py")

_alpha_spec = importlib.util.spec_from_file_location("prd_capability_check", _ALPHA_PATH)
pcc = importlib.util.module_from_spec(_alpha_spec)
# Register before exec_module so @dataclass and typing annotations in α resolve.
if "prd_capability_check" not in sys.modules:
    sys.modules["prd_capability_check"] = pcc
_alpha_spec.loader.exec_module(pcc)


# ---------------------------------------------------------------------------
# main() stub — returns 64 (EX_USAGE) until fully implemented
# ---------------------------------------------------------------------------

def main(argv: List[str]) -> int:
    """CLI entry-point.  Returns 64 (EX_USAGE) until subcommands are implemented."""
    return 64


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
