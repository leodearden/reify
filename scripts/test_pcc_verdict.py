#!/usr/bin/env python3
"""
test_pcc_verdict.py — the pure decision layer of
scripts/prd-capability-check.py: verdict(), observe(), evaluate() and
harness_exit_code(), driven by synthetic ProbeRuns.  Shared loader and helpers:
scripts/_pcc_test_support.py; runner: tests/infra/test_prd_capability_check.sh.
"""

import dataclasses
import unittest
from typing import Any

from _pcc_test_support import (
    _CACHE_DENIED_STDERR,
    pcc,
)


# ---------------------------------------------------------------------------
# step-03 (RED): pure verdict() truth table
# ---------------------------------------------------------------------------

class TestVerdictTruthTable(unittest.TestCase):
    """Tests for Verdict/Observation constants and verdict() pure function.

    These tests FAIL until step-04 adds Verdict, Observation, verdict().
    """

    def test_observation_constants_exist(self):
        """Observation constants PRESENT, ABSENT, INDETERMINATE must exist."""
        self.assertTrue(hasattr(pcc, "PRESENT"), "missing PRESENT")
        self.assertTrue(hasattr(pcc, "ABSENT"), "missing ABSENT")
        self.assertTrue(hasattr(pcc, "INDETERMINATE"), "missing INDETERMINATE")

    def test_verdict_constants_exist(self):
        """Verdict constants PASS, FAIL, UNPROVABLE must exist."""
        self.assertTrue(hasattr(pcc, "PASS"), "missing PASS")
        self.assertTrue(hasattr(pcc, "FAIL"), "missing FAIL")
        self.assertTrue(hasattr(pcc, "UNPROVABLE"), "missing UNPROVABLE")

    def test_verdict_function_exists(self):
        self.assertTrue(hasattr(pcc, "verdict"), "missing verdict()")
        self.assertTrue(callable(pcc.verdict))

    # ── PASS cases ────────────────────────────────────────────────────────────

    def test_present_expected_present_is_pass(self):
        """PRESENT + expected present → PASS."""
        v = pcc.verdict(pcc.PRESENT, "present")
        self.assertEqual(v, pcc.PASS)

    def test_absent_expected_absent_is_pass(self):
        """ABSENT + expected absent → PASS."""
        v = pcc.verdict(pcc.ABSENT, "absent")
        self.assertEqual(v, pcc.PASS)

    # ── FAIL cases ────────────────────────────────────────────────────────────

    def test_present_expected_absent_is_fail(self):
        """PRESENT + expected absent → FAIL."""
        v = pcc.verdict(pcc.PRESENT, "absent")
        self.assertEqual(v, pcc.FAIL)

    def test_absent_expected_present_is_fail(self):
        """ABSENT + expected present → FAIL."""
        v = pcc.verdict(pcc.ABSENT, "present")
        self.assertEqual(v, pcc.FAIL)

    # ── UNPROVABLE cases ──────────────────────────────────────────────────────

    def test_indeterminate_expected_present_is_unprovable(self):
        """INDETERMINATE + expected present → UNPROVABLE."""
        v = pcc.verdict(pcc.INDETERMINATE, "present")
        self.assertEqual(v, pcc.UNPROVABLE)

    def test_indeterminate_expected_absent_is_unprovable(self):
        """INDETERMINATE + expected absent → UNPROVABLE."""
        v = pcc.verdict(pcc.INDETERMINATE, "absent")
        self.assertEqual(v, pcc.UNPROVABLE)

    # ── constants are distinct ────────────────────────────────────────────────

    def test_observation_constants_are_distinct(self):
        """PRESENT, ABSENT, INDETERMINATE must be pairwise distinct."""
        self.assertNotEqual(pcc.PRESENT, pcc.ABSENT)
        self.assertNotEqual(pcc.PRESENT, pcc.INDETERMINATE)
        self.assertNotEqual(pcc.ABSENT, pcc.INDETERMINATE)

    def test_verdict_constants_are_distinct(self):
        """PASS, FAIL, UNPROVABLE must be pairwise distinct."""
        self.assertNotEqual(pcc.PASS, pcc.FAIL)
        self.assertNotEqual(pcc.PASS, pcc.UNPROVABLE)
        self.assertNotEqual(pcc.FAIL, pcc.UNPROVABLE)


# ---------------------------------------------------------------------------
# The typed probe outcome: HarnessFailureKind, HarnessFailure, ProbeRun
# ---------------------------------------------------------------------------

