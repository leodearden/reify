#!/usr/bin/env python3
"""
test_verify_scope_source_mode.py — stdlib unittest for the SOURCE MODE of
tests/infra/test_verify_scope.sh.

Discovered through the thin wrapper tests/infra/test_verify_scope_source_mode.sh
(run_all.sh globs `test_*.sh` only). Sourcing the suite must define every
function it declares and return before the first scenario: no banner, no
fixture, no EXIT trap. Run directly, it must still install the EXIT trap that
reclaims its fixtures. The suite is driven as a black box through
`bash -c 'source ...'` or `bash <suite>`; every value reaches bash through the
environment, never by interpolation into the -c text (a sourced file also
inherits the caller's positionals, so they are no channel either).
"""

import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SUITE = REPO_ROOT / "tests" / "infra" / "test_verify_scope.sh"

NAME = r"[A-Za-z_][A-Za-z0-9_]*"

FUNC_DEF = re.compile(
    rf"^[ \t]*(?:function[ \t]+(?P<keyword>{NAME})(?:[ \t]*\(\))?|(?P<posix>{NAME})[ \t]*\(\))\s*\{{",
    re.M,
)

DOCS_ONLY_CLASSIFICATION = "RUN_RUST=0 RUN_GUI=0 RUN_OCCT_GATE=0"

SOURCE_MODE_RETURN = 'if [ "${BASH_SOURCE[0]}" != "$0" ]; then return 0; fi\n'

REFUSAL_PHRASE = "is not a fixture this shell built"


def declared_functions(path):
    return {
        match["keyword"] or match["posix"]
        for match in FUNC_DEF.finditer(Path(path).read_text())
    }


def refusal_line(helper):
    """The line _require_fixture prints when `helper` itself refuses its fixture variable."""
    return re.compile(
        rf"^{re.escape(helper)}: \${NAME} {re.escape(REFUSAL_PHRASE)} ",
        re.M,
    )


def child_env(tmpdir, target=SUITE, extra_env=None):
    env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
    env["TMPDIR"] = str(tmpdir)
    env["_SM_TARGET"] = str(target)
    env.update(extra_env or {})
    return env


def run_sourced(target, snippet, *, cwd, tmpdir, extra_env=None):
    return subprocess.run(
        ["bash", "-c", 'source "$_SM_TARGET"\n' + snippet],
        cwd=cwd,
        env=child_env(tmpdir, target, extra_env),
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
    )


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

    def source_suite(self, snippet, *, cwd=None, extra_env=None):
        return run_sourced(
            SUITE, snippet, cwd=cwd or self.cwd, tmpdir=self.tmpdir, extra_env=extra_env
        )

    def describe(self, proc):
        return f"rc={proc.returncode}\nstdout: {proc.stdout!r}\nstderr: {proc.stderr!r}"

    def unreachable_after_source(self, target):
        proc = run_sourced(target, "declare -F", cwd=self.cwd, tmpdir=self.tmpdir)
        self.assertEqual(
            proc.returncode,
            0,
            f"sourcing {target} aborted before `declare -F` ran\n{self.describe(proc)}",
        )
        defined = {line.split()[-1] for line in proc.stdout.splitlines() if line.strip()}
        return declared_functions(target) - defined


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
        unreachable = self.unreachable_after_source(SUITE)
        self.assertEqual(
            unreachable,
            set(),
            f"{sorted(unreachable)} are declared in {SUITE.name} but not defined "
            "after `source`: define them above the source-mode return",
        )

    def test_declared_functions_reads_every_definition_form(self):
        fixture = self.make_tmpdir() / "fixture.sh"
        fixture.write_text(
            "posix() { :; }\n"
            "  indented () {\n    :\n  }\n"
            "brace_below()\n{\n    :\n}\n"
            "function keyword { :; }\n"
            "function keyword_parens() { :; }\n"
            "# commented() { :; }\n"
            "echo 'function quoted { :; }'\n"
        )
        self.assertEqual(
            declared_functions(fixture),
            {"posix", "indented", "brace_below", "keyword", "keyword_parens"},
        )

    def test_unreachable_check_flags_a_function_defined_after_the_return(self):
        fixture = self.make_tmpdir() / "fixture.sh"
        fixture.write_text(
            "early() { :; }\nfunction early_kw { :; }\n"
            + SOURCE_MODE_RETURN
            + "late() { :; }\nfunction late_kw { :; }\n"
        )
        self.assertEqual(self.unreachable_after_source(fixture), {"late", "late_kw"})

        fixture.write_text("early() { :; }\nfunction early_kw { :; }\n" + SOURCE_MODE_RETURN)
        self.assertEqual(self.unreachable_after_source(fixture), set())

    def test_unreachable_check_reports_a_source_that_aborts(self):
        fixture = self.make_tmpdir() / "fixture.sh"
        fixture.write_text(
            "set -e\nearly() { :; }\necho 'probe: source aborted' >&2\nfalse\n"
            + SOURCE_MODE_RETURN
            + "late() { :; }\n"
        )
        with self.assertRaises(AssertionError) as caught:
            self.unreachable_after_source(fixture)
        self.assertIn("probe: source aborted", str(caught.exception))


