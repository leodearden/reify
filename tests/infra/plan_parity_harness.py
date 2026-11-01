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
listed divergences that B1's corpus reaches and PRD §12's single plan
divergence, each asserted positively both ways by check_divergence(). Every
other scenario compares byte-equal; there is no per-scenario opt-out.

GridParity runs one action's full grid (PRD D13: role × profile × scope, with
--include-infra) in two clone states; each grid shard member subclasses it.

The scenario fixtures the members share live here too: the PATH farms and
stub cargos for the nextest and priority cases (path_farm, nextest_env), the
changed paths S22 B14 plans one at a time (CHANGED_PATH_FIXTURES), and
PremisedParity, which checks each Case's bash-side premise (it reaches the
shape it is named for) before its parity, so a case cannot pass vacuously.
"""

import dataclasses
import itertools
import os
import re
import shutil
import sys
import tempfile
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
    "Case",
    "Divergence",
    "ExpectedDivergence",
    "GridParity",
    "InParityClone",
    "PremisedParity",
    "CHANGED_PATH_FIXTURES",
    "NEXTEST_ABSENT_PROBE",
    "SYSTEM_BIN_DIRS",
    "base_env",
    "check_divergence",
    "compare",
    "discard_clone",
    "grid_scenarios",
    "nextest_env",
    "path_farm",
    "plan",
    "plan_sections",
    "run_both",
    "run_many",
    "shared_clone",
    "shared_env",
    "stage_written",
    "the_clone",
    "toolchain_env",
    "write_stub",
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
    ("# heavy partition — ", "testregion.py"),
)
STDERR_HINTS = (
    (re.compile(r"gen-nextest-config"), "nextest.py"),
    (re.compile(r"release-sensitive"), "testregion.py"),
    (re.compile(r"stale jobserver FIFO"), "environment.py"),
    (re.compile(r"cargo-nextest|cargo nextest"), "environment.py"),
    (re.compile(r"REIFY_VERIFY_[A-Z_]*TIMEOUT"), "knobs.py"),
    (re.compile(r"nice|ionice|priority"), "policy.py"),
    (re.compile(r"MERGE_HEAD|--scope|merge-base|forcing"), "scope.py"),
    (re.compile(r"REIFY_AFFECTED_CRATES_OVERRIDE|affected|narrow"), "narrowing.py"),
    (re.compile(r"REIFY_GUI_RETRY_SPECS"), "build.py"),
)
COMMAND_HINTS = (
    (re.compile(r"cargo nextest run|cargo test"), "nextest.py (the pass text: render.py)"),
    (
        re.compile(r"@@SEMAPHORE|test-run semaphore|psi-gate|RELEASE-PASS|reify-verify-attempt"),
        "testregion.py (the region's steps: render.py)",
    ),
)
SYSTEM_BIN_DIRS = (Path("/usr/bin"), Path("/bin"))
# nextest_env's probe body for a cargo with no nextest subcommand.
NEXTEST_ABSENT_PROBE = "    echo 'error: no such command: `nextest`' >&2\n    exit 101\n"
# The changed paths tests/infra/test_verify_scope.sh plans one at a time with
# `plan_for staged` (S22 B14); stage_written writes each as plan_for does.
CHANGED_PATH_FIXTURES = (
    "Cargo.lock",
    "crates/reify-doc/src/lib.rs",
    "crates/reify-eval/src/cache.rs",
    "dark-factory-orchestrator.yaml",
    "docs/gui-event-channels.md",
    "docs/gui-event-channels/solver-progress.md",
    "docs/note.md",
    "docs/prds/v0_6/foo.md",
    "examples/foo.ri",
    "examples/foo/bar.ri",
    "examples/auto/probe.ri",
    "gui/src/editor/foo.ts",
    "gui/src-tauri/src/main.rs",
    "scripts/prd-capability-check.py",
    "some_unrecognised_file",
    "tests/prd-gate/corpus-probe-set.json",
    "tests/prd-gate/fixtures/geometry_let_selector_consumer.ri",
    "tests/prd-gate/fixtures/new_prd_fixture.ri",
    "tests/prd-gate/fixtures/Probe.RI",
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


def stage_written(clone: Clone, path: str):
    """plan_for's fixture: `path` holds exactly 'x\\n', staged."""
    target = clone.root / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text("x\n", encoding="utf-8")
    clone.git("add", "--", path)


def toolchain_env() -> dict:
    """RUSTUP_HOME and CARGO_HOME as the real HOME resolves them, so a scratch HOME keeps cargo working."""
    env = base_env()
    home = env["HOME"]
    return {
        "RUSTUP_HOME": env.get("RUSTUP_HOME", f"{home}/.rustup"),
        "CARGO_HOME": env.get("CARGO_HOME", f"{home}/.cargo"),
    }


def write_stub(path: Path, body: str):
    path.write_text(f"#!/bin/sh\n{body}", encoding="utf-8")
    path.chmod(0o755)


def path_farm(scratch: Path, *without: str) -> str:
    """A PATH directory under `scratch` linking every /usr/bin and /bin entry except `without`."""
    farm = scratch / "-".join(("farm-without", *without))
    farm.mkdir()
    for directory in SYSTEM_BIN_DIRS:
        for entry in directory.iterdir():
            link = farm / entry.name
            if entry.name not in without and not os.path.lexists(link):
                link.symlink_to(entry)
    for tool in without:
        if shutil.which(tool, path=str(farm)) is not None:
            raise AssertionError(f"premise: {tool} is off the farm")
    return str(farm)


