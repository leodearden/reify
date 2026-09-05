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
    synthesize <results.json>     — emit BatchVerdict JSON
                                    {blocks, blocking, report, malformed,
                                     fixture_absent, executed, total};
                                    exit 0 (nothing blocks) or 1 (blocks)

                                    Exit 0 is NOT by itself a clean pass:
                                    malformed / fixture-absent records do not
                                    block, so a batch can exit 0 having
                                    verified nothing.  Read `executed` against
                                    `total` before trusting it.

Reuses α (`prd-capability-check.py`) in-process via importlib — the same loader
pattern used by `test_prd_capability_check.py`.  α's file is NOT edited.

Design decisions:
  D1: negative-assertion mandate — a `rejection` premise binds probe_kind="check"
      with expected.observation="present" (NOT "absent").  The polarity is
      deterministic and unit-tested, NOT left to the LLM Enumerator.
  D2: `synthesize_batch` unions Prover + Adversary results; blocks on any
      FAIL / UNPROVABLE / HARNESS_ERROR; Adversary can only ADD blocking
      signals (net-positive recall, PRD decision 5).  A blocking verdict only
      counts when the record carries executed-probe evidence (a non-empty
      command AND a non-None exit_code) — PRD §6 decision 4 makes captured
      output mandatory on every verdict, so an evidence-free verdict is an
      unexecuted promise (a harness defect) and is reported as MALFORMED
      rather than tabulated as a premise falsification.
  D3: reuse α's exact output shape via importlib — no re-implementation.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import re
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
# normalize_command() — coerce a captured `command` field to a token list
# ---------------------------------------------------------------------------

def normalize_command(value: Any) -> List[str]:
    """Coerce an α result record's `command` field to a list of string tokens.

    α always emits `command` as a list of argv tokens, but a record can reach
    this harness from an LLM agent (Prover / Adversary) that returned a
    ready-to-paste shell STRING instead.  Rendering that with `" ".join(...)`
    joins it CHARACTER-by-character:

        " ".join("reify eval f.ri")  ->  "r e i f y   e v a l   f . r i"

    which destroys the captured evidence a human is meant to re-run (PRD §6
    decision 4).  Normalization rules:

        list / tuple  ->  [str(item) for item in value]   (argv tokens)
        str           ->  [value]                          (ONE token)
        anything else ->  []                               (no evidence)

    A string becomes a SINGLE-element list precisely so that `" ".join(...)`
    reproduces it verbatim.  `shlex.split` is deliberately NOT used: splitting
    would rewrite captured evidence into a re-quoted form that is no longer
    byte-identical to what was recorded, and a malformed quote would raise
    mid-report.  Captured evidence is reported, never re-derived.

    Args:
        value: The raw `command` field as received (any JSON-decoded type).

    Returns:
        A list of string tokens; empty when the value carries no command.
    """
    if isinstance(value, str):
        return [value]
    if isinstance(value, (list, tuple)):
        return [str(item) for item in value]
    return []


# ---------------------------------------------------------------------------
# BatchVerdict — output of synthesize_batch
# ---------------------------------------------------------------------------

# Verdict constants reused from α (imported via pcc above)
_BLOCKING_VERDICTS = frozenset({"FAIL", "UNPROVABLE", "HARNESS_ERROR"})


# Record classification (task #7257 ARM 1).  These are RECORD categories, NOT
# new verdicts — PRD §6 decision 3 fixes the verdict vocabulary at
# PASS / FAIL / UNPROVABLE (+ HARNESS_ERROR) and this harness does not extend it.
CAT_NON_BLOCKING = "NON_BLOCKING"   # verdict outside _BLOCKING_VERDICTS (incl. PASS
                                    # and premise-shaped records with no verdict key)
CAT_MALFORMED = "MALFORMED"         # blocking verdict with no executed-probe evidence
CAT_FIXTURE_ABSENT = "FIXTURE_ABSENT"   # probe ran but its fixture does not exist
CAT_BLOCKING = "BLOCKING"           # a real, evidence-backed falsification


