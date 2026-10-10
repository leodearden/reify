#!/usr/bin/env python3
"""
test_e3_analysis.py — stdlib unittest for the pre-registered E3 §7 analysis
(docs/prds/v0_6/e3-granularity/analysis/: e3_protocol, e3_stats, e3_sources,
e3_model, e3_analysis).

WHAT RUNS THIS: not the gate directly. run_all.sh discovers `test_*.sh` only,
so the discovered member is the thin wrapper tests/infra/test_e3_analysis.sh,
which invokes this file. A bare .py here would be silently never run.

IMPORT IDIOM: tests/infra/ (for the hermetic fixture builders in
e3_analysis_fixtures.py) and the analysis directory are prepended to sys.path,
so the modules import by their plain names without installation.

Every fixture lives under a tempfile dir: no network, no shared state, no
live runs.db, task store or main ref.
"""

import dataclasses
import sys
import unittest
from datetime import datetime, timedelta, timezone
from fractions import Fraction
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
INFRA_DIR = REPO_ROOT / "tests" / "infra"
ANALYSIS_DIR = REPO_ROOT / "docs" / "prds" / "v0_6" / "e3-granularity" / "analysis"

sys.path.insert(0, str(INFRA_DIR))
sys.path.insert(0, str(ANALYSIS_DIR))

import e3_analysis_fixtures as fx  # noqa: E402  (after sys.path manipulation)
from e3_protocol import E3, Arm, ProtocolViolation  # noqa: E402

UTC = timezone.utc


def _replace_prd(protocol, slug, **changes):
    """protocol.prds with the PRD named slug rebuilt with changes."""
    return tuple(
        dataclasses.replace(prd, **changes) if prd.slug == slug else prd
        for prd in protocol.prds
    )


def _registered_ids(protocol):
    ids = set(protocol.carve_outs)
    for prd in protocol.prds:
        ids |= prd.coalesced | prd.reused
        if prd.stamped_range is not None:
            low, high = prd.stamped_range
            ids |= set(range(low, high + 1))
    return ids


