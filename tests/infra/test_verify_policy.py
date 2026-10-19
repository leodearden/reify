#!/usr/bin/env python3
"""
test_verify_policy.py — the Python verify package's CLI against live bash
(S18 α, task 9095; PRD docs/prds/v0_6/verify-structured-plan-and-role-table.md
D3, D4, D8, D11, D14).

Every check drives a public seam as a subprocess: `bash scripts/verify.sh`,
`python3 -I -B scripts/verify.py` and, for registration,
`scripts/verify-pipeline-guard.sh`. This module never imports reify_verify;
its expectations are bash's observable behaviour or the PRD's tables.

Every child env is hermetic_env() with every REIFY_* and DF_* key also removed,
so a knob the gate's own verify_env sets cannot change a plan under test. Each
bash argv that names an action carries --print-plan ahead of the token that
provokes its error: a case that unexpectedly validates prints a plan rather
than running a real gate.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its
thin wrapper tests/infra/test_verify_policy.sh.
"""

import json
import os
import re
import subprocess
import sys
import tempfile
import unittest
from dataclasses import dataclass, field
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(INFRA_DIR))
from verify_ledger_env import hermetic_env  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[2]
VERIFY_PY = REPO_ROOT / "scripts" / "verify.py"
KNOBS_DOC = REPO_ROOT / "docs" / "notes" / "verify-pipeline-knobs.md"
HANG_GUARD_SECS = 300
ERROR_PREFIX = "verify.sh: ERROR — "
WALL_RE = re.compile(r"timeout --kill-after=60 (\S+) ")


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
    raw_stderr: bytes


def _run(cmd, env, cwd):
    proc = subprocess.run(
        cmd,
        cwd=cwd,
        env=env,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=HANG_GUARD_SECS,
    )
    return Outcome(
        proc.returncode,
        proc.stdout.decode("utf-8"),
        proc.stderr.decode("utf-8"),
        proc.stderr,
    )


def run_bash(argv, env, tree=REPO_ROOT):
    return _run(["bash", "scripts/verify.sh", *argv], env, tree)


def run_py(argv, env, cwd=REPO_ROOT):
    return _run([sys.executable, "-I", "-B", str(VERIFY_PY), *argv], env, cwd)


def python_help(env=None):
    outcome = run_py(["--help"], env if env is not None else base_env())
    assert outcome.returncode == 0, outcome.stderr
    return outcome.stdout


def json_lines(outcome):
    assert outcome.returncode == 0, outcome.stderr
    return [json.loads(line) for line in outcome.stdout.splitlines()]


def knob_report(env):
    """(records by name, stderr) of `verify.py --print-knobs` under `env`."""
    outcome = run_py(["--print-knobs"], env)
    return {record["name"]: record for record in json_lines(outcome)}, outcome.stderr


@dataclass(frozen=True)
class PlanScenario:
    argv: tuple
    env: tuple = ()


def bash_plan(scenario, env):
    outcome = run_bash(scenario.argv, {**env, **dict(scenario.env)})
    assert outcome.returncode == 0, outcome.stderr
    return outcome


MERGE_TEST = PlanScenario(
    ("test", "--profile", "both", "--print-plan"), (("DF_VERIFY_ROLE", "merge"),)
)
MERGE_ALL = PlanScenario(
    ("all", "--profile", "both", "--print-plan"), (("DF_VERIFY_ROLE", "merge"),)
)
TASK_TYPECHECK = PlanScenario(("typecheck", "--print-plan"))


@dataclass(frozen=True)
class WallSite:
    """The one plan line a duration knob's wall is rendered on."""

    knob: str
    scenario: PlanScenario
    includes: tuple
    excludes: tuple = ()

    def wall(self, plan_stdout):
        lines = [
            line
            for line in plan_stdout.splitlines()
            if all(s in line for s in self.includes)
            and not any(s in line for s in self.excludes)
        ]
        assert len(lines) == 1, f"{self.knob}: {len(lines)} lines match {self.includes}"
        match = WALL_RE.search(lines[0])
        assert match, f"{self.knob}: no timeout wall on {lines[0]!r}"
        return match.group(1)


