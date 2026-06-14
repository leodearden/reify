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


if __name__ == "__main__":
    unittest.main()
