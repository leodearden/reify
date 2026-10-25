#!/usr/bin/env python3
"""
test_e3_sources.py — stdlib unittest for e3_sources, the read-only boundary
readers of the pre-registered E3 §7 analysis
(docs/prds/v0_6/e3-granularity/analysis/): the task snapshot, runs.db and the
git merge reader.

WHAT RUNS THIS: not the gate directly. run_all.sh discovers `test_*.sh` only,
so the discovered member is the thin wrapper tests/infra/test_e3_analysis.sh,
which runs this file beside its two siblings (test_e3_sources.py,
test_e3_model.py, test_e3_analysis.py). A .py it did not name would be
silently never run.

IMPORT IDIOM: tests/infra/ (for the hermetic fixture builders in
e3_analysis_fixtures.py) and the analysis directory are prepended to sys.path,
so the modules import by their plain names without installation.

Every fixture lives under a tempfile dir: no network, no shared state, no
live runs.db, task store or main ref.
"""

import json
import sqlite3
import sys
import tempfile
import unittest
from datetime import datetime, timedelta
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
INFRA_DIR = REPO_ROOT / "tests" / "infra"
ANALYSIS_DIR = REPO_ROOT / "docs" / "prds" / "v0_6" / "e3-granularity" / "analysis"

sys.path.insert(0, str(INFRA_DIR))
sys.path.insert(0, str(ANALYSIS_DIR))

import e3_analysis_fixtures as fx  # noqa: E402  (after sys.path manipulation)
import e3_sources  # noqa: E402
from e3_sources import AttemptOutcome, Severity, TaskStatus  # noqa: E402
from e3_protocol import UTC  # noqa: E402

at = fx.at


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
        self.assertIs(standard.status, TaskStatus.DONE)
        self.assertIs(snapshot.task(1101).status, TaskStatus.PENDING)
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

    def test_fetch_refuses_a_status_outside_the_store_vocabulary(self):
        payload = fx.snapshot_payload(self.protocol, overrides={101: {"status": "superseded"}})
        with self.assertRaises(ValueError) as caught:
            self._fetch(payload)
        self.assertIn("superseded", str(caught.exception))

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
            document = json.loads(path.read_text())
            self.assertEqual(document["schema"], "e3-task-snapshot/1")
            self.assertEqual({t["id"]: t["status"] for t in document["tasks"]}[1102], "done")
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
                         ("r1", at(1, 1), at(1, 2), AttemptOutcome.REQUEUED))
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

    def test_crashed_run_end_is_known_once_the_next_run_starts(self):
        in_crashed_run = self._attempts(1102)[0]
        self.assertEqual(in_crashed_run.run_end_known_at, at(2, 0))

    def test_latest_run_without_completion_is_live(self):
        live = self._attempts(1102)[1]
        self.assertIsNone(live.run_ended_at)
        self.assertIsNone(live.run_end_known_at)

    def test_closed_attempts_carry_their_run_end(self):
        self.assertEqual(self._attempts(1101)[0].run_ended_at, at(1, 10))

    def test_completed_run_end_is_known_when_it_completes(self):
        self.assertEqual(self._attempts(1101)[0].run_end_known_at, at(1, 10))

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
                         (1101, at(1, 4), Severity.BLOCKING, None, "review_issues"))
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

    def test_an_outcome_or_severity_outside_the_vocabulary_is_refused(self):
        run = {"run_id": "r1", "started_at": at(1, 0), "completed_at": at(1, 10)}
        for name, event in (("outcome", fx.completed(1101, "r1", at(1, 2), "vanished")),
                            ("severity", fx.escalated(1101, "r1", at(1, 4), "esc-x",
                                                      severity="dire"))):
            with self.subTest(name):
                path = Path(self.tmp.name) / f"unknown-{name}.db"
                fx.make_runs_db(path, [run], [fx.started(1101, "r1", at(1, 1)), event], [])
                with self.assertRaises(ValueError):
                    e3_sources.read_runs_db(path, {1101})

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

    def test_no_requested_task_merged_reads_no_merge(self):
        self.assertEqual(e3_sources.read_task_merges(self.repo, "main", {999}), ())

    def test_unknown_ref_raises_source_error_with_git_stderr(self):
        with self.assertRaises(e3_sources.SourceError) as caught:
            e3_sources.read_task_merges(self.repo, "no-such-ref", {101})
        self.assertIn("no-such-ref", str(caught.exception))


if __name__ == "__main__":
    unittest.main()
