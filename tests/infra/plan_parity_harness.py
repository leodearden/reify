"""
plan_parity_harness.py — B1, the plan-parity runner: bash `scripts/verify.sh`
and Python `scripts/verify.py` planning one scenario side by side in the
shared clone of the tree under test (S18 γ1, task 9098; PRD
docs/prds/v0_6/verify-structured-plan-and-role-table.md D8, D13).

Not a test module itself (no `test_` prefix): run_all.sh never discovers it,
and the plan-parity members import it via the sys.path idiom.

Both sides run under one identical env: base_env() (no REIFY_*/DF_*), no
nextest retry sleep, and the two jobserver FIFO knobs pinned to a path that is
never a FIFO, so a live balancer restart cannot change a plan mid-grid. A
scenario overrides any of these; an override of None unsets the variable.

compare() is byte-for-byte on exit code, stdout and stderr. Its message names
the first differing line, the plan section it sits in, the module of
scripts/reify_verify/ that produces that section, and the port-window rule.

EXPECTED_DIVERGENCES is the whole list of differences B1 accepts: PRD D8's
listed divergences that γ1's corpus reaches, each asserted positively both
ways by check_divergence(). Every other scenario compares byte-equal; there
is no per-scenario opt-out.

GridParity runs one action's full grid (PRD D13: role × profile × scope, with
--include-infra) in two clone states; each grid shard member subclasses it.
"""

import dataclasses
import itertools
import os
import re
import sys
from collections.abc import Callable, Mapping
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(INFRA_DIR))
from scope_record_harness import (  # noqa: E402
    CONCURRENT_READS,
    Clone,
    InClone,
    Outcome,
    run_command,
    base_env,
    discard_clone,
    the_clone,
)
from tree_under_test import shared_clone  # noqa: E402

__all__ = (
    "Clone",
    "EXPECTED_DIVERGENCES",
    "PORT_WINDOW_RULE",
    "Scenario",
    "Pair",
    "Divergence",
    "ExpectedDivergence",
    "GridParity",
    "InParityClone",
    "base_env",
    "check_divergence",
    "compare",
    "discard_clone",
    "grid_scenarios",
    "plan_sections",
    "run_both",
    "run_many",
    "shared_clone",
    "shared_env",
    "the_clone",
)

PORT_WINDOW_RULE = (
    "PRD D13 — from γ1 to ζ any scripts/verify.sh plan change must be mirrored "
    "in scripts/reify_verify/ in the same diff."
)
ENVIRONMENT_MARKER = "# --- environment"
COMMANDS_MARKER = "# --- commands"
SECTIONS = ("exit code", "header", "environment", "commands", "stderr")
JOBSERVER_FIFO_KNOBS = ("REIFY_JOBSERVER_TASK_FIFO", "REIFY_JOBSERVER_MERGE_FIFO")

HEADER_HINTS = (
    ("@@REIFY_RETRY_SCOPE=", "build.py (the retry marker's counts) / render.py (its text)"),
    ("# scope decision — ", "scope.py"),
    ("# narrowing — ", "narrowing.py"),
    ("# heavy partition — ", "build.py"),
)
STDERR_HINTS = (
    (re.compile(r"stale jobserver FIFO"), "environment.py"),
    (re.compile(r"cargo-nextest|cargo nextest"), "environment.py"),
    (re.compile(r"REIFY_VERIFY_[A-Z_]*TIMEOUT"), "knobs.py"),
    (re.compile(r"nice|ionice|priority"), "policy.py"),
    (re.compile(r"MERGE_HEAD|--scope|merge-base|forcing"), "scope.py"),
    (re.compile(r"REIFY_AFFECTED_CRATES_OVERRIDE|affected|narrow"), "narrowing.py"),
    (re.compile(r"REIFY_GUI_RETRY_SPECS"), "build.py"),
)


@dataclass(frozen=True)
class Scenario:
    """One verify invocation planned by both sides.

    env: overrides of the shared env; a None value unsets that variable.
    setup: run on the clone before both sides plan; may return more env
    overrides (e.g. a scratch HOME it built).
    cwd: the directory both sides run from, relative to the clone root.
    """

    label: str
    argv: tuple[str, ...]
    env: Mapping[str, str | None] = field(default_factory=dict)
    setup: Callable[[Clone], Mapping[str, str | None] | None] | None = None
    cwd: str | None = None


