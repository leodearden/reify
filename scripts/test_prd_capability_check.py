#!/usr/bin/env python3
"""
test_prd_capability_check.py — stdlib unittest for scripts/prd-capability-check.py.

Loads the hyphenated prd-capability-check.py via importlib (the jobserver pattern)
since the filename is not importable by name.  Exercises all pure functions and the
CLI main() in hermetic golden tests — real subprocess probes are skip-guarded.
"""

import importlib.util
import json
import os
import sys
import unittest
from typing import Any

# ---------------------------------------------------------------------------
# Module loader — load scripts/prd-capability-check.py into `pcc`
# ---------------------------------------------------------------------------

_SCRIPTS_DIR = os.path.dirname(os.path.abspath(__file__))
_HARNESS_PATH = os.path.join(_SCRIPTS_DIR, "prd-capability-check.py")

_spec = importlib.util.spec_from_file_location("prd_capability_check", _HARNESS_PATH)
pcc = importlib.util.module_from_spec(_spec)
# Register in sys.modules before exec_module so @dataclass and typing annotations
# resolve correctly (they look up cls.__module__ in sys.modules at decoration time).
sys.modules["prd_capability_check"] = pcc
_spec.loader.exec_module(pcc)


# ---------------------------------------------------------------------------
# Scaffold — basic importability and required symbols
# ---------------------------------------------------------------------------

class TestScaffold(unittest.TestCase):
    """Minimal sanity-check that the module is importable and main() exists."""

    def test_module_importable(self):
        self.assertIsNotNone(pcc)

    def test_main_present(self):
        self.assertTrue(
            hasattr(pcc, "main"),
            "prd-capability-check.py must export a main() function",
        )

    def test_main_is_callable(self):
        self.assertTrue(callable(pcc.main))

    def test_main_help_exits_0(self):
        """main(['--help']) must exit 0 (argparse --help behavior)."""
        # main() catches SystemExit and returns the code as an int
        rc = pcc.main(["--help"])
        self.assertEqual(rc, 0, "main(['--help']) must return 0")

    def test_main_no_args_is_usage_error(self):
        """main([]) with no probe-set path must return 64 (EX_USAGE)."""
        rc = pcc.main([])
        self.assertEqual(rc, 64, "main([]) with no args must return 64 (EX_USAGE)")


# ---------------------------------------------------------------------------
# Locate the repo root for resolving committed probe-set paths
# ---------------------------------------------------------------------------

_REPO_ROOT = os.path.dirname(_SCRIPTS_DIR)
_EXAMPLE_PROBE_SET = os.path.join(_REPO_ROOT, "tests", "prd-gate", "example-probe-set.json")


# ---------------------------------------------------------------------------
# step-01 (RED): probe-set JSON round-trip tests
# ---------------------------------------------------------------------------

