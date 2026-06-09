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


# ---------------------------------------------------------------------------
# step-5: Tests for estimate_s (synthetic fixtures)
# ---------------------------------------------------------------------------

class TestEstimateS(unittest.TestCase):
    """Tests for estimate_s using controlled synthetic cluster sets."""

    def _make_cluster_with_size(
        self,
        n: int,
        base_seconds: int = 0,
        task_id_prefix: str = "9",
    ) -> sn_gate.Cluster:
        """Return a cluster with n merges."""
        from datetime import datetime, timedelta
        base = datetime(2026, 6, 9, 0, 0, 0, tzinfo=timezone.utc)
        merges = []
        for i in range(n):
            ts = base + timedelta(seconds=base_seconds + i * 5)
            tid = f"{task_id_prefix}{base_seconds:06d}{i}"
            merges.append(sn_gate.CommitInfo(
                sha=f"m{base_seconds}{i}", timestamp=ts,
                subject=f"Merge task/{tid} into main",
                is_merge=True, task_id=tid,
            ))
        return sn_gate.Cluster(merges=merges)

    def _make_fix_commit(self, seconds_after_base: int, strong_task_id: str = None) -> sn_gate.CommitInfo:
        from datetime import datetime, timedelta
        # Places fix-forward relative to a base of 2026-06-09T00:00:00Z
        ts = datetime(2026, 6, 9, 0, 0, 0, tzinfo=timezone.utc) + timedelta(seconds=seconds_after_base)
        if strong_task_id:
            subj = f"fix(task/{strong_task_id}): correct bad merge"
        else:
            subj = "fix(verify): generic timeout patch"
        return sn_gate.CommitInfo(sha=f"fix{seconds_after_base}", timestamp=ts,
                                  subject=subj, is_merge=False, task_id=None)

    def _build_following_index(
        self,
        clusters: list,
        fix_commits_by_cluster_idx: dict,
    ) -> dict:
        """Build a simple following_index for testing: cluster→list of fix commits."""
        return {i: fix_commits_by_cluster_idx.get(i, []) for i in range(len(clusters))}

    # ── 10 two-member clusters, 3 STRONG follows ────────────────────────────

    def test_10_two_member_clusters_3_strong_follows(self):
        """s(2) = 0.7 with 10 clusters, 3 followed strongly → ambiguous_frac = 0."""
        clusters = [self._make_cluster_with_size(2, base_seconds=i * 1000) for i in range(10)]
        fix_idx: dict = {}
        # Clusters 0, 1, 2 are followed by a strong fix-forward
        for cluster_i in [0, 1, 2]:
            cluster = clusters[cluster_i]
            # A fix commit that references the first task_id in the cluster
            tid = cluster.task_ids[0]
            fix_ts_offset = cluster_i * 1000 + 100  # well within lookahead
            fix_commit = self._make_fix_commit(fix_ts_offset, strong_task_id=tid)
            fix_idx[cluster_i] = [fix_commit]

        following_index = self._build_following_index(clusters, fix_idx)
        result = sn_gate.estimate_s(clusters, following_index, n=2, lookahead=86400)
        self.assertEqual(result.sample_size, 10)
        self.assertAlmostEqual(result.s_point, 0.7, places=9)
        self.assertAlmostEqual(result.ambiguous_frac, 0.0, places=9)

    # ── Case with one weak follow → ambiguous_frac > 0 ─────────────────────

    def test_weak_follow_gives_ambiguous_frac(self):
        """2 clusters, 1 followed (weakly) → s=0.5, ambiguous_frac=1.0."""
        clusters = [self._make_cluster_with_size(2, base_seconds=i * 1000) for i in range(2)]
        fix_idx = {
            0: [self._make_fix_commit(100, strong_task_id=None)],  # weak fix
        }
        following_index = self._build_following_index(clusters, fix_idx)
        result = sn_gate.estimate_s(clusters, following_index, n=2, lookahead=86400)
        self.assertEqual(result.sample_size, 2)
        self.assertAlmostEqual(result.s_point, 0.5, places=9)
        self.assertAlmostEqual(result.ambiguous_frac, 1.0, places=9)

    # ── n=3 sampling (only clusters of size ≥3 count) ──────────────────────

    def test_n3_counts_only_size_ge3_clusters(self):
        """When n=3: size-2 clusters excluded; only size-3 clusters count."""
        size3_clusters = [self._make_cluster_with_size(3, base_seconds=i * 2000) for i in range(5)]
        size2_clusters = [self._make_cluster_with_size(2, base_seconds=10000 + i * 2000) for i in range(5)]
        all_clusters = size3_clusters + size2_clusters
        fix_idx: dict = {}
        # Follow 2 of the size-3 clusters strongly
        for cluster_i in [0, 1]:
            tid = size3_clusters[cluster_i].task_ids[0]
            fix_commit = self._make_fix_commit(cluster_i * 2000 + 100, strong_task_id=tid)
            fix_idx[cluster_i] = [fix_commit]
        following_index = self._build_following_index(all_clusters, fix_idx)
        result = sn_gate.estimate_s(all_clusters, following_index, n=3, lookahead=86400)
        # Only 5 size-3 clusters eligible; 2 followed → s = 1 - 2/5 = 0.6
        self.assertEqual(result.sample_size, 5)
        self.assertAlmostEqual(result.s_point, 0.6, places=9)

    def test_no_eligible_clusters_returns_nan(self):
        """When no clusters meet the size threshold, sample_size=0 and s=nan."""
        import math
        clusters = [self._make_cluster_with_size(1) for _ in range(5)]  # all singletons
        following_index = {i: [] for i in range(5)}
        result = sn_gate.estimate_s(clusters, following_index, n=2, lookahead=86400)
        self.assertEqual(result.sample_size, 0)
        self.assertTrue(math.isnan(result.s_point))

    def test_zero_follows_gives_s1(self):
        """No fix-forwards → s=1.0 (perfect success rate)."""
        clusters = [self._make_cluster_with_size(2, base_seconds=i * 1000) for i in range(5)]
        following_index = {i: [] for i in range(5)}
        result = sn_gate.estimate_s(clusters, following_index, n=2, lookahead=86400)
        self.assertEqual(result.sample_size, 5)
        self.assertAlmostEqual(result.s_point, 1.0, places=9)
        self.assertAlmostEqual(result.ambiguous_frac, 0.0, places=9)

    def test_all_followed_gives_s0(self):
        """All clusters followed by fix-forward → s=0.0."""
        clusters = [self._make_cluster_with_size(2, base_seconds=i * 1000) for i in range(4)]
        fix_idx = {}
        for i, cluster in enumerate(clusters):
            fix_idx[i] = [self._make_fix_commit(i * 1000 + 50)]
        following_index = self._build_following_index(clusters, fix_idx)
        result = sn_gate.estimate_s(clusters, following_index, n=2, lookahead=86400)
        self.assertAlmostEqual(result.s_point, 0.0, places=9)

    def test_mixed_strong_weak_ambiguous_frac(self):
        """4 followed: 2 strong, 2 weak → ambiguous_frac = 0.5."""
        clusters = [self._make_cluster_with_size(2, base_seconds=i * 1000) for i in range(4)]
        fix_idx = {}
        for i, cluster in enumerate(clusters):
            if i < 2:
                # Strong follow
                tid = cluster.task_ids[0]
                fix_idx[i] = [self._make_fix_commit(i * 1000 + 50, strong_task_id=tid)]
            else:
                # Weak follow
                fix_idx[i] = [self._make_fix_commit(i * 1000 + 50, strong_task_id=None)]
        following_index = self._build_following_index(clusters, fix_idx)
        result = sn_gate.estimate_s(clusters, following_index, n=2, lookahead=86400)
        self.assertEqual(result.sample_size, 4)
        self.assertAlmostEqual(result.s_point, 0.0, places=9)  # all 4 followed
        self.assertAlmostEqual(result.ambiguous_frac, 0.5, places=9)  # 2/4 weak


