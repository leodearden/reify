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
# Observation and verdict constants
# ---------------------------------------------------------------------------

# Observation values (what the probe runner sees when it runs a probe)
PRESENT = "present"            # the asserted capability/behavior is present
ABSENT = "absent"              # the asserted capability/behavior is absent
INDETERMINATE = "indeterminate"  # cannot determine (ir kind: unrelated error)

# Verdict values (result of comparing observed vs expected)
PASS = "PASS"
FAIL = "FAIL"
UNPROVABLE = "UNPROVABLE"

# ---------------------------------------------------------------------------
# Valid constants for validation
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


# ---------------------------------------------------------------------------
# ProbeRun — captured subprocess output
# ---------------------------------------------------------------------------

@dataclass
class ProbeRun:
    """Captured output from running a probe command."""
    exit_code: int
    stdout: str
    stderr: str


# ---------------------------------------------------------------------------
# Observation logic: match_predicate + observe()
# ---------------------------------------------------------------------------

# Harness-error sentinel — an internal signal meaning "the observation cannot
# be trusted because the probe tool itself failed" (e.g. grammar load-failure).
# Never returned as a public PRESENT/ABSENT observation.
_HARNESS_ERROR = "_harness_error"


def match_predicate(run: ProbeRun, match: Dict[str, Any]) -> bool:
    """Return True iff all fields in the match dict are satisfied by `run`.

    All set fields must hold simultaneously (AND semantics).
    An empty match dict {} is always satisfied (no criterion).

    Supported fields:
        exit_code (int)     — run.exit_code must equal this value
        stderr_contains (str) — this string must appear in run.stderr
        stdout_contains (str) — this string must appear in run.stdout
    """
    if "exit_code" in match:
        if run.exit_code != match["exit_code"]:
            return False
    if "stderr_contains" in match:
        if match["stderr_contains"] not in run.stderr:
            return False
    if "stdout_contains" in match:
        if match["stdout_contains"] not in run.stdout:
            return False
    return True


def observe(probe_kind: str, run: ProbeRun, match: Dict[str, Any]) -> str:
    """Determine observation (PRESENT/ABSENT/INDETERMINATE or _HARNESS_ERROR).

    grammar:
        exit 0 → PRESENT (no parse errors)
        exit 1 + "(ERROR" in combined output → ABSENT
        exit 1 + "Failed to load language" in stderr → _HARNESS_ERROR
        any other exit → _HARNESS_ERROR

    check:
        match predicate satisfied → PRESENT
        match predicate not satisfied → ABSENT

    ir (eval-error proxy, asymmetric):
        exit 0 → ABSENT  (sound by determinism; §6 G6(b))
        exit ≠ 0, asserted signature (stderr_contains in match) in stderr → PRESENT
        exit ≠ 0, signature absent → INDETERMINATE

    Args:
        probe_kind: "grammar", "check", or "ir".
        run: Captured subprocess output (exit_code, stdout, stderr).
        match: Match predicate dict from the probe's expected.match field.

    Returns:
        PRESENT, ABSENT, INDETERMINATE, or _HARNESS_ERROR.
    """
    if probe_kind == "grammar":
        combined = run.stdout + run.stderr
        if run.exit_code == 0:
            return PRESENT
        if "Failed to load language" in run.stderr:
            return _HARNESS_ERROR
        if "(ERROR" in combined:
            return ABSENT
        # exit ≠ {0, 1} or unexpected content — treat as harness error
        return _HARNESS_ERROR

    if probe_kind == "check":
        return PRESENT if match_predicate(run, match) else ABSENT

    if probe_kind == "ir":
        if run.exit_code == 0:
            return ABSENT
        # exit ≠ 0: check for the asserted signature in stderr
        sig = match.get("stderr_contains")
        if sig and sig in run.stderr:
            return PRESENT
        return INDETERMINATE

    # Unknown kind — this shouldn't happen after validation, but be safe
    return _HARNESS_ERROR


# ---------------------------------------------------------------------------
# Result — evaluation output with mandatory captured evidence
# ---------------------------------------------------------------------------