@dataclass(frozen=True)
class Pair:
    bash: Outcome
    py: Outcome


@dataclass(frozen=True)
class Divergence:
    section: str
    message: str


def shared_env(clone: Clone, overrides: Mapping[str, str | None] | None = None) -> dict:
    """The one env both sides of a scenario run under."""
    unpinned = clone.root.parent / "no-jobserver-fifo"
    env = base_env(
        REIFY_NEXTEST_PROBE_RETRY_SLEEP="0",
        **{knob: str(unpinned / knob.lower()) for knob in JOBSERVER_FIFO_KNOBS},
    )
    for name, value in dict(overrides or {}).items():
        if value is None:
            env.pop(name, None)
        else:
            env[name] = value
    return env


def _prepared(clone: Clone, scenario: Scenario) -> tuple[dict, Path]:
    """(env, cwd) for `scenario`, after its setup has run on `clone`."""
    overrides = dict(scenario.env)
    if scenario.setup is not None:
        overrides.update(scenario.setup(clone) or {})
    cwd = clone.root / scenario.cwd if scenario.cwd else clone.root
    return shared_env(clone, overrides), cwd


def _run_bash(clone: Clone, scenario: Scenario, env, cwd) -> Outcome:
    verify_sh = os.path.relpath(clone.root / "scripts" / "verify.sh", cwd)
    return run_command(["bash", verify_sh, *scenario.argv], env, cwd)


def _run_py(clone: Clone, scenario: Scenario, env, cwd) -> Outcome:
    verify_py = clone.root / "scripts" / "verify.py"
    return run_command([sys.executable, "-I", "-B", str(verify_py), *scenario.argv], env, cwd)


def run_bash(clone: Clone, scenario: Scenario) -> Outcome:
    """Run `scenario` through bash verify.sh alone, in `clone`."""
    return _run_bash(clone, scenario, *_prepared(clone, scenario))


def run_both(clone: Clone, scenario: Scenario) -> Pair:
    """Run `scenario` through bash verify.sh, then verify.py, in `clone`."""
    env, cwd = _prepared(clone, scenario)
    return Pair(_run_bash(clone, scenario, env, cwd), _run_py(clone, scenario, env, cwd))


def run_many(clone: Clone, scenarios) -> list[tuple[Scenario, Pair]]:
    """run_both over setup-free scenarios, concurrently; print-plan never mutates the clone."""
    scenarios = list(scenarios)
    assert all(s.setup is None for s in scenarios), "a scenario with setup must run alone"
    with ThreadPoolExecutor(max_workers=CONCURRENT_READS) as pool:
        pairs = list(pool.map(lambda s: run_both(clone, s), scenarios))
    return list(zip(scenarios, pairs))


def plan_sections(stdout: str) -> dict[str, list[str]]:
    """{header, environment, commands}: stdout split at the two markers.

    Each marker line opens its own section; a section absent from `stdout`
    (a refusal, an error) is an empty list.
    """
    sections = {"header": [], "environment": [], "commands": []}
    current = "header"
    for line in stdout.splitlines():
        if line.startswith(ENVIRONMENT_MARKER):
            current = "environment"
        elif line.startswith(COMMANDS_MARKER):
            current = "commands"
        sections[current].append(line)
    return sections


def _first_difference(bash_lines, py_lines):
    for index in range(max(len(bash_lines), len(py_lines))):
        b = bash_lines[index] if index < len(bash_lines) else None
        p = py_lines[index] if index < len(py_lines) else None
        if b != p:
            return index, b, p
    return None


def _header_hint(line: str | None) -> str:
    for prefix, module in HEADER_HINTS:
        if line is not None and line.startswith(prefix):
            return module
    if line is not None and not line.startswith("#"):
        return "narrowing.py (the multi-line narrowing header)"
    return "render.py"


def _stderr_hint(line: str | None) -> str:
    for pattern, module in STDERR_HINTS:
        if line is not None and pattern.search(line):
            return module
    return "build.py (the planning stage that emits it)"


