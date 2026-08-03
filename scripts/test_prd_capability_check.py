#!/usr/bin/env python3
"""
test_prd_capability_check.py — stdlib unittest for scripts/prd-capability-check.py.

Loads the hyphenated prd-capability-check.py via importlib (the jobserver pattern)
since the filename is not importable by name.  Exercises all pure functions and the
CLI main() in hermetic golden tests — real subprocess probes are skip-guarded.
"""

import functools
import importlib.util
import io
import json
import os
import shlex
import shutil
import subprocess
import sys
import tempfile
import textwrap
import time
import unittest
import unittest.mock
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

    def test_load_rejects_non_list_probes(self):
        """load_probe_set raises ValueError when 'probes' is not a list (e.g. an int)."""
        text = json.dumps({"probes": 42})
        with self.assertRaises(ValueError):
            pcc.load_probe_set(text)

    def test_load_rejects_non_dict_probe_items(self):
        """load_probe_set raises ValueError when probe items are not dicts."""
        text = json.dumps({"probes": [1, "string_item"]})
        with self.assertRaises(ValueError):
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
            exit_code=0,
            stdout="",
            stderr="",
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

    def test_binary_not_found_sentinel_flows_to_exit_70(self):
        """_BINARY_NOT_FOUND_SENTINEL in stderr → observe → HARNESS_ERROR verdict → exit 70."""
        probe = pcc.Probe(
            capability="sentinel-flow-test",
            probe_kind="check",
            fixture="some/fixture.ri",
            expected={"observation": "present", "match": {}},
        )

        def sentinel_runner(p):
            return pcc.ProbeRun(
                exit_code=127,
                stdout="",
                stderr=pcc._BINARY_NOT_FOUND_SENTINEL + ": [Errno 2] No such file or directory",
            )

        result = pcc.evaluate(probe, runner=sentinel_runner)
        self.assertEqual(result.verdict, pcc._HARNESS_ERROR)
        self.assertEqual(pcc.harness_exit_code([result]), 70)


# ---------------------------------------------------------------------------
# step-13 (RED): main(argv) integration + skip-guarded real e2e
# ---------------------------------------------------------------------------

# Helper: parser.c path for the skip-guard
_TS_GRAMMAR_PARSER = os.path.join(_REPO_ROOT, "tree-sitter-reify", "src", "parser.c")
_REIFY_RELEASE = os.path.join(_REPO_ROOT, "target", "release", "reify")
_REIFY_DEBUG = os.path.join(_REPO_ROOT, "target", "debug", "reify")
_REIFY_BUILT = os.path.isfile(_REIFY_RELEASE) or os.path.isfile(_REIFY_DEBUG)

# The grammar e2e needs tree-sitter to actually LOAD the reify grammar, which
# isfile(parser.c) does not establish: in a sandboxed agent role the grammar is
# generated but tree-sitter cannot write ~/.cache/tree-sitter/lock/, so the probe
# fails to load the language, observe() classifies it _HARNESS_ERROR, and the
# suite reported a spurious FAIL (exit 70) instead of a clean skip.
#
# Order matters.  isfile() runs first and short-circuits, so a lane that never
# generated the grammar pays no subprocess; only when parser.c exists do we spend
# one probe asking whether it can be loaded.
def _compute_grammar_e2e_status() -> tuple:
    """Return (available, reason) for the grammar e2e.  Never raises.

    The bare `except Exception` is deliberate: the only sane failure mode here is
    "cannot confirm the substrate → skip the one test that needs it".  The
    alternative, measured on this branch, is losing every test in the module.
    It is defence-in-depth, not the fix — the trigger is fixed at source in
    run_probe(), which represents an unlaunchable binary rather than raising.
    """
    if not os.path.isfile(_TS_GRAMMAR_PARSER):
        return (False, f"{_TS_GRAMMAR_PARSER} not found — grammar never generated")
    try:
        usable, reason = pcc.grammar_substrate_usable()
    except Exception as exc:
        return (False, f"the grammar substrate could not be interrogated ({exc!r})")
    return (bool(usable), reason)


@functools.lru_cache(maxsize=1)
def _grammar_e2e_status() -> tuple:
    """Memoized _compute_grammar_e2e_status(), evaluated on FIRST USE.

    Deliberately not a module-level constant: the behavioural half spawns a real
    tree-sitter (on a cold cache, a cc compile of the grammar first), so as an
    import-time constant every invocation of this module paid it — including the
    ~140 hermetic tests that need none of it.
    """
    return _compute_grammar_e2e_status()


