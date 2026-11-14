#!/usr/bin/env python3
"""
test_verify_plan_model.py — the Python verify package's plan model, in
process: the invariants its records and build_steps enforce, and the seams no
--print-plan reaches (S18 γ1 task 9098, γ2 task 9099; PRD
docs/prds/v0_6/verify-structured-plan-and-role-table.md D2, D8, D12).

The plan-parity members (test_verify_plan_parity*.py) compare whole plans
through the CLI and never import reify_verify. What is out of their reach is
here:

- the invariants, each stated by the test class that pins it;
- the region's refusal of a quote-bearing path knob for every path it
  renders, of which B1 asserts one (the attempt sidecar), and the retry
  subset's own lines;
- nextest.nextest_config_path's generate arm, which only the executor reaches,
  and the removal of a generated config when planning fails after it;
- bashcall.source_delta over a scratch ~/.cargo/env, and the shell options
  the lib seams run under, over a scratch lib.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its
thin wrapper tests/infra/test_verify_plan_model.sh.
"""

import dataclasses
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

INFRA_DIR = Path(__file__).resolve().parent
REPO_ROOT = INFRA_DIR.parents[1]
sys.path.insert(0, str(INFRA_DIR))
sys.path.insert(0, str(REPO_ROOT / "scripts"))
from verify_ledger_env import hermetic_env  # noqa: E402

from reify_verify import (  # noqa: E402
    bashcall,
    build,
    cli,
    environment,
    gatepaths,
    narrowing,
    nextest,
    policy,
    render,
    retry,
    scope,
    selection,
    testregion,
)
from reify_verify.bashcall import BashCallError, SourcedFile  # noqa: E402
from reify_verify.invocation import Action  # noqa: E402
from reify_verify.nextest import (  # noqa: E402
    ConfigOrigin,
    ConfirmPaths,
    ConfirmRecording,
    NextestConfig,
    PassKind,
    PassSpec,
    TestRunner,
)
from reify_verify.plan import (  # noqa: E402
    CONFIRM_RECORDER,
    ArtifactAssertion,
    AttemptStamp,
    Cargo,
    CargoTestRun,
    Chain,
    ConfirmRecorded,
    Guarded,
    Marker,
    NextestFlags,
    NextestRun,
    NodeProject,
    Packages,
    Phase,
    Refused,
    Script,
    SlotFdClosed,
    Step,
    StepKind,
    Workspace,
)
from reify_verify.policy import HeavyPartition, Profile, Role  # noqa: E402

LINT_ALL_INFRA = ["lint", "--scope", "all", "--include-infra", "--print-plan"]
GUI_CRATE = Packages((narrowing.GUI_CRATE,))
OVERRIDE_CONFIG = NextestConfig("/caller/nextest.toml", ConfigOrigin.OVERRIDE)
RECORDING = ConfirmRecording("j.xml", ((Profile.DEBUG, ConfirmPaths("m.txt", "s.json")),))


def base_env(**overrides):
    """hermetic_env() without any REIFY_* or DF_* key, then `overrides`."""
    env = {k: v for k, v in hermetic_env().items() if not k.startswith(("REIFY_", "DF_"))}
    env.update(overrides)
    return env


def lint_inputs(env) -> build.PlanInputs:
    """PlanInputs for `verify.sh lint --scope all --include-infra`, resolved as the stages resolve them.

    A lint plan has no test region. The role must not be SELECTIVE: its
    selected leaves would need the git-env scrub prefix, which build resolves
    privately.
    """
    invocation = cli.parse_invocation(list(LINT_ALL_INFRA), env)
    if invocation.action is None:
        raise AssertionError("LINT_ALL_INFRA names no action")
    role_policy = policy.ROLE_TABLE[invocation.role]
    if role_policy.infra_tier is policy.InfraTier.SELECTIVE:
        raise AssertionError(f"role {invocation.role.value} is SELECTIVE")
    planning = environment.build(invocation.role, role_policy, env, REPO_ROOT).planning
    decision, _ = scope.decide(invocation, REPO_ROOT, env)
    selected, _ = selection.select_infra(decision, REPO_ROOT, planning)
    narrowed, _ = narrowing.narrow(decision, invocation.narrow, REPO_ROOT, planning)
    families = build.Families.of(invocation.action)
    vitest, _ = build.VitestArm.of(families, narrowed, planning)
    walls, _ = build.Walls.resolve(role_policy, env)
    prio = policy.resolve_prio(
        role_policy,
        invocation.role.value,
        shutil.which("nice", path=env.get("PATH")) is not None,
        shutil.which("ionice", path=env.get("PATH")) is not None,
    )
    return build.PlanInputs(
        families=families,
        include_infra=invocation.include_infra,
        decision=decision,
        narrowing=narrowed,
        selection=selected,
        role_policy=role_policy,
        cargo_prio=prio.prefix,
        walls=walls,
        heavy_filter=bashcall.read_export(build.HEAVY_FILTER, REPO_ROOT, env),
        node_lane=build.NodeLane.compose(families, decision, vitest),
        infra_suite_active=False,
        git_env_scrub_prefix=None,
        test_region=None,
    )


