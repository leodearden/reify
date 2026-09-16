#!/usr/bin/env python3
"""
test_flake_density_report.py — stdlib unittest for scripts/flake-density-report.py.

The first infra test authored in Python under the bash-to-Python migration
policy (docs/notes/infra-test-bash-to-python-migration-policy.md), and the
worked example that document points at.

WHAT RUNS THIS: not the gate directly. run_all.sh discovers `test_*.sh` only
(header :22-24, glob repeated at :1347/:1456/:2013), so the discovered member
is the thin wrapper tests/infra/test_flake_density_report.sh, which invokes
this file. A bare .py here would be silently never run.

IMPORT IDIOM: scripts/ is prepended to sys.path so a sibling import inside the
tool resolves without installation — the scripts/test_sn_gate.py:1-13 idiom.
The tool's own filename is hyphenated (repo script convention) and so is not a
legal module name, so it is loaded by path via importlib rather than by a bare
`import`.

Fixture ledgers are written under tempfile dirs; no fixture file is committed.
"""

import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPTS_DIR = REPO_ROOT / "scripts"
TOOL_PATH = SCRIPTS_DIR / "flake-density-report.py"

sys.path.insert(0, str(SCRIPTS_DIR))


def _load_tool():
    spec = importlib.util.spec_from_file_location("flake_density_report", TOOL_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {TOOL_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


fdr = _load_tool()


def _row(test, ts="2026-07-19T22:22:50Z", role="merge", task="unknown",
         branch="HEAD", run_id="r1"):
    """One ledger record in the producer's schema (run_all.sh:711-715)."""
    return {"ts": ts, "test": test, "role": role, "task": task,
            "branch": branch, "run_id": run_id}


class LedgerFixture(unittest.TestCase):
    """Writes JSON Lines ledgers into a per-test tempdir."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmpdir = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)

    def write_ledger(self, rows, name="flaky-ledger.jsonl"):
        """rows: dicts are JSON-encoded; str entries are written verbatim."""
        path = self.tmpdir / name
        with path.open("w") as fh:
            for row in rows:
                fh.write(row if isinstance(row, str) else json.dumps(row))
                fh.write("\n")
        return path

    def run_cli(self, *args, env=None):
        cli_env = dict(os.environ)
        cli_env.pop("REIFY_RUN_ALL_FLAKY_LEDGER", None)
        if env:
            cli_env.update(env)
        return subprocess.run(
            [sys.executable, str(TOOL_PATH), *args],
            capture_output=True, text=True, env=cli_env,
        )


class TestRanking(LedgerFixture):

    def test_per_member_counts_ranked_descending(self):
        ledger = self.write_ledger(
            [_row("test_a.sh")] * 3 + [_row("test_b.sh")] * 5 + [_row("test_c.sh")]
        )
        report = fdr.build_report(fdr.read_ledger(ledger).rows)
        self.assertEqual(
            [(m.test, m.flakes) for m in report.members],
            [("test_b.sh", 5), ("test_a.sh", 3), ("test_c.sh", 1)],
        )

    def test_ties_broken_deterministically_by_name(self):
        ledger = self.write_ledger(
            [_row("test_zebra.sh")] * 2 + [_row("test_alpha.sh")] * 2
            + [_row("test_mid.sh")] * 2
        )
        report = fdr.build_report(fdr.read_ledger(ledger).rows)
        self.assertEqual(
            [m.test for m in report.members],
            ["test_alpha.sh", "test_mid.sh", "test_zebra.sh"],
        )

    def test_top_truncates_the_ranking(self):
        ledger = self.write_ledger(
            [_row("test_a.sh")] * 4 + [_row("test_b.sh")] * 3
            + [_row("test_c.sh")] * 2 + [_row("test_d.sh")]
        )
        rows = fdr.read_ledger(ledger).rows
        self.assertEqual(len(fdr.build_report(rows).members), 4)
        truncated = fdr.build_report(rows, top=2)
        self.assertEqual([m.test for m in truncated.members],
                         ["test_a.sh", "test_b.sh"])
        self.assertEqual(truncated.truncated_to, 2)

    def test_share_of_recorded_flakes(self):
        ledger = self.write_ledger([_row("test_a.sh")] * 3 + [_row("test_b.sh")])
        report = fdr.build_report(fdr.read_ledger(ledger).rows)
        shares = {m.test: m.share for m in report.members}
        self.assertAlmostEqual(shares["test_a.sh"], 0.75)
        self.assertAlmostEqual(shares["test_b.sh"], 0.25)
        self.assertAlmostEqual(sum(shares.values()), 1.0)


class TestDenominators(LedgerFixture):

    def test_distinct_run_ids_are_counted_separately_from_rows(self):
        """Rows are not runs: one run that flaked twice is ONE run_id."""
        ledger = self.write_ledger([
            _row("test_a.sh", run_id="r1"),
            _row("test_b.sh", run_id="r1"),
            _row("test_a.sh", run_id="r2"),
        ])
        report = fdr.build_report(fdr.read_ledger(ledger).rows)
        self.assertEqual(report.records, 3)
        self.assertEqual(report.distinct_runs, 2)
        by_test = {m.test: m for m in report.members}
        self.assertEqual(by_test["test_a.sh"].flakes, 2)
        self.assertEqual(by_test["test_a.sh"].distinct_runs, 2)
        self.assertEqual(by_test["test_b.sh"].distinct_runs, 1)

    def test_no_density_without_total_runs(self):
        """run_all.sh:729-731 — a clean run writes NO line, so the ledger alone
        cannot support a flakes-per-gate density."""
        ledger = self.write_ledger([_row("test_a.sh")] * 3)
        report = fdr.build_report(fdr.read_ledger(ledger).rows)
        self.assertIsNone(report.total_runs)
        self.assertIsNone(report.density)
        self.assertTrue(all(m.density is None for m in report.members))

    def test_density_emitted_with_total_runs(self):
        ledger = self.write_ledger(
            [_row("test_a.sh")] * 3 + [_row("test_b.sh")]
        )
        report = fdr.build_report(fdr.read_ledger(ledger).rows, total_runs=200)
        self.assertEqual(report.total_runs, 200)
        self.assertAlmostEqual(report.density, 4 / 200)
        by_test = {m.test: m for m in report.members}
        self.assertAlmostEqual(by_test["test_a.sh"].density, 3 / 200)

    def test_text_report_always_carries_the_denominator_caveat(self):
        ledger = self.write_ledger([_row("test_a.sh")])
        text = fdr.format_text(fdr.build_report(fdr.read_ledger(ledger).rows))
        self.assertIn(fdr.DENOMINATOR_CAVEAT, text)

    def test_zero_total_runs_is_rejected_not_divided_by(self):
        ledger = self.write_ledger([_row("test_a.sh")])
        result = self.run_cli("--ledger", str(ledger), "--total-runs", "0")
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("Traceback", result.stderr)


class TestGroupingKeys(LedgerFixture):

    def test_task_and_branch_do_not_split_a_members_count(self):
        """`task`/`branch` are "unknown"/"HEAD" in 167/167 real records (the
        merge lane runs detached), so they are not grouping keys."""
        ledger = self.write_ledger([
            _row("test_a.sh", task="unknown", branch="HEAD", run_id="r1"),
            _row("test_a.sh", task="7430", branch="task/7430", run_id="r2"),
            _row("test_a.sh", task="unknown", branch="main", run_id="r3"),
        ])
        report = fdr.build_report(fdr.read_ledger(ledger).rows)
        self.assertEqual(len(report.members), 1)
        self.assertEqual(report.members[0].flakes, 3)

    def test_role_is_a_secondary_grouping_key(self):
        ledger = self.write_ledger([
            _row("test_a.sh", role="merge"),
            _row("test_a.sh", role="merge"),
            _row("test_a.sh", role="background"),
        ])
        report = fdr.build_report(fdr.read_ledger(ledger).rows)
        self.assertEqual(report.members[0].roles,
                         {"background": 1, "merge": 2})


class TestTolerance(LedgerFixture):

    def test_malformed_lines_are_skipped_with_a_warning(self):
        """Mirrors dark-factory chronic_flake.py:150,153 — the two readers of
        the same file must agree on what a malformed record means."""
        ledger = self.write_ledger([
            _row("test_a.sh"),
            "{not json at all",
            "[1, 2, 3]",
            "",
            _row("test_b.sh"),
        ])
        result = fdr.read_ledger(ledger)
        self.assertEqual(len(result.rows), 2)
        self.assertEqual(len(result.warnings), 2)
        report = fdr.build_report(result.rows)
        self.assertEqual(report.records, 2)

    def test_malformed_lines_do_not_abort_the_cli(self):
        ledger = self.write_ledger([_row("test_a.sh"), "{not json at all"])
        result = self.run_cli("--ledger", str(ledger))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("Traceback", result.stderr)

    def test_rows_without_a_test_field_are_skipped(self):
        ledger = self.write_ledger([
            _row("test_a.sh"),
            {"ts": "2026-07-19T22:22:50Z", "role": "merge", "run_id": "r9"},
        ])
        result = fdr.read_ledger(ledger)
        self.assertEqual(len(result.rows), 1)
        self.assertEqual(len(result.warnings), 1)

    def test_empty_ledger_reports_zero_members(self):
        ledger = self.write_ledger([])
        report = fdr.build_report(fdr.read_ledger(ledger).rows)
        self.assertEqual(report.records, 0)
        self.assertEqual(report.members, [])
        self.assertIn(fdr.DENOMINATOR_CAVEAT, fdr.format_text(report))

    def test_missing_ledger_exits_cleanly_with_a_diagnostic(self):
        missing = self.tmpdir / "does-not-exist.jsonl"
        result = self.run_cli("--ledger", str(missing))
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("Traceback", result.stderr)
        self.assertIn(str(missing), result.stderr)


class TestWindowing(LedgerFixture):

    def test_since_drops_records_before_the_window(self):
        ledger = self.write_ledger([
            _row("test_old.sh", ts="2026-07-01T00:00:00Z"),
            _row("test_new.sh", ts="2026-09-01T00:00:00Z"),
        ])
        rows = fdr.read_ledger(ledger).rows
        report = fdr.build_report(rows, since="2026-08-01T00:00:00Z")
        self.assertEqual([m.test for m in report.members], ["test_new.sh"])
        self.assertEqual(report.records, 1)

    def test_since_excludes_undated_records_and_says_so(self):
        ledger = self.write_ledger([
            _row("test_new.sh", ts="2026-09-01T00:00:00Z"),
            {"test": "test_undated.sh", "role": "merge", "run_id": "r5"},
        ])
        rows = fdr.read_ledger(ledger).rows
        self.assertEqual(len(rows), 2)
        report = fdr.build_report(rows, since="2026-08-01T00:00:00Z")
        self.assertEqual([m.test for m in report.members], ["test_new.sh"])
        self.assertEqual(report.undated_excluded, 1)

    def test_undated_records_are_kept_when_no_window_is_given(self):
        ledger = self.write_ledger([
            {"test": "test_undated.sh", "role": "merge", "run_id": "r5"},
        ])
        report = fdr.build_report(fdr.read_ledger(ledger).rows)
        self.assertEqual(report.records, 1)
        self.assertEqual(report.undated_excluded, 0)


class TestLedgerPathResolution(LedgerFixture):

    def test_env_var_is_the_default_source(self):
        """Same precedence as the producer, run_all.sh:1236."""
        ledger = self.write_ledger([_row("test_from_env.sh")])
        result = self.run_cli(
            "--json", env={"REIFY_RUN_ALL_FLAKY_LEDGER": str(ledger)})
        self.assertEqual(result.returncode, 0, result.stderr)
        payload = json.loads(result.stdout)
        self.assertEqual(payload["ledger_path"], str(ledger))
        self.assertEqual(payload["members"][0]["test"], "test_from_env.sh")

    def test_explicit_ledger_flag_beats_the_env_var(self):
        chosen = self.write_ledger([_row("test_chosen.sh")], name="chosen.jsonl")
        ignored = self.write_ledger([_row("test_ignored.sh")], name="ignored.jsonl")
        result = self.run_cli(
            "--ledger", str(chosen), "--json",
            env={"REIFY_RUN_ALL_FLAKY_LEDGER": str(ignored)})
        self.assertEqual(result.returncode, 0, result.stderr)
        payload = json.loads(result.stdout)
        self.assertEqual(payload["members"][0]["test"], "test_chosen.sh")

    def test_repo_relative_path_is_the_final_fallback(self):
        self.assertEqual(
            fdr.resolve_ledger_path(explicit=None, env_value=None),
            REPO_ROOT / "data" / "verify-logs" / "flaky-ledger.jsonl",
        )


class TestOutputModes(LedgerFixture):

    def test_json_mode_emits_parseable_json(self):
        ledger = self.write_ledger([_row("test_a.sh")] * 2 + [_row("test_b.sh")])
        result = self.run_cli("--ledger", str(ledger), "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        payload = json.loads(result.stdout)
        self.assertEqual(payload["records"], 3)
        self.assertEqual(payload["distinct_runs"], 1)
        self.assertEqual(
            [(m["test"], m["flakes"]) for m in payload["members"]],
            [("test_a.sh", 2), ("test_b.sh", 1)],
        )
        self.assertIsNone(payload["density"])

    def test_default_mode_emits_human_text_not_json(self):
        ledger = self.write_ledger([_row("test_a.sh")] * 2)
        result = self.run_cli("--ledger", str(ledger))
        self.assertEqual(result.returncode, 0, result.stderr)
        with self.assertRaises(json.JSONDecodeError):
            json.loads(result.stdout)
        self.assertIn("test_a.sh", result.stdout)
        self.assertIn(fdr.DENOMINATOR_CAVEAT, result.stdout)

    def test_help_exits_zero(self):
        result = self.run_cli("--help")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--total-runs", result.stdout)


if __name__ == "__main__":
    unittest.main()
