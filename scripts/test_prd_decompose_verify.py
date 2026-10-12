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

import io
import json
import os
import tempfile
import unittest
import unittest.mock
from typing import Any

from _pdv_test_support import _PDV_MJS, _REPO_ROOT, _load_pcc, _load_pdv, _reify_missing_fixture_stderr

pdv = _load_pdv()
pcc = _load_pcc()

# ---------------------------------------------------------------------------
# Repo-root helpers for skip-guards
# ---------------------------------------------------------------------------

_REIFY_RELEASE = os.path.join(_REPO_ROOT, "target", "release", "reify")
_REIFY_DEBUG = os.path.join(_REPO_ROOT, "target", "debug", "reify")
_REIFY_BUILT = os.path.isfile(_REIFY_RELEASE) or os.path.isfile(_REIFY_DEBUG)

_LEAF_FALSE = os.path.join(_REPO_ROOT, "tests", "prd-gate", "leaf-false-premise.json")
_LEAF_TRUE = os.path.join(_REPO_ROOT, "tests", "prd-gate", "leaf-true-premise.json")

_LEAF_FIXTURE = "tests/prd-gate/fixtures/leaf.ri"


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

# ---------------------------------------------------------------------------
# #6876 step-11 (RED): the "value" assertion kind
# ---------------------------------------------------------------------------

