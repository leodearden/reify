#!/usr/bin/env python3
"""
test_sn_gate.py — stdlib unittest for sn_gate.py pure functions.

Co-located with sn_gate.py in scripts/; the script directory is prepended to
sys.path so `import sn_gate` resolves without installation.
"""

import sys
import os
import unittest

# Ensure scripts/ directory is on the path for `import sn_gate`.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import sn_gate  # noqa: E402  (imported after sys.path manipulation)


from datetime import timezone


class TestScaffold(unittest.TestCase):
    """Minimal sanity-check that the module is importable and the main stubs exist."""

    def test_module_importable(self):
        self.assertIsNotNone(sn_gate)

    def test_required_functions_present(self):
        for fn in [
            "parse_git_log_line",
            "cluster_merges",
            "is_fix_forward",
            "attribute_fix_forward",
            "estimate_s",
            "decide",
            "build_report",
            "summarize_runs_db",
            "main",
        ]:
            self.assertTrue(
                hasattr(sn_gate, fn),
                f"sn_gate is missing function: {fn}",
            )


# ---------------------------------------------------------------------------
# step-1: Tests for parse_git_log_line and cluster_merges (synthetic fixtures)
# ---------------------------------------------------------------------------

class TestParseGitLogLine(unittest.TestCase):
    """Tests for parse_git_log_line using synthetic %h|%cI|%s fixture lines."""

    MERGE_LINE = "abc1234|2026-06-09T11:18:00+00:00|Merge task/4421 into main"
    FIX_LINE = "def5678|2026-06-09T12:00:00+00:00|fix(verify): raise timeout"
    REVERT_LINE = "bbb9999|2026-06-09T13:00:00+00:00|revert: undo bad merge"

    def test_merge_is_flagged_as_merge(self):
        info = sn_gate.parse_git_log_line(self.MERGE_LINE)
        self.assertIsNotNone(info)
        self.assertTrue(info.is_merge, "Expected is_merge=True for 'Merge task/...' subject")

    def test_merge_extracts_task_id(self):
        info = sn_gate.parse_git_log_line(self.MERGE_LINE)
        self.assertIsNotNone(info)
        self.assertEqual(info.task_id, "4421")

    def test_fix_subject_is_not_merge(self):
        info = sn_gate.parse_git_log_line(self.FIX_LINE)
        self.assertIsNotNone(info)
        self.assertFalse(info.is_merge, "Expected is_merge=False for fix(...) subject")
        self.assertIsNone(info.task_id)

    def test_revert_subject_is_not_merge(self):
        info = sn_gate.parse_git_log_line(self.REVERT_LINE)
        self.assertIsNotNone(info)
        self.assertFalse(info.is_merge, "Expected is_merge=False for revert subject")

    def test_timestamp_is_tz_aware_datetime(self):
        info = sn_gate.parse_git_log_line(self.MERGE_LINE)
        self.assertIsNotNone(info)
        from datetime import datetime
        self.assertIsInstance(info.timestamp, datetime)
        self.assertIsNotNone(info.timestamp.tzinfo, "timestamp must be tz-aware")

    def test_timestamp_value_correct(self):
        info = sn_gate.parse_git_log_line(self.MERGE_LINE)
        self.assertIsNotNone(info)
        from datetime import datetime
        expected = datetime(2026, 6, 9, 11, 18, 0, tzinfo=timezone.utc)
        self.assertEqual(info.timestamp, expected)

    def test_sha_extracted(self):
        info = sn_gate.parse_git_log_line(self.MERGE_LINE)
        self.assertIsNotNone(info)
        self.assertEqual(info.sha, "abc1234")

    def test_empty_line_returns_none(self):
        self.assertIsNone(sn_gate.parse_git_log_line(""))
        self.assertIsNone(sn_gate.parse_git_log_line("   "))

    def test_malformed_line_returns_none(self):
        # Missing pipe separators
        self.assertIsNone(sn_gate.parse_git_log_line("abc1234 2026-06-09 Merge task/1 into main"))

    def test_subject_preserved(self):
        info = sn_gate.parse_git_log_line(self.FIX_LINE)
        self.assertIsNotNone(info)
        self.assertEqual(info.subject, "fix(verify): raise timeout")

    def test_subject_with_pipes_preserved(self):
        # Subject itself contains a pipe — only first two pipes are split
        line = "abc1234|2026-06-09T11:18:00+00:00|docs: add table|with pipe"
        info = sn_gate.parse_git_log_line(line)
        self.assertIsNotNone(info)
        self.assertEqual(info.subject, "docs: add table|with pipe")


