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

import os
import shlex
import tempfile
import unittest

from _pcc_test_support import (
    pcc,
    _write_ts_stub,
    make_fixture_repo,
    probe_context,
)


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


class TestEnsureGrammarCurrent(unittest.TestCase):
    """ensure_grammar_current(ctx) runs the context's generator and reports (ok, reason)."""

    def setUp(self):
        self.tmp = self.enterContext(tempfile.TemporaryDirectory(prefix="pcc_generate_"))
        self.root = make_fixture_repo(os.path.join(self.tmp, "repo"))
        self.src = os.path.join(self.root, "tree-sitter-reify", "src")
        self.marker = os.path.join(self.tmp, "generator_runs")

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


if __name__ == "__main__":
    unittest.main()
