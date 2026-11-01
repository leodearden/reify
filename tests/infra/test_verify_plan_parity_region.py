#!/usr/bin/env python3
"""
test_verify_plan_parity_region.py — B1's test-region member: bash
`scripts/verify.sh` and Python `scripts/verify.py` print the same test and all
plans under the knobs, narrowings and toolchains that shape the test region
(S18 γ2, task 9099; PRD docs/prds/v0_6/verify-structured-plan-and-role-table.md
D8, D13, §12).

Each case compares exit code, stdout and stderr byte for byte, after a
premise on bash's side: the case reaches the region shape it is named for (the
heavy partition, a narrowed selector, the cargo-test fallback, the confirm
paths, a config override, ...), so a bash change that stops reaching it reds
here rather than passing vacuously. The grids are the test and all shards; the
failed_only retry is test_verify_plan_parity_retry.py.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its
thin wrapper tests/infra/test_verify_plan_parity_region.sh.
"""

import itertools
import sys
import tempfile
import unittest
from dataclasses import dataclass
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(INFRA_DIR))
from plan_parity_harness import (  # noqa: E402
    CHANGED_PATH_FIXTURES,
    InParityClone,
    Scenario,
    discard_clone,
    nextest_env,
    path_farm,
    run_both,
    run_many,
    stage_written,
)

TEST_ACTIONS = ("test", "all")
ROLES = (None, "merge", "offline", "background")
RUST_CHANGE = "crates/reify-doc/src/lib.rs"
GUI_FEATURE_PASS = "cargo nextest run -p reify-gui --features gui"
PLACEHOLDER_NAME = "reify-nextest-occt.<print-plan-placeholder>"
HEAVY_HEADER = "# heavy partition — HEAVY="
# heavy_partition's answer under REIFY_GATE_EXCLUDE_HEAVY=1, per role.
HEAVY_EXCLUDED_BY_ROLE = {None: "excluded", "merge": "excluded", "offline": "only", "background": "included"}
NEXTEST_ABSENT = " nextest=0 "
ABSENT_PROBE = "    echo 'error: no such command: `nextest`' >&2\n    exit 101\n"
FAILING_PROBE = "    echo 'error: the B1 stub nextest probe failed' >&2\n    exit 101\n"
CONFIRM_OVERRIDES = {
    "REIFY_VERIFY_CONFIRM_MANIFEST_DEBUG": "target/b1/manifest-debug.txt",
    "REIFY_VERIFY_CONFIRM_MANIFEST_RELEASE": "target/b1/manifest-release.txt",
    "REIFY_VERIFY_CONFIRM_SIDECAR_DEBUG": "target/b1/sidecar-debug.json",
    "REIFY_VERIFY_CONFIRM_SIDECAR_RELEASE": "target/b1/sidecar-release.json",
    "REIFY_VERIFY_CONFIRM_JUNIT": "target/b1/confirm.xml",
}
CALLER_CONFIG = "/nonexistent/b1-region/caller-nextest.toml"
REGION_TIMEOUT_KNOBS = (
    "REIFY_VERIFY_TEST_TIMEOUT",
    "REIFY_VERIFY_TEST_TIMEOUT_RELEASE",
    "REIFY_VERIFY_GUI_FEATURE_TEST_TIMEOUT",
)
RUN_ALL = "tests/infra/run_all.sh"


def tearDownModule():
    discard_clone()


@dataclass(frozen=True)
class Case:
    """A scenario and its bash-side premise: exit code, texts bash's stdout holds and lacks, texts its stderr holds."""

    scenario: Scenario
    stdout: tuple[str, ...] = ()
    absent: tuple[str, ...] = ()
    stderr: tuple[str, ...] = ()
    exit_code: int = 0


def plan(label, *argv, env=None) -> Scenario:
    return Scenario(label, (*argv, "--print-plan"), dict(env or {}))


def role_label(role) -> str:
    return role or "task"


