#!/usr/bin/env python3
"""
test_prd_capability_check.py — stdlib unittest for scripts/prd-capability-check.py.

Loads the hyphenated prd-capability-check.py via importlib (the jobserver pattern)
since the filename is not importable by name.  Exercises all pure functions and the
CLI main() in hermetic golden tests — real subprocess probes are skip-guarded.
"""

import importlib.util
import io
import json
import os
import shlex
import shutil
import sys
import tempfile
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
        """check → [reify, check, fixture]"""
        probe = self._make_probe(
            "check", "tests/prd-gate/fixtures/revolute_silent_accept.ri"
        )
        with unittest.mock.patch.dict(os.environ, {"REIFY_BIN": "reify"}):
            cmd = pcc.build_command(probe)
        self.assertEqual(
            cmd,
            ["reify", "check", "tests/prd-gate/fixtures/revolute_silent_accept.ri"],
        )

    def test_build_command_ir_shape(self):
        """ir → [reify, eval, fixture]"""
        probe = self._make_probe("ir", "tests/prd-gate/fixtures/ir_clean_eval.ri")
        with unittest.mock.patch.dict(os.environ, {"REIFY_BIN": "reify"}):
            cmd = pcc.build_command(probe)
        self.assertEqual(
            cmd,
            ["reify", "eval", "tests/prd-gate/fixtures/ir_clean_eval.ri"],
        )

    def test_build_command_grammar_shape(self):
        """grammar → [tree-sitter, parse, --quiet, fixture]"""
        probe = self._make_probe(
            "grammar", "tests/prd-gate/fixtures/arrow_type.ri"
        )
        with unittest.mock.patch.dict(os.environ, {"TREE_SITTER_BIN": "tree-sitter"}):
            cmd = pcc.build_command(probe)
        self.assertEqual(
            cmd,
            ["tree-sitter", "parse", "--quiet",
             "tests/prd-gate/fixtures/arrow_type.ri"],
        )

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


# ---------------------------------------------------------------------------
# step-13 (RED): main(argv) integration + skip-guarded real e2e
# ---------------------------------------------------------------------------

# Helper: parser.c path for the skip-guard
_TS_GRAMMAR_PARSER = os.path.join(_REPO_ROOT, "tree-sitter-reify", "src", "parser.c")
_REIFY_RELEASE = os.path.join(_REPO_ROOT, "target", "release", "reify")
_REIFY_DEBUG = os.path.join(_REPO_ROOT, "target", "debug", "reify")
_REIFY_BUILT = os.path.isfile(_REIFY_RELEASE) or os.path.isfile(_REIFY_DEBUG)
_TS_GRAMMAR_AVAILABLE = os.path.isfile(_TS_GRAMMAR_PARSER)


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

    @unittest.skipUnless(
        _TS_GRAMMAR_AVAILABLE,
        "tree-sitter-reify/src/parser.c not found; skip grammar e2e",
    )
    def test_e2e_arrow_type_grammar_is_fail(self):
        """Real tree-sitter parse: arrow_type.ri with expected present → FAIL (exit 1, stable)."""
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


if __name__ == "__main__":
    unittest.main()
