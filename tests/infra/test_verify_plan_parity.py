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
    compare,
    discard_clone,
    run_both,
    run_many,
    shared_clone,
)

PLAN_ACTIONS = ("lint", "typecheck")
ROLES = (None, "merge", "offline", "background")
COMMANDS_MARKER = "# --- commands ("
RENDER_MODULE = "scripts/reify_verify/render.py"
SELF_TEST_PLAN = Scenario(
    "self-test", ("lint", "--scope", "all", "--include-infra", "--print-plan")
)


def tearDownModule():
    discard_clone()


def role_env(role):
    return {"DF_VERIFY_ROLE": role}


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