class TestValueAssertionKind(unittest.TestCase):
    """Binding a premise that asserts something about the PRINTED VALUE.

    `ir` is exit-code-only on the clean branch, so a premise of the shape "eval
    exits 0 AND prints a finite in-range number" bound as `ir` asserts only the
    first half.  `value` is the kind that can see the second half; this pins
    that an Enumerator emitting `assertion_kind="value"` reaches it.

    The non-vacuity rules are NOT re-implemented here — they are α's, inherited
    through pcc.load_probe_set, and that is exactly what these tests measure.
    """

    _FIXTURE = "tests/prd-gate/fixtures/value_clean_eval_cells.ri"
    _SPEC = {
        "pattern": r"ValueCleanEvalCells\.damping_ratio = ([-+0-9.eE]+)",
        "min": 0.01,
        "max": 0.03,
    }

    def _premise(self, match=None, fixture=None, assertion_kind="value"):
        return pdv.Premise(
            text="damping_ratio evaluates to a finite in-range number",
            assertion_kind=assertion_kind,
            fixture=fixture if fixture is not None else self._FIXTURE,
            match=match if match is not None else {"stdout_value": dict(self._SPEC)},
            capability="damping ratio is finite and in range",
        )

    # ── premise_to_probe ──────────────────────────────────────────────────────

    def test_value_binds_value_probe_kind(self):
        self.assertEqual(
            pdv.premise_to_probe(self._premise())["probe_kind"], "value"
        )

    def test_value_binds_observation_present(self):
        """The asserted value IS present — the same polarity the rejection row
        documents: probe that the assertion HOLDS, not that a failure is absent."""
        self.assertEqual(
            pdv.premise_to_probe(self._premise())["expected"]["observation"],
            "present",
        )

    def test_match_is_passed_through_verbatim(self):
        """α owns the predicate's shape; D3 must not reshape it on the way."""
        match = {"stdout_value": dict(self._SPEC)}
        probe = pdv.premise_to_probe(self._premise(match=match))
        self.assertEqual(probe["expected"]["match"], match)

    def test_fixture_path_is_preserved(self):
        probe = pdv.premise_to_probe(self._premise())
        self.assertEqual(probe["fixture"], self._FIXTURE)

    def test_unknown_assertion_kind_still_rejected_and_lists_value(self):
        with self.assertRaises(ValueError) as ctx:
            pdv.premise_to_probe(self._premise(assertion_kind="nonsense"))
        message = str(ctx.exception)
        self.assertIn("nonsense", message)
        for kind in ("rejection", "parses", "resolves", "produces", "ir", "value"):
            self.assertIn(kind, message)

    # ── bind_premises → α round-trip ──────────────────────────────────────────

    def test_bind_premises_round_trips_through_alpha(self):
        result = pdv.bind_premises([self._premise()])
        probes = pcc.load_probe_set(json.dumps(result))
        self.assertEqual(len(probes), 1)
        self.assertEqual(probes[0].probe_kind, "value")
        self.assertEqual(
            probes[0].expected["match"]["stdout_value"], self._SPEC
        )

    def test_bind_premises_rejects_a_value_premise_with_no_predicate(self):
        """Inherited from α's single validation site, not re-checked here."""
        with self.assertRaises(ValueError) as ctx:
            pdv.bind_premises([self._premise(match={})])
        self.assertIn("stdout_value", str(ctx.exception))

    def test_bind_premises_rejects_a_value_premise_with_no_constraint(self):
        with self.assertRaises(ValueError) as ctx:
            pdv.bind_premises([
                self._premise(match={"stdout_value": {"pattern": r"X = ([0-9.]+)"}})
            ])
        message = str(ctx.exception)
        self.assertIn("probe[0]", message)
        for constraint in ("min", "max", "finite"):
            self.assertIn(constraint, message)

    # ── REGRESSION: the five existing bindings are unchanged ──────────────────

    def test_existing_bindings_unchanged(self):
        expected = {
            "rejection": ("check", "present"),
            "parses": ("grammar", "present"),
            "resolves": ("check", "present"),
            "produces": ("ir", "present"),
            "ir": ("ir", "absent"),
        }
        for kind, (probe_kind, observation) in expected.items():
            with self.subTest(assertion_kind=kind):
                self.assertEqual(pdv._ASSERTION_KIND_MAP[kind], (probe_kind, observation))


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
            stderr=_reify_missing_fixture_stderr("/fixture.ri"),
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

    @unittest.skipUnless(_REIFY_BUILT, "reify binary not built; skip boundary e2e")
    def test_real_alpha_missing_fixture_is_fixture_absent(self):
        """Real α + reify on a never-written fixture is fixture-absent, not blocking.

        Pins the fixture anchor against what α actually records, so #7257's
        false block cannot quietly return.
        """
        with tempfile.TemporaryDirectory(prefix="pdv_e2e_absent_") as tmpdir:
            fixture = os.path.join(tmpdir, "never_written.ri")
            results = self._run_probes_with_alpha({"probes": [
                {"capability": "ir absent", "probe_kind": "ir", "fixture": fixture,
                 "expected": {"observation": "present",
                              "match": {"stderr_contains": "E_NEVER"}}},
                {"capability": "check absent", "probe_kind": "check",
                 "fixture": fixture,
                 "expected": {"observation": "present", "match": {"exit_code": 0}}},
            ]})
        bv = pdv.synthesize_batch({"prover": results, "adversary": []})
        self.assertEqual(sorted(bv.fixture_absent), ["check absent", "ir absent"],
                         bv.report)
        self.assertFalse(bv.blocks, bv.report)


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
            "command": ["reify", "eval", _LEAF_FIXTURE],
            "exit_code": exit_code,
            "stdout": "",
            "stderr": stderr,
        }

    # ── (1) the verbatim stderr from the observed run ────────────────────────

    def test_observed_enoent_stderr_is_not_a_falsification(self):
        """reify's 'Error reading <fixture>: ... (os error 2)' → fixture-absent."""
        rec = self._result("fixture-absent cap",
                           stderr=_reify_missing_fixture_stderr(_LEAF_FIXTURE))
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertNotIn("fixture-absent cap", bv.blocking)
        self.assertFalse(bv.blocks)
        self.assertIn("fixture-absent cap", bv.fixture_absent)

    # ── (2) both signature forms, case-insensitively ─────────────────────────

    def test_bare_phrase_without_the_fixture_is_not_enough(self):
        """A bare 'No such file or directory' names no path, so it still blocks."""
        rec = self._result("bare-enoent cap", stderr="No such file or directory")
        self.assertFalse(pdv.fixture_absent_evidence(rec))
        self.assertEqual(pdv.classify_record(rec), pdv.CAT_BLOCKING)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertTrue(bv.blocks)
        self.assertIn("bare-enoent cap", bv.blocking)
        self.assertEqual(bv.fixture_absent, [])

    def test_signature_match_is_case_insensitive(self):
        """Lower-cased diagnostics match too — the signature is normalized."""
        rec = self._result(
            "lowercase-enoent cap",
            stderr=f"error reading {_LEAF_FIXTURE}: no such file or directory")
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertIn("lowercase-enoent cap", bv.fixture_absent)
        self.assertFalse(bv.blocks)

    # ── (3) the Rust io::Error rendering on its own ──────────────────────────

    def test_bare_os_error_2_signature(self):
        """Rust renders ENOENT as 'os error 2'; that alone classifies fixture-absent."""
        rec = self._result("os-error-2 cap",
                           stderr=f"failed to open input {_LEAF_FIXTURE}: os error 2")
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
                           stderr=_reify_missing_fixture_stderr(_LEAF_FIXTURE))
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertEqual(bv.executed, 1)
        self.assertEqual(bv.total, 1)

    def test_fixture_absent_is_named_in_its_own_report_section(self):
        """The capability stays visible, under a fixture-absent label."""
        rec = self._result("fixture-absent cap",
                           stderr=_reify_missing_fixture_stderr(_LEAF_FIXTURE))
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertIn("fixture-absent cap", bv.report)
        self.assertIn("FIXTURE ABSENT", bv.report)

    def test_fixture_absent_is_not_counted_as_malformed(self):
        """The two categories are distinct: one ran without a target, one never ran."""
        rec = self._result("fixture-absent cap",
                           stderr=_reify_missing_fixture_stderr(_LEAF_FIXTURE))
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
# task #7257 step-17 (RED): the errno signature must be anchored, not a prefix
# ---------------------------------------------------------------------------

