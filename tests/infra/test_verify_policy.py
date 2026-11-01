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
import shutil
import subprocess
import sys
import tempfile
import unittest
from dataclasses import dataclass, field
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(INFRA_DIR))
from tree_under_test import shared_clone  # noqa: E402
from verify_ledger_env import hermetic_env  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[2]
VERIFY_PY = REPO_ROOT / "scripts" / "verify.py"
KNOBS_DOC = REPO_ROOT / "docs" / "notes" / "verify-pipeline-knobs.md"
HANG_GUARD_SECS = 300
ERROR_PREFIX = "verify.sh: ERROR — "
WALL_RE = re.compile(r"timeout --kill-after=60 (\S+) ")
BASH = shutil.which("bash") or "/bin/bash"


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


def run_bash(argv, env, tree=REPO_ROOT, bash="bash"):
    return _run([bash, "scripts/verify.sh", *argv], env, tree)


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
        "--print-plan=json",
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
    """`--print-knobs` record shape, the six duration knobs against bash's walls,
    one sampled knob per other parser, and the doc. It does not check that the
    registry covers every knob verify.sh reads."""

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


def split_role_policy(lines):
    """(role lines by role, profile lines by profile) of a role-policy report."""
    roles = {line["role"]: line for line in lines if line["kind"] == "role"}
    profiles = {line["profile"]: line for line in lines if line["kind"] == "profile"}
    return roles, profiles


D3_FIELDS = (
    "default_profile",
    "prio_class",
    "force_scope_all",
    "jobserver",
    "heavy",
    "fail_fast",
    "records_confirm_manifest",
    "release_wall_default",
    "release_selector",
    "infra_tier",
    "attempt_sidecar",
    "release_delta_skip_eligible",
    "sweep_ledger",
    "gui_feature_test_pass",
    "admission_exempt",
)
T, F = True, False
D3_ROWS = {
    "task": ("debug", "nice -n 15 ionice -c 2 -n 7 ", F, "task", "exclude-eligible", T, F,
             "90m", "release-sensitive", "selective", F, F, F, T, F),
    "merge": ("both", "nice -n 5 ", T, "merge", "exclude-eligible", T, F,
              "90m", "release-sensitive", "wholesale", T, T, F, T, T),
    "offline": ("release", "nice -n 19 ionice -c3 ", F, "none", "only", F, T,
                "13h", "workspace", "selective", F, F, F, F, F),
    "background": ("both", "nice -n 19 ionice -c3 ", T, "task", "none", T, F,
                   "90m", "release-sensitive", "wholesale", F, F, T, T, F),
}
D4_FIELDS = (
    "cargo_release_flag",
    "wall_knob",
    "narrowable",
    "confirm_manifest",
    "confirm_sidecar",
    "retry_filter_env",
)
D4_KNOB_FIELDS = ("wall_knob", "confirm_manifest", "confirm_sidecar", "retry_filter_env")
D4_ROWS = {
    "debug": ("", "REIFY_VERIFY_TEST_TIMEOUT", T, "REIFY_VERIFY_CONFIRM_MANIFEST_DEBUG",
              "REIFY_VERIFY_CONFIRM_SIDECAR_DEBUG", "REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE_DEBUG"),
    "release": ("--release", "REIFY_VERIFY_TEST_TIMEOUT_RELEASE", F,
                "REIFY_VERIFY_CONFIRM_MANIFEST_RELEASE", "REIFY_VERIFY_CONFIRM_SIDECAR_RELEASE",
                "REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE_RELEASE"),
}


