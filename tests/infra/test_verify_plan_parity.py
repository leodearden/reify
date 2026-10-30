#!/usr/bin/env python3
"""
test_verify_plan_parity.py — B1's core member: bash `scripts/verify.sh` and
Python `scripts/verify.py` print the same plan (S18 γ1, task 9098; PRD
docs/prds/v0_6/verify-structured-plan-and-role-table.md D8, D13).

Every check plans one scenario on both sides, as subprocesses, in the shared
clone of the tree under test, and compares exit code, stdout and stderr byte
for byte. This module never imports reify_verify. Bash is the oracle until the
cutover: a divergence reds, and its message names the module to edit and the
port-window rule (PRD D13). The one exception is the harness's
EXPECTED_DIVERGENCES (PRD D8's listed divergences), which
TestExpectedDivergences asserts positively both ways.

The runner, the scenario record and compare() live in
tests/infra/plan_parity_harness.py. The lint and typecheck grids are the
sibling shards test_verify_plan_parity_{lint,typecheck}.py.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its
thin wrapper tests/infra/test_verify_plan_parity.sh.
"""

import itertools
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest
from dataclasses import dataclass
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(INFRA_DIR))
from plan_parity_harness import (  # noqa: E402
    EXPECTED_DIVERGENCES,
    PORT_WINDOW_RULE,
    Clone,
    InParityClone,
    Scenario,
    base_env,
    check_divergence,
    compare,
    discard_clone,
    plan_sections,
    run_both,
    run_many,
    shared_clone,
    shared_env,
)

PLAN_ACTIONS = ("lint", "typecheck")
TEST_ACTIONS = ("test", "all")
ROLES = (None, "merge", "offline", "background")
PROFILE_ARGS = ((), ("--profile", "debug"), ("--profile", "release"), ("--profile", "both"))
COMMANDS_MARKER = "# --- commands ("
RENDER_MODULE = "scripts/reify_verify/render.py"
SELF_TEST_PLAN = Scenario(
    "self-test", ("lint", "--scope", "all", "--include-infra", "--print-plan")
)
ENVIRONMENT_SECTIONS = ("exit code", "environment", "stderr")
TASK_FIFO = "REIFY_JOBSERVER_TASK_FIFO"
MERGE_FIFO = "REIFY_JOBSERVER_MERGE_FIFO"
BOOT_ID = Path("/proc/sys/kernel/random/boot_id")
FOREIGN_BOOT_ID = "00000000-0000-0000-0000-000000000000"
RUST_CHANGE = "crates/reify-doc/src/lib.rs"
GUI_CHANGE = "gui/src/editor/foo.ts"
DOCS_CHANGE = "docs/note.md"
EMPTY_PLAN = "# (no commands — nothing to verify for this action/scope)"
INFRA_MAPPED_DOCS = "docs/notes/verify-pipeline-knobs.md"
POLICY_LEAF = "( for _vt in tests/infra/test_verify_policy.sh; do "
LEAF_TAIL = ' -u REIFY_VERIFY_EVENT_LOG bash "$_vt" || exit $?; done )'
LD_SCRUB = 'export LD_LIBRARY_PATH="${REIFY_AMBIENT_LD_LIBRARY_PATH-}"; '
PLAIN_GUI_PROJECT = f"{LD_SCRUB}if test -d gui; then (cd gui && "
FULL_GUI_SUITE = "../scripts/gui-vitest-run.sh'); fi"
VALID_GUI_SPECS = "src/__tests__/a.test.ts src/__tests__/b.test.ts"
GUI_SPECS_REJECTED = "REIFY_GUI_RETRY_SPECS contains characters outside [A-Za-z0-9._/ -]"
TEST_REGION_BEGIN = (
    "# >>> test-run semaphore: ACQUIRE held slot — clock-stop region BEGINS "
    "(TEST-EXECUTION gated, held in verify.sh)"
)
EX_UNAVAILABLE = 69
FAILED_ONLY = {"REIFY_VERIFY_RETRY_SCOPE": "failed_only"}
RETRY_MARKER = "@@REIFY_RETRY_SCOPE=failed_only@@ nextest_debug=0 nextest_release=0 "
PLAN_HEADER = "# verify.sh plan — "
RUN_ALL = "tests/infra/run_all.sh"
LINT_ALL = ("lint", "--scope", "all", "--print-plan")
# _resolve_timeout_knob's call order in scripts/verify.sh.
TIMEOUT_KNOBS = (
    "REIFY_VERIFY_TEST_TIMEOUT",
    "REIFY_VERIFY_TEST_TIMEOUT_RELEASE",
    "REIFY_VERIFY_PREBUILD_TIMEOUT",
    "REIFY_VERIFY_CLIPPY_TIMEOUT",
    "REIFY_VERIFY_GUI_FEATURE_TEST_TIMEOUT",
    "REIFY_VERIFY_CHECK_TIMEOUT",
)
SYSTEM_BIN_DIRS = (Path("/usr/bin"), Path("/bin"))
NEXTEST_ABSENT = " nextest=0 "
NEXTEST_PRESENT = " nextest=1 "
# The present-but-failing stub prints these lines, then a blank one that $(…) strips.
STUB_PROBE_LINES = ("error: the B1 stub nextest probe failed", "  caused by: a stub")
PROBE_EXHAUSTED = (
    "cargo-nextest is present on PATH but the availability probe (`cargo nextest --version`) "
    "failed persistently across 3/3 retries (4 probes total, including the initial attempt; "
    "last retry rc=101)"
)
REAPER_LIB = "scripts/lib_proc_reaper.sh"
HEAVY_FILTER_LIB = "scripts/heavy-test-filter-lib.sh"
HEAVY_FILTER_EXPORT = re.compile(r"^export REIFY_HEAVY_NEXTEST_FILTER=.*$", re.MULTILINE)
EMPTY_HEAVY_FILTER = (
    "verify.sh: ERROR — REIFY_HEAVY_NEXTEST_FILTER empty after sourcing heavy-test-filter-lib.sh"
)
JSON_PLAN = "--print-plan=json"
VERIFY_LEDGER = "scripts/verify-ledger.py"
RECORD_SEQUENCE = re.compile(r"(retry_scope )?header scope narrowing (heavy_partition )?(env )*(step )*")
NOT_RECORD_FIELDS = ("v", "record")
# label, the path staged first (None: none), argv before the plan flag, env.
JSON_CASES = (
    ("lint merge all", None, ("lint", "--scope", "all"), {"DF_VERIFY_ROLE": "merge"}),
    ("lint task staged --narrow, crate change", RUST_CHANGE, ("lint", "--scope", "staged", "--narrow"), {}),
    ("typecheck task all", None, ("typecheck", "--scope", "all"), {}),
    ("test gui-only staged, retry specs", GUI_CHANGE, ("test", "--scope", "staged"),
     {**FAILED_ONLY, "REIFY_GUI_RETRY_SPECS": VALID_GUI_SPECS}),
    ("lint docs-only, empty plan", DOCS_CHANGE, ("lint", "--scope", "staged"), {}),
)
# The changed paths tests/infra/test_verify_scope.sh plans one at a time with
# `plan_for staged` (S22 B14); each is written as 'x\n' and staged, as plan_for does.
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