class TestFixtureAbsentErrnoAnchoring(unittest.TestCase):
    """`os error 2` as a bare substring swallows every errno that STARTS with 2.

    `_FIXTURE_ABSENT_SIGNATURES` matches with `any(sig in lowered ...)`, so the
    literal `"os error 2"` is a PREFIX test, not an errno test: it fires on
    ENOTDIR(20), EISDIR(21), EINVAL(22), EMFILE(24), ENOSPC(28) and every
    3-digit 2XX errno.  Each of those is a genuine, executed falsification that
    silently stops blocking — the fixture-absent carve-out (which exists so a
    not-yet-written deliverable is not scored as a design defect) becomes a
    hole that swallows real findings.  This is the exact failure mode the
    carve-out was added to avoid, inverted.

    GREEN in task #7257 step-18 (anchored `os error 2(?![0-9])`).

    The class carries BOTH halves deliberately: the hostile errnos that must
    start blocking again, AND the three genuine ENOENT spellings the suite
    already depends on, so the fix's blast radius is pinned from both sides.
    """

    #: (label, stderr) for errnos whose decimal rendering begins with "2" but
    #: which are NOT ENOENT.  Every one is a real failure that must block.
    HOSTILE_ERRNOS = (
        ("ENOTDIR", f"Error reading {_LEAF_FIXTURE}: Not a directory (os error 20)"),
        ("EISDIR", f"Error reading {_LEAF_FIXTURE}: Is a directory (os error 21)"),
        ("EINVAL", f"Error reading {_LEAF_FIXTURE}: Invalid argument (os error 22)"),
        ("EMFILE", f"Error reading {_LEAF_FIXTURE}: Too many open files (os error 24)"),
        ("ENOSPC",
         f"Error reading {_LEAF_FIXTURE}: No space left on device (os error 28)"),
    )
    ERRNO_212 = f"probe aborted reading {_LEAF_FIXTURE} (os error 212)"

    def _result(self, capability: str, verdict: str = "FAIL",
                stderr: str = "", exit_code: int = 1) -> dict:
        """An EXECUTED α result record (real command, real exit code).

        Same shape as TestFixtureAbsent._result — the evidence gate must be
        satisfied so classification reaches the fixture-absent branch at all.
        """
        return {
            "capability": capability,
            "probe_kind": "ir",
            "verdict": verdict,
            "command": ["reify", "eval", _LEAF_FIXTURE],
            "exit_code": exit_code,
            "stdout": "",
            "stderr": stderr,
        }

    # ── (1) every 2X errno is a real falsification and must block ────────────

    def test_two_x_errnos_still_block(self):
        """ENOTDIR/EISDIR/EINVAL/EMFILE/ENOSPC are falsifications, not absences."""
        for label, stderr in self.HOSTILE_ERRNOS:
            with self.subTest(errno=label):
                cap = f"{label} cap"
                rec = self._result(cap, stderr=stderr)
                bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
                self.assertTrue(
                    bv.blocks,
                    f"{label} ({stderr!r}) is a real failure and must block")
                self.assertIn(cap, bv.blocking)
                self.assertNotIn(cap, bv.fixture_absent)

    # ── (2) the 2XX range too ────────────────────────────────────────────────

    def test_three_digit_two_hundred_errno_still_blocks(self):
        """A 3-digit errno beginning with 2 must not be read as ENOENT either."""
        rec = self._result("errno-212 cap", stderr=self.ERRNO_212)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertTrue(bv.blocks)
        self.assertIn("errno-212 cap", bv.blocking)
        self.assertNotIn("errno-212 cap", bv.fixture_absent)

    # ── (3) unit-level sweep, so a regression localises to the matcher ───────

    def test_matcher_rejects_non_enoent_errnos_directly(self):
        """`fixture_absent_evidence` itself is False for every hostile errno.

        Asserted against the predicate rather than through `synthesize_batch`
        so a future regression points at the signature table, not at the
        classification pipeline downstream of it.
        """
        table = self.HOSTILE_ERRNOS + (("errno-212", self.ERRNO_212),)
        for label, stderr in table:
            with self.subTest(errno=label):
                rec = self._result(f"{label} cap", stderr=stderr)
                self.assertFalse(
                    pdv.fixture_absent_evidence(rec),
                    f"{stderr!r} is not a fixture-absent signature")

    # ── (4) REGRESSION PRESERVATION: genuine ENOENT still classifies ─────────

    def test_genuine_enoent_spellings_still_classify_fixture_absent(self):
        """The three spellings the suite already relies on must keep working.

        The anchor must permit a bare trailing `os error 2` at end-of-string
        (no closing paren) — requiring `)` would break the spelling asserted by
        TestFixtureAbsent.test_bare_os_error_2_signature.  The phrase-only
        spelling is the perror shape, `<program>: <path>: <strerror>`.
        """
        spellings = (
            ("paren", _reify_missing_fixture_stderr(_LEAF_FIXTURE)),
            ("bare-errno-eos", f"failed to open input {_LEAF_FIXTURE}: os error 2"),
            ("phrase-only", f"cat: {_LEAF_FIXTURE}: No such file or directory"),
        )
        for label, stderr in spellings:
            with self.subTest(spelling=label):
                cap = f"enoent-{label} cap"
                rec = self._result(cap, stderr=stderr)
                self.assertTrue(
                    pdv.fixture_absent_evidence(rec),
                    f"{stderr!r} is a genuine ENOENT spelling")
                bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
                self.assertIn(cap, bv.fixture_absent)
                self.assertFalse(bv.blocks)

    # ── (5) near-miss guard ──────────────────────────────────────────────────

    def test_no_such_device_blocks(self):
        """'No such device (os error 19)' matches NEITHER signature.

        Textually adjacent to ENOENT's phrase ("No such ...") and numerically
        adjacent to the 2X block, so it pins both edges at once.
        """
        rec = self._result(
            "enodev cap",
            stderr=f"Error reading {_LEAF_FIXTURE}: No such device (os error 19)")
        self.assertFalse(pdv.fixture_absent_evidence(rec))
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertTrue(bv.blocks)
        self.assertIn("enodev cap", bv.blocking)
        self.assertEqual(bv.fixture_absent, [])