def has_probe_evidence(rec: Dict[str, Any]) -> bool:
    """True iff the record carries evidence that a probe process actually ran.

    PRD §6 decision 4 makes captured output mandatory on every verdict: "Every
    D1 result carries the exact command + stdout/stderr + exit code, so a human
    (or D4) can re-derive the verdict without re-running."  The minimum
    re-derivable evidence is therefore BOTH:

      - a non-empty command (what was run), and
      - a non-None exit_code (that a process produced an outcome).

    `exit_code` is tested with `is not None`, NOT truthiness — exit_code 0 is a
    real, and very common, process outcome.

    Args:
        rec: An α --json result record (or anything shaped like one).

    Returns:
        True when both command and exit_code evidence are present.
    """
    if not normalize_command(rec.get("command")):
        return False
    return rec.get("exit_code") is not None


# stderr signatures that mean "the probe ran but its target file was not there".
# Both spellings occur: Rust's io::Error renders ENOENT as "(os error 2)", while
# Python/CLI wrappers render the strerror text.  Matched case-insensitively.
#
# The two are deliberately NOT the same kind of pattern.  The strerror text is a
# complete phrase, so a plain substring test carries no prefix hazard.  The
# errno rendering does: `"os error 2"` as a bare substring is a PREFIX test, not
# an errno test, and swallows ENOTDIR(20) / EISDIR(21) / EINVAL(22) /
# EMFILE(24) / ENOSPC(28) and the whole 2XX range.  The negative lookahead is
# what makes it mean ENOENT specifically.  It must be a lookahead rather than a
# required closing paren, because a bare trailing `os error 2` at end-of-string
# is a spelling that occurs in the wild and must still match.
_FIXTURE_ABSENT_PHRASE = "no such file or directory"
_FIXTURE_ABSENT_ERRNO_RE = re.compile(r"os error 2(?![0-9])")


def fixture_absent_evidence(rec: Dict[str, Any]) -> bool:
    """True iff the record's captured stderr says the probe target did not exist.

    Detection is STDERR-SIGNATURE based, not filesystem based, deliberately.
    Re-stat-ing the fixture here would be a time-of-check/time-of-run split: the
    synthesize step runs after the probes, potentially in a different working
    directory, a different worktree, or after the leaf's own deliverable has
    since been written.  A filesystem answer at synthesize time is therefore an
    answer to a different question than "could this probe find its target when
    it ran".  The captured stderr IS the record of what happened at run time,
    and PRD §6 decision 4 exists precisely so this is answerable from the
    record alone.

    Carve-out for α's binary-not-found sentinel: a missing `reify` binary makes
    the OS emit the SAME ENOENT text (α wraps it as
    `f"{_BINARY_NOT_FOUND_SENTINEL}: {exc}"`), but nothing was probed at all, so
    it is a real harness failure that must keep blocking.  The sentinel is read
    from α rather than re-declared as a literal so the two cannot drift.

    The errno signature is ANCHORED (`os error 2(?![0-9])`).  ENOENT is errno 2,
    and an unanchored `"os error 2"` substring is a prefix test that also fires
    on ENOTDIR(20), EISDIR(21), EINVAL(22), EMFILE(24), ENOSPC(28) and every
    3-digit 2XX errno — each a genuine, executed falsification that would
    silently stop blocking, inverting the carve-out into a hole that swallows
    real findings.  The lookahead (rather than a required closing paren) is
    load-bearing: a bare trailing `os error 2` at end-of-string is a real
    spelling and must still match.

    KNOWN LIMITATION (semantic half, not fixed here): the strerror PHRASE is
    still an unanchored substring test against arbitrary captured stderr, so a
    genuine FAIL whose EXPECTED diagnostic legitimately quotes that text — e.g.
    a `rejection` premise asserting that reify emits a good error message for an
    unresolvable import — is mis-routed to FIXTURE_ABSENT and stops blocking.
    The damage is bounded: a mis-route only ever downgrades BLOCKS to
    INCOMPLETE, never INCOMPLETE to PASS, because a fixture-absent record does
    not count as verifying.  The principled fix is for α to tag the record
    structurally at probe time instead of synthesize inferring intent from
    prose; filed as follow-up ticket tkt_0RTA2N4QBVAPDQ6AYE45GS0CP7.

    Args:
        rec: An α --json result record.

    Returns:
        True when the stderr carries a fixture-absent signature and is not α's
        binary-not-found sentinel.
    """
    stderr = str(rec.get("stderr", ""))
    if pcc._BINARY_NOT_FOUND_SENTINEL in stderr:
        return False
    lowered = stderr.lower()
    return (_FIXTURE_ABSENT_PHRASE in lowered
            or bool(_FIXTURE_ABSENT_ERRNO_RE.search(lowered)))


