#!/usr/bin/env python3
"""
test_pcc_run_probe.py — running one probe in scripts/prd-capability-check.py:
run_probe(), build_command()'s argv and binary location, and the fixture
presence run_probe() measures before launch.
Shared loader and helpers: scripts/_pcc_test_support.py; runner:
tests/infra/test_prd_capability_check.sh.
"""

import os
import shlex
import shutil
import tempfile
import textwrap
import unittest
import unittest.mock

from _pcc_test_support import (
    pcc,
    _REPO_ROOT,
    _VALUE_CELLS_FIXTURE,
    _ts_stub_echo_cache,
    _ts_stub_not_executable,
)


# ---------------------------------------------------------------------------
# step-09 (RED): run_probe() command construction + binary location + capture
# ---------------------------------------------------------------------------

class TestRunProbe(unittest.TestCase):
    """Tests for run_probe() + build_command shapes, via stub binaries.

    build_command shapes pass immediately (step-08 implemented it).
    run_probe tests FAIL until step-10 adds run_probe() + full binary resolution.
    """

    def setUp(self):
        self._tmpdir = tempfile.mkdtemp(prefix="prd_gate_test_")
        self._stub_idx = 0

    def tearDown(self):
        shutil.rmtree(self._tmpdir, ignore_errors=True)

    def _make_stub(self, stdout_text="", stderr_text="", exit_code=0, print_cwd=False):
        """Create a temporary executable stub shell script."""
        self._stub_idx += 1
        path = os.path.join(self._tmpdir, f"stub{self._stub_idx}")
        lines = ["#!/bin/sh"]
        if print_cwd:
            lines.append('echo "CWD=$PWD"')
        if stdout_text:
            lines.append(f"printf '%s' {shlex.quote(stdout_text)}")
        if stderr_text:
            lines.append(f"printf '%s' {shlex.quote(stderr_text)} >&2")
        lines.append(f"exit {exit_code}")
        with open(path, "w") as f:
            f.write("\n".join(lines) + "\n")
        os.chmod(path, 0o755)
        return path

    def _make_probe(self, kind="check",
                    fixture="tests/prd-gate/fixtures/ir_clean_eval.ri",
                    match=None):
        return pcc.Probe(
            capability="test",
            probe_kind=kind,
            fixture=fixture,
            expected={"observation": "present",
                      "match": match if match is not None else {}},
        )

    # ── build_command shapes ──────────────────────────────────────────────────

    def test_build_command_check_shape(self):
        """check → [reify, check, <abs-fixture>]; fixture is resolved to absolute path."""
        fixture_rel = "tests/prd-gate/fixtures/revolute_silent_accept.ri"
        probe = self._make_probe("check", fixture_rel)
        with unittest.mock.patch.dict(os.environ, {"REIFY_BIN": "reify"}):
            cmd = pcc.build_command(probe)
        # Binary + subcommand are unchanged.
        self.assertEqual(cmd[:2], ["reify", "check"])
        # Fixture is resolved to an absolute path.
        self.assertTrue(os.path.isabs(cmd[-1]),
                        f"fixture must be absolute; got {cmd[-1]!r}")
        self.assertTrue(cmd[-1].endswith(fixture_rel),
                        f"absolute fixture must end with repo-relative path; got {cmd[-1]!r}")

    def test_build_command_ir_shape(self):
        """ir → [reify, eval, <abs-fixture>]; fixture is resolved to absolute path."""
        fixture_rel = "tests/prd-gate/fixtures/ir_clean_eval.ri"
        probe = self._make_probe("ir", fixture_rel)
        with unittest.mock.patch.dict(os.environ, {"REIFY_BIN": "reify"}):
            cmd = pcc.build_command(probe)
        # Binary + subcommand are unchanged.
        self.assertEqual(cmd[:2], ["reify", "eval"])
        # Fixture is resolved to an absolute path.
        self.assertTrue(os.path.isabs(cmd[-1]),
                        f"fixture must be absolute; got {cmd[-1]!r}")
        self.assertTrue(cmd[-1].endswith(fixture_rel),
                        f"absolute fixture must end with repo-relative path; got {cmd[-1]!r}")

    def test_build_command_grammar_shape(self):
        """grammar → [tree-sitter, parse, --quiet, <abs-fixture>]; fixture is absolute."""
        fixture_rel = "tests/prd-gate/fixtures/arrow_type.ri"
        probe = self._make_probe("grammar", fixture_rel)
        with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": "tree-sitter"}):
            cmd = pcc.build_command(probe)
        # Binary + flags are unchanged.
        self.assertEqual(cmd[:3], ["tree-sitter", "parse", "--quiet"])
        # Fixture is resolved to an absolute path.
        self.assertTrue(os.path.isabs(cmd[-1]),
                        f"fixture must be absolute; got {cmd[-1]!r}")
        self.assertTrue(cmd[-1].endswith(fixture_rel),
                        f"absolute fixture must end with repo-relative path; got {cmd[-1]!r}")

    def test_build_command_reify_bin_override(self):
        """REIFY_BIN env var overrides the reify binary in the command."""
        probe = self._make_probe("check", "x.ri")
        with unittest.mock.patch.dict(os.environ, {"REIFY_BIN": "/custom/reify"}):
            cmd = pcc.build_command(probe)
        self.assertEqual(cmd[0], "/custom/reify")

    def test_build_command_tree_sitter_bin_override(self):
        """TREE_SITTER_BIN env var overrides the tree-sitter binary in the command."""
        probe = self._make_probe("grammar", "x.ri")
        with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": "/custom/ts"}):
            cmd = pcc.build_command(probe)
        self.assertEqual(cmd[0], "/custom/ts")

    # ── run_probe existence and basic capture ─────────────────────────────────

    def test_run_probe_exists(self):
        """run_probe must be a callable in the harness module."""
        self.assertTrue(hasattr(pcc, "run_probe"), "missing run_probe()")
        self.assertTrue(callable(pcc.run_probe))

    def test_run_probe_returns_proberun(self):
        """run_probe(probe) returns a ProbeRun instance."""
        stub = self._make_stub(exit_code=0)
        probe = self._make_probe("check")
        with unittest.mock.patch.dict(os.environ, {"REIFY_BIN": stub}):
            run = pcc.run_probe(probe)
        self.assertIsInstance(run, pcc.ProbeRun)

    def test_run_probe_captures_exit_code(self):
        """run_probe captures the subprocess exit code."""
        stub = self._make_stub(exit_code=42)
        probe = self._make_probe("ir")
        with unittest.mock.patch.dict(os.environ, {"REIFY_BIN": stub}):
            run = pcc.run_probe(probe)
        self.assertEqual(run.exit_code, 42)

    def test_run_probe_captures_stdout(self):
        """run_probe captures stdout from the subprocess."""
        stub = self._make_stub(stdout_text="hello stdout", exit_code=0)
        probe = self._make_probe("check")
        with unittest.mock.patch.dict(os.environ, {"REIFY_BIN": stub}):
            run = pcc.run_probe(probe)
        self.assertIn("hello stdout", run.stdout)

    def test_run_probe_captures_stderr(self):
        """run_probe captures stderr from the subprocess."""
        stub = self._make_stub(stderr_text="hello stderr", exit_code=1)
        probe = self._make_probe("check")
        with unittest.mock.patch.dict(os.environ, {"REIFY_BIN": stub}):
            run = pcc.run_probe(probe)
        self.assertIn("hello stderr", run.stderr)

    def test_grammar_run_probe_uses_tree_sitter_stub(self):
        """Grammar probe runs via TREE_SITTER_BIN stub."""
        stub = self._make_stub(stdout_text="ts-ran", exit_code=0)
        probe = self._make_probe("grammar")
        with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}):
            run = pcc.run_probe(probe)
        self.assertEqual(run.exit_code, 0)
        self.assertIn("ts-ran", run.stdout)

    def test_grammar_run_probe_cwd_is_tree_sitter_reify_dir(self):
        """Grammar probe subprocess CWD must be the tree-sitter-reify directory."""
        stub = self._make_stub(print_cwd=True, exit_code=0)
        probe = self._make_probe("grammar")
        with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}):
            run = pcc.run_probe(probe)
        # Stub echoes CWD=$PWD; CWD must reference the tree-sitter-reify dir
        self.assertIn(
            "tree-sitter-reify", run.stdout,
            "grammar probe must run with CWD inside tree-sitter-reify/",
        )

    # ── harness-error cases: missing binary, grammar load failure ─────────────

    def test_missing_reify_binary_produces_harness_error(self):
        """REIFY_BIN=/nonexistent → evaluate() verdict is not PASS/FAIL/UNPROVABLE."""
        probe = pcc.Probe(
            capability="test",
            probe_kind="check",
            fixture="tests/prd-gate/fixtures/revolute_silent_accept.ri",
            expected={"observation": "present", "match": {"exit_code": 1}},
        )
        with unittest.mock.patch.dict(os.environ, {"REIFY_BIN": "/nonexistent-reify-xyz"}):
            result = pcc.evaluate(probe)  # uses the real run_probe default
        self.assertNotIn(
            result.verdict,
            (pcc.PASS, pcc.FAIL, pcc.UNPROVABLE),
            "missing binary must produce harness-error verdict, not PASS/FAIL/UNPROVABLE",
        )

    def test_non_executable_tree_sitter_binary_produces_harness_error(self):
        """A tree-sitter CLI that EXISTS but cannot be LAUNCHED → sentinel, not a raise.

        EACCES raises PermissionError, which `except FileNotFoundError` did not
        catch; run_probe() must represent it as "could not be launched" like
        ENOENT, or the exception escapes grammar_substrate_usable() and turns a
        one-test skip into a whole-suite loss.  Merely calling run_probe() is
        part of the assertion — an escaping exception errors the test outright.
        """
        stub = _ts_stub_not_executable(self._tmpdir)
        self.assertTrue(
            os.path.isfile(stub),
            "precondition: the stub must EXIST, else this degenerates into the "
            "already-covered missing-CLI case",
        )
        self.assertFalse(
            os.access(stub, os.X_OK),
            "precondition: the stub must not be executable",
        )
        probe = self._make_probe("grammar", "tests/prd-gate/fixtures/arrow_type.ri")
        with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}):
            run = pcc.run_probe(probe)
        self.assertIn(
            pcc._BINARY_NOT_FOUND_SENTINEL, run.stderr,
            "an unlaunchable binary must reach observe() through the same "
            "sentinel channel as a missing one",
        )
        self.assertEqual(run.exit_code, 127)

    def test_grammar_load_failure_produces_harness_error(self):
        """Grammar 'Failed to load language' → harness-error verdict from evaluate()."""
        stub = self._make_stub(
            stderr_text="Failed to load language: reify",
            exit_code=1,
        )
        probe = pcc.Probe(
            capability="test",
            probe_kind="grammar",
            fixture="tests/prd-gate/fixtures/arrow_type.ri",
            expected={"observation": "absent", "match": {}},
        )
        with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}):
            result = pcc.evaluate(probe)  # uses the real run_probe default
        self.assertNotIn(
            result.verdict,
            (pcc.PASS, pcc.FAIL, pcc.UNPROVABLE),
            "grammar load failure must produce harness-error verdict",
        )