# ---------------------------------------------------------------------------
# step-7: Tests for decide (deterministic go/no-go)
# ---------------------------------------------------------------------------

class TestDecide(unittest.TestCase):
    """Tests for the decide() function covering all classification branches."""

    # ── GO cases ─────────────────────────────────────────────────────────────

    def test_go_n2_s2_clears_with_margin(self):
        """s2=0.7 clears N=2 with margin (0.7≥0.6+margin), n2=20, not ambiguous."""
        result = sn_gate.decide(s2=0.7, n2=20, s3=0.2, n3=15,
                                amb2=0.0, amb3=0.0)
        self.assertEqual(result.classification, "GO")
        self.assertEqual(result.chosen_N, 2)

    def test_go_n3_preferred_over_n2_when_both_clear(self):
        """s3=0.50>0.40 clears N=3 (and s2=0.7 also clears N=2) → GO, chosen_N=3."""
        result = sn_gate.decide(s2=0.7, n2=20, s3=0.50, n3=15,
                                amb2=0.0, amb3=0.0)
        self.assertEqual(result.classification, "GO")
        self.assertEqual(result.chosen_N, 3, "Must pick largest clearing N")

    def test_go_n3_minimum_above_upper_band(self):
        """s3 just at upper band edge (1/3+0.2/3 = 0.3+0.0667=0.3667) → margin just OK."""
        # upper_band_N3 = 1/3 + 0.2/3 ≈ 0.4000
        result = sn_gate.decide(s2=0.7, n2=15, s3=0.42, n3=12,
                                amb2=0.0, amb3=0.0)
        self.assertEqual(result.classification, "GO")
        self.assertEqual(result.chosen_N, 3)

    # ── NO-GO cases ──────────────────────────────────────────────────────────

    def test_no_go_s2_below_lower_band(self):
        """No N clears AND s2=0.30 ≤ 0.40 (lower band), n2=20, not ambiguous → NO-GO."""
        result = sn_gate.decide(s2=0.30, n2=20, s3=0.20, n3=15,
                                amb2=0.0, amb3=0.0)
        self.assertEqual(result.classification, "NO-GO")
        self.assertIsNone(result.chosen_N)

    def test_no_go_s2_exactly_at_lower_band(self):
        """s2=0.40 is at the lower band boundary → NO-GO (comfortably ≤ 0.40)."""
        result = sn_gate.decide(s2=0.40, n2=20, s3=0.20, n3=15,
                                amb2=0.0, amb3=0.0)
        self.assertEqual(result.classification, "NO-GO")

    # ── MARGINAL: deciding N in band ─────────────────────────────────────────

    def test_marginal_band_n2(self):
        """s2=0.55 in N=2 band (0.40, 0.60) → MARGINAL."""
        result = sn_gate.decide(s2=0.55, n2=20, s3=0.20, n3=15,
                                amb2=0.0, amb3=0.0)
        self.assertEqual(result.classification, "MARGINAL")
        self.assertIsNone(result.chosen_N)

    def test_marginal_deciding_n3_in_band(self):
        """s3=0.38 ∈ N=3 band (0.2667, 0.40) but clears 1/3=0.333... → deciding N=3 in band → MARGINAL.
        s2=0.7 also clears N=2 with margin, but since N=3 is the LARGEST clearing N and it's in band,
        the result must be MARGINAL (no silent fall-back to N=2)."""
        result = sn_gate.decide(s2=0.7, n2=20, s3=0.38, n3=15,
                                amb2=0.0, amb3=0.0)
        self.assertEqual(result.classification, "MARGINAL",
                         "Marginality judged on the deciding (largest-clearing) N, NOT a fall-back N=2")
        self.assertIsNone(result.chosen_N)
        self.assertEqual(result.deciding_N, 3)

    # ── MARGINAL: thin sample ────────────────────────────────────────────────

    def test_marginal_thin_sample(self):
        """s2=0.7, n2=6 < 10 (thin) → MARGINAL."""
        result = sn_gate.decide(s2=0.7, n2=6, s3=0.2, n3=5,
                                amb2=0.0, amb3=0.0)
        self.assertEqual(result.classification, "MARGINAL")
        self.assertIsNone(result.chosen_N)

    def test_marginal_thin_at_boundary(self):
        """n2=9 (< thin=10) → MARGINAL even if s2 clears with margin."""
        result = sn_gate.decide(s2=0.7, n2=9, s3=0.2, n3=8,
                                amb2=0.0, amb3=0.0)
        self.assertEqual(result.classification, "MARGINAL")

    def test_go_exactly_at_thin_boundary(self):
        """n2=10 (= thin=10) → NOT thin, so if s also clears with margin → GO."""
        result = sn_gate.decide(s2=0.7, n2=10, s3=0.2, n3=8,
                                amb2=0.0, amb3=0.0)
        self.assertEqual(result.classification, "GO")
        self.assertEqual(result.chosen_N, 2)

    # ── MARGINAL: ambiguous attribution ──────────────────────────────────────

    def test_marginal_ambiguous_n2(self):
        """s2=0.7 clears with margin, n2=20, but amb2>0 → MARGINAL."""
        result = sn_gate.decide(s2=0.7, n2=20, s3=0.2, n3=15,
                                amb2=0.5, amb3=0.0)
        self.assertEqual(result.classification, "MARGINAL")

    def test_marginal_ambiguous_n3(self):
        """s3=0.50 clears N=3 with margin, n3=15, but amb3>0 → MARGINAL at deciding N=3."""
        result = sn_gate.decide(s2=0.7, n2=20, s3=0.50, n3=15,
                                amb2=0.0, amb3=0.5)
        self.assertEqual(result.classification, "MARGINAL")

    # ── MARGINAL: no N clears, not comfortable ────────────────────────────────

    def test_marginal_no_clear_s2_in_band(self):
        """s2=0.45 in N=2 band (no N clears) → MARGINAL."""
        result = sn_gate.decide(s2=0.45, n2=20, s3=0.20, n3=15,
                                amb2=0.0, amb3=0.0)
        self.assertEqual(result.classification, "MARGINAL")
        self.assertIsNone(result.chosen_N)

    # ── deciding_N set correctly ─────────────────────────────────────────────

    def test_deciding_n_is_none_when_no_n_clears(self):
        """When no N clears 1/N, deciding_N is None."""
        result = sn_gate.decide(s2=0.30, n2=20, s3=0.20, n3=15,
                                amb2=0.0, amb3=0.0)
        self.assertIsNone(result.deciding_N)

    def test_deciding_n_is_2_when_only_n2_clears(self):
        """When only N=2 clears, deciding_N=2."""
        result = sn_gate.decide(s2=0.7, n2=20, s3=0.20, n3=15,
                                amb2=0.0, amb3=0.0)
        self.assertEqual(result.deciding_N, 2)

    def test_deciding_n_is_3_when_n3_clears(self):
        """When N=3 clears (even if N=2 also clears), deciding_N=3."""
        result = sn_gate.decide(s2=0.7, n2=20, s3=0.50, n3=15,
                                amb2=0.0, amb3=0.0)
        self.assertEqual(result.deciding_N, 3)

    # ── per_n structure ──────────────────────────────────────────────────────

    def test_per_n_keys_present(self):
        """per_n must contain keys 2 and 3."""
        result = sn_gate.decide(s2=0.7, n2=20, s3=0.2, n3=15,
                                amb2=0.0, amb3=0.0)
        self.assertIn(2, result.per_n)
        self.assertIn(3, result.per_n)

    def test_per_n_has_required_fields(self):
        """Each per_n entry must have s, n, clears, margin, in_band, thin, ambiguous."""
        result = sn_gate.decide(s2=0.7, n2=20, s3=0.2, n3=15,
                                amb2=0.0, amb3=0.0)
        for n in [2, 3]:
            for key in ["s", "n", "clears", "margin", "in_band", "thin", "ambiguous"]:
                self.assertIn(key, result.per_n[n], f"per_n[{n}] missing key: {key}")


