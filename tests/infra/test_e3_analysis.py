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
live runs.db, task store or main ref. The one tracked input is the committed
analysis/results/*/tasks-snapshot.json, read-only, by CommittedRecordTests.
"""

import contextlib
import dataclasses
import io
import json
import shutil
import sqlite3
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
import e3_model  # noqa: E402
import e3_analysis  # noqa: E402
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

    def test_fetch_rejects_a_record_for_another_id_as_a_source_fault(self):
        payload = fx.snapshot_payload(self.protocol, overrides={101: {"id": 999}})
        with self.assertRaises(e3_sources.SourceError) as caught:
            self._fetch(payload)
        self.assertIn("101", str(caught.exception))
        self.assertIn("999", str(caught.exception))

    def test_read_rejects_a_fetch_instant_without_offset(self):
        snapshot, _, _ = self._fetch()
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "tasks-snapshot.json"
            e3_sources.write_task_snapshot(snapshot, path)
            document = json.loads(path.read_text())
            document["fetched_at"] = "2026-10-10T20:00:00"
            path.write_text(json.dumps(document))
            with self.assertRaises(e3_sources.SourceError):
                e3_sources.read_task_snapshot(path)

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


def at(day, hour, minute=0, second=0, month=9):
    return datetime(2026, month, day, hour, minute, second, tzinfo=UTC)


class RunsDbSourceTests(unittest.TestCase):
    """r1 completed; r2 crashed (no completed_at, a later run exists); r3 live."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.path = Path(self.tmp.name) / "runs.db"
        runs = [
            {"run_id": "r1", "started_at": at(1, 0), "completed_at": at(1, 10)},
            {"run_id": "rX", "started_at": at(1, 5), "completed_at": at(1, 6),
             "project_id": "dark-factory"},
            {"run_id": "r2", "started_at": at(1, 11), "completed_at": None},
            {"run_id": "r3", "started_at": at(2, 0), "completed_at": None},
        ]
        events = [
            fx.started(1101, "r1", at(1, 1)),
            fx.completed(1101, "r1", at(1, 2), "requeued"),
            fx.started(1101, "r1", at(1, 3)),
            fx.escalated(1101, "r1", at(1, 4), "esc-1101-1", severity="blocking"),
            fx.started(1101, "rX", at(1, 5, 30)),
            fx.completed(1101, "rX", at(1, 5, 40), "done"),
            fx.completed(1101, "r2", at(1, 11, 30), "cancelled"),
            fx.started(1102, "r2", at(1, 12)),
            fx.escalated(1102, "r2", at(1, 12, 30), "esc-1102-1", level=1),
            {"timestamp": at(1, 13), "run_id": "r2", "task_id": None,
             "event_type": "orchestrator_heartbeat", "data": {}},
            fx.started(1102, "r3", at(2, 1)),
            fx.started(1103, "r3", at(2, 2)),
        ]
        invocations = [
            {"run_id": "r1", "task_id": 1101, "role": "architect", "cost_usd": 0.5,
             "started_at": at(1, 1, 1)},
            {"run_id": "r1", "task_id": 1101, "role": "implementer", "cost_usd": 1.25,
             "started_at": at(1, 3, 1)},
            {"run_id": "r1", "task_id": 1101, "role": "reviewer_comprehensive",
             "cost_usd": 0.75, "started_at": at(1, 3, 30)},
            {"run_id": "r2", "task_id": 1101, "role": "steward", "cost_usd": 0.1,
             "started_at": at(1, 11, 15)},
            {"run_id": "r3", "task_id": 1103, "role": "architect", "cost_usd": 9.0,
             "started_at": at(2, 2, 1)},
            {"run_id": "rX", "task_id": 1101, "role": "architect", "cost_usd": 100.0,
             "started_at": at(1, 5, 31), "project_id": "dark-factory"},
        ]
        fx.make_runs_db(self.path, runs, events, invocations)
        self.data = e3_sources.read_runs_db(self.path, {1101, 1102})

    def _attempts(self, task_id):
        return [a for a in self.data.attempts if a.task_id == task_id]

    def test_start_pairs_with_first_later_completion_in_same_run(self):
        first, second = self._attempts(1101)
        self.assertEqual((first.run_id, first.started_at, first.ended_at, first.outcome),
                         ("r1", at(1, 1), at(1, 2), "requeued"))
        self.assertEqual(second.started_at, at(1, 3))
        self.assertIsNone(second.ended_at)
        self.assertIsNone(second.outcome)

    def test_attempts_across_runs_in_time_order(self):
        attempts = self._attempts(1102)
        self.assertEqual([a.run_id for a in attempts], ["r2", "r3"])
        self.assertEqual([a.started_at for a in attempts], [at(1, 12), at(2, 1)])

    def test_unclosed_attempt_in_completed_run_ends_with_the_run(self):
        second = self._attempts(1101)[1]
        self.assertEqual(second.run_ended_at, at(1, 10))

    def test_crashed_run_ends_at_its_last_event(self):
        in_crashed_run = self._attempts(1102)[0]
        self.assertEqual(in_crashed_run.run_ended_at, at(1, 13))

    def test_latest_run_without_completion_is_live(self):
        live = self._attempts(1102)[1]
        self.assertIsNone(live.run_ended_at)

    def test_closed_attempts_carry_their_run_end(self):
        self.assertEqual(self._attempts(1101)[0].run_ended_at, at(1, 10))

    def test_invocations_cover_every_role_of_requested_tasks(self):
        self.assertEqual(
            sorted((i.task_id, i.role, i.cost_usd, i.started_at) for i in self.data.invocations),
            sorted([
                (1101, "architect", 0.5, at(1, 1, 1)),
                (1101, "implementer", 1.25, at(1, 3, 1)),
                (1101, "reviewer_comprehensive", 0.75, at(1, 3, 30)),
                (1101, "steward", 0.1, at(1, 11, 15)),
            ]))

    def test_escalations_parse_with_optional_severity(self):
        by_id = {e.escalation_id: e for e in self.data.escalations}
        self.assertEqual(set(by_id), {"esc-1101-1", "esc-1102-1"})
        blocking = by_id["esc-1101-1"]
        self.assertEqual((blocking.task_id, blocking.created_at, blocking.severity,
                          blocking.level, blocking.category),
                         (1101, at(1, 4), "blocking", None, "review_issues"))
        self.assertIsNone(by_id["esc-1102-1"].severity)
        self.assertEqual(by_id["esc-1102-1"].level, 1)

    def test_timestamps_are_aware_utc(self):
        moments = [a.started_at for a in self.data.attempts]
        moments += [i.started_at for i in self.data.invocations]
        moments += [e.created_at for e in self.data.escalations]
        for moment in moments:
            self.assertEqual(moment.utcoffset(), timedelta(0))

    def test_other_projects_are_ignored(self):
        self.assertNotIn("rX", {a.run_id for a in self.data.attempts})
        self.assertNotIn(100.0, {i.cost_usd for i in self.data.invocations})

    def test_missing_database_raises_without_creating_it(self):
        missing = Path(self.tmp.name) / "absent" / "runs.db"
        with self.assertRaises(sqlite3.Error):
            e3_sources.read_runs_db(missing, {1101})
        self.assertFalse(missing.exists())
        bare = Path(self.tmp.name) / "absent.db"
        with self.assertRaises(sqlite3.Error):
            e3_sources.read_runs_db(bare, {1101})
        self.assertFalse(bare.exists())


class GitSourceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.repo = Path(cls.tmp.name) / "repo"
        new_file = "".join(f"new line {n}\n" for n in range(10))
        cls.shas = fx.make_merge_repo(cls.repo, [
            (101, "2026-09-27T21:10:57+01:00", {
                "feature/new.txt": new_file,
                fx.BASE_FILE: fx.base_with_edits({2, 7, 11}),
                "blob.bin": b"\x00\x01\x02\x03",
            }),
            (102, "2026-09-28T10:00:00+00:00", {"other.txt": "a\nb\n"}),
            (103, "2026-09-29T10:00:00+00:00", {"t103.txt": "1\n"}),
            (103, "2026-09-30T10:00:00+00:00", {"t103.txt": "1\n2\n"}),
        ])
        cls.merges = e3_sources.read_task_merges(cls.repo, "main", {101, 103})

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def _of(self, task_id):
        return [m for m in self.merges if m.task_id == task_id]

    def test_merge_carries_first_parent_numstat(self):
        (merge,) = self._of(101)
        self.assertEqual(merge.commit, self.shas[0][1])
        self.assertEqual((merge.insertions, merge.deletions), (13, 3))
        self.assertEqual(merge.loc, 16)

    def test_committer_date_is_normalised_to_utc(self):
        (merge,) = self._of(101)
        self.assertEqual(merge.merged_at, datetime(2026, 9, 27, 20, 10, 57, tzinfo=UTC))
        self.assertEqual(merge.merged_at.utcoffset(), timedelta(0))

    def test_unrequested_and_non_task_merges_are_ignored(self):
        self.assertEqual({m.task_id for m in self.merges}, {101, 103})
        self.assertEqual(len(self.merges), 3)

    def test_task_merged_twice_yields_two_merges(self):
        merges = sorted(self._of(103), key=lambda m: m.merged_at)
        self.assertEqual([m.commit for m in merges], [self.shas[2][1], self.shas[3][1]])
        self.assertEqual([(m.insertions, m.deletions) for m in merges], [(1, 0), (1, 0)])

    def test_unknown_ref_raises_source_error_with_git_stderr(self):
        with self.assertRaises(e3_sources.SourceError) as caught:
            e3_sources.read_task_merges(self.repo, "no-such-ref", {101})
        self.assertIn("no-such-ref", str(caught.exception))


class AttributionTests(unittest.TestCase):
    def setUp(self):
        self.protocol = fx.mini_protocol()

    def _attribute(self, overrides=None):
        snapshot = fx.task_snapshot(self.protocol, overrides)
        return {task.id: task for task in e3_model.attribute(snapshot, self.protocol)}

    def _violation(self, overrides=None, snapshot=None):
        snapshot = snapshot or fx.task_snapshot(self.protocol, overrides)
        with self.assertRaises(ProtocolViolation) as caught:
            e3_model.attribute(snapshot, self.protocol)
        return caught.exception

    def test_coarse_members_come_from_the_tag(self):
        tasks = self._attribute()
        for task_id, weight in ((101, 2), (102, 3), (201, 2), (202, 3)):
            task = tasks[task_id]
            self.assertIs(task.arm, Arm.COARSE)
            self.assertTrue(task.is_coalesced)
            self.assertFalse(task.is_reused)
            self.assertEqual(task.scope_weight, weight)
            self.assertTrue(task.counts_toward_floor)
        reused = tasks[103]
        self.assertEqual((reused.prd_slug, reused.is_reused, reused.is_coalesced,
                          reused.scope_weight, reused.counts_toward_floor),
                         ("c1", True, False, 1, False))

    def test_standard_members_are_untagged_stamped_leaves(self):
        tasks = self._attribute()
        s1 = {i: tasks[i] for i in range(1101, 1106)}
        for task in s1.values():
            self.assertIs(task.arm, Arm.STANDARD)
            self.assertEqual(task.prd_slug, "s1")
            self.assertEqual(task.scope_weight, 1)
        self.assertEqual({i for i, t in s1.items() if not t.counts_toward_floor}, {1104, 1105})

    def test_retired_carve_outs_and_infrastructure_never_appear(self):
        tasks = self._attribute()
        self.assertEqual(len(tasks), 2 * 3 + 2 * 5)
        for excluded in (fx.MINI_RETIRED, *fx.MINI_CARVE_OUTS, *fx.MINI_INFRASTRUCTURE):
            self.assertNotIn(excluded, tasks)

    def test_results_are_sorted_by_id(self):
        snapshot = fx.task_snapshot(self.protocol)
        ids = [task.id for task in e3_model.attribute(snapshot, self.protocol)]
        self.assertEqual(ids, sorted(ids))

    def test_amended_task_carries_its_amendment(self):
        tasks = self._attribute()
        self.assertEqual(tasks[1102].amendment, self.protocol.amendment_for(1102))
        self.assertIsNone(tasks[1101].amendment)

    def test_missing_registered_coarse_task_raises(self):
        violation = self._violation({102: None})
        self.assertIn("102", str(violation))

    def test_untagged_registered_coarse_task_raises(self):
        violation = self._violation({102: {"metadata": {"x_e3_arm": None}}})
        self.assertIn("{101, 102, 103}", str(violation))
        self.assertIn("{101, 103}", str(violation))

    def test_coarse_tag_for_unregistered_task_raises(self):
        violation = self._violation({201: {"metadata": {"x_e3_prd": "c1",
                                                        "prd_path": "docs/prds/v0_6/c1.md"}}})
        self.assertIn("201", str(violation))

    def test_coarse_tag_naming_an_unknown_prd_raises(self):
        self._violation({104: {"metadata": {"x_e3_arm": "coarse", "x_e3_prd": "zz",
                                            "prd_path": "docs/prds/v0_6/zz.md"}}})

    def test_reused_flag_must_match_registration(self):
        self._violation({101: {"metadata": {"x_e3_reused": True}}})
        self._violation({103: {"metadata": {"x_e3_reused": None}}})

    def test_coarse_prd_path_must_match_its_x_e3_prd(self):
        violation = self._violation({101: {"metadata": {"prd_path": "docs/prds/v0_6/c2.md"}}})
        self.assertIn("101", str(violation))

    def test_standard_member_count_must_match(self):
        violation = self._violation({1103: None})
        self.assertIn("s1", str(violation))
        self.assertIn("5", str(violation))
        self.assertIn("4", str(violation))

    def test_untagged_standard_task_outside_stamped_range_raises(self):
        violation = self._violation({1199: {"metadata": {"prd_path": "docs/prds/v0_6/s1.md"}}})
        self.assertIn("1199", str(violation))

    def test_snapshot_from_another_project_raises(self):
        snapshot = dataclasses.replace(fx.task_snapshot(self.protocol), project_root="/elsewhere")
        violation = self._violation(snapshot=snapshot)
        self.assertIn("/elsewhere", str(violation))


RUN_END = datetime(2026, 9, 10, 12, 0, tzinfo=UTC)


def attempt(task_id=1101, started_at=None, ended_at=None, outcome=None,
            run_ended_at=RUN_END, run_id="r1"):
    return e3_sources.Attempt(
        task_id=task_id, run_id=run_id,
        started_at=started_at or RUN_END - timedelta(hours=6),
        ended_at=ended_at, outcome=outcome, run_ended_at=run_ended_at)


class CensoringTests(unittest.TestCase):
    def setUp(self):
        self.protocol = fx.mini_protocol()
        self.arm_tasks = e3_model.attribute(fx.task_snapshot(self.protocol), self.protocol)

    def _classify(self, **fields):
        return e3_model.classify_attempt(attempt(**fields), self.protocol)

    def test_cancelled_at_the_run_boundary_is_drain_censored(self):
        self.assertIs(
            self._classify(ended_at=RUN_END - timedelta(seconds=40), outcome="cancelled"),
            e3_model.AttemptClass.DRAIN_CENSORED)

    def test_cancelled_far_from_the_run_end_is_rework(self):
        self.assertIs(
            self._classify(ended_at=RUN_END - timedelta(hours=2), outcome="cancelled"),
            e3_model.AttemptClass.COUNTED)

    def test_drain_window_is_inclusive(self):
        window = self.protocol.drain_window
        self.assertIs(self._classify(ended_at=RUN_END - window, outcome="cancelled"),
                      e3_model.AttemptClass.DRAIN_CENSORED)
        self.assertIs(
            self._classify(ended_at=RUN_END - window - timedelta(seconds=1), outcome="cancelled"),
            e3_model.AttemptClass.COUNTED)

    def test_unfinished_attempt_in_an_ended_run_is_drain_censored(self):
        self.assertIs(self._classify(ended_at=None), e3_model.AttemptClass.DRAIN_CENSORED)

    def test_unfinished_attempt_in_a_live_run_is_in_flight(self):
        self.assertIs(self._classify(ended_at=None, run_ended_at=None),
                      e3_model.AttemptClass.IN_FLIGHT)

    def test_other_outcomes_always_count(self):
        for outcome in ("done", "requeued", "blocked", "escalated"):
            for distance in (timedelta(seconds=10), timedelta(hours=3)):
                self.assertIs(
                    self._classify(ended_at=RUN_END - distance, outcome=outcome),
                    e3_model.AttemptClass.COUNTED, (outcome, distance))

    def _freeze(self, attempts):
        return e3_model.freeze_window_set(self.arm_tasks, attempts, self.protocol)

    def test_freeze_window_set_matches_expected(self):
        before = fx.MINI_FREEZE_CUTOFF - timedelta(minutes=10)
        attempts = [
            attempt(task_id=1101, started_at=before),
            attempt(task_id=1101, started_at=fx.MINI_FREEZE_CUTOFF + timedelta(days=2)),
            attempt(task_id=1102, started_at=fx.MINI_FREEZE_CUTOFF),
            attempt(task_id=150, started_at=before),
            attempt(task_id=fx.MINI_RETIRED, started_at=before),
        ]
        self.assertEqual(self._freeze(attempts), {1101})

    def test_freeze_window_drift_raises_with_both_sets(self):
        before = fx.MINI_FREEZE_CUTOFF - timedelta(minutes=10)
        with self.assertRaises(ProtocolViolation) as caught:
            self._freeze([attempt(task_id=1101, started_at=before),
                          attempt(task_id=202, started_at=before)])
        self.assertEqual(caught.exception.invariant, "freeze-window set")
        self.assertIn("{1101}", str(caught.exception))
        self.assertIn("{202, 1101}", str(caught.exception))
        with self.assertRaises(ProtocolViolation) as caught:
            self._freeze([])
        self.assertEqual(caught.exception.invariant, "freeze-window set")
        self.assertIn("{}", str(caught.exception))


AS_OF = datetime(2026, 10, 10, 20, 0, tzinfo=UTC)


def inv(task_id, cost, started_at, role="implementer"):
    return e3_sources.Invocation(task_id=task_id, role=role, started_at=started_at,
                                 cost_usd=cost)


def esc(task_id, escalation_id, created_at, severity="blocking", level=None):
    return e3_sources.Escalation(task_id=task_id, escalation_id=escalation_id,
                                 created_at=created_at, severity=severity, level=level,
                                 category="review_issues")


def merge(task_id, commit, merged_at, insertions, deletions=0):
    return e3_sources.Merge(task_id=task_id, commit=commit, merged_at=merged_at,
                            insertions=insertions, deletions=deletions)


def closed(task_id, started_at, outcome, ended_at=None, run_ended_at=None):
    ended_at = ended_at or started_at + timedelta(hours=1)
    return attempt(task_id=task_id, started_at=started_at, ended_at=ended_at,
                   outcome=outcome, run_ended_at=run_ended_at or ended_at + timedelta(hours=5))


def drained(task_id, started_at):
    ended_at = started_at + timedelta(hours=1)
    return attempt(task_id=task_id, started_at=started_at, ended_at=ended_at,
                   outcome="cancelled", run_ended_at=ended_at + timedelta(seconds=20))


class EndpointScenario:
    """A mini 2v2 protocol with freeze set {1101, 203}, at AS_OF:
    c1: 101 landed (loc 1000, $20, 2 counted + 1 drained attempt, 2 blocking
        escalations, straddles the era), 102 unlanded (1 counted attempt, 1
        blocking escalation, pre-era), 103 reused unlanded;
    c2: 202 landed (weight 3), 201 unlanded (weight 2), 203 freeze-window;
    s1: 1101 freeze-window but landed, 1102 landed (loc 200, $4, provenance
        mismatch, amended), 1103 done but merged after AS_OF, 1104 done with no
        merge, 1105 pending yet merged after AS_OF;
    s2: nothing landed;
    carve-outs: 150 landed (loc 40, $1.50, 1 counted attempt), 151 untouched."""

    def __init__(self):
        self.protocol = fx.mini_protocol(expected_freeze_set=frozenset({1101, 203}))
        self.freeze_set = frozenset({1101, 203})
        self.snapshot = fx.task_snapshot(self.protocol, {
            101: fx.done_override("m101"), 202: fx.done_override("m202"),
            1101: fx.done_override("m1101"), 1102: fx.done_override("not-a-merge"),
            1103: fx.done_override("m1103"), 1104: fx.done_override("m1104"),
            150: fx.done_override("m150"),
        })
        self.arm_tasks = e3_model.attribute(self.snapshot, self.protocol)
        pre, post = at(28, 12, month=8), at(15, 9)
        late = AS_OF + timedelta(days=3)
        self.runs = e3_sources.RunsData(
            attempts=(
                closed(101, pre, "requeued"), closed(101, post, "done"), drained(101, post),
                closed(101, late, "done"),
                closed(102, pre, "blocked"),
                drained(203, at(27, 9, month=8)),
                closed(1101, at(27, 8, month=8), "done"), drained(1101, at(2, 1)),
                closed(1102, at(20, 9), "done"),
                drained(1103, at(21, 9)),
                closed(150, at(22, 9), "done"),
                attempt(task_id=202, started_at=AS_OF - timedelta(hours=2),
                        ended_at=AS_OF + timedelta(hours=1), outcome="done",
                        run_ended_at=AS_OF + timedelta(hours=2)),
            ),
            invocations=(
                inv(101, 10.0, pre, "architect"), inv(101, 5.0, post),
                inv(101, 5.0, post, "reviewer_comprehensive"), inv(101, 7.0, late),
                inv(102, 3.0, pre), inv(1101, 2.5, at(27, 8, month=8)),
                inv(1102, 4.0, at(20, 9)), inv(202, 6.0, AS_OF - timedelta(hours=2)),
                inv(150, 1.5, at(22, 9)),
            ),
            escalations=(
                esc(101, "esc-101-1", post), esc(101, "esc-101-1", post + timedelta(hours=1)),
                esc(101, "esc-101-2", post, level=1), esc(101, "esc-101-3", post, severity=None),
                esc(101, "esc-101-4", post, severity="info"), esc(101, "esc-101-5", late),
                esc(102, "esc-102-1", pre),
            ),
        )
        self.merges = (
            merge(101, "m101", at(20, 9), 900, 100),
            merge(202, "m202", at(22, 9), 300),
            merge(1101, "m1101", at(5, 9), 50),
            merge(1102, "m1102", at(25, 9), 150, 50),
            merge(1103, "m1103", AS_OF + timedelta(days=5), 80),
            merge(1105, "m1105", AS_OF + timedelta(days=6), 10),
            merge(150, "m150", at(23, 9), 30, 10),
        )

    def outcomes(self, as_of=AS_OF):
        return e3_model.task_outcomes(self.arm_tasks, self.snapshot, self.runs, self.merges,
                                      self.freeze_set, as_of, self.protocol)


class EndpointTests(unittest.TestCase):
    def setUp(self):
        self.scenario = EndpointScenario()
        self.outcomes = {o.task.id: o for o in self.scenario.outcomes()}
        self.ledgers = {task_id: o.ledger for task_id, o in self.outcomes.items()}

    def _endpoints(self, drop=frozenset()):
        return {e.prd_slug: e for e in e3_model.prd_endpoints(
            tuple(self.outcomes.values()), self.scenario.protocol, drop=drop)}

    def test_landed_requires_done_and_a_merge_by_as_of(self):
        landed = self.ledgers[101]
        self.assertEqual(landed.landed_at, at(20, 9))
        self.assertEqual(landed.loc, 1000)
        self.assertEqual([m.commit for m in landed.merges], ["m101"])
        self.assertIsNone(self.ledgers[1103].landed_at)
        self.assertEqual(self.ledgers[1103].loc, 0)
        self.assertIsNone(self.ledgers[1104].landed_at)
        self.assertIsNone(self.ledgers[1105].landed_at)

    def test_cost_sums_every_role_started_by_as_of(self):
        self.assertEqual(self.ledgers[101].cost, 20.0)
        self.assertEqual(self.ledgers[1102].cost, 4.0)
        self.assertEqual(self.ledgers[103].cost, 0.0)

    def test_attempts_split_by_class_as_of(self):
        ledger = self.ledgers[101]
        self.assertEqual((ledger.attempts_counted, ledger.attempts_drain_censored,
                          ledger.attempts_in_flight), (2, 1, 0))

    def test_attempt_ending_after_as_of_is_in_flight(self):
        ledger = self.ledgers[202]
        self.assertEqual((ledger.attempts_counted, ledger.attempts_in_flight), (0, 1))

    def test_blocking_escalations_are_distinct_blocking_ids_by_as_of(self):
        self.assertEqual(self.ledgers[101].blocking_escalations, 2)
        self.assertEqual(self.ledgers[102].blocking_escalations, 1)

    def test_freeze_window_task_is_flagged_and_never_eligible(self):
        frozen = self.outcomes[1101]
        self.assertEqual(frozen.excluded, "freeze-window")
        self.assertEqual(frozen.ledger.landed_at, at(5, 9))
        self.assertFalse(frozen.landed_eligible)
        self.assertTrue(self.outcomes[1102].landed_eligible)
        self.assertIsNone(self.outcomes[1102].excluded)

    def test_era_by_attempt_starts(self):
        self.assertEqual(self.outcomes[101].era, "straddle")
        self.assertEqual(self.outcomes[102].era, "pre")
        self.assertEqual(self.outcomes[1102].era, "post")
        self.assertIsNone(self.outcomes[103].era)

    def test_integrity_anomalies(self):
        kinds = {task_id: {a.kind for a in ledger.anomalies}
                 for task_id, ledger in self.ledgers.items()}
        self.assertEqual(kinds[1104], {"done-without-merge"})
        self.assertEqual(kinds[1105], {"merged-but-not-done"})
        self.assertEqual(kinds[1102], {"provenance-not-a-task-merge"})
        self.assertEqual(kinds[101], set())
        self.assertEqual(kinds[1103], set())

    def test_usd_per_kloc_over_landed_eligible_tasks(self):
        endpoints = self._endpoints()
        self.assertEqual(endpoints["c1"].usd_per_kloc, 20.0)
        self.assertEqual(endpoints["s1"].usd_per_kloc, 20.0)
        self.assertEqual(endpoints["c2"].usd_per_kloc, 20.0)
        self.assertIsNone(endpoints["s2"].usd_per_kloc)

    def test_rework_and_escalation_rates(self):
        c1 = self._endpoints()["c1"]
        self.assertEqual(c1.attempts_per_task, 2.0)
        self.assertEqual(c1.attempts_per_kloc, 2.0)
        self.assertEqual(c1.blocking_esc_per_task, 2.0)
        self.assertEqual(c1.blocking_esc_per_kloc, 2.0)
        s2 = self._endpoints()["s2"]
        self.assertIsNone(s2.attempts_per_task)
        self.assertIsNone(s2.blocking_esc_per_kloc)

    def test_task_counts_are_disjoint_and_complete(self):
        endpoints = self._endpoints()
        expected = {"c1": (1, 2, 0), "c2": (1, 1, 1), "s1": (1, 3, 1), "s2": (0, 5, 0)}
        for slug, counts in expected.items():
            e = endpoints[slug]
            self.assertEqual((e.n_landed, e.n_right_censored, e.n_freeze_excluded), counts)
            self.assertEqual(e.n_landed + e.n_right_censored + e.n_freeze_excluded, e.n_tasks)

    def test_landed_scope_share_weights_constituents(self):
        endpoints = self._endpoints()
        self.assertEqual(endpoints["c2"].landed_scope_share, Fraction(3, 5))
        self.assertEqual(endpoints["c1"].landed_scope_share, Fraction(2, 6))
        self.assertEqual(endpoints["s2"].landed_scope_share, Fraction(0))

    def test_unlanded_work_is_reported_separately(self):
        c1 = self._endpoints()["c1"]
        self.assertEqual((c1.unlanded_attempts, c1.unlanded_blocking_escalations), (1, 1))

    def test_drop_removes_a_task_from_every_count(self):
        s1 = self._endpoints(drop=frozenset({1102}))["s1"]
        self.assertEqual((s1.n_tasks, s1.n_landed), (4, 0))
        self.assertIsNone(s1.usd_per_kloc)

    def test_arm_censoring_counts_are_three_separate_counts(self):
        counts = {c.arm: c for c in e3_model.arm_censoring_counts(tuple(self.outcomes.values()))}
        coarse, standard = counts[Arm.COARSE], counts[Arm.STANDARD]
        self.assertEqual((coarse.freeze_window_excluded, coarse.right_censored,
                          coarse.drain_censored_attempts), (1, 3, 1))
        self.assertEqual((standard.freeze_window_excluded, standard.right_censored,
                          standard.drain_censored_attempts), (1, 8, 1))

    def test_carve_outs_get_ledgers_beside_the_arms(self):
        scenario = self.scenario
        ledgers = e3_model.carve_out_ledgers(scenario.snapshot, scenario.runs, scenario.merges,
                                             AS_OF, scenario.protocol)
        self.assertEqual([ledger.task_id for ledger in ledgers], [150, 151])
        landed, untouched = ledgers
        self.assertEqual((landed.status, landed.landed_at, landed.loc, landed.cost,
                          landed.attempts_counted), ("done", at(23, 9), 40, 1.5, 1))
        self.assertEqual((untouched.status, untouched.landed_at, untouched.cost,
                          untouched.attempts_counted), ("pending", None, 0.0, 0))
        self.assertFalse({150, 151} & set(self.outcomes))

    def test_earlier_as_of_is_a_frozen_snapshot(self):
        earlier = {o.task.id: o.ledger for o in self.scenario.outcomes(as_of=at(21, 0))}
        self.assertIsNone(earlier[202].landed_at)
        self.assertEqual(earlier[101].landed_at, at(20, 9))
        self.assertIsNone(earlier[1102].landed_at)


def landings_outcomes(protocol, landings, statuses=None, freeze_set=frozenset({1101}),
                      as_of=AS_OF):
    """Outcomes where each task in landings is done and merged at its instant;
    statuses overrides other tasks' snapshot status."""
    overrides = {task_id: fx.done_override(f"m{task_id}") for task_id in landings}
    overrides.update({task_id: {"status": status} for task_id, status in (statuses or {}).items()})
    snapshot = fx.task_snapshot(protocol, overrides)
    arm_tasks = e3_model.attribute(snapshot, protocol)
    merges = tuple(merge(task_id, f"m{task_id}", landed_at, 100)
                   for task_id, landed_at in landings.items())
    runs = e3_sources.RunsData(attempts=(), invocations=(), escalations=())
    return e3_model.task_outcomes(arm_tasks, snapshot, runs, merges, freeze_set, as_of,
                                  protocol)


ALL_FLOORS = {101: at(10, 0), 102: at(11, 0), 201: at(12, 0), 202: at(13, 0),
              1102: at(14, 0), 1103: at(15, 0), 1201: at(16, 0), 1202: at(17, 0)}
ALL_CLOSE = {103: at(18, 0), 203: at(19, 0), 1105: at(20, 0), 1205: at(21, 0)}
ALL_CLOSE_BUT_1205 = {k: v for k, v in ALL_CLOSE.items() if k != 1205}


class StopRuleTests(unittest.TestCase):
    def setUp(self):
        self.protocol = fx.mini_protocol()

    def _probe(self, landings, operator_rule_i_instant=None, **kwargs):
        outcomes = landings_outcomes(self.protocol, landings, **kwargs)
        return e3_model.stop_rule_probe(outcomes, self.protocol, operator_rule_i_instant)

    def _floor(self, probe, slug):
        (floor,) = [f for f in probe.per_prd if f.prd_slug == slug]
        return floor

    def test_coarse_counts_coalesced_landings_only(self):
        probe = self._probe({101: at(10, 0), 103: at(11, 0)})
        self.assertEqual(self._floor(probe, "c1").qualifying, ((101, at(10, 0)),))

    def test_standard_excludes_gate_and_close_leaves(self):
        probe = self._probe({1102: at(10, 0), 1103: at(11, 0), 1104: at(12, 0),
                             1105: at(13, 0)})
        self.assertEqual([task_id for task_id, _ in self._floor(probe, "s1").qualifying],
                         [1102, 1103])

    def test_freeze_window_landings_never_qualify(self):
        probe = self._probe({1101: at(10, 0), 1102: at(11, 0)})
        self.assertEqual(self._floor(probe, "s1").qualifying, ((1102, at(11, 0)),))

    def test_floor_met_at_the_kth_landing(self):
        probe = self._probe({1103: at(12, 0), 1102: at(11, 0), 101: at(10, 0)})
        s1 = self._floor(probe, "s1")
        self.assertTrue(s1.floor_met)
        self.assertEqual(s1.floor_instant, at(12, 0))
        c1 = self._floor(probe, "c1")
        self.assertFalse(c1.floor_met)
        self.assertIsNone(c1.floor_instant)
        self.assertIs(c1.arm, Arm.COARSE)

    def test_rule_ii_instant_is_the_floor_completing_landing(self):
        probe = self._probe(ALL_FLOORS)
        self.assertTrue(probe.rule_ii_met)
        self.assertEqual(probe.rule_ii_instant, at(17, 0))
        self.assertEqual(probe.trigger_instant, at(17, 0))
        short = self._probe({k: v for k, v in ALL_FLOORS.items() if k != 202})
        self.assertFalse(short.rule_ii_met)
        self.assertIsNone(short.rule_ii_instant)
        self.assertIsNone(short.trigger_instant)

    def test_rule_i_all_close_leaves_terminal(self):
        probe = self._probe(ALL_CLOSE)
        self.assertTrue(probe.rule_i.met)
        self.assertEqual(probe.rule_i.instant, at(21, 0))
        self.assertFalse(probe.rule_i.undetermined)
        self.assertFalse(probe.rule_ii_met)
        self.assertEqual(probe.trigger_instant, at(21, 0))

    def test_rule_i_with_a_cancelled_close_leaf_is_undetermined(self):
        probe = self._probe(ALL_CLOSE_BUT_1205, statuses={1205: "cancelled"})
        self.assertFalse(probe.rule_i.met)
        self.assertTrue(probe.rule_i.undetermined)
        self.assertEqual(probe.rule_i.cancelled, (1205,))
        self.assertIsNone(probe.rule_i.instant)
        self.assertEqual(probe.rule_i.last_merge, at(20, 0))
        self.assertIsNone(probe.trigger_instant)

    def test_operator_fixed_rule_i_instant_determines_the_trigger(self):
        probe = self._probe(ALL_CLOSE_BUT_1205, statuses={1205: "cancelled"},
                            operator_rule_i_instant=at(25, 0))
        self.assertTrue(probe.rule_i.met)
        self.assertTrue(probe.rule_i.operator_fixed)
        self.assertFalse(probe.rule_i.undetermined)
        self.assertEqual(probe.rule_i.instant, at(25, 0))
        self.assertEqual(probe.trigger_instant, at(25, 0))

    def test_operator_rule_i_instant_before_the_last_close_merge_is_refused(self):
        with self.assertRaises(e3_model.OperatorInstantRefused) as caught:
            self._probe(ALL_CLOSE_BUT_1205, statuses={1205: "cancelled"},
                        operator_rule_i_instant=at(19, 0))
        self.assertIn("last close-leaf merge", str(caught.exception))

    def test_operator_rule_i_instant_is_refused_unless_rule_i_is_undetermined(self):
        for landings, statuses in ((ALL_CLOSE, None), (ALL_CLOSE_BUT_1205, None)):
            with self.assertRaises(e3_model.OperatorInstantRefused):
                self._probe(landings, statuses=statuses, operator_rule_i_instant=at(25, 0))

    def test_undetermined_rule_i_withholds_a_trigger_it_may_precede(self):
        close = {103: at(2, 0), 203: at(3, 0), 1105: at(4, 0)}
        probe = self._probe({**ALL_FLOORS, **close}, statuses={1205: "cancelled"})
        self.assertTrue(probe.rule_ii_met)
        self.assertTrue(probe.rule_i.undetermined)
        self.assertIsNone(probe.trigger_instant)

    def test_undetermined_rule_i_after_the_floor_keeps_the_rule_ii_trigger(self):
        probe = self._probe({**ALL_FLOORS, **ALL_CLOSE_BUT_1205}, statuses={1205: "cancelled"})
        self.assertTrue(probe.rule_i.undetermined)
        self.assertEqual(probe.trigger_instant, at(17, 0))

    def test_rule_i_unmet_while_a_close_leaf_is_open(self):
        probe = self._probe(ALL_CLOSE_BUT_1205)
        self.assertFalse(probe.rule_i.met)
        self.assertFalse(probe.rule_i.undetermined)
        self.assertIsNone(probe.rule_i.instant)

    def test_rule_i_open_close_leaf_hides_other_cancellations(self):
        landings = {k: v for k, v in ALL_CLOSE_BUT_1205.items() if k != 1105}
        probe = self._probe(landings, statuses={1205: "cancelled"})
        self.assertFalse(probe.rule_i.met)
        self.assertEqual(probe.rule_i.cancelled, ())

    def test_rule_i_close_leaf_merged_after_as_of_is_not_terminal(self):
        probe = self._probe({**ALL_CLOSE, 1205: AS_OF + timedelta(days=1)})
        self.assertFalse(probe.rule_i.met)

    def test_trigger_is_the_earliest_met_rule(self):
        probe = self._probe({**ALL_FLOORS, **ALL_CLOSE})
        self.assertTrue(probe.rule_i.met and probe.rule_ii_met)
        self.assertEqual(probe.trigger_instant, at(17, 0))

    def test_todays_analogue_has_no_trigger_and_ignores_later_landings(self):
        landings = {1102: at(27, 20), 1101: at(12, 7), 1103: AS_OF + timedelta(days=2)}
        probe = self._probe(landings)
        self.assertEqual(self._floor(probe, "s1").qualifying, ((1102, at(27, 20)),))
        for slug in ("c1", "c2", "s2"):
            self.assertEqual(self._floor(probe, slug).qualifying, ())
        self.assertIsNone(probe.trigger_instant)
        later = self._probe(landings, as_of=AS_OF + timedelta(days=3))
        self.assertTrue(self._floor(later, "s1").floor_met)


TRIGGER = at(30, 12)


def prd_endpoint(slug, arm, usd_per_kloc, **rates):
    """A PrdEndpoints carrying the given rates; every count zero."""
    fields = {f.name: 0 for f in dataclasses.fields(e3_model.PrdEndpoints)
              if f.name not in ("prd_slug", "arm", "label")}
    fields.update(usd_per_kloc=usd_per_kloc, attempts_per_task=None, attempts_per_kloc=None,
                  blocking_esc_per_task=None, blocking_esc_per_kloc=None,
                  landed_scope_share=None)
    fields.update(rates)
    return e3_model.PrdEndpoints(prd_slug=slug, arm=arm, **fields)


def endpoints_for(coarse_values, standard_values):
    return tuple(
        [prd_endpoint(f"c{i}", Arm.COARSE, v) for i, v in enumerate(coarse_values, 1)]
        + [prd_endpoint(f"s{j}", Arm.STANDARD, v) for j, v in enumerate(standard_values, 1)])


def probe_with_trigger(trigger, cancelled_close_leaves=()):
    rule_i = e3_model.RuleI(met=False, instant=None, cancelled=cancelled_close_leaves,
                            last_merge=None, operator_fixed=False)
    return e3_model.StopRuleProbe(
        per_prd=(), rule_ii_met=trigger is not None, rule_ii_instant=trigger,
        rule_i=rule_i, trigger_instant=trigger)


class ConfirmatoryTests(unittest.TestCase):
    def setUp(self):
        self.protocol = fx.mini_protocol(n_coarse=4, n_standard=3)

    def _verdict(self, trigger, endpoints):
        return e3_model.confirmatory_verdict(probe_with_trigger(trigger), endpoints,
                                             self.protocol)

    def test_no_trigger_is_descriptive(self):
        verdict = self._verdict(None, None)
        self.assertEqual(verdict.label, "DESCRIPTIVE")
        self.assertIn("stop rule not met", verdict.reason)
        self.assertIsNone(verdict.test)
        self.assertIsNone(verdict.trigger_instant)
        self.assertIsNone(verdict.rejects_null)

    def test_undetermined_rule_i_is_descriptive_without_claiming_the_rule_unmet(self):
        verdict = e3_model.confirmatory_verdict(
            probe_with_trigger(None, cancelled_close_leaves=(1205,)), None, self.protocol)
        self.assertEqual(verdict.label, "DESCRIPTIVE")
        self.assertIn("undetermined", verdict.reason)
        self.assertIn("1205", verdict.reason)
        self.assertNotIn("not met", verdict.reason)
        self.assertIsNone(verdict.test)

    def test_full_split_runs_the_exact_test(self):
        coarse, standard = [8.0, 9.5, 11.0, 30.0], [20.0, 25.0, 40.0]
        verdict = self._verdict(TRIGGER, endpoints_for(coarse, standard))
        self.assertEqual(verdict.label, "CONFIRMATORY")
        self.assertEqual(verdict.split, (4, 3))
        self.assertEqual(verdict.min_p, Fraction(1, 35))
        self.assertEqual(verdict.test, e3_stats.exact_mann_whitney_u(coarse, standard))
        self.assertEqual(verdict.trigger_instant, TRIGGER)
        self.assertEqual(verdict.rejects_null, verdict.test.p <= self.protocol.alpha)

    def test_two_v_two_cannot_reach_alpha(self):
        verdict = self._verdict(TRIGGER, endpoints_for([1.0, 2.0, None, None],
                                                       [3.0, 4.0, None]))
        self.assertEqual(verdict.label, "DESCRIPTIVE")
        self.assertEqual(verdict.split, (2, 2))
        self.assertEqual(verdict.min_p, Fraction(1, 6))
        self.assertIn("1/6", verdict.reason)
        self.assertIsNone(verdict.test)

    def test_three_v_two_passes_the_inclusive_gate(self):
        verdict = self._verdict(TRIGGER, endpoints_for([1.0, 2.0, 3.0, None],
                                                       [4.0, 5.0, None]))
        self.assertEqual(verdict.label, "CONFIRMATORY")
        self.assertEqual(verdict.min_p, Fraction(1, 10))
        self.assertEqual(verdict.test.p, Fraction(1, 10))
        self.assertTrue(verdict.rejects_null)

    def test_prd_without_a_value_does_not_contribute(self):
        verdict = self._verdict(TRIGGER, endpoints_for([1.0, 2.0, 3.0, None],
                                                       [4.0, 5.0, 6.0]))
        self.assertEqual(verdict.split, (3, 3))
        self.assertEqual(verdict.test.n_coarse, 3)


class DescriptiveTests(unittest.TestCase):
    def setUp(self):
        self.protocol = fx.mini_protocol(n_coarse=4, n_standard=2)
        self.endpoints = (
            prd_endpoint("c1", Arm.COARSE, 10.0, attempts_per_task=1.0),
            prd_endpoint("c2", Arm.COARSE, 20.0, attempts_per_task=2.0),
            prd_endpoint("c3", Arm.COARSE, 30.0),
            prd_endpoint("c4", Arm.COARSE, None),
            prd_endpoint("s1", Arm.STANDARD, 40.0),
            prd_endpoint("s2", Arm.STANDARD, 50.0),
        )

    def test_arm_medians_with_seeded_bootstrap_intervals(self):
        first = e3_model.descriptives(self.endpoints, self.protocol)
        second = e3_model.descriptives(self.endpoints, self.protocol)
        self.assertEqual(first, second)
        coarse = first.summary("usd_per_kloc", Arm.COARSE)
        self.assertEqual((coarse.n_prds, coarse.median), (3, 20.0))
        low, high = coarse.interval
        self.assertTrue(10.0 <= low <= 20.0 <= high <= 30.0)
        self.assertEqual(first.summary("usd_per_kloc", Arm.STANDARD).median, 45.0)
        self.assertEqual(first.summary("attempts_per_task", Arm.COARSE).median, 1.5)
        empty = first.summary("attempts_per_task", Arm.STANDARD)
        self.assertEqual(empty.n_prds, 0)
        self.assertIsNone(empty.median)
        self.assertIsNone(empty.interval)

    def test_every_metric_is_summarised_for_both_arms(self):
        summaries = e3_model.descriptives(self.endpoints, self.protocol).summaries
        self.assertEqual(
            {(s.metric, s.arm) for s in summaries},
            {(metric, arm) for metric in ("usd_per_kloc", "attempts_per_task",
                                          "attempts_per_kloc", "blocking_esc_per_task",
                                          "blocking_esc_per_kloc") for arm in Arm})

    def test_stratum_b_pair_side_by_side(self):
        pair = e3_model.descriptives(self.endpoints, self.protocol).stratum_b
        self.assertEqual([e.prd_slug for e in pair], ["c1", "s1"])

    def test_era_split_pools_landed_eligible_tasks(self):
        scenario = EndpointScenario()
        cells = {(c.arm, c.era): c
                 for c in e3_model.era_split(scenario.outcomes())}
        self.assertEqual(set(cells), {(arm, era) for arm in Arm
                                      for era in ("pre", "post", "straddle")})
        straddle = cells[(Arm.COARSE, "straddle")]
        self.assertEqual((straddle.n_tasks, straddle.usd_per_kloc, straddle.attempts_per_task,
                          straddle.blocking_esc_per_task), (1, 20.0, 2.0, 2.0))
        post = cells[(Arm.COARSE, "post")]
        self.assertEqual((post.n_tasks, post.usd_per_kloc, post.attempts_per_task), (1, 20.0, 0.0))
        standard_post = cells[(Arm.STANDARD, "post")]
        self.assertEqual((standard_post.n_tasks, standard_post.attempts_per_task), (1, 1.0))
        empty = cells[(Arm.COARSE, "pre")]
        self.assertEqual(empty.n_tasks, 0)
        self.assertIsNone(empty.usd_per_kloc)

    def test_every_descriptive_object_is_labelled(self):
        result = e3_model.descriptives(self.endpoints, self.protocol)
        self.assertEqual(result.label, "DESCRIPTIVE")
        self.assertEqual({s.label for s in result.summaries}, {"DESCRIPTIVE"})
        self.assertEqual({e.label for e in result.stratum_b}, {"DESCRIPTIVE"})
        scenario = EndpointScenario()
        self.assertEqual({c.label for c in e3_model.era_split(scenario.outcomes())},
                         {"DESCRIPTIVE"})


CLI_AS_OF = "2026-10-10T20:00:00Z"
MINI_4V3 = fx.mini_protocol(n_coarse=4, n_standard=3)
FLOOR_UNMET = [(1102, at(27, 20), 512, 31.05), (1101, at(12, 7), 2425, 107.58),
               (103, at(26, 9), 10, 1.0)]
MINI_CLOSE_LEAVES = tuple(prd.close_leaf for prd in MINI_4V3.prds)
STUB_URL = "http://stub.invalid/mcp"
FLOOR_ORDER = [101, 102, 201, 202, 301, 302, 401, 402, 1102, 1103, 1201, 1202, 1301, 1302]


def floor_cost(task_id):
    """Coarse PRD c<i> costs i per task, standard s<j> costs 4 + j: every
    coarse $/kLOC sits below every standard one at the trigger."""
    prd_index = task_id // 100
    return float(prd_index if task_id < 1000 else prd_index - 10 + 4)


FLOOR_MET = [(task_id, at(10 + n, 0), 100, floor_cost(task_id))
             for n, task_id in enumerate(FLOOR_ORDER)]
POST_TRIGGER = (1303, at(25, 0), 10000, 0.0)


def run_cli(argv, protocol=MINI_4V3, open_task_store=e3_sources.open_fused_memory):
    stdout, stderr = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
        code = e3_analysis.main(argv, protocol=protocol, open_task_store=open_task_store)
    return code, stdout.getvalue(), stderr.getvalue()


def source_argv(inputs, out_dir, as_of=CLI_AS_OF, runs_db=None):
    return ["--runs-db", str(runs_db or inputs["runs_db"]),
            "--repo", str(inputs["repo"]), "--ref", "main",
            "--as-of", as_of, "--out-dir", str(out_dir)]


def cli_argv(inputs, out_dir, as_of=CLI_AS_OF, runs_db=None):
    """A replay of the committed-style snapshot file."""
    return ["--tasks-snapshot", str(inputs["snapshot"])] + source_argv(inputs, out_dir, as_of,
                                                                       runs_db)


def live_argv(inputs, out_dir):
    """A live read of the task store at STUB_URL."""
    return ["--fused-memory-url", STUB_URL] + source_argv(inputs, out_dir)


def section(report, title):
    """The lines of report under the `== title` heading, up to the next one."""
    lines = report.split("\n")
    start = next(n for n, line in enumerate(lines) if line.startswith(f"== {title}"))
    end = next((n for n in range(start + 1, len(lines)) if lines[n].startswith("== ")),
               len(lines))
    return "\n".join(lines[start:end])


def labels(document):
    """Every 'label' value anywhere in a JSON document."""
    if isinstance(document, dict):
        found = [document["label"]] if "label" in document else []
        return found + [label for value in document.values() for label in labels(value)]
    if isinstance(document, list):
        return [label for value in document for label in labels(value)]
    return []


class CliTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        root = Path(cls.tmp.name)
        cls.unmet = fx.build_cli_inputs(root / "unmet", MINI_4V3, FLOOR_UNMET)
        cls.drift = fx.build_cli_inputs(root / "drift", MINI_4V3, FLOOR_UNMET,
                                        extra_starts=[(1201, fx.MINI_FREEZE_CUTOFF
                                                       - timedelta(hours=2))])
        cls.met = fx.build_cli_inputs(root / "met", MINI_4V3, FLOOR_MET + [POST_TRIGGER])
        cls.cancelled = fx.build_cli_inputs(
            root / "cancelled", MINI_4V3, FLOOR_MET,
            statuses={leaf: "cancelled" for leaf in MINI_CLOSE_LEAVES})
        cls.unmet_run = run_cli(cli_argv(cls.unmet, root / "unmet-out"))
        cls.unmet_out = root / "unmet-out"
        cls.met_run = run_cli(cli_argv(cls.met, root / "met-out"))
        cls.met_out = root / "met-out"
        cls.cancelled_run = run_cli(cli_argv(cls.cancelled, root / "cancelled-out"))
        cls.cancelled_out = root / "cancelled-out"

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def _out_dir(self, name):
        return Path(self.tmp.name) / name

    def test_floor_unmet_run_succeeds_and_prints_the_probe(self):
        code, stdout, stderr = self.unmet_run
        self.assertEqual(code, 0, stderr)
        probe = section(stdout, "STOP-RULE PROBE")
        for prd in MINI_4V3.prds:
            expected = "1/2" if prd.slug == "s1" else "0/2"
            self.assertRegex(
                probe, rf"(?m)^\s+{prd.arm.value}\s+{prd.slug}\s+{expected} qualifying")
        self.assertIn("#1102", probe)
        self.assertIn("trigger instant: none", probe)

    def test_floor_unmet_writes_report_and_endpoints(self):
        self.assertTrue((self.unmet_out / "report.txt").is_file())
        self.assertFalse((self.unmet_out / "tasks-snapshot.json").exists())
        document = json.loads((self.unmet_out / "endpoints.json").read_text())
        self.assertEqual(document["verdict"]["label"], "DESCRIPTIVE")
        found = labels(document)
        self.assertTrue(found)
        self.assertEqual(set(found), {"DESCRIPTIVE"})
        self.assertEqual((self.unmet_out / "report.txt").read_text(), self.unmet_run[1])

    def test_report_names_censoring_counts_and_exclusions(self):
        report = self.unmet_run[1]
        censoring = section(report, "CENSORING")
        for name in ("freeze-window excluded", "right-censored", "drain-censored attempts"):
            self.assertIn(name, censoring)
        self.assertIn("#1101", section(report, "EXCLUDED TASKS"))
        self.assertIn("#150 carve-out (§7.1): status pending", section(report, "EXCLUDED TASKS"))
        self.assertIn("#1102", section(report, "SENSITIVITY"))
        self.assertIn("DESCRIPTIVE", report.split("\n")[1])

    def test_floor_met_runs_one_look_frozen_at_the_trigger(self):
        code, stdout, stderr = self.met_run
        self.assertEqual(code, 0, stderr)
        document = json.loads((self.met_out / "endpoints.json").read_text())
        verdict = document["verdict"]
        self.assertEqual(verdict["label"], "CONFIRMATORY")
        self.assertEqual(verdict["trigger_instant"], "2026-09-23T00:00:00Z")
        self.assertEqual(document["stop_rule_probe"]["trigger_instant"], "2026-09-23T00:00:00Z")
        per_prd = {}
        for task_id, _, loc, cost in FLOOR_MET:
            slug = "c" + str(task_id // 100) if task_id < 1000 else "s" + str(task_id // 100 - 10)
            total_cost, total_loc = per_prd.get(slug, (0.0, 0))
            per_prd[slug] = (total_cost + cost, total_loc + loc)
        values = {slug: 1000 * c / l for slug, (c, l) in per_prd.items()}
        expected = e3_stats.exact_mann_whitney_u(
            [values[f"c{i}"] for i in range(1, 5)], [values[f"s{j}"] for j in range(1, 4)])
        self.assertEqual(verdict["test"]["u"]["exact"], str(expected.u))
        self.assertEqual(verdict["test"]["p"]["exact"], str(expected.p))
        self.assertEqual(document["current"]["kind"], "post-trigger descriptive update")
        self.assertIn("POST-TRIGGER DESCRIPTIVE UPDATE", stdout)

    def test_freeze_window_drift_fails_loudly_and_writes_nothing(self):
        out_dir = self._out_dir("drift-out")
        code, _, stderr = run_cli(cli_argv(self.drift, out_dir))
        self.assertEqual(code, 1)
        self.assertIn("freeze-window set", stderr)
        self.assertIn("{1101}", stderr)
        self.assertIn("{1101, 1201}", stderr)
        self.assertFalse(out_dir.exists() and any(out_dir.iterdir()))

    def test_missing_runs_db_is_an_unreadable_source(self):
        missing = Path(self.tmp.name) / "nowhere" / "runs.db"
        code, _, stderr = run_cli(cli_argv(self.unmet, self._out_dir("missing-out"),
                                           runs_db=missing))
        self.assertEqual(code, 3)
        self.assertIn(str(missing), stderr)

    def test_as_of_after_the_snapshot_is_refused(self):
        code, _, stderr = run_cli(cli_argv(self.unmet, self._out_dir("late-out"),
                                           as_of="2026-10-11T00:00:00Z"))
        self.assertEqual(code, 2)
        self.assertIn("as-of", stderr)

    def test_existing_record_is_never_overwritten(self):
        out_dir = self._out_dir("kept-out")
        out_dir.mkdir()
        (out_dir / "report.txt").write_text("committed record\n")
        code, _, _ = run_cli(cli_argv(self.unmet, out_dir))
        self.assertEqual(code, 2)
        self.assertEqual((out_dir / "report.txt").read_text(), "committed record\n")
        other = self._out_dir("other-out")
        other.mkdir()
        (other / "notes.txt").write_text("not a record\n")
        code, _, _ = run_cli(cli_argv(self.unmet, other))
        self.assertEqual(code, 2)
        self.assertEqual(sorted(p.name for p in other.iterdir()), ["notes.txt"])

    def test_endpoint5_reports_closed_prds_and_waits_for_the_rest(self):
        endpoint5 = section(self.unmet_run[1], "ENDPOINT 5")
        self.assertRegex(endpoint5, r"c1\s+close leaf #103: closed 2026-09-26T09:00:00Z; "
                                    r"window ends 2026-10-26T09:00:00Z")
        for prd in MINI_4V3.prds[1:]:
            self.assertRegex(endpoint5, rf"{prd.slug}\s+close leaf #{prd.close_leaf}: not closed")

    def test_undetermined_rule_i_withholds_the_look_and_says_why(self):
        code, stdout, stderr = self.cancelled_run
        self.assertEqual(code, 0, stderr)
        self.assertIn("rule (i) every close leaf terminal: undetermined",
                      section(stdout, "STOP-RULE PROBE"))
        label = stdout.split("\n")[1]
        self.assertIn("DESCRIPTIVE", label)
        self.assertIn("undetermined", label)
        self.assertNotIn("not met", label)
        document = json.loads((self.cancelled_out / "endpoints.json").read_text())
        self.assertTrue(document["stop_rule_probe"]["rule_ii_met"])
        self.assertIsNone(document["stop_rule_probe"]["trigger_instant"])
        self.assertEqual(document["stop_rule_probe"]["rule_i"]["cancelled"],
                         sorted(MINI_CLOSE_LEAVES))

    def test_operator_rule_i_instant_runs_the_one_look_frozen_there(self):
        out_dir = self._out_dir("operator-out")
        code, stdout, stderr = run_cli(cli_argv(self.cancelled, out_dir)
                                       + ["--rule-i-instant", "2026-09-20T00:00:00Z"])
        self.assertEqual(code, 0, stderr)
        self.assertIn("operator-fixed", section(stdout, "STOP-RULE PROBE"))
        document = json.loads((out_dir / "endpoints.json").read_text())
        self.assertTrue(document["stop_rule_probe"]["rule_i"]["operator_fixed"])
        verdict = document["verdict"]
        self.assertEqual(verdict["label"], "CONFIRMATORY")
        self.assertEqual(verdict["trigger_instant"], "2026-09-20T00:00:00Z")
        self.assertEqual(verdict["split"], [4, 2])

    def test_rule_i_instant_later_than_as_of_is_refused(self):
        out_dir = self._out_dir("operator-late-out")
        code, _, stderr = run_cli(cli_argv(self.cancelled, out_dir)
                                  + ["--rule-i-instant", "2026-10-10T21:00:00Z"])
        self.assertEqual(code, 2)
        self.assertIn("--rule-i-instant", stderr)
        self.assertFalse(out_dir.exists())

    def test_rule_i_instant_is_refused_unless_rule_i_is_undetermined(self):
        out_dir = self._out_dir("operator-open-out")
        code, _, stderr = run_cli(cli_argv(self.unmet, out_dir)
                                  + ["--rule-i-instant", "2026-10-01T00:00:00Z"])
        self.assertEqual(code, 2)
        self.assertIn("refused", stderr)
        self.assertFalse(out_dir.exists())

    def test_live_task_store_read_is_persisted_and_replays_identically(self):
        call_tool, calls = fx.fake_task_store(self.unmet["payload"])
        opened = []

        def open_task_store(url):
            opened.append(url)
            return call_tool

        live_out = self._out_dir("live-out")
        code, stdout, stderr = run_cli(live_argv(self.unmet, live_out),
                                       open_task_store=open_task_store)
        self.assertEqual(code, 0, stderr)
        self.assertEqual(opened, [STUB_URL])
        self.assertEqual(calls[0][0], "get_statuses")
        self.assertIn(e3_sources.MCP_SOURCE, stdout.split("\n")[3])
        written = e3_sources.read_task_snapshot(live_out / "tasks-snapshot.json")
        self.assertEqual(written.tasks, e3_sources.read_task_snapshot(self.unmet["snapshot"]).tasks)
        self.assertGreaterEqual(written.fetched_at, datetime.fromisoformat(CLI_AS_OF))
        replay_out = self._out_dir("live-replay-out")
        replayed = {**self.unmet, "snapshot": live_out / "tasks-snapshot.json"}
        code, _, stderr = run_cli(cli_argv(replayed, replay_out))
        self.assertEqual(code, 0, stderr)
        for name in ("report.txt", "endpoints.json"):
            self.assertEqual((replay_out / name).read_text(), (live_out / name).read_text(), name)
        self.assertFalse((replay_out / "tasks-snapshot.json").exists())

    def test_malformed_task_store_reply_is_an_unreadable_source(self):
        out_dir = self._out_dir("malformed-store-out")
        code, _, stderr = run_cli(live_argv(self.unmet, out_dir),
                                  open_task_store=lambda url: lambda name, arguments: {})
        self.assertEqual(code, 3)
        self.assertIn(STUB_URL, stderr)
        self.assertFalse(out_dir.exists())

    def test_malformed_runs_db_row_is_an_unreadable_source(self):
        corrupt = Path(self.tmp.name) / "corrupt-runs.db"
        shutil.copy(self.unmet["runs_db"], corrupt)
        with contextlib.closing(sqlite3.connect(corrupt)) as connection:
            connection.execute("UPDATE events SET data = '{not json'"
                               " WHERE event_type = 'task_completed'")
            connection.commit()
        code, _, stderr = run_cli(cli_argv(self.unmet, self._out_dir("corrupt-out"),
                                           runs_db=corrupt))
        self.assertEqual(code, 3)
        self.assertIn(str(corrupt), stderr)

    def test_unwritable_record_exits_4_and_prints_nothing(self):
        blocker = Path(self.tmp.name) / "a-file"
        blocker.write_text("not a directory\n")
        code, stdout, stderr = run_cli(cli_argv(self.unmet, blocker / "out"))
        self.assertEqual(code, 4)
        self.assertIn("RECORD NOT WRITTEN", stderr)
        self.assertEqual(stdout, "")
        self.assertEqual(blocker.read_text(), "not a directory\n")

    def test_default_out_dir_is_inside_the_analysis_results(self):
        as_of = datetime(2026, 10, 10, 20, 0, tzinfo=UTC)
        out_dir = e3_analysis.default_out_dir(as_of)
        self.assertEqual(out_dir, ANALYSIS_DIR / "results" / "20261010T200000Z")
        self.assertNotIn("tmp", out_dir.resolve().parts)


class CommittedRecordTests(unittest.TestCase):
    """The hermetic half of replaying a committed record: its task snapshot
    still reads and attributes under the registered protocol. The rest needs
    the live runs.db and main ref (results/README.md has the command)."""

    def test_every_committed_snapshot_still_reads_and_attributes(self):
        snapshots = sorted((ANALYSIS_DIR / "results").glob("*/tasks-snapshot.json"))
        self.assertTrue(snapshots)
        coarse = sum(len(prd.coalesced | prd.reused) for prd in E3.prds_in(Arm.COARSE))
        standard = sum(prd.stamped_count for prd in E3.prds_in(Arm.STANDARD))
        for path in snapshots:
            with self.subTest(record=path.parent.name):
                arm_tasks = e3_model.attribute(e3_sources.read_task_snapshot(path), E3)
                self.assertEqual(sum(1 for t in arm_tasks if t.arm is Arm.COARSE), coarse)
                self.assertEqual(sum(1 for t in arm_tasks if t.arm is Arm.STANDARD), standard)


if __name__ == "__main__":
    unittest.main()
