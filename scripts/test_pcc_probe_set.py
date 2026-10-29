#!/usr/bin/env python3
"""
test_pcc_probe_set.py — probe-set loading in scripts/prd-capability-check.py:
the scaffold, the JSON round-trip, the "value" kind's load-time schema, the
load-time non-vacuity rules every kind shares, and the pure stdout value
predicate.  Shared loader and helpers:
scripts/_pcc_test_support.py; runner: tests/infra/test_prd_capability_check.sh.
"""

import copy
import glob
import json
import os
import unittest

from _pcc_test_support import (
    pcc,
    _EXAMPLE_PROBE_SET,
    _REPO_ROOT,
    _VALUE_CELLS_FIXTURE,
    _VALUE_CELLS_STDOUT,
)


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


_CORPUS_PROBE_SET = os.path.join(_REPO_ROOT, "tests", "prd-gate", "corpus-probe-set.json")


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

    def test_committed_probe_set_has_one_record_per_kind(self):
        """The committed example-probe-set.json holds exactly one probe per kind."""
        with open(_EXAMPLE_PROBE_SET) as f:
            text = f.read()
        probes = pcc.load_probe_set(text)
        self.assertEqual(len(probes), len(pcc._VALID_PROBE_KINDS))

    def test_committed_probe_set_has_one_of_each_kind(self):
        """The worked example demonstrates every kind — derived, so it cannot drift.

        Spelling the expected set here is what let it fall a kind behind when a
        fourth was added; reading it off _VALID_PROBE_KINDS makes the next
        addition red this test until the example covers it too.
        """
        with open(_EXAMPLE_PROBE_SET) as f:
            text = f.read()
        probes = pcc.load_probe_set(text)
        kinds = {p.probe_kind for p in probes}
        self.assertEqual(kinds, set(pcc._VALID_PROBE_KINDS))


# ---------------------------------------------------------------------------
# #6876 step-01 (RED): the "value" probe kind — load-time schema and the
# non-vacuity contract.
# ---------------------------------------------------------------------------