class TestStepInvariants(unittest.TestCase):
    def test_a_kind_refuses_a_command_it_cannot_hold(self):
        with self.assertRaises(ValueError) as raised:
            Step(StepKind.BUILTIN, Phase.BOOKKEEPING, Script("scripts/x.sh"), None, None)
        message = str(raised.exception)
        for part in ("builtin", "Script", "ArtifactAssertion"):
            self.assertIn(part, message)

    def test_a_kind_holds_a_command_it_allows(self):
        step = Step(StepKind.TOOL, Phase.OTHER, Script("scripts/x.sh"), None, None)
        self.assertEqual(render.render_step(step), f"{render.LD_SCRUB}./scripts/x.sh")


class TestEmitterGateDeclarations(unittest.TestCase):
    """build_steps keeps each emitter's declared gate paths the complete list (PRD D12)."""

    @classmethod
    def setUpClass(cls):
        cls.inputs = lint_inputs(base_env(DF_VERIFY_ROLE="merge"))

    def emitter(self, declared):
        step = Step(StepKind.TOOL, Phase.OTHER, Script("scripts/named.sh"), "scripts/named.sh", None)
        return build.Emitter("probe emitter", declared, lambda _inputs: (step,))

    def test_an_undeclared_gate_path_is_refused(self):
        with self.assertRaises(ValueError) as raised:
            build.build_steps(self.inputs, (self.emitter(("scripts/other.sh",)),))
        message = str(raised.exception)
        self.assertIn("'probe emitter'", message)
        self.assertIn("'scripts/named.sh'", message)

    def test_a_declared_gate_path_is_kept(self):
        steps = build.build_steps(self.inputs, (self.emitter(("scripts/named.sh",)),))
        self.assertEqual([step.gate_path for step in steps], ["scripts/named.sh"])


class TestQuotedTextRecords(unittest.TestCase):
    """A record whose text renders inside single quotes refuses a single quote."""

    def test_guarded_else_warning(self):
        with self.assertRaises(ValueError):
            Guarded("-f", "a.sh", Script("a.sh"), "it's missing")

    def test_artifact_assertion_error(self):
        with self.assertRaises(ValueError):
            ArtifactAssertion("Cargo.toml", "target/x", "don't skip")

    def test_node_project_command(self):
        with self.assertRaises(ValueError):
            NodeProject("-d", "gui", "gui", 15, ("npm run 'typecheck'",))

    def test_node_project_command_is_never_a_compound(self):
        with self.assertRaises(ValueError):
            NodeProject("-d", "gui", "gui", 15, ("npm ci && npm test",))

    def test_node_project_wraps_only_several_commands(self):
        one = NodeProject("-f", "t/package-lock.json", "t", 10, ("npm ci",))
        two = NodeProject("-f", "t/package-lock.json", "t", 10, ("npm ci", "npm test"))
        self.assertEqual(
            render.render_command(one),
            "if test -f t/package-lock.json; then (cd t && timeout --kill-after=60 10m npm ci); fi",
        )
        self.assertIn("bash -c 'npm ci && npm test'", render.render_command(two))


class TestSourceDelta(unittest.TestCase):
    """bashcall.source_delta over a scratch $HOME/.cargo/env."""

    def delta_of(self, text, **env):
        with tempfile.TemporaryDirectory() as home:
            (Path(home) / ".cargo").mkdir()
            (Path(home) / ".cargo" / "env").write_text(text, encoding="utf-8")
            return bashcall.source_delta(
                SourcedFile(".cargo/env"), REPO_ROOT, base_env(HOME=home, **env)
            )

    def test_assignments_and_removals(self):
        delta = self.delta_of(
            "export DELTA_SET=one\nunset DELTA_GONE\n", DELTA_GONE="x", DELTA_KEPT="y"
        )
        self.assertEqual(delta.assigned, (("DELTA_SET", "one"),))
        self.assertEqual(delta.removed, ("DELTA_GONE",))

    def test_the_files_own_stdout_is_not_an_environment_record(self):
        delta = self.delta_of("printf 'noise without a newline'\nexport DELTA_SET=one\n")
        self.assertEqual(delta.assigned, (("DELTA_SET", "one"),))
        self.assertEqual(delta.removed, ())

    def test_a_file_that_ends_the_shell_is_an_error_not_an_empty_environment(self):
        with self.assertRaises(BashCallError) as raised:
            self.delta_of("exit 0\n")
        self.assertIn("ended before", str(raised.exception))

    def test_a_failing_file_is_an_error(self):
        with self.assertRaises(BashCallError):
            self.delta_of("return 3\n")


