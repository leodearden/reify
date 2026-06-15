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
# Premise — input record from the Enumerator / leaf fixture
# ---------------------------------------------------------------------------

@dataclass
class Premise:
    """A single premise asserted by a decompose-leaf signal.

    Fields:
        text            — human-readable assertion statement
        assertion_kind  — "rejection" | "parses" | "resolves" | "produces" | "ir"
        fixture         — repo-relative path to the .ri fixture file
        match           — match predicate dict for α's probe (exit_code, stderr_contains, …)
        capability      — optional human name; falls back to text if None
    """
    text: str
    assertion_kind: str
    fixture: str
    match: Dict[str, Any]
    capability: Optional[str] = None


# ---------------------------------------------------------------------------
# Assertion-kind → (probe_kind, observation) mapping
# ---------------------------------------------------------------------------

# Deterministic binding table (PRD D1 / negative-assertion mandate):
#   rejection → check / present  — the rejection FIRES → exit_code 1 → match satisfied → PRESENT
#                                   if reify accepts silently (exit 0) → match fails → ABSENT → FAIL
#   parses    → grammar / present — tree-sitter exits 0 (no parse errors) → PRESENT → PASS
#   resolves  → check / present  — reify check exits 0 → match {exit_code:0} satisfied → PRESENT
#   produces  → ir / present     — reify eval exits ≠ 0 with signature → PRESENT → PASS
#   ir        → ir / absent      — reify eval exits 0 (clean) → ABSENT → expected absent → PASS
_ASSERTION_KIND_MAP: Dict[str, tuple] = {
    "rejection": ("check",   "present"),
    "parses":    ("grammar", "present"),
    "resolves":  ("check",   "present"),
    "produces":  ("ir",      "present"),
    "ir":        ("ir",      "absent"),
}


def premise_to_probe(premise: Premise) -> Dict[str, Any]:
    """Bind a Premise record to an α probe dict.

    The binding is deterministic — assertion_kind → (probe_kind, observation) from
    _ASSERTION_KIND_MAP; the match dict is passed through verbatim from the premise.

    Negative-assertion mandate (D1): a "rejection" premise binds observation="present"
    (NOT "absent").  A "revolute rejects a non-axis arg" assertion must probe that the
    rejection FIRES (exit_code:1 → match satisfied → PRESENT → PASS).  If reify silently
    accepts (exit 0), the match is not satisfied → ABSENT → expected "present" → FAIL.
    This is the exact W1 polarity guard: a human or LLM would naturally write "absent"
    (the rejection is absent), which inverts the test sense and masks the 4575 bug.

    Args:
        premise: A Premise record from the Enumerator or a leaf fixture.

    Returns:
        A dict in α's committed-probe-set shape, accepted by pcc.load_probe_set.

    Raises:
        ValueError: if the assertion_kind is unknown.
    """
    kind = premise.assertion_kind
    if kind not in _ASSERTION_KIND_MAP:
        raise ValueError(
            f"unknown assertion_kind {kind!r}; "
            f"must be one of {sorted(_ASSERTION_KIND_MAP)}"
        )

    probe_kind, observation = _ASSERTION_KIND_MAP[kind]
    capability = premise.capability if premise.capability is not None else premise.text

    return {
        "capability": capability,
        "probe_kind": probe_kind,
        "fixture": premise.fixture,
        "expected": {
            "observation": observation,
            "match": premise.match,
        },
    }


# ---------------------------------------------------------------------------
# main() stub — returns 64 (EX_USAGE) until fully implemented
# ---------------------------------------------------------------------------

def main(argv: List[str]) -> int:
    """CLI entry-point.  Returns 64 (EX_USAGE) until subcommands are implemented."""
    return 64


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