class ExecutedModeTest(SourcedSuiteTestCase):
    """Run directly, the suite must still reclaim its fixtures through its EXIT trap."""

    def test_executed_suite_reclaims_its_first_fixture_on_exit(self):
        shim_dir = self.make_tmpdir()
        git_log = shim_dir / "git.log"
        failing_git = shim_dir / "git"
        failing_git.write_text('#!/bin/sh\nprintf \'%s\\n\' "$@" >> "$_SM_GIT_LOG"\nexit 1\n')
        failing_git.chmod(0o755)
        env = child_env(
            self.tmpdir,
            extra_env={"PATH": f"{shim_dir}:{os.environ['PATH']}", "_SM_GIT_LOG": str(git_log)},
        )

        proc = subprocess.run(
            ["bash", str(SUITE)],
            cwd=self.cwd,
            env=env,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
        )

        self.assertNotEqual(proc.returncode, 0, self.describe(proc))
        self.assertTrue(git_log.is_file(), f"the suite never reached git\n{self.describe(proc)}")
        git_args = git_log.read_text().splitlines()
        self.assertIn("-C", git_args, self.describe(proc))
        fixture = Path(git_args[git_args.index("-C") + 1])
        self.assertEqual(fixture.parent, self.tmpdir, self.describe(proc))
        self.assertEqual(
            sorted(p.name for p in self.tmpdir.iterdir()),
            [],
            "the executed suite exited without reclaiming its fixture: no EXIT trap ran cleanup",
        )


class FixtureRefusalTest(SourcedSuiteTestCase):
    """A helper handed a directory no constructor built must refuse and touch nothing."""

    FIXTURE_VARS = "FIX FIX_B FIX_MOD FIX_VS FIX_PGR FIX_GECR _FW_SHIM_DIR"

    # Every declared function outside this set is held to the refusal contract.
    FIXTURE_FREE = frozenset(
        {
            "cleanup",
            "_require_fixture",
            "make_fixture",
            "make_branch_fixture",
            "make_rename_diff_fail_shim",
            "plan_has",
            "plan_lacks",
            "plan_cmdcount",
            "refute",
            "_pg_derive",
            "_check_scope_header",
            "verify_pipeline_map_globs_resolve",
        }
    )

    def git(self, repo, *args):
        return subprocess.run(
            ["git", "-C", str(repo), *args],
            env=child_env(self.tmpdir),
            capture_output=True,
            text=True,
            check=True,
        ).stdout

    def make_scratch_repo(self):
        repo = self.make_tmpdir()
        self.git(repo, "init", "-q", "-b", "main")
        self.git(repo, "config", "user.email", "scratch@test.invalid")
        self.git(repo, "config", "user.name", "Scratch")
        (repo / "seeded.txt").write_text("seeded\n")
        self.git(repo, "add", "seeded.txt")
        self.git(repo, "commit", "-q", "-m", "seed")
        (repo / "staged.txt").write_text("staged\n")
        self.git(repo, "add", "staged.txt")
        return repo

    def snapshot(self, repo):
        files = sorted(
            str(p.relative_to(repo))
            for p in repo.rglob("*")
            if ".git" not in p.relative_to(repo).parts
        )
        return (
            self.git(repo, "rev-parse", "HEAD"),
            self.git(repo, "branch", "--list"),
            self.git(repo, "status", "--porcelain=v1", "--untracked-files=all"),
            files,
        )

    def test_every_fixture_consumer_refuses_a_directory_no_constructor_built(self):
        declared = declared_functions(SUITE)
        self.assertEqual(
            self.FIXTURE_FREE - declared,
            set(),
            f"FIXTURE_FREE names functions {SUITE.name} no longer declares",
        )
        helpers = sorted(declared - self.FIXTURE_FREE)
        for expected in ("plan_for", "plan_for_branch", "_plan_for_staged_shimmed"):
            self.assertIn(expected, helpers)
        snippet = (
            f"for v in {self.FIXTURE_VARS}; do printf -v \"$v\" '%s' \"$PWD\"; done; "
            '"$_SM_HELPER" seeded.txt moved.txt seeded.txt probe-line'
        )
        for helper in helpers:
            with self.subTest(helper=helper):
                repo = self.make_scratch_repo()
                before = self.snapshot(repo)
                proc = self.source_suite(
                    snippet, cwd=repo, extra_env={"_SM_HELPER": helper}
                )
                failure = (
                    f"{helper} did not refuse through _require_fixture; if it touches "
                    f"no fixture, list it in FIXTURE_FREE\n{self.describe(proc)}"
                )
                self.assertNotEqual(proc.returncode, 0, failure)
                self.assertRegex(proc.stderr, refusal_line(helper), failure)
                self.assertEqual(self.snapshot(repo), before, failure)

    def test_refusal_names_the_variable_and_its_constructor(self):
        cases = {
            "never-assigned": "plan_for_branch docs/probe.md",
            "empty": 'FIX_B=""; plan_for_branch docs/probe.md',
        }
        for case, snippet in cases.items():
            with self.subTest(case=case):
                repo = self.make_scratch_repo()
                before = self.snapshot(repo)
                proc = self.source_suite(snippet, cwd=repo)
                self.assertNotEqual(proc.returncode, 0, self.describe(proc))
                self.assertIn("FIX_B", proc.stderr, self.describe(proc))
                self.assertIn("make_branch_fixture FIX_B", proc.stderr, self.describe(proc))
                self.assertEqual(self.snapshot(repo), before, self.describe(proc))

    def test_shimmed_helper_requires_its_shim(self):
        proc = self.source_suite(
            "make_fixture FIX; printf 'FIXTURE=%s\\n' \"$FIX\"; "
            "_plan_for_staged_shimmed crates/reify-doc/src/lib.rs"
        )
        self.assertNotEqual(proc.returncode, 0, self.describe(proc))
        self.assertIn("_FW_SHIM_DIR", proc.stderr, self.describe(proc))
        self.assertIn("make_rename_diff_fail_shim", proc.stderr, self.describe(proc))
        fixture = fixture_path(proc.stdout)
        self.assertIsNotNone(fixture, self.describe(proc))
        self.assertEqual(
            self.git(fixture, "diff", "--cached", "--name-only"),
            "",
            "the helper staged a file before refusing",
        )


if __name__ == "__main__":
    unittest.main()
