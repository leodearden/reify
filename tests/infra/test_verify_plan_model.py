#!/usr/bin/env python3
"""
test_verify_plan_model.py — the Python verify package's plan model, in
process: the invariants its records and build_steps enforce, and the seams no
--print-plan reaches (S18 γ1 task 9098, γ2 task 9099; PRD
docs/prds/v0_6/verify-structured-plan-and-role-table.md D2, D8, D12).

The plan-parity members (test_verify_plan_parity*.py) compare whole plans
through the CLI and never import reify_verify. What is out of their reach is
here:

- the invariants: a step's command record must suit its kind, an emitter must
  declare every gate path its steps name, PlanInputs holds a test region
  exactly when the plan has one, and a record rendered inside quotes refuses
  that quote;
- nextest.nextest_config_path's generate arm, which only the executor reaches;
- bashcall.source_delta over a scratch ~/.cargo/env, and the shell options
  the lib seams run under, over a scratch lib.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its
thin wrapper tests/infra/test_verify_plan_model.sh.
"""

import dataclasses
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

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
from reify_verify.plan import (  # noqa: E402
    ArtifactAssertion,
    AttemptStamp,
    Guarded,
    Marker,
    NextestFlags,
    NodeProject,
    Phase,
    Refused,
    Script,
    Step,
    StepKind,
)
from reify_verify.policy import HeavyPartition, Profile  # noqa: E402

LINT_ALL_INFRA = ["lint", "--scope", "all", "--include-infra", "--print-plan"]


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

    verify.sh sources its libs under `set -euo pipefail` and calls their
    functions inside `$(...)` or under `||`, where errexit does not reach:
    so nounset and pipefail hold in a call, errexit does not, and all three
    hold while a lib is sourced.
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
        self.assertFalse(config.path.endswith("\n"), config.path)
        self.assertEqual(self.config_files(), {Path(config.path)})
        self.assertTrue(Path(config.path).is_file())

    def test_a_cli_thread_count_above_the_pool_is_the_generators_refusal(self):
        env = base_env(TMPDIR=str(self.tmpdir), REIFY_NEXTEST_TEST_THREADS="2")
        before = dict(env)
        refused = self.config_path(True, env, test_threads=8)
        self.assertIsInstance(refused, Refused)
        self.assertEqual(refused.exit_code, 64)
        self.assertEqual(refused.lines[-1], self.DIE_LINE)
        self.assertGreater(len(refused.lines), 1, refused.lines)
        self.assertTrue(
            all(line.startswith("gen-nextest-config.sh:") for line in refused.lines[:-1]),
            refused.lines,
        )
        self.assertIn("--test-threads=8", refused.lines[0])
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
            prio="", test_threads=None, config=None, heavy_filter="H", no_fail_fast=False, confirm=None
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


if __name__ == "__main__":
    unittest.main(verbosity=2)
