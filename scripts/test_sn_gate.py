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


# ---------------------------------------------------------------------------
# step-3: Tests for is_fix_forward and attribute_fix_forward (synthetic fixtures)
# ---------------------------------------------------------------------------

class TestIsFixForward(unittest.TestCase):
    """Tests for is_fix_forward using synthetic subject strings."""

    def _check_true(self, subject: str):
        self.assertTrue(
            sn_gate.is_fix_forward(subject),
            f"Expected is_fix_forward=True for: {subject!r}",
        )

    def _check_false(self, subject: str):
        self.assertFalse(
            sn_gate.is_fix_forward(subject),
            f"Expected is_fix_forward=False for: {subject!r}",
        )

    # Patterns that should match (case-insensitive)
    def test_fix_colon_matches(self):
        self._check_true("fix(verify): raise timeout")

    def test_fix_paren_matches(self):
        self._check_true("fix(infra): add retry")

    def test_fix_dash_matches(self):
        self._check_true("fix-forward: handle edge case")

    def test_fix_space_forward_matches(self):
        self._check_true("fix forward: correct merge")

    def test_revert_matches(self):
        self._check_true("revert: undo bad merge")

    def test_revert_quoted_matches(self):
        self._check_true("Revert \"some commit message\"")

    def test_hotfix_matches(self):
        self._check_true("hotfix: patch security issue")

    def test_case_insensitive_fix(self):
        self._check_true("Fix(lint): correct style")
        self._check_true("FIX: important patch")

    def test_case_insensitive_revert(self):
        self._check_true("REVERT: roll back task 123")

    def test_case_insensitive_hotfix(self):
        self._check_true("Hotfix: emergency patch")

    # Patterns that should NOT match
    def test_merge_does_not_match(self):
        self._check_false("Merge task/4421 into main")

    def test_feat_does_not_match(self):
        self._check_false("feat(sn_gate): add estimator")

    def test_docs_does_not_match(self):
        self._check_false("docs(prd): update manifest")

    def test_test_does_not_match(self):
        self._check_false("test(infra): add unit test")

    def test_chore_does_not_match(self):
        self._check_false("chore: bump version")

    def test_fixup_in_middle_does_not_match(self):
        # "fixup" in the middle of a line should not match (anchored to start)
        self._check_false("apply fixup for issue 123")

    def test_empty_string_does_not_match(self):
        self._check_false("")


class TestAttributeFixForward(unittest.TestCase):
    """Tests for attribute_fix_forward using synthetic cluster + commit fixtures."""

    def _make_commit(
        self,
        sha: str,
        seconds_offset: int,
        subject: str,
        is_merge: bool = False,
        task_id: str = None,
    ) -> sn_gate.CommitInfo:
        from datetime import datetime, timedelta
        base = datetime(2026, 6, 9, 12, 0, 0, tzinfo=timezone.utc)
        ts = base + timedelta(seconds=seconds_offset)
        return sn_gate.CommitInfo(sha=sha, timestamp=ts, subject=subject,
                                  is_merge=is_merge, task_id=task_id)

    def _make_cluster(self, *task_ids: str) -> sn_gate.Cluster:
        from datetime import datetime, timedelta
        base = datetime(2026, 6, 9, 12, 0, 0, tzinfo=timezone.utc)
        merges = []
        for i, tid in enumerate(task_ids):
            ts = base + timedelta(seconds=i * 5)
            merges.append(sn_gate.CommitInfo(
                sha=f"m{i}", timestamp=ts,
                subject=f"Merge task/{tid} into main",
                is_merge=True, task_id=tid,
            ))
        return sn_gate.Cluster(merges=merges)

    def test_no_following_commits_returns_not_followed(self):
        cluster = self._make_cluster("100", "101")
        followed, strong = sn_gate.attribute_fix_forward(cluster, [], lookahead_seconds=86400)
        self.assertFalse(followed)
        self.assertFalse(strong)

    def test_no_fix_forward_in_following_returns_not_followed(self):
        cluster = self._make_cluster("100", "101")
        following = [
            self._make_commit("c1", 60, "feat(x): add feature"),
            self._make_commit("c2", 120, "docs: update readme"),
        ]
        followed, strong = sn_gate.attribute_fix_forward(cluster, following, lookahead_seconds=86400)
        self.assertFalse(followed)

    def test_strong_hit_with_task_id_reference(self):
        # Fix-forward commit explicitly references task 100 (a cluster member)
        cluster = self._make_cluster("100", "101")
        following = [
            self._make_commit("fix1", 600, "fix(task/100): correct bad merge"),
        ]
        followed, strong = sn_gate.attribute_fix_forward(cluster, following, lookahead_seconds=86400)
        self.assertTrue(followed, "Should detect fix-forward")
        self.assertTrue(strong, "Should be strong attribution (task_id in subject)")

    def test_weak_hit_without_task_id_reference(self):
        # Fix-forward commit does NOT reference any cluster task_id
        cluster = self._make_cluster("100", "101")
        following = [
            self._make_commit("fix1", 600, "fix(verify): generic timeout patch"),
        ]
        followed, strong = sn_gate.attribute_fix_forward(cluster, following, lookahead_seconds=86400)
        self.assertTrue(followed, "Should detect fix-forward")
        self.assertFalse(strong, "Should be weak attribution (no task_id reference)")

    def test_fix_forward_beyond_lookahead_not_counted(self):
        cluster = self._make_cluster("100")
        # Fix-forward commit beyond the lookahead window
        following = [
            self._make_commit("fix1", 90000, "fix(x): out of window"),  # 90000s > 86400s
        ]
        followed, strong = sn_gate.attribute_fix_forward(cluster, following, lookahead_seconds=86400)
        self.assertFalse(followed, "Fix-forward beyond lookahead should not count")

    def test_first_fix_forward_in_window_is_returned(self):
        # First fix-forward wins; second fix-forward is ignored
        cluster = self._make_cluster("100")
        following = [
            self._make_commit("fix1", 60, "fix(verify): first fix"),  # weak
            self._make_commit("fix2", 120, "fix(task/100): second fix"),  # strong, but after fix1
        ]
        followed, strong = sn_gate.attribute_fix_forward(cluster, following, lookahead_seconds=86400)
        self.assertTrue(followed)
        # fix1 (weak) is the first fix-forward, so strong=False
        self.assertFalse(strong, "First fix-forward (weak) should be attributed")

    def test_empty_cluster_returns_not_followed(self):
        cluster = sn_gate.Cluster(merges=[])
        following = [self._make_commit("fix1", 60, "fix: something")]
        followed, strong = sn_gate.attribute_fix_forward(cluster, following)
        self.assertFalse(followed)

    def test_revert_is_detected_as_fix_forward(self):
        cluster = self._make_cluster("200")
        following = [
            self._make_commit("r1", 300, "revert: undo task/200 merge"),
        ]
        followed, strong = sn_gate.attribute_fix_forward(cluster, following, lookahead_seconds=86400)
        self.assertTrue(followed)

    def test_strong_attribution_via_task_in_parens(self):
        cluster = self._make_cluster("4421")
        following = [
            self._make_commit("f1", 100, "fix: correct issue (task 4421)"),
        ]
        followed, strong = sn_gate.attribute_fix_forward(cluster, following, lookahead_seconds=86400)
        self.assertTrue(followed)
        self.assertTrue(strong)


if __name__ == "__main__":
    unittest.main()