class TestClusterMerges(unittest.TestCase):
    """Tests for cluster_merges using synthetic CommitInfo fixtures."""

    def _make_merge(self, sha: str, seconds_offset: int, task_id: str = "1") -> sn_gate.CommitInfo:
        from datetime import datetime
        base = datetime(2026, 6, 9, 11, 18, 0, tzinfo=timezone.utc)
        from datetime import timedelta
        ts = base + timedelta(seconds=seconds_offset)
        return sn_gate.CommitInfo(sha=sha, timestamp=ts, subject=f"Merge task/{task_id} into main",
                                  is_merge=True, task_id=task_id)

    def test_empty_input_returns_empty(self):
        self.assertEqual(sn_gate.cluster_merges([]), [])

    def test_single_merge_is_singleton_cluster(self):
        m = self._make_merge("aaa", 0)
        clusters = sn_gate.cluster_merges([m])
        self.assertEqual(len(clusters), 1)
        self.assertEqual(clusters[0].size, 1)

    def test_five_merges_within_13s_is_one_cluster(self):
        # Simulates the 5-merge burst within 13s observed at 2026-06-09T11:18
        merges = [self._make_merge(f"s{i}", i * 3, str(i + 1)) for i in range(5)]
        # Gaps: 3s, 3s, 3s, 3s — all ≤ 180s default window
        clusters = sn_gate.cluster_merges(merges, window_seconds=180)
        self.assertEqual(len(clusters), 1, "All 5 merges should be in one cluster")
        self.assertEqual(clusters[0].size, 5)

    def test_merges_beyond_window_split_into_separate_clusters(self):
        # Two merges far apart
        m1 = self._make_merge("m1", 0, "100")
        m2 = self._make_merge("m2", 300, "200")  # 300s gap
        clusters = sn_gate.cluster_merges([m1, m2], window_seconds=180)
        self.assertEqual(len(clusters), 2)
        self.assertEqual(clusters[0].size, 1)
        self.assertEqual(clusters[1].size, 1)

    def test_merges_exactly_at_window_boundary_included(self):
        # Gap == window: included in same cluster
        m1 = self._make_merge("m1", 0, "100")
        m2 = self._make_merge("m2", 180, "200")  # exactly 180s
        clusters = sn_gate.cluster_merges([m1, m2], window_seconds=180)
        self.assertEqual(len(clusters), 1)
        self.assertEqual(clusters[0].size, 2)

    def test_merges_just_over_boundary_splits(self):
        # Gap = 181 > 180: splits into 2 clusters
        m1 = self._make_merge("m1", 0, "100")
        m2 = self._make_merge("m2", 181, "200")
        clusters = sn_gate.cluster_merges([m1, m2], window_seconds=180)
        self.assertEqual(len(clusters), 2)

    def test_burst_then_isolated(self):
        # 3 merges in a burst, then one isolated
        burst = [self._make_merge(f"b{i}", i * 10, str(i)) for i in range(3)]
        isolated = self._make_merge("iso", 1000, "99")
        clusters = sn_gate.cluster_merges(burst + [isolated], window_seconds=180)
        self.assertEqual(len(clusters), 2)
        self.assertEqual(clusters[0].size, 3)
        self.assertEqual(clusters[1].size, 1)

    def test_task_ids_in_cluster(self):
        merges = [self._make_merge(f"m{i}", i * 5, str(1000 + i)) for i in range(3)]
        clusters = sn_gate.cluster_merges(merges, window_seconds=180)
        self.assertEqual(len(clusters), 1)
        self.assertEqual(set(clusters[0].task_ids), {"1000", "1001", "1002"})

    def test_unordered_input_is_sorted(self):
        # Merges given in reverse order should still cluster correctly
        m1 = self._make_merge("m1", 0, "1")
        m2 = self._make_merge("m2", 10, "2")
        m3 = self._make_merge("m3", 20, "3")
        clusters = sn_gate.cluster_merges([m3, m1, m2], window_seconds=180)
        self.assertEqual(len(clusters), 1)
        self.assertEqual(clusters[0].size, 3)


if __name__ == "__main__":
    unittest.main()