# ---------------------------------------------------------------------------
# step-9: Tests for build_report and --json CLI output contract
# ---------------------------------------------------------------------------

import json as json_mod
import subprocess
import tempfile
import os

class TestBuildReport(unittest.TestCase):
    """Tests for build_report: JSON summary keys and markdown content."""

    def _make_estimates(self, s2=0.7, n2=15, amb2=0.0, s3=0.50, n3=12, amb3=0.0):
        return (
            sn_gate.EstimateResult(n=2, s_point=s2, sample_size=n2, ambiguous_frac=amb2),
            sn_gate.EstimateResult(n=3, s_point=s3, sample_size=n3, ambiguous_frac=amb3),
        )

    # ── GO case ──────────────────────────────────────────────────────────────

    def test_go_summary_keys(self):
        """JSON summary must contain all required keys for GO (per plan step-9 spec)."""
        r2, r3 = self._make_estimates()
        decision = sn_gate.decide(s2=0.7, n2=15, s3=0.50, n3=12, amb2=0.0, amb3=0.0)
        summary, _ = sn_gate.build_report(r2, r3, decision, window=300, lookahead=86400)
        for key in ["s2", "n2", "s3", "n3", "ambiguous", "classification", "chosen_N", "recommended_action"]:
            self.assertIn(key, summary, f"summary missing key: {key!r}")

    def test_go_recommended_action(self):
        """GO classification → recommended_action = 'flip 1705-1708 pending'."""
        r2, r3 = self._make_estimates()
        decision = sn_gate.decide(s2=0.7, n2=15, s3=0.50, n3=12, amb2=0.0, amb3=0.0)
        summary, _ = sn_gate.build_report(r2, r3, decision, window=300, lookahead=86400)
        self.assertEqual(summary["recommended_action"], "flip 1705-1708 pending")

    def test_no_go_recommended_action(self):
        """NO-GO classification → recommended_action = 'cancel 1705-1708 + info-escalate'."""
        r2 = sn_gate.EstimateResult(n=2, s_point=0.30, sample_size=20, ambiguous_frac=0.0)
        r3 = sn_gate.EstimateResult(n=3, s_point=0.20, sample_size=15, ambiguous_frac=0.0)
        decision = sn_gate.decide(s2=0.30, n2=20, s3=0.20, n3=15, amb2=0.0, amb3=0.0)
        summary, _ = sn_gate.build_report(r2, r3, decision, window=300, lookahead=86400)
        self.assertEqual(decision.classification, "NO-GO")
        self.assertEqual(summary["recommended_action"], "cancel 1705-1708 + info-escalate")

    def test_marginal_recommended_action(self):
        """MARGINAL classification → recommended_action = 'leave deferred + escalate'."""
        r2 = sn_gate.EstimateResult(n=2, s_point=0.55, sample_size=20, ambiguous_frac=0.0)
        r3 = sn_gate.EstimateResult(n=3, s_point=0.20, sample_size=15, ambiguous_frac=0.0)
        decision = sn_gate.decide(s2=0.55, n2=20, s3=0.20, n3=15, amb2=0.0, amb3=0.0)
        summary, _ = sn_gate.build_report(r2, r3, decision, window=300, lookahead=86400)
        self.assertEqual(decision.classification, "MARGINAL")
        self.assertEqual(summary["recommended_action"], "leave deferred + escalate")

    def test_summary_s2_n2_values_match_estimates(self):
        """summary s2/n2/s3/n3 must match the provided estimate objects."""
        r2, r3 = self._make_estimates(s2=0.65, n2=18, s3=0.42, n3=11)
        decision = sn_gate.decide(s2=0.65, n2=18, s3=0.42, n3=11, amb2=0.0, amb3=0.0)
        summary, _ = sn_gate.build_report(r2, r3, decision, window=300, lookahead=86400)
        self.assertAlmostEqual(summary["s2"], 0.65, places=5)
        self.assertEqual(summary["n2"], 18)
        self.assertAlmostEqual(summary["s3"], 0.42, places=5)
        self.assertEqual(summary["n3"], 11)

    # ── Markdown content ──────────────────────────────────────────────────────

    def test_markdown_contains_s2_value(self):
        """Markdown must contain the s(2) point estimate as a number."""
        r2, r3 = self._make_estimates(s2=0.700, n2=15)
        decision = sn_gate.decide(s2=0.700, n2=15, s3=0.50, n3=12, amb2=0.0, amb3=0.0)
        _, md = sn_gate.build_report(r2, r3, decision, window=300, lookahead=86400)
        self.assertIn("0.700", md, "Markdown must contain s(2)=0.700")

    def test_markdown_contains_n2_value(self):
        """Markdown must contain the N=2 sample size."""
        r2, r3 = self._make_estimates(n2=15)
        decision = sn_gate.decide(s2=0.7, n2=15, s3=0.50, n3=12, amb2=0.0, amb3=0.0)
        _, md = sn_gate.build_report(r2, r3, decision, window=300, lookahead=86400)
        self.assertIn("15", md, "Markdown must contain n(2)=15")

    def test_markdown_contains_s3_value(self):
        """Markdown must contain the s(3) point estimate as a number."""
        r2, r3 = self._make_estimates(s3=0.500, n3=12)
        decision = sn_gate.decide(s2=0.7, n2=15, s3=0.500, n3=12, amb2=0.0, amb3=0.0)
        _, md = sn_gate.build_report(r2, r3, decision, window=300, lookahead=86400)
        self.assertIn("0.500", md, "Markdown must contain s(3)=0.500")

    def test_markdown_contains_classification(self):
        """Markdown must contain the classification string."""
        r2, r3 = self._make_estimates()
        decision = sn_gate.decide(s2=0.7, n2=15, s3=0.50, n3=12, amb2=0.0, amb3=0.0)
        _, md = sn_gate.build_report(r2, r3, decision, window=300, lookahead=86400)
        self.assertIn(decision.classification, md)

    def test_markdown_contains_window_and_lookahead(self):
        """Markdown must reference the window and lookahead parameters (methodology)."""
        r2, r3 = self._make_estimates()
        decision = sn_gate.decide(s2=0.7, n2=15, s3=0.50, n3=12, amb2=0.0, amb3=0.0)
        _, md = sn_gate.build_report(r2, r3, decision, window=300, lookahead=86400)
        self.assertIn("300", md, "Markdown must contain window=300")
        self.assertIn("86400", md, "Markdown must contain lookahead=86400")