def classify_record(rec: Dict[str, Any]) -> str:
    """Classify a result record into one of the CAT_* categories.

    Precedence (first match wins):

      1. NON_BLOCKING   — the verdict is not one of FAIL / UNPROVABLE /
                          HARNESS_ERROR.  This covers PASS and also
                          premise-shaped records that carry no `verdict` key at
                          all (RESULTS_SCHEMA is loose enough today that a
                          premise validates as a result).  The evidence gate
                          deliberately does NOT re-litigate a non-blocking
                          record: it exists to stop unexecuted promises from
                          being tabulated as falsifications.
      2. MALFORMED      — a blocking verdict with no executed-probe evidence.
                          A harness defect, not a premise falsification.
      3. FIXTURE_ABSENT — the probe ran but its target file did not exist.
                          Ordered AFTER malformed so an evidence-free record is
                          never reclassified on the strength of a stderr string
                          no process produced.
      4. BLOCKING       — an evidence-backed falsification.

    Args:
        rec: An α --json result record.

    Returns:
        One of CAT_NON_BLOCKING / CAT_MALFORMED / CAT_FIXTURE_ABSENT /
        CAT_BLOCKING.
    """
    if rec.get("verdict", "") not in _BLOCKING_VERDICTS:
        return CAT_NON_BLOCKING
    if not has_probe_evidence(rec):
        return CAT_MALFORMED
    if fixture_absent_evidence(rec):
        return CAT_FIXTURE_ABSENT
    return CAT_BLOCKING


