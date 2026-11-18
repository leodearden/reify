#!/usr/bin/env python3
"""
test_pcc_grammar_generate.py — scripts/prd-capability-check.py brings
tree-sitter-reify/src up to date through scripts/tree-sitter-generate.sh before
any grammar probe, so no verdict comes from a grammar older than grammar.js.
The generator is ProbeContext.grammar_generator; every case here hands in a
stub for it over a fixture repo root, so the lane's own src/ is never written.
Shared loader and helpers: scripts/_pcc_test_support.py; runner:
tests/infra/test_prd_capability_check.sh.
"""

import io
import json
import os
import shlex
import tempfile
import unittest
import unittest.mock

from _pcc_test_support import (
    pcc,
    _write_ts_stub,
    make_fixture_repo,
    probe_context,
    recording_runner,
)

_GRAMMAR_FIXTURE = "tests/prd-gate/fixtures/arrow_type.ri"
_CHECK_FIXTURE = "tests/prd-gate/fixtures/revolute_silent_accept.ri"


def _generator_stub_succeeds(tmpdir, repo_root, marker):
    """A generator that records its run in `marker`, writes parser.c and reports on stdout."""
    src = os.path.join(repo_root, "tree-sitter-reify", "src")
    return _write_ts_stub(tmpdir, "generate_succeeds", f"""\
        echo run >> {shlex.quote(marker)}
        mkdir -p {shlex.quote(src)}
        : > {shlex.quote(os.path.join(src, "parser.c"))}
        echo "tree-sitter: generated parser files"
        exit 0
    """)


def _generator_stub_fails(tmpdir, marker):
    """A generator that records its run in `marker` and fails as a lane without the CLI does."""
    return _write_ts_stub(tmpdir, "generate_fails", f"""\
        echo run >> {shlex.quote(marker)}
        echo "ERROR: tree-sitter CLI not found on PATH." >&2
        echo "Install via: cargo install tree-sitter-cli" >&2
        exit 1
    """)


def _marker_lines(marker):
    """The runs a generator stub recorded in `marker`; [] when it never ran."""
    if not os.path.exists(marker):
        return []
    with open(marker) as fh:
        return fh.read().splitlines()


class _GeneratorFixture:
    """A fixture repo root with an empty src/, and a marker file for generator stubs."""

    def setUp(self):
        self.tmp = self.enterContext(tempfile.TemporaryDirectory(prefix="pcc_generate_"))
        self.root = make_fixture_repo(os.path.join(self.tmp, "repo"))
        self.src = os.path.join(self.root, "tree-sitter-reify", "src")
        self.marker = os.path.join(self.tmp, "generator_runs")


class TestEnsureGrammarCurrent(_GeneratorFixture, unittest.TestCase):
    """ensure_grammar_current(ctx) runs the context's generator and reports (ok, reason)."""

    def _ctx(self, generator):
        return probe_context(repo_root=self.root, grammar_generator=generator)

    def test_a_context_without_a_generator_runs_nothing(self):
        self.assertEqual(pcc.ensure_grammar_current(self._ctx(None)), (True, ""))
        self.assertEqual(os.listdir(self.src), [])

    def test_a_succeeding_generator_runs_once(self):
        generator = _generator_stub_succeeds(self.tmp, self.root, self.marker)
        self.assertEqual(pcc.ensure_grammar_current(self._ctx(generator)), (True, ""))
        self.assertEqual(len(_marker_lines(self.marker)), 1)
        self.assertTrue(os.path.isfile(os.path.join(self.src, "parser.c")))

    def test_the_generator_stdout_never_reaches_this_process_stdout(self):
        """Measured at file descriptor 1: a child inherits the fd, not sys.stdout.

        --json output shares that fd, so one generator line there corrupts it.
        """
        generator = _generator_stub_succeeds(self.tmp, self.root, self.marker)
        with tempfile.TemporaryFile(dir=self.tmp) as captured:
            saved = os.dup(1)
            try:
                os.dup2(captured.fileno(), 1)
                result = pcc.ensure_grammar_current(self._ctx(generator))
            finally:
                os.dup2(saved, 1)
                os.close(saved)
            captured.seek(0)
            self.assertEqual(captured.read(), b"")
        self.assertEqual(result, (True, ""))
        self.assertEqual(len(_marker_lines(self.marker)), 1)

    def test_a_failing_generator_is_reported_on_one_line(self):
        generator = _generator_stub_fails(self.tmp, self.marker)
        ok, reason = pcc.ensure_grammar_current(self._ctx(generator))
        self.assertFalse(ok)
        self.assertIn(generator, reason)
        self.assertIn("exited 1", reason)
        self.assertIn("tree-sitter CLI not found", reason)
        self.assertNotIn("\n", reason)
        self.assertEqual(len(_marker_lines(self.marker)), 1)

    def test_a_missing_generator_is_reported_not_raised(self):
        generator = os.path.join(self.tmp, "never-created", "tree-sitter-generate.sh")
        ok, reason = pcc.ensure_grammar_current(self._ctx(generator))
        self.assertFalse(ok)
        self.assertIn(generator, reason)
        self.assertNotIn("\n", reason)