class TestValueProbeSchema(unittest.TestCase):
    """load_probe_set's handling of the fourth probe kind, "value".

    A `value` probe asserts something about the VALUE `reify eval` printed on a
    clean (exit 0) run.  Its whole reason to exist is that `ir`/absent is
    exit-code-only on that branch, so the load-time job here is to refuse any
    `value` probe that would be armed-but-vacuous — no predicate, no capture
    group, or no numeric constraint — rather than let it report a confident
    PASS having asserted nothing.
    """

    def _make_probe_set_text(self, probe_dicts):
        return json.dumps({"probes": probe_dicts})

    def _value_probe(self, stdout_value=None, **overrides):
        """A minimal well-formed `value` probe dict, with targeted overrides."""
        match = {}
        if stdout_value is not None:
            match["stdout_value"] = stdout_value
        probe = {
            "capability": "damping ratio is a finite in-range number",
            "probe_kind": "value",
            "fixture": _VALUE_CELLS_FIXTURE,
            "expected": {"observation": "present", "match": match},
        }
        probe.update(overrides)
        return probe

    def _load_one(self, probe_dict):
        return pcc.load_probe_set(self._make_probe_set_text([probe_dict]))[0]

    def _assert_rejected(self, probe_dict, *needles):
        with self.assertRaises(ValueError) as ctx:
            pcc.load_probe_set(self._make_probe_set_text([probe_dict]))
        message = str(ctx.exception)
        for needle in needles:
            self.assertIn(needle, message)
        return message

    # ── ACCEPT ────────────────────────────────────────────────────────────────

    def test_value_kind_is_valid(self):
        """"value" is a member of the valid probe-kind vocabulary."""
        self.assertIn("value", pcc._VALID_PROBE_KINDS)

    def test_accepts_bounded_predicate(self):
        spec = {"pattern": r"X = ([0-9.]+)", "min": 0.01, "max": 0.03}
        probe = self._load_one(self._value_probe(spec))
        self.assertEqual(probe.probe_kind, "value")
        self.assertEqual(probe.expected["match"]["stdout_value"], spec)

    def test_accepts_explicit_group_index(self):
        spec = {"pattern": r"(\w+) = ([0-9.]+)", "group": 2, "min": 0.01}
        probe = self._load_one(self._value_probe(spec))
        loaded = probe.expected["match"]["stdout_value"]
        self.assertEqual(loaded["group"], 2)
        self.assertIsInstance(loaded["group"], int)

    def test_accepts_named_group(self):
        spec = {"pattern": r"X = (?P<val>[0-9.]+)", "group": "val", "finite": True}
        probe = self._load_one(self._value_probe(spec))
        self.assertEqual(probe.expected["match"]["stdout_value"]["group"], "val")

    def test_accepts_finite_alone(self):
        """`finite: true` with no bounds is a complete constraint on its own."""
        spec = {"pattern": r"X = (\S+)", "finite": True}
        probe = self._load_one(self._value_probe(spec))
        self.assertEqual(probe.expected["match"]["stdout_value"], spec)

    def test_accepts_min_alone_and_max_alone(self):
        for spec in (
            {"pattern": r"X = ([0-9.]+)", "min": 0.01},
            {"pattern": r"X = ([0-9.]+)", "max": 0.03},
        ):
            with self.subTest(spec=spec):
                probe = self._load_one(self._value_probe(spec))
                self.assertEqual(probe.expected["match"]["stdout_value"], spec)

    def test_stdout_value_preserved_verbatim(self):
        """Nested keys and their python types survive the load unchanged."""
        spec = {
            "pattern": r"ValueCleanEvalCells\.damping_ratio = ([-+0-9.eE]+)",
            "group": 1,
            "min": 0.01,
            "max": 0.03,
            "finite": True,
        }
        loaded = self._load_one(self._value_probe(spec)).expected["match"]["stdout_value"]
        self.assertEqual(loaded, spec)
        self.assertIsInstance(loaded["group"], int)
        self.assertIsInstance(loaded["min"], float)
        self.assertIsInstance(loaded["finite"], bool)

    def test_round_trip_preserves_stdout_value(self):
        spec = {"pattern": r"X = ([0-9.]+)", "group": 1, "min": 0.01, "max": 0.03}
        probes = pcc.load_probe_set(self._make_probe_set_text([self._value_probe(spec)]))
        reloaded = pcc.load_probe_set(pcc.dump_probe_set(probes))
        self.assertEqual(len(reloaded), 1)
        self.assertEqual(reloaded[0].probe_kind, "value")
        self.assertEqual(reloaded[0].expected["match"]["stdout_value"], spec)
        self.assertEqual(reloaded[0].expected, probes[0].expected)

    # ── REJECT: the predicate is missing entirely ─────────────────────────────

    def test_rejects_value_probe_with_no_match(self):
        probe = self._value_probe()
        del probe["expected"]["match"]
        self._assert_rejected(probe, "probe[0]", "stdout_value")

    def test_rejects_value_probe_with_empty_match(self):
        self._assert_rejected(self._value_probe(), "probe[0]", "stdout_value")

    def test_rejects_non_dict_stdout_value(self):
        for bogus in ("a string", 42, ["a", "list"], None):
            with self.subTest(bogus=bogus):
                probe = self._value_probe()
                probe["expected"]["match"]["stdout_value"] = bogus
                self._assert_rejected(probe, "probe[0]", "stdout_value")

    # ── REJECT: the locator half is unusable ──────────────────────────────────

    def test_rejects_missing_pattern(self):
        self._assert_rejected(self._value_probe({"min": 0.01}), "probe[0]", "pattern")

    def test_rejects_empty_pattern(self):
        self._assert_rejected(
            self._value_probe({"pattern": "", "min": 0.01}), "probe[0]", "pattern"
        )

    def test_rejects_non_string_pattern(self):
        self._assert_rejected(
            self._value_probe({"pattern": 7, "min": 0.01}), "probe[0]", "pattern"
        )

    def test_rejects_uncompilable_pattern(self):
        self._assert_rejected(
            self._value_probe({"pattern": "([0-9", "min": 0.01}), "probe[0]", "pattern"
        )

    def test_rejects_pattern_with_zero_capture_groups(self):
        """A pattern that only locates cannot yield a value to test."""
        self._assert_rejected(
            self._value_probe({"pattern": "damping_ratio = 0.018", "min": 0.01}),
            "probe[0]",
            "capture group",
        )

    def test_rejects_unknown_named_group(self):
        self._assert_rejected(
            self._value_probe(
                {"pattern": r"X = (?P<val>[0-9.]+)", "group": "missing", "min": 0.01}
            ),
            "probe[0]",
            "group",
        )

    def test_rejects_out_of_range_group_index(self):
        self._assert_rejected(
            self._value_probe({"pattern": r"X = ([0-9.]+)", "group": 3, "min": 0.01}),
            "probe[0]",
            "group",
        )

    def test_rejects_bad_group_type(self):
        for bogus in (1.5, [1], None):
            with self.subTest(bogus=bogus):
                self._assert_rejected(
                    self._value_probe(
                        {"pattern": r"X = ([0-9.]+)", "group": bogus, "min": 0.01}
                    ),
                    "probe[0]",
                    "group",
                )

    # ── REJECT: no numeric constraint (the core non-vacuity rule) ─────────────

    def test_rejects_locate_only_predicate(self):
        """Pattern but no min/max/finite: the probe asserts nothing about the value."""
        message = self._assert_rejected(
            self._value_probe({"pattern": r"X = ([0-9.]+)"}), "probe[0]"
        )
        for constraint in ("min", "max", "finite"):
            self.assertIn(constraint, message)

    def test_rejects_group_only_predicate(self):
        self._assert_rejected(
            self._value_probe({"pattern": r"X = ([0-9.]+)", "group": 1}), "probe[0]"
        )

    def test_rejects_finite_false(self):
        """`finite: false` reads as an opt-out the predicate does not offer.

        Finiteness is enforced unconditionally, so a probe written this way
        behaves exactly as if `true` had been given and its evidence line says
        "satisfies finite" — the author's spec and the probe's actual assertion
        disagree, which is this kind's own defect one level down.  Satisfying
        the non-vacuity rule by KEY PRESENCE alone would let it load.
        """
        message = self._assert_rejected(
            self._value_probe({"pattern": r"X = (\S+)", "finite": False}),
            "probe[0]",
            "finite",
        )
        self.assertIn("structural", message)

    def test_rejects_finite_false_alongside_a_real_bound(self):
        """Not just the lone-key case: the contradiction is the key itself."""
        self._assert_rejected(
            self._value_probe(
                {"pattern": r"X = (\S+)", "min": 0.01, "finite": False}
            ),
            "probe[0]",
            "finite",
        )

    # ── REJECT: constraint types and ordering ─────────────────────────────────

    def test_rejects_non_numeric_min(self):
        self._assert_rejected(
            self._value_probe({"pattern": r"X = ([0-9.]+)", "min": "0.01"}),
            "probe[0]",
            "min",
        )

    def test_rejects_non_numeric_max(self):
        self._assert_rejected(
            self._value_probe({"pattern": r"X = ([0-9.]+)", "max": [3]}),
            "probe[0]",
            "max",
        )

    def test_rejects_non_bool_finite(self):
        for bogus in ("true", 1, 0):
            with self.subTest(bogus=bogus):
                self._assert_rejected(
                    self._value_probe({"pattern": r"X = ([0-9.]+)", "finite": bogus}),
                    "probe[0]",
                    "finite",
                )

    def test_rejects_min_greater_than_max(self):
        self._assert_rejected(
            self._value_probe({"pattern": r"X = ([0-9.]+)", "min": 0.03, "max": 0.01}),
            "probe[0]",
            "min",
            "max",
        )

    def test_accepts_min_equal_to_max(self):
        """An exact-equality assertion is a legitimate, non-empty interval."""
        spec = {"pattern": r"X = ([0-9.]+)", "min": 0.018, "max": 0.018}
        probe = self._load_one(self._value_probe(spec))
        self.assertEqual(probe.expected["match"]["stdout_value"], spec)

    # ── REJECT: `match` keys that belong to the other kinds ───────────────────

    def test_rejects_exit_code_on_value_probe(self):
        """exit 0 is structural to the kind — a second spelling of it would drift."""
        probe = self._value_probe({"pattern": r"X = ([0-9.]+)", "min": 0.01})
        probe["expected"]["match"]["exit_code"] = 0
        self._assert_rejected(probe, "probe[0]", "exit_code")

    def test_rejects_stderr_contains_on_value_probe(self):
        probe = self._value_probe({"pattern": r"X = ([0-9.]+)", "min": 0.01})
        probe["expected"]["match"]["stderr_contains"] = "EvalError"
        self._assert_rejected(probe, "probe[0]", "stderr_contains")

    def test_rejects_stdout_contains_on_value_probe(self):
        """A stdout assertion on a value probe goes through `pattern`."""
        probe = self._value_probe({"pattern": r"X = ([0-9.]+)", "min": 0.01})
        probe["expected"]["match"]["stdout_contains"] = "damping_ratio"
        self._assert_rejected(probe, "probe[0]", "stdout_contains")

    # ── REJECT: stdout_value on a kind that would silently ignore it ──────────

    def test_rejects_stdout_value_on_other_kinds(self):
        """Armed-but-vacuous is the exact shape this kind exists to close."""
        for kind in ("grammar", "check", "ir"):
            with self.subTest(kind=kind):
                probe = self._value_probe({"pattern": r"X = ([0-9.]+)", "min": 0.01})
                probe["probe_kind"] = kind
                self._assert_rejected(probe, "probe[0]", "stdout_value", kind)

    # ── REGRESSION: the three existing kinds are untouched ────────────────────

    def test_existing_three_kinds_still_load(self):
        text = json.dumps({"probes": TestProbeSetRoundTrip.PROBE_DICTS})
        probes = pcc.load_probe_set(text)
        self.assertEqual([p.probe_kind for p in probes], ["grammar", "check", "ir"])
        for probe in probes:
            self.assertNotIn("stdout_value", probe.expected["match"])

    def test_bogus_kind_still_rejected_and_lists_four_kinds(self):
        probe = dict(TestProbeSetRoundTrip.PROBE_DICTS[0], probe_kind="nonsense")
        message = self._assert_rejected(probe, "nonsense")
        for kind in ("grammar", "check", "ir", "value"):
            self.assertIn(kind, message)

    def test_corpus_probe_set_carries_no_stdout_value(self):
        """#4609's delta corpus is unchanged by this task."""
        with open(_CORPUS_PROBE_SET) as fh:
            probes = pcc.load_probe_set(fh.read())
        self.assertEqual(len(probes), 3)
        for probe in probes:
            self.assertNotEqual(probe.probe_kind, "value")
            self.assertNotIn("stdout_value", probe.expected.get("match", {}))


