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
import tempfile
import unittest
from dataclasses import dataclass
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(INFRA_DIR))
from tree_under_test import scratch_git, shared_clone  # noqa: E402
from verify_ledger_env import hermetic_env  # noqa: E402

REPO_ROOT = INFRA_DIR.parents[1]
HANG_GUARD_SECS = 600
ERROR_PREFIX = "verify.sh: ERROR — "
RECORD_KEYS = {"v", "scope", "narrowing", "infra_selection"}
SCOPE_FLAGS = ("run_rust", "run_gui", "run_occt_gate", "gui_path_signal")
DOCS_PATH = "docs/getting-started.md"
MERGE_HEAD_NOTICE = "verify.sh: MERGE_HEAD present — forcing --scope all (merge in progress)"
MERGE_BASE_WARNING = (
    "verify.sh: WARNING — --scope branch could not resolve 'git merge-base main HEAD' "
    "(detached HEAD / missing local main ref / merge-base failure) — failing WIDE to "
    "--scope all (contract C5)"
)
FORCED_SCOPE_MARK = "forcing --scope all"


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


class Clone:
    """B1's shared clone of the tree under test (PRD D13), reset between tests."""

    def __init__(self, root: Path):
        self.root = root
        self.base = self.git("rev-parse", "HEAD")
        self.main = self.git("rev-parse", "--verify", "refs/heads/main")

    def git(self, *args, stdin=None) -> str:
        return scratch_git(self.root, *args, stdin=stdin).decode("utf-8").strip()

    def reset(self):
        self.git("reset", "-q", "--hard", self.base)
        self.git("clean", "-fdq")
        self.git("branch", "-f", "main", self.main)
        self.merge_head().unlink(missing_ok=True)

    def merge_head(self) -> Path:
        return self.root / self.git("rev-parse", "--git-path", "MERGE_HEAD")

    def append(self, path, text="\n"):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        with open(target, "a", encoding="utf-8") as handle:
            handle.write(text)

    def stage(self, path, text="\n"):
        """Append `text` to `path` (creating it) and stage it."""
        self.append(path, text)
        self.git("add", "--", path)

    def commit(self, message):
        self.git("commit", "-q", "--allow-empty", "-m", message)

    def py(self, argv, env, cwd=None):
        return run_py(argv, env, tree=self.root, cwd=cwd)

    def bash(self, argv, env):
        return run_bash(argv, env, tree=self.root)


_SCRATCH = None
_CLONE = None


def the_clone() -> Clone:
    """The module's one clone, built on first use (each `git clone` costs a checkout)."""
    global _SCRATCH, _CLONE
    if _CLONE is None:
        _SCRATCH = tempfile.TemporaryDirectory()
        _CLONE = Clone(shared_clone(Path(_SCRATCH.name) / "tree"))
    return _CLONE


def tearDownModule():
    if _SCRATCH is not None:
        _SCRATCH.cleanup()


class InClone(unittest.TestCase):
    """A test case that runs in the shared clone, reset to its base before each test."""

    @classmethod
    def setUpClass(cls):
        cls.clone = the_clone()

    def setUp(self):
        self.clone.reset()


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


class TestEffectiveScope(InClone):
    """MERGE_HEAD, force_scope_all and the merge base settle the scope before any diff."""

    STAGED = ["test", "--scope", "staged", "--print-scope"]

    def setUp(self):
        super().setUp()
        self.clone.stage(DOCS_PATH)

    def test_merge_head_is_found_from_the_repo_root(self):
        self.clone.merge_head().write_text(self.clone.base + "\n", encoding="utf-8")
        with tempfile.TemporaryDirectory() as foreign:
            py = self.clone.py(self.STAGED, base_env(), cwd=foreign)
        self.assertEqual(scope_record(py)["scope"]["mode"], "all")
        self.assertIn(MERGE_HEAD_NOTICE, py.stderr.splitlines())
        bash = self.clone.bash(["test", "--scope", "staged", "--print-plan"], base_env())
        self.assertEqual(bash.returncode, 0, bash.stderr)
        self.assertIn(
            MERGE_HEAD_NOTICE,
            bash.stderr.splitlines(),
            "premise: bash, run from the clone root, sees the same MERGE_HEAD",
        )

    def test_force_scope_all_roles(self):
        for role, forced in (("merge", True), ("background", True), ("task", False), ("offline", False)):
            with self.subTest(role=role):
                env = base_env(DF_VERIFY_ROLE=role)
                py = self.clone.py(self.STAGED, env)
                self.assertEqual(scope_record(py)["scope"]["mode"], "all" if forced else "staged")
                bash = self.clone.bash(["test", "--scope", "staged", "--print-plan"], env)
                self.assertEqual(bash.returncode, 0, bash.stderr)
                bash_notices = [l for l in bash.stderr.splitlines() if FORCED_SCOPE_MARK in l]
                py_notices = [l for l in py.stderr.splitlines() if FORCED_SCOPE_MARK in l]
                self.assertEqual(len(bash_notices), int(forced), bash.stderr)
                self.assertEqual(py_notices, bash_notices)

    def test_branch_without_main_fails_wide(self):
        self.clone.git("branch", "-D", "main")
        argv = ["test", "--scope", "branch", "--print-scope"]
        py = self.clone.py(argv, base_env())
        self.assertEqual(scope_record(py)["scope"]["mode"], "all")
        self.assertIn(MERGE_BASE_WARNING, py.stderr.splitlines())
        bash = self.clone.bash(["test", "--scope", "branch", "--print-plan"], base_env())
        self.assertEqual(bash.returncode, 0, bash.stderr)
        self.assertIn(MERGE_BASE_WARNING, bash.stderr.splitlines(), "premise: bash's exact C5 line")

    def test_branch_records_the_merge_base(self):
        self.clone.git("branch", "-f", "main", "HEAD")
        py = self.clone.py(["test", "--scope", "branch", "--print-scope"], base_env())
        scope = scope_record(py)["scope"]
        self.assertEqual(scope["mode"], "branch")
        self.assertEqual(scope["merge_base"], self.clone.git("merge-base", "main", "HEAD"))


if __name__ == "__main__":
    unittest.main(verbosity=2)