class ProtocolTests(unittest.TestCase):
    def test_mini_protocol_constructs(self):
        protocol = fx.mini_protocol()
        self.assertEqual(len(protocol.prds_in(Arm.COARSE)), 2)
        self.assertEqual(len(protocol.prds_in(Arm.STANDARD)), 2)
        self.assertEqual(protocol.prd("s1").stamped_count, 5)

    def test_coarse_prd_below_landing_floor_raises(self):
        base = fx.mini_protocol()
        prds = _replace_prd(base, "c2", coalesced=frozenset({201}))
        with self.assertRaises(ProtocolViolation) as caught:
            fx.mini_protocol(prds=prds)
        self.assertIn("landing-floor feasibility", caught.exception.invariant)
        message = str(caught.exception)
        self.assertIn("c2", message)
        self.assertIn("2", message)
        self.assertIn("1", message)

    def test_standard_gate_leaf_outside_stamped_range_raises(self):
        base = fx.mini_protocol()
        prds = _replace_prd(base, "s2", gate_leaves=frozenset({1299}))
        with self.assertRaises(ProtocolViolation) as caught:
            fx.mini_protocol(prds=prds)
        self.assertIn("1299", str(caught.exception))

    def test_standard_close_leaf_outside_stamped_range_raises(self):
        base = fx.mini_protocol()
        prds = _replace_prd(base, "s1", close_leaf=1106)
        with self.assertRaises(ProtocolViolation) as caught:
            fx.mini_protocol(prds=prds)
        self.assertIn("1106", str(caught.exception))

    def test_coarse_close_leaf_not_reused_raises(self):
        base = fx.mini_protocol()
        prds = _replace_prd(base, "c1", close_leaf=102)
        with self.assertRaises(ProtocolViolation) as caught:
            fx.mini_protocol(prds=prds)
        self.assertIn("102", str(caught.exception))

    def test_expected_freeze_set_overlapping_carve_outs_raises(self):
        with self.assertRaises(ProtocolViolation) as caught:
            fx.mini_protocol(expected_freeze_set=frozenset({1101, 150}))
        self.assertIn("150", str(caught.exception))

    def test_stratum_b_pair_must_name_one_registered_prd_per_arm(self):
        with self.assertRaises(ProtocolViolation):
            fx.mini_protocol(stratum_b_pair=("c1", "c2"))
        with self.assertRaises(ProtocolViolation):
            fx.mini_protocol(stratum_b_pair=("c1", "nope"))

    def test_replace_reruns_the_checks(self):
        with self.assertRaises(ProtocolViolation):
            dataclasses.replace(fx.mini_protocol(), landing_floor_k=3)

    def test_amendment_lookup(self):
        protocol = fx.mini_protocol()
        self.assertEqual(protocol.amendment_for(1102).source, "(c)")
        self.assertIsNone(protocol.amendment_for(1103))

    def test_mini_snapshot_candidates_cover_registration(self):
        protocol = fx.mini_protocol()
        candidates = set(protocol.snapshot_candidate_ids())
        self.assertTrue(_registered_ids(protocol) <= candidates)
        self.assertTrue(protocol.infrastructure <= candidates)

    def test_registered_e3_constant(self):
        coarse = {prd.slug for prd in E3.prds_in(Arm.COARSE)}
        standard = {prd.slug for prd in E3.prds_in(Arm.STANDARD)}
        self.assertEqual(coarse, {
            "spec-conformance-suite", "gui-purpose-surface",
            "gui-on-demand-measurement", "solution-set-completeness",
        })
        self.assertEqual(standard, {
            "driver-contract-implementation",
            "geometry-algebra-solver-unification", "solver-driver-parity",
        })
        self.assertEqual(
            {prd.slug for prd in E3.prds if prd.stratum == "B"},
            {"spec-conformance-suite", "driver-contract-implementation"},
        )
        coalesced = set().union(*(prd.coalesced for prd in E3.prds_in(Arm.COARSE)))
        self.assertEqual(coalesced, set(range(6898, 6914)))
        reused = set().union(*(prd.reused for prd in E3.prds_in(Arm.COARSE)))
        self.assertEqual(
            reused, {6706, 6719, 6745, 6748, 6768, 6789, 6791, 6836, 6839}
        )
        self.assertEqual(E3.prd("solver-driver-parity").stamped_count, 15)
        self.assertEqual(E3.prd("geometry-algebra-solver-unification").stamped_count, 19)
        self.assertEqual(E3.prd("driver-contract-implementation").stamped_count, 25)
        self.assertEqual(E3.landing_floor_k, 2)
        self.assertEqual(E3.alpha, Fraction(1, 10))
        self.assertEqual(E3.freeze_cutoff, datetime(2026, 8, 28, 7, 7, tzinfo=UTC))
        self.assertEqual(E3.expected_freeze_set, {6668, 6672, 6706, 6768, 6798})
        self.assertEqual(E3.drain_window, timedelta(seconds=300))
        self.assertEqual(E3.carve_outs, {6758, 6759})
        self.assertEqual(E3.infrastructure, {6897, 7029})
        candidates = set(E3.snapshot_candidate_ids())
        self.assertTrue(_registered_ids(E3) <= candidates)
        self.assertIn(7029, candidates)
        self.assertIn(6897, candidates)

    def test_e3_amendment_ledger(self):
        annotated = {
            6899, 6781, 6911, 6683, 6901, 6903, 6678, 6902, 6681, 6690,
            6898, 6908, 6909,
        }
        self.assertEqual({a.task_id for a in E3.amendments}, annotated)
        self.assertIsNone(E3.amendment_for(6798))

    def test_protocol_violation_names_invariant_and_values(self):
        violation = ProtocolViolation("freeze-window set", "{1, 2}", "{1, 3}")
        self.assertEqual(
            str(violation),
            "invariant freeze-window set violated: expected {1, 2}, found {1, 3}",
        )
        self.assertEqual(violation.invariant, "freeze-window set")
        self.assertEqual(violation.expected, "{1, 2}")
        self.assertEqual(violation.found, "{1, 3}")


if __name__ == "__main__":
    unittest.main()