# ---------------------------------------------------------------------------
# task #7304 (RED): the ENOENT must name the probe's own fixture
# ---------------------------------------------------------------------------

class TestFixtureAbsentNamesTheProbeTarget(unittest.TestCase):
    """Fixture-absent needs an ENOENT on a stderr line naming the probe's fixture.

    "Fixture absent" means the OS reported the probe's TARGET missing.  ENOENT
    text that is quoted, about another path, or on a line without the target
    says nothing about the target, so the FAIL it rides on must keep blocking.
    """

    def _result(self, capability: str, stderr: str, verdict: str = "FAIL",
                command=None) -> dict:
        return {
            "capability": capability,
            "probe_kind": "check",
            "verdict": verdict,
            "command": ["reify", "check", _LEAF_FIXTURE] if command is None else command,
            "exit_code": 1,
            "stdout": "",
            "stderr": stderr,
        }

    def _assert_blocks(self, rec: dict) -> None:
        cap = rec["capability"]
        self.assertFalse(pdv.fixture_absent_evidence(rec))
        self.assertEqual(pdv.classify_record(rec), pdv.CAT_BLOCKING)
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertTrue(bv.blocks)
        self.assertIn(cap, bv.blocking)
        self.assertEqual(bv.fixture_absent, [])

    def test_genuine_fail_quoting_the_enoent_text_blocks(self):
        self._assert_blocks(self._result(
            "quoted enoent cap",
            'AssertionError: expected the import diagnostic to read '
            '"No such file or directory (os error 2)", got "module \'lib\' not found"'))

    def test_enoent_about_a_different_file_blocks(self):
        self._assert_blocks(self._result(
            "other file cap",
            _reify_missing_fixture_stderr("tests/prd-gate/fixtures/helper_lib.ri")))

    def test_bare_errno_without_a_path_blocks(self):
        self._assert_blocks(self._result("bare errno cap", "failed to spawn: os error 2"))

    def test_relay_mislabelled_harness_enoent_blocks(self):
        """A FAIL whose argv is α's own invocation: the ENOENT is about α's script."""
        self._assert_blocks(self._result(
            "relayed harness cap",
            "python3: can't open file 'scripts/prd-capability-check.py': "
            "[Errno 2] No such file or directory",
            command=["python3", "scripts/prd-capability-check.py",
                     "--json", "/tmp/ps.json"]))

    def test_fixture_and_enoent_on_different_lines_blocks(self):
        self._assert_blocks(self._result(
            "split lines cap",
            f"warning: in {_LEAF_FIXTURE}: unused parameter\n"
            + _reify_missing_fixture_stderr("tests/prd-gate/fixtures/other.ri")))

    def test_argv_without_an_argument_blocks(self):
        self._assert_blocks(self._result(
            "no argument cap", "bash: reify: No such file or directory",
            command=["reify"]))

    def test_enoent_line_after_warnings_is_fixture_absent(self):
        rec = self._result(
            "warned absent cap",
            "warning: W_X: something\n" + _reify_missing_fixture_stderr(_LEAF_FIXTURE))
        self.assertEqual(pdv.classify_record(rec), pdv.CAT_FIXTURE_ABSENT)

    def test_longer_path_ending_in_the_fixture_name_blocks(self):
        """A relayed short fixture is a path token, not a suffix of any path."""
        for other in ("tests/prd-gate/fixtures/subleaf.ri", "/other/leaf.ri"):
            with self.subTest(other=other):
                self._assert_blocks(self._result(
                    f"suffix {other} cap", _reify_missing_fixture_stderr(other),
                    command=["reify", "check", "leaf.ri"]))

    def test_fixture_delimited_as_a_whole_token_is_fixture_absent(self):
        for label, stderr in (
            ("reify", _reify_missing_fixture_stderr("leaf.ri")),
            ("perror", "cat: leaf.ri: No such file or directory"),
            ("quoted", "can't open file 'leaf.ri': [Errno 2] No such file or directory"),
            ("backticked", "failed to read `leaf.ri` (os error 2)"),
        ):
            with self.subTest(spelling=label):
                rec = self._result(f"{label} cap", stderr,
                                   command=["reify", "check", "leaf.ri"])
                self.assertEqual(pdv.classify_record(rec), pdv.CAT_FIXTURE_ABSENT)

    def test_missing_grammar_fixture_still_blocks(self):
        """tree-sitter's missing-file report names neither the path nor ENOENT.

        Pins today's fail-closed outcome for grammar probes (#8122), whose
        fixture-absent case the reify-specific anchor cannot see.
        """
        self._assert_blocks(dict(
            self._result(
                "grammar absent cap",
                "Error: No files were found at or matched by the provided "
                "pathname/glob",
                command=["tree-sitter", "parse", "--quiet", _LEAF_FIXTURE]),
            probe_kind="grammar"))


