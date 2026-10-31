#!/usr/bin/env python3
"""
test_prd_decompose_verify_bind.py — importability, premise_to_probe(), bind_premises()
and value-assertion-kind tests for scripts/prd-decompose-verify.py.
"""

import json
import os
import sys
import unittest
from typing import Any

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from _pdv_test_support import load_pcc, load_pdv

pdv = load_pdv()
pcc = load_pcc()


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

    # ── (4) a premise with no fixture → error, for every kind ─────────────────

    def test_missing_fixture_raises_for_every_kind(self):
        """A premise with no fixture is a gap, not a pass — whatever its kind.

        The refusal is α's load rule (R14 M1), reached through bind_premises'
        one validation call, so it names the probe index α reports.
        """
        valid_match = {
            "rejection": {"exit_code": 1},
            "resolves": {"exit_code": 0},
            "parses": {},
            "produces": {"stderr_contains": "E"},
            "ir": {"stderr_contains": "EvalError"},
            "value": {"stdout_value": {"pattern": r"X = ([0-9.]+)", "min": 0}},
        }
        for kind, match in valid_match.items():
            with self.subTest(assertion_kind=kind):
                p = pdv.Premise(text=f"missing fixture {kind}", assertion_kind=kind,
                                fixture="", match=match, capability="missing")
                with self.assertRaises(ValueError) as ctx:
                    pdv.bind_premises([p])
                message = str(ctx.exception)
                self.assertIn("fixture", message)
                self.assertIn("probe[0]", message)

    def test_rejection_empty_match_binds_exit_code_1(self):
        """An empty rejection match is filled, not refused: the binding row itself
        requires exit_code 1, so silent-accept (exit 0) still FAILs the probe."""
        p = pdv.Premise(text="empty match rejection", assertion_kind="rejection",
                        fixture="tests/prd-gate/fixtures/revolute_silent_accept.ri",
                        match={}, capability="empty-match")
        result = pdv.bind_premises([p])
        self.assertEqual(result["probes"][0]["expected"]["match"], {"exit_code": 1})

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
            "rejection": ("check", "present", {"exit_code": 1}),
            "parses": ("grammar", "present", {}),
            "resolves": ("check", "present", {"exit_code": 0}),
            "produces": ("ir", "present", {"stderr_contains": "EvalError"}),
            "ir": ("ir", "absent", {"stderr_contains": "EvalError"}),
        }
        for kind, (probe_kind, observation, match) in expected.items():
            with self.subTest(assertion_kind=kind):
                probe = pdv.premise_to_probe(
                    self._premise(match=match, assertion_kind=kind)
                )
                self.assertEqual(
                    (probe["probe_kind"], probe["expected"]["observation"]),
                    (probe_kind, observation),
                )


# ---------------------------------------------------------------------------
# #8644 (S22 δ, D4 / §5.3): each binding row carries its required match
# ---------------------------------------------------------------------------

class TestBindingRequiredMatch(unittest.TestCase):
    """premise_to_probe writes the match keys a binding row requires.

    A `rejection` premise always probes exit_code 1 and a `resolves` premise
    exit_code 0, whether or not the premise wrote it; a premise that writes a
    different value is mis-kinded and refused rather than silently honoured.
    The other kinds' matches pass through as written.
    """

    _FIXTURE = "tests/prd-gate/fixtures/revolute_silent_accept.ri"
    _VALUE_SPEC = {"pattern": r"X = ([0-9.]+)", "min": 0.01}

    def _premise(self, kind, match):
        return pdv.Premise(text="the premise under test", assertion_kind=kind,
                           fixture=self._FIXTURE, match=match, capability="cap")

    def _bound_match(self, kind, match):
        return pdv.premise_to_probe(self._premise(kind, match))["expected"]["match"]

    def test_fills_the_exit_code_its_kind_requires(self):
        satisfied = "All constraints satisfied."
        cases = (
            ("resolves", {}, {"exit_code": 0}),
            ("resolves", {"stdout_contains": satisfied},
             {"exit_code": 0, "stdout_contains": satisfied}),
            ("resolves", {"exit_code": 0}, {"exit_code": 0}),
            ("rejection", {}, {"exit_code": 1}),
            ("rejection", {"stderr_contains": "diag"},
             {"exit_code": 1, "stderr_contains": "diag"}),
        )
        for kind, given, want in cases:
            with self.subTest(assertion_kind=kind, match=given):
                self.assertEqual(self._bound_match(kind, given), want)

    def test_other_kinds_bind_the_premise_match_unchanged(self):
        cases = (
            ("parses", {}),
            ("produces", {"stderr_contains": "X"}),
            ("ir", {"stderr_contains": "EvalError"}),
            ("value", {"stdout_value": dict(self._VALUE_SPEC)}),
        )
        for kind, given in cases:
            with self.subTest(assertion_kind=kind):
                self.assertEqual(self._bound_match(kind, given), given)

    def test_refuses_an_exit_code_that_contradicts_the_kind(self):
        """Honouring `resolves` + exit_code 1 would turn resolves into rejection."""
        with self.assertRaises(ValueError) as ctx:
            pdv.premise_to_probe(self._premise("resolves", {"exit_code": 1}))
        message = str(ctx.exception)
        for needle in ("resolves", "exit_code"):
            self.assertIn(needle, message)
        self.assertRegex(message, r"\b0\b")
        self.assertRegex(message, r"\b1\b")

        with self.assertRaises(ValueError) as ctx:
            pdv.premise_to_probe(self._premise("rejection", {"exit_code": 42}))
        message = str(ctx.exception)
        for needle in ("rejection", "exit_code", "42"):
            self.assertIn(needle, message)
        self.assertRegex(message, r"\b1\b")

    def test_refuses_a_produces_premise_with_no_signature(self):
        """Its ir probe could never pass, so α's single validation site refuses it."""
        with self.assertRaises(ValueError) as ctx:
            pdv.bind_premises([self._premise("produces", {})])
        message = str(ctx.exception)
        for needle in ("probe[0]", "stderr_contains"):
            self.assertIn(needle, message)

    def test_a_bound_match_is_a_fresh_dict(self):
        """Mutating one emitted probe reaches neither the table nor the premise."""
        premise_match = {}
        first = pdv.premise_to_probe(self._premise("resolves", premise_match))
        first["expected"]["match"]["exit_code"] = 7
        second = pdv.premise_to_probe(self._premise("resolves", {}))
        self.assertEqual(second["expected"]["match"], {"exit_code": 0})
        self.assertEqual(premise_match, {})

    def test_a_non_object_premise_match_is_a_value_error(self):
        """A ValueError, not a TypeError, so the bind CLI reports it as rc 64."""
        for kind in ("rejection", "resolves", "parses"):
            for match in (None, ["exit_code", 0]):
                with self.subTest(assertion_kind=kind, match=match):
                    with self.assertRaises(ValueError):
                        pdv.premise_to_probe(self._premise(kind, match))


if __name__ == "__main__":
    unittest.main()