# ---------------------------------------------------------------------------
# #8644 (S22 δ): load-time non-vacuity for every kind — a target to probe, only
# the match keys the kind consults, and a check probe that constrains its run.
# ---------------------------------------------------------------------------

_PRD_GATE_DIR = os.path.join(_REPO_ROOT, "tests", "prd-gate")


def _committed_prd_gate_json():
    """(basename, decoded object) for every committed tests/prd-gate/*.json."""
    documents = []
    for path in sorted(glob.glob(os.path.join(_PRD_GATE_DIR, "*.json"))):
        with open(path) as fh:
            documents.append((os.path.basename(path), json.load(fh)))
    return documents


class TestProbeSetNonVacuity(unittest.TestCase):
    """load_probe_set refuses a probe that would run yet could prove nothing.

    Each refusal case is the SECOND probe of a set whose first probe is valid,
    so a message naming probe[1] is what shows the index is the offender's.
    """

    _VALID_MATCH_BY_KIND = {
        "grammar": {},
        "check": {"exit_code": 1},
        "ir": {"stderr_contains": "E"},
        "value": {"stdout_value": {"pattern": r"X = ([0-9.]+)", "min": 0}},
    }

    def _probe(self, kind, match, fixture="tests/prd-gate/fixtures/arrow_type.ri"):
        return {
            "capability": f"a {kind} probe",
            "probe_kind": kind,
            "fixture": fixture,
            "expected": {"observation": "present", "match": copy.deepcopy(match)},
        }

    def _load(self, *probes):
        return pcc.load_probe_set(json.dumps({"probes": list(probes)}))

    def _assert_second_probe_rejected(self, bad_probe, *needles):
        valid_first = self._probe("check", {"exit_code": 1})
        with self.assertRaises(ValueError) as ctx:
            self._load(valid_first, bad_probe)
        message = str(ctx.exception)
        for needle in ("probe[1]",) + needles:
            self.assertIn(needle, message)
        return message

    # ── fixture: every kind needs a target (R14 M1) ───────────────────────────

    def test_refuses_a_probe_with_no_fixture_for_every_kind(self):
        for kind, match in self._VALID_MATCH_BY_KIND.items():
            for fixture in ("", "   ", None, 5):
                with self.subTest(kind=kind, fixture=fixture):
                    self._assert_second_probe_rejected(
                        self._probe(kind, match, fixture=fixture), "fixture"
                    )

    # ── check: at least one key that constrains the run (D4 / M2) ─────────────

    def test_refuses_a_check_probe_with_no_constraining_key(self):
        for match in (
            {},
            {"stderr_contains": ""},
            {"stdout_contains": ""},
            {"exit_code": True},
        ):
            with self.subTest(match=match):
                self._assert_second_probe_rejected(self._probe("check", match))

    def test_accepts_a_check_probe_with_one_constraining_key(self):
        for match in (
            {"exit_code": 0},
            {"exit_code": 1},
            {"stderr_contains": "x"},
            {"stdout_contains": "x"},
        ):
            with self.subTest(match=match):
                probes = self._load(self._probe("check", match))
                self.assertEqual(len(probes), 1)
                self.assertEqual(probes[0].expected["match"], match)

    # ── every kind: no match key its observation ignores ──────────────────────

    def test_refuses_a_match_key_the_kind_does_not_consult(self):
        value_spec = self._VALID_MATCH_BY_KIND["value"]["stdout_value"]
        cases = (
            ("check", {"exit_code": 0, "stderr_pattern": "x"}, "stderr_pattern"),
            ("check", {"exit_code": 1, "stderr_not_contains": "x"}, "stderr_not_contains"),
            ("grammar", {"exit_code": 0}, "exit_code"),
            ("ir", {"exit_code": 1, "stderr_contains": "E"}, "exit_code"),
            ("value", {"stdout_value": value_spec, "foo": 1}, "foo"),
        )
        for kind, match, key in cases:
            with self.subTest(kind=kind, key=key):
                message = self._assert_second_probe_rejected(
                    self._probe(kind, match), key
                )
                self.assertRegex(message, rf"\b{kind}\b")

    # ── the committed sets stay loadable ──────────────────────────────────────

    def test_every_committed_probe_set_loads(self):
        probe_sets = [
            (name, doc) for name, doc in _committed_prd_gate_json()
            if isinstance(doc, dict) and "probes" in doc
        ]
        self.assertTrue(probe_sets, f"no probe set found under {_PRD_GATE_DIR}")
        for name, doc in probe_sets:
            with self.subTest(probe_set=name):
                self.assertTrue(pcc.load_probe_set(json.dumps(doc)))

    def test_every_committed_prd_gate_json_is_a_probe_set_or_premises_file(self):
        """A file that is neither escapes both loaders, and so every check above."""
        for name, doc in _committed_prd_gate_json():
            with self.subTest(file=name):
                self.assertIsInstance(doc, dict)
                self.assertTrue(
                    "probes" in doc or "premises" in doc,
                    f"{name} has neither a 'probes' nor a 'premises' key",
                )