def _module_hint(section: str, bash_lines, py_lines, line: str | None) -> str:
    if section == "header":
        return _header_hint(line)
    if section == "environment":
        return "environment.py (the env lines) / render.py (their text)"
    if section == "commands":
        if line is not None and line.startswith(COMMANDS_MARKER):
            return "render.py"
        if len(bash_lines) == len(py_lines):
            return "render.py (the step text)"
        return "build.py (which steps the plan holds)"
    if section == "stderr":
        return _stderr_hint(line)
    return "cli.py / build.py (the exit decision)"


def _show(line: str | None) -> str:
    return "<absent>" if line is None else repr(line)


def _section_lines(outcome: Outcome, section: str) -> list[str]:
    if section == "stderr":
        return outcome.stderr.splitlines()
    return plan_sections(outcome.stdout)[section]


def compare(bash: Outcome, py: Outcome, sections=SECTIONS) -> Divergence | None:
    """The first divergence between bash's and python's outcome, or None.

    `sections` restricts the comparison to a subset of SECTIONS, checked in
    SECTIONS order.
    """
    unknown = set(sections) - set(SECTIONS)
    assert not unknown, f"unknown sections {sorted(unknown)}; want a subset of {SECTIONS}"
    for section in (s for s in SECTIONS if s in sections):
        if section == "exit code":
            if bash.returncode != py.returncode:
                return _divergence(
                    bash, py, section, section, "cli.py / build.py (the exit decision)",
                    f"  bash exited {bash.returncode}, python {py.returncode}",
                )
            continue
        bash_lines, py_lines = _section_lines(bash, section), _section_lines(py, section)
        diff = _first_difference(bash_lines, py_lines)
        if diff is not None:
            index, b, p = diff
            hint = _module_hint(section, bash_lines, py_lines, b if b is not None else p)
            return _divergence(
                bash, py, section, f"{section} (line {index + 1} of the section)", hint,
                f"  bash:   {_show(b)}\n  python: {_show(p)}",
            )
    return None


def _divergence(bash, py, section, where, module, detail) -> Divergence:
    message = "\n".join(
        (
            f"plan divergence in the {where}; edit scripts/reify_verify/{module}",
            detail,
            PORT_WINDOW_RULE,
            f"--- bash exit {bash.returncode} stderr:\n{bash.stderr}",
            f"--- python exit {py.returncode} stderr:\n{py.stderr}",
        )
    )
    return Divergence(section, message)


class InParityClone(InClone):
    """A test case planning scenarios in the shared clone, reset before each test."""

    def assert_parity(self, scenario: Scenario, pair: Pair, sections=SECTIONS):
        divergence = compare(pair.bash, pair.py, sections)
        if divergence is not None:
            self.fail(f"[{scenario.label}] {divergence.message}")

    def assert_bash_plans(self, scenario: Scenario, pair: Pair):
        self.assertEqual(
            pair.bash.returncode, 0,
            f"premise: bash plans [{scenario.label}]\n{pair.bash.stderr}",
        )


def _without_line(text: str, line: str) -> str:
    return "".join(kept for kept in text.splitlines(keepends=True) if kept.rstrip("\n") != line)


def _write_merge_head(clone: Clone):
    clone.merge_head().write_text(f"{clone.base}\n", encoding="utf-8")


@dataclass(frozen=True)
class ExpectedDivergence:
    """One of PRD D8's listed divergences, asserted positively both ways.

    python's outcome on `scenario` equals transform(bash's outcome on
    `reference`) over `sections`, and bash's outcome on `scenario` differs
    from python's. `reference` defaults to `scenario`; it is another
    invocation only where bash reaches python's outcome by a different route
    (the cwd defect 4 needs, or the text flag the JSON plan twins). Sections
    left out of `sections` are another member's subject, named by `citation`.
    """

    name: str
    citation: str
    scenario: Scenario
    transform: Callable[[Outcome], Outcome] = lambda outcome: outcome
    reference: Scenario | None = None
    sections: tuple[str, ...] = SECTIONS


