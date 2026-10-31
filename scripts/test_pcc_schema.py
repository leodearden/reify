#!/usr/bin/env python3
"""
test_pcc_schema.py — the vocabulary scripts/prd-capability-check.py publishes:
its StrEnums, published_schemas() and the --print-schema mode, held against the
committed probe sets and against load_probe_set().  Shared loader:
scripts/_pcc_test_support.py; schema checker: scripts/_schema_test_support.py;
runner: tests/infra/test_prd_capability_check.sh.
"""

import copy
import glob
import io
import json
import os
import subprocess
import sys
import unittest
import unittest.mock
from enum import StrEnum

from _pcc_test_support import _HARNESS_PATH, _REPO_ROOT, pcc
from _schema_test_support import assert_well_formed, schema_errors

_COMMITTED_PROBE_SETS = sorted(
    glob.glob(os.path.join(_REPO_ROOT, "tests", "prd-gate", "*-probe-set.json"))
)

_A_VALID_PROBE = {
    "capability": "arg-vs-param nominal rejection",
    "probe_kind": "check",
    "fixture": "tests/prd-gate/fixtures/revolute_silent_accept.ri",
    "expected": {"observation": "present", "match": {"exit_code": 1}},
}


def _values(enum_class):
    return [member.value for member in enum_class]


def _run_cli(*args):
    return subprocess.run(
        [sys.executable, _HARNESS_PATH, *args],
        capture_output=True, text=True, timeout=60,
    )


class TestVocabularyIsStrEnum(unittest.TestCase):
    def test_published_vocabularies_are_str_enums(self):
        for enum_class in (pcc.Verdict, pcc.ProbeKind, pcc.Observation, pcc.HarnessFailureKind):
            with self.subTest(vocabulary=enum_class.__name__):
                self.assertTrue(issubclass(enum_class, StrEnum))


class TestPrintSchemaCli(unittest.TestCase):
    def test_print_schema_prints_the_published_schemas(self):
        proc = _run_cli("--print-schema")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        printed = json.loads(proc.stdout)
        self.assertEqual(set(printed), {"probe_set", "result_record"})
        self.assertEqual(printed, pcc.published_schemas())
        for name, schema in printed.items():
            with self.subTest(member=name):
                assert_well_formed(self, schema)

    def test_print_schema_ignores_json_and_a_probe_set_argument(self):
        out = io.StringIO()
        with unittest.mock.patch("sys.stdout", out):
            rc = pcc.main(["--print-schema", "--json", "/nonexistent/probe-set.json"])
        self.assertEqual(rc, 0)
        self.assertEqual(json.loads(out.getvalue()), pcc.published_schemas())

    def test_print_schema_and_substrate_status_are_mutually_exclusive(self):
        proc = _run_cli("--print-schema", "--grammar-substrate-status")
        self.assertEqual(proc.returncode, 64, proc.stdout + proc.stderr)
        self.assertEqual(proc.stdout, "")
        self.assertIn("not allowed with", proc.stderr)


class TestResultRecordSchema(unittest.TestCase):
    """α publishes what α emits: every enum is α's own, and fixture_present is
    a required boolean (PRD §5.1, R14 M4)."""

    def setUp(self):
        self.record = pcc.published_schemas()["result_record"]
        self.properties = self.record["properties"]

    def test_verdict_enum_is_the_verdict_vocabulary(self):
        self.assertEqual(self.properties["verdict"]["enum"], _values(pcc.Verdict))

    def test_probe_kind_enum_is_the_probe_kind_vocabulary(self):
        self.assertEqual(self.properties["probe_kind"]["enum"], _values(pcc.ProbeKind))

    def test_harness_failure_kind_enum_is_the_harness_failure_vocabulary(self):
        kind = self.properties["harness_failure"]["properties"]["kind"]
        self.assertEqual(kind["enum"], _values(pcc.HarnessFailureKind))

    def test_fixture_present_is_a_required_boolean(self):
        self.assertEqual(self.properties["fixture_present"], {"type": "boolean"})
        self.assertIn("fixture_present", self.record["required"])

    def test_each_call_returns_fresh_schemas(self):
        first = pcc.published_schemas()
        first["result_record"]["properties"].clear()
        self.assertTrue(pcc.published_schemas()["result_record"]["properties"])


class TestProbeSetSchema(unittest.TestCase):
    """The probe_set schema and load_probe_set() give one answer."""

    def setUp(self):
        self.schema = pcc.published_schemas()["probe_set"]

    def _probe_set_with(self, **overrides):
        probe = copy.deepcopy(_A_VALID_PROBE)
        for key, value in overrides.items():
            if key == "observation":
                probe["expected"]["observation"] = value
            else:
                probe[key] = value
        return {"probes": [probe]}

    def test_every_committed_probe_set_conforms(self):
        self.assertTrue(_COMMITTED_PROBE_SETS)
        for path in _COMMITTED_PROBE_SETS:
            with self.subTest(probe_set=os.path.basename(path)):
                with open(path) as fh:
                    self.assertEqual(schema_errors(json.load(fh), self.schema), [])

    def test_the_valid_probe_conforms_and_loads(self):
        probe_set = self._probe_set_with()
        self.assertEqual(schema_errors(probe_set, self.schema), [])
        pcc.load_probe_set(json.dumps(probe_set))

    def test_schema_and_loader_both_refuse(self):
        for label, probe_set in (
            ("unknown probe_kind", self._probe_set_with(probe_kind="parse")),
            ("indeterminate is not expectable", self._probe_set_with(observation="indeterminate")),
        ):
            with self.subTest(case=label):
                self.assertTrue(schema_errors(probe_set, self.schema))
                with self.assertRaises(ValueError):
                    pcc.load_probe_set(json.dumps(probe_set))

    def test_expected_observation_enum_is_the_two_expectable_observations(self):
        probe = self.schema["properties"]["probes"]["items"]
        observation = probe["properties"]["expected"]["properties"]["observation"]
        self.assertEqual(
            observation["enum"], [pcc.Observation.PRESENT.value, pcc.Observation.ABSENT.value]
        )

    def test_probe_kind_enum_is_the_probe_kind_vocabulary(self):
        probe = self.schema["properties"]["probes"]["items"]
        self.assertEqual(probe["properties"]["probe_kind"]["enum"], _values(pcc.ProbeKind))


if __name__ == "__main__":
    unittest.main()
