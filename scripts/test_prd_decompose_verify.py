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
        return {{ blocks: false, blocking: [], report: "" }};
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


if __name__ == "__main__":
    unittest.main()