def _write_file_exists_stub(tmpdir, name):
    """Write a stub binary that exits 0 iff any argv entry is an existing file."""
    path = os.path.join(tmpdir, name)
    script = textwrap.dedent("""\
        #!/bin/sh
        # Exit 0 if any argument names an existing regular file; else exit 1.
        for arg in "$@"; do
            if [ -f "$arg" ]; then
                exit 0
            fi
        done
        exit 1
    """)
    with open(path, "w") as f:
        f.write(script)
    os.chmod(path, 0o755)
    return path


# ---------------------------------------------------------------------------
# step-15 (RED): regression — grammar-probe fixture must be absolute in build_command
# ---------------------------------------------------------------------------

class TestBuildCommandAbsoluteFixture(unittest.TestCase):
    """Regression tests for the grammar fixture-resolution bug.

    build_command() currently emits probe.fixture verbatim (repo-relative).
    When run_probe() executes grammar probes with CWD=tree-sitter-reify/, the
    relative path resolves under that wrong directory — the fixture is not found,
    tree-sitter exits 1, and observe() misclassifies the file-not-found as ABSENT,
    making every grammar verdict wrong.

    These tests FAIL on current code because build_command() returns a relative path:
    - test_*_fixture_is_absolute: os.path.isabs() on a relative path → False → FAIL.
    - test_grammar_run_probe_locates_fixture: stub exits 1 (file not found in CWD).
    """

    def setUp(self):
        self._tmpdir = tempfile.mkdtemp(prefix="prd_gate_abs_test_")
        self._stub_idx = 0

    def tearDown(self):
        shutil.rmtree(self._tmpdir, ignore_errors=True)

    def _make_probe(self, kind, fixture):
        return pcc.Probe(
            capability="abs-path-test",
            probe_kind=kind,
            fixture=fixture,
            expected={"observation": "present", "match": {}},
        )

    # ── (a) build_command must return absolute fixture paths for all kinds ─────

    def test_grammar_fixture_is_absolute(self):
        """grammar: build_command(probe, repo_root=_REPO_ROOT) last arg is absolute path."""
        fixture_rel = "tests/prd-gate/fixtures/arrow_type.ri"
        probe = self._make_probe("grammar", fixture_rel)
        with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": "tree-sitter"}):
            cmd = pcc.build_command(probe, repo_root=_REPO_ROOT)
        abs_fixture = cmd[-1]
        self.assertTrue(
            os.path.isabs(abs_fixture),
            f"build_command fixture must be absolute; got {abs_fixture!r}",
        )
        self.assertTrue(
            os.path.isfile(abs_fixture),
            f"absolute fixture path must exist on disk: {abs_fixture!r}",
        )
        self.assertTrue(
            abs_fixture.endswith(fixture_rel),
            f"absolute path must end with repo-relative fixture; got {abs_fixture!r}",
        )

    def test_check_fixture_is_absolute(self):
        """check: build_command(probe, repo_root=_REPO_ROOT) last arg is absolute path."""
        fixture_rel = "tests/prd-gate/fixtures/revolute_silent_accept.ri"
        probe = self._make_probe("check", fixture_rel)
        with unittest.mock.patch.dict(os.environ, {"REIFY_BIN": "reify"}):
            cmd = pcc.build_command(probe, repo_root=_REPO_ROOT)
        abs_fixture = cmd[-1]
        self.assertTrue(
            os.path.isabs(abs_fixture),
            f"build_command fixture must be absolute; got {abs_fixture!r}",
        )
        self.assertTrue(
            os.path.isfile(abs_fixture),
            f"absolute fixture path must exist on disk: {abs_fixture!r}",
        )
        self.assertTrue(
            abs_fixture.endswith(fixture_rel),
            f"absolute path must end with repo-relative fixture; got {abs_fixture!r}",
        )

    def test_ir_fixture_is_absolute(self):
        """ir: build_command(probe, repo_root=_REPO_ROOT) last arg is absolute path."""
        fixture_rel = "tests/prd-gate/fixtures/ir_clean_eval.ri"
        probe = self._make_probe("ir", fixture_rel)
        with unittest.mock.patch.dict(os.environ, {"REIFY_BIN": "reify"}):
            cmd = pcc.build_command(probe, repo_root=_REPO_ROOT)
        abs_fixture = cmd[-1]
        self.assertTrue(
            os.path.isabs(abs_fixture),
            f"build_command fixture must be absolute; got {abs_fixture!r}",
        )
        self.assertTrue(
            os.path.isfile(abs_fixture),
            f"absolute fixture path must exist on disk: {abs_fixture!r}",
        )
        self.assertTrue(
            abs_fixture.endswith(fixture_rel),
            f"absolute path must end with repo-relative fixture; got {abs_fixture!r}",
        )

    # ── (b) run_probe must locate the grammar fixture despite CWD change ───────

    def _make_file_exists_stub(self):
        """Create a stub TREE_SITTER_BIN that exits 0 iff any argv entry is an existing file."""
        self._stub_idx += 1
        return _write_file_exists_stub(self._tmpdir, f"ts_stub{self._stub_idx}")

    def test_grammar_run_probe_locates_fixture(self):
        """Grammar run_probe() passes an absolute fixture path so the stub locates it.

        Premises (verified on disk in prereq-3):
          - tests/prd-gate/fixtures/arrow_type.ri exists in the repo.

        The stub exits 0 iff any argv entry is an existing regular file (-f test).
        With a repo-relative path and CWD=tree-sitter-reify/, the relative path
        resolves incorrectly → stub exits 1.
        With an absolute path (after the fix) → stub exits 0 regardless of CWD.

        If this test fails with exit_code=1, it means build_command still returns
        a relative path that is not found under the grammar CWD.
        """
        fixture_rel = "tests/prd-gate/fixtures/arrow_type.ri"
        fixture_abs = os.path.join(_REPO_ROOT, fixture_rel)
        # Precondition: fixture must exist so the stub can find it when given abs path.
        self.assertTrue(
            os.path.isfile(fixture_abs),
            f"precondition: fixture must exist on disk: {fixture_abs}",
        )

        stub = self._make_file_exists_stub()
        probe = self._make_probe("grammar", fixture_rel)
        with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}):
            run = pcc.run_probe(probe)

        self.assertEqual(
            run.exit_code, 0,
            "grammar run_probe must pass an absolute fixture path so the stub locates it "
            "(exit 0 = file found; exit 1 = file not found, i.e. build_command returned "
            "a relative path that does not resolve under CWD=tree-sitter-reify/)",
        )