DURATION_SITES = (
    WallSite(
        "REIFY_VERIFY_TEST_TIMEOUT",
        MERGE_TEST,
        ("cargo nextest run --workspace",),
        (" --release",),
    ),
    WallSite(
        "REIFY_VERIFY_TEST_TIMEOUT_RELEASE", MERGE_TEST, ("cargo nextest run ", " --release")
    ),
    WallSite(
        "REIFY_VERIFY_PREBUILD_TIMEOUT", MERGE_ALL, ("cargo build --release -p reify-audit",)
    ),
    WallSite("REIFY_VERIFY_CLIPPY_TIMEOUT", MERGE_ALL, ("cargo clippy",)),
    WallSite(
        "REIFY_VERIFY_GUI_FEATURE_TEST_TIMEOUT",
        MERGE_ALL,
        ("cargo nextest run -p reify-gui --features gui",),
    ),
    WallSite("REIFY_VERIFY_CHECK_TIMEOUT", TASK_TYPECHECK, ("cargo check --workspace --tests",)),
)
CHECK_SITE = DURATION_SITES[-1]


@dataclass(frozen=True)
class ArgvCase:
    argv: tuple
    env: dict = field(default_factory=dict)

    def label(self):
        prefix = " ".join(f"{k}={v}" for k, v in self.env.items())
        return f"{prefix} {list(self.argv)!r}".strip()


ARGV_ERROR_CORPUS = (
    ArgvCase(()),
    ArgvCase(("bogus",)),
    ArgvCase(("--bogus",)),
    ArgvCase(("test", "--print-plan", "lint")),
    ArgvCase(("--profile=x",)),
    ArgvCase(("--print-plan", "--scope=bad", "test")),
    ArgvCase(("test", "--print-plan", "--profile=weird")),
    ArgvCase(("test", "--print-plan", "--profile", "weird")),
    ArgvCase(("test", "--print-plan", "--profile=")),
    ArgvCase(("--print-plan", "--profile", "--scope", "all")),
    ArgvCase(("test", "--print-plan", "--scope=nope")),
    ArgvCase(("test", "--print-plan", "--scope", "nope")),
    ArgvCase(("test", "--print-plan", "--profile=x", "--scope=y")),
    *(
        ArgvCase(("test", "--print-plan", f"--test-threads={value}"))
        for value in ("0", "007", "-1", "1.5", "")
    ),
    ArgvCase(("test", "--print-plan", "--test-threads", "abc")),
    *(
        ArgvCase((action, "--print-plan", "--confirm-failed"))
        for action in ("lint", "typecheck", "psi-gate", "compile-gate")
    ),
    ArgvCase(
        ("test", "--print-plan", "--confirm-failed"),
        {"REIFY_VERIFY_RETRY_SCOPE": "failed_only"},
    ),
    ArgvCase(
        ("all", "--print-plan", "--confirm-failed"),
        {"REIFY_VERIFY_RETRY_SCOPE": "x"},
    ),
    ArgvCase(("--bogus", "--help")),
    ArgvCase(("test", "--print-plan"), {"DF_VERIFY_ROLE": "bogus"}),
    ArgvCase(("test", "--print-plan"), {"DF_VERIFY_ROLE": "Merge"}),
)


class TestEntryPoint(unittest.TestCase):
    """verify.py finds its package from its own directory, whatever the cwd."""

    def test_help_from_a_foreign_cwd_under_isolated_mode(self):
        with tempfile.TemporaryDirectory() as cwd:
            outcome = run_py(["--help"], base_env(), cwd=cwd)
        self.assertEqual(outcome.returncode, 0, outcome.stderr)


