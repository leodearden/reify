#!/usr/bin/env python3
"""
test_verify_plan_model.py — the Python verify package's plan model, in
process: its records' invariants, and the test-side emitters no γ1-printable
plan reaches, compared with bash (S18 γ1, task 9098; PRD
docs/prds/v0_6/verify-structured-plan-and-role-table.md D2, D8, D12).

The plan-parity members (test_verify_plan_parity*.py) compare whole plans
through the CLI and never import reify_verify. Two things are out of their
reach, so this member imports the package:

- the invariants the records and build_steps enforce: a step's command
  record must suit its kind, an emitter must declare every gate path its
  steps name, and a record rendered inside single quotes refuses one;
- the steps that need the test family over the Rust crates: the test-side
  sync_comments check and the merge-tier pool (its release pre-builds, the
  #4624 BUILTIN assertion, the bin-sha stamp and run_all.sh). The CLI refuses
  such a plan until γ2 (#9099) ports the test region, so these steps are
  built here from a PlanInputs, and each rendered step must be a line of
  bash's plan, in bash's order. B1 compares them whole once γ2 lands.

It also drives bashcall.source_delta over a scratch ~/.cargo/env.

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
    policy,
    render,
    scope,
    selection,
)
from reify_verify.bashcall import BashCallError, SourcedFile  # noqa: E402
from reify_verify.plan import (  # noqa: E402
    ArtifactAssertion,
    Guarded,
    HeadStamp,
    NodeProject,
    Phase,
    Script,
    Step,
    StepKind,
)

HANG_GUARD_SECS = 300
COMMANDS_MARKER = "# --- commands"
TEST_ALL_INFRA = ["test", "--scope", "all", "--include-infra", "--print-plan"]


def base_env(**overrides):
    """hermetic_env() without any REIFY_* or DF_* key, then `overrides`."""
    env = {k: v for k, v in hermetic_env().items() if not k.startswith(("REIFY_", "DF_"))}
    env.update(overrides)
    return env


def bash_command_lines(argv, env) -> list[str]:
    """The command lines of bash's --print-plan: every non-comment line after the commands marker."""
    proc = subprocess.run(
        ["bash", str(REPO_ROOT / "scripts" / "verify.sh"), *argv],
        cwd=REPO_ROOT,
        env=env,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=HANG_GUARD_SECS,
    )
    if proc.returncode != 0:
        raise AssertionError(f"bash verify.sh {argv} exited {proc.returncode}: {proc.stderr!r}")
    lines = proc.stdout.decode("utf-8").splitlines()
    starts = [n for n, line in enumerate(lines) if line.startswith(COMMANDS_MARKER)]
    if len(starts) != 1:
        raise AssertionError(f"bash's plan has {len(starts)} commands markers")
    return [line for line in lines[starts[0] + 1:] if not line.startswith("#")]


def test_side_inputs(env, infra_suite_active=False) -> build.PlanInputs:
    """PlanInputs for `verify.sh test --scope all --include-infra`, resolved as the stages resolve them.

    The role must not be SELECTIVE: its selected leaves would need the
    git-env scrub prefix, which build resolves privately.
    """
    invocation = cli.parse_invocation(list(TEST_ALL_INFRA), env)
    if invocation.action is None:
        raise AssertionError("TEST_ALL_INFRA names no action")
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
        infra_suite_active=infra_suite_active,
        git_env_scrub_prefix=None,
    )


class TestTestSideEmittersAgainstBash(unittest.TestCase):
    """The steps the CLI cannot print yet, rendered, are lines of bash's plan in bash's order."""

    def assert_in_bash_order(self, steps, bash_lines):
        remaining = iter(bash_lines)
        for step in steps:
            rendered = render.render_step(step)
            if not any(line == rendered for line in remaining):
                self.fail(
                    f"{step.phase.value} step is not a line of bash's plan after the "
                    f"previous step's line:\n{rendered}\nbash's command lines:\n"
                    + "\n".join(bash_lines)
                )

    def test_merge_tier_pool_and_test_side_infra_check(self):
        env = base_env(DF_VERIFY_ROLE="merge")
        steps = build.build_steps(test_side_inputs(env))
        phases = [step.phase for step in steps]
        self.assertEqual(phases.count(Phase.RELEASE_PREBUILD), 2)
        self.assertIn(Phase.INFRA_POOL, phases)
        self.assertIn(Phase.INFRA_STATIC, phases)
        self.assertIn(StepKind.BUILTIN, [step.kind for step in steps])
        self.assertTrue(any(_holds(step, HeadStamp) for step in steps))
        self.assert_in_bash_order(steps, bash_command_lines(TEST_ALL_INFRA, env))

    def test_an_active_infra_suite_drops_the_pool_on_both_sides(self):
        env = base_env(DF_VERIFY_ROLE="merge", REIFY_INFRA_SUITE_ACTIVE="1")
        steps = build.build_steps(test_side_inputs(env, infra_suite_active=True))
        bash_lines = bash_command_lines(TEST_ALL_INFRA, env)
        self.assertNotIn(Phase.INFRA_POOL, [step.phase for step in steps])
        self.assertFalse(any(build.RUN_ALL.path in line for line in bash_lines), bash_lines)
        self.assertIn(Phase.INFRA_STATIC, [step.phase for step in steps])
        self.assert_in_bash_order(steps, bash_lines)


def _holds(step: Step, record_type) -> bool:
    command = step.command
    return isinstance(command, record_type) or (
        isinstance(command, Guarded) and isinstance(command.body, record_type)
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
        cls.inputs = test_side_inputs(base_env(DF_VERIFY_ROLE="merge"))

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


class TestPlanInputsScrubPrefix(unittest.TestCase):
    def test_a_prefix_without_selective_infra_is_refused(self):
        inputs = test_side_inputs(base_env(DF_VERIFY_ROLE="merge"))
        with self.assertRaises(ValueError) as raised:
            dataclasses.replace(inputs, git_env_scrub_prefix="env -u GIT_DIR")
        self.assertIn("selective infra", str(raised.exception))


if __name__ == "__main__":
    unittest.main(verbosity=2)
