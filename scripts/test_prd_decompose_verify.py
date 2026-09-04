#!/usr/bin/env python3
"""
test_prd_decompose_verify.py — stdlib unittest for scripts/prd-decompose-verify.py.

Loads the hyphenated prd-decompose-verify.py via importlib (the same pattern as
test_prd_capability_check.py) since the filename is not importable by name.
Exercises all pure functions and the CLI main() in hermetic golden tests —
real subprocess probes are skip-guarded on a built reify binary.

Test classes are added incrementally per TDD step:
  TestScaffold              — prereq-1 (importability)
  TestPremiseToProbe        — step-01 RED / step-02 GREEN
  TestBindPremises          — step-03 RED / step-04 GREEN
  TestSynthesizeBatch       — step-05 RED / step-06 GREEN
  TestMainCLI               — step-07 RED / step-08 GREEN
  TestMjsSyntaxValidity     — step-09 RED / step-10 GREEN
  TestBoundaryE2e           — step-11 RED / step-12 GREEN
"""

import importlib.util
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
import unittest.mock
from typing import Any

# ---------------------------------------------------------------------------
# Module loaders
# ---------------------------------------------------------------------------

_SCRIPTS_DIR = os.path.dirname(os.path.abspath(__file__))
_HARNESS_PATH = os.path.join(_SCRIPTS_DIR, "prd-decompose-verify.py")
_ALPHA_PATH = os.path.join(_SCRIPTS_DIR, "prd-capability-check.py")

# Load prd-decompose-verify.py as `pdv` (the module under test)
_pdv_spec = importlib.util.spec_from_file_location("prd_decompose_verify", _HARNESS_PATH)
pdv = importlib.util.module_from_spec(_pdv_spec)
sys.modules["prd_decompose_verify"] = pdv
_pdv_spec.loader.exec_module(pdv)

# Load prd-capability-check.py as `pcc` (so tests can use α's types + constants)
# pdv already loaded pcc into sys.modules; grab the registered instance.
_pcc_spec = importlib.util.spec_from_file_location("prd_capability_check", _ALPHA_PATH)
pcc = importlib.util.module_from_spec(_pcc_spec)
if "prd_capability_check" not in sys.modules:
    sys.modules["prd_capability_check"] = pcc
    _pcc_spec.loader.exec_module(pcc)
else:
    pcc = sys.modules["prd_capability_check"]

# ---------------------------------------------------------------------------
# Repo-root helpers for skip-guards
# ---------------------------------------------------------------------------

_REPO_ROOT = os.path.dirname(_SCRIPTS_DIR)
_REIFY_RELEASE = os.path.join(_REPO_ROOT, "target", "release", "reify")
_REIFY_DEBUG = os.path.join(_REPO_ROOT, "target", "debug", "reify")
_REIFY_BUILT = os.path.isfile(_REIFY_RELEASE) or os.path.isfile(_REIFY_DEBUG)

_FIXTURES_DIR = os.path.join(_REPO_ROOT, "tests", "prd-gate", "fixtures")
_LEAF_FALSE = os.path.join(_REPO_ROOT, "tests", "prd-gate", "leaf-false-premise.json")
_LEAF_TRUE = os.path.join(_REPO_ROOT, "tests", "prd-gate", "leaf-true-premise.json")
_PDV_MJS = os.path.join(_SCRIPTS_DIR, "prd-decompose-verify.mjs")


# ---------------------------------------------------------------------------
# prereq-1 / TestScaffold: basic importability
# ---------------------------------------------------------------------------

class TestScaffold(unittest.TestCase):
    """Sanity-check that prd-decompose-verify.py is importable and main() exists."""

    def test_module_importable(self):
        self.assertIsNotNone(pdv)

    def test_main_present(self):
        self.assertTrue(
            hasattr(pdv, "main"),
            "prd-decompose-verify.py must export a main() function",
        )

    def test_main_is_callable(self):
        self.assertTrue(callable(pdv.main))

    def test_alpha_loaded(self):
        """pdv must load α (pcc) in-process so downstream code can reuse it."""
        self.assertIsNotNone(pdv.pcc)
        self.assertTrue(
            hasattr(pdv.pcc, "load_probe_set"),
            "pdv.pcc must expose α's load_probe_set",
        )


# ---------------------------------------------------------------------------
# step-01 (RED): premise_to_probe() binding + negative-assertion polarity
# ---------------------------------------------------------------------------

class TestPremiseToProbe(unittest.TestCase):
    """Tests for Premise dataclass and premise_to_probe() binding rules.

    These tests FAIL until step-02 adds Premise + premise_to_probe.
    """

    # ── helpers ──────────────────────────────────────────────────────────────

    def _premise(self, assertion_kind: str, fixture: str = "tests/prd-gate/fixtures/revolute_silent_accept.ri",
                 match=None, text: str = "test premise", capability: str = "cap") -> Any:
        return pdv.Premise(
            text=text,
            assertion_kind=assertion_kind,
            fixture=fixture,
            match=match if match is not None else {},
            capability=capability,
        )

    def _probe(self, premise) -> dict:
        return pdv.premise_to_probe(premise)

    # ── (1) rejection → check / observation=present (W1 polarity guard) ──────

    def test_rejection_binds_check_probe_kind(self):
        """rejection premise → probe_kind=='check'."""
        p = self._premise("rejection", match={"exit_code": 1, "stderr_contains": "type mismatch"})
        probe = self._probe(p)
        self.assertEqual(probe["probe_kind"], "check")

    def test_rejection_binds_observation_present(self):
        """rejection premise → observation=='present' (NOT 'absent') — W1 polarity guard."""
        p = self._premise("rejection", match={"exit_code": 1, "stderr_contains": "type mismatch"})
        probe = self._probe(p)
        self.assertEqual(probe["expected"]["observation"], "present",
                         "rejection assertion must bind observation='present' so that "
                         "reify NOT rejecting causes ABSENT → FAIL (not ABSENT → PASS)")

    def test_rejection_observation_is_NOT_absent(self):
        """rejection premise must NOT bind observation='absent' — that would be the W1 slip."""
        p = self._premise("rejection", match={"exit_code": 1})
        probe = self._probe(p)
        self.assertNotEqual(probe["expected"]["observation"], "absent",
                            "W1 slip: rejection premise bound 'absent' (would make silent-accept PASS)")

    def test_rejection_match_names_diagnostic(self):
        """rejection premise match is passed through to the probe (names the rejection diagnostic)."""
        p = self._premise("rejection", match={"exit_code": 1, "stderr_contains": "arg type mismatch"})
        probe = self._probe(p)
        self.assertEqual(probe["expected"]["match"]["exit_code"], 1)
        self.assertEqual(probe["expected"]["match"]["stderr_contains"], "arg type mismatch")

    def test_rejection_match_exit_code_only(self):
        """rejection premise with match={exit_code:1} produces probe with that match."""
        p = self._premise("rejection", match={"exit_code": 1})
        probe = self._probe(p)
        self.assertEqual(probe["expected"]["match"]["exit_code"], 1)

    # ── (2) parses → grammar / present ───────────────────────────────────────

    def test_parses_binds_grammar_probe_kind(self):
        """parses premise → probe_kind=='grammar'."""
        p = self._premise("parses", fixture="tests/prd-gate/fixtures/ir_clean_eval.ri", match={})
        probe = self._probe(p)
        self.assertEqual(probe["probe_kind"], "grammar")

    def test_parses_binds_observation_present(self):
        """parses premise → observation=='present'."""
        p = self._premise("parses", fixture="tests/prd-gate/fixtures/ir_clean_eval.ri", match={})
        probe = self._probe(p)
        self.assertEqual(probe["expected"]["observation"], "present")

    # ── (3) resolves → check / present ───────────────────────────────────────

    def test_resolves_binds_check_probe_kind(self):
        """resolves premise → probe_kind=='check'."""
        p = self._premise("resolves", match={"exit_code": 0})
        probe = self._probe(p)
        self.assertEqual(probe["probe_kind"], "check")

    def test_resolves_binds_observation_present(self):
        """resolves premise → observation=='present'."""
        p = self._premise("resolves", match={"exit_code": 0})
        probe = self._probe(p)
        self.assertEqual(probe["expected"]["observation"], "present")

    # ── (4) produces/IR → ir / present with asserted stderr signature ─────────

    def test_produces_binds_ir_probe_kind(self):
        """produces premise → probe_kind=='ir'."""
        p = self._premise("produces", match={"stderr_contains": "CrossSubGeometryRef"})
        probe = self._probe(p)
        self.assertEqual(probe["probe_kind"], "ir")

    def test_produces_binds_observation_present(self):
        """produces premise → observation=='present'."""
        p = self._premise("produces", match={"stderr_contains": "CrossSubGeometryRef"})
        probe = self._probe(p)
        self.assertEqual(probe["expected"]["observation"], "present")

    def test_produces_match_contains_stderr_signature(self):
        """produces premise match (stderr_contains) is passed through to probe."""
        p = self._premise("produces", match={"stderr_contains": "CrossSubGeometryRef"})
        probe = self._probe(p)
        self.assertEqual(probe["expected"]["match"]["stderr_contains"], "CrossSubGeometryRef")

    # ── (5) ir (clean-eval) → ir / absent ────────────────────────────────────

    def test_ir_assertion_binds_ir_probe_kind(self):
        """ir assertion_kind → probe_kind=='ir'."""
        p = self._premise("ir", fixture="tests/prd-gate/fixtures/ir_clean_eval.ri",
                          match={"stderr_contains": "EvalError"})
        probe = self._probe(p)
        self.assertEqual(probe["probe_kind"], "ir")

    def test_ir_assertion_binds_observation_absent(self):
        """ir assertion_kind (clean-eval) → observation=='absent'."""
        p = self._premise("ir", fixture="tests/prd-gate/fixtures/ir_clean_eval.ri",
                          match={"stderr_contains": "EvalError"})
        probe = self._probe(p)
        self.assertEqual(probe["expected"]["observation"], "absent")

    # ── (6) fixture path is preserved ────────────────────────────────────────

    def test_fixture_path_is_preserved_rejection(self):
        """rejection probe carries the premise's fixture path."""
        fixture = "tests/prd-gate/fixtures/revolute_silent_accept.ri"
        p = self._premise("rejection", fixture=fixture, match={"exit_code": 1})
        probe = self._probe(p)
        self.assertEqual(probe["fixture"], fixture)

    def test_fixture_path_is_preserved_parses(self):
        """parses probe carries the premise's fixture path."""
        fixture = "tests/prd-gate/fixtures/ir_clean_eval.ri"
        p = self._premise("parses", fixture=fixture, match={})
        probe = self._probe(p)
        self.assertEqual(probe["fixture"], fixture)

    def test_fixture_path_is_preserved_produces(self):
        """produces probe carries the premise's fixture path."""
        fixture = "tests/prd-gate/fixtures/ir_clean_eval.ri"
        p = self._premise("produces", fixture=fixture, match={"stderr_contains": "EvalError"})
        probe = self._probe(p)
        self.assertEqual(probe["fixture"], fixture)

    # ── (7) capability field ──────────────────────────────────────────────────

    def test_capability_is_included_in_probe(self):
        """probe dict includes 'capability' field (from premise.capability)."""
        p = self._premise("rejection", capability="arg-vs-param rejection (4575)",
                          match={"exit_code": 1})
        probe = self._probe(p)
        self.assertIn("capability", probe)
        self.assertEqual(probe["capability"], "arg-vs-param rejection (4575)")

    def test_capability_falls_back_to_text(self):
        """When premise.capability is None, probe['capability'] falls back to premise.text."""
        p = pdv.Premise(
            text="some premise text",
            assertion_kind="parses",
            fixture="tests/prd-gate/fixtures/ir_clean_eval.ri",
            match={},
            capability=None,
        )
        probe = self._probe(p)
        self.assertIn("capability", probe)
        self.assertEqual(probe["capability"], "some premise text")

    # ── (8) probe is α load_probe_set-compatible ──────────────────────────────

    def test_rejection_probe_round_trips_through_alpha(self):
        """premise_to_probe output is accepted by α's load_probe_set."""
        p = self._premise("rejection", match={"exit_code": 1})
        probe = self._probe(p)
        probe_set = json.dumps({"probes": [probe]})
        # Must not raise
        probes = pcc.load_probe_set(probe_set)
        self.assertEqual(len(probes), 1)
        self.assertEqual(probes[0].probe_kind, "check")


# ---------------------------------------------------------------------------
# step-03 (RED): bind_premises() list→probe-set + negative-assertion enforcement
# ---------------------------------------------------------------------------