def tearDownModule():
    discard_clone()


def role_env(role):
    return {"DF_VERIFY_ROLE": role}


def host_boot_id() -> str:
    return BOOT_ID.read_text(encoding="utf-8").strip()


def toolchain_env() -> dict:
    """RUSTUP_HOME and CARGO_HOME as the real HOME resolves them, so a scratch HOME keeps cargo working."""
    env = base_env()
    home = env["HOME"]
    return {
        "RUSTUP_HOME": env.get("RUSTUP_HOME", f"{home}/.rustup"),
        "CARGO_HOME": env.get("CARGO_HOME", f"{home}/.cargo"),
    }


def dead_pid() -> int:
    """A pid no process can hold: one above the kernel's pid_max."""
    return int(Path("/proc/sys/kernel/pid_max").read_text(encoding="utf-8")) + 1


class TestPlanParityHeader(InParityClone):
    """The header section (retry marker, header, NOTE, scope, narrowing) and the exit code."""

    def scenarios(self):
        for action, role, infra, profile in itertools.product(
            PLAN_ACTIONS, ROLES, ((), ("--include-infra",)), ((), ("--profile", "both"))
        ):
            argv = (action, "--scope", "all", *infra, *profile, "--print-plan")
            yield Scenario(f"{' '.join(argv)} role={role}", argv, role_env(role))

    def test_header_and_exit_code(self):
        for scenario, pair in run_many(self.clone, self.scenarios()):
            with self.subTest(scenario.label):
                self.assert_bash_plans(scenario, pair)
                self.assert_parity(scenario, pair, ("exit code", "header"))


