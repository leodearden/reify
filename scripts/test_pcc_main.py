#!/usr/bin/env python3
"""
test_pcc_main.py — main(argv) end to end in scripts/prd-capability-check.py: CLI
integration, --grammar-substrate-status, value-probe reporting and the
skip-guarded real e2e.  Shared loader and helpers: scripts/_pcc_test_support.py;
runner: tests/infra/test_prd_capability_check.sh.
"""

import io
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
import unittest.mock
from typing import Any

from _pcc_test_support import (
    pcc,
    _CACHE_DENIED_STDERR,
    _EXAMPLE_PROBE_SET,
    _HARNESS_PATH,
    _REPO_ROOT,
    _VALUE_CELLS_FIXTURE,
    _VALUE_CELLS_STDOUT,
    _patch_parser_c,
    _require_grammar_substrate,
    _ts_stub_cache_denied,
    _ts_stub_clean,
    _ts_stub_not_executable,
)


# ---------------------------------------------------------------------------
# step-13 (RED): main(argv) integration + skip-guarded real e2e
# ---------------------------------------------------------------------------

_REIFY_RELEASE = os.path.join(_REPO_ROOT, "target", "release", "reify")
_REIFY_DEBUG = os.path.join(_REPO_ROOT, "target", "debug", "reify")
_REIFY_BUILT = os.path.isfile(_REIFY_RELEASE) or os.path.isfile(_REIFY_DEBUG)


class _MainHarness:
    """Shared plumbing for driving main() hermetically: capture, and stub runs.

    A mixin rather than a TestCase whose methods other classes borrow: reaching
    into another TestCase for helpers couples one test class's lifecycle to
    another's privates, and constructing a bare TestCase() to do it is a
    construction Python only tolerates by accident.
    """

    def _run_main_capturing(self, argv, runner=None):
        """Run main() with stdout/stderr captured.

        Returns (exit_code, stdout_text, stderr_text).
        If runner is not None, patches pcc.run_probe with it.
        """
        buf_out = io.StringIO()
        buf_err = io.StringIO()
        with unittest.mock.patch("sys.stdout", buf_out), \
             unittest.mock.patch("sys.stderr", buf_err):
            if runner is not None:
                with unittest.mock.patch.object(pcc, "run_probe", side_effect=runner):
                    rc = pcc.main(argv)
            else:
                rc = pcc.main(argv)
        return rc, buf_out.getvalue(), buf_err.getvalue()

    # stdout the value probe in example-probe-set.json expects to read.
    _VALUE_PASS_STDOUT = "ValueCleanEvalCells.damping_ratio = 0.018"

    # The run that makes each kind in example-probe-set.json PASS:
    #   grammar: expected present → exit 0 (PRESENT)
    #   check:   expected present, match {exit_code: 1} → exit 1 (match → PRESENT)
    #   ir:      expected absent,  match {stderr_contains: 'EvalError'} → exit 0 (ABSENT)
    #   value:   expected present, match {stdout_value: damping_ratio in
    #            [0.01, 0.03]} → exit 0 AND an in-range capture (PRESENT)
    _PASSING_RUNS = {
        "grammar": (0, "", ""),
        "check":   (1, "", "rejection: bad arg"),
        "ir":      (0, "a = 0.01 m", ""),
        "value":   (0, _VALUE_PASS_STDOUT, ""),
    }

    def _make_runner(self, by_kind):
        """Stub runner dispatching by probe.probe_kind, PASSing by default.

        A test names only the kinds it actually steers; everything else falls
        back to _PASSING_RUNS.  Otherwise every test that pins one kind's
        rendering would have to enumerate all the others, and adding a kind to
        the example probe set would mean editing all of them in lockstep.

        A key that is not a probe kind is a typo, and a silent one would be the
        worst kind: the steer would go nowhere and the test would assert against
        an all-PASSing run while believing it had forced a failure.
        """
        unknown = sorted(set(by_kind) - set(self._PASSING_RUNS))
        if unknown:
            raise KeyError(
                f"not a probe kind: {unknown}; steerable kinds are "
                f"{sorted(self._PASSING_RUNS)}"
            )
        runs = dict(self._PASSING_RUNS, **by_kind)

        def runner(probe: Any) -> Any:
            exit_code, stdout, stderr = runs[probe.probe_kind]
            return pcc.ProbeRun(exit_code=exit_code, stdout=stdout, stderr=stderr)
        return runner

    def _all_pass_runner(self):
        """Runner that causes every probe kind in example-probe-set.json to PASS."""
        return self._make_runner({})

    def _check_fail_runner(self):
        """Runner that makes the check probe FAIL (reify silent-accept)."""
        # exit 0 → no rejection → ABSENT, against an expected present.
        return self._make_runner({"check": (0, "All constraints satisfied.", "")})


