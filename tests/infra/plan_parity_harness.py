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
"""

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
    "PORT_WINDOW_RULE",
    "Scenario",
    "Pair",
    "Divergence",
    "InParityClone",
    "base_env",
    "compare",
    "discard_clone",
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


def shared_env(clone: Clone, overrides: Mapping[str, str | None] = ()) -> dict:
    """The one env both sides of a scenario run under."""
    unpinned = clone.root.parent / "no-jobserver-fifo"
    env = base_env(
        REIFY_NEXTEST_PROBE_RETRY_SLEEP="0",
        **{knob: str(unpinned / knob.lower()) for knob in JOBSERVER_FIFO_KNOBS},
    )
    for name, value in dict(overrides).items():
        if value is None:
            env.pop(name, None)
        else:
            env[name] = value
    return env


def run_both(clone: Clone, scenario: Scenario) -> Pair:
    """Run `scenario` through bash verify.sh, then verify.py, in `clone`."""
    overrides = dict(scenario.env)
    if scenario.setup is not None:
        overrides.update(scenario.setup(clone) or {})
    env = shared_env(clone, overrides)
    cwd = clone.root / scenario.cwd if scenario.cwd else clone.root
    verify_sh = os.path.relpath(clone.root / "scripts" / "verify.sh", cwd)
    verify_py = clone.root / "scripts" / "verify.py"
    bash = run_command(["bash", verify_sh, *scenario.argv], env, cwd)
    py = run_command([sys.executable, "-I", "-B", str(verify_py), *scenario.argv], env, cwd)
    return Pair(bash, py)


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