def _probe(capability, kind, fixture, match):
    return {"capability": capability, "probe_kind": kind, "fixture": fixture,
            "expected": {"observation": "present", "match": match}}


def _grammar_probe(capability="arrow type parses"):
    return _probe(capability, "grammar", _GRAMMAR_FIXTURE, {})


def _check_probe():
    return _probe("arg-vs-param rejection", "check", _CHECK_FIXTURE, {"exit_code": 1})


class TestMainBringsTheGrammarCurrentFirst(_GeneratorFixture, unittest.TestCase):
    """main() runs the generator before any grammar probe, and only when one will run."""

    def _ctx(self, generator, *, parser_c=False):
        make_fixture_repo(self.root, parser_c=parser_c)
        return probe_context(repo_root=self.root,
                             cache_home_override=os.path.join(self.tmp, "cache"),
                             grammar_generator=generator)

    def _probe_set(self, *probes):
        path = os.path.join(self.tmp, "probe-set.json")
        with open(path, "w") as fh:
            json.dump({"probes": list(probes)}, fh)
        return path

    def _run_main(self, argv, runner, ctx):
        """(exit code, stdout, stderr) of main() under `runner` and `ctx`."""
        out, err = io.StringIO(), io.StringIO()
        with unittest.mock.patch("sys.stdout", out), unittest.mock.patch("sys.stderr", err):
            rc = pcc.main(argv, runner=runner, ctx=ctx)
        return rc, out.getvalue(), err.getvalue()

    # -- --grammar-substrate-status -------------------------------------------

    def test_a_lane_that_never_generated_the_grammar_is_generated_not_skipped(self):
        ctx = self._ctx(_generator_stub_succeeds(self.tmp, self.root, self.marker))
        runner, calls = recording_runner()
        rc, out, _ = self._run_main(["--grammar-substrate-status"], runner, ctx)
        self.assertEqual(rc, 0, out)
        self.assertEqual(len(_marker_lines(self.marker)), 1)
        self.assertEqual(len(calls), 1)

    def test_a_grammar_that_cannot_be_made_current_is_never_probed(self):
        ctx = self._ctx(_generator_stub_fails(self.tmp, self.marker), parser_c=True)
        runner, calls = recording_runner()
        rc, out, _ = self._run_main(["--grammar-substrate-status"], runner, ctx)
        self.assertEqual(rc, 75)
        self.assertEqual(len(out.splitlines()), 1, out)
        self.assertTrue(out.startswith("grammar substrate: unusable: "), out)
        self.assertIn("tree-sitter CLI not found", out)
        self.assertEqual(calls, [])

    def test_the_never_generated_hint_names_the_generator(self):
        ctx = self._ctx(None)
        runner, _ = recording_runner()
        rc, out, _ = self._run_main(["--grammar-substrate-status"], runner, ctx)
        self.assertEqual(rc, 75)
        self.assertIn("tree-sitter-generate.sh", out)

    # -- probe-set mode ---------------------------------------------------------

    def test_the_generator_runs_once_before_the_first_probe(self):
        ctx = self._ctx(_generator_stub_succeeds(self.tmp, self.root, self.marker))
        inner, _ = recording_runner()
        generated_when_probed = []

        def runner(probe, probe_ctx, timeout=None):
            generated_when_probed.append(os.path.exists(self.marker))
            return inner(probe, probe_ctx, timeout)

        path = self._probe_set(_grammar_probe("first"), _grammar_probe("second"))
        self._run_main([path], runner, ctx)
        self.assertEqual(generated_when_probed, [True, True])
        self.assertEqual(len(_marker_lines(self.marker)), 1)

    def test_a_failed_generation_refuses_before_any_probe_runs(self):
        ctx = self._ctx(_generator_stub_fails(self.tmp, self.marker))
        runner, calls = recording_runner()
        rc, out, err = self._run_main(["--json", self._probe_set(_grammar_probe())],
                                      runner, ctx)
        self.assertEqual(rc, 70)
        self.assertIn("tree-sitter CLI not found", err)
        self.assertEqual(out, "")
        self.assertEqual(calls, [])

    def test_a_set_without_a_grammar_probe_never_runs_the_generator(self):
        ctx = self._ctx(_generator_stub_fails(self.tmp, self.marker))
        runner, calls = recording_runner(pcc.ProbeRun(1, "", ""))
        rc, out, _ = self._run_main([self._probe_set(_check_probe())], runner, ctx)
        self.assertEqual(rc, 0, out)
        self.assertEqual(len(calls), 1)
        self.assertFalse(os.path.exists(self.marker))


if __name__ == "__main__":
    unittest.main()