# ---------------------------------------------------------------------------
# task #7257 step-15 (RED): the user-observable signal, end to end
# ---------------------------------------------------------------------------

class TestUserObservableSignal(unittest.TestCase):
    """One synthesize call must make all four ARM 1 sub-signals legible at once.

    This is the report a human actually reads.  The observed pi report scored
    five capabilities as blocking when exactly one had been probed, rendered a
    string command character-by-character, and stated nowhere how many of its
    records carried evidence.  A reader could not tell a falsification from an
    unexecuted promise, and had no count to check the verdict against.

    GREEN in task #7257 step-16 (the counts header).
    """

    _STRING_CMD = "target/release/reify eval f.ri"
    _EXPLODED = "t a r g e t"

    def _mixed_batch(self):
        """One executed FAIL + three evidence-free + one fixture-absent + one string-command."""
        return {
            "prover": [
                # (1) the ONE genuine, evidence-backed falsification
                {"capability": "REAL executed fail", "probe_kind": "check",
                 "verdict": "FAIL",
                 "command": ["reify", "check", "tests/prd-gate/fixtures/x.ri"],
                 "exit_code": 0, "stdout": "All constraints satisfied.", "stderr": ""},
                # (2) three unexecuted promises
                {"capability": "vacuous-1", "probe_kind": "check", "verdict": "FAIL",
                 "command": [], "exit_code": None, "stdout": "", "stderr": ""},
                {"capability": "vacuous-2", "probe_kind": "ir", "verdict": "UNPROVABLE",
                 "command": [], "stdout": "", "stderr": ""},
                {"capability": "vacuous-3", "probe_kind": "check", "verdict": "FAIL",
                 "exit_code": None, "stdout": "", "stderr": ""},
                # (3) a probe that ran but could not find its fixture
                {"capability": "fixture-absent cap", "probe_kind": "ir", "verdict": "FAIL",
                 "command": ["reify", "eval", "tests/prd-gate/fixtures/not-yet.ri"],
                 "exit_code": 1, "stdout": "",
                 "stderr": _reify_missing_fixture_stderr(
                     "tests/prd-gate/fixtures/not-yet.ri")},
            ],
            "adversary": [
                # (4) an evidence-backed falsification whose command is a STRING
                {"capability": "string-command cap", "probe_kind": "ir", "verdict": "FAIL",
                 "command": self._STRING_CMD, "exit_code": 1, "stdout": "",
                 "stderr": "assertion did not hold"},
            ],
        }

    def test_only_evidence_backed_records_block(self):
        """blocks is true, and `blocking` is EXACTLY the evidence-backed set.

        The string-command record blocks too, and correctly so — a string
        command normalizes to one token, so the record does carry executed-probe
        evidence.  What it must NOT do is render exploded (asserted below).
        The three vacuous records and the fixture-absent one are what disappear
        from `blocking`, which is the signal: 6 records in, 2 real findings out,
        not 6 undifferentiated blockers.
        """
        bv = pdv.synthesize_batch(self._mixed_batch())
        self.assertTrue(bv.blocks)
        self.assertEqual(sorted(bv.blocking), ["REAL executed fail", "string-command cap"])

    def test_the_three_unexecuted_promises_are_listed_as_malformed(self):
        """Each vacuous record is still named — just under the right heading."""
        bv = pdv.synthesize_batch(self._mixed_batch())
        self.assertEqual(sorted(bv.malformed), ["vacuous-1", "vacuous-2", "vacuous-3"])

    def test_the_fixture_absent_record_is_listed_separately(self):
        """A missing deliverable is its own category, not a falsification."""
        bv = pdv.synthesize_batch(self._mixed_batch())
        self.assertEqual(bv.fixture_absent, ["fixture-absent cap"])

    def test_the_string_command_renders_verbatim(self):
        """Captured evidence stays re-runnable."""
        bv = pdv.synthesize_batch(self._mixed_batch())
        self.assertIn(self._STRING_CMD, bv.report)
        self.assertNotIn(self._EXPLODED, bv.report)

    def test_report_opens_with_a_machine_readable_counts_header(self):
        """The report states its own basis on line 1, before any evidence block.

        Without this a reader has to count report sections by hand to learn how
        much of the batch was actually probed — which is exactly the arithmetic
        nobody did on the pi report.
        """
        bv = pdv.synthesize_batch(self._mixed_batch())
        header = bv.report.splitlines()[0]
        self.assertIn("records:", header, f"report header missing; got {header!r}")
        for token in ("6 total", "3 with executed-probe evidence", "2 blocking",
                      "3 malformed", "1 fixture-absent"):
            self.assertIn(token, header,
                          f"counts header must state {token!r}; got {header!r}")

    def test_counts_header_is_emitted_even_when_nothing_blocks(self):
        """An all-clear report still states the basis of its all-clear."""
        bv = pdv.synthesize_batch({
            "prover": [{"capability": "cap", "probe_kind": "check", "verdict": "PASS",
                        "command": ["reify", "check", "f.ri"], "exit_code": 0,
                        "stdout": "", "stderr": ""}],
            "adversary": [],
        })
        header = bv.report.splitlines()[0]
        self.assertIn("records:", header, f"report header missing; got {header!r}")
        self.assertIn("1 total", header)
        self.assertIn("0 blocking", header)

    def test_counts_header_agrees_with_the_structured_fields(self):
        """The header is derived from the same counters, never hand-maintained."""
        bv = pdv.synthesize_batch(self._mixed_batch())
        header = bv.report.splitlines()[0]
        self.assertIn(f"{bv.total} total", header)
        self.assertIn(f"{bv.executed} with executed-probe evidence", header)
        self.assertIn(f"{len(bv.blocking)} blocking", header)
        self.assertIn(f"{len(bv.malformed)} malformed", header)
        self.assertIn(f"{len(bv.fixture_absent)} fixture-absent", header)