# ---------------------------------------------------------------------------
# #6876 step-05 (RED): stdout_value_satisfied() — the pure value predicate
# ---------------------------------------------------------------------------

class TestStdoutValuePredicate(unittest.TestCase):
    """The stdout half of a value probe, in isolation — no subprocesses.

    observe_stdout_value(run, spec) locates a capture in run.stdout, parses it
    as a float, and applies the spec's bounds.  It reads run.stdout and nothing
    else: the exit code is the OTHER half of the observation, decided by
    observe(), and mixing the two here would put the answer in two places.

    These tests read `.satisfied` off the reading rather than a boolean wrapper,
    so the predicate under test is the same call observe() makes.
    """


    def _run(self, stdout, exit_code=0, stderr=""):
        return pcc.ProbeRun(exit_code=exit_code, stdout=stdout, stderr=stderr)

    def _check(self, stdout, spec):
        return pcc.observe_stdout_value(self._run(stdout), spec).satisfied

    # ── locating the capture ──────────────────────────────────────────────────

    def test_match_in_range_is_true(self):
        self.assertTrue(
            self._check("X = 0.018\n", {"pattern": r"X = ([0-9.]+)", "min": 0.01})
        )

    def test_pattern_absent_is_false(self):
        self.assertFalse(
            self._check("Y = 0.018\n", {"pattern": r"X = ([0-9.]+)", "min": 0.01})
        )

    def test_empty_stdout_is_false(self):
        self.assertFalse(
            self._check("", {"pattern": r"X = ([0-9.]+)", "min": 0.01})
        )

    def test_selects_the_right_line_of_many(self):
        spec = {
            "pattern": r"degenerate_ratio = ([-+0-9.eE]+)",
            "min": -0.5,
            "max": 0.5,
        }
        self.assertTrue(self._check(_VALUE_CELLS_STDOUT, spec))

    def test_greedy_capture_does_not_span_lines(self):
        """No re.DOTALL: `.+` must stop at the newline.

        With DOTALL the capture would swallow the next two cells and fail to
        parse, so a True here is what proves the flag is off.
        """
        spec = {"pattern": r"damping_ratio = (.+)", "min": 0.01, "max": 0.03}
        self.assertTrue(self._check(_VALUE_CELLS_STDOUT, spec))

    def test_group_defaults_to_one(self):
        spec = {"pattern": r"(\w+) = ([0-9.]+)", "min": 0.0}
        # group 1 is the NAME, which does not parse as a float.
        self.assertFalse(self._check("damping = 0.018\n", spec))

    def test_explicit_group_index_selects_capture(self):
        spec = {"pattern": r"(\w+) = ([0-9.]+)", "group": 2, "min": 0.01, "max": 0.03}
        self.assertTrue(self._check("damping = 0.018\n", spec))

    def test_named_group_selects_capture(self):
        spec = {
            "pattern": r"\w+ = (?P<val>[0-9.]+)",
            "group": "val",
            "min": 0.01,
            "max": 0.03,
        }
        self.assertTrue(self._check("damping = 0.018\n", spec))

    def test_empty_capture_is_false(self):
        """A group that participated but matched nothing yields no value."""
        spec = {"pattern": r"X = ([0-9.]*)", "min": 0.0}
        self.assertFalse(self._check("X = \n", spec))

    def test_non_participating_optional_group_is_false(self):
        spec = {"pattern": r"X = (?:([0-9.]+)|undef)", "min": 0.0}
        self.assertFalse(self._check("X = undef\n", spec))

    # ── numeric constraints: bounds are INCLUSIVE ─────────────────────────────

    def test_min_only(self):
        spec = {"pattern": r"X = ([0-9.]+)", "min": 0.01}
        self.assertTrue(self._check("X = 0.018\n", spec))
        self.assertFalse(self._check("X = 0.002\n", spec))

    def test_max_only(self):
        spec = {"pattern": r"X = ([0-9.]+)", "max": 0.03}
        self.assertTrue(self._check("X = 0.018\n", spec))
        self.assertFalse(self._check("X = 0.31\n", spec))

    def test_bounds_are_inclusive_at_both_ends(self):
        spec = {"pattern": r"X = ([0-9.]+)", "min": 0.01, "max": 0.03}
        self.assertTrue(self._check("X = 0.01\n", spec))
        self.assertTrue(self._check("X = 0.03\n", spec))

    def test_just_outside_either_bound_is_false(self):
        spec = {"pattern": r"X = ([0-9.]+)", "min": 0.01, "max": 0.03}
        self.assertFalse(self._check("X = 0.009\n", spec))
        self.assertFalse(self._check("X = 0.031\n", spec))

    def test_degenerate_zero_fails_a_lower_bound(self):
        """The motivating #6876 defect: a clean eval that printed 0."""
        spec = {"pattern": r"degenerate_ratio = ([-+0-9.eE]+)", "min": 0.01}
        self.assertFalse(self._check(_VALUE_CELLS_STDOUT, spec))

    # ── tokens that are not numbers ───────────────────────────────────────────

    def test_unparseable_token_is_false(self):
        for stdout in ("X = undef\n", "X = m\n", "X = \n"):
            with self.subTest(stdout=stdout):
                self.assertFalse(
                    self._check(stdout, {"pattern": r"X = (\S*)", "min": -1e9})
                )

    def test_non_finite_tokens_are_false_under_a_bare_min(self):
        """Finiteness is STRUCTURAL, not opt-in.

        float("inf") >= 0.01 is True in Python, so a bounds-only check would
        wave inf straight through — re-opening a vacuity hole inside the kind
        that exists to close one.
        """
        spec = {"pattern": r"X = (\S+)", "min": 0.01}
        for token in ("nan", "inf", "-inf", "Infinity"):
            with self.subTest(token=token):
                self.assertFalse(self._check(f"X = {token}\n", spec))

    def test_finite_alone_accepts_a_number_and_rejects_garbage(self):
        spec = {"pattern": r"X = (\S+)", "finite": True}
        self.assertTrue(self._check("X = 0.018\n", spec))
        self.assertTrue(self._check("X = -12345.0\n", spec))
        self.assertFalse(self._check("X = undef\n", spec))
        self.assertFalse(self._check("X = inf\n", spec))

    def test_undef_cell_fails_a_finiteness_assertion(self):
        """The 'prints garbage with exit 0' case, on the real fixture shape."""
        spec = {"pattern": r"undef_ratio = (\S+)", "finite": True}
        self.assertFalse(self._check(_VALUE_CELLS_STDOUT, spec))

    def test_scientific_and_signed_notation_parse(self):
        spec = {"pattern": r"X = (\S+)", "min": 0.01, "max": 0.03}
        self.assertTrue(self._check("X = 1.8e-2\n", spec))
        self.assertTrue(self._check("X = +0.018\n", spec))
        self.assertFalse(
            self._check("X = -0.5\n", {"pattern": r"X = (\S+)", "min": 0.0})
        )
        self.assertTrue(
            self._check("X = -0.5\n", {"pattern": r"X = (\S+)", "max": 0.0})
        )

    # ── purity ────────────────────────────────────────────────────────────────

    def test_reads_only_stdout(self):
        """exit_code and stderr must not reach the answer."""
        spec = {"pattern": r"X = ([0-9.]+)", "min": 0.01, "max": 0.03}
        clean = pcc.observe_stdout_value(self._run("X = 0.018\n"), spec)
        noisy = pcc.observe_stdout_value(
            self._run("X = 0.018\n", exit_code=99, stderr="EvalError everywhere\n"),
            spec,
        )
        self.assertTrue(clean.satisfied)
        self.assertEqual(clean, noisy)

    def test_returns_a_plain_bool(self):
        spec = {"pattern": r"X = ([0-9.]+)", "min": 0.01}
        self.assertIsInstance(
            pcc.observe_stdout_value(self._run("X = 0.018\n"), spec).satisfied, bool
        )


if __name__ == "__main__":
    unittest.main()