class TestPlanParityEnvironment(InParityClone):
    """The environment section, stderr and the exit code: cargo env, loader path, jobserver."""

    def setUp(self):
        super().setUp()
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.scratch = Path(scratch.name)

    def check(self, label, env=(), expect=()):
        """Both actions agree; bash's output holds each `expect` text (the scenario's premise)."""
        scenarios = [
            Scenario(f"{label} ({action})", (action, "--scope", "all", "--print-plan"), dict(env))
            for action in PLAN_ACTIONS
        ]
        for scenario, pair in run_many(self.clone, scenarios):
            with self.subTest(scenario.label):
                self.assert_bash_plans(scenario, pair)
                for text in expect:
                    self.assertIn(text, pair.bash.stdout + pair.bash.stderr, "premise")
                self.assert_parity(scenario, pair, ENVIRONMENT_SECTIONS)

    def home(self, name, cargo_env=None) -> dict:
        """A scratch HOME, with `cargo_env` as its ~/.cargo/env, and the real toolchain."""
        home = self.scratch / name
        home.mkdir()
        if cargo_env is not None:
            (home / ".cargo").mkdir()
            (home / ".cargo" / "env").write_text(cargo_env, encoding="utf-8")
        return {"HOME": str(home), **toolchain_env()}

    def fifo(self, name, stamp=None) -> str:
        fifo = self.scratch / name
        os.mkfifo(fifo)
        if stamp is not None:
            Path(f"{fifo}.owner").write_text(stamp, encoding="utf-8")
        return str(fifo)

    def test_pinned_missing_fifo(self):
        self.check("FIFO knobs pinned to a missing path", expect=(" FIFO) — cargo uses its own job pool",))

    def test_home_without_cargo_env(self):
        self.check("no ~/.cargo/env", self.home("bare"), ("# # ~/.cargo/env not found",))

    def test_cargo_env_delta_reaches_the_plan(self):
        bin_dir = self.scratch / "cargo-env-bin"
        bin_dir.mkdir()
        cargo_env = (
            "export REIFY_PARITY_CARGO_ENV_PROBE=sourced\n"
            f"export PATH=\"{bin_dir}:$PATH\"\n"
            f"export LD_LIBRARY_PATH=\"{self.scratch}/from-cargo-env\"\n"
        )
        env = self.home("cargo", cargo_env)
        self.check(
            "~/.cargo/env sourced",
            env,
            (f"# . {env['HOME']}/.cargo/env", f"AMBIENT_LD_LIBRARY_PATH={self.scratch}/from-cargo-env"),
        )

    def test_ambient_loader_path(self):
        ambient = f"{self.scratch}/ambient"
        self.check("ambient LD_LIBRARY_PATH", {"LD_LIBRARY_PATH": ambient}, (f":{ambient}\n",))

    def test_fifo_owner_stamps(self):
        boot = host_boot_id()
        live = os.getpid()
        exported, stale = "export CARGO_MAKEFLAGS=", "WARNING — stale jobserver FIFO"
        cases = (
            ("no owner stamp", None, {}, exported),
            ("live owner", f"{live} {boot}\n", {}, exported),
            ("dead owner", f"{dead_pid()} {boot}\n", {}, stale),
            ("live owner of another boot", f"{live} {FOREIGN_BOOT_ID}\n", {}, stale),
            ("malformed pid", f"12x {boot}\n", {}, exported),
            ("stamp without a newline", f"{dead_pid()} {boot}", {}, exported),
            ("stale stamp, probe skipped", f"{dead_pid()} {boot}\n",
             {"REIFY_JOBSERVER_SKIP_LIVENESS_PROBE": "1"}, exported),
        )
        for n, (label, stamp, extra, premise) in enumerate(cases):
            fifo = self.fifo(f"fifo-{n}", stamp)
            self.check(f"task FIFO: {label}", {TASK_FIFO: fifo, **extra}, (premise,))

    def test_merge_role_live_fifo(self):
        fifo = self.fifo("merge-fifo", f"{os.getpid()} {host_boot_id()}\n")
        self.check(
            "merge FIFO live",
            {"DF_VERIFY_ROLE": "merge", MERGE_FIFO: fifo},
            (f"export CARGO_MAKEFLAGS=--jobserver-auth=fifo:{fifo}",),
        )

    def test_offline_role(self):
        self.check("offline role", {"DF_VERIFY_ROLE": "offline"}, ("(offline role — ",))

    def test_host_default_fifo(self):
        self.check("host default FIFO", {TASK_FIFO: None, MERGE_FIFO: None}, ("/tmp/reify-jobserver-task",))


class FullPlanParity(InParityClone):
    """Base of the members comparing full stdout, stderr and exit code; ACTIONS is the default action set."""

    ACTIONS = PLAN_ACTIONS

    def check(self, label, argv, env=(), expect=(), warns=(), actions=None):
        """Each action agrees; bash's stdout holds each `expect` text and its stderr each `warns` text (the premises)."""
        scenarios = [
            Scenario(f"{label} ({action})", (action, *argv, "--print-plan"), dict(env))
            for action in (self.ACTIONS if actions is None else actions)
        ]
        return self.check_scenarios(scenarios, expect, warns)

    def check_scenarios(self, scenarios, expect=(), warns=()):
        pairs = run_many(self.clone, scenarios)
        for scenario, pair in pairs:
            with self.subTest(scenario.label):
                self.assert_bash_plans(scenario, pair)
                for text in expect:
                    self.assertIn(text, pair.bash.stdout, "premise")
                for text in warns:
                    self.assertIn(text, pair.bash.stderr, "premise")
                self.assert_parity(scenario, pair)
        return pairs