class TestMain(_MainHarness, unittest.TestCase):
    """Tests for main(argv) integration — hermetic + skip-guarded real e2e.

    Most tests FAIL until step-14 implements main() properly (currently a stub
    that returns 64 for any valid probe-set path).
    """

    # ── the stub runner's own contract ───────────────────────────────────────

    def test_make_runner_rejects_a_kind_that_does_not_exist(self):
        """A mistyped steer must be loud, or the test it steers proves nothing.

        Defaulting unnamed kinds to _PASSING_RUNS is what keeps each test from
        enumerating all four — but it also means a typo'd key silently steers
        nothing, leaving the real probe PASSing while the test believes it has
        forced a failure and asserts against the wrong run.
        """
        with self.assertRaises(KeyError) as ctx:
            self._make_runner({"grammer": (1, "", "boom")})
        self.assertIn("grammer", str(ctx.exception))

        # The kinds that ARE steerable are exactly the harness's known kinds,
        # which are exactly the probe kinds the loader admits.
        self.assertEqual(set(self._PASSING_RUNS), set(pcc._VALID_PROBE_KINDS))

    # ── arg / IO errors → 64 ─────────────────────────────────────────────────

    def test_main_no_args_exits_64(self):
        """main([]) → 64 (usage error, argparse)."""
        rc, _, _ = self._run_main_capturing([])
        self.assertEqual(rc, 64)

    def test_main_missing_file_exits_64(self):
        """main(["/nonexistent/probe-set.json"]) → 64 (IO error reading file)."""
        rc, _, _ = self._run_main_capturing(["/nonexistent/probe-set.json"])
        self.assertEqual(rc, 64)

    def test_main_bad_json_exits_64(self):
        """main([<file with invalid JSON>]) → 64 (parse error)."""
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            f.write("not json at all")
            tmp = f.name
        try:
            rc, _, _ = self._run_main_capturing([tmp])
        finally:
            os.unlink(tmp)
        self.assertEqual(rc, 64)

    def test_main_empty_probe_set_exits_64(self):
        """main() with an empty 'probes' list → exit 64 (empty gate masks CI failures)."""
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            f.write(json.dumps({"probes": []}))
            tmp = f.name
        try:
            rc, _, err = self._run_main_capturing([tmp])
        finally:
            os.unlink(tmp)
        self.assertEqual(rc, 64, "empty probe set must return 64 (EX_USAGE)")
        self.assertIn("no probes", err, "error message must mention 'no probes'")

    # ── --grammar-substrate-status (5894 step-5) ──────────────────────────────
    #
    # Lets a shell caller ask "can a grammar probe run here?" WITHOUT running the
    # gate, so it can SKIP rather than report the sandbox's denial as a FAIL.
    # Driven by TREE_SITTER_BIN stubs, never ambient state — see
    # TestGrammarSubstrateUsable's docstring for why that is load-bearing.

    def _stub_dir(self):
        """Per-test tempdir for TREE_SITTER_BIN stubs, removed on test teardown.

        Uses addCleanup rather than setUp/tearDown so the other ~20 TestMain
        tests, which need no stubs, do not pay for a tempdir.
        """
        tmpdir = tempfile.mkdtemp(prefix="prd_gate_substrate_cli_")
        self.addCleanup(shutil.rmtree, tmpdir, ignore_errors=True)
        return tmpdir

    def _run_status_with_stub(self, stub):
        """Run main(["--grammar-substrate-status"]) against a TREE_SITTER_BIN stub.

        Pins parser.c present as well as the stub.  The mode checks the generated
        grammar before it probes, so without that patch every case below would
        silently depend on whether this lane happened to run `tree-sitter
        generate`, and would flip to 75 on one that had not — the ambient
        conditioning TestGrammarSubstrateUsable's docstring rules out.
        """
        with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}), \
             _patch_parser_c(True):
            return self._run_main_capturing(["--grammar-substrate-status"])

    def test_grammar_substrate_status_usable_returns_0(self):
        """(a) Usable substrate → exit 0 and a short 'usable' line on stdout."""
        rc, out, _ = self._run_status_with_stub(_ts_stub_clean(self._stub_dir()))
        self.assertEqual(rc, 0, "a usable grammar substrate must exit 0")
        self.assertIn("usable", out.lower(), "the mode must state its finding on stdout")

    def test_grammar_substrate_status_denied_returns_75(self):
        """(b) Cache denial → exit 75 (EX_TEMPFAIL) with the reason on stdout.

        75 rather than 1 or 70: those already mean "≥1 FAIL verdict" and
        "HARNESS_ERROR", so overloading either would leave a shell caller unable
        to tell "substrate unavailable, please SKIP" from a real gate result.
        """
        rc, out, _ = self._run_status_with_stub(_ts_stub_cache_denied(self._stub_dir()))
        self.assertEqual(
            rc, 75,
            "an unusable grammar substrate must exit 75 (EX_TEMPFAIL), distinct "
            "from 1 (FAIL) and 70 (HARNESS_ERROR)",
        )
        # The reason is what the shell prints verbatim as its SKIP line, so it
        # must name the subsystem — a bare "permission denied" is exactly what
        # made the original failure cost several probes to attribute.
        lowered = out.lower()
        self.assertIn("tree-sitter", lowered)
        self.assertIn("cache", lowered)

    def test_grammar_substrate_status_needs_no_probe_set(self):
        """(c) The flag stands alone — the required-positional check is never reached.

        Asserts the usage *message*, not the exit code: (a) already pins rc == 0
        on this identical invocation, so assertNotEqual(rc, 64) would be strictly
        weaker than its sibling and would happily accept a 70.
        """
        rc, _, err = self._run_status_with_stub(_ts_stub_clean(self._stub_dir()))
        self.assertNotIn(
            "PROBE_SET_JSON is required", err,
            "--grammar-substrate-status must satisfy the positional requirement "
            "on its own",
        )
        self.assertEqual(
            err.strip(), "",
            "a flag-only invocation is not an error and must say nothing on stderr",
        )

    def test_grammar_substrate_status_ignores_the_json_flag(self):
        """--json is accepted-and-ignored by the status mode, not a usage error.

        There is no result set to serialise; making it an error would break a
        caller that passes --json unconditionally.
        """
        stub = _ts_stub_cache_denied(self._stub_dir())
        with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}), \
             _patch_parser_c(True):
            rc, out, _ = self._run_main_capturing(
                ["--json", "--grammar-substrate-status"]
            )
        self.assertEqual(rc, 75, "--json must not perturb the mode's finding")
        self.assertIn("tree-sitter", out.lower(), "the reason is still emitted")
        with self.assertRaises(
            json.JSONDecodeError,
            msg="the status mode emits plain text; if it ever starts emitting "
                "JSON under --json, that is a contract change to document",
        ):
            json.loads(out)

    def test_grammar_substrate_status_ignores_a_positional_probe_set(self):
        """A PROBE_SET_JSON alongside the flag is discarded — not read, not evaluated.

        The bogus path is the load-bearing part: without mode precedence the
        unreadable file returns 64, so this pins that the mode short-circuits
        ahead of the probe-set IO as well as ahead of evaluation.
        """
        sentinel = unittest.mock.Mock(side_effect=AssertionError(
            "--grammar-substrate-status must not evaluate a probe set"
        ))
        stub = _ts_stub_clean(self._stub_dir())
        with unittest.mock.patch.object(pcc, "evaluate", sentinel), \
             unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}), \
             _patch_parser_c(True):
            rc, out, _ = self._run_main_capturing(
                ["--grammar-substrate-status", "/nonexistent/probe-set.json"]
            )
        self.assertEqual(
            rc, 0,
            "the substrate finding must win over the positional; a 64 here means "
            "the ignored probe set was read after all",
        )
        self.assertIn("usable", out.lower())
        sentinel.assert_not_called()

    def test_grammar_substrate_status_runs_no_probes(self):
        """The status mode must never invoke the probe runner.

        It reports on the substrate, so it cannot be allowed to perturb — or be
        perturbed by — any probe verdict.
        """
        sentinel = unittest.mock.Mock(side_effect=AssertionError(
            "--grammar-substrate-status must not evaluate probes"
        ))
        with unittest.mock.patch.object(pcc, "evaluate", sentinel):
            rc, _, _ = self._run_status_with_stub(_ts_stub_clean(self._stub_dir()))
        self.assertEqual(rc, 0)
        sentinel.assert_not_called()

    def test_grammar_substrate_status_ungenerated_grammar_returns_75(self):
        """No generated parser.c → exit 75 naming it, and no probe is spawned.

        grammar_substrate_usable() reports an ungenerated grammar USABLE on
        purpose — the load failure it produces carries no permission marker, and
        its narrowness contract keeps a missing or corrupt grammar loud rather
        than skippable.  So without a precondition check of its own, this mode
        would answer "usable" to a shell caller that then still collects exit 70
        from the probe run: the one answer the mode exists to spare it.
        """
        probe = unittest.mock.Mock(return_value=(True, ""))
        with _patch_parser_c(False), \
             unittest.mock.patch.object(pcc, "grammar_substrate_usable", probe):
            rc, out, _ = self._run_main_capturing(["--grammar-substrate-status"])
        self.assertEqual(
            rc, 75,
            "an ungenerated grammar must reach the caller as the same SKIP "
            "contract as any other unrunnable substrate, not as exit 0",
        )
        self.assertIn(
            "parser.c", out,
            "the reason must name the missing file; 'grammar unusable' with no "
            "path is the un-attributable message this task exists to remove",
        )
        self.assertEqual(
            probe.call_count, 0,
            "the isfile() precondition must short-circuit ahead of the probe: a "
            "lane with no grammar should pay no tree-sitter subprocess",
        )

    def test_grammar_substrate_status_generated_grammar_still_probes(self):
        """The precondition gates, it does not replace: a present parser.c still probes.

        Guards the inverse mistake — answering the whole question from isfile()
        would reinstate the path-existence guard whose blindness to a denied
        cache is the reported bug.
        """
        probe = unittest.mock.Mock(return_value=(True, ""))
        with _patch_parser_c(True), \
             unittest.mock.patch.object(pcc, "grammar_substrate_usable", probe):
            rc, _, _ = self._run_main_capturing(["--grammar-substrate-status"])
        self.assertEqual(rc, 0)
        self.assertEqual(
            probe.call_count, 1,
            "isfile(parser.c) says the grammar was GENERATED, never that "
            "tree-sitter can load it; the behavioural probe must still run",
        )

    def test_grammar_substrate_status_non_executable_cli_returns_75(self):
        """A present-but-unlaunchable tree-sitter CLI → exit 75, reason on stdout.

        Must not arrive as a traceback and exit 1 — indistinguishable from the
        harness itself being broken.
        """
        stub = _ts_stub_not_executable(self._stub_dir())
        self.assertTrue(
            os.path.isfile(stub),
            "precondition: the stub must EXIST, else this is the missing-CLI case",
        )
        rc, out, _ = self._run_status_with_stub(stub)
        self.assertEqual(
            rc, 75,
            "an unlaunchable tree-sitter CLI must exit 75 (EX_TEMPFAIL), like "
            "every other unusable-substrate cause",
        )
        self.assertIn("tree-sitter", out.lower(), "the reason must name the subsystem")

    # ── (d) regression guards for the argparse change ─────────────────────────

    def test_no_flag_and_no_probe_set_still_exits_64_with_message(self):
        """Making the positional optional must NOT silently accept an empty argv.

        test_main_no_args_exits_64 pins the code; this pins the message, which is
        the part an nargs="?" change can quietly drop.
        """
        rc, _, err = self._run_main_capturing([])
        self.assertEqual(rc, 64, "no flag and no probe set is still a usage error")
        self.assertTrue(err.strip(), "the usage error must explain itself on stderr")

    def test_probe_set_without_flag_is_unaffected(self):
        """A normal probe-set invocation behaves exactly as before the flag existed."""
        rc, out, _ = self._run_main_capturing(
            [str(_EXAMPLE_PROBE_SET)], runner=self._all_pass_runner()
        )
        self.assertEqual(rc, 0)
        self.assertIn(pcc.PASS, out)

    def test_bad_probe_set_without_flag_still_exits_64(self):
        """A missing probe set still returns 64, not the new 75."""
        rc, _, _ = self._run_main_capturing(["/nonexistent/probe-set.json"])
        self.assertEqual(rc, 64)

    # ── hermetic: exit code matches harness_exit_code ─────────────────────────

    def test_main_all_pass_returns_0(self):
        """main() over probe set with all-PASS stubs → exit code 0."""
        rc, _, _ = self._run_main_capturing(
            [str(_EXAMPLE_PROBE_SET)], runner=self._all_pass_runner()
        )
        self.assertEqual(rc, 0)

    def test_main_with_fail_returns_1(self):
        """main() with a FAIL result (check silent-accept) → exit code 1."""
        rc, _, _ = self._run_main_capturing(
            [str(_EXAMPLE_PROBE_SET)], runner=self._check_fail_runner()
        )
        self.assertEqual(rc, 1)

    # ── human output: mandatory evidence per probe ────────────────────────────

    def test_main_output_contains_verdict(self):
        """Human output contains the verdict string for each probe."""
        rc, out, _ = self._run_main_capturing(
            [str(_EXAMPLE_PROBE_SET)], runner=self._all_pass_runner()
        )
        self.assertIn(pcc.PASS, out, "output must contain 'PASS' verdict")

    def test_main_output_contains_command(self):
        """Human output contains the probe command (reify or tree-sitter)."""
        rc, out, _ = self._run_main_capturing(
            [str(_EXAMPLE_PROBE_SET)], runner=self._all_pass_runner()
        )
        has_cmd = "reify" in out or "tree-sitter" in out
        self.assertTrue(has_cmd, "output must show the probe command")

    def test_main_output_contains_exit_code_evidence(self):
        """Human output contains the captured exit code for each probe."""
        rc, out, _ = self._run_main_capturing(
            [str(_EXAMPLE_PROBE_SET)], runner=self._all_pass_runner()
        )
        # Exit codes 0 and 1 appear in stdout evidence
        self.assertTrue("0" in out or "1" in out, "output must include exit codes")

    def test_main_output_contains_capability(self):
        """Human output contains each probe's capability name."""
        rc, out, _ = self._run_main_capturing(
            [str(_EXAMPLE_PROBE_SET)], runner=self._all_pass_runner()
        )
        self.assertIn("arrow-type", out, "output must include probe capability names")

    # ── diagnosability of HARNESS_ERROR text output (5894 step-7) ─────────────
    #
    # The 200-char preview truncates away the one thing a HARNESS_ERROR reader
    # needs: WHAT was denied.  The measured signature is 253 chars and the cut
    # lands mid-path, so the reader sees a denial naming nothing — which is why
    # the original failure cost several probes to attribute.

    def _cache_denial_runner(self):
        """Runner whose grammar probe reproduces the measured cache denial.

        exit 1 + "Failed to load language" is what observe() classifies as
        _HARNESS_ERROR, so this exercises the real rendering path.
        """
        return self._make_runner({
            "grammar": (1, "", _CACHE_DENIED_STDERR),
        })

    def test_harness_error_stderr_shows_the_denied_path(self):
        """The HARNESS_ERROR budget is wide enough to carry the denied lock path.

        Not "full" — the sibling test_harness_error_stderr_is_capped_not_unbounded
        pins that a cap exists.  What matters is that the cap sits far above the
        signature the operator needs, which the 200-char preview did not.
        """
        rc, out, _ = self._run_main_capturing(
            [str(_EXAMPLE_PROBE_SET)], runner=self._cache_denial_runner()
        )
        self.assertEqual(rc, 70, "precondition: the grammar probe is a HARNESS_ERROR")
        self.assertIn(
            "/home/leo/.cache/tree-sitter/lock/reify-69604127a681544d.lock", out,
            "the 200-char preview cuts the lock path mid-token; a HARNESS_ERROR "
            "must show the operator WHAT was denied",
        )

    def test_harness_error_cache_denial_emits_actionable_hint(self):
        """A recognised cache denial adds a hint naming the cause and the next step."""
        rc, out, _ = self._run_main_capturing(
            [str(_EXAMPLE_PROBE_SET)], runner=self._cache_denial_runner()
        )
        lowered = out.lower()
        self.assertIn("tree-sitter", lowered)
        self.assertIn("cache", lowered)
        self.assertIn(
            "--grammar-substrate-status", out,
            "the hint must point at the mode that turns this into a clean SKIP",
        )

    def _assert_no_cache_denial_hint(self, out):
        """Assert the renderer printed no cache-denial hint for this result."""
        self.assertNotIn(
            "--grammar-substrate-status", out,
            "the hint must not be offered for a HARNESS_ERROR that is not a "
            "cache denial — pointing an operator at the substrate mode "
            "mis-attributes the failure this change exists to attribute",
        )
        self.assertNotIn("cache/lock", out)

    def test_missing_binary_harness_error_emits_no_cache_denial_hint(self):
        """A missing-binary HARNESS_ERROR is still 70, but carries no cache hint.

        The positive hint test alone passes with the guard widened, inverted or
        dropped — leaving the renderer calling a missing CLI a landlock denial.
        """
        runner = self._make_runner({
            "grammar": (127, "", pcc._BINARY_NOT_FOUND_SENTINEL +
                        ": [Errno 2] No such file or directory: 'tree-sitter'"),
        })
        rc, out, _ = self._run_main_capturing([str(_EXAMPLE_PROBE_SET)], runner=runner)
        self.assertEqual(rc, 70, "precondition: the grammar probe is a HARNESS_ERROR")
        self._assert_no_cache_denial_hint(out)

    def test_non_permission_load_failure_emits_no_cache_denial_hint(self):
        """A load failure with no permission indicator gets no cache hint either.

        The predicate's other narrowness half, through the renderer: a missing or
        corrupt grammar must not be written off as an environmental skip.
        """
        runner = self._make_runner({
            "grammar": (1, "", "Error: Failed to load language for path \"x.ri\"\n"
                               "Caused by: No language found for path\n"),
        })
        rc, out, _ = self._run_main_capturing([str(_EXAMPLE_PROBE_SET)], runner=runner)
        self.assertEqual(rc, 70, "precondition: the grammar probe is a HARNESS_ERROR")
        self._assert_no_cache_denial_hint(out)

    def test_non_harness_error_verdict_emits_no_cache_denial_hint(self):
        """A PASSing probe whose stderr merely LOOKS denied gets no hint.

        The predicate reads stderr alone, so a check/ir probe that quoted a load
        failure and a permission denial in its own output would print a
        tree-sitter cache/landlock hint under a PASS line — mis-attributing a
        healthy probe to the sandbox, the inverse of what the hint is for.  Only
        a HARNESS_ERROR means "could not run", which is what the hint explains.
        """
        runner = self._make_runner({
            # exit 1 satisfies the check probe's match → PRESENT → PASS, while
            # the stderr carries the full denial signature.
            "check":   (1, "", _CACHE_DENIED_STDERR),
        })
        rc, out, _ = self._run_main_capturing([str(_EXAMPLE_PROBE_SET)], runner=runner)
        self.assertEqual(rc, 0, "precondition: every probe must have PASSed")
        self._assert_no_cache_denial_hint(out)

    def test_harness_error_stderr_is_capped_not_unbounded(self):
        """The HARNESS_ERROR budget is large, but it is still a budget.

        Unbounded, a `reify eval` backtrace would bury the gate log in the one
        path meant to clarify it.  The measured denial (253 chars) fits ~16x over.
        """
        tail = "TAIL_BEYOND_THE_HARNESS_ERROR_CAP"
        flood = _CACHE_DENIED_STDERR + ("z" * pcc._HARNESS_ERROR_STDERR_CAP) + tail
        runner = self._make_runner({
            "grammar": (1, "", flood),
        })
        rc, out, _ = self._run_main_capturing([str(_EXAMPLE_PROBE_SET)], runner=runner)
        self.assertEqual(rc, 70, "precondition: the grammar probe is a HARNESS_ERROR")
        self.assertIn(
            "/home/leo/.cache/tree-sitter/lock/reify-69604127a681544d.lock", out,
            "the cap must sit far above the signature the operator needs",
        )
        self.assertNotIn(
            tail, out,
            "a HARNESS_ERROR's stderr must be bounded by "
            "_HARNESS_ERROR_STDERR_CAP, not emitted unbounded",
        )

    def test_json_output_keeps_full_harness_error_stderr(self):
        """--json is unaffected by the text renderer's cap.

        Pinned so moving the text cap cannot silently truncate the JSON contract.
        """
        tail = "TAIL_BEYOND_THE_HARNESS_ERROR_CAP"
        flood = _CACHE_DENIED_STDERR + ("z" * pcc._HARNESS_ERROR_STDERR_CAP) + tail
        runner = self._make_runner({
            "grammar": (1, "", flood),
        })
        rc, out, _ = self._run_main_capturing(
            ["--json", str(_EXAMPLE_PROBE_SET)], runner=runner
        )
        self.assertEqual(rc, 70, "precondition: the grammar probe is a HARNESS_ERROR")
        payload = json.loads(out)
        grammar = [r for r in payload["results"] if r["probe_kind"] == "grammar"][0]
        self.assertEqual(
            grammar["stderr"], flood,
            "--json must carry the probe's stderr verbatim, uncapped",
        )

    def test_non_harness_error_stderr_is_still_previewed(self):
        """PASS/FAIL results keep the 200-char preview so ordinary output stays compact.

        The wide budget is scoped to HARNESS_ERROR; widening it to every verdict
        would bury a normal run in probe stderr.
        """
        long_tail = "TAIL_BEYOND_200_CHARS"
        runner = self._make_runner({
            "check":   (1, "", "rejection: " + ("x" * 250) + long_tail),
        })
        rc, out, _ = self._run_main_capturing([str(_EXAMPLE_PROBE_SET)], runner=runner)
        self.assertEqual(rc, 0, "precondition: no HARNESS_ERROR in this run")
        self.assertNotIn(
            long_tail, out,
            "a non-HARNESS_ERROR result must still be truncated at 200 chars",
        )

    # ── --json output ─────────────────────────────────────────────────────────

    def test_main_json_is_parseable(self):
        """main() --json emits parseable JSON to stdout."""
        rc, out, _ = self._run_main_capturing(
            ["--json", str(_EXAMPLE_PROBE_SET)], runner=self._all_pass_runner()
        )
        try:
            json.loads(out)
        except json.JSONDecodeError as e:
            self.fail(f"--json output is not valid JSON: {e}\nGot: {out!r}")

    def test_main_json_has_required_fields(self):
        """--json results carry capability/probe_kind/verdict/command/exit_code/stdout/stderr."""
        rc, out, _ = self._run_main_capturing(
            ["--json", str(_EXAMPLE_PROBE_SET)], runner=self._all_pass_runner()
        )
        data = json.loads(out)
        # Accept either a list or {"results": [...]}
        items = data if isinstance(data, list) else data.get("results", [])
        self.assertGreater(len(items), 0, "--json must include at least one result")
        first = items[0]
        for fld in ("capability", "probe_kind", "verdict", "command",
                    "exit_code", "stdout", "stderr"):
            self.assertIn(fld, first, f"--json result must include '{fld}'")

    def test_main_json_verdict_is_string(self):
        """--json result.verdict is a string."""
        rc, out, _ = self._run_main_capturing(
            ["--json", str(_EXAMPLE_PROBE_SET)], runner=self._all_pass_runner()
        )
        data = json.loads(out)
        items = data if isinstance(data, list) else data.get("results", [])
        self.assertIsInstance(items[0]["verdict"], str)

    # ── skip-guarded real e2e (reify binary) ──────────────────────────────────

    @unittest.skipUnless(_REIFY_BUILT, "reify binary not built; skip real check e2e")
    def test_e2e_revolute_silent_accept_is_fail(self):
        """Real reify check: silent-accept (§3 4575) probe → FAIL (reify exits 0, stable)."""
        probe_json = json.dumps({"probes": [{
            "capability": "arg-vs-param rejection (4575 — should FAIL)",
            "probe_kind": "check",
            "fixture": "tests/prd-gate/fixtures/revolute_silent_accept.ri",
            "expected": {"observation": "present", "match": {"exit_code": 1}},
        }]})
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            f.write(probe_json)
            tmp = f.name
        try:
            rc, out, _ = self._run_main_capturing([tmp])
        finally:
            os.unlink(tmp)
        self.assertEqual(rc, 1, "revolute silent-accept → FAIL (exit 1)")
        self.assertIn(pcc.FAIL, out)

    # ── skip-guarded real e2e (tree-sitter grammar) ───────────────────────────

    def test_e2e_arrow_type_grammar_is_fail(self):
        """Real tree-sitter parse: arrow_type.ri with expected present → FAIL (exit 1, stable).

        Guarded in the body rather than by @skipUnless so the substrate probe is
        spent only when this test runs — see _grammar_e2e_status().
        """
        _require_grammar_substrate(self)
        probe_json = json.dumps({"probes": [{
            "capability": "arrow-type grammar (3979 — should FAIL)",
            "probe_kind": "grammar",
            "fixture": "tests/prd-gate/fixtures/arrow_type.ri",
            "expected": {"observation": "present", "match": {}},
        }]})
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            f.write(probe_json)
            tmp = f.name
        try:
            rc, out, _ = self._run_main_capturing([tmp])
        finally:
            os.unlink(tmp)
        # tree-sitter exits 1 for arrow_type.ri (grammar rejects it) → ABSENT
        # expected present → FAIL
        self.assertEqual(rc, 1, "arrow_type.ri with expected present → FAIL (exit 1)")
        self.assertIn(pcc.FAIL, out)