def _require_grammar_substrate(test) -> None:
    """skipTest unless a grammar probe can run here.  Call from a test body.

    Carries the MEASURED reason into the skip message: an unusable verdict caused
    by tree-sitter exceeding _SUBSTRATE_PROBE_TIMEOUT_S on a loaded machine is a
    coverage hole, not a real gap, and must be tellable from a genuine denial.
    """
    available, reason = _grammar_e2e_status()
    if not available:
        test.skipTest(f"grammar substrate unavailable — {reason}; skip grammar e2e")


# ---------------------------------------------------------------------------
# 5894 step-13 (RED): the skip-guard must never take the suite down with it
# ---------------------------------------------------------------------------

class TestGrammarAvailabilityGuard(unittest.TestCase):
    """Pins _compute_grammar_e2e_status() — the skip-guard's blast radius.

    Shared rationale.  The guard was a pure isfile() and could not raise; making
    it behavioural gave it a way to fail, and any exception escaping it loses the
    whole suite instead of one test — strictly worse than the spurious
    HARNESS_ERROR this task removes.  The house rule
    (tests/infra/test_prd_gate_corpus.sh:52-60) is that a toolchain which cannot
    be interrogated is a clean SKIP, so these hold the guard to it for ANY
    exception.  Tested through the UN-memoized function, so each case observes
    its own patches.
    """

    @staticmethod
    def _patch_parser_c(exists: bool):
        """Patch os.path.isfile so parser.c reports `exists`; other paths are real.

        Keeps every case independent of whether this lane happened to generate
        the grammar, without blinding unrelated isfile() calls.
        """
        real_isfile = os.path.isfile

        def fake_isfile(path):
            if path == _TS_GRAMMAR_PARSER:
                return exists
            return real_isfile(path)

        return unittest.mock.patch("os.path.isfile", side_effect=fake_isfile)

    def test_substrate_probe_exception_degrades_to_unavailable(self):
        """(a) ANY exception from the substrate probe → unavailable, never propagated."""
        with self._patch_parser_c(True), \
             unittest.mock.patch.object(
                 pcc, "grammar_substrate_usable",
                 side_effect=RuntimeError("boom")):
            available, reason = _compute_grammar_e2e_status()
        self.assertFalse(
            available,
            "a substrate that cannot be interrogated must degrade to 'skip the "
            "grammar e2e', not take the module down",
        )
        self.assertIn(
            "boom", reason,
            "the swallowed exception must survive into the skip message; a "
            "silent 'unavailable' is the coverage hole this guard trades for",
        )

    def test_usable_substrate_with_parser_c_is_available(self):
        """(b) parser.c present + substrate usable → available.

        The hardening must not silently disable the e2e on a healthy lane; that
        would be a coverage loss disguised as a fix.
        """
        with self._patch_parser_c(True), \
             unittest.mock.patch.object(
                 pcc, "grammar_substrate_usable", return_value=(True, "")):
            available, _ = _compute_grammar_e2e_status()
        self.assertTrue(available)

    def test_missing_parser_c_short_circuits_without_probing(self):
        """(c) No parser.c → unavailable, and the substrate is never probed.

        Pins the existing cheap short-circuit: a lane that never generated the
        grammar must pay no subprocess.  Asserted via call_count so a refactor
        cannot quietly reorder the conjunction.
        """
        probe = unittest.mock.Mock(return_value=(True, ""))
        with self._patch_parser_c(False), \
             unittest.mock.patch.object(pcc, "grammar_substrate_usable", probe):
            available, reason = _compute_grammar_e2e_status()
        self.assertFalse(available)
        self.assertEqual(
            probe.call_count, 0,
            "isfile(parser.c) must short-circuit before any subprocess is spent",
        )
        self.assertIn("parser.c", reason, "the skip message must name what is missing")

    def test_hermetic_import_spawns_no_substrate_probe(self):
        """The behavioural half is LAZY — importing this module must not probe.

        Read from a snapshot taken at the END of the module body, so the pin is
        on import itself and survives whatever tests have run by now.
        """
        self.assertEqual(
            _GRAMMAR_STATUS_CALLS_AT_IMPORT, 0,
            "_grammar_e2e_status() must not be evaluated at import time; only "
            "the grammar e2e should ever spend the subprocess",
        )


