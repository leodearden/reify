#!/usr/bin/env python3
"""
test_prd_decompose_verify_schema.py — the vocabulary scripts/prd-decompose-verify.py
publishes: its StrEnums, dataclasses, published_schemas() and the print-schema
subcommand, including the relay members it derives from α's result_record (PRD
docs/prds/v0_6/prd-probe-gate-typed-outcomes.md D5 as amended by R14 M4).
"""

import dataclasses
import json
import os
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from _pdv_test_support import PDV_PY, load_pcc, load_pdv
from _schema_test_support import assert_well_formed, schema_errors

pdv = load_pdv()
pcc = load_pcc()

_MEMBERS = {"premises", "results", "verdict", "aggregate_input", "aggregate"}

# One minimal, α-valid match per assertion kind: the kind's row supplies the rest.
_MINIMAL_MATCH = {
    "rejection": {},
    "parses": {},
    "resolves": {},
    "produces": {"stderr_contains": "EvalError"},
    "ir": {},
    "value": {"stdout_value": {"pattern": r"x = ([-+0-9.eE]+)", "min": 0}},
}


def _values(enum_class):
    return [member.value for member in enum_class]


def _field_names(dataclass_type):
    return {f.name for f in dataclasses.fields(dataclass_type)}


def _run_pdv(*argv):
    return subprocess.run(
        [sys.executable, PDV_PY, *argv], capture_output=True, text=True, timeout=60
    )


def _alpha_record(verdict="PASS", **overrides):
    """A record shaped exactly as α's --json emits it."""
    record = {
        "capability": "cap",
        "probe_kind": "check",
        "verdict": verdict,
        "command": ["reify", "check", "/abs/leaf.ri"],
        "exit_code": 1,
        "stdout": "",
        "stderr": "",
        "fixture": "tests/prd-gate/fixtures/leaf.ri",
        "fixture_present": True,
        "harness_failure": None,
    }
    record.update(overrides)
    return record


class TestPrintSchemaCli(unittest.TestCase):
    def test_print_schema_prints_every_member(self):
        proc = _run_pdv("print-schema")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        printed = json.loads(proc.stdout)
        self.assertEqual(set(printed), _MEMBERS)
        self.assertEqual(printed, pdv.published_schemas())
        for name, schema in printed.items():
            with self.subTest(member=name):
                assert_well_formed(self, schema)


class TestPremisesSchema(unittest.TestCase):
    def setUp(self):
        self.premise = pdv.published_schemas()["premises"]["properties"]["premises"]["items"]

    def test_assertion_kind_enum_is_the_assertion_kind_vocabulary(self):
        self.assertEqual(
            self.premise["properties"]["assertion_kind"]["enum"], _values(pdv.AssertionKind)
        )

    def test_required_fields(self):
        self.assertEqual(self.premise["required"], ["text", "assertion_kind", "fixture", "match"])

    def test_every_assertion_kind_binds(self):
        self.assertEqual(set(_MINIMAL_MATCH), set(_values(pdv.AssertionKind)))
        for kind in pdv.AssertionKind:
            with self.subTest(assertion_kind=kind.value):
                premise = pdv.Premise(
                    text=f"a {kind.value} premise",
                    assertion_kind=kind,
                    fixture="tests/prd-gate/fixtures/leaf.ri",
                    match=dict(_MINIMAL_MATCH[kind.value]),
                )
                [probe] = pdv.bind_premises([premise])["probes"]
                self.assertIn(probe["probe_kind"], set(pcc.ProbeKind))


class TestResultsSchema(unittest.TestCase):
    """The relay record is α's result_record widened by pdv, and only there."""

    def setUp(self):
        results = pdv.published_schemas()["results"]
        self.results = results
        self.relay = results["properties"]["prover"]["items"]

    def test_prover_and_adversary_relay_the_same_record(self):
        properties = self.results["properties"]
        self.assertEqual(properties["prover"]["items"], properties["adversary"]["items"])

    def test_relay_properties_are_alphas_except_fixture_present(self):
        alpha = pcc.published_schemas()["result_record"]["properties"]
        self.assertEqual(set(self.relay["properties"]), set(alpha))
        for name, schema in alpha.items():
            if name == "fixture_present":
                continue
            with self.subTest(property=name):
                self.assertEqual(self.relay["properties"][name], schema)

    def test_relay_required_fields(self):
        self.assertEqual(
            self.relay["required"],
            ["capability", "verdict", "command", "exit_code", "fixture_present"],
        )

    def test_fixture_present_null_is_admitted_only_beside_harness_error(self):
        without_field = _alpha_record()
        del without_field["fixture_present"]
        cases = (
            ("α-shaped PASS, fixture_present true", _alpha_record(), False),
            ("PASS, fixture_present null", _alpha_record(fixture_present=None), True),
            ("PASS without fixture_present", without_field, True),
            ("FAIL, fixture_present null", _alpha_record("FAIL", fixture_present=None), True),
            ("the Prover's fallback record", {
                "capability": "leaf", "verdict": "HARNESS_ERROR",
                "command": [], "exit_code": -1, "fixture_present": None,
            }, False),
        )
        for label, record, rejected in cases:
            with self.subTest(case=label):
                self.assertEqual(bool(schema_errors(record, self.relay)), rejected)


