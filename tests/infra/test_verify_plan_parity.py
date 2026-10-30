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
ENVIRONMENT_SECTIONS = ("exit code", "environment", "stderr")
TASK_FIFO = "REIFY_JOBSERVER_TASK_FIFO"
MERGE_FIFO = "REIFY_JOBSERVER_MERGE_FIFO"
BOOT_ID = Path("/proc/sys/kernel/random/boot_id")
FOREIGN_BOOT_ID = "00000000-0000-0000-0000-000000000000"


def tearDownModule():
    discard_clone()


def role_env(role):
    return {"DF_VERIFY_ROLE": role}


def host_boot_id() -> str:
    return BOOT_ID.read_text(encoding="utf-8").strip()


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

    def check(self, label, env=()):
        scenarios = [
            Scenario(f"{label} ({action})", (action, "--scope", "all", "--print-plan"), dict(env))
            for action in PLAN_ACTIONS
        ]
        for scenario, pair in run_many(self.clone, scenarios):
            with self.subTest(scenario.label):
                self.assert_bash_plans(scenario, pair)
                self.assert_parity(scenario, pair, ENVIRONMENT_SECTIONS)

    def home(self, name, cargo_env=None) -> str:
        home = self.scratch / name
        home.mkdir()
        if cargo_env is not None:
            (home / ".cargo").mkdir()
            (home / ".cargo" / "env").write_text(cargo_env, encoding="utf-8")
        return str(home)

    def fifo(self, name, stamp=None) -> str:
        fifo = self.scratch / name
        os.mkfifo(fifo)
        if stamp is not None:
            Path(f"{fifo}.owner").write_text(stamp, encoding="utf-8")
        return str(fifo)

    def test_pinned_missing_fifo(self):
        self.check("FIFO knobs pinned to a missing path")

    def test_home_without_cargo_env(self):
        self.check("no ~/.cargo/env", {"HOME": self.home("bare")})

    def test_cargo_env_delta_reaches_the_plan(self):
        bin_dir = self.scratch / "cargo-env-bin"
        bin_dir.mkdir()
        cargo_env = (
            "export REIFY_PARITY_CARGO_ENV_PROBE=sourced\n"
            f"export PATH=\"{bin_dir}:$PATH\"\n"
            f"export LD_LIBRARY_PATH=\"{self.scratch}/from-cargo-env\"\n"
        )
        self.check("~/.cargo/env sourced", {"HOME": self.home("cargo", cargo_env)})

    def test_ambient_loader_path(self):
        self.check("ambient LD_LIBRARY_PATH", {"LD_LIBRARY_PATH": f"{self.scratch}/ambient"})

    def test_fifo_owner_stamps(self):
        boot = host_boot_id()
        live = os.getpid()
        cases = (
            ("no owner stamp", None, {}),
            ("live owner", f"{live} {boot}\n", {}),
            ("dead owner", f"{dead_pid()} {boot}\n", {}),
            ("live owner of another boot", f"{live} {FOREIGN_BOOT_ID}\n", {}),
            ("malformed pid", f"12x {boot}\n", {}),
            ("stamp without a newline", f"{dead_pid()} {boot}", {}),
            ("stale stamp, probe skipped", f"{dead_pid()} {boot}\n",
             {"REIFY_JOBSERVER_SKIP_LIVENESS_PROBE": "1"}),
        )
        for n, (label, stamp, extra) in enumerate(cases):
            fifo = self.fifo(f"fifo-{n}", stamp)
            self.check(f"task FIFO: {label}", {TASK_FIFO: fifo, **extra})

    def test_merge_role_live_fifo(self):
        fifo = self.fifo("merge-fifo", f"{os.getpid()} {host_boot_id()}\n")
        self.check("merge FIFO live", {"DF_VERIFY_ROLE": "merge", MERGE_FIFO: fifo})

    def test_offline_role(self):
        self.check("offline role", {"DF_VERIFY_ROLE": "offline"})

    def test_host_default_fifo(self):
        self.check("host default FIFO", {TASK_FIFO: None, MERGE_FIFO: None})


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