class TestPlanParityCommands(FullPlanParity):
    """Full stdout, stderr and exit code of lint and typecheck plans."""

    def stage_written(self, path):
        """plan_for's fixture: `path` holds exactly 'x\\n', staged."""
        target = self.clone.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text("x\n", encoding="utf-8")
        self.clone.git("add", "--", path)

    def test_scope_all_under_each_role(self):
        for role in ROLES:
            self.check(f"--scope all role={role}", ("--scope", "all", "--include-infra"), role_env(role))

    def test_staged_rust_change(self):
        self.clone.stage(RUST_CHANGE)
        self.check("staged rust change", ("--scope", "staged"), expect=("NARROW_ACTIVE=0",))
        self.check(
            "staged rust change --narrow", ("--scope", "staged", "--narrow"),
            expect=("NARROW_ACTIVE=1", " -p reify-doc"),
        )

    def test_branch_rust_change(self):
        self.clone.start_branch()
        self.clone.stage(RUST_CHANGE)
        self.check("branch rust change", ("--scope", "branch"), expect=("NARROW_ACTIVE=1",))

    def test_affected_crates_overrides(self):
        self.clone.stage(RUST_CHANGE)
        for value in ("reify-cli reify-eval", "ALL", "bad*name", "  "):
            self.check(
                f"REIFY_AFFECTED_CRATES_OVERRIDE={value!r}",
                ("--scope", "staged", "--narrow"),
                {"REIFY_AFFECTED_CRATES_OVERRIDE": value},
                expect=(f"closure={value} ",),
            )

    def test_gui_only_change(self):
        self.clone.stage(GUI_CHANGE)
        self.check("gui-only change", ("--scope", "staged"), expect=("RUN_RUST=0 RUN_GUI=1",))

    def test_docs_only_change(self):
        self.clone.stage(DOCS_CHANGE)
        self.check("docs-only change", ("--scope", "staged"), expect=(EMPTY_PLAN,))

    def test_valid_timeout_knobs(self):
        self.check(
            "valid timeout knobs", ("--scope", "all"),
            {"REIFY_VERIFY_CLIPPY_TIMEOUT": "7m", "REIFY_VERIFY_CHECK_TIMEOUT": "11m"},
        )

    def test_test_only_knobs_leave_lint_unchanged(self):
        (_, plain), = self.check("plain lint", ("--scope", "all"), actions=("lint",))
        (_, heavy), = self.check(
            "lint, heavy excluded", ("--scope", "all"), {"REIFY_GATE_EXCLUDE_HEAVY": "1"},
            actions=("lint",),
        )
        (_, threads), = self.check("lint --test-threads=3", ("--scope", "all", "--test-threads=3"),
                                   actions=("lint",))
        self.assertEqual(heavy.bash.stdout, plain.bash.stdout, "premise: the heavy knob leaves lint alone")
        self.assertEqual(threads.bash.stdout, plain.bash.stdout, "premise: --test-threads leaves lint alone")

    def test_changed_path_fixtures(self):
        for path in CHANGED_PATH_FIXTURES:
            self.clone.reset()
            self.stage_written(path)
            self.check(f"staged {path}", ("--scope", "staged", "--include-infra"), actions=("lint",))


class TestPlanParityRegionFree(FullPlanParity):
    """Full stdout, stderr and exit code of test and all plans that hold no test region (RUN_RUST=0)."""

    ACTIONS = TEST_ACTIONS

    def test_empty_diff_under_each_profile(self):
        self.clone.start_branch()
        scenarios = []
        for action, scope, role, profile in itertools.product(
            TEST_ACTIONS, ("staged", "branch"), (None, "offline"), PROFILE_ARGS
        ):
            argv = (action, "--scope", scope, *profile, "--print-plan")
            scenarios.append(Scenario(f"{' '.join(argv)} role={role}", argv, role_env(role)))
        self.check_scenarios(scenarios, expect=("RUN_RUST=0", EMPTY_PLAN))

    def test_gui_only_change_runs_the_plain_node_lane(self):
        self.clone.stage(GUI_CHANGE)
        pairs = self.check(
            "gui-only change", ("--scope", "staged"),
            expect=("RUN_RUST=0 RUN_GUI=1", FULL_GUI_SUITE),
        )
        for scenario, pair in pairs:
            self.assertTrue(
                any(line.startswith(PLAIN_GUI_PROJECT) for line in pair.bash.stdout.splitlines()),
                f"premise: [{scenario.label}] the gui project is a foreground step of its own",
            )

    def test_gui_retry_specs(self):
        self.clone.stage(GUI_CHANGE)
        self.check(
            "valid REIFY_GUI_RETRY_SPECS", ("--scope", "staged"),
            {"REIFY_GUI_RETRY_SPECS": VALID_GUI_SPECS},
            expect=(f"../scripts/gui-vitest-run.sh {VALID_GUI_SPECS}'",),
        )
        for value in ("$(x)", "--run"):
            self.check(
                f"rejected REIFY_GUI_RETRY_SPECS={value!r}", ("--scope", "staged"),
                {"REIFY_GUI_RETRY_SPECS": value},
                expect=(FULL_GUI_SUITE,), warns=(GUI_SPECS_REJECTED,),
            )

    def test_selective_infra_leaf(self):
        self.clone.stage(INFRA_MAPPED_DOCS)
        self.check(
            "infra-mapped docs path", ("--scope", "staged"),
            expect=("RUN_RUST=0", POLICY_LEAF, LEAF_TAIL),
        )
        self.check(
            "infra-mapped docs path, offline role", ("--scope", "staged"),
            role_env("offline"), expect=("RUN_RUST=0", POLICY_LEAF),
        )
        self.check(
            "infra-mapped docs path inside an infra suite", ("--scope", "staged"),
            {"REIFY_INFRA_SUITE_ACTIVE": "1"}, expect=(EMPTY_PLAN,),
        )


