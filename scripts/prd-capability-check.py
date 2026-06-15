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
    PASS          — observed matches expected
    FAIL          — observed contradicts expected
    UNPROVABLE    — observation is INDETERMINATE (only possible for ir kind)
    HARNESS_ERROR — probe tool error: missing binary, grammar load failure, etc.
                    Emitted verbatim in both text and --json output; always triggers exit 70.

Harness exit codes:
    0   all PASS
    1   ≥1 FAIL
    2   ≥1 UNPROVABLE, 0 FAIL
    64  usage / argument error (sysexits EX_USAGE)
    70  tool / runtime error (sysexits EX_SOFTWARE) — HARNESS_ERROR verdict
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

# Sentinel injected into stderr by run_probe() when the probe binary is not found
# (FileNotFoundError).  observe() checks for this sentinel before kind-specific
# logic so all probe kinds classify missing-binary as _HARNESS_ERROR.
_BINARY_NOT_FOUND_SENTINEL = "[HARNESS ERROR: binary not found]"


# ---------------------------------------------------------------------------
# Binary resolution helpers
# ---------------------------------------------------------------------------

def _find_repo_root() -> str:
    """Locate the reify repo root by walking up from this script.

    Returns the first ancestor directory that contains 'Cargo.toml' (the
    workspace manifest) or a '.git' directory.  Falls back to the parent
    of the scripts/ directory if neither is found within a reasonable depth.
    """
    # scripts/ lives one level below the repo root
    scripts_dir = os.path.dirname(os.path.abspath(__file__))
    candidate = os.path.dirname(scripts_dir)

    cur = candidate
    for _ in range(6):
        if os.path.exists(os.path.join(cur, "Cargo.toml")):
            return cur
        if os.path.exists(os.path.join(cur, ".git")):
            return cur
        parent = os.path.dirname(cur)
        if parent == cur:
            break
        cur = parent

    return candidate  # best-effort fallback


def _resolve_reify_bin(repo_root: Optional[str] = None) -> str:
    """Resolve the reify binary path.

    Resolution order:
        1. REIFY_BIN environment variable (if set and non-empty)
        2. <repo_root>/target/release/reify   (pre-built release)
        3. <repo_root>/target/debug/reify     (debug build)
        4. "reify"                             (from PATH)
    """
    env_bin = os.environ.get("REIFY_BIN")
    if env_bin:
        return env_bin

    root = repo_root or _find_repo_root()
    for rel in ("target/release/reify", "target/debug/reify"):
        candidate = os.path.join(root, rel)
        if os.path.isfile(candidate) and os.access(candidate, os.X_OK):
            return candidate

    return "reify"  # fallback: expect it on PATH