# ---------------------------------------------------------------------------
# task #7257 amendment: HARNESS_ERROR is exempt from BOTH evidence downgrades
# ---------------------------------------------------------------------------

class TestHarnessErrorAlwaysBlocks(unittest.TestCase):
    """A HARNESS_ERROR blocks regardless of evidence, and is never fixture-absent.

    Two review findings, one root cause.  The evidence gate and the
    fixture-absent carve-out both exist to stop a PREMISE FALSIFICATION being
    claimed on no evidence.  HARNESS_ERROR makes no such claim — it reports
    that the probe machinery could not run — so routing it through either
    downgrade inverts its meaning and stops it blocking:

      - MALFORMED: the Prover prompt's own documented fallback for "I could not
        run anything at all" is `{verdict: "HARNESS_ERROR", command: [],
        exit_code: -1}`, which carries no evidence BY CONSTRUCTION.  Under a
        blanket gate, python3 missing / a wrong α path / a dead shell step
        silently became INCOMPLETE instead of BLOCKS.
      - FIXTURE_ABSENT: a harness-level ENOENT (`python3: can't open file ...
        [Errno 2] No such file or directory`) matched the ENOENT phrase and was
        reported as "the fixture is the leaf's own deliverable" — the opposite
        diagnosis.

    The blast radius is pinned in both directions: an evidence-free FAIL is
    still MALFORMED, and a FAIL carrying the same ENOENT stderr is still
    FIXTURE_ABSENT.
    """

    # The exact fallback record the Prover prompt tells the agent to emit.
    # _assert_the_mjs_prompt_still_emits_this_shape keeps it honest.
    def _prompt_fallback_record(self, capability: str = "leaf label (delta)") -> dict:
        return {
            "capability": capability,
            "probe_kind": "check",
            "verdict": "HARNESS_ERROR",
            "command": [],
            "exit_code": -1,
            "stdout": "",
            "stderr": "bind step exited 2: no such option --json",
        }

    def _executed(self, capability: str, verdict: str, stderr: str,
                  exit_code: int = 2) -> dict:
        """An EXECUTED record (real command, real exit code) with a chosen stderr."""
        return {
            "capability": capability,
            "probe_kind": "check",
            "verdict": verdict,
            "command": ["python3", "scripts/prd-capability-check.py",
                        "--json", "/tmp/ps.json"],
            "exit_code": exit_code,
            "stdout": "",
            "stderr": stderr,
        }

    # ── (1) the prompt's own fallback record produces the intended disposition ─

    def test_prompt_fallback_record_blocks(self):
        """The documented "nothing ran" record must still stop the batch."""
        rec = self._prompt_fallback_record()
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertTrue(bv.blocks,
                        "an evidence-free HARNESS_ERROR is the ONE path that means "
                        "'the harness itself died' — it must block")
        self.assertEqual(bv.blocking, ["leaf label (delta)"])

    def test_prompt_fallback_record_is_not_malformed(self):
        """It is not an unexecuted promise — nothing was promised."""
        rec = self._prompt_fallback_record()
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertEqual(bv.malformed, [])
        self.assertEqual(bv.fixture_absent, [])

    def test_prompt_fallback_record_is_classified_blocking(self):
        """Unit-level, so a regression localises to classify_record."""
        self.assertEqual(pdv.classify_record(self._prompt_fallback_record()),
                         pdv.CAT_BLOCKING)

    def test_the_mjs_prompt_still_emits_this_shape(self):
        """Drift guard: the record above is a COPY of the .mjs prompt template.

        If the prompt's fallback stops saying `command: [], exit_code: -1`, the
        tests above stop testing the shape that actually reaches the harness.
        """
        with open(_PDV_MJS, encoding="utf-8") as fh:
            src = fh.read()
        self.assertIn('verdict: "HARNESS_ERROR"', src,
                      "the Prover prompt no longer documents a HARNESS_ERROR fallback")
        self.assertIn("command: [], exit_code: -1", src,
                      "the fallback template no longer emits the evidence-free shape "
                      "these tests pin; re-sync _prompt_fallback_record")

    def test_evidence_free_harness_error_still_counts_zero_executed(self):
        """The exemption is about BLOCKING, not about faking evidence.

        `executed` must stay honest: nothing ran, so the counts header still
        reports 0 with executed-probe evidence even though the batch blocks.
        """
        bv = pdv.synthesize_batch({"prover": [self._prompt_fallback_record()],
                                   "adversary": []})
        self.assertEqual(bv.executed, 0)
        self.assertEqual(bv.total, 1)
        header = bv.report.splitlines()[0]
        self.assertIn("1 total", header)
        self.assertIn("0 with executed-probe evidence", header)
        self.assertIn("1 blocking", header)
        self.assertIn("0 malformed", header)

    # ── (2) a harness-level ENOENT is not a missing deliverable ──────────────

    def test_harness_error_with_enoent_stderr_still_blocks(self):
        """`python3: can't open file ...` is a broken harness, not an unwritten .ri."""
        rec = self._executed(
            "alpha script path cap", "HARNESS_ERROR",
            stderr="python3: can't open file 'scripts/prd-capability-check.py': "
                   "[Errno 2] No such file or directory")
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertTrue(bv.blocks)
        self.assertEqual(bv.blocking, ["alpha script path cap"])
        self.assertEqual(bv.fixture_absent, [],
                         "a harness-level ENOENT must not be reported as a missing "
                         "leaf deliverable")

    def test_harness_error_with_bare_errno_stderr_still_blocks(self):
        """The Rust-style spelling of the same failure is treated identically."""
        rec = self._executed("cwd cap", "HARNESS_ERROR",
                             stderr="failed to spawn probe: os error 2")
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertTrue(bv.blocks)
        self.assertEqual(bv.fixture_absent, [])

    def test_harness_error_enoent_is_classified_blocking(self):
        """Unit-level pin on the classify_record precedence order."""
        rec = self._executed("harness enoent cap", "HARNESS_ERROR",
                             stderr="No such file or directory")
        self.assertEqual(pdv.classify_record(rec), pdv.CAT_BLOCKING)

    # ── (3) blast radius: neither downgrade is disabled for probe verdicts ───

    def test_fail_with_the_same_enoent_stderr_is_still_fixture_absent(self):
        """The carve-out still applies where its rationale applies — to a probe."""
        rec = dict(
            self._executed("missing deliverable cap", "FAIL",
                           stderr=_reify_missing_fixture_stderr(_LEAF_FIXTURE)),
            command=["reify", "check", _LEAF_FIXTURE])
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertFalse(bv.blocks)
        self.assertEqual(bv.fixture_absent, ["missing deliverable cap"])

    def test_evidence_free_fail_is_still_malformed(self):
        """The evidence gate is intact for the verdicts it was written for."""
        rec = dict(self._prompt_fallback_record("vacuous cap"), verdict="FAIL")
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertFalse(bv.blocks)
        self.assertEqual(bv.malformed, ["vacuous cap"])

    def test_evidence_free_unprovable_is_still_malformed(self):
        """Same for UNPROVABLE — only HARNESS_ERROR is exempt."""
        rec = dict(self._prompt_fallback_record("vacuous cap"), verdict="UNPROVABLE")
        bv = pdv.synthesize_batch({"prover": [rec], "adversary": []})
        self.assertFalse(bv.blocks)
        self.assertEqual(bv.malformed, ["vacuous cap"])

    # ── (4) end to end through the CLI ───────────────────────────────────────

    def test_cli_exits_1_on_the_prompt_fallback_record_alone(self):
        """The exit code — what the Synthesize agent actually relays — is 1."""
        data = {"prover": [self._prompt_fallback_record()], "adversary": []}
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            json.dump(data, f)
            tmp = f.name
        buf_out, buf_err = io.StringIO(), io.StringIO()
        try:
            with unittest.mock.patch("sys.stdout", buf_out), \
                 unittest.mock.patch("sys.stderr", buf_err):
                rc = pdv.main(["synthesize", tmp])
        finally:
            os.unlink(tmp)
        self.assertEqual(rc, 1, "a dead harness must not exit 0")
        payload = json.loads(buf_out.getvalue())
        self.assertTrue(payload["blocks"])
        self.assertEqual(payload["blocking"], ["leaf label (delta)"])
        self.assertEqual(payload["malformed"], [])
        self.assertEqual(payload["executed"], 0)