class TestPlanParityRetryMarker(FullPlanParity):
    """The failed_only retry marker: stdout's first line iff a suite really narrowed, under REIFY_VERIFY_RETRY_SCOPE=failed_only."""

    def check_first_line(self, cases):
        """cases: (label, argv, env, the first line bash prints) — its premise, then full parity."""
        scenarios = [
            Scenario(label, (*argv, "--print-plan"), {**FAILED_ONLY, **env}) for label, argv, env, _ in cases
        ]
        pairs = self.check_scenarios(scenarios)
        for (label, _, _, first_line), (_, pair) in zip(cases, pairs):
            self.assertEqual(
                pair.bash.stdout.splitlines()[0][: len(first_line)], first_line,
                f"premise: [{label}] bash's first stdout line",
            )

    def test_run_all_subset(self):
        lint_all = ("lint", "--scope", "all")
        self.check_first_line((
            ("a matched and a stale member", lint_all,
             {"REIFY_RUN_ALL_MEMBER_SUBSET": "test_verify_policy.sh nope.sh"},
             f"{RETRY_MARKER}run_all=1 gui=0"),
            ("stale members only", lint_all,
             {"REIFY_RUN_ALL_MEMBER_SUBSET": "nope.sh gone.sh"}, PLAN_HEADER),
            ("a full retry", lint_all,
             {"REIFY_VERIFY_RETRY_SCOPE": "full", "REIFY_RUN_ALL_MEMBER_SUBSET": "test_verify_policy.sh"},
             PLAN_HEADER),
        ))

    def test_gui_specs(self):
        self.clone.stage(GUI_CHANGE)
        test_staged = ("test", "--scope", "staged")
        self.check_first_line((
            ("valid gui specs", test_staged, {"REIFY_GUI_RETRY_SPECS": VALID_GUI_SPECS},
             f"{RETRY_MARKER}run_all=0 gui=2"),
            ("rejected gui specs", test_staged, {"REIFY_GUI_RETRY_SPECS": "$(x)"}, PLAN_HEADER),
        ))

    def test_run_all_missing_counts_words(self):
        (self.clone.root / RUN_ALL).unlink()
        self.check_first_line((
            ("run_all.sh missing", ("lint", "--scope", "all"),
             {"REIFY_RUN_ALL_MEMBER_SUBSET": "first.sh second.sh"}, f"{RETRY_MARKER}run_all=2 gui=0"),
        ))


@dataclass(frozen=True)
class Expect:
    """A scenario and its premise: bash's exit code, texts its stderr holds in this order, texts its stdout holds."""

    scenario: Scenario
    exit_code: int = 0
    stderr: tuple[str, ...] = ()
    stdout: tuple[str, ...] = ()


def lint_all(label, env=(), argv=LINT_ALL) -> Scenario:
    return Scenario(label, argv, dict(env))


def prio_warning(role, has_nice):
    """The CARGO_PRIO warning bash prints for `role` on a host without ionice (and, unless has_nice, without nice)."""
    if has_nice:
        return None if role == "merge" else f"WARNING — ionice not found; {role} role using nice only"
    missing = "nice" if role == "merge" else "nice/ionice"
    return f"WARNING — {missing} not found; {role} role running at normal priority"