class RegionParity(InParityClone):
    def setUp(self):
        super().setUp()
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.scratch = Path(scratch.name)

    def check(self, *cases: Case):
        """Every case in the clone's current state, concurrently."""
        pairs = run_many(self.clone, (case.scenario for case in cases))
        for case, (scenario, pair) in zip(cases, pairs):
            self.check_pair(case, pair)

    def check_pair(self, case: Case, pair):
        scenario = case.scenario
        with self.subTest(scenario.label):
            self.assertEqual(
                pair.bash.returncode, case.exit_code,
                f"premise: bash's exit code [{scenario.label}]\n{pair.bash.stderr}",
            )
            for text in case.stdout:
                self.assertIn(text, pair.bash.stdout, f"premise [{scenario.label}]")
            for text in case.absent:
                self.assertNotIn(text, pair.bash.stdout, f"premise [{scenario.label}]")
            for text in case.stderr:
                self.assertIn(text, pair.bash.stderr, f"premise [{scenario.label}]")
            self.assert_parity(scenario, pair)


class TestRegionKnobs(RegionParity):
    """The knobs the region reads, each reaching the plan it changes."""

    def test_heavy_partition_under_each_role(self):
        self.check(*(
            Case(
                plan(f"{action} heavy excluded role={role_label(role)}", action, "--scope", "all",
                     env={"DF_VERIFY_ROLE": role, "REIFY_GATE_EXCLUDE_HEAVY": "1"}),
                stdout=(f"{HEAVY_HEADER}{HEAVY_EXCLUDED_BY_ROLE[role]}",),
            )
            for action, role in itertools.product(TEST_ACTIONS, ROLES)
        ))

    def test_the_heavy_flag_is_the_exact_string_one(self):
        self.check(*(
            Case(
                plan(f"REIFY_GATE_EXCLUDE_HEAVY={value!r}", "test", "--scope", "all",
                     env={"REIFY_GATE_EXCLUDE_HEAVY": value}),
                stdout=(f"{HEAVY_HEADER}included",),
            )
            for value in ("0", "true", " 1")
        ))

    def test_cli_test_threads_both_spellings(self):
        self.check(*(
            Case(plan(f"{action} {' '.join(flag)}", action, "--scope", "all", *flag), stdout=(" --test-threads=3",))
            for action, flag in itertools.product(TEST_ACTIONS, (("--test-threads=3",), ("--test-threads", "3")))
        ))

    def test_offline_confirm_paths(self):
        self.check(Case(
            plan("offline --profile both, every confirm path overridden", "test", "--scope", "all",
                 "--profile", "both", env={"DF_VERIFY_ROLE": "offline", **CONFIRM_OVERRIDES}),
            stdout=tuple(f'"{path}"' for path in CONFIRM_OVERRIDES.values()),
        ))

    def test_tmpdir_names_the_placeholder_directory(self):
        self.check(
            Case(plan("TMPDIR scratch", "test", "--scope", "all", env={"TMPDIR": str(self.scratch)}),
                 stdout=(f"--config-file {self.scratch}/{PLACEHOLDER_NAME}",)),
            Case(plan("TMPDIR empty", "test", "--scope", "all", env={"TMPDIR": ""}),
                 stdout=(f"--config-file /tmp/{PLACEHOLDER_NAME}",)),
        )

    def test_offline_nextest_config_override(self):
        self.check(Case(
            plan("offline, caller-owned nextest config", "test", "--scope", "all",
                 env={"DF_VERIFY_ROLE": "offline", "REIFY_VERIFY_NEXTEST_CONFIG": CALLER_CONFIG}),
            stdout=(f"--config-file {CALLER_CONFIG}",), absent=(PLACEHOLDER_NAME,),
        ))

    def test_merge_attempt_sidecar_override(self):
        self.check(Case(
            plan("merge, attempt sidecar overridden", "test", "--scope", "all",
                 env={"DF_VERIFY_ROLE": "merge", "REIFY_VERIFY_ATTEMPT_SIDECAR": "target/b1/attempt.json"}),
            stdout=('> "target/b1/attempt.json"',),
        ))

    def test_merge_active_infra_suite_drops_the_pool(self):
        self.check(*(
            Case(
                plan(f"{action} merge, infra suite active", action, "--scope", "all", "--include-infra",
                     env={"DF_VERIFY_ROLE": "merge", "REIFY_INFRA_SUITE_ACTIVE": "1"}),
                absent=(RUN_ALL,),
            )
            for action in TEST_ACTIONS
        ))

    def test_bogus_region_timeouts(self):
        self.check(*(
            Case(
                plan(f"{knob}=bogus role={role}", "test", "--scope", "all",
                     env={"DF_VERIFY_ROLE": role, knob: "bogus"}),
                stderr=(f"WARNING: invalid {knob}=bogus; using default ",),
            )
            for role, knob in itertools.product(("task", "merge"), REGION_TIMEOUT_KNOBS)
        ))

    def test_task_priority_without_ionice(self):
        self.check(Case(
            plan("no ionice, task", "test", "--scope", "all", env={"PATH": path_farm(self.scratch, "ionice")}),
            stderr=("WARNING — ionice not found; task role using nice only",),
        ))