class TestSeamShellOptions(unittest.TestCase):
    """bashcall's lib seams run under the shell options verify.sh's call sites run under.

    verify.sh runs under `set -euo pipefail` and calls lib functions inside
    `$(...)` or under `||`, where errexit does not reach: so nounset and
    pipefail hold in a call and errexit does not. A lib whose sourcing ends
    in a failing pipeline is not sourced.
    """

    LIB = "lib.sh"
    FUNCTIONS = (
        "piped(){ false | true; }\n"
        'unbound(){ echo "${B1_SEAM_UNSET_VAR}"; }\n'
        "continues(){ false; echo after; }\n"
    )

    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.repo_root = Path(scratch.name)
        self.env = base_env()
        self.env.pop("B1_SEAM_UNSET_VAR", None)

    def write_lib(self, text):
        (self.repo_root / self.LIB).write_text(text, encoding="utf-8")

    def ask(self, function):
        self.write_lib(self.FUNCTIONS)
        return bashcall.ask(bashcall.BashCall(self.LIB, function), (), self.repo_root, self.env)

    def test_a_failing_pipeline_stage_fails_the_call(self):
        self.assertNotEqual(self.ask("piped").returncode, 0)
        with self.assertRaises(BashCallError):
            bashcall.run(bashcall.BashCall(self.LIB, "piped"), (), self.repo_root, self.env)

    def test_an_unset_variable_fails_the_call(self):
        result = self.ask("unbound")
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(
            any("B1_SEAM_UNSET_VAR: unbound variable" in line for line in result.stderr_lines),
            result.stderr_lines,
        )

    def test_a_failing_command_does_not_end_the_call(self):
        result = self.ask("continues")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.lines, ("after",))

    def test_a_lib_whose_top_level_pipeline_fails_is_not_sourced(self):
        self.write_lib("B1_EXPORTED=x; export B1_EXPORTED; false | true\n")
        with self.assertRaises(BashCallError):
            bashcall.read_export(bashcall.BashExport(self.LIB, "B1_EXPORTED"), self.repo_root, self.env)