class TestProbeSetRoundTrip(unittest.TestCase):
    """Tests for load_probe_set / dump_probe_set / Probe round-trip.

    These tests FAIL until step-02 adds Probe, load_probe_set, dump_probe_set.
    """

    # ── inline fixture covering all three probe kinds ─────────────────────────

    PROBE_DICTS = [
        {
            "capability": "arrow-type grammar production",
            "probe_kind": "grammar",
            "fixture": "tests/prd-gate/fixtures/arrow_type.ri",
            "expected": {
                "observation": "present",
                "match": {},
            },
        },
        {
            "capability": "arg-vs-param rejection",
            "probe_kind": "check",
            "fixture": "tests/prd-gate/fixtures/revolute_silent_accept.ri",
            "expected": {
                "observation": "present",
                "match": {"exit_code": 1},
            },
        },
        {
            "capability": "clean eval baseline",
            "probe_kind": "ir",
            "fixture": "tests/prd-gate/fixtures/ir_clean_eval.ri",
            "expected": {
                "observation": "absent",
                "match": {"stderr_contains": "EvalError"},
            },
        },
    ]

    def _make_probe_set_text(self, probe_dicts):
        return json.dumps({"probes": probe_dicts})

    def test_load_parse_all_three_kinds(self):
        """load_probe_set produces one Probe per dict, all fields preserved."""
        text = self._make_probe_set_text(self.PROBE_DICTS)
        probes = pcc.load_probe_set(text)
        self.assertEqual(len(probes), 3)
        self.assertEqual(probes[0].probe_kind, "grammar")
        self.assertEqual(probes[1].probe_kind, "check")
        self.assertEqual(probes[2].probe_kind, "ir")

    def test_load_preserves_capability(self):
        text = self._make_probe_set_text(self.PROBE_DICTS)
        probes = pcc.load_probe_set(text)
        self.assertEqual(probes[0].capability, "arrow-type grammar production")
        self.assertEqual(probes[1].capability, "arg-vs-param rejection")
        self.assertEqual(probes[2].capability, "clean eval baseline")

    def test_load_preserves_fixture(self):
        text = self._make_probe_set_text(self.PROBE_DICTS)
        probes = pcc.load_probe_set(text)
        self.assertEqual(probes[0].fixture, "tests/prd-gate/fixtures/arrow_type.ri")
        self.assertEqual(probes[1].fixture, "tests/prd-gate/fixtures/revolute_silent_accept.ri")
        self.assertEqual(probes[2].fixture, "tests/prd-gate/fixtures/ir_clean_eval.ri")

    def test_load_preserves_observation(self):
        text = self._make_probe_set_text(self.PROBE_DICTS)
        probes = pcc.load_probe_set(text)
        self.assertEqual(probes[0].expected["observation"], "present")
        self.assertEqual(probes[1].expected["observation"], "present")
        self.assertEqual(probes[2].expected["observation"], "absent")

    def test_load_preserves_match_exit_code(self):
        text = self._make_probe_set_text(self.PROBE_DICTS)
        probes = pcc.load_probe_set(text)
        # grammar has empty match
        self.assertEqual(probes[0].expected["match"], {})
        # check has exit_code: 1
        self.assertEqual(probes[1].expected["match"]["exit_code"], 1)

    def test_load_preserves_match_stderr_contains(self):
        text = self._make_probe_set_text(self.PROBE_DICTS)
        probes = pcc.load_probe_set(text)
        # ir has stderr_contains
        self.assertEqual(probes[2].expected["match"]["stderr_contains"], "EvalError")

    def test_round_trip_identical(self):
        """load_probe_set(dump_probe_set(probes)) reproduces the same Probe list."""
        text = self._make_probe_set_text(self.PROBE_DICTS)
        probes = pcc.load_probe_set(text)
        dumped = pcc.dump_probe_set(probes)
        probes2 = pcc.load_probe_set(dumped)
        self.assertEqual(len(probes2), len(probes))
        for p1, p2 in zip(probes, probes2):
            self.assertEqual(p1.capability, p2.capability)
            self.assertEqual(p1.probe_kind, p2.probe_kind)
            self.assertEqual(p1.fixture, p2.fixture)
            self.assertEqual(p1.expected, p2.expected)

    def test_dump_is_valid_json(self):
        """dump_probe_set produces valid JSON that can be loaded back."""
        text = self._make_probe_set_text(self.PROBE_DICTS)
        probes = pcc.load_probe_set(text)
        dumped = pcc.dump_probe_set(probes)
        obj = json.loads(dumped)  # must not raise
        self.assertIn("probes", obj)
        self.assertEqual(len(obj["probes"]), 3)

    # ── match predicate fields (stdout_contains) ─────────────────────────────

    def test_load_preserves_stdout_contains(self):
        """stdout_contains match field is round-tripped correctly."""
        dicts = [
            {
                "capability": "stdout check",
                "probe_kind": "check",
                "fixture": "some/fixture.ri",
                "expected": {
                    "observation": "present",
                    "match": {"stdout_contains": "All constraints satisfied."},
                },
            }
        ]
        text = self._make_probe_set_text(dicts)
        probes = pcc.load_probe_set(text)
        self.assertEqual(
            probes[0].expected["match"]["stdout_contains"],
            "All constraints satisfied.",
        )

    # ── validation: bad probe_kind ────────────────────────────────────────────

    def test_load_rejects_bad_probe_kind(self):
        """load_probe_set raises an error for an unknown probe_kind."""
        dicts = [dict(self.PROBE_DICTS[0], probe_kind="invalid_kind")]
        text = self._make_probe_set_text(dicts)
        with self.assertRaises(Exception):
            pcc.load_probe_set(text)

    def test_load_rejects_unknown_observation(self):
        """load_probe_set raises an error for an unknown observation value."""
        probe = dict(self.PROBE_DICTS[0])
        probe["expected"] = {"observation": "maybe", "match": {}}
        text = self._make_probe_set_text([probe])
        with self.assertRaises(Exception):
            pcc.load_probe_set(text)

    def test_load_rejects_missing_fixture(self):
        """load_probe_set raises an error when fixture field is absent."""
        probe = {
            "capability": "test",
            "probe_kind": "grammar",
            "expected": {"observation": "present", "match": {}},
            # no "fixture" key
        }
        text = self._make_probe_set_text([probe])
        with self.assertRaises(Exception):
            pcc.load_probe_set(text)

    def test_load_rejects_missing_capability(self):
        """load_probe_set raises an error when capability field is absent."""
        probe = {
            "probe_kind": "grammar",
            "fixture": "some/file.ri",
            "expected": {"observation": "present", "match": {}},
            # no "capability" key
        }
        text = self._make_probe_set_text([probe])
        with self.assertRaises(Exception):
            pcc.load_probe_set(text)

    def test_load_rejects_missing_probes_key(self):
        """load_probe_set raises an error if top-level 'probes' key is absent."""
        text = json.dumps([])  # a JSON array instead of an object with "probes"
        with self.assertRaises(Exception):
            pcc.load_probe_set(text)

    # ── committed example-probe-set.json ─────────────────────────────────────

    def test_committed_probe_set_parses_into_3_records(self):
        """The committed example-probe-set.json parses into exactly 3 Probe records."""
        with open(_EXAMPLE_PROBE_SET) as f:
            text = f.read()
        probes = pcc.load_probe_set(text)
        self.assertEqual(len(probes), 3)

    def test_committed_probe_set_has_one_of_each_kind(self):
        """The committed probe set has one grammar, one check, and one ir probe."""
        with open(_EXAMPLE_PROBE_SET) as f:
            text = f.read()
        probes = pcc.load_probe_set(text)
        kinds = {p.probe_kind for p in probes}
        self.assertEqual(kinds, {"grammar", "check", "ir"})


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
        """grammar: 'Failed to load language' in stderr → harness-error sentinel."""
        run = self._run(1, stderr="Failed to load language: reify")
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
        self.assertEqual(result.exit_code, 0)

    def test_result_has_stdout(self):
        """Result must carry the stdout from the captured run."""
        probe = self._make_check_probe("present")
        result = pcc.evaluate(
            probe,
            runner=self._stub_runner(0, stdout="All constraints satisfied."),
        )
        self.assertEqual(result.stdout, "All constraints satisfied.")

    def test_result_has_stderr(self):
        """Result must carry the stderr from the captured run."""
        probe = self._make_ir_probe("absent")
        result = pcc.evaluate(
            probe,
            runner=self._stub_runner(1, stderr="error: unresolved type: Transform3"),
        )
        self.assertIn("unresolved type", result.stderr)

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


if __name__ == "__main__":
    unittest.main()
