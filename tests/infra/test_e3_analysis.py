#!/usr/bin/env python3
"""
test_e3_analysis.py — stdlib unittest for the registered E3 protocol and the
e3_analysis CLI of the pre-registered E3 §7 analysis
(docs/prds/v0_6/e3-granularity/analysis/).

WHAT RUNS THIS: not the gate directly. run_all.sh discovers `test_*.sh` only,
so the discovered member is the thin wrapper tests/infra/test_e3_analysis.sh,
which runs this file beside its two siblings (test_e3_sources.py,
test_e3_model.py, test_e3_analysis.py). A .py it did not name would be
silently never run.

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
import sys
import tempfile
import unittest
from unittest import mock
from datetime import datetime, timedelta
from fractions import Fraction
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
INFRA_DIR = REPO_ROOT / "tests" / "infra"
ANALYSIS_DIR = REPO_ROOT / "docs" / "prds" / "v0_6" / "e3-granularity" / "analysis"

sys.path.insert(0, str(INFRA_DIR))
sys.path.insert(0, str(ANALYSIS_DIR))

import e3_analysis_fixtures as fx  # noqa: E402  (after sys.path manipulation)
import e3_analysis  # noqa: E402
import e3_model  # noqa: E402
import e3_sources  # noqa: E402
import e3_stats  # noqa: E402
from e3_protocol import E3, UTC, Arm, ProtocolViolation  # noqa: E402

at = fx.at


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

    def test_an_internal_failure_is_a_defect_exit_5_not_a_protocol_violation(self):
        out_dir = self._out_dir("defect-out")
        with mock.patch.object(e3_model, "era_split", side_effect=KeyError("era-split bug")):
            code, stdout, stderr = run_cli(cli_argv(self.unmet, out_dir))
        self.assertEqual(code, 5)
        self.assertIn("E3 ANALYSIS DEFECT", stderr)
        self.assertIn("era-split bug", stderr)
        self.assertNotIn("PROTOCOL VIOLATION", stderr)
        self.assertEqual(stdout, "")
        self.assertFalse(out_dir.exists())

    def test_an_instant_without_an_offset_or_unparseable_is_a_usage_error(self):
        for as_of in ("2026-10-10T20:00:00", "yesterday"):
            with self.subTest(as_of=as_of):
                out_dir = self._out_dir("bad-as-of-out")
                code, _, stderr = run_cli(cli_argv(self.unmet, out_dir, as_of=as_of))
                self.assertEqual(code, 2)
                self.assertIn("--as-of", stderr)
                self.assertFalse(out_dir.exists())

    def test_live_read_under_an_unregistered_project_root_is_a_usage_error(self):
        opened = []
        out_dir = self._out_dir("elsewhere-live-out")
        code, _, stderr = run_cli(live_argv(self.unmet, out_dir) + ["--project-root", "/elsewhere"],
                                  open_task_store=opened.append)
        self.assertEqual(code, 2)
        self.assertIn("--project-root /elsewhere", stderr)
        self.assertNotIn("PROTOCOL VIOLATION", stderr)
        self.assertEqual(opened, [])
        self.assertFalse(out_dir.exists())

    def test_replay_takes_project_root_as_a_path_default_only(self):
        out_dir = self._out_dir("elsewhere-replay-out")
        code, _, stderr = run_cli(cli_argv(self.unmet, out_dir) + ["--project-root", "/elsewhere"])
        self.assertEqual(code, 0, stderr)
        self.assertEqual((out_dir / "report.txt").read_text(), self.unmet_run[1])

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