class TestPlanParityStartup(InParityClone):
    """Full stdout, stderr and exit code of what verify does before it plans: knob and priority warnings,
    scope notices, the nextest availability probe, and the load-time refusals."""

    def setUp(self):
        super().setUp()
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.scratch = Path(scratch.name)

    def check(self, *expects: Expect):
        for expect, (scenario, pair) in zip(expects, run_many(self.clone, (e.scenario for e in expects))):
            with self.subTest(scenario.label):
                self.assertEqual(
                    pair.bash.returncode, expect.exit_code,
                    f"premise: bash's exit code [{scenario.label}]\n{pair.bash.stderr}",
                )
                position = 0
                for text in expect.stderr:
                    found = pair.bash.stderr.find(text, position)
                    self.assertNotEqual(found, -1, f"premise: bash's stderr holds {text!r}, in order")
                    position = found + len(text)
                for text in expect.stdout:
                    self.assertIn(text, pair.bash.stdout, "premise")
                self.assert_parity(scenario, pair)

    def path_farm(self, *without: str) -> str:
        """A PATH directory linking every /usr/bin and /bin entry except `without`."""
        farm = self.scratch / "-".join(("farm-without", *without))
        farm.mkdir()
        for directory in SYSTEM_BIN_DIRS:
            for entry in directory.iterdir():
                link = farm / entry.name
                if entry.name not in without and not os.path.lexists(link):
                    link.symlink_to(entry)
        for tool in without:
            self.assertIsNone(shutil.which(tool, path=str(farm)), f"premise: {tool} is off the farm")
        return str(farm)

    def nextest_env(self, name, nextest_probe, cargo_nextest=False) -> dict:
        """A scratch HOME without ~/.cargo/env, and PATH = a stub cargo whose `nextest --version` runs
        `nextest_probe` (every other cargo call execs the real one), then /usr/bin and /bin."""
        stubs = self.scratch / name
        stubs.mkdir()
        real_cargo = shutil.which("cargo", path=base_env()["PATH"])
        self.write_stub(
            stubs / "cargo",
            'if [ "$1" = nextest ] && [ "$2" = --version ]; then\n'
            f"{nextest_probe}"
            "fi\n"
            f'exec "{real_cargo}" "$@"\n',
        )
        if cargo_nextest:
            self.write_stub(stubs / "cargo-nextest", "exit 0\n")
        path = os.pathsep.join((str(stubs), *map(str, SYSTEM_BIN_DIRS)))
        self.assertEqual(
            shutil.which("cargo-nextest", path=path) is not None, cargo_nextest,
            "premise: cargo-nextest is on PATH iff the scenario stubs it",
        )
        home = self.scratch / f"{name}-home"
        home.mkdir()
        return {"HOME": str(home), "PATH": path, **toolchain_env()}

    @staticmethod
    def write_stub(path: Path, body: str):
        path.write_text(f"#!/bin/sh\n{body}", encoding="utf-8")
        path.chmod(0o755)

    def test_bogus_timeout_knobs(self):
        expects = []
        for role in ("task", "merge"):
            for knob in TIMEOUT_KNOBS:
                expects.append(Expect(
                    lint_all(f"{knob}=bogus role={role}", {"DF_VERIFY_ROLE": role, knob: "bogus"}),
                    stderr=(f"WARNING: invalid {knob}=bogus; using default ",),
                ))
            expects.append(Expect(
                lint_all(f"all six knobs bogus role={role}",
                         {"DF_VERIFY_ROLE": role, **dict.fromkeys(TIMEOUT_KNOBS, "bogus")}),
                stderr=tuple(f"WARNING: invalid {knob}=bogus; using default " for knob in TIMEOUT_KNOBS),
            ))
        self.check(*expects)

    def test_priority_without_ionice_or_nice(self):
        farms = {True: self.path_farm("ionice"), False: self.path_farm("ionice", "nice")}
        expects = []
        for role, has_nice in itertools.product(ROLES, (True, False)):
            named = role or "task"
            warning = prio_warning(named, has_nice)
            expects.append(Expect(
                lint_all(f"no ionice{'' if has_nice else ', no nice'} role={named}",
                         {"DF_VERIFY_ROLE": role, "PATH": farms[has_nice]}),
                stderr=(warning,) if warning else (),
            ))
        self.check(*expects)

    def test_merge_head_forces_scope_all(self):
        self.clone.merge_head().write_text(f"{self.clone.base}\n", encoding="utf-8")
        self.check(Expect(
            lint_all("MERGE_HEAD present", argv=("lint", "--scope", "staged", "--print-plan")),
            stderr=("MERGE_HEAD present — forcing --scope all",),
        ))

    def test_merge_role_forces_scope_all(self):
        self.check(Expect(
            lint_all("merge role, --scope staged", role_env("merge"), ("lint", "--scope", "staged", "--print-plan")),
            stderr=("DF_VERIFY_ROLE=merge — forcing --scope all",),
        ))

    def test_branch_without_main_fails_wide(self):
        self.clone.git("branch", "-D", "main")
        self.check(Expect(
            lint_all("no local main", argv=("lint", "--scope", "branch", "--print-plan")),
            stderr=("could not resolve 'git merge-base main HEAD'",),
        ))

    def test_nextest_probe(self):
        absent = self.nextest_env(
            "absent", "    echo 'error: no such command: `nextest`' >&2\n    exit 101\n"
        )
        failing = self.nextest_env(
            "failing", f"    printf '%s\\n' {shlex.join(STUB_PROBE_LINES)} '' >&2\n    exit 101\n",
            cargo_nextest=True,
        )
        counter = self.scratch / "probe-count"
        flaky = self.nextest_env(
            "flaky",
            f'    n=$(($(cat "{counter}" 2>/dev/null || echo 0) + 1))\n'
            f'    echo "$n" > "{counter}"\n'
            '    if [ $((n % 3)) -ne 0 ]; then echo "error: flaky probe $n" >&2; exit 101; fi\n'
            "    echo 'cargo-nextest 0.9.0'\n    exit 0\n",
            cargo_nextest=True,
        )
        self.check(
            Expect(lint_all("nextest absent", absent), stdout=(NEXTEST_ABSENT,)),
            Expect(
                lint_all("nextest present but failing", failing), exit_code=1,
                stderr=(PROBE_EXHAUSTED, "Last probe stderr: " + "\n".join(STUB_PROBE_LINES) + "\n"),
            ),
            Expect(lint_all("nextest flaky", flaky), stdout=(NEXTEST_PRESENT,)),
        )
        self.assertEqual(
            counter.read_text(encoding="utf-8").strip(), "6",
            "premise: each side probed three times, failing twice",
        )

    def test_missing_lib_is_refused(self):
        (self.clone.root / REAPER_LIB).unlink()
        self.check(Expect(
            lint_all(f"{REAPER_LIB} deleted"), exit_code=1,
            stderr=(f"verify.sh: ERROR — {REAPER_LIB} not found next to verify.sh",),
        ))

    def test_empty_heavy_filter_is_refused(self):
        lib = self.clone.root / HEAVY_FILTER_LIB
        source, edits = HEAVY_FILTER_EXPORT.subn("export REIFY_HEAVY_NEXTEST_FILTER=''", lib.read_text(encoding="utf-8"))
        self.assertEqual(edits, 1, f"premise: {HEAVY_FILTER_LIB} exports the filter on one line")
        lib.write_text(source, encoding="utf-8")
        self.check(Expect(lint_all("empty heavy filter"), exit_code=1, stderr=(EMPTY_HEAVY_FILTER,)))

    def test_c_locale_and_empty_role(self):
        self.check(
            Expect(lint_all("LC_ALL=C", {"LC_ALL": "C"})),
            Expect(lint_all("DF_VERIFY_ROLE=''", {"DF_VERIFY_ROLE": ""}), stdout=(" role=task",)),
        )