class TestNextestConfigPath(unittest.TestCase):
    """nextest.nextest_config_path's three arms.

    No --print-plan reaches the generate arm (it is the executor's, from ε),
    so this class drives the function itself, in the repo checkout.
    """

    PLACEHOLDER_NAME = "reify-nextest-occt.<print-plan-placeholder>"
    DIE_LINE = (
        "verify.sh: ERROR — gen-nextest-config.sh refused to generate the nextest "
        "config (exit 64); its diagnostic is immediately above"
    )

    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.tmpdir = Path(scratch.name)

    def config_files(self) -> set[Path]:
        return set(self.tmpdir.glob("reify-nextest-occt.*"))

    def generator_stderr(self, env, test_threads) -> list[str]:
        """What gen-nextest-config.sh itself prints on stderr under `env` and the CLI thread count."""
        run = subprocess.run(
            [str(REPO_ROOT / nextest.GEN_NEXTEST_CONFIG.path)],
            cwd=REPO_ROOT,
            env={**env, nextest.CLI_TEST_THREADS: str(test_threads)},
            capture_output=True,
            text=True,
        )
        if run.returncode == 0:
            Path(run.stdout.rstrip("\n")).unlink(missing_ok=True)
        return run.stderr.splitlines()

    def config_path(self, executing, raw_env, planning_env=None, test_threads=None):
        return nextest.nextest_config_path(
            executing,
            raw_env,
            raw_env if planning_env is None else planning_env,
            test_threads,
            REPO_ROOT,
        )

    def test_an_override_is_the_callers_file_in_both_modes(self):
        env = base_env(REIFY_VERIFY_NEXTEST_CONFIG="/some/caller.toml", TMPDIR=str(self.tmpdir))
        for executing in (False, True):
            with self.subTest(executing=executing):
                config = self.config_path(executing, env)
                self.assertEqual(
                    config, nextest.NextestConfig("/some/caller.toml", nextest.ConfigOrigin.OVERRIDE)
                )
        self.assertEqual(self.config_files(), set())

    def test_the_override_is_read_from_the_raw_env(self):
        raw = base_env(TMPDIR=str(self.tmpdir))
        planning = dict(raw, REIFY_VERIFY_NEXTEST_CONFIG="/planning/only.toml")
        config = self.config_path(False, raw, planning)
        self.assertEqual(config.origin, nextest.ConfigOrigin.PLACEHOLDER)

    def test_print_mode_names_the_placeholder_under_the_planning_tmpdir(self):
        raw = base_env(TMPDIR="/raw/tmp")
        planning = dict(raw, TMPDIR=str(self.tmpdir))
        config = self.config_path(False, raw, planning)
        self.assertEqual(
            config,
            nextest.NextestConfig(
                f"{self.tmpdir}/{self.PLACEHOLDER_NAME}", nextest.ConfigOrigin.PLACEHOLDER
            ),
        )
        self.assertEqual(self.config_files(), set())

    def test_an_unset_or_empty_tmpdir_falls_back_to_tmp(self):
        unset = {k: v for k, v in base_env().items() if k != "TMPDIR"}
        for label, env in (("unset", unset), ("empty", dict(unset, TMPDIR=""))):
            with self.subTest(tmpdir=label):
                config = self.config_path(False, env)
                self.assertEqual(config.path, f"/tmp/{self.PLACEHOLDER_NAME}")
                self.assertEqual(config.origin, nextest.ConfigOrigin.PLACEHOLDER)

    def test_execute_mode_generates_one_file_the_executor_owns(self):
        env = base_env(TMPDIR=str(self.tmpdir), REIFY_NEXTEST_TEST_THREADS="2")
        config = self.config_path(True, env)
        self.assertIsInstance(config, nextest.NextestConfig)
        self.addCleanup(Path(config.path).unlink, missing_ok=True)
        self.assertEqual(config.origin, nextest.ConfigOrigin.GENERATED)
        self.assertEqual(config.diagnostics, ())
        self.assertFalse(config.path.endswith("\n"), config.path)
        self.assertEqual(self.config_files(), {Path(config.path)})
        self.assertTrue(Path(config.path).is_file())

    def test_a_generated_config_carries_the_generators_success_note(self):
        env = base_env(TMPDIR=str(self.tmpdir), REIFY_NEXTEST_TEST_THREADS="2")
        config = self.config_path(True, env, test_threads=1)
        self.assertIsInstance(config, nextest.NextestConfig)
        self.addCleanup(Path(config.path).unlink, missing_ok=True)
        self.assertEqual(config.origin, nextest.ConfigOrigin.GENERATED)
        expected = self.generator_stderr(env, 1)
        self.assertTrue(expected, "premise: a CLI cap under the pool makes the generator print a note")
        self.assertEqual(list(config.diagnostics), expected)

    def test_a_cli_thread_count_above_the_pool_is_the_generators_refusal(self):
        env = base_env(TMPDIR=str(self.tmpdir), REIFY_NEXTEST_TEST_THREADS="2")
        before = dict(env)
        refused = self.config_path(True, env, test_threads=8)
        self.assertIsInstance(refused, Refused)
        self.assertEqual(refused.exit_code, 64)
        self.assertEqual(refused.lines[-1], self.DIE_LINE)
        expected = self.generator_stderr(env, 8)
        self.assertTrue(expected, "premise: the generator prints why it refuses")
        self.assertEqual(list(refused.lines[:-1]), expected)
        self.assertEqual(env, before)
        self.assertNotIn("REIFY_NEXTEST_CLI_TEST_THREADS", env)
        self.assertEqual(self.config_files(), set())


class TestPlanInputsScrubPrefix(unittest.TestCase):
    def test_a_prefix_without_selective_infra_is_refused(self):
        inputs = lint_inputs(base_env(DF_VERIFY_ROLE="merge"))
        with self.assertRaises(ValueError) as raised:
            dataclasses.replace(inputs, git_env_scrub_prefix="env -u GIT_DIR")
        self.assertIn("selective infra", str(raised.exception))


class TestPlanInputsTestRegion(unittest.TestCase):
    """PlanInputs holds a test region exactly when the plan has the test family over RUN_RUST."""

    @classmethod
    def setUpClass(cls):
        cls.inputs = lint_inputs(base_env(DF_VERIFY_ROLE="merge"))

    def region(self) -> testregion.TestRegion:
        runner = nextest.TestRunner(
            prio="", test_threads=None, nextest=False, config=None, heavy_filter="H",
            no_fail_fast=False, confirm=None,
        )
        return testregion.TestRegion(
            attempt_stamp=None, passes=(), gui_feature=None, runner=runner,
            heavy_header=HeavyPartition.INCLUDED, retry_applied=retry.NO_NEXTEST_SUBSETS, diagnostics=(),
        )

    def test_a_region_on_a_plan_without_one_is_refused(self):
        self.assertTrue(self.inputs.decision.run_rust, "premise: scope all runs the Rust families")
        with self.assertRaises(ValueError) as raised:
            dataclasses.replace(self.inputs, test_region=self.region())
        message = str(raised.exception)
        self.assertIn("test_region=", message)
        self.assertIn("no test region", message)

    def test_no_region_on_a_plan_with_one_is_refused(self):
        with self.assertRaises(ValueError) as raised:
            dataclasses.replace(self.inputs, families=build.Families.of(Action.TEST))
        message = str(raised.exception)
        self.assertIn("test_region=None", message)
        self.assertIn("has a test region", message)