class TestTypedProbeOutcome(unittest.TestCase):
    """How the machinery failed is a typed value, enforced where it is built."""

    def test_harness_failure_kinds_are_exactly_the_four_json_values(self):
        values = {kind.value for kind in pcc.HarnessFailureKind}
        self.assertEqual(values, {"launch", "setup", "timeout", "grammar_load"})
        for kind in pcc.HarnessFailureKind:
            with self.subTest(kind=kind):
                self.assertIsInstance(kind, str)

    def test_cache_denied_is_refused_on_any_kind_but_grammar_load(self):
        with self.assertRaises(ValueError) as ctx:
            pcc.HarnessFailure(pcc.HarnessFailureKind.LAUNCH, "x", cache_denied=True)
        message = str(ctx.exception)
        self.assertIn("launch", message)
        self.assertIn("cache_denied", message)

    def test_a_plain_string_kind_is_refused(self):
        with self.assertRaises(TypeError):
            pcc.HarnessFailure("launch", "x")

    def test_harness_failure_and_probe_run_are_frozen(self):
        failure = pcc.HarnessFailure(pcc.HarnessFailureKind.TIMEOUT, "slow")
        with self.assertRaises(dataclasses.FrozenInstanceError):
            failure.detail = "changed"
        run = pcc.ProbeRun(0, "", "")
        with self.assertRaises(dataclasses.FrozenInstanceError):
            run.exit_code = 1

    def test_probe_run_defaults_to_no_failure_and_a_present_fixture(self):
        run = pcc.ProbeRun(0, "", "")
        self.assertIsNone(run.harness_failure)
        self.assertIs(run.fixture_present, True)


# ---------------------------------------------------------------------------
# step-05 (RED): observation determination per probe kind
# ---------------------------------------------------------------------------

class TestObservation(unittest.TestCase):
    """Tests for observe() and match_predicate() using synthetic ProbeRun fixtures.

    These tests FAIL until step-06 adds ProbeRun, match_predicate, observe().
    """

    def _run(self, exit_code: int, stdout: str = "", stderr: str = "") -> Any:
        """Build a synthetic ProbeRun-like object (or namedtuple/dataclass)."""
        return pcc.ProbeRun(exit_code=exit_code, stdout=stdout, stderr=stderr)

    # ── match_predicate ───────────────────────────────────────────────────────

    def test_match_empty_predicate_always_true(self):
        """Empty match dict {} is always satisfied."""
        run = self._run(0, stdout="hello", stderr="")
        self.assertTrue(pcc.match_predicate(run, {}))

    def test_match_exit_code_satisfied(self):
        """match {exit_code: 1} is satisfied when exit_code == 1."""
        run = self._run(1)
        self.assertTrue(pcc.match_predicate(run, {"exit_code": 1}))

    def test_match_exit_code_not_satisfied(self):
        """match {exit_code: 1} is NOT satisfied when exit_code == 0."""
        run = self._run(0)
        self.assertFalse(pcc.match_predicate(run, {"exit_code": 1}))

    def test_match_stderr_contains_satisfied(self):
        """match {stderr_contains: 'Error'} satisfied when 'Error' in stderr."""
        run = self._run(1, stderr="Error: something went wrong")
        self.assertTrue(pcc.match_predicate(run, {"stderr_contains": "Error"}))

    def test_match_stderr_contains_not_satisfied(self):
        """match {stderr_contains: 'Error'} NOT satisfied when absent from stderr."""
        run = self._run(1, stderr="warning: minor issue")
        self.assertFalse(pcc.match_predicate(run, {"stderr_contains": "Error"}))

    def test_match_stdout_contains_satisfied(self):
        """match {stdout_contains: 'All constraints satisfied.'} satisfied."""
        run = self._run(0, stdout="All constraints satisfied.")
        self.assertTrue(pcc.match_predicate(run, {"stdout_contains": "All constraints satisfied."}))

    def test_match_stdout_contains_not_satisfied(self):
        """match {stdout_contains: 'All constraints satisfied.'} NOT satisfied when absent."""
        run = self._run(0, stdout="")
        self.assertFalse(pcc.match_predicate(run, {"stdout_contains": "All constraints satisfied."}))

    def test_match_combined_all_must_hold(self):
        """Combined match: all set fields must hold simultaneously (AND)."""
        # Both exit_code and stderr_contains
        run = self._run(1, stderr="rejection: type mismatch")
        self.assertTrue(pcc.match_predicate(run, {"exit_code": 1, "stderr_contains": "rejection"}))
        # exit_code matches but stderr_contains does not
        self.assertFalse(pcc.match_predicate(run, {"exit_code": 1, "stderr_contains": "EvalError"}))
        # Neither matches
        run2 = self._run(0, stderr="clean")
        self.assertFalse(pcc.match_predicate(run2, {"exit_code": 1, "stderr_contains": "rejection"}))

    # ── observe() for grammar kind ────────────────────────────────────────────

    def test_grammar_exit0_is_present(self):
        """grammar: exit 0 → PRESENT (no parse errors)."""
        run = self._run(0)
        obs = pcc.observe("grammar", run, {})
        self.assertEqual(obs, pcc.PRESENT)

    def test_grammar_exit1_with_error_node_is_absent(self):
        """grammar: exit 1 with '(ERROR' in output → ABSENT."""
        run = self._run(1, stderr="(ERROR [1,12]-[1,32])")
        obs = pcc.observe("grammar", run, {})
        self.assertEqual(obs, pcc.ABSENT)

    def test_grammar_exit1_with_failed_to_load_language_is_harness_error(self):
        """grammar: the boundary's GRAMMAR_LOAD failure → harness-error observation."""
        stderr = "Failed to load language: reify"
        run = pcc.ProbeRun(1, "", stderr, harness_failure=pcc.grammar_load_failure(stderr))
        obs = pcc.observe("grammar", run, {})
        # Must not be PRESENT or ABSENT — it must be a harness-error signal
        self.assertNotEqual(obs, pcc.PRESENT)
        self.assertNotEqual(obs, pcc.ABSENT)

    # ── observe() for check kind ──────────────────────────────────────────────

    def test_check_match_satisfied_is_present(self):
        """check: match predicate satisfied → PRESENT."""
        # exit_code=1 predicate satisfied
        run = self._run(1, stderr="error: type mismatch")
        obs = pcc.observe("check", run, {"exit_code": 1})
        self.assertEqual(obs, pcc.PRESENT)

    def test_check_match_not_satisfied_is_absent(self):
        """check: match predicate not satisfied → ABSENT."""
        # The §3 4575 case: reify exits 0 + 'All constraints satisfied.', no rejection diag
        run = self._run(0, stdout="All constraints satisfied.")
        obs = pcc.observe("check", run, {"exit_code": 1})
        self.assertEqual(obs, pcc.ABSENT)

    def test_check_4575_silent_accept_is_absent(self):
        """§3 4575: exit 0, 'All constraints satisfied.', empty stderr → no rejection → ABSENT."""
        run = self._run(0, stdout="All constraints satisfied.", stderr="")
        # We probe for presence of a rejection diagnostic (exit_code: 1)
        obs = pcc.observe("check", run, {"exit_code": 1})
        self.assertEqual(obs, pcc.ABSENT)

    # ── observe() for ir kind ─────────────────────────────────────────────────

    def test_ir_exit0_clean_is_absent(self):
        """ir: exit 0 clean → ABSENT (sound by determinism §6 G6(b))."""
        run = self._run(0, stdout="a = 0.01 m", stderr="")
        obs = pcc.observe("ir", run, {"stderr_contains": "CrossSubGeometryRef"})
        self.assertEqual(obs, pcc.ABSENT)

    def test_ir_exit_nonzero_with_signature_is_present(self):
        """ir: exit ≠ 0 with asserted signature in stderr → PRESENT."""
        run = self._run(101, stderr="thread panicked: CrossSubGeometryRef would panic in eval_expr")
        obs = pcc.observe("ir", run, {"stderr_contains": "CrossSubGeometryRef"})
        self.assertEqual(obs, pcc.PRESENT)

    def test_ir_exit_nonzero_without_signature_is_indeterminate(self):
        """ir: exit ≠ 0 with an UNRELATED error (signature absent) → INDETERMINATE."""
        run = self._run(1, stderr="error: unresolved type: Transform3")
        obs = pcc.observe("ir", run, {"stderr_contains": "CrossSubGeometryRef"})
        self.assertEqual(obs, pcc.INDETERMINATE)