# ---------------------------------------------------------------------------
# #6876 step-09 (RED): value-probe result legibility + the real-binary e2e
# ---------------------------------------------------------------------------

# The constraint the committed example holds the value-cells fixture's first cell to.
_DAMPING_SPEC = {
    "pattern": r"ValueCleanEvalCells\.damping_ratio = ([-+0-9.eE]+)",
    "min": 0.01,
    "max": 0.03,
}


class TestValueProbeReporting(_MainHarness, unittest.TestCase):
    """A value FAIL must say WHICH defect it found.

    "the pattern matched and captured 0, which is below min 0.01" and "the
    pattern never matched" are different defects — a wrong value versus a
    renamed field or a mis-aimed probe — and the renderer's 200-char stdout
    preview cannot tell them apart on real multi-cell output.  So the evidence
    is computed once, alongside the verdict, and carried on the Result.

    The e2e half (against the real binary) is the end-to-end proof of the whole
    chain; its VERDICTS are already green after step-08, and it is the evidence
    surface here plus the committed example's fourth row that make this RED.
    """


    # ── hermetic plumbing ─────────────────────────────────────────────────────

    def _probe_set_text(self, value_spec=None):
        """One probe of every kind, so scoping can be asserted in one run."""
        return json.dumps({"probes": [
            {
                "capability": "grammar row",
                "probe_kind": "grammar",
                "fixture": "tests/prd-gate/fixtures/arrow_type.ri",
                "expected": {"observation": "present", "match": {}},
            },
            {
                "capability": "check row",
                "probe_kind": "check",
                "fixture": "tests/prd-gate/fixtures/revolute_silent_accept.ri",
                "expected": {"observation": "present", "match": {"exit_code": 1}},
            },
            {
                "capability": "ir row",
                "probe_kind": "ir",
                "fixture": "tests/prd-gate/fixtures/ir_clean_eval.ri",
                "expected": {
                    "observation": "absent",
                    "match": {"stderr_contains": "EvalError"},
                },
            },
            {
                "capability": "value row",
                "probe_kind": "value",
                "fixture": _VALUE_CELLS_FIXTURE,
                "expected": {
                    "observation": "present",
                    "match": {
                        "stdout_value": value_spec or dict(_DAMPING_SPEC)
                    },
                },
            },
        ]})

    def _run_main(self, argv_prefix=(), value_spec=None, value_stdout=None,
                  value_exit=0):
        # Only the value row is steered; the other three keep _PASSING_RUNS, so
        # a scoping assertion reads against rows that are otherwise ordinary.
        runner = self._make_runner({
            "value": (value_exit,
                      _VALUE_CELLS_STDOUT if value_stdout is None else value_stdout,
                      ""),
        })
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as fh:
            fh.write(self._probe_set_text(value_spec))
            tmp = fh.name
        try:
            return self._run_main_capturing(list(argv_prefix) + [tmp], runner=runner)
        finally:
            os.unlink(tmp)

    def _rows(self, out):
        """Split rendered output into one block per probe.

        Rows are delimited by their `[VERDICT]` header, not by blank lines: a
        multi-line stdout preview (the real `reify eval` shape) puts blank lines
        INSIDE a row.
        """
        return re.split(r"\n(?=\[)", out.strip())

    def _value_lines(self, text):
        return [
            line for line in text.splitlines() if line.strip().startswith("value:")
        ]

    def _json_records(self, out):
        return json.loads(out)["results"]

    # ── the evidence is computed once, with the verdict ───────────────────────

    def test_evaluate_populates_value_observation_only_for_value_kind(self):
        def runner(probe):
            return pcc.ProbeRun(exit_code=0, stdout=_VALUE_CELLS_STDOUT, stderr="")

        probes = pcc.load_probe_set(self._probe_set_text())
        by_kind = {p.probe_kind: pcc.evaluate(p, runner=runner) for p in probes}
        for kind in ("grammar", "check", "ir"):
            with self.subTest(kind=kind):
                self.assertIsNone(by_kind[kind].value_observation)
        self.assertIsNotNone(by_kind["value"].value_observation)

    def test_value_observation_is_the_reading_the_verdict_came_from(self):
        """SPOT: the explanation and the observation are ONE computation.

        Not merely "they agree" — the Result carries the very object
        observe_with_evidence() derived the observation from, so there is no
        second search of stdout that could drift out of step with the first.
        """
        for stdout in (_VALUE_CELLS_STDOUT, "nothing here\n",
                       "ValueCleanEvalCells.damping_ratio = 0\n"):
            with self.subTest(stdout=stdout):
                run = pcc.ProbeRun(exit_code=0, stdout=stdout, stderr="")
                probe = [
                    p for p in pcc.load_probe_set(self._probe_set_text())
                    if p.probe_kind == "value"
                ][0]
                result = pcc.evaluate(probe, runner=lambda _p: run)
                obs, reading = pcc.observe_with_evidence(
                    "value", run, {"stdout_value": _DAMPING_SPEC}
                )
                self.assertEqual(result.value_observation, reading)
                self.assertEqual(result.observation, obs)
                self.assertEqual(
                    obs, pcc._value_reading_observation(result.value_observation)
                )

    def test_value_observation_records_capture_and_failed_constraint(self):
        cases = [
            (_VALUE_CELLS_STDOUT, True, "0.018", None),
            ("ValueCleanEvalCells.damping_ratio = 0\n", False, "0", "min"),
            ("ValueCleanEvalCells.damping_ratio = 9.9\n", False, "9.9", "max"),
            ("nothing here\n", False, None, "pattern"),
        ]
        probe = [
            p for p in pcc.load_probe_set(self._probe_set_text())
            if p.probe_kind == "value"
        ][0]
        for stdout, satisfied, captured, failed in cases:
            with self.subTest(stdout=stdout):
                run = pcc.ProbeRun(exit_code=0, stdout=stdout, stderr="")
                obs = pcc.evaluate(probe, runner=lambda _p: run).value_observation
                self.assertEqual(obs.satisfied, satisfied)
                self.assertEqual(obs.captured, captured)
                self.assertEqual(obs.failed_constraint, failed)

    def test_non_finite_capture_names_the_finite_constraint(self):
        spec = {"pattern": r"undef_ratio = (\S+)", "finite": True}
        probes = pcc.load_probe_set(self._probe_set_text(value_spec=spec))
        probe = [p for p in probes if p.probe_kind == "value"][0]
        run = pcc.ProbeRun(exit_code=0, stdout=_VALUE_CELLS_STDOUT, stderr="")
        obs = pcc.evaluate(probe, runner=lambda _p: run).value_observation
        self.assertFalse(obs.satisfied)
        self.assertEqual(obs.captured, "undef")
        self.assertEqual(obs.failed_constraint, "finite")

    # ── text renderer ─────────────────────────────────────────────────────────

    def test_text_renderer_emits_exactly_one_value_line(self):
        _, out, _ = self._run_main()
        self.assertEqual(len(self._value_lines(out)), 1, out)

    def test_text_value_line_is_scoped_to_value_rows(self):
        """Scoped exactly as the existing `hint:` line is scoped to HARNESS_ERROR."""
        _, out, _ = self._run_main()
        rows = self._rows(out)
        self.assertEqual(len(rows), 4, out)
        for row in rows:
            is_value_row = "kind:      value" in row
            self.assertEqual(bool(self._value_lines(row)), is_value_row, row)

    def test_text_value_line_names_the_capture_on_pass(self):
        """The captured value is the evidence on PASS too, not only on FAIL."""
        rc, out, _ = self._run_main()
        self.assertEqual(rc, 0, out)
        line = self._value_lines(out)[0]
        self.assertIn("0.018", line)

    def test_text_value_line_names_the_violated_constraint_on_fail(self):
        rc, out, _ = self._run_main(
            value_stdout="ValueCleanEvalCells.damping_ratio = 0\n"
        )
        self.assertEqual(rc, 1, out)
        line = self._value_lines(out)[0]
        self.assertIn("0", line)
        self.assertIn("min", line)

    def test_text_value_line_distinguishes_a_pattern_miss(self):
        """A mis-aimed probe must not read as a wrong value.

        It is UNPROVABLE (rc 2), not FAIL: nothing was read, so there is no
        negative to report — but the evidence line still says WHY, which is the
        whole reason an INDETERMINATE value row carries a reading at all.
        """
        rc, out, _ = self._run_main(value_stdout="something else entirely\n")
        self.assertEqual(rc, 2, out)
        self.assertIn(f"[{pcc.UNPROVABLE}]", out)
        line = self._value_lines(out)[0]
        self.assertIn("pattern", line)

    def test_text_value_line_absent_when_exit_is_nonzero(self):
        """INDETERMINATE produced no value, so there is no capture to report."""
        rc, out, _ = self._run_main(value_exit=1)
        self.assertEqual(rc, 2, out)
        self.assertEqual(self._value_lines(out), [], out)

    # ── --json ────────────────────────────────────────────────────────────────

    def test_json_carries_value_observation_on_value_rows_only(self):
        _, out, _ = self._run_main(argv_prefix=["--json"])
        records = self._json_records(out)
        by_kind = {r["probe_kind"]: r for r in records}
        for kind in ("grammar", "check", "ir"):
            with self.subTest(kind=kind):
                self.assertNotIn(
                    "value_observation", by_kind[kind],
                    "the record shape of existing kinds must not change",
                )
        self.assertIn("value_observation", by_kind["value"])

    def test_json_value_observation_is_structured(self):
        _, out, _ = self._run_main(
            argv_prefix=["--json"],
            value_stdout="ValueCleanEvalCells.damping_ratio = 0\n",
        )
        record = {r["probe_kind"]: r for r in self._json_records(out)}["value"]
        self.assertEqual(
            record["value_observation"],
            {"satisfied": False, "captured": "0", "failed_constraint": "min"},
        )

    def test_json_value_observation_survives_an_unprovable_pattern_miss(self):
        """An UNPROVABLE value row still says WHY, which is the point of it.

        The reading is carried whenever stdout was READ, not only when the
        answer was PRESENT/ABSENT — otherwise a mis-aimed probe would report
        "unprovable" with nothing an operator could act on.
        """
        _, out, _ = self._run_main(
            argv_prefix=["--json"], value_stdout="something else entirely\n"
        )
        record = {r["probe_kind"]: r for r in self._json_records(out)}["value"]
        self.assertEqual(record["verdict"], pcc.UNPROVABLE)
        self.assertEqual(
            record["value_observation"],
            {"satisfied": False, "captured": None, "failed_constraint": "pattern"},
        )

    def test_json_value_observation_absent_when_nothing_was_read(self):
        """A non-zero exit produced no stdout to read, so there is no reading."""
        _, out, _ = self._run_main(argv_prefix=["--json"], value_exit=1)
        record = {r["probe_kind"]: r for r in self._json_records(out)}["value"]
        self.assertEqual(record["verdict"], pcc.UNPROVABLE)
        self.assertNotIn("value_observation", record)

    def test_json_value_observation_present_on_pass(self):
        _, out, _ = self._run_main(argv_prefix=["--json"])
        record = {r["probe_kind"]: r for r in self._json_records(out)}["value"]
        self.assertEqual(
            record["value_observation"],
            {"satisfied": True, "captured": "0.018", "failed_constraint": None},
        )

    # ── the committed worked example ──────────────────────────────────────────

    def test_committed_example_value_row_passes_under_stub(self):
        rc, out, _ = self._run_main_capturing(
            [str(_EXAMPLE_PROBE_SET)], runner=self._all_pass_runner()
        )
        self.assertEqual(rc, 0, out)
        value_rows = [
            row for row in self._rows(out) if "kind:      value" in row
        ]
        self.assertEqual(len(value_rows), 1, out)
        self.assertTrue(value_rows[0].startswith(f"[{pcc.PASS}]"), value_rows[0])

    def test_committed_example_value_row_points_at_the_fixture(self):
        with open(_EXAMPLE_PROBE_SET) as fh:
            probes = pcc.load_probe_set(fh.read())
        value_probes = [p for p in probes if p.probe_kind == "value"]
        self.assertEqual(len(value_probes), 1)
        self.assertEqual(value_probes[0].fixture, _VALUE_CELLS_FIXTURE)
        self.assertEqual(value_probes[0].expected["observation"], "present")

    # ── real-binary e2e over the committed fixture ────────────────────────────

    def _e2e(self, probe_kind, expected_observation, match):
        probe_json = json.dumps({"probes": [{
            "capability": f"#6876 e2e ({probe_kind})",
            "probe_kind": probe_kind,
            "fixture": _VALUE_CELLS_FIXTURE,
            "expected": {"observation": expected_observation, "match": match},
        }]})
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as fh:
            fh.write(probe_json)
            tmp = fh.name
        try:
            return self._run_main_capturing([tmp])
        finally:
            os.unlink(tmp)

    @unittest.skipUnless(_REIFY_BUILT, "reify binary not built")
    def test_e2e_damping_ratio_in_range_is_pass(self):
        rc, out, _ = self._e2e("value", "present", {
            "stdout_value": dict(_DAMPING_SPEC)
        })
        self.assertEqual(rc, 0, out)
        self.assertIn(f"[{pcc.PASS}]", out)

    @unittest.skipUnless(_REIFY_BUILT, "reify binary not built")
    def test_e2e_degenerate_ratio_is_fail(self):
        """The motivating defect: a clean eval that printed 0."""
        rc, out, _ = self._e2e("value", "present", {
            "stdout_value": {
                "pattern": r"ValueCleanEvalCells\.degenerate_ratio = ([-+0-9.eE]+)",
                "min": 0.01,
            }
        })
        self.assertEqual(rc, 1, out)
        self.assertIn(f"[{pcc.FAIL}]", out)

    @unittest.skipUnless(_REIFY_BUILT, "reify binary not built")
    def test_e2e_undef_ratio_is_fail(self):
        """Prints garbage, still exits 0."""
        rc, out, _ = self._e2e("value", "present", {
            "stdout_value": {
                "pattern": r"ValueCleanEvalCells\.undef_ratio = (\S+)",
                "finite": True,
            }
        })
        self.assertEqual(rc, 1, out)
        self.assertIn(f"[{pcc.FAIL}]", out)

    @unittest.skipUnless(_REIFY_BUILT, "reify binary not built")
    def test_e2e_same_fixture_still_passes_an_ir_probe_vacuously(self):
        """The vacuous pass still holds — the new kind adds signal, it does not
        change `ir`."""
        rc, out, _ = self._e2e("ir", "absent", {"stderr_contains": "EvalError"})
        self.assertEqual(rc, 0, out)
        self.assertIn(f"[{pcc.PASS}]", out)