class TestArgvErrorParity(unittest.TestCase):
    """Every argv error bash reports, python reports identically."""

    @classmethod
    def setUpClass(cls):
        cls.help_text = python_help()

    def assert_parity(self, case):
        env = base_env(**case.env)
        bash = run_bash(case.argv, env)
        py = run_py(case.argv, env)
        bash_lines = bash.stderr.splitlines()
        self.assertEqual(bash.returncode, 64, f"premise: bash rejects {case.label()}")
        self.assertEqual(py.returncode, bash.returncode, case.label())
        self.assertEqual(py.stdout, "", case.label())
        first = py.stderr.split("\n", 1)[0]
        self.assertEqual(first, bash_lines[0], case.label())
        if len(bash_lines) > 1:
            self.assertEqual(py.stderr, first + "\n" + self.help_text, case.label())
        else:
            self.assertEqual(py.stderr, first + "\n", case.label())

    def test_corpus(self):
        for case in ARGV_ERROR_CORPUS:
            with self.subTest(case=case.label()):
                self.assert_parity(case)

    def test_error_line_bytes_under_the_c_locale(self):
        env = base_env(LC_ALL="C")
        bash = run_bash(["--bogus"], env)
        py = run_py(["--bogus"], env)
        self.assertEqual(py.returncode, bash.returncode)
        self.assertEqual(
            py.raw_stderr.split(b"\n", 1)[0], bash.raw_stderr.split(b"\n", 1)[0]
        )
        self.assertIn("—".encode("utf-8"), py.raw_stderr.split(b"\n", 1)[0])


class TestDefect5Divergence(unittest.TestCase):
    """A missing option value: bash dies in ${2:?} with exit 1; python exits 64."""

    CASES = (
        (("test", "--print-plan", "--profile"), "--profile"),
        (("test", "--print-plan", "--scope"), "--scope"),
        (("test", "--print-plan", "--test-threads"), "--test-threads"),
        (("test", "--print-plan", "--profile", ""), "--profile"),
    )

    def test_missing_value(self):
        env = base_env()
        for argv, flag in self.CASES:
            with self.subTest(argv=argv):
                bash = run_bash(argv, env)
                self.assertEqual(bash.returncode, 1)
                self.assertIn("requires an argument", bash.stderr)
                py = run_py(argv, env)
                self.assertEqual(py.returncode, 64)
                self.assertEqual(py.stdout, "")
                self.assertEqual(py.stderr, f"{ERROR_PREFIX}{flag} requires a value\n")


class TestUsage(unittest.TestCase):
    """The usage text is complete, keeps its headers and ends at the last option."""

    FLAGS = (
        "--profile",
        "--scope",
        "--narrow",
        "--include-infra",
        "--print-plan",
        "--test-threads",
        "--confirm-failed",
        "-h|--help",
    )

    @classmethod
    def setUpClass(cls):
        cls.help_text = python_help()

    def test_help_forms_exit_zero_on_both_sides(self):
        env = base_env()
        for argv in (["--help"], ["-h"], ["--help", "--bogus"], ["test", "--help"]):
            with self.subTest(argv=argv):
                self.assertEqual(run_bash(argv, env).returncode, 0)
                py = run_py(argv, env)
                self.assertEqual(py.returncode, 0, py.stderr)
                self.assertEqual(py.stdout, self.help_text)

    def test_header_lines_dark_factory_recognises(self):
        lines = self.help_text.splitlines()
        self.assertTrue(any(re.fullmatch(r"\s*Usage:\s*", line) for line in lines))
        self.assertTrue(any(re.fullmatch(r"\s*Options:\s*", line) for line in lines))

    def test_every_option_is_listed(self):
        for flag in self.FLAGS:
            with self.subTest(flag=flag):
                self.assertIn(flag, self.help_text)

    def test_ends_at_the_last_option(self):
        last = [line for line in self.help_text.splitlines() if line.strip()][-1]
        self.assertTrue(last.lstrip().startswith("-h|--help"), last)

    def test_knob_warnings_do_not_precede_help(self):
        env = base_env(REIFY_VERIFY_CHECK_TIMEOUT="bogus")
        py = run_py(["--help"], env)
        self.assertEqual(py.returncode, 0)
        self.assertEqual(py.stderr, "")
        bash = run_bash(["--help"], env)
        self.assertIn(
            "verify.sh: WARNING: invalid REIFY_VERIFY_CHECK_TIMEOUT=bogus; "
            "using default 30m",
            bash.stderr.splitlines(),
        )