class TestRolePolicyTable(unittest.TestCase):
    """`--print-role-policy` prints the PRD's D3 role table and D4 profile table."""

    @classmethod
    def setUpClass(cls):
        cls.outcome = run_py(["--print-role-policy"], base_env())
        cls.lines = json_lines(cls.outcome)
        cls.roles, cls.profiles = split_role_policy(cls.lines)

    def test_shape(self):
        self.assertEqual(len(self.lines), 6)
        self.assertTrue(all(line["v"] == 1 for line in self.lines))
        self.assertEqual(
            [line["role"] for line in self.lines if line["kind"] == "role"],
            list(D3_ROWS),
        )
        self.assertEqual(
            [line["profile"] for line in self.lines if line["kind"] == "profile"],
            list(D4_ROWS),
        )
        self.assertEqual(self.outcome.stderr, "")

    def test_role_policies_equal_d3(self):
        for role, row in D3_ROWS.items():
            with self.subTest(role=role):
                policy = self.roles[role]["policy"]
                self.assertEqual(set(policy), set(D3_FIELDS))
                expected = dict(zip(D3_FIELDS, row))
                prio = policy["prio_class"]
                self.assertIsInstance(prio, dict)
                self.assertEqual(prio["prefix"], expected.pop("prio_class"))
                actual = {k: v for k, v in policy.items() if k != "prio_class"}
                self.assertEqual(actual, expected)

    def test_idle_roles_share_one_prio_class(self):
        self.assertEqual(
            self.roles["offline"]["policy"]["prio_class"]["name"],
            self.roles["background"]["policy"]["prio_class"]["name"],
        )

    def test_profile_policies_equal_d4(self):
        for profile, row in D4_ROWS.items():
            with self.subTest(profile=profile):
                policy = self.profiles[profile]["policy"]
                self.assertEqual(set(policy), set(D4_FIELDS))
                self.assertEqual(policy, dict(zip(D4_FIELDS, row)))

    def test_profile_knobs_are_registry_knobs(self):
        records, _ = knob_report(base_env())
        for profile, line in self.profiles.items():
            for name in D4_KNOB_FIELDS:
                with self.subTest(profile=profile, field=name):
                    self.assertIn(line["policy"][name], records)

    def test_release_wall_default_is_the_registry_default(self):
        records, _ = knob_report(base_env())
        registry_default = records["REIFY_VERIFY_TEST_TIMEOUT_RELEASE"]["default"]
        for role in ("task", "merge", "background"):
            with self.subTest(role=role):
                self.assertEqual(
                    self.roles[role]["policy"]["release_wall_default"], registry_default
                )

    def test_queries_are_exclusive(self):
        py = run_py(["--print-knobs", "--print-role-policy"], base_env())
        self.assertEqual(py.returncode, 64)
        self.assertEqual(py.stdout, "")
        self.assertEqual(
            py.stderr,
            f"{ERROR_PREFIX}query already set to '--print-knobs', "
            "got '--print-role-policy'\n",
        )
        repeated = run_py(["--print-role-policy", "--print-role-policy"], base_env())
        self.assertEqual(repeated.returncode, 0, repeated.stderr)
        self.assertEqual(repeated.stdout, self.outcome.stdout)

    def test_a_query_takes_no_action(self):
        cases = (
            (["lint", "--print-knobs"], "--print-knobs", "lint"),
            (["--print-role-policy", "test", "--print-plan"], "--print-role-policy", "test"),
            (["all", "--print-plan", "--print-role-policy"], "--print-role-policy", "all"),
        )
        for argv, query, action in cases:
            with self.subTest(argv=argv):
                py = run_py(argv, base_env())
                self.assertEqual(py.returncode, 64)
                self.assertEqual(py.stdout, "")
                self.assertEqual(
                    py.stderr, f"{ERROR_PREFIX}query '{query}' takes no action (got '{action}')\n"
                )

    def test_a_query_takes_the_same_validation_path(self):
        bogus = run_py(["--print-role-policy"], base_env(DF_VERIFY_ROLE="bogus"))
        self.assertEqual(bogus.returncode, 64)
        self.assertEqual(
            bogus.stderr,
            f"{ERROR_PREFIX}unknown DF_VERIFY_ROLE 'bogus' "
            "(want task|merge|offline|background)\n",
        )
        weird = run_py(["--print-role-policy", "--profile=weird"], base_env())
        self.assertEqual(weird.returncode, 64)
        self.assertEqual(
            weird.stderr,
            f"{ERROR_PREFIX}invalid --profile 'weird' (want debug|release|both)\n",
        )

    def test_bash_rejects_the_query_flag(self):
        bash = run_bash(["--print-plan", "--print-role-policy"], base_env())
        self.assertEqual(bash.returncode, 64)
        self.assertEqual(
            bash.stderr.splitlines()[0],
            f"{ERROR_PREFIX}unknown argument '--print-role-policy'",
        )


PLAN_HEADER_PREFIX = "# verify.sh plan — "
HEAVY_HEADER_PREFIX = "# heavy partition — HEAVY="
CARGO_LINE_RE = re.compile(r"timeout --kill-after=60 (\S+) (.*?)cargo (.*)")
MAKEFLAGS_EXPORT = "export CARGO_MAKEFLAGS=--jobserver-auth=fifo:"
OFFLINE_MAKEFLAGS_NOTE = "CARGO_MAKEFLAGS left unset (offline role"
NEXTEST_RUN = "nextest run "
GUI_FEATURE_ARGS = "-p reify-gui --features gui"
ROLES = tuple(D3_ROWS)