# ---------------------------------------------------------------------------
# task #7257 amendment: the report format cannot drift between sections
# ---------------------------------------------------------------------------

class TestEvidenceBlockIsShared(unittest.TestCase):
    """All three report sections render evidence through ONE builder.

    They drifted within one task: the fixture-absent branch silently omitted
    `stdout`, so a probe that wrote to stdout before failing to find its fixture
    lost that evidence — contrary to PRD §6 decision 4's "exact command +
    stdout/stderr + exit code".  A reader comparing sections must be comparing
    like with like.
    """

    _STDOUT = "partial output before the open failed"

    def _rec(self, capability: str, verdict: str, stderr: str,
             command=None, exit_code=1) -> dict:
        return {
            "capability": capability,
            "probe_kind": "ir",
            "verdict": verdict,
            "command": ["reify", "eval", "f.ri"] if command is None else command,
            "exit_code": exit_code,
            "stdout": self._STDOUT,
            "stderr": stderr,
        }

    def test_fixture_absent_section_carries_stdout(self):
        """The drift that motivated the extraction."""
        bv = pdv.synthesize_batch({"prover": [self._rec(
            "fixture-absent cap", "FAIL",
            _reify_missing_fixture_stderr("f.ri"))], "adversary": []})
        self.assertEqual(bv.fixture_absent, ["fixture-absent cap"])
        self.assertIn(f"  stdout:    {self._STDOUT}", bv.report)

    def test_malformed_section_carries_stdout(self):
        bv = pdv.synthesize_batch({"prover": [self._rec(
            "vacuous cap", "FAIL", "", command=[], exit_code=None)],
            "adversary": []})
        self.assertEqual(bv.malformed, ["vacuous cap"])
        self.assertIn(f"  stdout:    {self._STDOUT}", bv.report)

    def test_blocking_section_carries_stdout(self):
        bv = pdv.synthesize_batch({"prover": [self._rec(
            "real fail cap", "FAIL", "type mismatch")], "adversary": []})
        self.assertEqual(bv.blocking, ["real fail cap"])
        self.assertIn(f"  stdout:    {self._STDOUT}", bv.report)

    def test_all_three_sections_share_one_line_format(self):
        """Same record, three categories — the evidence lines must be identical."""
        common = dict(command=["reify", "eval", "f.ri"], exit_code=1)
        block = pdv._evidence_block("FAIL", "cap", "prover", "reify eval f.ri",
                                    1, self._STDOUT, "stderr text")
        for stderr, category in (
            ("type mismatch: expected axis", "blocking"),
            (_reify_missing_fixture_stderr("f.ri"), "fixture-absent"),
        ):
            with self.subTest(category=category):
                bv = pdv.synthesize_batch({"prover": [
                    self._rec("cap", "FAIL", stderr, **common)], "adversary": []})
                rendered = "\n".join(
                    ln for ln in bv.report.splitlines()
                    if ln.startswith(("[FAIL]", "  command:", "  exit_code:",
                                      "  stdout:", "  stderr:")))
                self.assertEqual(
                    rendered,
                    block.replace("stderr text", stderr),
                    f"the {category} section renders a different evidence block",
                )

    def test_empty_stdout_and_stderr_lines_are_omitted(self):
        """An evidence block never pads the report with contentless lines."""
        block = pdv._evidence_block("FAIL", "cap", "prover", "reify eval f.ri",
                                    1, "", "")
        self.assertNotIn("stdout:", block)
        self.assertNotIn("stderr:", block)
        self.assertEqual(block.splitlines()[0], "[FAIL] cap (role: prover)")



if __name__ == "__main__":
    unittest.main()