# ---------------------------------------------------------------------------
# step-07 (RED): evaluate() over injected synthetic runs
# ---------------------------------------------------------------------------

# ---------------------------------------------------------------------------
# #6876 step-07 (RED): observe()'s "value" arm and the vacuity-closure contrast
# ---------------------------------------------------------------------------

class TestValueObservation(unittest.TestCase):
    """observe()'s fourth arm — this is the #6876 vector, stated as a test.

    Same run, same exit 0: `ir` cannot see the value and `value` can.  The
    contrast tests below build ONE ProbeRun and read it through both arms, so
    the vacuous-pass and the real-fail are provably about the same evidence
    rather than about two conveniently-different fixtures.
    """

    _DAMPING = {"pattern": r"damping_ratio = ([-+0-9.eE]+)", "min": 0.01, "max": 0.03}

    def _run(self, exit_code, stdout="", stderr=""):
        return pcc.ProbeRun(exit_code=exit_code, stdout=stdout, stderr=stderr)

    def _value(self, spec):
        return {"stdout_value": spec}

    # ── the observation model ─────────────────────────────────────────────────

    def test_exit_zero_predicate_satisfied_is_present(self):
        run = self._run(0, stdout="damping_ratio = 0.018\n")
        self.assertEqual(
            pcc.observe("value", run, self._value(self._DAMPING)), pcc.PRESENT
        )

    def test_exit_zero_value_out_of_range_is_absent(self):
        run = self._run(0, stdout="damping_ratio = 0\n")
        self.assertEqual(
            pcc.observe("value", run, self._value(self._DAMPING)), pcc.ABSENT
        )

    def test_exit_zero_pattern_missing_is_indeterminate(self):
        """A mis-aimed pattern read nothing, so it cannot report a negative.

        Same rule as the non-zero exit below: a renamed output field or a typo'd
        pattern is indistinguishable from a value that WAS read and found
        wanting, and ABSENT would turn a broken probe into a confident finding.
        """
        run = self._run(0, stdout="something_else = 0.018\n")
        self.assertEqual(
            pcc.observe("value", run, self._value(self._DAMPING)),
            pcc.INDETERMINATE,
        )

    def test_absent_is_reserved_for_a_value_that_was_actually_read(self):
        """The discriminator between the two negative answers, side by side.

        Both runs exit 0 and neither satisfies the spec; only the one whose
        capture was parsed earns ABSENT.  Were a pattern miss ABSENT too, a
        value probe pinned `observation: absent` — which load_probe_set accepts
        — would PASS forever the moment its pattern stopped matching, asserting
        nothing at all.
        """
        read = self._run(0, stdout="damping_ratio = 0\n")
        unread = self._run(0, stdout="something_else = 0.018\n")
        self.assertEqual(
            pcc.observe("value", read, self._value(self._DAMPING)), pcc.ABSENT
        )
        self.assertEqual(
            pcc.observe("value", unread, self._value(self._DAMPING)),
            pcc.INDETERMINATE,
        )
        # And the mis-aimed probe is UNPROVABLE under BOTH polarities, so no
        # expectation can launder it into a pass.
        for polarity in ("present", "absent"):
            with self.subTest(polarity=polarity):
                self.assertEqual(
                    pcc.verdict(
                        pcc.observe("value", unread, self._value(self._DAMPING)),
                        polarity,
                    ),
                    pcc.UNPROVABLE,
                )

    def test_exit_zero_undef_capture_is_absent(self):
        run = self._run(0, stdout="damping_ratio = undef\n")
        spec = {"pattern": r"damping_ratio = (\S+)", "finite": True}
        self.assertEqual(pcc.observe("value", run, self._value(spec)), pcc.ABSENT)

    def test_nonzero_exit_is_indeterminate_even_with_satisfying_stdout(self):
        """An eval error means no value was produced.

        "capability absent" and "fixture/harness broken" are indistinguishable
        on that branch, so reporting ABSENT would manufacture a confident
        negative finding out of a broken probe — the mirror image of the
        vacuity this kind exists to close.  Asserted with stdout that WOULD
        satisfy the predicate, so nothing but the exit code can be deciding it.
        """
        for code in (1, 101):
            with self.subTest(exit_code=code):
                run = self._run(code, stdout="damping_ratio = 0.018\n")
                self.assertEqual(
                    pcc.observe("value", run, self._value(self._DAMPING)),
                    pcc.INDETERMINATE,
                )

    def test_nonzero_exit_with_empty_stdout_is_indeterminate(self):
        run = self._run(1, stderr="EvalError: boom\n")
        self.assertEqual(
            pcc.observe("value", run, self._value(self._DAMPING)),
            pcc.INDETERMINATE,
        )

    # ── a harness failure still beats the kind dispatch ───────────────────────

    def _failed(self, exit_code, kind, detail):
        return pcc.ProbeRun(
            exit_code, "", "", harness_failure=pcc.HarnessFailure(kind, detail)
        )

    def test_launch_failure_is_harness_error_not_indeterminate(self):
        run = self._failed(127, pcc.HarnessFailureKind.LAUNCH, "[Errno 2] reify")
        self.assertEqual(
            pcc.observe("value", run, self._value(self._DAMPING)),
            pcc._HARNESS_ERROR,
        )

    def test_timeout_is_harness_error_not_indeterminate(self):
        run = self._failed(124, pcc.HarnessFailureKind.TIMEOUT, "timed out after 30s")
        self.assertEqual(
            pcc.observe("value", run, self._value(self._DAMPING)),
            pcc._HARNESS_ERROR,
        )

    # ── verdict() composition ─────────────────────────────────────────────────

    def test_verdict_composition(self):
        self.assertEqual(pcc.verdict(pcc.PRESENT, "present"), pcc.PASS)
        self.assertEqual(pcc.verdict(pcc.ABSENT, "present"), pcc.FAIL)
        self.assertEqual(pcc.verdict(pcc.INDETERMINATE, "present"), pcc.UNPROVABLE)

    # ── THE VACUITY-CLOSURE CONTRACT ──────────────────────────────────────────

    def test_degenerate_zero_ir_passes_vacuously_but_value_fails(self):
        """One run, exit 0, printing the motivating `damping_ratio: 0` defect."""
        run = self._run(0, stdout="ValueCleanEvalCells.degenerate_ratio = 0\n")

        ir_obs = pcc.observe("ir", run, {"stderr_contains": "EvalError"})
        self.assertEqual(ir_obs, pcc.ABSENT)
        self.assertEqual(pcc.verdict(ir_obs, "absent"), pcc.PASS)

        value_obs = pcc.observe(
            "value",
            run,
            self._value(
                {"pattern": r"degenerate_ratio = ([-+0-9.eE]+)", "min": 0.01}
            ),
        )
        self.assertEqual(value_obs, pcc.ABSENT)
        self.assertEqual(pcc.verdict(value_obs, "present"), pcc.FAIL)

    def test_undef_cell_ir_passes_vacuously_but_value_fails(self):
        """The same contrast for 'prints garbage with exit 0'."""
        run = self._run(0, stdout="ValueCleanEvalCells.undef_ratio = undef\n")

        ir_obs = pcc.observe("ir", run, {"stderr_contains": "EvalError"})
        self.assertEqual(ir_obs, pcc.ABSENT)
        self.assertEqual(pcc.verdict(ir_obs, "absent"), pcc.PASS)

        value_obs = pcc.observe(
            "value",
            run,
            self._value({"pattern": r"undef_ratio = (\S+)", "finite": True}),
        )
        self.assertEqual(value_obs, pcc.ABSENT)
        self.assertEqual(pcc.verdict(value_obs, "present"), pcc.FAIL)

    # ── REGRESSION: the three existing arms, on the same synthetic runs ───────

    def test_existing_arms_undisturbed(self):
        clean = self._run(0, stdout="damping_ratio = 0.018\n")
        self.assertEqual(pcc.observe("ir", clean, {}), pcc.ABSENT)
        self.assertEqual(
            pcc.observe("ir", clean, {"stderr_contains": "EvalError"}), pcc.ABSENT
        )

        signed = self._run(1, stderr="EvalError: contract violated\n")
        self.assertEqual(
            pcc.observe("ir", signed, {"stderr_contains": "EvalError"}), pcc.PRESENT
        )
        unsigned = self._run(1, stderr="something unrelated\n")
        self.assertEqual(
            pcc.observe("ir", unsigned, {"stderr_contains": "EvalError"}),
            pcc.INDETERMINATE,
        )

        self.assertEqual(
            pcc.observe("check", self._run(1), {"exit_code": 1}), pcc.PRESENT
        )
        self.assertEqual(
            pcc.observe("check", self._run(0), {"exit_code": 1}), pcc.ABSENT
        )

        self.assertEqual(pcc.observe("grammar", self._run(0), {}), pcc.PRESENT)
        self.assertEqual(pcc.observe("grammar", self._run(1), {}), pcc.ABSENT)
        load_failure = pcc.ProbeRun(
            1, "", "", harness_failure=pcc.HarnessFailure(
                pcc.HarnessFailureKind.GRAMMAR_LOAD, "Failed to load language: reify"
            ),
        )
        self.assertEqual(
            pcc.observe("grammar", load_failure, {}), pcc._HARNESS_ERROR
        )

    def test_grammar_arm_reads_the_field_not_the_text(self):
        """The load-failure TEXT alone is ABSENT: stderr is parsed at the boundary.

        run_probe() and evaluate() turn tree-sitter's signature into
        harness_failure; observe() reads only that field, never stderr.
        """
        run = self._run(1, stderr="Error: Failed to load language for path \"x.ri\"\n")
        self.assertEqual(pcc.observe("grammar", run, {}), pcc.ABSENT)