class TestBindPremises(unittest.TestCase):
    """Tests for bind_premises() list→probe-set conversion.

    These tests FAIL until step-04 implements bind_premises.
    """

    def _make_premises(self):
        return [
            pdv.Premise(text="revolute rejects non-axis", assertion_kind="rejection",
                        fixture="tests/prd-gate/fixtures/revolute_silent_accept.ri",
                        match={"exit_code": 1}, capability="arg-vs-param rejection"),
            pdv.Premise(text="ir_clean_eval parses", assertion_kind="parses",
                        fixture="tests/prd-gate/fixtures/ir_clean_eval.ri",
                        match={}, capability="clean eval grammar"),
            pdv.Premise(text="ir_clean_eval no eval error", assertion_kind="ir",
                        fixture="tests/prd-gate/fixtures/ir_clean_eval.ri",
                        match={"stderr_contains": "EvalError"}, capability="eval-error proxy"),
        ]

    # ── (1) one probe per premise ─────────────────────────────────────────────

    def test_bind_produces_probe_per_premise(self):
        """bind_premises produces one probe per premise."""
        premises = self._make_premises()
        result = pdv.bind_premises(premises)
        self.assertIn("probes", result)
        self.assertEqual(len(result["probes"]), len(premises))

    def test_bind_returns_dict_with_probes_key(self):
        """bind_premises returns a dict with a 'probes' key (α probe-set format)."""
        premises = self._make_premises()
        result = pdv.bind_premises(premises)
        self.assertIsInstance(result, dict)
        self.assertIn("probes", result)
        self.assertIsInstance(result["probes"], list)

    # ── (2) round-trip: bind → load_probe_set ────────────────────────────────

    def test_bind_output_accepted_by_alpha_load_probe_set(self):
        """bind_premises output JSON is accepted by α's load_probe_set."""
        premises = self._make_premises()
        result = pdv.bind_premises(premises)
        probe_set_json = json.dumps(result)
        # Must not raise
        probes = pcc.load_probe_set(probe_set_json)
        self.assertEqual(len(probes), 3)

    def test_bind_probe_kinds_match_assertion_kinds(self):
        """bind_premises maps assertion_kind correctly for all premises in a mixed list."""
        premises = self._make_premises()
        result = pdv.bind_premises(premises)
        kinds = {p["probe_kind"] for p in result["probes"]}
        # rejection → check, parses → grammar, ir → ir
        self.assertIn("check", kinds)
        self.assertIn("grammar", kinds)
        self.assertIn("ir", kinds)

    # ── (3) every rejection premise yields a probe (none dropped) ─────────────

    def test_all_rejection_premises_yield_probes(self):
        """Every rejection premise yields exactly one probe — none are silently dropped."""
        premises = [
            pdv.Premise(text="R1", assertion_kind="rejection",
                        fixture="tests/prd-gate/fixtures/revolute_silent_accept.ri",
                        match={"exit_code": 1}, capability="R1"),
            pdv.Premise(text="R2", assertion_kind="rejection",
                        fixture="tests/prd-gate/fixtures/revolute_silent_accept.ri",
                        match={"exit_code": 1, "stderr_contains": "diag"}, capability="R2"),
        ]
        result = pdv.bind_premises(premises)
        rejection_probes = [p for p in result["probes"] if p["probe_kind"] == "check"
                            and p["expected"]["observation"] == "present"]
        self.assertEqual(len(rejection_probes), 2,
                         "Both rejection premises must yield probes (none dropped)")

    # ── (4) rejection premise missing fixture → error ─────────────────────────

    def test_rejection_missing_fixture_raises(self):
        """A rejection premise with no fixture raises a clear error (gap, not a pass)."""
        p = pdv.Premise(text="missing fixture rejection", assertion_kind="rejection",
                        fixture="", match={"exit_code": 1}, capability="missing")
        with self.assertRaises(Exception) as ctx:
            pdv.bind_premises([p])
        self.assertIn(
            "fixture",
            str(ctx.exception).lower(),
            "Error message must mention 'fixture' for a rejection premise with no fixture path",
        )

    def test_rejection_empty_match_raises(self):
        """A rejection premise with empty match raises — empty match is satisfied
        unconditionally (α's match_predicate returns True for {}), so reify can silently
        accept and the probe still PASSes.  This is the exact 4575 silent-accept class
        bind_premises exists to catch."""
        p = pdv.Premise(text="empty match rejection", assertion_kind="rejection",
                        fixture="tests/prd-gate/fixtures/revolute_silent_accept.ri",
                        match={}, capability="empty-match")
        with self.assertRaises(Exception) as ctx:
            pdv.bind_premises([p])
        self.assertIn(
            "match",
            str(ctx.exception).lower(),
            "Error message must mention 'match' for a rejection premise with empty match dict",
        )

    # ── (5) empty list → empty probe-set ────────────────────────────────────

    def test_empty_premises_gives_empty_probe_set(self):
        """bind_premises([]) → {'probes': []}."""
        result = pdv.bind_premises([])
        self.assertEqual(result, {"probes": []})

    # ── (6) observations are correct for each assertion kind ──────────────────

    def test_rejection_probe_has_observation_present(self):
        """bind_premises sets observation='present' for rejection premises."""
        p = pdv.Premise(text="R", assertion_kind="rejection",
                        fixture="tests/prd-gate/fixtures/revolute_silent_accept.ri",
                        match={"exit_code": 1}, capability="R")
        result = pdv.bind_premises([p])
        probe = result["probes"][0]
        self.assertEqual(probe["expected"]["observation"], "present")

    def test_ir_probe_has_observation_absent(self):
        """bind_premises sets observation='absent' for ir premises."""
        p = pdv.Premise(text="I", assertion_kind="ir",
                        fixture="tests/prd-gate/fixtures/ir_clean_eval.ri",
                        match={"stderr_contains": "EvalError"}, capability="I")
        result = pdv.bind_premises([p])
        probe = result["probes"][0]
        self.assertEqual(probe["expected"]["observation"], "absent")


# ---------------------------------------------------------------------------
# step-05 (RED): synthesize_batch() blocking semantics + captured-output mandate
# ---------------------------------------------------------------------------