# ---------------------------------------------------------------------------
# run_probe() measures whether the fixture exists when the probe launches
# ---------------------------------------------------------------------------

class TestRunProbeFixturePresence(unittest.TestCase):
    """fixture_present is measured on the very path the launched argv carries.

    The probe still launches when the fixture is absent, so the record keeps
    its executed evidence; what the absence means is observe()'s business.
    """

    _PRESENT = "tests/prd-gate/fixtures/arrow_type.ri"

    def setUp(self):
        self._tmpdir = tempfile.mkdtemp(prefix="prd_gate_presence_")

    def tearDown(self):
        shutil.rmtree(self._tmpdir, ignore_errors=True)

    @staticmethod
    def _probe(kind, fixture):
        return pcc.Probe(
            capability="presence",
            probe_kind=kind,
            fixture=fixture,
            expected={"observation": "present", "match": {}},
        )

    def _never_written_fixtures(self):
        absolute = os.path.join(self._tmpdir, "never_written.ri")
        relative = os.path.join(
            "tests", "prd-gate", "fixtures",
            f"s22_never_written_{os.path.basename(self._tmpdir)}.ri",
        )
        for fixture in (absolute, relative):
            self.assertFalse(
                os.path.exists(os.path.join(_REPO_ROOT, fixture)),
                f"precondition: {fixture} must not exist",
            )
        return {"absolute": absolute, "repo-relative": relative}

    def test_committed_fixture_is_present_and_the_tool_agrees(self):
        stub = _write_file_exists_stub(self._tmpdir, "ts_file_exists")
        with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}):
            run = pcc.run_probe(self._probe("grammar", self._PRESENT))
        self.assertIs(run.fixture_present, True)
        self.assertEqual(
            run.exit_code, 0,
            "the stub found the fixture on the argv path, so the measured path "
            "and the launched path agree",
        )

    def test_never_written_fixture_is_absent_and_the_tool_agrees(self):
        stub = _write_file_exists_stub(self._tmpdir, "ts_file_exists")
        for label, fixture in self._never_written_fixtures().items():
            with self.subTest(fixture=label):
                with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}):
                    run = pcc.run_probe(self._probe("grammar", fixture))
                self.assertIs(run.fixture_present, False)
                self.assertEqual(run.exit_code, 1, "the stub found no file either")

    def test_absent_fixture_still_launches_the_probe(self):
        stub = os.path.join(self._tmpdir, "reify_says_ran")
        with open(stub, "w") as f:
            f.write("#!/bin/sh\necho ran\nexit 3\n")
        os.chmod(stub, 0o755)
        for label, fixture in self._never_written_fixtures().items():
            with self.subTest(fixture=label):
                with unittest.mock.patch.dict(os.environ, {"REIFY_BIN": stub}):
                    run = pcc.run_probe(self._probe("check", fixture))
                self.assertIs(run.fixture_present, False)
                self.assertIn("ran", run.stdout, "the probe must still have run")
                self.assertEqual(run.exit_code, 3, "its own exit code is captured")