class TestMain(unittest.TestCase):
    """Tests for main(argv) integration — hermetic + skip-guarded real e2e.

    Most tests FAIL until step-14 implements main() properly (currently a stub
    that returns 64 for any valid probe-set path).
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

    def _make_runner(self, by_kind):
        """Stub runner that dispatches by probe.probe_kind."""
        def runner(probe: Any) -> Any:
            exit_code, stdout, stderr = by_kind[probe.probe_kind]
            return pcc.ProbeRun(exit_code=exit_code, stdout=stdout, stderr=stderr)
        return runner

    def _all_pass_runner(self):
        """Runner that causes all three probe kinds in example-probe-set.json to PASS.

        example-probe-set.json probes:
          grammar: expected present → need exit 0 (PRESENT)
          check:   expected present, match {exit_code: 1} → need exit 1 (match → PRESENT)
          ir:      expected absent,  match {stderr_contains: 'EvalError'} → need exit 0 (ABSENT)
        """
        return self._make_runner({
            "grammar": (0, "", ""),
            "check":   (1, "", "rejection: bad arg"),
            "ir":      (0, "a = 0.01 m", ""),
        })

    def _check_fail_runner(self):
        """Runner that makes the check probe FAIL (reify silent-accept)."""
        return self._make_runner({
            "grammar": (0, "", ""),
            "check":   (0, "All constraints satisfied.", ""),  # exit 0 → no rejection → ABSENT
            "ir":      (0, "a = 0.01 m", ""),
        })

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
        """Run main(["--grammar-substrate-status"]) against a TREE_SITTER_BIN stub."""
        with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}):
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
        with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}):
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
             unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}):
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
            "check":   (1, "", "rejection: bad arg"),
            "ir":      (0, "a = 0.01 m", ""),
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
            "check":   (1, "", "rejection: bad arg"),
            "ir":      (0, "a = 0.01 m", ""),
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
            "check":   (1, "", "rejection: bad arg"),
            "ir":      (0, "a = 0.01 m", ""),
        })
        rc, out, _ = self._run_main_capturing([str(_EXAMPLE_PROBE_SET)], runner=runner)
        self.assertEqual(rc, 70, "precondition: the grammar probe is a HARNESS_ERROR")
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
            "check":   (1, "", "rejection: bad arg"),
            "ir":      (0, "a = 0.01 m", ""),
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
            "check":   (1, "", "rejection: bad arg"),
            "ir":      (0, "a = 0.01 m", ""),
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
            "grammar": (0, "", ""),
            "check":   (1, "", "rejection: " + ("x" * 250) + long_tail),
            "ir":      (0, "a = 0.01 m", ""),
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
        path = os.path.join(self._tmpdir, f"ts_stub{self._stub_idx}")
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
# 5894 step-1 (RED): grammar_cache_denied() — narrow cache-denial predicate
# ---------------------------------------------------------------------------

# The exact stderr tree-sitter 0.26.8 emits when the grammar loads but its
# on-disk lock/cache directory is not writable.  Measured in a sandboxed agent
# role (`touch ~/.cache/tree-sitter/lock/_probe` → EACCES while the directory is
# drwxrwxr-x owned by the invoking user — the denial is a landlock LSM hook, not
# a mode bit, which is precisely why an os.access(..., W_OK) probe cannot see it).
_CACHE_DENIED_STDERR = (
    'Error: Failed to load language for path "/home/leo/src/reify/tests/prd-gate/'
    'fixtures/arrow_type.ri"\n'
    "Caused by: Failed to load language in current directory:\n"
    "Permission denied (os error 13) "
    "(/home/leo/.cache/tree-sitter/lock/reify-69604127a681544d.lock)\n"
)


# ---------------------------------------------------------------------------
# TREE_SITTER_BIN stub factories (module-level so both the grammar_substrate_usable()
# unit tests and the --grammar-substrate-status CLI tests in TestMain share one
# definition of "what a denied / clean / rejecting tree-sitter looks like").
#
# These are forward-referenced from TestMain, which is defined earlier in the
# file — module-level names resolve at call time, so the ordering is fine and the
# stubs stay next to the measured stderr signature they encode.
# ---------------------------------------------------------------------------

def _write_ts_stub(tmpdir: str, name: str, body: str) -> str:
    """Write an executable /bin/sh stub at <tmpdir>/<name> and return its path."""
    path = os.path.join(tmpdir, name)
    with open(path, "w") as f:
        f.write("#!/bin/sh\n" + textwrap.dedent(body))
    os.chmod(path, 0o755)
    return path


def _ts_stub_cache_denied(tmpdir: str, name: str = "ts_stub_denied") -> str:
    """Stub reproducing the measured cache-denial: stderr signature, exit 1.

    The stderr text is written to a sibling file and cat'd rather than embedded
    in a heredoc: the measured signature is unindented, so textwrap.dedent would
    find no common prefix, silently leave the stub body indented, and the heredoc
    terminator would never match.
    """
    stderr_file = os.path.join(tmpdir, name + "_stderr.txt")
    with open(stderr_file, "w") as f:
        f.write(_CACHE_DENIED_STDERR)
    return _write_ts_stub(tmpdir, name, f"""\
        cat {shlex.quote(stderr_file)} >&2
        exit 1
    """)


def _ts_stub_parse_error(tmpdir: str, name: str = "ts_stub_parse_error") -> str:
    """Stub reproducing an ordinary grammar rejection: parse error, exit 1."""
    return _write_ts_stub(tmpdir, name, """\
        echo 'x.ri\t0 ms\t(ERROR [0, 0] - [3, 0])' >&2
        exit 1
    """)


def _ts_stub_clean(tmpdir: str, name: str = "ts_stub_clean") -> str:
    """Stub reproducing a clean parse: exit 0, no output."""
    return _write_ts_stub(tmpdir, name, "exit 0\n")


def _ts_stub_hangs(tmpdir: str, name: str = "ts_stub_hangs", seconds: int = 5) -> str:
    """Stub that never answers within the test's patched timeout.

    Models a tree-sitter wedged on ~/.cache/tree-sitter/lock/<grammar>.lock.
    Tests pair it with a sub-second _SUBSTRATE_PROBE_TIMEOUT_S, so the sleep only
    has to outlast that bound.
    """
    return _write_ts_stub(tmpdir, name, f"sleep {seconds}\n")


def _ts_stub_not_executable(tmpdir: str, name: str = "ts_stub_not_exec") -> str:
    """Stub that EXISTS on disk but carries no execute bit.

    Deliberately NOT the missing-CLI case: every isfile()/exists() guard upstream
    passes and only the exec fails (EACCES).  Callers assert os.path.isfile() on
    the returned path so it cannot silently degenerate into that case.
    """
    path = _write_ts_stub(tmpdir, name, "exit 0\n")
    os.chmod(path, 0o644)
    return path


class TestGrammarCacheDenied(unittest.TestCase):
    """Pins pcc.grammar_cache_denied(run) -> bool.  Hermetic: no subprocess.

    Shared rationale for every case below.  The predicate is deliberately NARROW,
    requiring a grammar *load* failure AND a permission denial simultaneously.
    The negatives are the load-bearing half: they prove it can absorb neither a
    genuine grammar regression (a *parse* error, exit 1 with no load failure) nor
    a non-permission load failure.  Both must keep reaching _HARNESS_ERROR/exit
    70 — the skip this predicate authorizes happens in the CALLERS, never in the
    probe verdict.
    """

    @staticmethod
    def _run(exit_code, stderr, stdout=""):
        return pcc.ProbeRun(exit_code=exit_code, stdout=stdout, stderr=stderr)

    # ── positives ─────────────────────────────────────────────────────────────

    def test_measured_sandbox_signature_is_denied(self):
        """(a) The verbatim measured sandbox stderr → True."""
        self.assertTrue(
            pcc.grammar_cache_denied(self._run(1, _CACHE_DENIED_STDERR)),
            "the measured sandbox cache-denial signature must be recognised",
        )

    def test_os_error_13_spelling_alone_is_denied(self):
        """(b) A denial spelled only 'os error 13' (no 'Permission denied') → True.

        tree-sitter surfaces the errno either way depending on which layer
        reports it; keying on only one spelling would make the guard flaky.
        """
        stderr = (
            "Error: Failed to load language for path \"x.ri\"\n"
            "Caused by: os error 13\n"
        )
        self.assertTrue(pcc.grammar_cache_denied(self._run(1, stderr)))

    # ── negatives (the narrowness guards) ─────────────────────────────────────

    def test_plain_parse_failure_is_not_denied(self):
        """(c) A real grammar rejection (exit 1, no load failure) → False.

        The ABSENT path.  Swallowing it would silently green a probe that
        legitimately FAILs — the risk this predicate is narrowed against.
        """
        stderr = "x.ri\t0 ms\t(ERROR [0, 0] - [3, 0])\n"
        self.assertFalse(pcc.grammar_cache_denied(self._run(1, stderr)))

    def test_successful_run_is_not_denied(self):
        """(d) exit 0 with empty stderr → False."""
        self.assertFalse(pcc.grammar_cache_denied(self._run(0, "")))

    def test_load_failure_without_permission_indicator_is_not_denied(self):
        """(e) A load failure with NO permission indicator → False.

        A broken or absent grammar must still reach HARNESS_ERROR (70).  Fails
        first if the predicate is widened to 'any load failure'.
        """
        stderr = (
            "Error: Failed to load language for path \"x.ri\"\n"
            "Caused by: No language found for path\n"
        )
        self.assertFalse(pcc.grammar_cache_denied(self._run(1, stderr)))

    def test_permission_denial_without_load_failure_is_not_denied(self):
        """A permission error unrelated to grammar loading → False.

        Both halves are required.  An EACCES reading the *fixture* is a real
        harness error the operator needs to see, not a grammar-substrate skip.
        """
        stderr = "Error: Permission denied (os error 13) (x.ri)\n"
        self.assertFalse(pcc.grammar_cache_denied(self._run(1, stderr)))

    # ── the two classifiers must agree about a load failure ───────────────────

    def test_denied_run_is_also_a_harness_error_to_observe(self):
        """Anything this predicate calls a denial must also be HARNESS_ERROR.

        Stated over behaviour rather than over which constant a branch reads.
        Were the two classifiers to disagree, a harness error would be promoted
        into a real PASS/FAIL verdict — the mis-attribution this task removes.
        """
        for stderr in (
            _CACHE_DENIED_STDERR,
            "Error: Failed to load language in current directory:\n"
            "Permission denied (os error 13) (~/.cache/tree-sitter/lock/x.lock)\n",
        ):
            with self.subTest(stderr=stderr[:40]):
                run = self._run(1, stderr)
                self.assertTrue(
                    pcc.grammar_cache_denied(run),
                    "precondition: this is a recognised cache denial",
                )
                self.assertEqual(
                    pcc.observe("grammar", run, {}), pcc._HARNESS_ERROR,
                    "a run classified as a cache denial must never reach a "
                    "PRESENT/ABSENT observation",
                )


# ---------------------------------------------------------------------------
# 5894 step-3 (RED): grammar_substrate_usable() — behavioural substrate probe
# ---------------------------------------------------------------------------

class TestGrammarSubstrateUsable(unittest.TestCase):
    """Pins pcc.grammar_substrate_usable() -> (bool, reason).

    Shared rationale.  Every case is driven by a TREE_SITTER_BIN stub, never by
    ambient sandbox state — load-bearing, not tidiness: whether
    ~/.cache/tree-sitter/lock is writable depends on which agent role runs the
    suite, so an ambient-conditioned test would flip RED/GREEN by role and could
    not be turned green by the role that has to fix it.
    """

    def setUp(self):
        self._tmpdir = tempfile.mkdtemp(prefix="prd_gate_substrate_test_")

    def tearDown(self):
        shutil.rmtree(self._tmpdir, ignore_errors=True)

    def _cache_denied_stub(self):
        return _ts_stub_cache_denied(self._tmpdir)

    def _parse_error_stub(self):
        return _ts_stub_parse_error(self._tmpdir)

    def _clean_stub(self):
        return _ts_stub_clean(self._tmpdir)

    def _not_executable_stub(self):
        return _ts_stub_not_executable(self._tmpdir)

    @staticmethod
    def _call(stub):
        with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": stub}):
            return pcc.grammar_substrate_usable()

    # ── (a) cache denial → unusable, with an actionable reason ────────────────

    def test_cache_denial_is_unusable_with_reason(self):
        """Cache-denial stderr → (False, reason) naming the tree-sitter cache."""
        usable, reason = self._call(self._cache_denied_stub())
        self.assertFalse(usable, "a denied grammar cache is an unusable substrate")
        self.assertTrue(reason, "an unusable substrate must explain itself")
        # The reason is printed verbatim as a SKIP line, so it has to name the
        # subsystem — "permission denied" alone is what made the original
        # failure cost several probes to attribute.
        lowered = reason.lower()
        self.assertIn("tree-sitter", lowered)
        self.assertIn("cache", lowered)

    # ── (b) a parse error is NOT a broken substrate ───────────────────────────

    def test_parse_error_is_usable(self):
        """An ordinary parse rejection → (True, "").

        Reading an unparseable fixture as a broken substrate would skip the e2e
        precisely when the grammar works — the inverse of the bug being fixed.
        """
        self.assertEqual(self._call(self._parse_error_stub()), (True, ""))

    # ── (c) clean run → usable ────────────────────────────────────────────────

    def test_clean_run_is_usable(self):
        """exit 0 → (True, "")."""
        self.assertEqual(self._call(self._clean_stub()), (True, ""))

    # ── (d) missing CLI → unusable ────────────────────────────────────────────

    def test_missing_cli_is_unusable_with_reason(self):
        """TREE_SITTER_BIN pointing at nothing → (False, reason) naming the CLI.

        run_probe() turns the FileNotFoundError into _BINARY_NOT_FOUND_SENTINEL,
        so this arrives through the same channel as the denial and must be
        distinguished by the reason text, not by the boolean.
        """
        missing = os.path.join(self._tmpdir, "definitely-not-here")
        self.assertFalse(os.path.exists(missing), "precondition: stub must not exist")
        usable, reason = self._call(missing)
        self.assertFalse(usable, "a missing tree-sitter CLI is an unusable substrate")
        self.assertTrue(reason, "an unusable substrate must explain itself")
        self.assertIn("tree-sitter", reason.lower())

    def test_launch_failure_reason_carries_the_offending_path(self):
        """The launch-failure reason must name WHAT could not be launched.

        One sentinel covers every launch OSError, so a missing grammar cwd raises
        the same FileNotFoundError as a missing CLI.  Only the errno detail
        distinguishes them, and it must survive into the SKIP line.
        """
        missing = os.path.join(self._tmpdir, "definitely-not-here")
        _, cli_reason = self._call(missing)
        self.assertIn(
            missing, cli_reason,
            "the errno detail naming the missing CLI must reach the reason",
        )

        # Same sentinel, different subsystem: an absent grammar directory.
        with unittest.mock.patch.object(
            pcc, "_find_repo_root", return_value=os.path.join(self._tmpdir, "no-repo")
        ):
            usable, cwd_reason = self._call(self._clean_stub())
        self.assertFalse(usable, "an absent grammar directory is an unusable substrate")
        self.assertIn(
            "tree-sitter-reify", cwd_reason,
            "an absent grammar directory must be attributable from the reason, "
            "not silently reported as a missing CLI",
        )

    # ── (e) present-but-unlaunchable CLI → unusable, never a raise ────────────

    def test_non_executable_cli_is_unusable_with_reason(self):
        """A CLI that exists but cannot be launched → (False, reason), not a raise.

        Until run_probe() represented an EACCES launch the way it represents
        ENOENT, the PermissionError escaped instead.  Returning normally is as
        load-bearing as the boolean — the skip-guard cannot recover from a raise.
        """
        stub = self._not_executable_stub()
        self.assertTrue(
            os.path.isfile(stub),
            "precondition: the stub must EXIST, else this degenerates into "
            "test_missing_cli_is_unusable_with_reason",
        )
        usable, reason = self._call(stub)
        self.assertFalse(
            usable,
            "a tree-sitter CLI that cannot be launched is an unusable substrate",
        )
        self.assertTrue(reason, "an unusable substrate must explain itself")
        self.assertIn("tree-sitter", reason.lower())

    # ── (f) a wedged CLI → unusable, and BOUNDED ──────────────────────────────

    def test_hanging_cli_is_unusable_within_the_timeout(self):
        """A tree-sitter that never answers → (False, reason), and it returns.

        The failure mode the behavioural guard introduced: the old isfile() guard
        structurally could not block.  Unbounded, a tree-sitter wedged on the very
        grammar lock this task is about would hang the suite rather than skip one
        test, and the exception-swallowing guard cannot help — a hang raises
        nothing.  Bounded to 0.4s here; production uses
        _SUBSTRATE_PROBE_TIMEOUT_S, generous for cold-cache compiles.
        """
        stub = _ts_stub_hangs(self._tmpdir)
        with unittest.mock.patch.object(pcc, "_SUBSTRATE_PROBE_TIMEOUT_S", 0.4):
            started = time.monotonic()
            usable, reason = self._call(stub)
            elapsed = time.monotonic() - started
        self.assertFalse(usable, "a tree-sitter that will not answer is unusable")
        self.assertLess(
            elapsed, 10.0,
            "the substrate probe must be time-bounded; unbounded, a wedged "
            "tree-sitter hangs the suite at import instead of skipping one test",
        )
        self.assertTrue(reason, "an unusable substrate must explain itself")
        self.assertIn("tree-sitter", reason.lower())

    def test_timeout_is_a_harness_error_not_an_absent_observation(self):
        """A timed-out probe must never read as a real observation.

        subprocess.TimeoutExpired derives from SubprocessError, not OSError, so
        run_probe()'s launch-failure catch cannot cover it; if the sentinel it
        substitutes were not classified, an exit 124 would fall through
        observe()'s grammar branch as an ordinary non-zero exit.
        """
        with unittest.mock.patch.dict(
            os.environ, {"TREE_SITTER_BIN": _ts_stub_hangs(self._tmpdir)}
        ):
            run = pcc.run_probe(
                pcc.Probe(
                    capability="wedged",
                    probe_kind="grammar",
                    fixture=pcc._SUBSTRATE_PROBE_FIXTURE,
                    expected={"observation": "present", "match": {}},
                ),
                timeout=0.4,
            )
        self.assertIn(pcc._PROBE_TIMEOUT_SENTINEL, run.stderr)
        self.assertEqual(
            pcc.observe("grammar", run, {}), pcc._HARNESS_ERROR,
            "a probe that never finished cannot be trusted as PRESENT/ABSENT",
        )

    def test_probe_set_runs_stay_unbounded(self):
        """run_probe() defaults to no timeout, so gate verdicts are unchanged.

        A bound on the ordinary probe path would manufacture a HARNESS_ERROR out
        of a slow machine — `reify eval` on a heavy fixture may legitimately run
        long.  The bound is opt-in, for callers that cannot afford to block.
        """
        calls = []

        def fake_run(*args, **kwargs):
            calls.append(kwargs)
            return subprocess.CompletedProcess(args=args, returncode=0, stdout="", stderr="")

        probe = pcc.Probe(
            capability="unbounded",
            probe_kind="grammar",
            fixture=pcc._SUBSTRATE_PROBE_FIXTURE,
            expected={"observation": "present", "match": {}},
        )
        with unittest.mock.patch.object(subprocess, "run", side_effect=fake_run):
            pcc.run_probe(probe)
        # Assert the launch HAPPENED before asserting what it was passed: read
        # through .get() with a None default, this passes vacuously if
        # subprocess.run is never reached at all (build_command raising, or a
        # future short-circuit), silently stopping guarding the thing it exists for.
        self.assertEqual(len(calls), 1, "run_probe() must have launched exactly once")
        captured = calls[0]
        self.assertIn(
            "timeout", captured,
            "run_probe() must pass `timeout` explicitly, so the default is "
            "visible at the call site rather than inherited from subprocess",
        )
        self.assertIsNone(
            captured["timeout"],
            "run_probe() must stay unbounded by default; only callers that ask "
            "for a bound get one",
        )

    # ── shape ─────────────────────────────────────────────────────────────────

    def test_returns_bool_and_str_pair(self):
        """The return is a (bool, str) 2-tuple in both directions."""
        for stub in (self._clean_stub(), self._cache_denied_stub()):
            usable, reason = self._call(stub)
            self.assertIsInstance(usable, bool)
            self.assertIsInstance(reason, str)


# Taken after the whole module body has executed, so it records exactly what
# IMPORT cost — read by TestGrammarAvailabilityGuard to pin that the substrate
# probe stayed lazy.  Must remain the last statement before the entry-point.
_info = _grammar_e2e_status.cache_info()
_GRAMMAR_STATUS_CALLS_AT_IMPORT = _info.hits + _info.misses


if __name__ == "__main__":
    unittest.main()