class TestVerdictSchema(unittest.TestCase):
    def setUp(self):
        self.verdict = pdv.published_schemas()["verdict"]

    def test_required_fields_are_stated(self):
        self.assertEqual(
            self.verdict["required"], ["blocks", "blocking", "report", "executed", "total"]
        )

    def test_properties_are_the_batch_verdict_fields(self):
        self.assertEqual(set(self.verdict["properties"]), _field_names(pdv.BatchVerdict))

    def test_synthesize_output_conforms(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = os.path.join(tmpdir, "results.json")
            with open(path, "w") as fh:
                json.dump({"prover": [_alpha_record()], "adversary": []}, fh)
            proc = _run_pdv("synthesize", path)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        printed = json.loads(proc.stdout)
        self.assertEqual(schema_errors(printed, self.verdict), [])
        self.assertEqual(set(printed), _field_names(pdv.BatchVerdict))


class TestAggregateSchemas(unittest.TestCase):
    def setUp(self):
        schemas = pdv.published_schemas()
        self.aggregate_input = schemas["aggregate_input"]
        self.aggregate = schemas["aggregate"]
        self.leaf_result = self.aggregate_input["properties"]["leaf_results"]["items"]
        self.row = self.aggregate["properties"]["leaf_dispositions"]["items"]

    def test_batch_disposition_enum(self):
        self.assertEqual(
            self.aggregate["properties"]["disposition"]["enum"], _values(pdv.BatchDisposition)
        )

    def test_leaf_disposition_enum(self):
        self.assertEqual(self.row["properties"]["disposition"]["enum"], _values(pdv.LeafDisposition))

    def test_leaf_result_statuses(self):
        statuses = [
            option["properties"]["status"]["const"]
            for option in self.leaf_result["oneOf"]
            if "properties" in option
        ]
        self.assertEqual(sorted(statuses), sorted(_values(pdv.LeafResultStatus)))

    def test_aggregate_properties_are_the_workflow_verdict_fields(self):
        self.assertEqual(set(self.aggregate["properties"]), _field_names(pdv.WorkflowVerdict))
        self.assertEqual(set(self.row["properties"]), _field_names(pdv.LeafDispositionRow))

    def test_a_serialised_workflow_verdict_conforms(self):
        row = pdv.LeafDispositionRow(
            label="leaf", disposition=pdv.LeafDisposition.VERIFIED, blocks=False,
            blocking=(), executed=1, total=1, malformed=(), fixture_absent=(),
        )
        verdict = pdv.WorkflowVerdict(
            disposition=pdv.BatchDisposition.PASS, blocks=False, blocking=(),
            summary="γ PASS — all 1 leaf(ves) verified (1 probed)",
            leaf_dispositions=(row,), leaves_total=1, leaves_probed=1,
            leaves_unenumerated=0, leaves_not_verified=0, unenumerated_leaves=(),
            not_verified_leaves=(), malformed_records=0, fixture_absent_records=0,
        )
        serialised = json.loads(json.dumps(dataclasses.asdict(verdict)))
        self.assertEqual(schema_errors(serialised, self.aggregate), [])

    def test_aggregate_input_admits_each_leaf_result_shape(self):
        synthesized_verdict = {
            "blocks": False, "blocking": [], "executed": 1, "total": 1,
            "malformed": [], "fixture_absent": [],
        }
        leaf_results = [
            None,
            {"label": "a", "status": "unenumerated"},
            {"label": "b", "status": "synthesize_failed", "reason": "agent returned null"},
            {"label": "c", "status": "synthesized", "verdict": synthesized_verdict},
        ]
        batch = {"leaves": [{"label": label} for label in "abcd"], "leaf_results": leaf_results}
        self.assertEqual(schema_errors(batch, self.aggregate_input), [])

    def test_aggregate_input_refuses_an_unknown_status(self):
        batch = {"leaves": [{"label": "a"}],
                 "leaf_results": [{"label": "a", "status": "skipped"}]}
        self.assertTrue(schema_errors(batch, self.aggregate_input))


class TestRecordCategory(unittest.TestCase):
    def test_categories(self):
        self.assertEqual(
            set(_values(pdv.RecordCategory)),
            {"NON_BLOCKING", "MALFORMED", "FIXTURE_ABSENT", "BLOCKING"},
        )

    def test_classify_record_returns_a_record_category(self):
        for record in (_alpha_record(), _alpha_record("FAIL"),
                       _alpha_record("FAIL", fixture_present=False),
                       _alpha_record("FAIL", command=[])):
            with self.subTest(record=record):
                self.assertIsInstance(pdv.classify_record(record), pdv.RecordCategory)


if __name__ == "__main__":
    unittest.main()