class TestAbsentFixtureIsIndeterminate(unittest.TestCase):
    """A probe with no target cannot prove anything (PRD D3).

    Each case is a run that WOULD have been PRESENT or ABSENT had its fixture
    existed, so nothing but fixture_present can be deciding the answer.
    """

    _DAMPING = {"pattern": r"damping_ratio = ([-+0-9.eE]+)", "min": 0.01, "max": 0.03}

    # (kind, run fields, match): each an otherwise-decisive observation.
    _DECISIVE_RUNS = (
        ("grammar", (0, "", ""), {}),
        ("check", (1, "", "Error reading x.ri"), {"exit_code": 1}),
        ("ir", (0, "a = 0.01 m", ""), {"stderr_contains": "EvalError"}),
        ("value", (0, "damping_ratio = 0.018\n", ""), {"stdout_value": _DAMPING}),
    )

    def test_absent_fixture_is_indeterminate_for_every_kind(self):
        for kind, (exit_code, stdout, stderr), match in self._DECISIVE_RUNS:
            with self.subTest(kind=kind):
                present = pcc.ProbeRun(exit_code, stdout, stderr)
                self.assertIn(
                    pcc.observe(kind, present, match), (pcc.PRESENT, pcc.ABSENT),
                    "precondition: with its fixture present the run is decisive",
                )
                absent = pcc.ProbeRun(exit_code, stdout, stderr, fixture_present=False)
                self.assertEqual(
                    pcc.observe_with_evidence(kind, absent, match),
                    (pcc.INDETERMINATE, None),
                    "no value reading is taken from a run with no target",
                )

    def test_absent_fixture_evaluates_unprovable_and_exits_2(self):
        for kind, (exit_code, stdout, stderr), match in self._DECISIVE_RUNS:
            with self.subTest(kind=kind):
                probe = pcc.Probe(
                    capability=f"{kind} on a missing fixture",
                    probe_kind=kind,
                    fixture="tests/prd-gate/fixtures/never_written.ri",
                    expected={"observation": "present", "match": match},
                )
                result = pcc.evaluate(
                    probe,
                    runner=lambda _p: pcc.ProbeRun(
                        exit_code, stdout, stderr, fixture_present=False
                    ),
                )
                self.assertEqual(result.verdict, pcc.UNPROVABLE)
                self.assertEqual(pcc.harness_exit_code([result]), 2)

    def test_a_harness_failure_outranks_an_absent_fixture(self):
        """The machinery failing is the louder fact."""
        run = pcc.ProbeRun(
            127, "", "",
            harness_failure=pcc.HarnessFailure(
                pcc.HarnessFailureKind.LAUNCH, "[Errno 2] No such file or directory"
            ),
            fixture_present=False,
        )
        for kind, _, match in self._DECISIVE_RUNS:
            with self.subTest(kind=kind):
                self.assertEqual(pcc.observe(kind, run, match), pcc._HARNESS_ERROR)