@dataclass(frozen=True)
class CargoLine:
    wall: str
    prio: str
    args: str


class BashPlan:
    """The facts a bash `--print-plan` stdout states, read on demand."""

    def __init__(self, stdout):
        self.text = stdout
        self.lines = stdout.splitlines()

    def header(self):
        line = next(line for line in self.lines if line.startswith(PLAN_HEADER_PREFIX))
        return dict(token.split("=", 1) for token in line[len(PLAN_HEADER_PREFIX):].split())

    def heavy_partition(self):
        return next(
            line[len(HEAVY_HEADER_PREFIX):]
            for line in self.lines
            if line.startswith(HEAVY_HEADER_PREFIX)
        )

    def jobserver_fifo(self):
        for line in self.lines:
            _, found, path = line.partition(MAKEFLAGS_EXPORT)
            if found:
                return path
        assert any(OFFLINE_MAKEFLAGS_NOTE in line for line in self.lines), (
            "the plan names neither a CARGO_MAKEFLAGS fifo nor the offline note"
        )
        return None

    def cargo_lines(self):
        return [
            CargoLine(*match.groups())
            for line in self.lines
            if not line.startswith("#")
            for match in [CARGO_LINE_RE.search(line)]
            if match
        ]

    def prio(self):
        prios = {line.prio for line in self.cargo_lines()}
        assert len(prios) == 1, f"cargo lines disagree on the prio prefix: {prios}"
        return prios.pop()

    def nextest(self, kind):
        """The `cargo nextest run` lines of one kind: debug, release or gui-feature."""
        return [
            line
            for line in self.cargo_lines()
            if line.args.startswith(NEXTEST_RUN) and _nextest_kind(line.args) == kind
        ]


def _nextest_kind(args):
    if GUI_FEATURE_ARGS in args:
        return "gui-feature"
    return "release" if "--release" in args.split() else "debug"


def bash_role_plan(role, argv, env, **kwargs):
    outcome = run_bash(argv, {**env, "DF_VERIFY_ROLE": role}, **kwargs)
    assert outcome.returncode == 0, outcome.stderr
    return BashPlan(outcome.stdout), outcome.stderr


def python_roles(env):
    outcome = run_py(["--print-role-policy"], env)
    roles, _ = split_role_policy(json_lines(outcome))
    return roles, outcome.stderr