class TestQuotedRegionRecords(unittest.TestCase):
    """A region record rendered inside quotes refuses that quote, naming the record and field."""

    def assert_refused(self, build_record, record, field):
        with self.assertRaises(ValueError) as raised:
            build_record()
        message = str(raised.exception)
        self.assertIn(record, message)
        self.assertIn(field, message)

    def flags(self, **overrides):
        fields = dict(
            heavy=HeavyPartition.EXCLUDED, heavy_filter="package(a)", no_fail_fast=False,
            test_threads=None, retry_ids=(),
        )
        fields.update(overrides)
        return NextestFlags(**fields)

    def test_a_retry_id_holding_a_single_quote(self):
        self.assert_refused(lambda: self.flags(retry_ids=("it's::a_test",)), "NextestFlags", "retry_ids")

    def test_a_heavy_filter_holding_a_double_quote(self):
        self.assert_refused(lambda: self.flags(heavy_filter='test("x")'), "NextestFlags", "heavy_filter")

    def test_a_marker_holding_a_single_quote(self):
        self.assert_refused(lambda: Marker("it's skipped"), "Marker", "text")

    def test_an_attempt_stamp_path_holding_a_double_quote(self):
        self.assert_refused(
            lambda: AttemptStamp('target/"attempt".json', (Profile.DEBUG,)), "AttemptStamp", "path"
        )

    def test_a_heavy_filter_holding_a_single_quote_beside_retry_ids(self):
        self.assert_refused(
            lambda: self.flags(heavy_filter="test('x')", retry_ids=("a::b",)), "NextestFlags", "heavy_filter"
        )

    def test_a_heavy_filter_holding_a_single_quote_alone_is_double_quoted(self):
        run = nextest_run(flags=self.flags(heavy_filter="test('x')"))
        self.assertIn(""" -E "not (test('x'))" """, render.render_command(run))

    def test_a_confirm_path_holding_a_double_quote(self):
        for field in ("junit", "manifest", "sidecar"):
            with self.subTest(field=field):
                paths = {"junit": "j.xml", "manifest": "m.txt", "sidecar": "s.json", field: 'a"b'}
                self.assert_refused(
                    lambda: ConfirmRecorded(nextest_run(), profile=Profile.DEBUG, **paths),
                    "ConfirmRecorded",
                    field,
                )


def nextest_run(**overrides) -> NextestRun:
    fields = dict(
        selector=Workspace(), release=False, features=(), wall="60m", prio="",
        flags=NextestFlags(HeavyPartition.INCLUDED, "H", False, None, ()), config_file="c.toml",
    )
    fields.update(overrides)
    return NextestRun(**fields)


def recording_runner(**overrides) -> TestRunner:
    fields = dict(
        prio="", test_threads=None, nextest=True, config=OVERRIDE_CONFIG, heavy_filter="H",
        no_fail_fast=True, confirm=RECORDING,
    )
    fields.update(overrides)
    return TestRunner(**fields)


def region_inputs(role: Role, raw_env=None, **overrides) -> testregion.RegionInputs:
    """A print-mode region over both profiles that reaches reify-gui, under `raw_env` as both envs."""
    env = base_env() if raw_env is None else raw_env
    fields = dict(
        role_policy=policy.ROLE_TABLE[role],
        profiles=(Profile.DEBUG, Profile.RELEASE),
        debug_selector=Workspace(),
        release_declared=("reify-doc",),
        release_delta_skips=False,
        reaches_gui=True,
        walls=testregion.PassWalls("60m", "90m", "30m"),
        prio="",
        test_threads=None,
        heavy_filter="H",
        nextest=True,
        executing=False,
        raw_env=env,
        planning_env=env,
        repo_root=REPO_ROOT,
    )
    fields.update(overrides)
    return testregion.RegionInputs(**fields)


class TestSlotFdClosed(unittest.TestCase):
    """The slot-closed wrapper a CARGO step admits holds one test pass, bare or behind its crate's guard."""

    def test_a_bare_or_guarded_test_pass_is_held(self):
        run = nextest_run()
        bodies = (
            run,
            CargoTestRun(Workspace(), False, (), "60m", "", 1),
            ConfirmRecorded(run, "j.xml", "m.txt", "s.json", Profile.DEBUG),
            gatepaths.GUI_CRATE_GUARD.around(run),
        )
        for body in bodies:
            with self.subTest(body=type(body).__name__):
                step = Step(StepKind.CARGO, Phase.DEBUG_NEXTEST, SlotFdClosed(body), None, None)
                self.assertTrue(render.render_step(step).endswith(" 9<&-"))

    def test_any_other_body_is_refused(self):
        bodies = (
            Script("scripts/x.sh"),
            Cargo("build", Workspace(), (), "60m", ""),
            Guarded("-f", "Cargo.toml", Script("scripts/x.sh")),
            Guarded("-f", "Cargo.toml", Chain((nextest_run(), Script("scripts/x.sh")))),
        )
        for body in bodies:
            with self.subTest(body=body):
                with self.assertRaises(ValueError) as raised:
                    SlotFdClosed(body)
                self.assertIn("SlotFdClosed", str(raised.exception))