class TestEvaluate(unittest.TestCase):
    """Tests for evaluate() over injected synthetic ProbeRun fixtures.

    Verifies three golden verdicts (PASS/FAIL/UNPROVABLE) and mandatory evidence.
    These tests FAIL until step-08 adds Result and evaluate().
    """

    def _make_grammar_probe(self, expected_obs: str = "present") -> Any:
        return pcc.Probe(
            capability="arrow-type grammar production",
            probe_kind="grammar",
            fixture="tests/prd-gate/fixtures/arrow_type.ri",
            expected={"observation": expected_obs, "match": {}},
        )

    def _make_check_probe(self, expected_obs: str = "present") -> Any:
        return pcc.Probe(
            capability="arg-vs-param rejection (4575)",
            probe_kind="check",
            fixture="tests/prd-gate/fixtures/revolute_silent_accept.ri",
            expected={"observation": expected_obs, "match": {"exit_code": 1}},
        )

    def _make_ir_probe(self, expected_obs: str = "absent") -> Any:
        return pcc.Probe(
            capability="eval-error proxy clean baseline",
            probe_kind="ir",
            fixture="tests/prd-gate/fixtures/ir_clean_eval.ri",
            expected={"observation": expected_obs, "match": {"stderr_contains": "CrossSubGeometryRef"}},
        )

    def _stub_runner(self, exit_code: int, stdout: str = "", stderr: str = "") -> Any:
        """Return a runner function that returns a fixed ProbeRun."""
        def runner(probe):  # noqa: ANN202
            return pcc.ProbeRun(exit_code=exit_code, stdout=stdout, stderr=stderr)
        return runner

    # ── (a) grammar probe, exit 0, expected present → PASS ───────────────────

    def test_grammar_pass(self):
        """grammar probe, run exit 0, expected present → PASS."""
        probe = self._make_grammar_probe("present")
        result = pcc.evaluate(probe, runner=self._stub_runner(0))
        self.assertEqual(result.verdict, pcc.PASS)

    # ── (b) check probe = §3 4575 → FAIL ─────────────────────────────────────

    def test_check_4575_fail(self):
        """§3 4575: check probe, exit 0 + 'All constraints satisfied.', expected present → FAIL."""
        probe = self._make_check_probe("present")
        # Reify exits 0 with 'All constraints satisfied.', no rejection diag
        result = pcc.evaluate(
            probe,
            runner=self._stub_runner(0, stdout="All constraints satisfied.", stderr=""),
        )
        self.assertEqual(result.verdict, pcc.FAIL)

    # ── (c) ir probe, unrelated error, expected absent → UNPROVABLE ──────────

    def test_ir_unrelated_error_unprovable(self):
        """ir probe, exit ≠ 0 with unrelated error, no asserted signature → UNPROVABLE."""
        probe = self._make_ir_probe("absent")
        result = pcc.evaluate(
            probe,
            runner=self._stub_runner(1, stderr="error: unresolved type: Transform3"),
        )
        self.assertEqual(result.verdict, pcc.UNPROVABLE)

    # ── mandatory evidence: command, exit_code, stdout, stderr ───────────────

    def test_result_has_command(self):
        """Result must carry the exact command argv."""
        probe = self._make_grammar_probe("present")
        result = pcc.evaluate(probe, runner=self._stub_runner(0))
        self.assertIsNotNone(result.command, "result.command must not be None")
        self.assertIsInstance(result.command, list, "result.command must be a list")
        self.assertGreater(len(result.command), 0, "result.command must be non-empty")

    def test_result_has_exit_code(self):
        """Result must carry the exit_code from the captured run."""
        probe = self._make_grammar_probe("present")
        result = pcc.evaluate(probe, runner=self._stub_runner(0))
        self.assertEqual(result.run.exit_code, 0)

    def test_result_has_stdout(self):
        """Result must carry the stdout from the captured run."""
        probe = self._make_check_probe("present")
        result = pcc.evaluate(
            probe,
            runner=self._stub_runner(0, stdout="All constraints satisfied."),
        )
        self.assertEqual(result.run.stdout, "All constraints satisfied.")

    def test_result_has_stderr(self):
        """Result must carry the stderr from the captured run."""
        probe = self._make_ir_probe("absent")
        result = pcc.evaluate(
            probe,
            runner=self._stub_runner(1, stderr="error: unresolved type: Transform3"),
        )
        self.assertIn("unresolved type", result.run.stderr)

    def test_result_carries_probe(self):
        """Result must carry the original Probe object."""
        probe = self._make_grammar_probe("present")
        result = pcc.evaluate(probe, runner=self._stub_runner(0))
        self.assertIs(result.probe, probe)

    def test_result_carries_observation(self):
        """Result must carry the observation value."""
        probe = self._make_grammar_probe("present")
        result = pcc.evaluate(probe, runner=self._stub_runner(0))
        self.assertEqual(result.observation, pcc.PRESENT)


