#!/usr/bin/env python3
"""
test_verify_scope_record.py — the Python scope decision, narrowing and infra
selection against live bash (S18 β, task 9097; PRD
docs/prds/v0_6/verify-structured-plan-and-role-table.md D5, D6, D11, B7).

Every check drives a public seam as a subprocess: `python3 -I -B
scripts/verify.py ... --print-scope` and `bash scripts/verify.sh ...
--print-plan`. This module never imports reify_verify. Bash is the oracle: a
port's spec is today's behaviour, so most expectations are what bash prints in
the same tree. Where β deliberately differs from bash, the check asserts
python alone and its message names the reason.

Every child env is hermetic_env() with every REIFY_* and DF_* key also removed,
so a knob the gate's own verify_env sets cannot change a decision under test.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its
thin wrapper tests/infra/test_verify_scope_record.sh.
"""

import json
import subprocess
import sys
import unittest
from dataclasses import dataclass
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(INFRA_DIR))
from verify_ledger_env import hermetic_env  # noqa: E402

REPO_ROOT = INFRA_DIR.parents[1]
HANG_GUARD_SECS = 600
ERROR_PREFIX = "verify.sh: ERROR — "
RECORD_KEYS = {"v", "scope", "narrowing", "infra_selection"}
SCOPE_FLAGS = ("run_rust", "run_gui", "run_occt_gate", "gui_path_signal")


def base_env(**overrides):
    """hermetic_env() without any REIFY_* or DF_* key, then `overrides`."""
    env = {
        k: v
        for k, v in hermetic_env().items()
        if not k.startswith(("REIFY_", "DF_"))
    }
    env.update(overrides)
    return env


@dataclass(frozen=True)
class Outcome:
    returncode: int
    stdout: str
    stderr: str


def _run(cmd, env, cwd):
    proc = subprocess.run(
        cmd,
        cwd=cwd,
        env=env,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=HANG_GUARD_SECS,
    )
    return Outcome(proc.returncode, proc.stdout.decode("utf-8"), proc.stderr.decode("utf-8"))


def run_py(argv, env, tree=REPO_ROOT, cwd=None):
    """`verify.py` of `tree`, run from `cwd` (default: the tree's root)."""
    verify_py = Path(tree) / "scripts" / "verify.py"
    return _run(
        [sys.executable, "-I", "-B", str(verify_py), *argv],
        env,
        tree if cwd is None else cwd,
    )


def run_bash(argv, env, tree=REPO_ROOT):
    return _run(
        ["bash", "scripts/verify.sh", *argv],
        {**env, "REIFY_NEXTEST_PROBE_RETRY_SLEEP": "0"},
        tree,
    )


def scope_record(outcome):
    """The one JSON record a successful --print-scope prints."""
    assert outcome.returncode == 0, outcome.stderr
    lines = outcome.stdout.splitlines()
    assert len(lines) == 1, outcome.stdout
    record = json.loads(lines[0])
    assert record["v"] == 1, record
    return record


def python_help():
    outcome = run_py(["--help"], base_env())
    assert outcome.returncode == 0, outcome.stderr
    return outcome.stdout


class TestPrintScopeCli(unittest.TestCase):
    """--print-scope is an action query on the same validation path (D11)."""

    @classmethod
    def setUpClass(cls):
        cls.help_text = python_help()

    def assert_int_flag(self, record, key, value):
        self.assertIs(type(record[key]), int, f"{key} is a JSON int: {record}")
        self.assertEqual(record[key], value, key)

    def test_scope_all_record(self):
        for argv in (["all", "--print-scope"], ["test", "--scope", "all", "--print-scope"]):
            with self.subTest(argv=argv):
                outcome = run_py(argv, base_env())
                record = scope_record(outcome)
                self.assertEqual(outcome.stderr, "")
                self.assertEqual(set(record), RECORD_KEYS)
                scope = record["scope"]
                self.assertEqual(scope["mode"], "all")
                for flag in SCOPE_FLAGS:
                    self.assert_int_flag(scope, flag, 1)
                self.assertEqual(scope["changed_paths"], [])
                self.assertEqual(scope["rename_sources"], [])
                narrowing = record["narrowing"]
                self.assert_int_flag(narrowing, "narrow_active", 0)
                self.assert_int_flag(narrowing, "from_diff", 0)
                self.assert_int_flag(narrowing, "run_gui_vitest", 1)
                self.assertEqual(record["infra_selection"], [])

    def assert_usage_error(self, argv, message, with_help=False):
        outcome = run_py(argv, base_env())
        self.assertEqual(outcome.returncode, 64, outcome.stderr)
        self.assertEqual(outcome.stdout, "")
        expected = f"{ERROR_PREFIX}{message}\n" + (self.help_text if with_help else "")
        self.assertEqual(outcome.stderr, expected)

    def test_missing_action(self):
        self.assert_usage_error(
            ["--print-scope"], "missing action (test|lint|typecheck|all)", with_help=True
        )

    def test_a_non_gate_action_is_refused(self):
        for action in ("psi-gate", "compile-gate"):
            with self.subTest(action=action):
                self.assert_usage_error(
                    [action, "--print-scope"],
                    f"query '--print-scope' takes action test|lint|typecheck|all "
                    f"(got '{action}')",
                )

    def test_a_conflicting_flag_is_refused(self):
        cases = (
            (["all", "--print-scope", "--print-plan"], "--print-plan"),
            (["test", "--print-scope", "--confirm-failed"], "--confirm-failed"),
        )
        for argv, flag in cases:
            with self.subTest(argv=argv):
                self.assert_usage_error(
                    argv, f"query '--print-scope' does not combine with {flag}"
                )

    def test_queries_are_exclusive(self):
        self.assert_usage_error(
            ["--print-knobs", "--print-scope"],
            "query already set to '--print-knobs', got '--print-scope'",
        )

    def test_the_same_validation_path(self):
        self.assert_usage_error(
            ["all", "--print-scope", "--profile=weird"],
            "invalid --profile 'weird' (want debug|release|both)",
        )

    def test_bash_rejects_the_query_flag(self):
        bash = run_bash(["--print-plan", "--print-scope"], base_env())
        self.assertEqual(bash.returncode, 64)
        self.assertEqual(
            bash.stderr.splitlines()[0],
            f"{ERROR_PREFIX}unknown argument '--print-scope'",
            "D8 listed divergence (fold-verifier V12): bash has no --print-scope",
        )

    def test_help_lists_the_query(self):
        self.assertIn("--print-scope", self.help_text)
        last = [line for line in self.help_text.splitlines() if line.strip()][-1]
        self.assertTrue(last.lstrip().startswith("-h|--help"), last)


if __name__ == "__main__":
    unittest.main(verbosity=2)