OFFLINE_DUPLICATE_KNOB_WARNING = (
    "verify.sh: WARNING: invalid REIFY_VERIFY_TEST_TIMEOUT_RELEASE=bogus; using default 90m"
)
LINT_ALL_PLAN = ("lint", "--scope", "all", "--print-plan")
MERGE_HEAD_FROM_SCRIPTS = Scenario(
    "MERGE_HEAD present, run from scripts/", ("lint", "--scope", "staged", "--print-plan"),
    setup=_write_merge_head, cwd="scripts",
)

EXPECTED_DIVERGENCES = (
    ExpectedDivergence(
        "offline-single-knob-warning",
        "PRD D8 listed divergences: a malformed timeout knob warns once, where offline today warns twice",
        Scenario(
            "offline role, bogus release test timeout", LINT_ALL_PLAN,
            {"DF_VERIFY_ROLE": "offline", "REIFY_VERIFY_TEST_TIMEOUT_RELEASE": "bogus"},
        ),
        lambda bash: dataclasses.replace(
            bash, stderr=_without_line(bash.stderr, OFFLINE_DUPLICATE_KNOB_WARNING)
        ),
    ),
    ExpectedDivergence(
        "print-plan-json-accepted",
        "PRD D8 listed divergences: --print-plan=json is accepted, where bash exits 64 on it "
        "(fold-verifier V12); its stdout is TestJsonPlanParity's subject",
        Scenario("json plan", ("lint", "--scope", "all", "--print-plan=json")),
        reference=Scenario("text plan", LINT_ALL_PLAN),
        sections=("exit code", "stderr"),
    ),
    ExpectedDivergence(
        "merge-head-from-foreign-cwd",
        "PRD D8 listed divergences: the defect 4 fix (§2.4 — the MERGE_HEAD force was "
        "evaluated against the caller's cwd)",
        MERGE_HEAD_FROM_SCRIPTS,
        reference=dataclasses.replace(MERGE_HEAD_FROM_SCRIPTS, cwd=None),
    ),
)


def check_divergence(clone: Clone, expected: ExpectedDivergence) -> str | None:
    """None when `expected` holds both ways in `clone`, else the failure message."""
    pair = run_both(clone, expected.scenario)
    if compare(pair.bash, pair.py) is None:
        return (
            f"[{expected.name}] expected divergence did not occur — bash converged; "
            f"delete the entry ({expected.citation})"
        )
    reference = pair.bash if expected.reference is None else run_bash(clone, expected.reference)
    divergence = compare(expected.transform(reference), pair.py, expected.sections)
    if divergence is not None:
        return (
            f"[{expected.name}] python is not bash's outcome transformed as {expected.citation} "
            f"lists (the 'bash' side below is the transformed one)\n{divergence.message}"
        )
    return None


GRID_ROLES = (None, "merge", "offline", "background")
GRID_PROFILES = ((), ("--profile", "debug"), ("--profile", "release"), ("--profile", "both"))
GRID_SCOPES = ("all", "staged", "branch")
GRID_CRATE_CHANGE = "crates/reify-doc/src/lib.rs"


def grid_scenarios(action: str) -> list[Scenario]:
    """`action`'s full grid (PRD D13): role × profile × scope, each with --include-infra."""
    scenarios = []
    for role, profile, scope in itertools.product(GRID_ROLES, GRID_PROFILES, GRID_SCOPES):
        argv = (action, "--scope", scope, *profile, "--include-infra", "--print-plan")
        scenarios.append(Scenario(f"{' '.join(argv)} role={role}", argv, {"DF_VERIFY_ROLE": role}))
    return scenarios


class GridParity(InParityClone):
    """One action's grid, ACTION, compared in full in each clone state; a shard sets ACTION."""

    ACTION: str

    def assert_grid(self):
        for scenario, pair in run_many(self.clone, grid_scenarios(self.ACTION)):
            with self.subTest(scenario.label):
                self.assert_bash_plans(scenario, pair)
                self.assert_parity(scenario, pair)

    def assert_staged_change_grid(self):
        """The grid over a staged crate change on a started branch, so staged and branch both see it."""
        self.clone.start_branch()
        self.clone.stage(GRID_CRATE_CHANGE)
        self.assertEqual(self.clone.name_list("--cached"), [GRID_CRATE_CHANGE], "premise: the change is staged")
        self.assert_grid()
