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
# bind_premises() — list of Premise → α probe-set dict
# ---------------------------------------------------------------------------

def bind_premises(premises: List[Premise]) -> Dict[str, Any]:
    """Bind a list of Premise records to an α committed-probe-set dict.

    Maps each premise through premise_to_probe and assembles the result into
    the α probe-set format accepted by pcc.load_probe_set:
        {"probes": [probe_dict, ...]}

    Negative-assertion enforcement:
        A `rejection` premise with an empty fixture string is a gap — a
        rejection assertion with no probe target would silently pass, masking
        the missing check.  Such a premise raises ValueError.

    Validation:
        The assembled probe-set is validated by pcc.load_probe_set so any
        binding error (unknown probe_kind, missing observation) surfaces here
        rather than at probe-run time.

    Args:
        premises: List of Premise records (may be empty → empty probe-set).

    Returns:
        A dict {"probes": [...]}.  Round-trips through pcc.load_probe_set.

    Raises:
        ValueError: if a rejection premise has an empty fixture path, or if
                    the assembled probe dict is rejected by α's validation.
    """
    probes = []
    for premise in premises:
        # Negative-assertion mandate: a rejection with no fixture is a gap.
        if premise.assertion_kind == "rejection" and not premise.fixture.strip():
            raise ValueError(
                f"rejection premise {premise.text!r} has no fixture path — "
                "a rejection assertion with no probe target is a gap, not a pass"
            )

        # Negative-assertion mandate: a rejection with an empty match dict is
        # satisfied unconditionally (α's match_predicate returns True for {}) —
        # reify can silently accept and the probe still PASSes.  That is the
        # exact 4575 silent-accept class this harness exists to catch.
        if premise.assertion_kind == "rejection" and not premise.match:
            raise ValueError(
                f"rejection premise {premise.text!r} has no match constraints — "
                "an empty match is satisfied unconditionally and cannot detect "
                "silent-accept (add e.g. match={{\"exit_code\": 1}} or "
                "match={{\"exit_code\": 1, \"stderr_contains\": \"<diag>\"}})"
            )

        probe = premise_to_probe(premise)
        probes.append(probe)

    probe_set = {"probes": probes}

    # Validate through α so binding errors surface here, not at probe-run time.
    if probes:
        pcc.load_probe_set(json.dumps(probe_set))  # raises ValueError on invalid shape

    return probe_set


# ---------------------------------------------------------------------------
# BatchVerdict — output of synthesize_batch
# ---------------------------------------------------------------------------

# Verdict constants reused from α (imported via pcc above)
_BLOCKING_VERDICTS = frozenset({"FAIL", "UNPROVABLE", "HARNESS_ERROR"})


@dataclass
class BatchVerdict:
    """Result of synthesizing Prover + Adversary α result records.

    Fields:
        blocks    — True iff any FAIL / UNPROVABLE / HARNESS_ERROR was found
        blocking  — list of capability strings for blocking probes
        report    — human/machine-readable string embedding captured evidence
                    (command, exit_code, stdout, stderr) per blocking probe
    """
    blocks: bool
    blocking: List[str]
    report: str


# ---------------------------------------------------------------------------
# synthesize_batch() — union Prover + Adversary → BatchVerdict
# ---------------------------------------------------------------------------

def synthesize_batch(role_results: Dict[str, List[Dict[str, Any]]]) -> BatchVerdict:
    """Synthesize Prover + Adversary α result records into a BatchVerdict.

    Consumes α's --json result records (shape:
        {capability, probe_kind, verdict, command, exit_code, stdout, stderr}
    ) keyed by role ("prover" / "adversary").

    Union semantics (PRD decision 5):
        - Block on any FAIL / UNPROVABLE / HARNESS_ERROR from either role.
        - Adversary can only ADD blocking signals; it never clears a Prover FAIL.
        - An all-PASS Adversary does NOT clear a Prover FAIL.

    Args:
        role_results: dict with "prover" and "adversary" keys, each mapping to
                      a list of α --json result records.  Both keys are optional
                      (default to empty list).

    Returns:
        A BatchVerdict with blocks, blocking list, and a report string
        embedding captured evidence for each blocking probe.
    """
    prover_records = role_results.get("prover", [])
    adversary_records = role_results.get("adversary", [])

    # Union all records; track role for the report.
    all_records: List[Dict[str, Any]] = []
    for rec in prover_records:
        all_records.append(dict(rec, _role="prover"))
    for rec in adversary_records:
        all_records.append(dict(rec, _role="adversary"))

    blocking: List[str] = []
    report_parts: List[str] = []

    for rec in all_records:
        verdict = rec.get("verdict", "")
        if verdict in _BLOCKING_VERDICTS:
            capability = rec.get("capability", "<unknown>")
            role = rec.get("_role", "unknown")
            blocking.append(capability)

            # Build evidence block for this blocking probe.
            cmd_str = " ".join(rec.get("command", []))
            exit_code = rec.get("exit_code", "?")
            stdout = rec.get("stdout", "")
            stderr = rec.get("stderr", "")

            parts = [
                f"[{verdict}] {capability} (role: {role})",
                f"  command:   {cmd_str}",
                f"  exit_code: {exit_code}",
            ]
            if stdout:
                parts.append(f"  stdout:    {stdout}")
            if stderr:
                parts.append(f"  stderr:    {stderr}")

            report_parts.append("\n".join(parts))

    blocks = len(blocking) > 0
    report = "\n\n".join(report_parts) if report_parts else ""

    return BatchVerdict(blocks=blocks, blocking=blocking, report=report)


# ---------------------------------------------------------------------------
# _parse_premises_file() — read premises JSON and return list of Premise
# ---------------------------------------------------------------------------

