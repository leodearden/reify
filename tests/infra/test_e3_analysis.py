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
import json
import statistics
import sys
import tempfile
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
import e3_stats  # noqa: E402
import e3_sources  # noqa: E402
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


class StatsTests(unittest.TestCase):
    """The 4v3 null distribution of U is the Gaussian binomial [7 choose 3]_q:
    1,1,2,3,4,4,5,4,4,3,2,1,1 — so P(U<=0)=1/35, P(U<=1)=2/35, P(U<=2)=4/35."""

    def test_perfect_separation_in_prediction_direction(self):
        result = e3_stats.exact_mann_whitney_u(coarse=[1, 2, 3, 4], standard=[5, 6, 7])
        self.assertEqual(result.u, 0)
        self.assertEqual(result.p, Fraction(1, 35))
        self.assertEqual(result.arrangements, 35)
        self.assertEqual((result.n_coarse, result.n_standard), (4, 3))

    def test_one_inverted_pair(self):
        result = e3_stats.exact_mann_whitney_u(coarse=[1, 2, 3, 5], standard=[4, 6, 7])
        self.assertEqual(result.u, 1)
        self.assertEqual(result.p, Fraction(2, 35))

    def test_two_inverted_pairs(self):
        result = e3_stats.exact_mann_whitney_u(coarse=[1, 2, 3, 6], standard=[4, 5, 7])
        self.assertEqual(result.u, 2)
        self.assertEqual(result.p, Fraction(4, 35))

    def test_coarse_all_higher_is_against_prediction(self):
        result = e3_stats.exact_mann_whitney_u(coarse=[4, 5, 6, 7], standard=[1, 2, 3])
        self.assertEqual(result.u, 12)
        self.assertEqual(result.p, 1)

    def test_ties_score_half(self):
        result = e3_stats.exact_mann_whitney_u(coarse=[5.0] * 4, standard=[5.0] * 3)
        self.assertEqual(result.u, 6)
        self.assertEqual(result.p, 1)

    def test_partial_tie_counts_half_pair(self):
        result = e3_stats.exact_mann_whitney_u(coarse=[1, 2, 3, 4], standard=[4, 6, 7])
        self.assertEqual(result.u, Fraction(1, 2))

    def test_min_achievable_p_table(self):
        table = {
            (4, 3): Fraction(1, 35), (3, 3): Fraction(1, 20), (4, 2): Fraction(1, 15),
            (3, 2): Fraction(1, 10), (2, 3): Fraction(1, 10), (2, 2): Fraction(1, 6),
            (4, 1): Fraction(1, 5),
        }
        for (n_coarse, n_standard), expected in table.items():
            self.assertEqual(e3_stats.min_achievable_p(n_coarse, n_standard), expected)

    def test_three_v_two_perfect_separation_reaches_alpha_inclusive(self):
        result = e3_stats.exact_mann_whitney_u(coarse=[1, 2, 3], standard=[4, 5])
        self.assertEqual(result.p, Fraction(1, 10))
        self.assertTrue(result.p <= E3.alpha)

    def test_empty_group_raises(self):
        with self.assertRaises(ValueError):
            e3_stats.exact_mann_whitney_u(coarse=[], standard=[1, 2])
        with self.assertRaises(ValueError):
            e3_stats.exact_mann_whitney_u(coarse=[1], standard=[])

    def test_bootstrap_interval_is_seeded_and_brackets_the_median(self):
        values = [3.0, 9.0, 1.5, 7.25, 4.0, 12.0, 5.5]
        first = e3_stats.bootstrap_interval(
            values, statistic=statistics.median, seed="s", resamples=2000,
            level=Fraction(9, 10))
        second = e3_stats.bootstrap_interval(
            values, statistic=statistics.median, seed="s", resamples=2000,
            level=Fraction(9, 10))
        self.assertEqual(first, second)
        low, high = first
        self.assertLessEqual(low, statistics.median(values))
        self.assertLessEqual(statistics.median(values), high)

    def test_bootstrap_interval_degenerate_inputs(self):
        self.assertEqual(
            e3_stats.bootstrap_interval([4.5], statistic=statistics.median, seed="s",
                                        resamples=50, level=Fraction(9, 10)),
            (4.5, 4.5))
        self.assertIsNone(
            e3_stats.bootstrap_interval([], statistic=statistics.median, seed="s",
                                        resamples=50, level=Fraction(9, 10)))