class TestPassInvariants(unittest.TestCase):
    """A recording runner runs --no-fail-fast; the gui-feature pass is never filtered nor recorded;
    a recording pass carries no guard."""

    def test_a_recording_runner_runs_no_fail_fast(self):
        with self.assertRaises(ValueError) as raised:
            recording_runner(no_fail_fast=False)
        message = str(raised.exception)
        for part in ("TestRunner", "--no-fail-fast", "records=True", "fail_fast=True"):
            self.assertIn(part, message)

    def test_the_gui_feature_pass_carries_no_filter(self):
        for label, extra in (
            ("heavy excluded", {"heavy": HeavyPartition.EXCLUDED}),
            ("retry ids", {"retry_ids": ("a::b",)}),
        ):
            with self.subTest(label):
                with self.assertRaises(ValueError) as raised:
                    PassSpec(PassKind.GUI_FEATURE, Profile.DEBUG, GUI_CRATE, "30m", **extra)
                self.assertIn("gui-feature", str(raised.exception))

    def test_a_recording_pass_carries_no_guard(self):
        guard = gatepaths.GUI_CRATE_GUARD
        with self.assertRaises(ValueError) as raised:
            PassSpec(PassKind.PROFILE, Profile.DEBUG, Workspace(), "60m", guard=guard)
        message = str(raised.exception)
        for part in ("PassSpec", CONFIRM_RECORDER, guard.setup.path):
            self.assertIn(part, message)
        PassSpec(PassKind.GUI_FEATURE, Profile.DEBUG, GUI_CRATE, "30m", features=("gui",), guard=guard)

    def test_emitting_a_guarded_recorded_pass_is_refused_even_past_a_loosened_spec(self):
        guard = gatepaths.GUI_CRATE_GUARD
        with mock.patch.object(PassSpec, "__post_init__", lambda self: None):
            spec = PassSpec(PassKind.PROFILE, Profile.DEBUG, Workspace(), "60m", guard=guard)
        with self.assertRaises(ValueError) as raised:
            nextest.emit_pass(spec, recording_runner())
        message = str(raised.exception)
        for part in ("emit_pass", CONFIRM_RECORDER, guard.setup.path):
            self.assertIn(part, message)

    def test_only_a_profile_pass_records(self):
        runner = recording_runner()
        profile = nextest.emit_pass(PassSpec(PassKind.PROFILE, Profile.DEBUG, Workspace(), "60m"), runner)
        gui = nextest.emit_pass(
            PassSpec(
                PassKind.GUI_FEATURE, Profile.DEBUG, GUI_CRATE, "30m", features=("gui",),
                guard=gatepaths.GUI_CRATE_GUARD,
            ),
            runner,
        )
        self.assertIn(f"./{CONFIRM_RECORDER} record", render.render_step(profile))
        self.assertEqual(profile.gate_path, CONFIRM_RECORDER)
        self.assertNotIn(CONFIRM_RECORDER, render.render_step(gui))
        self.assertEqual(gui.gate_path, gatepaths.GUI_CRATE_GUARD.setup.path)

    def test_the_kind_not_the_features_names_the_phase(self):
        runner = recording_runner(confirm=None, no_fail_fast=False)
        gui = PassSpec(PassKind.GUI_FEATURE, Profile.DEBUG, GUI_CRATE, "30m")
        cases = (
            (gui, runner, Phase.GUI_FEATURE_NEXTEST),
            (gui, dataclasses.replace(runner, nextest=False, config=None), Phase.GUI_FEATURE_CARGO_TEST),
            (PassSpec(PassKind.PROFILE, Profile.DEBUG, Workspace(), "60m", features=("x",)), runner, Phase.DEBUG_NEXTEST),
            (PassSpec(PassKind.PROFILE, Profile.RELEASE, Workspace(), "90m"), runner, Phase.RELEASE_NEXTEST),
        )
        for spec, run_as, phase in cases:
            with self.subTest(phase=phase.value):
                self.assertEqual(nextest.emit_pass(spec, run_as).phase, phase)

    def test_every_roles_region_resolves(self):
        for role in Role:
            with self.subTest(role=role.value):
                region = testregion.resolve(region_inputs(role))
                self.assertIsInstance(region, testregion.TestRegion)
                gui = [step for step in testregion.steps(region) if step.phase is Phase.GUI_FEATURE_NEXTEST]
                self.assertEqual(len(gui), int(policy.ROLE_TABLE[role].gui_feature_test_pass))
                for step in gui:
                    self.assertNotIn(CONFIRM_RECORDER, render.render_step(step))