class TestNotPortedActions(unittest.TestCase):
    """α never reports success for an action it does not run."""

    def test_actions_refuse(self):
        env = base_env()
        for argv in (["lint"], ["test", "--print-plan"]):
            with self.subTest(argv=argv):
                py = run_py(argv, env)
                self.assertNotEqual(py.returncode, 0)
                self.assertEqual(py.stdout, "")
                lines = py.stderr.splitlines()
                self.assertEqual(len(lines), 1, py.stderr)
                self.assertTrue(lines[0].startswith(ERROR_PREFIX), lines[0])


class TestKnobRegistry(unittest.TestCase):
    """`--print-knobs` lists every knob verify.sh reads, resolved as bash does."""

    RECORD_KEYS = {"v", "name", "parser", "default", "meaning", "value"}
    PARSERS = {"duration", "text", "text-unset-only", "flag-one", "flag-set"}
    VALID_DURATIONS = ("7", "7s", "7m", "7h", "7d")
    INVALID_DURATIONS = ("7x", "7mm", "m", "-1", "1.5", " 7")
    OVERRIDES = dict(
        zip((site.knob for site in DURATION_SITES), ("11m", "12m", "13m", "14m", "15m", "16m"))
    )
    PARSER_CASES = (
        ("REIFY_GATE_EXCLUDE_HEAVY", "flag-one", (("1", True), ("0", False), ("true", False), ("", False))),
        ("REIFY_PSI_GATE_THRESHOLD", "text", ((None, "50"), ("", "50"), ("70", "70"))),
        ("REIFY_PSI_GATE_MEM_FULL_THRESHOLD", "text-unset-only", ((None, "10"), ("", ""))),
        ("REIFY_INFRA_SUITE_ACTIVE", "flag-set", (("x", True), ("", False))),
        ("REIFY_VERIFY_LEDGER_DIR", "text", ((None, None), ("/tmp/x", "/tmp/x"))),
    )

    @classmethod
    def setUpClass(cls):
        cls.records, cls.stderr = knob_report(base_env())

    def check_timeout(self, raw):
        env = base_env(REIFY_VERIFY_CHECK_TIMEOUT=raw)
        bash = bash_plan(CHECK_SITE.scenario, env)
        records, py_stderr = knob_report(env)
        return (
            CHECK_SITE.wall(bash.stdout),
            bash.stderr.splitlines(),
            records[CHECK_SITE.knob]["value"],
            py_stderr,
        )

    def test_record_shape(self):
        outcome = run_py(["--print-knobs"], base_env())
        records = json_lines(outcome)
        self.assertEqual(self.stderr, "")
        self.assertTrue(records)
        names = [record["name"] for record in records]
        self.assertEqual(len(names), len(set(names)))
        for record in records:
            with self.subTest(knob=record.get("name")):
                self.assertEqual(set(record), self.RECORD_KEYS)
                self.assertEqual(record["v"], 1)
                self.assertTrue(record["name"].startswith("REIFY_"))
                self.assertIn(record["parser"], self.PARSERS)
                self.assertTrue(record["meaning"].strip())
                self.assertNotIn("\n", record["meaning"])
                default = record["default"]
                if isinstance(default, dict):
                    self.assertEqual(set(default), {"derived"})
                    self.assertTrue(default["derived"].strip())
                else:
                    self.assertIsInstance(default, str)

    def test_duration_defaults_equal_bash_walls(self):
        plans = {}
        for site in DURATION_SITES:
            with self.subTest(knob=site.knob):
                if site.scenario not in plans:
                    plans[site.scenario] = bash_plan(site.scenario, base_env()).stdout
                record = self.records[site.knob]
                self.assertEqual(record["parser"], "duration")
                self.assertEqual(record["default"], site.wall(plans[site.scenario]))

    def test_duration_overrides_render_on_bash_lines_and_as_values(self):
        env = base_env(**self.OVERRIDES)
        records, py_stderr = knob_report(env)
        self.assertEqual(py_stderr, "")
        plans = {}
        for site in DURATION_SITES:
            with self.subTest(knob=site.knob):
                if site.scenario not in plans:
                    plans[site.scenario] = bash_plan(site.scenario, env).stdout
                self.assertEqual(site.wall(plans[site.scenario]), self.OVERRIDES[site.knob])
                self.assertEqual(records[site.knob]["value"], self.OVERRIDES[site.knob])

    def test_valid_durations_are_kept_verbatim(self):
        for raw in self.VALID_DURATIONS:
            with self.subTest(raw=raw):
                bash_wall, _, py_value, py_stderr = self.check_timeout(raw)
                self.assertEqual(bash_wall, raw)
                self.assertEqual(py_value, raw)
                self.assertEqual(py_stderr, "")

    def test_invalid_durations_warn_once_and_use_the_default(self):
        default = self.records[CHECK_SITE.knob]["default"]
        for raw in self.INVALID_DURATIONS:
            with self.subTest(raw=raw):
                warning = (
                    f"verify.sh: WARNING: invalid {CHECK_SITE.knob}={raw}; "
                    f"using default {default}"
                )
                bash_wall, bash_lines, py_value, py_stderr = self.check_timeout(raw)
                self.assertEqual(bash_wall, default)
                self.assertEqual(py_value, default)
                self.assertIn(warning, bash_lines)
                self.assertEqual(py_stderr.splitlines().count(warning), 1, py_stderr)

    def test_empty_duration_is_the_default_without_a_warning(self):
        default = self.records[CHECK_SITE.knob]["default"]
        bash_wall, bash_lines, py_value, py_stderr = self.check_timeout("")
        self.assertEqual(bash_wall, default)
        self.assertEqual(py_value, default)
        self.assertFalse([line for line in bash_lines if "WARNING: invalid" in line])
        self.assertEqual(py_stderr, "")

    def test_every_other_parser_kind(self):
        for name, parser, cases in self.PARSER_CASES:
            self.assertEqual(self.records[name]["parser"], parser, name)
            for raw, expected in cases:
                with self.subTest(knob=name, raw=raw):
                    env = base_env() if raw is None else base_env(**{name: raw})
                    records, _ = knob_report(env)
                    self.assertEqual(records[name]["value"], expected)

    def test_a_derived_default_is_not_a_value(self):
        record = self.records["REIFY_VERIFY_LEDGER_DIR"]
        self.assertIsNone(record["value"])
        self.assertIsInstance(record["default"], dict)

    def test_every_knob_is_documented(self):
        doc = KNOBS_DOC.read_text(encoding="utf-8")
        for name in self.records:
            with self.subTest(knob=name):
                self.assertIn(f"`{name}`", doc)

    def test_knob_warnings_fire_after_argv(self):
        env = base_env(REIFY_VERIFY_CHECK_TIMEOUT="bogus")
        py = run_py(["--print-knobs", "--bogus"], env)
        self.assertEqual(py.returncode, 64)
        self.assertEqual(py.stdout, "")
        self.assertEqual(
            py.stderr.splitlines()[0], f"{ERROR_PREFIX}unknown argument '--bogus'"
        )
        self.assertNotIn("WARNING", py.stderr)

    def test_bash_rejects_the_query_flag(self):
        bash = run_bash(["--print-plan", "--print-knobs"], base_env())
        self.assertEqual(bash.returncode, 64)
        self.assertEqual(
            bash.stderr.splitlines()[0], f"{ERROR_PREFIX}unknown argument '--print-knobs'"
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