class TestSynthesizeBatch(unittest.TestCase):
    """Tests for BatchVerdict + synthesize_batch() blocking semantics.

    Uses synthetic α --json result records.
    These tests FAIL until step-06 implements BatchVerdict + synthesize_batch.
    """

    def _result(self, capability: str, verdict: str,
                exit_code: int = 0, stdout: str = "", stderr: str = "") -> dict:
        """Build a synthetic α --json result record."""
        return {
            "capability": capability,
            "probe_kind": "check",
            "verdict": verdict,
            "command": ["reify", "check", "/fixture.ri"],
            "exit_code": exit_code,
            "stdout": stdout,
            "stderr": stderr,
        }

    # ── (1) all-PASS prover + empty adversary → does not block ───────────────

    def test_all_pass_prover_empty_adversary_does_not_block(self):
        """All PASS prover results + empty adversary → blocks==False."""
        role_results = {
            "prover": [
                self._result("cap-A", "PASS"),
                self._result("cap-B", "PASS"),
            ],
            "adversary": [],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertFalse(bv.blocks)

    def test_all_pass_prover_and_adversary_does_not_block(self):
        """All PASS in both prover and adversary → blocks==False."""
        role_results = {
            "prover": [self._result("cap-A", "PASS")],
            "adversary": [self._result("cap-B", "PASS")],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertFalse(bv.blocks)

    # ── (2) prover FAIL → blocks + capability listed ─────────────────────────

    def test_prover_fail_blocks(self):
        """Any prover FAIL → blocks==True."""
        role_results = {
            "prover": [
                self._result("cap-A", "PASS"),
                self._result("cap-B", "FAIL", exit_code=0, stdout="All constraints satisfied."),
            ],
            "adversary": [],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertTrue(bv.blocks)

    def test_prover_fail_lists_failing_capability(self):
        """prover FAIL → failing capability name is in bv.blocking."""
        role_results = {
            "prover": [
                self._result("cap-FAILING", "FAIL",
                             exit_code=0, stdout="All constraints satisfied."),
            ],
            "adversary": [],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertIn("cap-FAILING", bv.blocking)

    # ── (3) UNPROVABLE → blocks ───────────────────────────────────────────────

    def test_prover_unprovable_blocks(self):
        """Any UNPROVABLE → blocks==True."""
        role_results = {
            "prover": [self._result("cap-U", "UNPROVABLE", exit_code=1, stderr="unrelated err")],
            "adversary": [],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertTrue(bv.blocks)

    def test_prover_unprovable_listed(self):
        """UNPROVABLE capability is listed in bv.blocking."""
        role_results = {
            "prover": [self._result("cap-U", "UNPROVABLE", exit_code=1)],
            "adversary": [],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertIn("cap-U", bv.blocking)

    # ── (4) HARNESS_ERROR → blocks ───────────────────────────────────────────

    def test_prover_harness_error_blocks(self):
        """Any HARNESS_ERROR → blocks==True."""
        role_results = {
            "prover": [self._result("cap-HE", "HARNESS_ERROR")],
            "adversary": [],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertTrue(bv.blocks)

    def test_prover_harness_error_listed(self):
        """HARNESS_ERROR capability is listed in bv.blocking."""
        role_results = {
            "prover": [self._result("cap-HE", "HARNESS_ERROR")],
            "adversary": [],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertIn("cap-HE", bv.blocking)

    # ── (5) adversary-only FAIL (falsification / unlisted premise) → blocks ───

    def test_adversary_only_fail_blocks(self):
        """Adversary-only FAIL (unlisted premise / falsification) → blocks==True."""
        role_results = {
            "prover": [self._result("cap-A", "PASS")],
            "adversary": [self._result("cap-ADVERSARY-FAIL", "FAIL",
                                       exit_code=0, stdout="")],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertTrue(bv.blocks)

    def test_adversary_fail_listed_in_blocking(self):
        """Adversary FAIL capability appears in bv.blocking."""
        role_results = {
            "prover": [],
            "adversary": [self._result("adv-FAIL-cap", "FAIL", exit_code=0)],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertIn("adv-FAIL-cap", bv.blocking)

    # ── (6) adversary PASS never clears a prover FAIL ─────────────────────────

    def test_adversary_pass_cannot_clear_prover_fail(self):
        """Adversary PASS never clears a prover FAIL (net-positive recall)."""
        role_results = {
            "prover": [self._result("cap-B", "FAIL",
                                    exit_code=0, stdout="All constraints satisfied.")],
            "adversary": [self._result("cap-B", "PASS", exit_code=1)],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertTrue(bv.blocks,
                        "Adversary PASS must not clear a prover FAIL — it can only add signals")

    # ── (7) report contains captured evidence for blocking probes ─────────────

    def test_report_contains_command_for_failing_probe(self):
        """bv.report includes the exact command for each blocking probe."""
        role_results = {
            "prover": [
                self._result("cap-FAIL", "FAIL",
                             exit_code=0, stdout="All constraints satisfied.", stderr=""),
            ],
            "adversary": [],
        }
        bv = pdv.synthesize_batch(role_results)
        # The command is ["reify", "check", "/fixture.ri"] — at least "reify" must appear
        self.assertIn("reify", bv.report)

    def test_report_contains_exit_code_for_failing_probe(self):
        """bv.report includes the labelled exit_code line for each blocking probe."""
        role_results = {
            "prover": [self._result("cap-FAIL", "FAIL", exit_code=0, stdout="x")],
            "adversary": [],
        }
        bv = pdv.synthesize_batch(role_results)
        # The labelled "exit_code: 0" line must appear so the assertion can't pass
        # simply because the digit '0' appears elsewhere (e.g. in timestamps or names).
        self.assertIn("exit_code: 0", bv.report)

    def test_report_contains_stdout_for_failing_probe(self):
        """bv.report includes the captured stdout for each blocking probe."""
        role_results = {
            "prover": [
                self._result("cap-FAIL", "FAIL",
                             exit_code=0, stdout="All constraints satisfied.", stderr="")
            ],
            "adversary": [],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertIn("All constraints satisfied.", bv.report)

    def test_report_contains_stderr_for_failing_probe(self):
        """bv.report includes the captured stderr for each blocking probe."""
        role_results = {
            "prover": [
                self._result("cap-FAIL", "FAIL",
                             exit_code=1, stdout="", stderr="error: type mismatch")
            ],
            "adversary": [],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertIn("error: type mismatch", bv.report)

    # ── (8) BatchVerdict has required fields ──────────────────────────────────

    def test_batch_verdict_has_blocks_field(self):
        """BatchVerdict has a .blocks bool field."""
        bv = pdv.synthesize_batch({"prover": [], "adversary": []})
        self.assertIsInstance(bv.blocks, bool)

    def test_batch_verdict_has_blocking_list(self):
        """BatchVerdict has a .blocking list field."""
        bv = pdv.synthesize_batch({"prover": [], "adversary": []})
        self.assertIsInstance(bv.blocking, list)

    def test_batch_verdict_has_report_string(self):
        """BatchVerdict has a .report string field."""
        bv = pdv.synthesize_batch({"prover": [], "adversary": []})
        self.assertIsInstance(bv.report, str)

    def test_blocking_is_empty_when_all_pass(self):
        """bv.blocking is empty when nothing blocks."""
        bv = pdv.synthesize_batch({"prover": [self._result("cap", "PASS")], "adversary": []})
        self.assertEqual(bv.blocking, [])


# ---------------------------------------------------------------------------
# step-07 (RED): CLI main(argv) integration
# ---------------------------------------------------------------------------

class TestMainCLI(unittest.TestCase):
    """Tests for main(argv) subcommands (bind / synthesize).

    These tests FAIL until step-08 implements main() properly.
    """

    def _run_main(self, argv):
        """Run pdv.main(argv) capturing stdout/stderr; returns (exit_code, stdout, stderr)."""
        buf_out = io.StringIO()
        buf_err = io.StringIO()
        with unittest.mock.patch("sys.stdout", buf_out), \
             unittest.mock.patch("sys.stderr", buf_err):
            rc = pdv.main(argv)
        return rc, buf_out.getvalue(), buf_err.getvalue()

    # ── --help ────────────────────────────────────────────────────────────────

    def test_help_exits_0(self):
        """main(['--help']) → 0."""
        rc, _, _ = self._run_main(["--help"])
        self.assertEqual(rc, 0)

    # ── no args / usage errors → 64 ──────────────────────────────────────────

    def test_no_args_exits_64(self):
        """main([]) → 64 (EX_USAGE)."""
        rc, _, _ = self._run_main([])
        self.assertEqual(rc, 64)

    def test_bind_missing_file_exits_64(self):
        """main(['bind', '/nonexistent']) → 64 (IO error)."""
        rc, _, _ = self._run_main(["bind", "/nonexistent-premises-xyz.json"])
        self.assertEqual(rc, 64)

    def test_synthesize_missing_file_exits_64(self):
        """main(['synthesize', '/nonexistent']) → 64 (IO error)."""
        rc, _, _ = self._run_main(["synthesize", "/nonexistent-results-xyz.json"])
        self.assertEqual(rc, 64)

    def test_bind_bad_json_exits_64(self):
        """main(['bind', <invalid-json-file>]) → 64."""
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            f.write("not json at all")
            tmp = f.name
        try:
            rc, _, _ = self._run_main(["bind", tmp])
        finally:
            os.unlink(tmp)
        self.assertEqual(rc, 64)

    # ── bind subcommand ───────────────────────────────────────────────────────

    def test_bind_writes_probe_set_json_to_stdout(self):
        """main(['bind', <premises.json>]) writes valid α probe-set JSON to stdout."""
        premises_data = {
            "premises": [
                {
                    "text": "revolute rejects non-axis",
                    "assertion_kind": "rejection",
                    "fixture": "tests/prd-gate/fixtures/revolute_silent_accept.ri",
                    "match": {"exit_code": 1},
                    "capability": "arg-vs-param rejection",
                }
            ]
        }
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            json.dump(premises_data, f)
            tmp = f.name
        try:
            rc, out, _ = self._run_main(["bind", tmp])
        finally:
            os.unlink(tmp)
        self.assertEqual(rc, 0, f"bind must exit 0, got {rc}")
        # Output must be parseable JSON
        try:
            obj = json.loads(out)
        except json.JSONDecodeError as e:
            self.fail(f"bind stdout is not valid JSON: {e}\nGot: {out!r}")
        # Must be accepted by α's load_probe_set
        pcc.load_probe_set(out)  # must not raise

    def test_bind_exits_0(self):
        """main(['bind', <valid-premises.json>]) exits 0."""
        premises_data = {
            "premises": [
                {
                    "text": "test premise",
                    "assertion_kind": "parses",
                    "fixture": "tests/prd-gate/fixtures/ir_clean_eval.ri",
                    "match": {},
                    "capability": "test",
                }
            ]
        }
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            json.dump(premises_data, f)
            tmp = f.name
        try:
            rc, _, _ = self._run_main(["bind", tmp])
        finally:
            os.unlink(tmp)
        self.assertEqual(rc, 0)

    # ── synthesize subcommand ─────────────────────────────────────────────────

    def _make_results_file(self, prover=None, adversary=None):
        """Write a results JSON file and return its path."""
        data = {
            "prover": prover or [],
            "adversary": adversary or [],
        }
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            json.dump(data, f)
            return f.name

    def _result_record(self, capability: str, verdict: str, exit_code=0,
                       stdout="", stderr="") -> dict:
        return {
            "capability": capability,
            "probe_kind": "check",
            "verdict": verdict,
            "command": ["reify", "check", "/fixture.ri"],
            "exit_code": exit_code,
            "stdout": stdout,
            "stderr": stderr,
        }

    def test_synthesize_all_pass_exits_0(self):
        """synthesize with all-PASS results exits 0."""
        tmp = self._make_results_file(
            prover=[self._result_record("cap", "PASS")],
            adversary=[],
        )
        try:
            rc, _, _ = self._run_main(["synthesize", tmp])
        finally:
            os.unlink(tmp)
        self.assertEqual(rc, 0)

    def test_synthesize_with_fail_exits_1(self):
        """synthesize with a FAIL result exits 1."""
        tmp = self._make_results_file(
            prover=[self._result_record("cap-FAIL", "FAIL",
                                        exit_code=0, stdout="All constraints satisfied.")],
        )
        try:
            rc, _, _ = self._run_main(["synthesize", tmp])
        finally:
            os.unlink(tmp)
        self.assertEqual(rc, 1)

    def test_synthesize_emits_batch_verdict_json(self):
        """synthesize emits a BatchVerdict JSON to stdout."""
        tmp = self._make_results_file(
            prover=[self._result_record("cap", "PASS")],
        )
        try:
            rc, out, _ = self._run_main(["synthesize", tmp])
        finally:
            os.unlink(tmp)
        try:
            obj = json.loads(out)
        except json.JSONDecodeError as e:
            self.fail(f"synthesize stdout is not valid JSON: {e}\nGot: {out!r}")
        self.assertIn("blocks", obj)
        self.assertIn("blocking", obj)

    def test_synthesize_blocking_verdict_json_has_blocks_true(self):
        """synthesize with FAIL result: 'blocks' field is true in JSON output."""
        tmp = self._make_results_file(
            prover=[self._result_record("cap-F", "FAIL", exit_code=0)],
        )
        try:
            rc, out, _ = self._run_main(["synthesize", tmp])
        finally:
            os.unlink(tmp)
        obj = json.loads(out)
        self.assertTrue(obj["blocks"])

    # ── task #7257 step-07 (RED): the evidence gate on the CLI surface ───────

    def _evidence_free_record(self, capability: str, verdict: str = "FAIL") -> dict:
        """A blocking verdict carrying no executed-probe evidence."""
        return {
            "capability": capability,
            "probe_kind": "check",
            "verdict": verdict,
            "command": [],
            "exit_code": None,
            "stdout": "",
            "stderr": "",
        }

    def _fixture_absent_record(self, capability: str) -> dict:
        """An executed FAIL whose probe could not find its target file."""
        return self._result_record(
            capability, "FAIL", exit_code=1,
            stderr="Error: No such file or directory (os error 2)",
        )

    def test_synthesize_only_evidence_free_fails_exits_0(self):
        """A file of nothing but unexecuted promises does not block the batch."""
        tmp = self._make_results_file(
            prover=[self._evidence_free_record("vacuous-1"),
                    self._evidence_free_record("vacuous-2", "UNPROVABLE")],
        )
        try:
            rc, out, _ = self._run_main(["synthesize", tmp])
        finally:
            os.unlink(tmp)
        self.assertEqual(rc, 0, f"evidence-free records must not block; stdout={out!r}")
        obj = json.loads(out)
        self.assertFalse(obj["blocks"])
        self.assertEqual(sorted(obj["malformed"]), ["vacuous-1", "vacuous-2"])

    def test_synthesize_one_executed_fail_among_vacuous_exits_1_with_one_blocker(self):
        """The real finding still blocks, and it is the ONLY thing listed."""
        tmp = self._make_results_file(
            prover=[self._evidence_free_record("vacuous-1"),
                    self._result_record("REAL fail", "FAIL", exit_code=1,
                                        stderr="type mismatch: expected axis"),
                    self._evidence_free_record("vacuous-2")],
            adversary=[self._evidence_free_record("vacuous-3")],
        )
        try:
            rc, out, _ = self._run_main(["synthesize", tmp])
        finally:
            os.unlink(tmp)
        self.assertEqual(rc, 1)
        obj = json.loads(out)
        self.assertEqual(obj["blocking"], ["REAL fail"])
        self.assertEqual(len(obj["blocking"]), 1)

    def test_synthesize_json_carries_all_seven_keys_with_right_types(self):
        """The emitted JSON is the full BatchVerdict, not just the old three keys."""
        tmp = self._make_results_file(
            prover=[self._result_record("cap", "PASS")],
        )
        try:
            _, out, _ = self._run_main(["synthesize", tmp])
        finally:
            os.unlink(tmp)
        obj = json.loads(out)
        for key in ("blocks", "blocking", "report", "malformed",
                    "fixture_absent", "executed", "total"):
            self.assertIn(key, obj, f"synthesize JSON is missing {key!r}")
        self.assertIsInstance(obj["blocks"], bool)
        self.assertIsInstance(obj["blocking"], list)
        self.assertIsInstance(obj["report"], str)
        self.assertIsInstance(obj["malformed"], list)
        self.assertIsInstance(obj["fixture_absent"], list)
        self.assertIsInstance(obj["executed"], int)
        self.assertIsInstance(obj["total"], int)

    def test_synthesize_only_fixture_absent_exits_0_and_names_it(self):
        """A missing fixture is a deliverable signal, not a batch-blocking failure."""
        tmp = self._make_results_file(
            prover=[self._fixture_absent_record("fixture-absent cap")],
        )
        try:
            rc, out, _ = self._run_main(["synthesize", tmp])
        finally:
            os.unlink(tmp)
        self.assertEqual(rc, 0, f"fixture-absent must not block; stdout={out!r}")
        obj = json.loads(out)
        self.assertFalse(obj["blocks"])
        self.assertEqual(obj["fixture_absent"], ["fixture-absent cap"])

    def test_synthesize_all_pass_reports_a_clean_basis(self):
        """Happy-path regression guard: executed == total, nothing set aside."""
        tmp = self._make_results_file(
            prover=[self._result_record("cap-A", "PASS"),
                    self._result_record("cap-B", "PASS")],
            adversary=[self._result_record("cap-C", "PASS")],
        )
        try:
            rc, out, _ = self._run_main(["synthesize", tmp])
        finally:
            os.unlink(tmp)
        self.assertEqual(rc, 0)
        obj = json.loads(out)
        self.assertEqual(obj["total"], 3)
        self.assertEqual(obj["executed"], 3)
        self.assertEqual(obj["malformed"], [])
        self.assertEqual(obj["fixture_absent"], [])


# ---------------------------------------------------------------------------
# step-09 (RED): Workflow .mjs syntax-validity contract
# ---------------------------------------------------------------------------

_NODE_ON_PATH = bool(__import__("shutil").which("node"))


class TestMjsSyntaxValidity(unittest.TestCase):
    """Tests that prd-decompose-verify.mjs exists and is valid ESM.

    Scoped to syntax validity only — NOT prose/role-name grepping and NOT
    executing the module (execution would hit undefined Workflow globals).
    These tests FAIL until step-10 authors the .mjs file.
    """

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs syntax check")
    def test_mjs_file_exists(self):
        """scripts/prd-decompose-verify.mjs must exist."""
        self.assertTrue(
            os.path.isfile(_PDV_MJS),
            f"scripts/prd-decompose-verify.mjs not found at {_PDV_MJS}",
        )

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs syntax check")
    def test_mjs_node_check_passes(self):
        """Wrapped-form node --check: export-stripped body wrapped in async function is valid syntax.

        After step-17, the .mjs has a top-level `return` which raw `node --check`
        rejects with SyntaxError: Illegal return statement (top-level return is not
        valid ESM). Validate harness-faithful syntax instead: strip `export const
        meta` → `const meta`, wrap the body in `async function __wf() { ... }`, and
        node --check that wrapped form. This mirrors the Workflow harness which wraps
        the script body in an async function before evaluating it.
        """
        with open(_PDV_MJS) as fh:
            src = fh.read()
        stripped = src.replace("export const meta", "const meta")
        wrapped = f"async function __wf() {{\n{stripped}\n}}"
        with tempfile.NamedTemporaryFile(mode="w", suffix=".mjs", delete=False) as f:
            f.write(wrapped)
            tmp_path = f.name
        try:
            result = subprocess.run(
                ["node", "--check", tmp_path],
                capture_output=True,
                text=True,
            )
            self.assertEqual(
                result.returncode, 0,
                f"node --check returned {result.returncode}; stderr: {result.stderr!r}",
            )
        finally:
            os.unlink(tmp_path)


# ---------------------------------------------------------------------------
# step-11 (RED): consumer-side boundary e2e (PRD §10), skip-guarded on reify
# ---------------------------------------------------------------------------

class TestBoundaryE2e(unittest.TestCase):
    """Consumer-side boundary tests: false-premise leaf BLOCKS, true-premise leaf PASSES.

    PRD §10 boundary contract, realized at the deterministic-harness layer.
    Skip-guarded on a built reify binary (both the false and true leaf need real eval).
    These tests FAIL until step-12 finalizes the fixtures and synthesis glue.
    """

    def _load_premises_from_leaf(self, leaf_path: str):
        """Read a leaf fixture JSON and return a list of Premise objects."""
        with open(leaf_path) as fh:
            data = json.load(fh)
        premises = []
        for rec in data["premises"]:
            premises.append(pdv.Premise(
                text=rec["text"],
                assertion_kind=rec["assertion_kind"],
                fixture=rec["fixture"],
                match=rec.get("match", {}),
                capability=rec.get("capability"),
            ))
        return premises

    def _run_probes_with_alpha(self, probe_set_dict: dict) -> list:
        """Evaluate all probes in probe_set_dict using α's evaluate(), return result dicts."""
        probe_set_json = json.dumps(probe_set_dict)
        probes = pcc.load_probe_set(probe_set_json)
        result_dicts = []
        for probe in probes:
            r = pcc.evaluate(probe)
            result_dicts.append({
                "capability": r.probe.capability,
                "probe_kind": r.probe.probe_kind,
                "verdict": r.verdict,
                "command": r.command,
                "exit_code": r.exit_code,
                "stdout": r.stdout,
                "stderr": r.stderr,
            })
        return result_dicts

    @unittest.skipUnless(_REIFY_BUILT, "reify binary not built; skip boundary e2e")
    def test_false_premise_leaf_blocks(self):
        """leaf-false-premise.json → bind → α evaluate → synthesize → blocks==True.

        PRD §10: the §3 4575 silent-accept leaf MUST block.
        The rejection probe expects exit_code:1 (rejection fires), but real reify
        exits 0 ('All constraints satisfied.') → ABSENT → expected present → FAIL.
        """
        premises = self._load_premises_from_leaf(_LEAF_FALSE)
        self.assertGreater(len(premises), 0, "leaf-false-premise.json must have premises")

        probe_set = pdv.bind_premises(premises)
        results = self._run_probes_with_alpha(probe_set)

        bv = pdv.synthesize_batch({"prover": results, "adversary": []})

        self.assertTrue(bv.blocks,
                        "leaf-false-premise.json must BLOCK (4575 silent-accept leaf must fail)")

    @unittest.skipUnless(_REIFY_BUILT, "reify binary not built; skip boundary e2e")
    def test_false_premise_report_contains_4575_evidence(self):
        """Blocking report contains captured 4575 evidence (exit_code 0, 'All constraints satisfied.')."""
        premises = self._load_premises_from_leaf(_LEAF_FALSE)
        probe_set = pdv.bind_premises(premises)
        results = self._run_probes_with_alpha(probe_set)
        bv = pdv.synthesize_batch({"prover": results, "adversary": []})

        self.assertTrue(bv.blocks, "must block to have evidence in report")
        # The real reify exits 0 with 'All constraints satisfied.' for revolute_silent_accept.ri
        self.assertIn("All constraints satisfied.", bv.report,
                      "report must capture the 4575 evidence: 'All constraints satisfied.'")

    @unittest.skipUnless(_REIFY_BUILT, "reify binary not built; skip boundary e2e")
    def test_true_premise_leaf_passes(self):
        """leaf-true-premise.json → bind → α evaluate → synthesize → blocks==False."""
        premises = self._load_premises_from_leaf(_LEAF_TRUE)
        self.assertGreater(len(premises), 0, "leaf-true-premise.json must have premises")

        probe_set = pdv.bind_premises(premises)
        results = self._run_probes_with_alpha(probe_set)

        bv = pdv.synthesize_batch({"prover": results, "adversary": []})

        self.assertFalse(bv.blocks,
                         f"leaf-true-premise.json must PASS; blocking: {bv.blocking}; "
                         f"report: {bv.report}")


# ---------------------------------------------------------------------------
# step-14 (RED): Workflow .mjs injected-globals runtime contract
# ---------------------------------------------------------------------------

class TestMjsInjectedGlobalsContract(unittest.TestCase):
    """Execute prd-decompose-verify.mjs under a faithful mock of Workflow's injected
    globals and ONLY those: agent, parallel, pipeline, log, phase, args, budget, workflow.

    The Workflow tool injects exactly this set; tmp_file and shell are NOT injected.
    Against the CURRENT .mjs this FAILS: stage 2 calls writeTempJson/runHarness which
    reference non-injected tmp_file/shell globals, throwing ReferenceError at import-eval.
    Passes after step-15 rewrites the .mjs to use ONLY injected globals.
    """

    _SENTINEL = "WORKFLOW_CONTRACT_OK"

    def _harness_source(self) -> str:
        """Build the Node.js ESM harness script source."""
        mjs_abs = _PDV_MJS.replace("\\", "\\\\")
        return f"""\
// Faithful mock of the Workflow tool's injected globals.
// ONLY globals the Workflow tool injects are set — tmp_file and shell are NOT.

const SENTINEL = "{self._SENTINEL}";
const MJS_PATH = "{mjs_abs}";

// ── mock: agent(prompt, opts) — returns canned shapes based on opts.phase ──────
globalThis.agent = async (prompt, opts = {{}}) => {{
    const phase = (opts.phase || "").toLowerCase();
    if (phase === "enumerate") {{
        return {{
            premises: [{{
                text: "revolute rejects non-axis arg",
                assertion_kind: "rejection",
                fixture: "tests/prd-gate/fixtures/revolute_silent_accept.ri",
                match: {{ exit_code: 1 }},
                capability: "arg-vs-param rejection (mock)",
            }}],
        }};
    }}
    if (phase === "prove") {{
        return {{
            prover: [{{
                capability: "arg-vs-param rejection (mock)",
                probe_kind: "check",
                verdict: "PASS",
                command: ["reify", "check", "tests/prd-gate/fixtures/revolute_silent_accept.ri"],
                exit_code: 1,
                stdout: "",
                stderr: "type mismatch",
            }}],
            adversary: [],
        }};
    }}
    if (phase === "adversary") {{
        return {{ prover: [], adversary: [] }};
    }}
    if (phase === "synthesize") {{
        // Full BatchVerdict shape (task #7257): an evidence-free {{blocks:false}}
        // would now be dispositioned NOT_VERIFIED, so the mock must report a
        // real executed probe for these contract tests to drive a VERIFIED leaf.
        return {{ blocks: false, blocking: [], report: "",
                 malformed: [], fixture_absent: [], executed: 1, total: 1 }};
    }}
    // fallback
    return {{}};
}};

// ── mock: pipeline(items, ...stages) — threads each item through stages in order ─
globalThis.pipeline = async (items, ...stages) => {{
    const results = [];
    for (const item of items) {{
        let val = item;
        for (const stage of stages) {{
            val = await stage(val, item, results.length);
        }}
        results.push(val);
    }}
    return results;
}};

// ── mock: parallel(thunks) — Promise.all of called thunks ────────────────────
globalThis.parallel = async (thunks) => Promise.all(thunks.map(t => t()));

// ── mock: log — no-op ─────────────────────────────────────────────────────────
globalThis.log = (..._a) => {{}};

// ── mock: phase — no-op ───────────────────────────────────────────────────────
globalThis.phase = (..._a) => {{}};

// ── mock: args — a single leaf to drive every phase (Enumerate→Prove‖Adversary→Synthesize)
globalThis.args = [{{ signal: "revolute rejects a non-axis arg (mock leaf)", text: "mock leaf" }}];

// ── mock: budget ──────────────────────────────────────────────────────────────
globalThis.budget = {{ total: null, spent: () => 0, remaining: () => Infinity }};

// ── mock: workflow ────────────────────────────────────────────────────────────
globalThis.workflow = async () => {{}};

// ── execute the .mjs via AsyncFunction (harness-faithful) ──────────────────────
import {{ readFileSync }} from "node:fs";
try {{
    let src = readFileSync(MJS_PATH, "utf8");
    src = src.replace("export const meta", "const meta");
    const AsyncFunction = Object.getPrototypeOf(async function () {{}}).constructor;
    await new AsyncFunction(src)();
    console.log(SENTINEL);
}} catch (e) {{
    console.error("IMPORT_FAILED:", e.message);
    process.exit(1);
}}
"""

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs contract test")
    def test_mjs_runs_under_injected_globals_only(self):
        """prd-decompose-verify.mjs runs to completion under ONLY Workflow-injected globals.

        Fails against the current .mjs (writeTempJson/runHarness reference non-injected
        tmp_file/shell globals → ReferenceError). Passes after step-15 rewrite.
        """
        harness_src = self._harness_source()
        with tempfile.NamedTemporaryFile(mode="w", suffix=".mjs", delete=False) as f:
            f.write(harness_src)
            harness_path = f.name
        try:
            result = subprocess.run(
                ["node", "--input-type=module"],
                input=harness_src,
                capture_output=True,
                text=True,
                timeout=30,
            )
            self.assertEqual(
                result.returncode, 0,
                f"node exited {result.returncode}; stderr: {result.stderr!r}; stdout: {result.stdout!r}",
            )
            self.assertIn(
                self._SENTINEL, result.stdout,
                f"sentinel not found in stdout; stdout: {result.stdout!r}; stderr: {result.stderr!r}",
            )
        finally:
            os.unlink(harness_path)

    _RESULT_MARK = "WF_RESULT_JSON:"

    def _result_capturing_source(self) -> str:
        """Build a Node ESM harness that captures the .mjs body's RETURN VALUE.

        The Workflow harness wraps the script body in an async function and takes
        the result from its top-level `return`. After step-17, the .mjs has a
        native top-level `return`, so stripping `export const meta` → `const meta`
        and wrapping in AsyncFunction is sufficient — no return injection needed.
        """
        globals_setup = self._harness_source().split("// ── execute the .mjs")[0]
        return globals_setup + f"""\
// ── capture the .mjs body's return value ─────────────────────────────────────
import {{ readFileSync }} from "node:fs";
const RESULT_MARK = "{self._RESULT_MARK}";
let src = readFileSync(MJS_PATH, "utf8");
// Strip the ESM `export` so the body is legal inside a function.
src = src.replace("export const meta", "const meta");
// The .mjs now has a native top-level `return` (step-17) — no injection needed.
const AsyncFunction = Object.getPrototypeOf(async function () {{}}).constructor;
const body = new AsyncFunction(src);
const result = await body();
console.log(RESULT_MARK + JSON.stringify(result));
"""

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs contract test")
    def test_mjs_surfaces_aggregate_verdict_shape(self):
        """The .mjs body's return value is the aggregate verdict with the right shape.

        Complements test_mjs_returns_verdict_under_documented_contract: that test
        asserts the return value is NOT undefined; this test additionally asserts
        the shape (blocks/leaf_verdicts/summary), the mock-leaf count, and the
        non-blocking outcome. Uses _result_capturing_source() which strips `export`
        and wraps in AsyncFunction — after step-17, no return injection is needed.
        """
        harness_src = self._result_capturing_source()
        result = subprocess.run(
            ["node", "--input-type=module"],
            input=harness_src,
            capture_output=True,
            text=True,
            timeout=30,
        )
        self.assertEqual(
            result.returncode, 0,
            f"node exited {result.returncode}; stderr: {result.stderr!r}; stdout: {result.stdout!r}",
        )
        marker_lines = [ln for ln in result.stdout.splitlines()
                        if ln.startswith(self._RESULT_MARK)]
        self.assertTrue(
            marker_lines,
            f"no result marker in stdout; stdout: {result.stdout!r}; stderr: {result.stderr!r}",
        )
        payload = marker_lines[-1][len(self._RESULT_MARK):]
        self.assertNotEqual(
            payload, "undefined",
            "workflow body completion value is undefined — result was dropped "
            "(dead-const regression: IIFE bound to an unreturned variable)",
        )
        verdict = json.loads(payload)
        self.assertIsInstance(verdict, dict,
                              f"aggregate verdict must be an object; got {verdict!r}")
        # Aggregate-verdict shape (the contract β/D4 consumes).
        for key in ("blocks", "leaf_verdicts", "summary"):
            self.assertIn(key, verdict,
                          f"aggregate verdict missing '{key}'; got keys {sorted(verdict)}")
        self.assertIsInstance(verdict["blocks"], bool)
        self.assertIsInstance(verdict["leaf_verdicts"], list)
        self.assertIsInstance(verdict["summary"], str)
        # The mock drives one leaf through to a non-blocking synthesize verdict.
        self.assertFalse(verdict["blocks"],
                         f"mock leaf must not block; got verdict {verdict!r}")
        self.assertEqual(len(verdict["leaf_verdicts"]), 1,
                         f"one mock leaf → one leaf verdict; got {verdict['leaf_verdicts']!r}")

    # ── step-16 RED / step-17 GREEN ───────────────────────────────────────────

    def _honest_contract_source(self) -> str:
        """Build a Node ESM harness that wraps the .mjs body via AsyncFunction
        with NO source rewriting beyond stripping the ESM `export` keyword.

        The Workflow harness documentation states the script body is wrapped in
        an async function and the result taken from its top-level `return` (every
        Workflow doc example ends `return {...}`). A bare IIFE expression at the
        end of the body evaluates the inner IIFE but the OUTER AsyncFunction still
        has no return statement — it resolves to undefined. This test catches that.
        """
        globals_setup = self._harness_source().split("// ── execute the .mjs")[0]
        honest_section = (
            "// ── honest contract: strip export only, NO return injection ────────────────\n"
            "import { readFileSync } from \"node:fs\";\n"
            "const WF_VERDICT_MARK = \"WF_VERDICT_JSON:\";\n"
            "let src = readFileSync(MJS_PATH, \"utf8\");\n"
            "// ONLY strip the ESM `export` keyword — do NOT inject any `return` statement.\n"
            "src = src.replace(\"export const meta\", \"const meta\");\n"
            "const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;\n"
            "const result = await new AsyncFunction(src)();\n"
            "// null sentinel: undefined (missing top-level return) renders as null.\n"
            "console.log(WF_VERDICT_MARK + JSON.stringify(result ?? null));\n"
        )
        return globals_setup + honest_section

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs contract test")
    def test_mjs_returns_verdict_under_documented_contract(self):
        """Workflow contract: .mjs body, wrapped as AsyncFunction with NO source
        rewriting beyond stripping `export`, must RETURN the aggregate verdict.

        The Workflow tool documentation states the harness wraps the script body in
        an async function and takes the result from its top-level `return` — every
        Workflow doc example ends `return {...}`. This test models that exactly: it
        does NOT inject any `return` statement. If the .mjs body has no top-level
        `return` (bare IIFE expression), AsyncFunction returns undefined and the
        {blocks, leaf_verdicts, summary} verdict is silently dropped.

        Against the CURRENT bare-IIFE .mjs, AsyncFunction over the export-stripped
        body returns undefined (the `await (async function runWorkflow(){...})()`
        is a bare expression statement in the outer function — no return), so this
        test FAILS (RED). Passes after step-17 rewrites the .mjs with a top-level
        `return`.
        """
        harness_src = self._honest_contract_source()
        result = subprocess.run(
            ["node", "--input-type=module"],
            input=harness_src,
            capture_output=True,
            text=True,
            timeout=30,
        )
        self.assertEqual(
            result.returncode, 0,
            f"node exited {result.returncode}; stderr: {result.stderr!r}; stdout: {result.stdout!r}",
        )
        MARK = "WF_VERDICT_JSON:"
        marker_lines = [ln for ln in result.stdout.splitlines() if ln.startswith(MARK)]
        self.assertTrue(
            marker_lines,
            f"no verdict marker in stdout; stdout: {result.stdout!r}; stderr: {result.stderr!r}",
        )
        payload = marker_lines[-1][len(MARK):]
        self.assertNotEqual(
            payload, "null",
            "workflow body returned null/undefined under documented Workflow contract: "
            "bare IIFE expression doesn't return from the outer AsyncFunction, "
            "silently dropping the {blocks, leaf_verdicts, summary} verdict",
        )
        verdict = json.loads(payload)
        self.assertIsInstance(verdict, dict,
                              f"aggregate verdict must be an object; got {verdict!r}")
        for key in ("blocks", "leaf_verdicts", "summary"):
            self.assertIn(key, verdict,
                          f"aggregate verdict missing key '{key}'; keys: {sorted(verdict)}")
        self.assertIsInstance(verdict["blocks"], bool)
        self.assertIsInstance(verdict["leaf_verdicts"], list)
        self.assertIsInstance(verdict["summary"], str)
        self.assertFalse(verdict["blocks"],
                         f"mock leaf must not block; verdict: {verdict!r}")
        self.assertEqual(
            len(verdict["leaf_verdicts"]), 1,
            f"one mock leaf → one leaf verdict; got {verdict['leaf_verdicts']!r}",
        )

    # ── task #4960 RED: mega-leaf fan-out regression (end-to-end) ────────────
    #
    # Reuses _harness_source()'s injected-globals mock verbatim but parameterizes
    # globalThis.args and captures globalThis.log, then runs the FULL .mjs body
    # (export-stripped -> AsyncFunction, same as _result_capturing_source) to
    # assert the user-observable fan-out signal: leaf_verdicts.length.

    _LOG_MARK = "WF_LOG_JSON:"

    def _fanout_source(self, args_js_expr: str) -> str:
        """Build a Node ESM harness like _result_capturing_source() but with a
        PARAMETERIZED globalThis.args and a CAPTURING globalThis.log, so a test
        can drive args→leaves normalization end-to-end and observe both the
        returned verdict and any degradation warnings.
        """
        base = self._harness_source()

        fixed_args_line = (
            'globalThis.args = [{ signal: "revolute rejects a non-axis arg (mock leaf)", '
            'text: "mock leaf" }];'
        )
        self.assertIn(fixed_args_line, base,
                      "fixture drift: _harness_source() args mock line changed shape")
        base = base.replace(fixed_args_line, f"globalThis.args = {args_js_expr};")

        noop_log_line = "globalThis.log = (..._a) => {};"
        self.assertIn(noop_log_line, base,
                      "fixture drift: _harness_source() log mock line changed shape")
        base = base.replace(
            noop_log_line,
            "globalThis.__LOG_LINES = [];\n"
            "globalThis.log = (..._a) => { globalThis.__LOG_LINES.push(_a.map(String).join(\" \")); };",
        )

        globals_setup = base.split("// ── execute the .mjs")[0]
        return globals_setup + f"""\
// ── capture the .mjs body's return value + log lines ──────────────────────────
import {{ readFileSync }} from "node:fs";
const RESULT_MARK = "{self._RESULT_MARK}";
const LOG_MARK = "{self._LOG_MARK}";
let src = readFileSync(MJS_PATH, "utf8");
src = src.replace("export const meta", "const meta");
const AsyncFunction = Object.getPrototypeOf(async function () {{}}).constructor;
const body = new AsyncFunction(src);
const result = await body();
console.log(RESULT_MARK + JSON.stringify(result));
console.log(LOG_MARK + JSON.stringify(globalThis.__LOG_LINES || []));
"""

    def _run_fanout(self, args_js_expr: str):
        """Run the .mjs body under _fanout_source() and return (verdict, log_lines)."""
        harness_src = self._fanout_source(args_js_expr)
        result = subprocess.run(
            ["node", "--input-type=module"],
            input=harness_src,
            capture_output=True,
            text=True,
            timeout=30,
        )
        self.assertEqual(
            result.returncode, 0,
            f"node exited {result.returncode}; stderr: {result.stderr!r}; stdout: {result.stdout!r}",
        )
        result_lines = [ln for ln in result.stdout.splitlines() if ln.startswith(self._RESULT_MARK)]
        log_lines = [ln for ln in result.stdout.splitlines() if ln.startswith(self._LOG_MARK)]
        self.assertTrue(result_lines, f"no result marker in stdout; stdout: {result.stdout!r}")
        self.assertTrue(log_lines, f"no log marker in stdout; stdout: {result.stdout!r}")
        verdict = json.loads(result_lines[-1][len(self._RESULT_MARK):])
        logs = json.loads(log_lines[-1][len(self._LOG_MARK):])
        return verdict, logs

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs contract test")
    def test_stringified_leaf_array_fans_out_per_leaf(self):
        """task #4960: Workflow args arriving JSON-stringified must still fan out
        one leaf per element, not collapse into a single mega-leaf.

        Against the CURRENT .mjs, line 111's inline
        `Array.isArray(args) ? args : (args ? [args] : [])` sees a truthy STRING
        (JSON.stringify of the leaf array), Array.isArray fails, and the whole
        batch becomes ONE mega-leaf: leaf_verdicts.length is 1, not 3. RED until
        normalizeLeaves() is wired in.
        """
        args_expr = 'JSON.stringify(["mock-leaf-0", "mock-leaf-1", "mock-leaf-2"])'
        verdict, _logs = self._run_fanout(args_expr)
        self.assertIsInstance(verdict["leaf_verdicts"], list)
        self.assertEqual(
            len(verdict["leaf_verdicts"]), 3,
            f"stringified 3-leaf array must fan out to 3 leaf verdicts; got {verdict['leaf_verdicts']!r}",
        )

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs contract test")
    def test_non_json_string_args_falls_back_with_warning(self):
        """A non-JSON string in args has no recoverable leaf structure: it must
        fall back to a single mega-leaf AND log a loud degradation warning so the
        fan-out loss stays visible (task #4960 — the fix must not fail silently).

        Against the CURRENT .mjs, the single-leaf fallback already happens (a
        random string was never an array), but NO warning is logged anywhere —
        this test's warning assertion is RED until normalizeLeaves() calls warn().
        """
        args_expr = json.dumps("not valid json {[")
        verdict, logs = self._run_fanout(args_expr)
        self.assertEqual(
            len(verdict["leaf_verdicts"]), 1,
            f"non-JSON string must fall back to exactly one leaf; got {verdict['leaf_verdicts']!r}",
        )
        combined_log = " ".join(logs).lower()
        self.assertTrue(
            "mega-leaf" in combined_log or "mega leaf" in combined_log
            or "fan-out" in combined_log or "fan out" in combined_log,
            f"expected a mega-leaf/fan-out degradation warning in captured log; got {logs!r}",
        )


# ---------------------------------------------------------------------------
# task #4960 (RED): normalizeLeaves() pure-helper contract — mega-leaf trap
# ---------------------------------------------------------------------------

class TestMjsNormalizeLeaves(unittest.TestCase):
    """Pure-unit tests for the normalizeLeaves(rawArgs, warn) helper, source-
    sliced directly out of prd-decompose-verify.mjs (everything BEFORE the
    `const _wfResult = await` IIFE anchor) and evaluated via `new Function` —
    no injected-globals mock, no IIFE execution.

    Guards the mega-leaf trap (task #4960): when Workflow args arrive
    JSON-stringified, `Array.isArray(args)` is false and the whole stringified
    batch collapses into ONE leaf. normalizeLeaves must detect a JSON-
    stringified array and restore per-leaf fan-out, while leaving every
    non-string input byte-for-byte unchanged (real array / single object /
    undefined / null).

    FAILS until normalizeLeaves is added to the .mjs: the head-slice eval
    throws ReferenceError('normalizeLeaves is not defined'), the Node harness
    exits non-zero, and the test fails on the returncode assertion.
    """

    _MARK = "NORMALIZE_LEAVES_RESULT:"

    def _harness_source(self) -> str:
        mjs_abs = _PDV_MJS.replace("\\", "\\\\")
        return f"""\
import {{ readFileSync }} from "node:fs";

const MARK = "{self._MARK}";
const MJS_PATH = "{mjs_abs}";

let src = readFileSync(MJS_PATH, "utf8");
const ANCHOR = "const _wfResult = await";
const anchorIdx = src.indexOf(ANCHOR);
if (anchorIdx === -1) {{
    console.error("ANCHOR_NOT_FOUND: " + ANCHOR);
    process.exit(1);
}}
let head = src.slice(0, anchorIdx);
head = head.replace("export const meta", "const meta");

let normalizeLeaves;
try {{
    normalizeLeaves = new Function(head + "\\nreturn normalizeLeaves;")();
}} catch (e) {{
    console.error("HELPER_EXTRACT_FAILED: " + e.message);
    process.exit(1);
}}
if (typeof normalizeLeaves !== "function") {{
    console.error("HELPER_NOT_A_FUNCTION: " + typeof normalizeLeaves);
    process.exit(1);
}}

function run(rawArgs) {{
    const warnCalls = [];
    const warn = (...a) => {{ warnCalls.push(a.map(String).join(" ")); }};
    const result = normalizeLeaves(rawArgs, warn);
    return {{
        length: Array.isArray(result) ? result.length : null,
        isArray: Array.isArray(result),
        warnCalls,
    }};
}}

const L = [{{ signal: "leaf-1" }}, {{ signal: "leaf-2" }}, {{ signal: "leaf-3" }}];

const cases = {{
    stringified_array: run(JSON.stringify(L)),
    real_array_passthrough: run(L),
    non_json_string: run("not json {{["),
    json_non_array_object: run(JSON.stringify({{ a: 1 }})),
    undefined_input: run(undefined),
    empty_string: run(""),
    single_object_leaf: run({{ signal: "solo" }}),
}};

console.log(MARK + JSON.stringify(cases));
"""

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs helper contract test")
    def test_normalize_leaves_pure_helper_contract(self):
        harness_src = self._harness_source()
        result = subprocess.run(
            ["node", "--input-type=module"],
            input=harness_src,
            capture_output=True,
            text=True,
            timeout=30,
        )
        self.assertEqual(
            result.returncode, 0,
            f"node exited {result.returncode}; stderr: {result.stderr!r}; stdout: {result.stdout!r}",
        )
        marker_lines = [ln for ln in result.stdout.splitlines() if ln.startswith(self._MARK)]
        self.assertTrue(
            marker_lines,
            f"no result marker in stdout; stdout: {result.stdout!r}; stderr: {result.stderr!r}",
        )
        cases = json.loads(marker_lines[-1][len(self._MARK):])

        # (a) stringified array -> real per-leaf fan-out, no degradation warning.
        self.assertEqual(
            cases["stringified_array"]["length"], 3,
            f"JSON-stringified 3-leaf array must fan out to 3 leaves; got {cases['stringified_array']!r}",
        )
        self.assertEqual(
            cases["stringified_array"]["warnCalls"], [],
            "a well-formed stringified leaf array must not log a degradation warning",
        )

        # (b) real array passthrough — unchanged behavior, no warning.
        self.assertEqual(
            cases["real_array_passthrough"]["length"], 3,
            f"real array must pass through unchanged; got {cases['real_array_passthrough']!r}",
        )
        self.assertEqual(cases["real_array_passthrough"]["warnCalls"], [])

        # (c) non-JSON string -> single mega-leaf fallback + loud warning.
        self.assertEqual(
            cases["non_json_string"]["length"], 1,
            f"non-JSON string must fall back to one mega-leaf; got {cases['non_json_string']!r}",
        )
        self.assertTrue(
            cases["non_json_string"]["warnCalls"],
            "non-JSON string fallback must log a degradation warning",
        )
        warn_text = " ".join(cases["non_json_string"]["warnCalls"]).lower()
        self.assertTrue(
            "mega-leaf" in warn_text or "mega leaf" in warn_text
            or "fan-out" in warn_text or "fan out" in warn_text,
            f"warning must name the mega-leaf/fan-out degradation; got {cases['non_json_string']['warnCalls']!r}",
        )

        # edge: JSON string parsing to a non-array object -> single mega-leaf + warning.
        self.assertEqual(
            cases["json_non_array_object"]["length"], 1,
            f"non-array JSON must fall back to one mega-leaf; got {cases['json_non_array_object']!r}",
        )
        self.assertTrue(
            cases["json_non_array_object"]["warnCalls"],
            "non-array JSON parse result must log a degradation warning",
        )

        # edge: undefined / empty string -> zero leaves (unchanged pre-fix behavior).
        self.assertEqual(cases["undefined_input"]["length"], 0)
        self.assertEqual(cases["undefined_input"]["warnCalls"], [])
        self.assertEqual(cases["empty_string"]["length"], 0)

        # edge: single object leaf (non-string, non-array) -> one leaf, unchanged.
        self.assertEqual(cases["single_object_leaf"]["length"], 1)
        self.assertEqual(cases["single_object_leaf"]["warnCalls"], [])


# ---------------------------------------------------------------------------
# task #7257 step-01 (RED): command normalization (ARM 1, item 3)
# ---------------------------------------------------------------------------

class TestCommandNormalization(unittest.TestCase):
    """`command` may arrive as a string; rendering it must not explode it.

    Observed on this branch before the fix: a record carrying
    `"command": "target/release/reify eval f.ri"` (a STRING, not a list) was
    rendered by synthesize_batch's `" ".join(rec.get("command", []))` as
    `t a r g e t / r e l e a s e / r e i f y   e v a l   f . r i` — Python
    joins a string character-by-character.  The captured evidence a human is
    meant to re-run became unreadable.

    GREEN in task #7257 step-02 (pdv.normalize_command).
    """

    _STRING_CMD = "target/release/reify eval f.ri"
    _EXPLODED = "t a r g e t"

    # ── (a) unit tests for normalize_command ─────────────────────────────────

    def test_list_round_trips_as_list_of_str(self):
        """A list of strings round-trips unchanged."""
        self.assertEqual(
            pdv.normalize_command(["reify", "check", "/fixture.ri"]),
            ["reify", "check", "/fixture.ri"],
        )

    def test_list_items_are_stringified(self):
        """Non-str items in a list are coerced to str (evidence stays renderable)."""
        self.assertEqual(pdv.normalize_command(["reify", 7, None]), ["reify", "7", "None"])

    def test_tuple_becomes_list(self):
        """A tuple normalizes to a list (JSON round-trips give lists, tests give tuples)."""
        out = pdv.normalize_command(("reify", "check"))
        self.assertIsInstance(out, list)
        self.assertEqual(out, ["reify", "check"])

    def test_string_becomes_single_element_list(self):
        """A STRING command becomes ONE token, so `" ".join` renders it verbatim."""
        self.assertEqual(pdv.normalize_command(self._STRING_CMD), [self._STRING_CMD])

    def test_none_becomes_empty_list(self):
        """None (absent command) normalizes to []."""
        self.assertEqual(pdv.normalize_command(None), [])

    def test_int_becomes_empty_list(self):
        """A non-str, non-sequence value carries no command evidence → []."""
        self.assertEqual(pdv.normalize_command(7), [])

    def test_empty_list_stays_empty(self):
        """An explicit empty list stays empty (no evidence)."""
        self.assertEqual(pdv.normalize_command([]), [])

    # ── (b) end-to-end through synthesize_batch ──────────────────────────────

    def _string_command_record(self) -> dict:
        """A blocking record whose `command` is a STRING rather than a list."""
        return {
            "capability": "string-command capability",
            "probe_kind": "ir",
            "verdict": "FAIL",
            "command": self._STRING_CMD,
            "exit_code": 1,
            "stdout": "",
            "stderr": "assertion did not hold",
        }

    def test_string_command_renders_verbatim_in_report(self):
        """The report shows the command exactly as captured."""
        bv = pdv.synthesize_batch({"prover": [self._string_command_record()], "adversary": []})
        self.assertIn(
            self._STRING_CMD, bv.report,
            f"string command must render verbatim; report was:\n{bv.report}",
        )

    def test_string_command_is_not_character_exploded(self):
        """The report must NOT contain the character-spaced explosion."""
        bv = pdv.synthesize_batch({"prover": [self._string_command_record()], "adversary": []})
        self.assertNotIn(
            self._EXPLODED, bv.report,
            f"string command was exploded character-by-character; report was:\n{bv.report}",
        )


# ---------------------------------------------------------------------------
# task #7257 step-03 (RED): evidence gate for blocking records (ARM 1, item 1)
# ---------------------------------------------------------------------------

class TestEvidenceGate(unittest.TestCase):
    """A blocking verdict with no executed-probe evidence is a harness defect.

    PRD §6 decision 4: "Captured output is mandatory on every verdict. […]
    Every D1 result carries the exact command + stdout/stderr + exit code, so a
    human (or D4) can re-derive the verdict without re-running."  An
    evidence-free record is therefore not a valid falsification at all — it is
    an unexecuted promise, and tabulating it as `blocking` buries the ONE real
    finding among N vacuous ones (the observed pi-report failure).

    GREEN in task #7257 step-04 (has_probe_evidence / classify_record).
    """

    def _result(self, capability: str, verdict: str, *,
                command: Any = ("reify", "check", "/fixture.ri"),
                exit_code: Any = 1,
                stdout: str = "", stderr: str = "",
                omit_command: bool = False,
                omit_exit_code: bool = False) -> dict:
        """Build a synthetic α --json result record, with omissions on request."""
        rec: dict = {
            "capability": capability,
            "probe_kind": "check",
            "verdict": verdict,
            "command": list(command) if isinstance(command, tuple) else command,
            "exit_code": exit_code,
            "stdout": stdout,
            "stderr": stderr,
        }
        if omit_command:
            del rec["command"]
        if omit_exit_code:
            del rec["exit_code"]
        return rec

    # ── (1)(2) evidence-free FAIL: not blocking, but named as malformed ──────

    def test_evidence_free_fail_does_not_block(self):
        """A FAIL with command [] and exit_code None must not appear in blocking."""
        rec = self._result("evidence-free cap", "FAIL", command=[], exit_code=None)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertNotIn("evidence-free cap", bv.blocking)
        self.assertFalse(bv.blocks, "an unexecuted promise must not block the batch")

    def test_evidence_free_fail_is_reported_as_malformed(self):
        """The same record's capability IS surfaced — as malformed, not blocking."""
        rec = self._result("evidence-free cap", "FAIL", command=[], exit_code=None)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertIn("evidence-free cap", bv.malformed)

    # ── (3)(4) partial evidence is still no evidence ─────────────────────────

    def test_missing_command_key_is_malformed(self):
        """A record with no `command` key at all → malformed, not blocking."""
        rec = self._result("no-command cap", "FAIL", omit_command=True)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertIn("no-command cap", bv.malformed)
        self.assertNotIn("no-command cap", bv.blocking)

    def test_missing_exit_code_key_is_malformed(self):
        """A non-empty command but no `exit_code` key → no process outcome → malformed."""
        rec = self._result("no-exit-code cap", "UNPROVABLE", omit_exit_code=True)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertIn("no-exit-code cap", bv.malformed)
        self.assertNotIn("no-exit-code cap", bv.blocking)

    def test_exit_code_zero_is_valid_evidence(self):
        """exit_code 0 is a real outcome — `is not None`, not truthiness."""
        rec = self._result("exit-zero cap", "FAIL", exit_code=0)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertIn("exit-zero cap", bv.blocking)
        self.assertEqual(bv.malformed, [])

    # ── (5) the headline signal: one real finding is no longer buried ────────

    def test_one_executed_fail_among_evidence_free_fails_is_the_only_blocker(self):
        """The real finding stands alone; the three vacuous ones do not dilute it."""
        role_results = {
            "prover": [
                self._result("vacuous-1", "FAIL", command=[], exit_code=None),
                self._result("REAL executed fail", "FAIL", exit_code=1,
                             stderr="type mismatch: expected axis"),
                self._result("vacuous-2", "FAIL", omit_command=True),
            ],
            "adversary": [
                self._result("vacuous-3", "UNPROVABLE", command=[], omit_exit_code=True),
            ],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertTrue(bv.blocks)
        self.assertEqual(bv.blocking, ["REAL executed fail"])
        self.assertEqual(sorted(bv.malformed), ["vacuous-1", "vacuous-2", "vacuous-3"])

    # ── (6) existing blocking semantics preserved for executed probes ────────

    def test_executed_unprovable_still_blocks(self):
        """An executed UNPROVABLE keeps blocking (unchanged semantics)."""
        rec = self._result("unprovable cap", "UNPROVABLE", exit_code=2)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertTrue(bv.blocks)
        self.assertIn("unprovable cap", bv.blocking)

    def test_executed_harness_error_still_blocks(self):
        """An executed HARNESS_ERROR keeps blocking (unchanged semantics)."""
        rec = self._result("harness-error cap", "HARNESS_ERROR", exit_code=-1,
                           stderr="probe runner crashed")
        bv = pdv.synthesize_batch({"prover": [], "adversary": [rec]})
        self.assertTrue(bv.blocks)
        self.assertIn("harness-error cap", bv.blocking)

    # ── (7) a PREMISE-shaped record is neither blocking nor malformed ────────

    def test_premise_shaped_record_is_neither_blocking_nor_malformed(self):
        """RESULTS_SCHEMA is loose enough today that a premise validates as a result.

        Such a record has no `verdict` key at all.  It is not a falsification
        and it is not an evidence-free BLOCKING verdict either — the evidence
        gate must not manufacture a malformed entry out of it.
        """
        premise_shaped = {
            "capability": "a revolute joint rejects a non-axis argument",
            "assertion_kind": "rejection",
            "fixture": "tests/prd-gate/fixtures/revolute-non-axis.ri",
        }
        bv = pdv.synthesize_batch({"prover": [premise_shaped], "adversary": []})
        self.assertFalse(bv.blocks)
        self.assertEqual(bv.blocking, [])
        self.assertEqual(bv.malformed, [])

    def test_pass_record_is_not_malformed_even_without_evidence(self):
        """The gate applies to BLOCKING verdicts only; a PASS is not re-litigated."""
        rec = self._result("passing cap", "PASS", command=[], exit_code=None)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertEqual(bv.malformed, [])
        self.assertEqual(bv.blocking, [])

    # ── (8) executed / total counters ────────────────────────────────────────

    def test_executed_and_total_counters_on_a_mixed_set(self):
        """`executed` counts records with probe evidence; `total` counts all records."""
        role_results = {
            "prover": [
                self._result("p-pass", "PASS", exit_code=0),
                self._result("p-fail", "FAIL", exit_code=1),
                self._result("p-vacuous", "FAIL", command=[], exit_code=None),
            ],
            "adversary": [
                self._result("a-pass", "PASS", exit_code=0),
                self._result("a-vacuous", "UNPROVABLE", omit_command=True),
            ],
        }
        bv = pdv.synthesize_batch(role_results)
        self.assertEqual(bv.total, 5)
        self.assertEqual(bv.executed, 3)

    def test_counters_are_zero_on_an_empty_batch(self):
        """An empty batch reports 0 executed of 0 total (not a silent pass basis)."""
        bv = pdv.synthesize_batch({"prover": [], "adversary": []})
        self.assertEqual(bv.total, 0)
        self.assertEqual(bv.executed, 0)

    def test_malformed_report_section_names_the_capability(self):
        """A malformed record is visible in the report, labelled as a harness defect."""
        rec = self._result("evidence-free cap", "FAIL", command=[], exit_code=None)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertIn("evidence-free cap", bv.report)
        self.assertIn("MALFORMED", bv.report)


# ---------------------------------------------------------------------------
# task #7257 step-05 (RED): fixture-absent ≠ falsification (ARM 1, item 4)
# ---------------------------------------------------------------------------

class TestFixtureAbsent(unittest.TestCase):
    """A probe that could not find its fixture has falsified nothing.

    During decompose, the .ri fixture a premise probes is very often the leaf's
    own deliverable — it does not exist yet, by construction.  Scoring the
    resulting ENOENT as a premise falsification reports a design defect where
    there is only a missing file, which is what the observed pi report did.

    GREEN in task #7257 step-06 (fixture_absent_evidence).
    """

    def _result(self, capability: str, verdict: str = "FAIL",
                stderr: str = "", exit_code: int = 1) -> dict:
        """An EXECUTED α result record (real command, real exit code)."""
        return {
            "capability": capability,
            "probe_kind": "ir",
            "verdict": verdict,
            "command": ["reify", "eval", "tests/prd-gate/fixtures/leaf.ri"],
            "exit_code": exit_code,
            "stdout": "",
            "stderr": stderr,
        }

    # ── (1) the verbatim stderr from the observed run ────────────────────────

    def test_observed_enoent_stderr_is_not_a_falsification(self):
        """'Error: No such file or directory (os error 2)' → fixture-absent."""
        rec = self._result("fixture-absent cap",
                           stderr="Error: No such file or directory (os error 2)")
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertNotIn("fixture-absent cap", bv.blocking)
        self.assertFalse(bv.blocks)
        self.assertIn("fixture-absent cap", bv.fixture_absent)

    # ── (2) both signature forms, case-insensitively ─────────────────────────

    def test_bare_no_such_file_signature(self):
        """A bare 'No such file or directory' is enough."""
        rec = self._result("bare-enoent cap", stderr="No such file or directory")
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertIn("bare-enoent cap", bv.fixture_absent)
        self.assertFalse(bv.blocks)

    def test_signature_match_is_case_insensitive(self):
        """Lower-cased diagnostics match too — the signature is normalized."""
        rec = self._result("lowercase-enoent cap", stderr="no such file or directory")
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertIn("lowercase-enoent cap", bv.fixture_absent)
        self.assertFalse(bv.blocks)

    # ── (3) the Rust io::Error rendering on its own ──────────────────────────

    def test_bare_os_error_2_signature(self):
        """Rust renders ENOENT as 'os error 2'; that alone classifies fixture-absent."""
        rec = self._result("os-error-2 cap", stderr="failed to open input: os error 2")
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertIn("os-error-2 cap", bv.fixture_absent)
        self.assertFalse(bv.blocks)

    # ── (4)(5) the over-reach guards ─────────────────────────────────────────

    def test_unrelated_diagnostic_still_blocks(self):
        """A genuine falsification with an unrelated stderr is untouched."""
        rec = self._result("real fail cap", stderr="type mismatch: expected axis")
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertTrue(bv.blocks)
        self.assertIn("real fail cap", bv.blocking)
        self.assertEqual(bv.fixture_absent, [])

    def test_empty_stderr_still_blocks(self):
        """An executed FAIL with no stderr at all is still a falsification."""
        rec = self._result("silent fail cap", stderr="")
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertTrue(bv.blocks)
        self.assertIn("silent fail cap", bv.blocking)
        self.assertEqual(bv.fixture_absent, [])

    # ── (6) missing BINARY is not a missing fixture ──────────────────────────

    def test_binary_not_found_sentinel_still_blocks(self):
        """α's binary-not-found sentinel emits the SAME ENOENT text but must block.

        A missing `reify` binary is a real harness failure: nothing was probed
        and the batch cannot be trusted.  The fixture-absent carve-out must not
        swallow it just because the OS worded both errors the same way.
        """
        stderr = (f"{pcc._BINARY_NOT_FOUND_SENTINEL}: [Errno 2] "
                  "No such file or directory: 'target/release/reify'")
        rec = self._result("missing binary cap", verdict="HARNESS_ERROR",
                           stderr=stderr, exit_code=127)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertTrue(bv.blocks, "a missing binary must still block")
        self.assertIn("missing binary cap", bv.blocking)
        self.assertNotIn("missing binary cap", bv.fixture_absent)

    # ── (7) counters and report placement ────────────────────────────────────

    def test_fixture_absent_record_counts_as_executed(self):
        """The probe DID run — it just could not find its fixture."""
        rec = self._result("fixture-absent cap",
                           stderr="Error: No such file or directory (os error 2)")
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertEqual(bv.executed, 1)
        self.assertEqual(bv.total, 1)

    def test_fixture_absent_is_named_in_its_own_report_section(self):
        """The capability stays visible, under a fixture-absent label."""
        rec = self._result("fixture-absent cap",
                           stderr="Error: No such file or directory (os error 2)")
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertIn("fixture-absent cap", bv.report)
        self.assertIn("FIXTURE ABSENT", bv.report)

    def test_fixture_absent_is_not_counted_as_malformed(self):
        """The two categories are distinct: one ran without a target, one never ran."""
        rec = self._result("fixture-absent cap",
                           stderr="Error: No such file or directory (os error 2)")
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertEqual(bv.malformed, [])

    def test_passing_record_with_enoent_stderr_is_untouched(self):
        """The carve-out applies to blocking verdicts only."""
        rec = self._result("passing cap", verdict="PASS",
                           stderr="No such file or directory", exit_code=0)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertEqual(bv.fixture_absent, [])
        self.assertEqual(bv.blocking, [])
        self.assertFalse(bv.blocks)


# ---------------------------------------------------------------------------
# task #7257 step-09 (RED): RESULTS_SCHEMA must constrain the record shape
# ---------------------------------------------------------------------------

class TestMjsResultsSchema(unittest.TestCase):
    """The agent-output schema is the FIRST line of defence for ARM 1.

    RESULTS_SCHEMA today declares `prover`/`adversary` as
    `{type:"array", items:{type:"object"}}` — no required keys, no verdict
    enum, no type on `command`.  Consequences measured on this branch:

      - a PREMISE record validates as a RESULT record;
      - a record with no `command`/`exit_code` at all validates, which is
        exactly the unexecuted promise the Python evidence gate now catches;
      - `"command": "target/release/reify eval f.ri"` (a STRING) validates,
        which is the character-explosion source.

    The Python gate is the second line of defence and stays.  This schema kills
    the malformed shapes at source so an agent cannot emit them at all.

    Source-sliced out of the .mjs the same way TestMjsNormalizeLeaves does:
    everything BEFORE the `const _wfResult = await` IIFE anchor, evaluated via
    `new Function` — no injected-globals mock, no IIFE execution.

    GREEN in task #7257 step-10.
    """

    _MARK = "SCHEMAS_RESULT:"
    _REQUIRED_KEYS = {"capability", "verdict", "command", "exit_code"}
    _VERDICT_ENUM = ["PASS", "FAIL", "UNPROVABLE", "HARNESS_ERROR"]

    def _harness_source(self) -> str:
        mjs_abs = _PDV_MJS.replace("\\", "\\\\")
        return f"""\
import {{ readFileSync }} from "node:fs";

const MARK = "{self._MARK}";
const MJS_PATH = "{mjs_abs}";

let src = readFileSync(MJS_PATH, "utf8");
const ANCHOR = "const _wfResult = await";
const anchorIdx = src.indexOf(ANCHOR);
if (anchorIdx === -1) {{
    console.error("ANCHOR_NOT_FOUND: " + ANCHOR);
    process.exit(1);
}}
let head = src.slice(0, anchorIdx);
head = head.replace("export const meta", "const meta");

let schemas;
try {{
    schemas = new Function(head + "\\nreturn {{ RESULTS_SCHEMA, VERDICT_SCHEMA }};")();
}} catch (e) {{
    console.error("SCHEMA_EXTRACT_FAILED: " + e.message);
    process.exit(1);
}}
console.log(MARK + JSON.stringify(schemas));
"""

    def _schemas(self) -> dict:
        """Run the extraction harness and return {RESULTS_SCHEMA, VERDICT_SCHEMA}."""
        harness_src = self._harness_source()
        result = subprocess.run(
            ["node", "--input-type=module"],
            input=harness_src, capture_output=True, text=True, timeout=30,
        )
        self.assertEqual(
            result.returncode, 0,
            f"node exited {result.returncode}; stderr: {result.stderr!r}",
        )
        lines = [ln for ln in result.stdout.splitlines() if ln.startswith(self._MARK)]
        self.assertTrue(lines, f"no schema marker in stdout; stdout: {result.stdout!r}")
        return json.loads(lines[-1][len(self._MARK):])

    def _record_items(self, role: str) -> dict:
        """The `items` sub-schema constraining one α result record for `role`."""
        results = self._schemas()["RESULTS_SCHEMA"]
        self.assertIn(role, results["properties"],
                      f"RESULTS_SCHEMA has no {role!r} property")
        prop = results["properties"][role]
        self.assertIn("items", prop, f"{role} declares no `items` record constraint")
        return prop["items"]

    # ── (1)(2) both roles constrain the record's required keys ───────────────

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs schema test")
    def test_prover_items_require_the_evidence_keys(self):
        """A prover record must declare capability, verdict, command AND exit_code."""
        items = self._record_items("prover")
        required = set(items.get("required", []))
        self.assertTrue(
            self._REQUIRED_KEYS.issubset(required),
            f"prover items.required is missing {self._REQUIRED_KEYS - required}; "
            f"got {sorted(required)}",
        )

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs schema test")
    def test_adversary_items_require_the_evidence_keys(self):
        """The Adversary is constrained too — not just the Prover.

        The Adversary is the role that can only ADD blocking signals, so an
        unconstrained adversary record is the cheapest route to a vacuous block.
        """
        items = self._record_items("adversary")
        required = set(items.get("required", []))
        self.assertTrue(
            self._REQUIRED_KEYS.issubset(required),
            f"adversary items.required is missing {self._REQUIRED_KEYS - required}; "
            f"got {sorted(required)}",
        )

    # ── (3) the verdict vocabulary is closed ─────────────────────────────────

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs schema test")
    def test_verdict_is_a_closed_enum(self):
        """PRD §6 decision 3 fixes the verdict vocabulary; the schema pins it."""
        for role in ("prover", "adversary"):
            with self.subTest(role=role):
                props = self._record_items(role).get("properties", {})
                self.assertIn("verdict", props, f"{role} items declares no verdict")
                self.assertEqual(props["verdict"].get("enum"), self._VERDICT_ENUM)

    # ── (4) the constraint that kills the string-command shape at source ─────

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs schema test")
    def test_command_is_an_array_of_strings(self):
        """`command` must be argv tokens, never a ready-to-paste shell string."""
        for role in ("prover", "adversary"):
            with self.subTest(role=role):
                props = self._record_items(role).get("properties", {})
                self.assertIn("command", props, f"{role} items declares no command")
                self.assertEqual(
                    props["command"],
                    {"type": "array", "items": {"type": "string"}},
                    f"{role} command must be an array of strings; got {props['command']!r}",
                )

    # ── (5) exit_code is an integer, and null is not an accepted type ────────

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs schema test")
    def test_exit_code_is_an_integer_and_not_nullable(self):
        """A null exit_code is the unexecuted-promise shape; the schema rejects it."""
        for role in ("prover", "adversary"):
            with self.subTest(role=role):
                props = self._record_items(role).get("properties", {})
                self.assertIn("exit_code", props, f"{role} items declares no exit_code")
                declared = props["exit_code"].get("type")
                self.assertEqual(declared, "integer",
                                 f"{role} exit_code.type must be 'integer'; got {declared!r}")
                self.assertNotIn(
                    "null", declared if isinstance(declared, list) else [declared],
                    f"{role} exit_code must not accept null",
                )

    # ── (6) VERDICT_SCHEMA declares, but does not require, the new fields ────

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs schema test")
    def test_verdict_schema_declares_the_new_batchverdict_fields(self):
        """The harness now emits malformed/fixture_absent/executed/total."""
        props = self._schemas()["VERDICT_SCHEMA"].get("properties", {})
        for key in ("malformed", "fixture_absent", "executed", "total"):
            self.assertIn(key, props,
                          f"VERDICT_SCHEMA declares no {key!r}; got {sorted(props)}")

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs schema test")
    def test_verdict_schema_required_is_unchanged(self):
        """The new fields are DECLARED, not REQUIRED.

        Requiring them would hard-fail the Synthesize agent against any harness
        build that predates them, turning a compatible additive change into an
        outage.  The .mjs defaults them with `?? []` / `?? 0` instead.
        """
        required = self._schemas()["VERDICT_SCHEMA"].get("required", [])
        self.assertEqual(required, ["blocks", "blocking", "report"])


# ---------------------------------------------------------------------------
# task #7257 steps 11/13: parameterized .mjs scenario harness
# ---------------------------------------------------------------------------

_MJS_RESULT_MARK = "SCENARIO_RESULT:"
_MJS_PHASES_MARK = "SCENARIO_PHASES:"


def _mjs_scenario_source(leaves_js: str, responses_js: str) -> str:
    """Build a Node ESM harness that runs the FULL .mjs body under a mock of
    Workflow's injected globals and ONLY those, with a PARAMETERIZED agent.

    Args:
        leaves_js:    a JS expression for globalThis.args (the leaf array).
        responses_js: a JS expression evaluating to an object mapping a
                      lower-cased phase name to either a value or a
                      (prompt, opts) => value function.

    The harness records every phase the .mjs actually invokes, so a test can
    assert on stages that were SKIPPED as well as on the returned verdict.
    """
    mjs_abs = _PDV_MJS.replace("\\", "\\\\")
    return f"""\
import {{ readFileSync }} from "node:fs";

const MJS_PATH = "{mjs_abs}";
const RESULT_MARK = "{_MJS_RESULT_MARK}";
const PHASES_MARK = "{_MJS_PHASES_MARK}";

// ── mock: agent(prompt, opts) — parameterized, and records each phase ────────
globalThis.__PHASES = [];
const RESPONSES = {responses_js};
globalThis.agent = async (prompt, opts = {{}}) => {{
    const phase = (opts.phase || "").toLowerCase();
    globalThis.__PHASES.push(phase);
    const r = RESPONSES[phase];
    if (typeof r === "function") return r(prompt, opts);
    if (r !== undefined) return r;
    return {{}};
}};

// ── mock: pipeline(items, ...stages) — threads each item through in order ────
globalThis.pipeline = async (items, ...stages) => {{
    const results = [];
    for (const item of items) {{
        let val = item;
        for (const stage of stages) {{
            val = await stage(val, item, results.length);
        }}
        results.push(val);
    }}
    return results;
}};

globalThis.parallel = async (thunks) => Promise.all(thunks.map(t => t()));
globalThis.__LOG_LINES = [];
globalThis.log = (..._a) => {{ globalThis.__LOG_LINES.push(_a.map(String).join(" ")); }};
globalThis.phase = (..._a) => {{}};
globalThis.args = {leaves_js};
globalThis.budget = {{ total: null, spent: () => 0, remaining: () => Infinity }};
globalThis.workflow = async () => {{}};

// ── execute the .mjs body and capture its top-level return ──────────────────
let src = readFileSync(MJS_PATH, "utf8");
src = src.replace("export const meta", "const meta");
const AsyncFunction = Object.getPrototypeOf(async function () {{}}).constructor;
const result = await new AsyncFunction(src)();
console.log(RESULT_MARK + JSON.stringify(result));
console.log(PHASES_MARK + JSON.stringify(globalThis.__PHASES));
"""


class _MjsScenarioMixin:
    """Shared runner for the parameterized .mjs scenario harness."""

    # A leaf that enumerates one premise, probes it, and synthesizes a clean,
    # EVIDENCE-BACKED verified verdict — the "normal path" control.
    VERIFIED_RESPONSES = """{
    enumerate: { premises: [{
        text: "revolute rejects non-axis arg",
        assertion_kind: "rejection",
        fixture: "tests/prd-gate/fixtures/revolute_silent_accept.ri",
        match: { exit_code: 1 },
        capability: "arg-vs-param rejection (mock)",
    }] },
    prove: { prover: [{
        capability: "arg-vs-param rejection (mock)",
        probe_kind: "check",
        verdict: "PASS",
        command: ["reify", "check", "f.ri"],
        exit_code: 1,
        stdout: "",
        stderr: "type mismatch",
    }], adversary: [] },
    adversary: { prover: [], adversary: [] },
    synthesize: { blocks: false, blocking: [], report: "",
                  malformed: [], fixture_absent: [], executed: 1, total: 1 },
}"""

    # A leaf whose Enumerator returns nothing at all.
    UNENUMERATED_RESPONSES = """{
    enumerate: { premises: [] },
    prove: { prover: [], adversary: [] },
    adversary: { prover: [], adversary: [] },
    synthesize: { blocks: false, blocking: [], report: "",
                  malformed: [], fixture_absent: [], executed: 0, total: 0 },
}"""

    def _run_scenario(self, leaves_js: str, responses_js: str):
        """Run the .mjs under the scenario harness; return (verdict, phases)."""
        harness_src = _mjs_scenario_source(leaves_js, responses_js)
        result = subprocess.run(
            ["node", "--input-type=module"],
            input=harness_src, capture_output=True, text=True, timeout=60,
        )
        self.assertEqual(
            result.returncode, 0,
            f"node exited {result.returncode}; stderr: {result.stderr!r}; "
            f"stdout: {result.stdout!r}",
        )
        res_lines = [ln for ln in result.stdout.splitlines()
                     if ln.startswith(_MJS_RESULT_MARK)]
        ph_lines = [ln for ln in result.stdout.splitlines()
                    if ln.startswith(_MJS_PHASES_MARK)]
        self.assertTrue(res_lines, f"no result marker; stdout: {result.stdout!r}")
        self.assertTrue(ph_lines, f"no phases marker; stdout: {result.stdout!r}")
        verdict = json.loads(res_lines[-1][len(_MJS_RESULT_MARK):])
        phases = json.loads(ph_lines[-1][len(_MJS_PHASES_MARK):])
        return verdict, phases


# ---------------------------------------------------------------------------
# task #7257 step-11 (RED): a zero-premise leaf is not a verified leaf (ARM 2)
# ---------------------------------------------------------------------------

class TestMjsUnenumeratedLeaf(unittest.TestCase, _MjsScenarioMixin):
    """A leaf whose Enumerator returned zero premises was never probed.

    Measured on this branch BEFORE the fix, driving one zero-premise leaf:
        leaf_verdicts[0] == {leafLabel, blocks: false, blocking: [], report: ""}
        summary          == "γ PASS — all 1 leaf(ves) verified"
        phases           == ["enumerate", "synthesize"]

    That is byte-identical in shape to a genuinely verified leaf, so the ONE
    signal that matters — "nothing was checked here" — is unrecoverable from
    the return value.  It also still burned a Synthesize agent call to
    synthesize an empty record set (α over {} is vacuously non-blocking).

    GREEN in task #7257 step-12.
    """

    _LABEL = "zero-premise leaf (delta)"
    _LEAF_JS = '[{ signal: "zero-premise leaf (delta)" }]'

    def _unenumerated(self):
        return self._run_scenario(self._LEAF_JS, self.UNENUMERATED_RESPONSES)

    # ── (1)(2) the per-leaf disposition ──────────────────────────────────────

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_leaf_verdict_carries_unenumerated_disposition(self):
        """The leaf verdict says, in one field, that nothing was enumerated."""
        verdict, _ = self._unenumerated()
        self.assertEqual(len(verdict["leaf_verdicts"]), 1)
        leaf = verdict["leaf_verdicts"][0]
        self.assertEqual(
            leaf.get("disposition"), "UNENUMERATED",
            f"zero-premise leaf must be dispositioned UNENUMERATED; got {leaf!r}",
        )

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_unenumerated_differs_from_a_verified_leaf(self):
        """The whole point: the two outcomes must be distinguishable."""
        unenum, _ = self._unenumerated()
        verified, _ = self._run_scenario(
            '[{ signal: "normally verified leaf" }]', self.VERIFIED_RESPONSES)

        unenum_leaf = unenum["leaf_verdicts"][0]
        verified_leaf = verified["leaf_verdicts"][0]

        self.assertEqual(verified_leaf.get("disposition"), "VERIFIED",
                         f"control leaf must be VERIFIED; got {verified_leaf!r}")
        self.assertNotEqual(
            unenum_leaf.get("disposition"), verified_leaf.get("disposition"),
            "a never-probed leaf must not share a disposition with a verified one",
        )
        # Both are non-blocking — which is exactly why `blocks` alone is not enough.
        self.assertFalse(unenum_leaf["blocks"])
        self.assertFalse(verified_leaf["blocks"])

    # ── (3)(4)(5) the batch-level signal ─────────────────────────────────────

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_batch_disposition_is_not_pass(self):
        """A batch that probed nothing did not pass."""
        verdict, _ = self._unenumerated()
        self.assertIn("disposition", verdict,
                      f"batch verdict has no disposition; keys {sorted(verdict)}")
        self.assertNotEqual(verdict["disposition"], "PASS")

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_unenumerated_leaf_label_is_listed_at_top_level(self):
        """The caller can name the never-probed leaf without opening journal.jsonl."""
        verdict, _ = self._unenumerated()
        self.assertIn("unenumerated_leaves", verdict,
                      f"batch verdict has no unenumerated_leaves; keys {sorted(verdict)}")
        self.assertEqual(verdict["unenumerated_leaves"], [self._LABEL])

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_summary_names_the_leaf_and_the_probed_count(self):
        """The one-line summary must not read as a pass."""
        verdict, _ = self._unenumerated()
        summary = verdict["summary"]
        self.assertIn(self._LABEL, summary,
                      f"summary must name the never-probed leaf; got {summary!r}")
        self.assertIn("0 of 1", summary,
                      f"summary must state the probed count; got {summary!r}")
        self.assertNotIn("PASS", summary,
                         f"a never-probed batch must not summarize as PASS; got {summary!r}")

    # ── (6) the stage-3 short-circuit ────────────────────────────────────────

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_synthesize_agent_is_not_called_for_a_zero_premise_leaf(self):
        """There is nothing to synthesize — do not pay an agent to say so.

        α over an empty record set is vacuously non-blocking, so the call could
        only ever return a clean verdict.  Skipping it is both cheaper and one
        less way to launder 'nothing ran' into 'nothing failed'.
        """
        _, phases = self._unenumerated()
        self.assertNotIn(
            "synthesize", phases,
            f"stage 3 must short-circuit on an unenumerated leaf; phases {phases!r}",
        )
        self.assertIn("enumerate", phases,
                      f"the Enumerator must still run; phases {phases!r}")

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_verified_control_leaf_still_calls_synthesize(self):
        """The short-circuit is scoped to the empty case — regression guard."""
        _, phases = self._run_scenario(
            '[{ signal: "normally verified leaf" }]', self.VERIFIED_RESPONSES)
        self.assertIn("synthesize", phases,
                      f"the normal path must still synthesize; phases {phases!r}")


# ---------------------------------------------------------------------------
# task #7257 step-13 (RED): the batch return must surface the probed count
# ---------------------------------------------------------------------------

class TestMjsBatchDisposition(unittest.TestCase, _MjsScenarioMixin):
    """A session must be able to answer "how many leaves were actually probed?"
    from the workflow's return value, without opening journal.jsonl.

    Today the aggregate carries only blocks / leaf_verdicts / summary, so a
    batch in which NO leaf was ever probed is reported as
    "γ PASS — all N leaf(ves) verified" — the ARM 2 failure.  A third
    disposition is needed between BLOCKS and PASS: INCOMPLETE, meaning nothing
    was falsified but nothing was verified either.

    GREEN in task #7257 step-14.
    """

    # ── scenario A: one verified leaf + one zero-premise leaf ────────────────

    _A_LEAVES = '[{ signal: "verified leaf (alpha)" }, { signal: "zero-premise leaf (beta)" }]'
    _A_RESPONSES = """{
    enumerate: (prompt) => prompt.includes("zero-premise")
        ? { premises: [] }
        : { premises: [{
              text: "revolute rejects non-axis arg",
              assertion_kind: "rejection",
              fixture: "tests/prd-gate/fixtures/revolute_silent_accept.ri",
              match: { exit_code: 1 },
              capability: "arg-vs-param rejection (mock)",
          }] },
    prove: { prover: [{
        capability: "arg-vs-param rejection (mock)",
        probe_kind: "check", verdict: "PASS",
        command: ["reify", "check", "f.ri"], exit_code: 1,
        stdout: "", stderr: "type mismatch",
    }], adversary: [] },
    adversary: { prover: [], adversary: [] },
    synthesize: { blocks: false, blocking: [], report: "",
                  malformed: [], fixture_absent: [], executed: 1, total: 1 },
}"""

    # ── scenario B: two normally verified leaves ─────────────────────────────

    _B_LEAVES = '[{ signal: "verified leaf (alpha)" }, { signal: "verified leaf (gamma)" }]'

    # ── scenario C: a leaf that probed nothing because every record was malformed ─

    _C_LEAVES = '[{ signal: "malformed-records leaf (epsilon)" }]'
    _C_RESPONSES = """{
    enumerate: { premises: [{
        text: "revolute rejects non-axis arg",
        assertion_kind: "rejection",
        fixture: "tests/prd-gate/fixtures/revolute_silent_accept.ri",
        match: { exit_code: 1 },
        capability: "arg-vs-param rejection (mock)",
    }] },
    prove: { prover: [{
        capability: "arg-vs-param rejection (mock)",
        probe_kind: "check", verdict: "FAIL",
        command: [], exit_code: -1, stdout: "", stderr: "",
    }], adversary: [] },
    adversary: { prover: [], adversary: [] },
    synthesize: { blocks: false, blocking: [],
                  report: "MALFORMED (no executed-probe evidence ...)",
                  malformed: ["arg-vs-param rejection (mock)"],
                  fixture_absent: [], executed: 0, total: 1 },
}"""

    def _assert_consumer_contract(self, verdict):
        """The pre-existing keys β/D4 consume must survive every change."""
        for key in ("blocks", "leaf_verdicts", "summary"):
            self.assertIn(key, verdict,
                          f"consumer-contract key {key!r} missing; got {sorted(verdict)}")
        self.assertIsInstance(verdict["blocks"], bool)
        self.assertIsInstance(verdict["leaf_verdicts"], list)
        self.assertIsInstance(verdict["summary"], str)

    # ── (A) a mixed batch is INCOMPLETE, not PASS ────────────────────────────

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_mixed_batch_reports_probed_counts(self):
        """One of two leaves was probed — the return must say exactly that."""
        verdict, _ = self._run_scenario(self._A_LEAVES, self._A_RESPONSES)
        self._assert_consumer_contract(verdict)
        self.assertEqual(verdict.get("leaves_total"), 2)
        self.assertEqual(verdict.get("leaves_probed"), 1)
        self.assertEqual(verdict.get("leaves_unenumerated"), 1)

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_mixed_batch_names_the_unprobed_leaf(self):
        """The never-probed leaf is nameable straight off the return value."""
        verdict, _ = self._run_scenario(self._A_LEAVES, self._A_RESPONSES)
        self.assertEqual(verdict.get("unenumerated_leaves"),
                         ["zero-premise leaf (beta)"])

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_mixed_batch_disposition_is_incomplete_and_does_not_block(self):
        """INCOMPLETE is the third outcome: nothing falsified, nothing verified."""
        verdict, _ = self._run_scenario(self._A_LEAVES, self._A_RESPONSES)
        self.assertEqual(verdict.get("disposition"), "INCOMPLETE")
        self.assertFalse(verdict["blocks"],
                         "an incomplete batch has falsified nothing — it must not block")

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_mixed_batch_summary_states_the_probed_count(self):
        """The one-line summary carries the count a reader actually needs."""
        verdict, _ = self._run_scenario(self._A_LEAVES, self._A_RESPONSES)
        summary = verdict["summary"]
        self.assertIn("1 of 2", summary, f"summary must state 1 of 2; got {summary!r}")
        self.assertIn("zero-premise leaf (beta)", summary,
                      f"summary must name the never-probed leaf; got {summary!r}")
        self.assertNotIn("PASS", summary, f"INCOMPLETE must not read PASS; got {summary!r}")

    # ── (B) an all-verified batch still passes, and says on what basis ───────

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_all_verified_batch_disposition_is_pass(self):
        """The happy path is unchanged in outcome — regression guard."""
        verdict, _ = self._run_scenario(self._B_LEAVES, self.VERIFIED_RESPONSES)
        self._assert_consumer_contract(verdict)
        self.assertEqual(verdict.get("disposition"), "PASS")
        self.assertFalse(verdict["blocks"])
        self.assertEqual(verdict.get("leaves_probed"), verdict.get("leaves_total"))
        self.assertEqual(verdict.get("leaves_total"), 2)
        self.assertEqual(verdict.get("leaves_unenumerated"), 0)

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_all_verified_summary_reports_the_probed_count(self):
        """Even a pass states its basis, so 'verified' is never taken on trust."""
        verdict, _ = self._run_scenario(self._B_LEAVES, self.VERIFIED_RESPONSES)
        summary = verdict["summary"]
        self.assertIn("PASS", summary, f"an all-verified batch passes; got {summary!r}")
        self.assertIn("2 probed", summary,
                      f"summary must state the probed count; got {summary!r}")

    # ── (C) a leaf that executed nothing is NOT_VERIFIED, batch INCOMPLETE ───

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_leaf_with_no_executed_records_is_not_verified(self):
        """executed === 0 with malformed records ⇒ that leaf verified nothing."""
        verdict, _ = self._run_scenario(self._C_LEAVES, self._C_RESPONSES)
        self._assert_consumer_contract(verdict)
        leaf = verdict["leaf_verdicts"][0]
        self.assertEqual(leaf.get("disposition"), "NOT_VERIFIED",
                         f"leaf that executed no probe must be NOT_VERIFIED; got {leaf!r}")

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_malformed_records_are_counted_at_batch_level(self):
        """The harness-defect count is visible without walking leaf_verdicts."""
        verdict, _ = self._run_scenario(self._C_LEAVES, self._C_RESPONSES)
        self.assertEqual(verdict.get("malformed_records"), 1)
        self.assertEqual(verdict.get("fixture_absent_records"), 0)

    @unittest.skipUnless(_NODE_ON_PATH, "node not on PATH; skip .mjs scenario test")
    def test_not_verified_leaf_makes_the_batch_incomplete_without_blocking(self):
        """A harness defect is neither a pass nor a falsification."""
        verdict, _ = self._run_scenario(self._C_LEAVES, self._C_RESPONSES)
        self.assertEqual(verdict.get("disposition"), "INCOMPLETE")
        self.assertFalse(verdict["blocks"],
                         "a malformed record is a harness defect, not a falsification")
        self.assertEqual(verdict.get("leaves_not_verified"), 1)
        self.assertEqual(verdict.get("not_verified_leaves"),
                         ["malformed-records leaf (epsilon)"])


if __name__ == "__main__":
    unittest.main()