@dataclass
class BatchVerdict:
    """Result of synthesizing Prover + Adversary α result records.

    Fields:
        blocks         — True iff any evidence-backed FAIL / UNPROVABLE /
                         HARNESS_ERROR was found
        blocking       — capability strings for evidence-backed blocking probes
        report         — human/machine-readable string.  Line 1 is always a
                         machine-readable counts header stating total /
                         executed / blocking / malformed / fixture-absent;
                         labelled sections below it embed the captured evidence
                         (command, exit_code, stdout, stderr) per record
        malformed      — capability strings for blocking verdicts that carry NO
                         executed-probe evidence.  A harness defect, not a
                         falsification: these do NOT set `blocks`, but they mean
                         the batch was not actually verified.
        fixture_absent — capability strings whose probe could not run because
                         the fixture does not exist (populated from step-06).
        executed       — number of records carrying executed-probe evidence
        total          — total number of records synthesized

    `malformed` and `fixture_absent` default to empty and `executed`/`total` to
    0 so the historical 3-argument construction stays valid.

    NOTE for consumers: `blocks == False` alone is NOT a clean pass.  A batch
    with malformed or fixture-absent records was not verified; read `executed`
    against `total` before treating the batch as evidence of anything.
    """
    blocks: bool
    blocking: List[str]
    report: str
    malformed: List[str] = field(default_factory=list)
    fixture_absent: List[str] = field(default_factory=list)
    executed: int = 0
    total: int = 0


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

    Evidence gate (PRD §6 decision 4, task #7257 ARM 1):
        A blocking verdict is only tabulated as `blocking` when the record
        carries executed-probe evidence (see has_probe_evidence).  A blocking
        verdict with no evidence is an UNEXECUTED PROMISE — a harness defect —
        and is routed to `malformed` instead, where it is reported but does not
        set `blocks`.  Without this, one real falsification was buried among N
        vacuous ones and a human reading the report could not tell them apart.

    Fixture-absent carve-out (task #7257 ARM 1):
        A probe that ran but could not find its target file has falsified
        nothing — during decompose the fixture is very often the leaf's own
        deliverable.  Such records are routed to `fixture_absent`, reported
        under their own label, and do NOT set `blocks`.  Detection is
        stderr-signature based rather than filesystem based; see
        fixture_absent_evidence for the time-of-check/time-of-run rationale and
        for the binary-not-found carve-out that keeps a missing `reify` binary
        blocking.

    Args:
        role_results: dict with "prover" and "adversary" keys, each mapping to
                      a list of α --json result records.  Both keys are optional
                      (default to empty list).

    Returns:
        A BatchVerdict with blocks, blocking / malformed / fixture_absent
        capability lists, executed / total counters, and a report string
        embedding captured evidence for each non-passing probe.
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
    malformed: List[str] = []
    fixture_absent: List[str] = []
    report_parts: List[str] = []
    malformed_parts: List[str] = []
    fixture_absent_parts: List[str] = []
    executed = 0

    for rec in all_records:
        verdict = rec.get("verdict", "")
        capability = rec.get("capability", "<unknown>")
        role = rec.get("_role", "unknown")

        if has_probe_evidence(rec):
            executed += 1

        category = classify_record(rec)
        if category == CAT_NON_BLOCKING:
            continue

        # Captured evidence, normalized for rendering only — never rewritten.
        cmd_str = " ".join(normalize_command(rec.get("command")))
        exit_code = rec.get("exit_code", "?")
        stdout = rec.get("stdout", "")
        stderr = rec.get("stderr", "")

        if category == CAT_MALFORMED:
            malformed.append(capability)
            parts = [
                f"[{verdict}] {capability} (role: {role})",
                f"  command:   {cmd_str}",
                f"  exit_code: {exit_code}",
            ]
            if stdout:
                parts.append(f"  stdout:    {stdout}")
            if stderr:
                parts.append(f"  stderr:    {stderr}")
            malformed_parts.append("\n".join(parts))
            continue

        if category == CAT_FIXTURE_ABSENT:
            fixture_absent.append(capability)
            parts = [
                f"[{verdict}] {capability} (role: {role})",
                f"  command:   {cmd_str}",
                f"  exit_code: {exit_code}",
            ]
            if stderr:
                parts.append(f"  stderr:    {stderr}")
            fixture_absent_parts.append("\n".join(parts))
            continue

        blocking.append(capability)

        # Build evidence block for this blocking probe.
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

    # Machine-readable counts header, always first and always emitted — an
    # all-clear report must state the basis of its all-clear too.  Without it a
    # reader has to count report sections by hand to learn how much of the batch
    # was actually probed, which is how a report of N blocking capabilities got
    # read as N falsifications when only one had been executed.
    sections: List[str] = [
        f"records: {len(all_records)} total, {executed} with executed-probe evidence, "
        f"{len(blocking)} blocking, {len(malformed)} malformed, "
        f"{len(fixture_absent)} fixture-absent"
    ]
    if report_parts:
        sections.append("\n\n".join(report_parts))
    if malformed_parts:
        sections.append(
            "MALFORMED (no executed-probe evidence — harness defect, not a "
            "falsification):\n\n" + "\n\n".join(malformed_parts)
        )
    if fixture_absent_parts:
        sections.append(
            "FIXTURE ABSENT (probe could not run — the fixture is the leaf's own "
            "deliverable; not a falsification):\n\n"
            + "\n\n".join(fixture_absent_parts)
        )
    report = "\n\n".join(sections)

    return BatchVerdict(
        blocks=blocks,
        blocking=blocking,
        report=report,
        malformed=malformed,
        fixture_absent=fixture_absent,
        executed=executed,
        total=len(all_records),
    )


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

                    The emitted object carries:
                        blocks          bool  — any evidence-backed falsification
                        blocking        [str] — those capabilities
                        report          str   — captured evidence, by section
                        malformed       [str] — blocking verdicts with NO
                                                executed-probe evidence
                        fixture_absent  [str] — probes whose target file was missing
                        executed        int   — records with probe evidence
                        total           int   — records synthesized

    Exit codes:
        0   success (bind: OK; synthesize: nothing blocks)
        1   synthesize: at least one evidence-backed probe blocks
        64  usage / argument / IO / parse error  (sysexits EX_USAGE)

    IMPORTANT — exit 0 is NOT by itself a clean pass.  MALFORMED and
    fixture-absent records do not block, so a batch can exit 0 having verified
    nothing.  A caller that wants "verified" rather than merely "not falsified"
    must additionally require empty `malformed` and `fixture_absent` and read
    `executed` against `total`.
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
            "malformed": bv.malformed,
            "fixture_absent": bv.fixture_absent,
            "executed": bv.executed,
            "total": bv.total,
        }
        sys.stdout.write(json.dumps(output, indent=4))
        sys.stdout.write("\n")
        return 1 if bv.blocks else 0

    # Should not reach here
    sys.stderr.write(f"error: unknown subcommand {args.subcmd!r}\n")
    return 64


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