class TestCLIJsonOutput(unittest.TestCase):
    """Black-box CLI smoke tests for --json output contract."""

    def _make_git_log_fixture(self) -> str:
        """Return a minimal git-log-style fixture with a 3-merge cluster."""
        return "\n".join([
            "abc0001|2026-06-01T12:00:00+00:00|Merge task/100 into main",
            "abc0002|2026-06-01T12:00:10+00:00|Merge task/101 into main",
            "abc0003|2026-06-01T12:00:20+00:00|Merge task/102 into main",
            "abc0004|2026-06-01T13:00:00+00:00|feat(x): some feature",
        ])

    def test_json_output_has_required_keys(self):
        """--json output must contain all required top-level keys."""
        fixture = self._make_git_log_fixture()
        with tempfile.NamedTemporaryFile(mode="w", suffix=".txt", delete=False) as f:
            f.write(fixture)
            tmppath = f.name
        try:
            result = subprocess.run(
                ["python3", "scripts/sn_gate.py",
                 "--git-log-file", tmppath,
                 "--window", "300",
                 "--lookahead", "86400",
                 "--json"],
                capture_output=True, text=True, check=True,
            )
            data = json_mod.loads(result.stdout)
            for key in ["s2", "n2", "s3", "n3", "classification", "chosen_N", "recommended_action"]:
                self.assertIn(key, data, f"JSON output missing key: {key!r}")
        finally:
            os.unlink(tmppath)

    def test_json_recommended_action_is_valid_string(self):
        """--json recommended_action must be one of the three known values."""
        valid = {"flip 1705-1708 pending", "cancel 1705-1708 + info-escalate", "leave deferred + escalate"}
        fixture = self._make_git_log_fixture()
        with tempfile.NamedTemporaryFile(mode="w", suffix=".txt", delete=False) as f:
            f.write(fixture)
            tmppath = f.name
        try:
            result = subprocess.run(
                ["python3", "scripts/sn_gate.py",
                 "--git-log-file", tmppath,
                 "--json"],
                capture_output=True, text=True, check=True,
            )
            data = json_mod.loads(result.stdout)
            self.assertIn(data["recommended_action"], valid)
        finally:
            os.unlink(tmppath)


if __name__ == "__main__":
    unittest.main()