def _resolve_tree_sitter_bin() -> str:
    """Resolve the tree-sitter binary path.

    Resolution order:
        1. TREE_SITTER_BIN environment variable (if set and non-empty)
        2. "tree-sitter"                        (from PATH)
    """
    return os.environ.get("TREE_SITTER_BIN", "tree-sitter")


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

    if not isinstance(obj["probes"], list):
        raise ValueError(
            f"'probes' must be a list, got {type(obj['probes']).__name__}"
        )

    probes: List[Probe] = []
    for i, raw in enumerate(obj["probes"]):
        if not isinstance(raw, dict):
            raise ValueError(
                f"probe[{i}] must be an object (dict), got {type(raw).__name__}"
            )
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
_HARNESS_ERROR = "HARNESS_ERROR"


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
        exit 1 without "Failed to load language" in stderr → ABSENT
            (NOTE: tree-sitter with --quiet may suppress the "(ERROR …)" tree
             output entirely, so any exit 1 that is not a load-failure is
             classified as ABSENT, regardless of whether "(ERROR" appears)
        exit 1 with "Failed to load language" in stderr → _HARNESS_ERROR
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
    # Universal harness-error check: binary not found (any probe kind).
    # run_probe() injects _BINARY_NOT_FOUND_SENTINEL into stderr on FileNotFoundError
    # so all kinds surface missing binaries as _HARNESS_ERROR, not as ABSENT/FAIL.
    if _BINARY_NOT_FOUND_SENTINEL in run.stderr:
        return _HARNESS_ERROR

    if probe_kind == "grammar":
        if run.exit_code == 0:
            return PRESENT
        if "Failed to load language" in run.stderr:
            return _HARNESS_ERROR
        if run.exit_code == 1:
            # Parse error (the grammar produced ERROR nodes).  tree-sitter with
            # --quiet may suppress the "(ERROR ...)" tree output entirely, so we
            # classify any exit 1 without a load-failure stderr as ABSENT rather
            # than requiring "(ERROR" to appear in the combined output.
            return ABSENT
        # exit ≠ {0, 1} — unexpected; treat as harness error
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
        observation — PRESENT / ABSENT / INDETERMINATE / HARNESS_ERROR
        verdict     — PASS / FAIL / UNPROVABLE / HARNESS_ERROR (tool errors → exit 70)
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
        grammar  → [tree-sitter, parse, --quiet, <abs-fixture>]
        check    → [reify, check, <abs-fixture>]
        ir       → [reify, eval, <abs-fixture>]

    Fixture-path resolution: build_command() resolves probe.fixture to an
    absolute path via os.path.join(repo_root, probe.fixture) so that the path
    survives any CWD change — in particular, grammar probes run with
    CWD=<repo_root>/tree-sitter-reify/ and a relative fixture would resolve
    incorrectly under that directory.  If probe.fixture is already absolute it
    is used verbatim.  repo_root defaults to _find_repo_root() when None.

    The same repo_root is passed to _resolve_reify_bin() so the binary and the
    fixture are resolved consistently.  run_probe() calls build_command() with
    the same repo_root it uses for CWD, so the recorded evidence command and
    the executed argv are identical.

    Args:
        probe:     The probe to build a command for.
        repo_root: Repo root directory for resolving relative fixture paths and
                   locating the reify binary.  Defaults to _find_repo_root().

    Returns:
        A list of strings — the exact argv to be passed to subprocess.run().
    """
    root = repo_root if repo_root is not None else _find_repo_root()

    # Resolve fixture to absolute path so it survives CWD changes.
    fixture = probe.fixture
    if not os.path.isabs(fixture):
        fixture = os.path.join(root, fixture)

    if probe.probe_kind == "grammar":
        ts_bin = _resolve_tree_sitter_bin()
        return [ts_bin, "parse", "--quiet", fixture]

    reify_bin = _resolve_reify_bin(repo_root=root)

    if probe.probe_kind == "check":
        return [reify_bin, "check", fixture]

    if probe.probe_kind == "ir":
        return [reify_bin, "eval", fixture]

    # Should not reach here after load_probe_set validation, but be defensive.
    raise ValueError(f"unknown probe_kind: {probe.probe_kind!r}")


# ---------------------------------------------------------------------------
# run_probe() — the real subprocess runner
# ---------------------------------------------------------------------------

def run_probe(probe: Probe) -> ProbeRun:
    """Run a probe command in a subprocess and return captured output.

    Delegates command construction to build_command(), which resolves
    probe.fixture to an absolute path so it survives the CWD change.
    Grammar probes run with CWD = <repo_root>/tree-sitter-reify/ so that
    the tree-sitter parser can locate its grammar (src/parser.c must be
    generated first).  build_command() uses the same repo_root so the
    recorded fixture path and the executed path are identical.

    FileNotFoundError (missing binary) is caught and represented as a ProbeRun
    with _BINARY_NOT_FOUND_SENTINEL in stderr and exit_code=127.  observe()
    detects this sentinel and returns _HARNESS_ERROR for any probe kind.

    Args:
        probe: The Probe to run.

    Returns:
        A ProbeRun with exit_code, stdout, and stderr from the subprocess.
        On FileNotFoundError, the sentinel is embedded in stderr so that
        observe() can classify it as a harness error.
    """
    repo_root = _find_repo_root()
    cmd = build_command(probe, repo_root=repo_root)

    # Grammar probes must run with CWD inside tree-sitter-reify/ so that
    # `tree-sitter parse` can resolve the reify grammar (the grammar dir
    # contains the package.json that points tree-sitter at the grammar).
    cwd: Optional[str] = None
    if probe.probe_kind == "grammar":
        cwd = os.path.join(repo_root, "tree-sitter-reify")

    try:
        proc = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            cwd=cwd,
        )
        return ProbeRun(
            exit_code=proc.returncode,
            stdout=proc.stdout,
            stderr=proc.stderr,
        )
    except FileNotFoundError as exc:
        # Binary not found: signal to observe() via the sentinel in stderr.
        return ProbeRun(
            exit_code=127,
            stdout="",
            stderr=f"{_BINARY_NOT_FOUND_SENTINEL}: {exc}",
        )


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
        runner = run_probe

    # Build the command with the same repo_root that run_probe() uses, so the
    # recorded evidence command matches the actually-executed command exactly.
    repo_root = _find_repo_root()
    cmd = build_command(probe, repo_root=repo_root)

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
# harness_exit_code() — aggregate results into a harness exit code
# ---------------------------------------------------------------------------

def harness_exit_code(results: List[Result]) -> int:
    """Compute the harness exit code from a list of Results.

    Precedence (highest to lowest):
        70  — ≥1 harness-error result  (tool/runtime error; sysexits EX_SOFTWARE)
         1  — ≥1 FAIL result           (at least one probe contradicts expectation)
         2  — ≥1 UNPROVABLE, 0 FAIL   (at least one probe is indeterminate)
         0  — all PASS

    Note: 64 (usage error) is returned by main() for bad arguments, not here.

    Args:
        results: List of Result objects from evaluate().

    Returns:
        An integer exit code: 0, 1, 2, or 70.
    """
    verdicts = [r.verdict for r in results]

    if _HARNESS_ERROR in verdicts:
        return 70

    if FAIL in verdicts:
        return 1

    if UNPROVABLE in verdicts:
        return 2

    return 0


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
    """CLI entry-point.  Returns an exit code (0/1/2/64/70).

    Usage:
        prd-capability-check.py [--json] PROBE_SET_JSON

    Reads the committed probe-set JSON from PROBE_SET_JSON, evaluates every
    probe with the real runner, prints per-probe evidence (or --json), and
    returns harness_exit_code(results).

    Exit codes:
        0   all PASS
        1   ≥1 FAIL
        2   ≥1 UNPROVABLE, 0 FAIL
        64  usage / argument / IO error (EX_USAGE)
        70  tool / runtime error — missing binary, grammar load failure (EX_SOFTWARE)
    """
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
        args = parser.parse_args(argv)
    except SystemExit as e:
        code = e.code if isinstance(e.code, int) else 64
        return 0 if code == 0 else 64

    # --- Load probe set from file ---
    try:
        with open(args.probe_set) as fh:
            text = fh.read()
    except OSError as exc:
        sys.stderr.write(f"error: cannot read probe set '{args.probe_set}': {exc}\n")
        return 64  # EX_USAGE

    try:
        probes = load_probe_set(text)
    except ValueError as exc:
        sys.stderr.write(f"error: invalid probe set: {exc}\n")
        return 64  # EX_USAGE

    if not probes:
        sys.stderr.write(
            f"error: probe set '{args.probe_set}' contains no probes; "
            "an empty probe set is likely a misconfiguration.\n"
        )
        return 64  # EX_USAGE — vacuous all-pass masks broken CI gates

    # --- Evaluate each probe with the real runner ---
    results = [evaluate(probe) for probe in probes]

    # --- Emit output ---
    if args.emit_json:
        json_records = [
            {
                "capability": r.probe.capability,
                "probe_kind": r.probe.probe_kind,
                "verdict": r.verdict,
                "command": r.command,
                "exit_code": r.exit_code,
                "stdout": r.stdout,
                "stderr": r.stderr,
            }
            for r in results
        ]
        sys.stdout.write(json.dumps({"results": json_records}, indent=2))
        sys.stdout.write("\n")
    else:
        for r in results:
            cmd_str = " ".join(r.command)
            stdout_preview = r.stdout[:200] if r.stdout else "(empty)"
            stderr_preview = r.stderr[:200] if r.stderr else "(empty)"
            sys.stdout.write(f"[{r.verdict}] {r.probe.capability}\n")
            sys.stdout.write(f"  kind:      {r.probe.probe_kind}\n")
            sys.stdout.write(f"  fixture:   {r.probe.fixture}\n")
            sys.stdout.write(f"  command:   {cmd_str}\n")
            sys.stdout.write(f"  exit_code: {r.exit_code}\n")
            sys.stdout.write(f"  stdout:    {stdout_preview}\n")
            sys.stdout.write(f"  stderr:    {stderr_preview}\n")
            sys.stdout.write("\n")

    return harness_exit_code(results)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