class TestRunnerArm(unittest.TestCase):
    """The runner states whether nextest or the cargo-test fallback runs its passes; config None
    only means no pass names a config."""

    def test_a_nextest_runner_without_a_config_refuses_to_emit_a_pass(self):
        runner = recording_runner(config=None, confirm=None, no_fail_fast=False)
        with self.assertRaises(ValueError) as raised:
            nextest.emit_pass(PassSpec(PassKind.PROFILE, Profile.RELEASE, Workspace(), "90m"), runner)
        message = str(raised.exception)
        self.assertIn("emit_pass", message)
        self.assertIn("config", message)
        self.assertIn("kind=profile", message)
        self.assertIn("profile=release", message)

    def test_the_cargo_test_fallback_names_no_config(self):
        with self.assertRaises(ValueError) as raised:
            recording_runner(nextest=False, confirm=None, no_fail_fast=False)
        message = str(raised.exception)
        self.assertIn("TestRunner", message)
        self.assertIn("config", message)

    def test_the_cargo_test_fallback_records_nothing(self):
        with self.assertRaises(ValueError) as raised:
            recording_runner(nextest=False, config=None)
        message = str(raised.exception)
        self.assertIn("TestRunner", message)
        self.assertIn("confirm", message)

    def test_the_fallback_runs_cargo_test(self):
        runner = recording_runner(nextest=False, config=None, confirm=None, no_fail_fast=False)
        step = nextest.emit_pass(PassSpec(PassKind.PROFILE, Profile.DEBUG, Workspace(), "60m"), runner)
        self.assertIsInstance(step.command, SlotFdClosed)
        self.assertIsInstance(step.command.body, CargoTestRun)
        self.assertEqual(step.phase, Phase.DEBUG_CARGO_TEST)

    def test_a_region_with_no_pass_naming_a_config_keeps_its_arm(self):
        for has_nextest in (True, False):
            with self.subTest(nextest=has_nextest):
                region = testregion.resolve(
                    region_inputs(
                        Role.MERGE, profiles=(Profile.RELEASE,), release_delta_skips=True,
                        reaches_gui=False, nextest=has_nextest,
                    )
                )
                self.assertIsInstance(region, testregion.TestRegion)
                self.assertFalse(
                    any(isinstance(spec, PassSpec) for spec in region.passes),
                    "premise: the release-delta decision skipped the only pass",
                )
                self.assertIsNone(region.gui_feature, "premise: no gui-feature pass")
                self.assertIsNone(region.runner.config)
                self.assertIs(region.runner.nextest, has_nextest)


class TestRolePolicyInvariants(unittest.TestCase):
    """A recording role runs --no-fail-fast, and may still run the gui-feature pass, which records nothing."""

    RECORDING_ROLE = policy.ROLE_TABLE[Role.OFFLINE]

    def test_a_recording_role_is_not_fail_fast(self):
        self.assertTrue(self.RECORDING_ROLE.records_confirm_manifest, "premise: offline records")
        self.assertFalse(self.RECORDING_ROLE.fail_fast, "premise: offline is not fail-fast")
        with self.assertRaises(ValueError) as raised:
            dataclasses.replace(self.RECORDING_ROLE, fail_fast=True)
        message = str(raised.exception)
        for part in ("RolePolicy", "--no-fail-fast", "records=True", "fail_fast=True"):
            self.assertIn(part, message)

    def test_a_recording_role_may_run_the_gui_feature_pass(self):
        role_policy = dataclasses.replace(self.RECORDING_ROLE, gui_feature_test_pass=True)
        region = testregion.resolve(region_inputs(Role.OFFLINE, role_policy=role_policy))
        self.assertIsInstance(region, testregion.TestRegion)
        manifest = region.runner.confirm.paths(Profile.DEBUG).manifest
        rendered = [(step.phase, render.render_step(step)) for step in testregion.steps(region)]
        recording = [phase for phase, text in rendered if manifest in text]
        self.assertEqual(recording, [Phase.DEBUG_NEXTEST])
        gui = [text for phase, text in rendered if phase is Phase.GUI_FEATURE_NEXTEST]
        self.assertEqual(len(gui), 1, "premise: the region runs the gui-feature pass")
        self.assertNotIn(CONFIRM_RECORDER, gui[0])