FETCHED_AT = datetime(2026, 10, 10, 20, 0, tzinfo=UTC)


class SnapshotSourceTests(unittest.TestCase):
    def setUp(self):
        self.protocol = fx.mini_protocol()
        self.payload = fx.snapshot_payload(
            self.protocol, overrides={1102: fx.done_override("abc123")})

    def _fetch(self, payload=None):
        call_tool, calls = fx.fake_task_store(payload or self.payload)
        candidates = self.protocol.snapshot_candidate_ids() + (99999,)
        snapshot = e3_sources.fetch_task_snapshot(
            call_tool, self.protocol.project_root, candidates, FETCHED_AT)
        return snapshot, calls, candidates

    def test_fetch_reads_statuses_once_then_only_known_tasks(self):
        snapshot, calls, candidates = self._fetch()
        status_calls = [args for name, args in calls if name == "get_statuses"]
        self.assertEqual(status_calls, [{
            "project_root": self.protocol.project_root,
            "ids": [str(i) for i in candidates],
        }])
        self.assertEqual(calls[0][0], "get_statuses")
        fetched = [int(args["id"]) for name, args in calls if name == "get_task"]
        self.assertEqual(sorted(fetched), sorted(set(self.payload) & set(candidates)))
        self.assertNotIn(99999, fetched)
        self.assertEqual(snapshot.fetched_at, FETCHED_AT)
        self.assertEqual(snapshot.project_root, self.protocol.project_root)
        self.assertEqual({t.id for t in snapshot.tasks}, set(fetched))

    def test_fetch_parses_metadata_into_records(self):
        snapshot, _, _ = self._fetch()
        coalesced = snapshot.task(101)
        self.assertEqual(coalesced.x_e3_arm, "coarse")
        self.assertEqual(coalesced.x_e3_prd, "c1")
        self.assertFalse(coalesced.x_e3_reused)
        self.assertIsInstance(coalesced.constituents, tuple)
        self.assertEqual(len(coalesced.constituents), 2)
        self.assertEqual(coalesced.prd_path, "docs/prds/v0_6/c1.md")
        self.assertTrue(snapshot.task(103).x_e3_reused)
        standard = snapshot.task(1102)
        self.assertEqual(standard.status, "done")
        self.assertEqual(standard.provenance_commit, "abc123")
        self.assertIsNone(standard.x_e3_arm)
        self.assertEqual(standard.constituents, ())
        self.assertIsNone(snapshot.task(1101).provenance_commit)
        self.assertIsNone(snapshot.task(99999))

    def test_fetch_rejects_a_record_for_another_id(self):
        payload = fx.snapshot_payload(self.protocol, overrides={101: {"id": 999}})
        with self.assertRaises(ProtocolViolation) as caught:
            self._fetch(payload)
        self.assertEqual(caught.exception.invariant, "snapshot record id")
        self.assertIn("999", str(caught.exception))

    def test_write_then_read_round_trips(self):
        snapshot, _, _ = self._fetch()
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "tasks-snapshot.json"
            e3_sources.write_task_snapshot(snapshot, path)
            self.assertEqual(json.loads(path.read_text())["schema"], "e3-task-snapshot/1")
            self.assertEqual(e3_sources.read_task_snapshot(path), snapshot)

    def test_read_rejects_another_schema(self):
        snapshot, _, _ = self._fetch()
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "tasks-snapshot.json"
            e3_sources.write_task_snapshot(snapshot, path)
            document = json.loads(path.read_text())
            document["schema"] = "e3-task-snapshot/0"
            path.write_text(json.dumps(document))
            with self.assertRaises(ValueError):
                e3_sources.read_task_snapshot(path)

    def test_fused_memory_url_reads_mcp_config(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / ".mcp.json"
            path.write_text(json.dumps({"mcpServers": {
                "fused-memory": {"type": "http", "url": "http://127.0.0.1:8002/mcp"},
            }}))
            self.assertEqual(e3_sources.fused_memory_url(path), "http://127.0.0.1:8002/mcp")
            path.write_text(json.dumps({"mcpServers": {"escalation": {"url": "x"}}}))
            with self.assertRaises(ValueError) as caught:
                e3_sources.fused_memory_url(path)
            self.assertIn(str(path), str(caught.exception))


if __name__ == "__main__":
    unittest.main()