class TestResolvedPolicyMatchesBashPlans(unittest.TestCase):
    """Each role line's policy and `resolved` values are what today's bash plans show."""

    @classmethod
    def setUpClass(cls):
        cls._fifo_dir = tempfile.TemporaryDirectory()
        fifo_dir = Path(cls._fifo_dir.name)
        for name in ("task", "merge"):
            os.mkfifo(fifo_dir / name)
        cls.fifo_env = base_env(
            REIFY_JOBSERVER_TASK_FIFO=str(fifo_dir / "task"),
            REIFY_JOBSERVER_MERGE_FIFO=str(fifo_dir / "merge"),
            REIFY_JOBSERVER_SKIP_LIVENESS_PROBE="1",
        )
        cls.roles, cls.stderr = python_roles(cls.fifo_env)
        cls.both = {
            role: bash_role_plan(role, ["test", "--profile", "both", "--print-plan"], cls.fifo_env)[0]
            for role in ROLES
        }

    @classmethod
    def tearDownClass(cls):
        cls._fifo_dir.cleanup()

    def test_nextest_is_present(self):
        for role, plan in self.both.items():
            self.assertEqual(
                plan.header()["nextest"], "1", f"{role}: cargo-nextest must be on PATH"
            )

    def test_resolved_shape(self):
        self.assertEqual(self.stderr, "")
        for role in ROLES:
            with self.subTest(role=role):
                resolved = self.roles[role]["resolved"]
                self.assertEqual(
                    set(resolved),
                    {"walls", "prio_prefix", "prio_warning", "jobserver_fifo", "heavy_partition"},
                )
                self.assertEqual(set(resolved["walls"]), {"debug", "release"})

    def test_default_profile(self):
        for role in ROLES:
            with self.subTest(role=role):
                plan, _ = bash_role_plan(role, ["test", "--print-plan"], base_env())
                self.assertEqual(
                    plan.header()["profile"], self.roles[role]["policy"]["default_profile"]
                )

    def test_profile_both_plans(self):
        for role, plan in self.both.items():
            with self.subTest(role=role):
                line = self.roles[role]
                policy, resolved = line["policy"], line["resolved"]
                (debug,) = plan.nextest("debug")
                (release,) = plan.nextest("release")
                self.assertEqual(debug.wall, resolved["walls"]["debug"])
                self.assertEqual(release.wall, resolved["walls"]["release"])
                self.assertEqual(plan.prio(), resolved["prio_prefix"])
                self.assertIsNone(resolved["prio_warning"])
                workspace = policy["release_selector"] == "workspace"
                self.assertEqual(release.args[len(NEXTEST_RUN):].startswith("--workspace"), workspace)
                if not workspace:
                    self.assertTrue(release.args[len(NEXTEST_RUN):].startswith("-p "))
                for nextest in (debug, release):
                    self.assertEqual(
                        "--no-fail-fast" in nextest.args.split(), not policy["fail_fast"]
                    )
                self.assertEqual(
                    "_confirm_rc=$?" in plan.text, policy["records_confirm_manifest"]
                )
                self.assertEqual(
                    bool(plan.nextest("gui-feature")), policy["gui_feature_test_pass"]
                )
                self.assertEqual(plan.heavy_partition(), resolved["heavy_partition"])
                self.assertEqual(plan.jobserver_fifo(), resolved["jobserver_fifo"])

    def test_heavy_exclusion_knob(self):
        env = base_env(REIFY_GATE_EXCLUDE_HEAVY="1")
        roles, _ = python_roles(env)
        expected = {"task": "excluded", "merge": "excluded", "offline": "only", "background": "included"}
        for role in ROLES:
            with self.subTest(role=role):
                plan, _ = bash_role_plan(role, ["test", "--print-plan"], env)
                self.assertEqual(plan.heavy_partition(), roles[role]["resolved"]["heavy_partition"])
                self.assertEqual(plan.heavy_partition(), expected[role])

    def test_wall_overrides(self):
        env = base_env(REIFY_VERIFY_TEST_TIMEOUT="7m", REIFY_VERIFY_TEST_TIMEOUT_RELEASE="7h")
        roles, py_stderr = python_roles(env)
        self.assertEqual(py_stderr, "")
        for role in ROLES:
            with self.subTest(role=role):
                plan, _ = bash_role_plan(role, ["test", "--profile", "both", "--print-plan"], env)
                walls = roles[role]["resolved"]["walls"]
                self.assertEqual(walls, {"debug": "7m", "release": "7h"})
                self.assertEqual(plan.nextest("debug")[0].wall, walls["debug"])
                self.assertEqual(plan.nextest("release")[0].wall, walls["release"])

    def test_a_malformed_release_wall_warns_once_per_default(self):
        knob = "REIFY_VERIFY_TEST_TIMEOUT_RELEASE"
        env = base_env(**{knob: "abc"})
        roles, py_stderr = python_roles(env)
        bash_lines = set()
        for role in ROLES:
            with self.subTest(role=role):
                plan, bash_stderr = bash_role_plan(
                    role, ["test", "--profile", "both", "--print-plan"], env
                )
                bash_lines.update(bash_stderr.splitlines())
                release = roles[role]["resolved"]["walls"]["release"]
                self.assertEqual(release, "13h" if role == "offline" else "90m")
                self.assertEqual(plan.nextest("release")[0].wall, release)
        expected = [
            f"verify.sh: WARNING: invalid {knob}=abc; using default {default}"
            for default in ("90m", "13h")
        ]
        self.assertEqual(py_stderr.splitlines(), expected)
        for line in expected:
            self.assertIn(line, bash_lines)


class TestForcedScopeInTreeUnderTestClone(unittest.TestCase):
    """force_scope_all is what bash does with --scope staged, in a clone with no MERGE_HEAD."""

    @classmethod
    def setUpClass(cls):
        cls._scratch = tempfile.TemporaryDirectory()
        cls.clone = shared_clone(Path(cls._scratch.name) / "tree")
        cls.roles, _ = python_roles(base_env())

    @classmethod
    def tearDownClass(cls):
        cls._scratch.cleanup()

    def test_scope_staged(self):
        for role in ROLES:
            with self.subTest(role=role):
                forced = self.roles[role]["policy"]["force_scope_all"]
                plan, stderr = bash_role_plan(
                    role, ["test", "--scope", "staged", "--print-plan"], base_env(), tree=self.clone
                )
                self.assertEqual(plan.header()["scope"], "all" if forced else "staged")
                notice = (
                    f"verify.sh: DF_VERIFY_ROLE={role} — forcing --scope all "
                    "(integrity gate never narrows, contract C2)"
                )
                self.assertEqual(notice in stderr.splitlines(), forced)