class TestRegionNarrowing(RegionParity):
    """The debug pass's selector and the gui-feature pass's gate follow the narrowing."""

    def test_narrowed_staged_and_branch_scopes(self):
        self.clone.start_branch()
        self.clone.stage(RUST_CHANGE)
        self.check(*(
            case
            for action in TEST_ACTIONS
            for case in (
                Case(plan(f"{action} staged --narrow", action, "--scope", "staged", "--narrow"),
                     stdout=("NARROW_ACTIVE=1", " -p reify-doc")),
                Case(plan(f"{action} branch", action, "--scope", "branch"), stdout=("NARROW_ACTIVE=1",)),
            )
        ))

    def test_affected_crates_overrides(self):
        self.clone.stage(RUST_CHANGE)
        reaches_gui = {"reify-gui reify-eval": True, "reify-doc": False}
        self.check(*(
            Case(
                plan(f"REIFY_AFFECTED_CRATES_OVERRIDE={value!r}", "test", "--scope", "staged", "--narrow",
                     env={"REIFY_AFFECTED_CRATES_OVERRIDE": value}),
                stdout=(f"closure={value} ", *((GUI_FEATURE_PASS,) if reaches_gui.get(value) else ())),
                absent=(GUI_FEATURE_PASS,) if reaches_gui.get(value) is False else (),
            )
            for value in ("reify-gui reify-eval", "reify-doc", "ALL", "bad*name", "  ")
        ))

    def test_task_override_without_a_gui_feature_pass(self):
        self.clone.stage(RUST_CHANGE)
        self.check(Case(
            plan("task staged reify-doc, caller-owned nextest config", "test", "--scope", "staged", "--narrow",
                 env={"REIFY_VERIFY_NEXTEST_CONFIG": CALLER_CONFIG}),
            stdout=(f"--config-file {CALLER_CONFIG}",), absent=(GUI_FEATURE_PASS, PLACEHOLDER_NAME),
        ))

    def test_changed_path_fixtures(self):
        for path in CHANGED_PATH_FIXTURES:
            self.clone.reset()
            stage_written(self.clone, path)
            scenario = plan(f"test staged {path}", "test", "--scope", "staged", "--include-infra")
            self.check_pair(Case(scenario), run_both(self.clone, scenario))


class TestRegionToolchain(RegionParity):
    """Without nextest the passes fall back to cargo test; a failing probe refuses the plan."""

    def test_nextest_absent_under_each_role(self):
        env = nextest_env(self.scratch, "absent", ABSENT_PROBE)
        cases = [
            Case(
                plan(f"{action} nextest absent role={role_label(role)}", action, "--scope", "all",
                     env={**env, "DF_VERIFY_ROLE": role}),
                stdout=(NEXTEST_ABSENT, "cargo test ", f"{HEAVY_HEADER}included"),
            )
            for action, role in itertools.product(TEST_ACTIONS, ROLES)
        ]
        cases.append(Case(
            plan("test nextest absent --test-threads=2", "test", "--scope", "all", "--test-threads=2", env=env),
            stdout=(NEXTEST_ABSENT, "-- --test-threads=2"),
        ))
        self.check(*cases)

    def test_nextest_present_but_failing(self):
        env = nextest_env(self.scratch, "failing", FAILING_PROBE, cargo_nextest=True)
        self.check(Case(plan("test nextest failing", "test", "--scope", "all", env=env), exit_code=1))


if __name__ == "__main__":
    unittest.main(verbosity=2)
