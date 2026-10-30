#!/usr/bin/env python3
"""
test_verify_plan_parity.py — B1's core member: bash `scripts/verify.sh` and
Python `scripts/verify.py` print the same plan (S18 γ1, task 9098; PRD
docs/prds/v0_6/verify-structured-plan-and-role-table.md D8, D13).

Every check plans one scenario on both sides, as subprocesses, in the shared
clone of the tree under test, and compares exit code, stdout and stderr byte
for byte. This module never imports reify_verify. Bash is the oracle until the
cutover: a divergence reds, and its message names the module to edit and the
port-window rule (PRD D13).

The runner, the scenario record and compare() live in
tests/infra/plan_parity_harness.py. The lint and typecheck grids are the
sibling shards test_verify_plan_parity_{lint,typecheck}.py.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its
thin wrapper tests/infra/test_verify_plan_parity.sh.
"""

import itertools
import os
import sys
import tempfile
import unittest
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(INFRA_DIR))
from plan_parity_harness import (  # noqa: E402
    PORT_WINDOW_RULE,
    Clone,
    InParityClone,
    Scenario,
    base_env,
    compare,
    discard_clone,
    run_both,
    run_many,
    shared_clone,
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
        self.assertIsNotNone(divergence, "the edited render.py did not reach B1's plan")
        self.assertIn(PORT_WINDOW_RULE, divergence.message)
        self.assertIn("render.py", divergence.message)


if __name__ == "__main__":
    unittest.main(verbosity=2)