# ---------------------------------------------------------------------------
# #6876 step-03 (RED): build_command() argv for the "value" kind
# ---------------------------------------------------------------------------

class TestBuildCommandValueKind(unittest.TestCase):
    """A `value` probe runs the same command an `ir` probe does.

    The kind names the OBSERVATION MODEL, not the command: both ask
    `reify eval <fixture>` and differ only in what they read off the result.
    Pinning argv equality rather than re-spelling the expected list is what
    keeps the two from drifting apart into two ways of asking one question.
    """

    _VALUE_MATCH = {
        "stdout_value": {
            "pattern": r"ValueCleanEvalCells\.damping_ratio = ([-+0-9.eE]+)",
            "min": 0.01,
            "max": 0.03,
        }
    }

    def _make_probe(self, kind, fixture=None, match=None):
        return pcc.Probe(
            capability="damping ratio is in range",
            probe_kind=kind,
            fixture=fixture if fixture is not None else _VALUE_CELLS_FIXTURE,
            expected={
                "observation": "present",
                "match": match if match is not None else {},
            },
        )

    def test_value_argv_shape(self):
        """value → [reify, eval, <abs-fixture>]."""
        probe = self._make_probe("value", match=dict(self._VALUE_MATCH))
        with unittest.mock.patch.dict(os.environ, {"REIFY_BIN": "reify"}):
            cmd = pcc.build_command(probe, repo_root=_REPO_ROOT)
        self.assertEqual(
            cmd, ["reify", "eval", os.path.join(_REPO_ROOT, _VALUE_CELLS_FIXTURE)]
        )

    def test_value_argv_identical_to_ir_argv(self):
        """SPOT: the two kinds differ in observation model, never in argv."""
        value_probe = self._make_probe("value", match=dict(self._VALUE_MATCH))
        ir_probe = self._make_probe("ir", match={"stderr_contains": "EvalError"})
        with unittest.mock.patch.dict(os.environ, {"REIFY_BIN": "reify"}):
            value_cmd = pcc.build_command(value_probe, repo_root=_REPO_ROOT)
            ir_cmd = pcc.build_command(ir_probe, repo_root=_REPO_ROOT)
        self.assertEqual(value_cmd, ir_cmd)

    def test_value_honours_reify_bin_override(self):
        probe = self._make_probe("value", match=dict(self._VALUE_MATCH))
        with unittest.mock.patch.dict(os.environ, {"REIFY_BIN": "/custom/reify"}):
            cmd = pcc.build_command(probe, repo_root=_REPO_ROOT)
        self.assertEqual(cmd[0], "/custom/reify")

    def test_value_resolves_relative_fixture(self):
        probe = self._make_probe("value", match=dict(self._VALUE_MATCH))
        with unittest.mock.patch.dict(os.environ, {"REIFY_BIN": "reify"}):
            cmd = pcc.build_command(probe, repo_root=_REPO_ROOT)
        self.assertTrue(os.path.isabs(cmd[-1]), f"got {cmd[-1]!r}")
        self.assertTrue(cmd[-1].endswith(_VALUE_CELLS_FIXTURE), f"got {cmd[-1]!r}")

    def test_value_passes_absolute_fixture_through(self):
        absolute = os.path.join(_REPO_ROOT, _VALUE_CELLS_FIXTURE)
        probe = self._make_probe(
            "value", fixture=absolute, match=dict(self._VALUE_MATCH)
        )
        with unittest.mock.patch.dict(os.environ, {"REIFY_BIN": "reify"}):
            cmd = pcc.build_command(probe, repo_root="/some/other/root")
        self.assertEqual(cmd[-1], absolute)

    def test_value_probe_inherits_ambient_env(self):
        """run_probe() special-cases cwd/env for `grammar` only.

        A value probe drives `reify eval`, which never touches the tree-sitter
        CLI cache, so overriding its environment would be unjustified blast
        radius — exactly the reasoning already pinned for `check`.  Measured
        through a stub that echoes back what it actually received, since the
        child's environment is what no in-process assertion can observe.
        """
        with tempfile.TemporaryDirectory() as tmpdir:
            stub = _ts_stub_echo_cache(tmpdir, name="reify_stub_value_env")
            sentinel = os.path.join(tmpdir, "ambient_cache_value")
            os.makedirs(sentinel, exist_ok=True)
            probe = self._make_probe("value", match=dict(self._VALUE_MATCH))
            with unittest.mock.patch.dict(
                os.environ, {"REIFY_BIN": stub, "XDG_CACHE_HOME": sentinel}
            ):
                run = pcc.run_probe(probe)
        self.assertEqual(run.exit_code, 0, f"stub failed: {run.stderr!r}")
        self.assertEqual(run.stdout.strip(), f"XDG_CACHE_HOME={sentinel}")


if __name__ == "__main__":
    unittest.main()
