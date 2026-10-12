#!/usr/bin/env python3
"""
test_verify_scope_source_mode.py — stdlib unittest for the SOURCE MODE of
tests/infra/test_verify_scope.sh.

Discovered through the thin wrapper tests/infra/test_verify_scope_source_mode.sh
(run_all.sh globs `test_*.sh` only). Sourcing the suite must define every
function it declares and return before the first scenario: no banner, no
fixture, no EXIT trap. The suite is driven as a black box through
`bash -c 'source ...'`; every value reaches bash through the environment,
never by interpolation into the -c text (a sourced file also inherits the
caller's positionals, so they are no channel either).
"""

import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SUITE = REPO_ROOT / "tests" / "infra" / "test_verify_scope.sh"

FUNC_DEF = re.compile(r"^[ \t]*([A-Za-z_][A-Za-z0-9_]*)[ \t]*\(\)[ \t]*\{", re.M)

DOCS_ONLY_CLASSIFICATION = "RUN_RUST=0 RUN_GUI=0 RUN_OCCT_GATE=0"

SOURCE_MODE_RETURN = 'if [ "${BASH_SOURCE[0]}" != "$0" ]; then return 0; fi\n'


def declared_functions(path):
    return set(FUNC_DEF.findall(Path(path).read_text()))


def child_env(tmpdir, target=SUITE):
    env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
    env["TMPDIR"] = str(tmpdir)
    env["_SM_TARGET"] = str(target)
    return env


def run_sourced(target, snippet, *, cwd, tmpdir):
    return subprocess.run(
        ["bash", "-c", 'source "$_SM_TARGET"\n' + snippet],
        cwd=cwd,
        env=child_env(tmpdir, target),
        capture_output=True,
        text=True,
    )


def unreachable_after_source(target, cwd, tmpdir):
    proc = run_sourced(target, "declare -F", cwd=cwd, tmpdir=tmpdir)
    defined = {line.split()[-1] for line in proc.stdout.splitlines() if line.strip()}
    return declared_functions(target) - defined


def fixture_path(stdout):
    for line in stdout.splitlines():
        if line.startswith("FIXTURE="):
            return Path(line[len("FIXTURE="):])
    return None


class SourcedSuiteTestCase(unittest.TestCase):
    def make_tmpdir(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        return Path(tmp.name)

    def setUp(self):
        self.tmpdir = self.make_tmpdir()
        self.cwd = self.make_tmpdir()

    def source_suite(self, snippet):
        return run_sourced(SUITE, snippet, cwd=self.cwd, tmpdir=self.tmpdir)

    def describe(self, proc):
        return f"rc={proc.returncode}\nstdout: {proc.stdout!r}\nstderr: {proc.stderr!r}"


class SourceModeContractTest(SourcedSuiteTestCase):
    def test_sourcing_returns_control_and_runs_no_scenario(self):
        proc = self.source_suite("printf 'RETURNED\\n'")
        self.assertEqual(proc.returncode, 0, self.describe(proc))
        self.assertEqual(proc.stdout, "RETURNED\n", self.describe(proc))
        self.assertEqual(
            sorted(p.name for p in self.tmpdir.iterdir()),
            [],
            "sourcing built a fixture under TMPDIR",
        )

    def test_sourcing_installs_no_exit_trap(self):
        proc = self.source_suite("make_fixture FIX; printf 'FIXTURE=%s\\n' \"$FIX\"")
        self.assertEqual(proc.returncode, 0, self.describe(proc))
        fixture = fixture_path(proc.stdout)
        self.assertIsNotNone(fixture, self.describe(proc))
        self.assertEqual(fixture.parent, self.tmpdir, self.describe(proc))
        self.assertTrue(
            fixture.is_dir(),
            "the fixture was reclaimed when bash exited: sourcing installed an EXIT trap",
        )

        proc = self.source_suite(
            "make_fixture FIX; cleanup; [ ! -e \"$FIX\" ] && printf 'RECLAIMED\\n'"
        )
        self.assertIn("RECLAIMED", proc.stdout, self.describe(proc))

    def test_sourced_helpers_capture_plans_from_any_cwd(self):
        with self.subTest(scope="staged"):
            proc = self.source_suite(
                "make_fixture FIX; plan_for staged docs/note.md; printf '%s\\n' \"$PLAN_OUT\""
            )
            self.assertIn(DOCS_ONLY_CLASSIFICATION, proc.stdout, self.describe(proc))
        with self.subTest(scope="branch"):
            proc = self.source_suite(
                "make_branch_fixture FIX_B; plan_for_branch docs/note.md; "
                "printf '%s\\n' \"$PLAN_OUT\""
            )
            self.assertIn("scope=branch", proc.stdout, self.describe(proc))
            self.assertIn(DOCS_ONLY_CLASSIFICATION, proc.stdout, self.describe(proc))

    def test_every_declared_function_is_defined_after_sourcing(self):
        declared = declared_functions(SUITE)
        self.assertIn("make_fixture", declared)
        self.assertIn("plan_for_branch", declared)
        unreachable = unreachable_after_source(SUITE, self.cwd, self.tmpdir)
        self.assertEqual(
            unreachable,
            set(),
            f"{sorted(unreachable)} are declared in {SUITE.name} but not defined "
            "after `source`: define them above the source-mode return",
        )

    def test_unreachable_check_flags_a_function_defined_after_the_return(self):
        fixture = self.make_tmpdir() / "fixture.sh"
        fixture.write_text("early() { :; }\n" + SOURCE_MODE_RETURN + "late() { :; }\n")
        self.assertEqual(unreachable_after_source(fixture, self.cwd, self.tmpdir), {"late"})

        fixture.write_text("early() { :; }\n" + SOURCE_MODE_RETURN)
        self.assertEqual(unreachable_after_source(fixture, self.cwd, self.tmpdir), set())


if __name__ == "__main__":
    unittest.main()