def path_farm(dest, excluded):
    """A PATH directory linking every executable on PATH except `excluded`."""
    dest.mkdir()
    linked = set(excluded)
    for directory in base_env()["PATH"].split(os.pathsep):
        if not os.path.isdir(directory):
            continue
        for entry in sorted(os.scandir(directory), key=lambda e: e.name):
            if entry.name in linked or not entry.is_file() or not os.access(entry.path, os.X_OK):
                continue
            (dest / entry.name).symlink_to(entry.path)
            linked.add(entry.name)
    return dest


class TestPrioDegradedTools(unittest.TestCase):
    """Without ionice, or without nice, the prio prefix and warning degrade as bash's do."""

    FARMS = {"no-ionice": ("ionice",), "no-nice": ("nice", "ionice")}

    @classmethod
    def setUpClass(cls):
        cls._scratch = tempfile.TemporaryDirectory()
        cls.farms = {
            name: path_farm(Path(cls._scratch.name) / name, excluded)
            for name, excluded in cls.FARMS.items()
        }

    @classmethod
    def tearDownClass(cls):
        cls._scratch.cleanup()

    def test_degraded_prio(self):
        for farm_name, farm in self.farms.items():
            env = base_env(PATH=str(farm))
            roles, _ = python_roles(env)
            for role in ROLES:
                with self.subTest(farm=farm_name, role=role):
                    plan, stderr = bash_role_plan(role, ["lint", "--print-plan"], env, bash=BASH)
                    resolved = roles[role]["resolved"]
                    self.assertEqual(plan.prio(), resolved["prio_prefix"])
                    warnings = [line for line in stderr.splitlines() if "not found;" in line]
                    self.assertEqual(warnings, [resolved["prio_warning"]] if resolved["prio_warning"] else [])


INFRA_MAP = REPO_ROOT / "scripts" / "verify-pipeline-infra-tests.txt"
PACKAGE_DIR = REPO_ROOT / "scripts" / "reify_verify"
PIPELINE_GUARD = REPO_ROOT / "scripts" / "verify-pipeline-guard.sh"
VERIFY_GLOB = "tests/infra/test_verify_*.sh"
POLICY_WRAPPER = "tests/infra/test_verify_policy.sh"


def infra_map_rows():
    """The map's active rows: both whitespace-separated fields non-empty."""
    rows = set()
    for line in INFRA_MAP.read_text(encoding="utf-8").splitlines():
        if re.match(r"\s*#", line):
            continue
        fields = line.split()
        if len(fields) >= 2:
            rows.add((fields[0], fields[1]))
    return rows


def package_modules():
    modules = [f"scripts/reify_verify/{path.name}" for path in sorted(PACKAGE_DIR.glob("*.py"))]
    return [*modules, "scripts/verify.py"]


class TestInfraMapRegistration(unittest.TestCase):
    """Moving logic out of verify.sh never narrows task-tier infra selection (D14)."""

    def assert_row(self, rows, artifact, glob):
        self.assertTrue((artifact, glob) in rows, f"no map row: {artifact}    {glob}")

    def test_every_package_module_selects_both_globs(self):
        rows = infra_map_rows()
        modules = package_modules()
        self.assertGreater(len(modules), 1)
        for module in modules:
            with self.subTest(module=module):
                self.assert_row(rows, module, VERIFY_GLOB)
                self.assert_row(rows, module, POLICY_WRAPPER)

    def test_this_suites_own_files_select_its_wrapper(self):
        rows = infra_map_rows()
        for path in (
            "tests/infra/test_verify_policy.py",
            "tests/infra/tree_under_test.py",
            "docs/notes/verify-pipeline-knobs.md",
        ):
            with self.subTest(path=path):
                self.assert_row(rows, path, POLICY_WRAPPER)

    def test_the_guard_counts_every_module_as_registered(self):
        for module in package_modules():
            with self.subTest(module=module):
                outcome = _run(
                    ["bash", str(PIPELINE_GUARD), "is-registered", module], base_env(), REPO_ROOT
                )
                self.assertEqual(outcome.returncode, 0, outcome.stdout + outcome.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