def nextest_env(scratch: Path, name: str, nextest_probe: str, cargo_nextest=False) -> dict:
    """A scratch HOME without ~/.cargo/env, and PATH = a stub cargo whose `nextest --version` runs
    `nextest_probe` (every other cargo call execs the real one), then /usr/bin and /bin."""
    stubs = scratch / name
    stubs.mkdir()
    real_cargo = shutil.which("cargo", path=base_env()["PATH"])
    write_stub(
        stubs / "cargo",
        'if [ "$1" = nextest ] && [ "$2" = --version ]; then\n'
        f"{nextest_probe}"
        "fi\n"
        f'exec "{real_cargo}" "$@"\n',
    )
    if cargo_nextest:
        write_stub(stubs / "cargo-nextest", "exit 0\n")
    path = os.pathsep.join((str(stubs), *map(str, SYSTEM_BIN_DIRS)))
    if (shutil.which("cargo-nextest", path=path) is not None) != cargo_nextest:
        raise AssertionError("premise: cargo-nextest is on PATH iff the scenario stubs it")
    home = scratch / f"{name}-home"
    home.mkdir()
    return {"HOME": str(home), "PATH": path, **toolchain_env()}


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
        for pattern, module in COMMAND_HINTS:
            if line is not None and pattern.search(line):
                return module
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


@dataclass(frozen=True)
class Case:
    """A scenario and its bash-side premise.

    The premise is bash's exit code, the start of its first stdout line, texts
    its stdout holds and lacks, and texts its stderr holds (in this order) and
    lacks.
    """

    scenario: Scenario
    stdout: tuple[str, ...] = ()
    absent: tuple[str, ...] = ()
    stderr: tuple[str, ...] = ()
    stderr_absent: tuple[str, ...] = ()
    first_line: str | None = None
    exit_code: int = 0


def plan(label: str, *argv: str, env: Mapping[str, str | None] | None = None) -> Scenario:
    """`<argv> --print-plan` under the env overrides."""
    return Scenario(label, (*argv, "--print-plan"), dict(env or {}))


class PremisedParity(InParityClone):
    """Cases checked premise first, then for full parity; each test has a scratch directory."""

    def setUp(self):
        super().setUp()
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.scratch = Path(scratch.name)

    def check(self, *cases: Case):
        """Every case in the clone's current state, concurrently."""
        pairs = run_many(self.clone, (case.scenario for case in cases))
        for case, (_, pair) in zip(cases, pairs):
            self.check_pair(case, pair)

    def check_pair(self, case: Case, pair: Pair):
        label = case.scenario.label
        with self.subTest(label):
            bash = pair.bash
            self.assertEqual(bash.returncode, case.exit_code, f"premise: bash's exit code [{label}]\n{bash.stderr}")
            if case.first_line is not None:
                first = next(iter(bash.stdout.splitlines()), "")
                self.assertEqual(first[: len(case.first_line)], case.first_line, f"premise: bash's first stdout line [{label}]")
            for text in case.stdout:
                self.assertIn(text, bash.stdout, f"premise [{label}]")
            for text in case.absent:
                self.assertNotIn(text, bash.stdout, f"premise [{label}]")
            position = 0
            for text in case.stderr:
                found = bash.stderr.find(text, position)
                self.assertNotEqual(found, -1, f"premise: bash's stderr holds {text!r}, in order [{label}]")
                position = found + len(text)
            for text in case.stderr_absent:
                self.assertNotIn(text, bash.stderr, f"premise: bash's stderr [{label}]")
            self.assert_parity(case.scenario, pair)


def _without_line(text: str, line: str) -> str:
    return "".join(kept for kept in text.splitlines(keepends=True) if kept.rstrip("\n") != line)


def _gui_pass_names_the_override(bash: Outcome) -> Outcome:
    lines = bash.stdout.splitlines(keepends=True)
    edited = [
        PLACEHOLDER_CONFIG_FILE.sub(f"--config-file {CALLER_NEXTEST_CONFIG}", line)
        if GUI_FEATURE_PASS in line else line
        for line in lines
    ]
    return dataclasses.replace(bash, stdout="".join(edited))


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
CALLER_NEXTEST_CONFIG = "/nonexistent/b1-scratch/caller-nextest.toml"
GUI_FEATURE_PASS = "cargo nextest run -p reify-gui --features gui"
PLACEHOLDER_CONFIG_FILE = re.compile(r"--config-file \S*reify-nextest-occt\.<print-plan-placeholder>")
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
    ExpectedDivergence(
        "gui-feature-pass-honours-nextest-config",
        "PRD §12's single plan divergence: the gui-feature pass names the caller's "
        "REIFY_VERIFY_NEXTEST_CONFIG, where bash's hand copy names the generated file "
        "(fk-b072e820e926)",
        Scenario(
            "task role, caller-owned nextest config", ("test", "--scope", "all", "--print-plan"),
            {"REIFY_VERIFY_NEXTEST_CONFIG": CALLER_NEXTEST_CONFIG},
        ),
        _gui_pass_names_the_override,
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