class TestJsonPlanParity(InParityClone):
    """`--print-plan=json` (PRD D7) is the text plan's typed twin: its records carry what the text says."""

    def py(self, argv, env):
        outcome = self.clone.py(argv, shared_env(self.clone, env))
        self.assertEqual(outcome.returncode, 0, f"verify.py {' '.join(argv)}\n{outcome.stderr}")
        return outcome.stdout

    def classified_phases(self, renders):
        proc = subprocess.run(
            [sys.executable, "-I", "-B", str(self.clone.root / VERIFY_LEDGER), "classify"],
            input="".join(f"{render}\0" for render in renders).encode("utf-8"),
            cwd=self.clone.root, env=base_env(), capture_output=True, check=True,
        )
        return [entry["phase"] for entry in json.loads(proc.stdout)]

    def test_json_records_match_the_text_plan(self):
        for label, staged, argv, env in JSON_CASES:
            with self.subTest(label):
                self.clone.reset()
                if staged is not None:
                    self.clone.stage(staged)
                records = [json.loads(line) for line in self.py((*argv, JSON_PLAN), env).splitlines()]
                text = plan_sections(self.py((*argv, "--print-plan"), env))
                scope_record = json.loads(self.py((*argv, "--print-scope"), env))
                self.check_records(label, records, text, scope_record)

    def check_records(self, label, records, text, scope_record):
        self.assertTrue(all(record.get("v") == 1 for record in records), records)
        names = [record["record"] for record in records]
        self.assertRegex(f"{' '.join(names)} ", f"^{RECORD_SEQUENCE.pattern}$")
        by_name = {record["record"]: record for record in records}
        if "retry_scope" in by_name:
            self.assertEqual(text["header"][0].split()[0], "@@REIFY_RETRY_SCOPE=failed_only@@")
        self.check_header(by_name["header"], text["header"])
        for name in ("scope", "narrowing"):
            self.assertEqual(fields_of(by_name[name]), scope_record[name], f"{label}: {name}")
        envs = [record for record in records if record["record"] == "env"]
        self.assertEqual([record["render"] for record in envs], text["environment"][1:])
        steps = [record for record in records if record["record"] == "step"]
        self.check_steps(label, steps, text["commands"])

    def check_header(self, header, text_header):
        line = next(line for line in text_header if line.startswith(PLAN_HEADER))
        tokens = dict(token.split("=", 1) for token in line[len(PLAN_HEADER):].split())
        fields = fields_of(header)
        profiles = fields.pop("profiles")
        self.assertEqual({name: str(value) for name, value in fields.items()}, tokens)
        both = tokens["profile"] == "both"
        self.assertEqual(profiles, ["debug", "release"] if both else [tokens["profile"]])

    def check_steps(self, label, steps, text_commands):
        renders = [step["render"] for step in steps]
        self.assertEqual(renders, [line for line in text_commands[1:] if not line.startswith("#")])
        self.assertEqual([step["index"] for step in steps], list(range(len(steps))))
        self.assertEqual([step["phase"] for step in steps], self.classified_phases(renders))
        for step in steps:
            self.assertIn("type", step["command"])
            self.assertEqual(step["scrub"], step["kind"] == "tool", step)
            self.assertEqual(step["render"].startswith(LD_SCRUB), step["scrub"], step)
            executes = "structural" if step["kind"] == "main_shell" else "render"
            self.assertEqual(step["executes"], executes, step)
        gate_paths = sorted({step["gate_path"] for step in steps if step["gate_path"] is not None})
        if gate_paths:
            tracked = self.clone.git("ls-files", "--", *gate_paths).splitlines()
            self.assertEqual(sorted(tracked), gate_paths, f"{label}: every gate path is a tracked file")