# ---------------------------------------------------------------------------
# #8644 (S22 δ, B8): the real CLI refuses a vacuous probe set as a usage error
# ---------------------------------------------------------------------------

class TestLoadRefusalCli(unittest.TestCase):
    """prd-capability-check.py, run as a real process, exits 64 naming the probe.

    REIFY_BIN names a binary that does not exist, so a probe set the loader
    wrongly accepts ends in a harness error instead of a real `reify` run, and
    can never be mistaken for the usage error asserted here.
    """

    def _run_cli(self, bad_probe):
        valid_first = {
            "capability": "a valid check probe",
            "probe_kind": "check",
            "fixture": "tests/prd-gate/fixtures/arrow_type.ri",
            "expected": {"observation": "present", "match": {"exit_code": 1}},
        }
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "probe-set.json")
            with open(path, "w") as fh:
                json.dump({"probes": [valid_first, bad_probe]}, fh)
            env = dict(os.environ, REIFY_BIN=os.path.join(tmp, "no-such-reify"))
            return subprocess.run(
                [sys.executable, _HARNESS_PATH, path],
                capture_output=True, text=True, env=env,
            )

    def _check_probe(self, match, fixture="tests/prd-gate/fixtures/arrow_type.ri"):
        return {
            "capability": "a check probe",
            "probe_kind": "check",
            "fixture": fixture,
            "expected": {"observation": "present", "match": match},
        }

    def _assert_usage_error_naming_second_probe(self, bad_probe, *needles):
        proc = self._run_cli(bad_probe)
        self.assertEqual(proc.returncode, 64, proc.stderr)
        for needle in ("probe[1]",) + needles:
            self.assertIn(needle, proc.stderr)

    def test_check_probe_with_empty_match_is_a_usage_error(self):
        self._assert_usage_error_naming_second_probe(self._check_probe({}))

    def test_probe_with_empty_fixture_is_a_usage_error(self):
        self._assert_usage_error_naming_second_probe(
            self._check_probe({"exit_code": 1}, fixture="")
        )

    def test_check_probe_with_only_an_empty_stderr_signature_is_a_usage_error(self):
        self._assert_usage_error_naming_second_probe(
            self._check_probe({"stderr_contains": ""})
        )

    def test_check_probe_with_only_an_empty_stdout_signature_is_a_usage_error(self):
        self._assert_usage_error_naming_second_probe(
            self._check_probe({"stdout_contains": ""})
        )

    def test_check_probe_with_a_misspelt_match_key_is_a_usage_error(self):
        self._assert_usage_error_naming_second_probe(
            self._check_probe({"exit_code": 0, "stderr_pattern": "x"}),
            "stderr_pattern",
        )


if __name__ == "__main__":
    unittest.main()