class TestEvaluateTypesAGrammarLoadFailure(unittest.TestCase):
    """evaluate() types a load failure whatever runner produced the run.

    A runner that hands back a bare ProbeRun of tree-sitter's output, with no
    harness_failure, must not turn a grammar that never loaded into ABSENT: for
    a rejection probe (expected absent) that is a false PASS.
    """

    _LOAD_FAILURE = 'Error: Failed to load language for path "x.ri"\n'

    def _evaluate_grammar(self, expected_obs, exit_code, stderr):
        probe = pcc.Probe(
            capability="a grammar rejection",
            probe_kind="grammar",
            fixture="tests/prd-gate/fixtures/arrow_type.ri",
            expected={"observation": expected_obs, "match": {}},
        )
        return pcc.evaluate(
            probe, runner=lambda _p: pcc.ProbeRun(exit_code, "", stderr)
        )

    def test_a_bare_load_failure_is_a_harness_error_not_a_pass(self):
        result = self._evaluate_grammar("absent", 1, self._LOAD_FAILURE)
        self.assertEqual(result.verdict, pcc._HARNESS_ERROR)
        self.assertEqual(pcc.harness_exit_code([result]), 70)
        self.assertEqual(
            result.run.harness_failure.kind, pcc.HarnessFailureKind.GRAMMAR_LOAD,
            "the record names the failure, so it is not an un-attributable row",
        )
        self.assertFalse(result.run.harness_failure.cache_denied)

    def test_a_bare_cache_denial_is_typed_as_one(self):
        result = self._evaluate_grammar("present", 1, _CACHE_DENIED_STDERR)
        self.assertEqual(result.verdict, pcc._HARNESS_ERROR)
        self.assertTrue(result.run.harness_failure.cache_denied)

    def test_a_parse_error_still_answers_absent(self):
        """Narrowness: exit 1 with no load-failure signature is the grammar's answer."""
        result = self._evaluate_grammar("absent", 1, "")
        self.assertEqual(result.verdict, pcc.PASS)
        self.assertIsNone(result.run.harness_failure)

    def test_a_runners_own_failure_is_kept(self):
        launch = pcc.HarnessFailure(pcc.HarnessFailureKind.LAUNCH, "[Errno 2] x")
        probe = pcc.Probe(
            capability="a grammar rejection",
            probe_kind="grammar",
            fixture="tests/prd-gate/fixtures/arrow_type.ri",
            expected={"observation": "absent", "match": {}},
        )
        result = pcc.evaluate(
            probe,
            runner=lambda _p: pcc.ProbeRun(
                127, "", self._LOAD_FAILURE, harness_failure=launch
            ),
        )
        self.assertIs(result.run.harness_failure, launch)

    def test_only_a_grammar_run_can_be_a_load_failure(self):
        probe = pcc.Probe(
            capability="a check whose own stderr quotes tree-sitter",
            probe_kind="check",
            fixture="tests/prd-gate/fixtures/revolute_silent_accept.ri",
            expected={"observation": "present", "match": {"exit_code": 1}},
        )
        result = pcc.evaluate(
            probe, runner=lambda _p: pcc.ProbeRun(1, "", self._LOAD_FAILURE)
        )
        self.assertEqual(result.verdict, pcc.PASS)
        self.assertIsNone(result.run.harness_failure)