def _parse_premises_file(path: str) -> List[Premise]:
    """Read a premises JSON file and return a list of Premise objects.

    Premises file format:
        {
            "premises": [
                {
                    "text": "...",
                    "assertion_kind": "rejection|parses|resolves|produces|ir",
                    "fixture": "...",
                    "match": {...},
                    "capability": "..."   // optional
                }
            ]
        }

    Raises:
        OSError: if the file cannot be read.
        ValueError: if the JSON is invalid or missing required fields.
    """
    try:
        with open(path) as fh:
            data = json.load(fh)
    except json.JSONDecodeError as e:
        raise ValueError(f"premises file {path!r} is not valid JSON: {e}") from e

    if not isinstance(data, dict) or "premises" not in data:
        raise ValueError(
            f"premises file {path!r} must be an object with a top-level 'premises' key"
        )

    premises = []
    for i, rec in enumerate(data["premises"]):
        if not isinstance(rec, dict):
            raise ValueError(f"premises[{i}] must be an object (dict)")
        for field_name in ("text", "assertion_kind", "fixture"):
            if field_name not in rec:
                raise ValueError(f"premises[{i}] is missing required field {field_name!r}")
        premises.append(Premise(
            text=rec["text"],
            assertion_kind=rec["assertion_kind"],
            fixture=rec["fixture"],
            match=rec.get("match", {}),
            capability=rec.get("capability"),
        ))
    return premises


# ---------------------------------------------------------------------------
# _parse_results_file() — read synthesize input JSON
# ---------------------------------------------------------------------------

def _parse_results_file(path: str) -> Dict[str, List[Dict[str, Any]]]:
    """Read a results JSON file and return a role_results dict.

    Results file format:
        {
            "prover": [...α result records...],
            "adversary": [...α result records...]
        }

    Both keys are optional (default to empty list).

    Raises:
        OSError: if the file cannot be read.
        ValueError: if the JSON is invalid.
    """
    try:
        with open(path) as fh:
            data = json.load(fh)
    except json.JSONDecodeError as e:
        raise ValueError(f"results file {path!r} is not valid JSON: {e}") from e

    if not isinstance(data, dict):
        raise ValueError(f"results file {path!r} must be a JSON object")

    return {
        "prover": data.get("prover", []),
        "adversary": data.get("adversary", []),
    }


# ---------------------------------------------------------------------------
# main() — CLI entry-point with bind / synthesize subcommands
# ---------------------------------------------------------------------------

def main(argv: List[str]) -> int:
    """CLI entry-point.  Returns an exit code (0/1/64).

    Usage:
        prd-decompose-verify.py bind      <premises.json>
        prd-decompose-verify.py synthesize <results.json>

    Subcommands:
        bind        Read {"premises":[...]} from premises.json, emit α probe-set
                    JSON to stdout.  Exit 0 on success, 64 on usage/IO/parse error.

        synthesize  Read {"prover":[...], "adversary":[...]} result records from
                    results.json, emit BatchVerdict JSON to stdout.
                    Exit 0 if nothing blocks, 1 if blocks, 64 on error.

    Exit codes:
        0   success (bind: OK; synthesize: all pass)
        1   synthesize: at least one probe blocks
        64  usage / argument / IO / parse error  (sysexits EX_USAGE)
    """
    parser = argparse.ArgumentParser(
        prog="prd-decompose-verify.py",
        description="Deterministic harness for γ decompose-phase verification.",
    )
    sub = parser.add_subparsers(dest="subcmd")

    # bind subcommand
    bind_p = sub.add_parser("bind", help="Bind premises to an α probe-set JSON.")
    bind_p.add_argument("premises", metavar="PREMISES_JSON",
                        help="Path to a {premises:[...]} JSON file.")

    # synthesize subcommand
    syn_p = sub.add_parser("synthesize",
                            help="Synthesize α result records into a BatchVerdict.")
    syn_p.add_argument("results", metavar="RESULTS_JSON",
                       help="Path to a {prover:[...], adversary:[...]} results JSON file.")

    try:
        args = parser.parse_args(argv)
    except SystemExit as e:
        code = e.code if isinstance(e.code, int) else 64
        return 0 if code == 0 else 64

    if args.subcmd is None:
        sys.stderr.write("error: a subcommand is required: bind | synthesize\n")
        parser.print_help(sys.stderr)
        return 64

    # ── bind ──────────────────────────────────────────────────────────────────
    if args.subcmd == "bind":
        try:
            premises = _parse_premises_file(args.premises)
        except OSError as exc:
            sys.stderr.write(f"error: cannot read premises file: {exc}\n")
            return 64
        except ValueError as exc:
            sys.stderr.write(f"error: {exc}\n")
            return 64

        try:
            probe_set = bind_premises(premises)
        except ValueError as exc:
            sys.stderr.write(f"error: {exc}\n")
            return 64

        sys.stdout.write(json.dumps(probe_set, indent=4))
        sys.stdout.write("\n")
        return 0

    # ── synthesize ────────────────────────────────────────────────────────────
    if args.subcmd == "synthesize":
        try:
            role_results = _parse_results_file(args.results)
        except OSError as exc:
            sys.stderr.write(f"error: cannot read results file: {exc}\n")
            return 64
        except ValueError as exc:
            sys.stderr.write(f"error: {exc}\n")
            return 64

        bv = synthesize_batch(role_results)

        output = {
            "blocks": bv.blocks,
            "blocking": bv.blocking,
            "report": bv.report,
        }
        sys.stdout.write(json.dumps(output, indent=4))
        sys.stdout.write("\n")
        return 1 if bv.blocks else 0

    # Should not reach here
    sys.stderr.write(f"error: unknown subcommand {args.subcmd!r}\n")
    return 64


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