def fields_of(record) -> dict:
    return {name: value for name, value in record.items() if name not in NOT_RECORD_FIELDS}


class TestExpectedDivergences(InParityClone):
    """PRD D8's listed divergences in γ1's corpus, each asserted positively both ways.

    python equals bash's outcome as the entry transforms it, and bash differs
    from python; a stale entry reds, naming itself for deletion.
    """

    def test_each_listed_divergence_holds(self):
        for expected in EXPECTED_DIVERGENCES:
            with self.subTest(expected.name):
                self.clone.reset()
                failure = check_divergence(self.clone, expected)
                if failure is not None:
                    self.fail(failure)


class TestTestRegionNotYetPlanned(InParityClone):
    """γ1 refuses a test or all plan that holds a test region, rather than print it without one.

    γ2 (#9099) ports the region and deletes this class.
    """

    def test_a_test_region_plan_is_refused(self):
        scenarios = [
            Scenario(f"{action} --scope all role={role}", (action, "--scope", "all", "--print-plan"), role_env(role))
            for action in TEST_ACTIONS
            for role in ROLES
        ]
        for scenario, pair in run_many(self.clone, scenarios):
            with self.subTest(scenario.label):
                self.assert_bash_plans(scenario, pair)
                self.assertIn(TEST_REGION_BEGIN, pair.bash.stdout.splitlines(), "premise: a test region")
                self.assertEqual(pair.py.returncode, EX_UNAVAILABLE, pair.py.stderr)
                self.assertEqual(pair.py.stdout, "")
                lines = pair.py.stderr.splitlines()
                self.assertEqual(len(lines), 1, pair.py.stderr)
                self.assertTrue(lines[0].startswith("verify.sh: ERROR — "), lines[0])
                self.assertIn("test region", lines[0])
                self.assertIn("S18 cutover", lines[0])


class TestPlanParitySelfTest(InParityClone):
    """B1 tests the tree under test: an uncommitted package edit shows up as a divergence."""

    def nested(self, scratch: str) -> Clone:
        return Clone(shared_clone(Path(scratch) / "nested", source=self.clone.root))

    def test_a_clean_tree_shows_no_divergence(self):
        with tempfile.TemporaryDirectory() as scratch:
            pair = run_both(self.nested(scratch), SELF_TEST_PLAN)
        self.assert_parity(SELF_TEST_PLAN, pair)

    def test_an_uncommitted_render_edit_is_a_named_divergence(self):
        render = self.clone.root / RENDER_MODULE
        self.assertTrue(render.is_file(), f"premise: {RENDER_MODULE} exists")
        source = render.read_text(encoding="utf-8")
        self.assertIn(COMMANDS_MARKER, source, f"premise: {RENDER_MODULE} renders the marker")
        render.write_text(
            source.replace(COMMANDS_MARKER, "# --- commands, edited by B1's self-test ("),
            encoding="utf-8",
        )
        with tempfile.TemporaryDirectory() as scratch:
            pair = run_both(self.nested(scratch), SELF_TEST_PLAN)
        divergence = compare(pair.bash, pair.py)
        if divergence is None:
            self.fail("the edited render.py did not reach B1's plan")
        self.assertIn(PORT_WINDOW_RULE, divergence.message)
        self.assertIn("render.py", divergence.message)


if __name__ == "__main__":
    unittest.main(verbosity=2)