class TestQuotedPathKnobs(unittest.TestCase):
    """A path knob holding the quote its step renders it inside refuses the region, naming knob and value."""

    def assert_knob_refused(self, outcome, knob, value):
        self.assertIsInstance(outcome, Refused)
        self.assertEqual(outcome.exit_code, 1)
        self.assertEqual(len(outcome.lines), 1, outcome.lines)
        self.assertTrue(outcome.lines[0].startswith("verify.sh: ERROR — "), outcome.lines)
        self.assertIn(f"{knob}={value} holds a double quote", outcome.lines[0])

    def test_the_attempt_sidecar(self):
        value = 'target/a"b.json'
        outcome = testregion.resolve(region_inputs(Role.MERGE, base_env(REIFY_VERIFY_ATTEMPT_SIDECAR=value)))
        self.assert_knob_refused(outcome, "REIFY_VERIFY_ATTEMPT_SIDECAR", value)

    def test_a_role_that_stamps_no_sidecar_reads_none(self):
        env = base_env(REIFY_VERIFY_ATTEMPT_SIDECAR='target/a"b.json')
        self.assertIsInstance(testregion.resolve(region_inputs(Role.TASK, env)), testregion.TestRegion)

    def test_each_confirm_path(self):
        names = (
            "REIFY_VERIFY_CONFIRM_JUNIT",
            "REIFY_VERIFY_CONFIRM_MANIFEST_DEBUG",
            "REIFY_VERIFY_CONFIRM_SIDECAR_DEBUG",
            "REIFY_VERIFY_CONFIRM_MANIFEST_RELEASE",
            "REIFY_VERIFY_CONFIRM_SIDECAR_RELEASE",
        )
        for knob in names:
            with self.subTest(knob=knob):
                value = f'target/{knob.lower()}"x'
                outcome = testregion.resolve(region_inputs(Role.OFFLINE, base_env(**{knob: value})))
                self.assert_knob_refused(outcome, knob, value)

    def test_only_the_paths_a_pass_records_to_are_read(self):
        env = base_env(REIFY_VERIFY_CONFIRM_MANIFEST_RELEASE='target/m"x')
        for label, overrides in (
            ("debug only", {"profiles": (Profile.DEBUG,)}),
            ("cargo test records nothing", {"nextest": False}),
        ):
            with self.subTest(label):
                outcome = testregion.resolve(region_inputs(Role.OFFLINE, env, **overrides))
                self.assertIsInstance(outcome, testregion.TestRegion)


class TestRetrySubsetLines(unittest.TestCase):
    """retry.subset's lines of the package's own: the unquotable id and the non-integer ceiling."""

    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.filter_file = Path(scratch.name) / "filter.txt"

    def subset(self, ids, max_subset="5000"):
        self.filter_file.write_text(ids, encoding="utf-8")
        planning = base_env(REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE=str(self.filter_file))
        return retry.subset(Profile.DEBUG, planning, max_subset, REPO_ROOT)

    def test_an_unquotable_id_runs_the_pass_in_full(self):
        ids, lines = self.subset("a::b\nit's::c\n")
        self.assertEqual(ids, ())
        self.assertEqual(len(lines), 1, lines)
        self.assertTrue(lines[0].startswith(f"{retry.REFUSED}unquotable id — it's::c "), lines)
        self.assertTrue(lines[0].endswith("(profile=debug, full verify)"), lines)

    def test_a_non_integer_ceiling_warns_and_applies_none(self):
        ids, lines = self.subset("a::b\nc::d\n", max_subset="abc")
        self.assertEqual(ids, ("a::b", "c::d"))
        self.assertEqual(lines, (retry.no_ceiling_warning("abc"),))
        self.assertIn("REIFY_VERIFY_RETRY_MAX_SUBSET=abc", lines[0])

    def test_an_integer_ceiling_prints_nothing(self):
        self.assertEqual(self.subset("a::b\n", max_subset=" 5"), (("a::b",), ()))


class TestGeneratedConfigCleanup(unittest.TestCase):
    """A nextest config execute mode generated is removed when no plan will name it."""

    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.tmpdir = Path(scratch.name)

    def generated(self) -> list[Path]:
        return sorted(self.tmpdir.glob("reify-nextest-occt.*"))

    def test_discard_removes_only_a_generated_file(self):
        for origin in ConfigOrigin:
            with self.subTest(origin=origin.value):
                path = self.tmpdir / f"config-{origin.value}.toml"
                path.write_text("x", encoding="utf-8")
                NextestConfig(str(path), origin).discard()
                self.assertEqual(path.exists(), origin is not ConfigOrigin.GENERATED)

    def test_a_failure_after_the_region_removes_its_generated_config(self):
        env = base_env(TMPDIR=str(self.tmpdir))
        invocation = cli.parse_invocation(["test", "--scope", "all"], env)
        self.assertIsNone(invocation.plan_format, "premise: an execute-mode invocation")
        self.assertIsInstance(build.plan_invocation(invocation, env, REPO_ROOT), build.Planned)
        self.assertEqual(len(self.generated()), 1, "premise: planning it generates one config")
        for path in self.generated():
            path.unlink()
        with mock.patch.object(build, "build_steps", side_effect=RuntimeError("after the region")):
            with self.assertRaises(RuntimeError):
                build.plan_invocation(invocation, env, REPO_ROOT)
        self.assertEqual(self.generated(), [])


if __name__ == "__main__":
    unittest.main(verbosity=2)