@dataclass
class Result:
    """Output of evaluating a single probe, with mandatory captured evidence.

    Fields:
        probe       — the original Probe object
        command     — exact command argv that was run (list of strings)
        exit_code   — process exit code
        stdout      — captured stdout text
        stderr      — captured stderr text
        observation — PRESENT / ABSENT / INDETERMINATE / _HARNESS_ERROR
        verdict     — PASS / FAIL / UNPROVABLE  (or _HARNESS_ERROR for tool errors)
    """
    probe: Probe
    command: List[str]
    exit_code: int
    stdout: str
    stderr: str
    observation: str
    verdict: str


# ---------------------------------------------------------------------------
# build_command() — construct probe argv from probe kind and fixture
# ---------------------------------------------------------------------------

def build_command(probe: Probe, repo_root: Optional[str] = None) -> List[str]:
    """Construct the exact command argv for a probe.

    Binary resolution (used by run_probe; also injectable via env overrides):
        grammar  → TREE_SITTER_BIN (default "tree-sitter")
        check/ir → REIFY_BIN (default "reify")

    Command shapes:
        grammar  → [tree-sitter, parse, --quiet, <fixture>]
        check    → [reify, check, <fixture>]
        ir       → [reify, eval, <fixture>]

    The fixture path in the command is as-given in the probe record
    (repo-relative).  run_probe() resolves it to an absolute path and
    sets CWD for grammar probes; build_command() returns the logical argv
    for display and testing purposes.

    Args:
        probe:     The probe to build a command for.
        repo_root: Optional repo root for resolving fixture paths.  Unused
                   by this function directly; provided for forward-compat
                   with step-10's full resolution logic.

    Returns:
        A list of strings — the exact argv to be passed to subprocess.run().
    """
    fixture = probe.fixture

    if probe.probe_kind == "grammar":
        ts_bin = os.environ.get("TREE_SITTER_BIN", "tree-sitter")
        return [ts_bin, "parse", "--quiet", fixture]

    reify_bin = os.environ.get("REIFY_BIN", "reify")

    if probe.probe_kind == "check":
        return [reify_bin, "check", fixture]

    if probe.probe_kind == "ir":
        return [reify_bin, "eval", fixture]

    # Should not reach here after load_probe_set validation, but be defensive.
    raise ValueError(f"unknown probe_kind: {probe.probe_kind!r}")


# ---------------------------------------------------------------------------
# evaluate() — wire run → observe → verdict → Result
# ---------------------------------------------------------------------------

def evaluate(probe: Probe, runner: Any = None) -> Result:
    """Evaluate a single probe and return a Result with mandatory evidence.

    Args:
        probe:  The probe to evaluate.
        runner: A callable (probe) -> ProbeRun.  Defaults to the real
                subprocess runner (run_probe), which is injected in step-10.
                Tests pass synthetic runners for hermeticity.

    Returns:
        A Result carrying the exact command, captured exit/stdout/stderr,
        observation, and verdict.
    """
    if runner is None:
        # Default: the real subprocess runner (implemented in step-10).
        runner = run_probe  # type: ignore[name-defined]  # noqa: F821

    # Build the command argv for this probe (used for display/evidence).
    cmd = build_command(probe)

    # Run the probe and capture output.
    run = runner(probe)

    # Determine observation.
    match = probe.expected.get("match", {})
    obs = observe(probe.probe_kind, run, match)

    # Determine verdict.
    if obs == _HARNESS_ERROR:
        verd = _HARNESS_ERROR
    else:
        expected_obs = probe.expected["observation"]
        verd = verdict(obs, expected_obs)

    return Result(
        probe=probe,
        command=cmd,
        exit_code=run.exit_code,
        stdout=run.stdout,
        stderr=run.stderr,
        observation=obs,
        verdict=verd,
    )


# ---------------------------------------------------------------------------
# Verdict logic
# ---------------------------------------------------------------------------

def verdict(observation: str, expected_observation: str) -> str:
    """Return PASS/FAIL/UNPROVABLE by comparing observation to expected.

    Truth table:
        PRESENT  + expected present  → PASS
        ABSENT   + expected absent   → PASS
        PRESENT  + expected absent   → FAIL
        ABSENT   + expected present  → FAIL
        INDETERMINATE + (any)        → UNPROVABLE

    Args:
        observation: One of PRESENT, ABSENT, INDETERMINATE.
        expected_observation: "present" or "absent" (from the probe's expected field).

    Returns:
        PASS, FAIL, or UNPROVABLE.
    """
    if observation == INDETERMINATE:
        return UNPROVABLE
    if observation == expected_observation:
        return PASS
    return FAIL


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