# ---------------------------------------------------------------------------
# step-11 (RED): harness exit-code aggregation
# ---------------------------------------------------------------------------

class TestHarnessExitCode(unittest.TestCase):
    """Tests for harness_exit_code(results) exit-code aggregation.

    Tests FAIL until step-12 implements harness_exit_code().
    """

    def _make_result(self, verdict: str) -> Any:
        """Build a synthetic Result with the given verdict string."""
        probe = pcc.Probe(
            capability="test",
            probe_kind="check",
            fixture="some/fixture.ri",
            expected={"observation": "present", "match": {}},
        )
        return pcc.Result(
            probe=probe,
            command=["reify", "check", "some/fixture.ri"],
            run=pcc.ProbeRun(0, "", ""),
            observation=pcc.PRESENT,
            verdict=verdict,
        )

    # ── 0: all PASS ───────────────────────────────────────────────────────────

    def test_all_pass_exits_0(self):
        """All PASS results → exit code 0."""
        results = [
            self._make_result(pcc.PASS),
            self._make_result(pcc.PASS),
            self._make_result(pcc.PASS),
        ]
        self.assertEqual(pcc.harness_exit_code(results), 0)

    def test_single_pass_exits_0(self):
        """Single PASS → exit code 0."""
        self.assertEqual(pcc.harness_exit_code([self._make_result(pcc.PASS)]), 0)

    # ── 1: ≥1 FAIL (FAIL beats UNPROVABLE) ───────────────────────────────────

    def test_one_fail_exits_1(self):
        """≥1 FAIL result (with PASSes) → exit code 1."""
        results = [
            self._make_result(pcc.PASS),
            self._make_result(pcc.FAIL),
            self._make_result(pcc.PASS),
        ]
        self.assertEqual(pcc.harness_exit_code(results), 1)

    def test_fail_with_unprovable_exits_1(self):
        """≥1 FAIL + ≥1 UNPROVABLE → exit code 1 (FAIL beats UNPROVABLE)."""
        results = [
            self._make_result(pcc.FAIL),
            self._make_result(pcc.UNPROVABLE),
            self._make_result(pcc.PASS),
        ]
        self.assertEqual(pcc.harness_exit_code(results), 1)

    # ── 2: ≥1 UNPROVABLE, 0 FAIL ─────────────────────────────────────────────

    def test_unprovable_only_exits_2(self):
        """≥1 UNPROVABLE and 0 FAIL → exit code 2."""
        results = [
            self._make_result(pcc.PASS),
            self._make_result(pcc.UNPROVABLE),
        ]
        self.assertEqual(pcc.harness_exit_code(results), 2)

    def test_multiple_unprovable_exits_2(self):
        """Multiple UNPROVABLE (no FAIL) → exit code 2."""
        results = [
            self._make_result(pcc.UNPROVABLE),
            self._make_result(pcc.UNPROVABLE),
        ]
        self.assertEqual(pcc.harness_exit_code(results), 2)

    # ── 70: harness-error result ──────────────────────────────────────────────

    def test_harness_error_exits_70(self):
        """≥1 harness-error result → exit code 70."""
        results = [self._make_result(pcc._HARNESS_ERROR)]
        self.assertEqual(pcc.harness_exit_code(results), 70)

    def test_harness_error_beats_fail(self):
        """harness-error + FAIL → exit code 70 (harness error takes highest priority)."""
        results = [
            self._make_result(pcc._HARNESS_ERROR),
            self._make_result(pcc.FAIL),
        ]
        self.assertEqual(pcc.harness_exit_code(results), 70)

    def test_harness_error_beats_unprovable(self):
        """harness-error + UNPROVABLE → exit code 70."""
        results = [
            self._make_result(pcc._HARNESS_ERROR),
            self._make_result(pcc.UNPROVABLE),
        ]
        self.assertEqual(pcc.harness_exit_code(results), 70)

    # ── determinism ───────────────────────────────────────────────────────────

    def test_determinism_same_runner_same_verdicts(self):
        """Same probe + same injected runner → identical verdicts both evaluations."""
        probe = pcc.Probe(
            capability="determinism-test",
            probe_kind="grammar",
            fixture="tests/prd-gate/fixtures/arrow_type.ri",
            expected={"observation": "present", "match": {}},
        )

        def fixed_runner(p: Any) -> Any:
            return pcc.ProbeRun(exit_code=0, stdout="", stderr="")

        r1 = pcc.evaluate(probe, runner=fixed_runner)
        r2 = pcc.evaluate(probe, runner=fixed_runner)
        self.assertEqual(r1.verdict, r2.verdict)

    def test_determinism_same_exit_code(self):
        """Same probe set + same injected runner → identical harness_exit_code."""
        probe = pcc.Probe(
            capability="determinism-test",
            probe_kind="check",
            fixture="tests/prd-gate/fixtures/revolute_silent_accept.ri",
            expected={"observation": "present", "match": {"exit_code": 1}},
        )

        def fixed_runner(p: Any) -> Any:
            return pcc.ProbeRun(exit_code=0, stdout="All constraints satisfied.", stderr="")

        results1 = [pcc.evaluate(probe, runner=fixed_runner)]
        results2 = [pcc.evaluate(probe, runner=fixed_runner)]
        self.assertEqual(
            pcc.harness_exit_code(results1),
            pcc.harness_exit_code(results2),
        )

    def test_harness_error_full_precedence(self):
        """PASS + FAIL + UNPROVABLE + HARNESS_ERROR → exit 70 (highest priority wins)."""
        results = [
            self._make_result(pcc.PASS),
            self._make_result(pcc.FAIL),
            self._make_result(pcc.UNPROVABLE),
            self._make_result(pcc._HARNESS_ERROR),
        ]
        self.assertEqual(pcc.harness_exit_code(results), 70)

    def test_launch_failure_flows_to_exit_70(self):
        """harness_failure LAUNCH → observe → HARNESS_ERROR verdict → exit 70."""
        probe = pcc.Probe(
            capability="launch-failure-flow-test",
            probe_kind="check",
            fixture="some/fixture.ri",
            expected={"observation": "present", "match": {}},
        )

        def launch_failure_runner(p):
            return pcc.ProbeRun(
                exit_code=127,
                stdout="",
                stderr="",
                harness_failure=pcc.HarnessFailure(
                    pcc.HarnessFailureKind.LAUNCH, "[Errno 2] No such file or directory"
                ),
            )

        result = pcc.evaluate(probe, runner=launch_failure_runner)
        self.assertEqual(result.verdict, pcc._HARNESS_ERROR)
        self.assertEqual(pcc.harness_exit_code([result]), 70)


if __name__ == "__main__":
    unittest.main()
