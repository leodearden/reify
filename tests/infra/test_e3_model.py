#!/usr/bin/env python3
"""
test_e3_model.py — stdlib unittest for the pure §7 computation of the
pre-registered E3 analysis (docs/prds/v0_6/e3-granularity/analysis/): the
exact statistics in e3_stats, and e3_model's attribution, censoring,
endpoints, stop rule, confirmatory gate and descriptives.

WHAT RUNS THIS: not the gate directly. run_all.sh discovers `test_*.sh` only,
so the discovered member is the thin wrapper tests/infra/test_e3_analysis.sh,
which runs this file beside its two siblings (test_e3_sources.py,
test_e3_model.py, test_e3_analysis.py). A .py it did not name would be
silently never run.

IMPORT IDIOM: tests/infra/ (for the hermetic fixture builders in
e3_analysis_fixtures.py) and the analysis directory are prepended to sys.path,
so the modules import by their plain names without installation.

Every input is built in memory from the fixtures; nothing is read from disk.
"""

import dataclasses
import statistics
import sys
import unittest
from datetime import datetime, timedelta
from fractions import Fraction
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
INFRA_DIR = REPO_ROOT / "tests" / "infra"
ANALYSIS_DIR = REPO_ROOT / "docs" / "prds" / "v0_6" / "e3-granularity" / "analysis"

sys.path.insert(0, str(INFRA_DIR))
sys.path.insert(0, str(ANALYSIS_DIR))

import e3_analysis_fixtures as fx  # noqa: E402  (after sys.path manipulation)
import e3_model  # noqa: E402
import e3_sources  # noqa: E402
from e3_sources import AttemptOutcome, Severity, TaskStatus  # noqa: E402
import e3_stats  # noqa: E402
from e3_protocol import E3, UTC, Arm, ProtocolViolation  # noqa: E402

at = fx.at


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
            run_ended_at=RUN_END, run_id="r1", run_end_known_at=None):
    """An Attempt whose run's end is known when it happens unless
    run_end_known_at says later (a crashed run)."""
    return e3_sources.Attempt(
        task_id=task_id, run_id=run_id,
        started_at=started_at or RUN_END - timedelta(hours=6),
        ended_at=ended_at, outcome=outcome, run_ended_at=run_ended_at,
        run_end_known_at=run_end_known_at or run_ended_at)


class CensoringTests(unittest.TestCase):
    def setUp(self):
        self.protocol = fx.mini_protocol()
        self.arm_tasks = e3_model.attribute(fx.task_snapshot(self.protocol), self.protocol)

    def _classify(self, **fields):
        return e3_model.classify_attempt(attempt(**fields), self.protocol)

    def test_cancelled_at_the_run_boundary_is_drain_censored(self):
        self.assertIs(
            self._classify(ended_at=RUN_END - timedelta(seconds=40), outcome=AttemptOutcome.CANCELLED),
            e3_model.AttemptClass.DRAIN_CENSORED)

    def test_cancelled_far_from_the_run_end_is_rework(self):
        self.assertIs(
            self._classify(ended_at=RUN_END - timedelta(hours=2), outcome=AttemptOutcome.CANCELLED),
            e3_model.AttemptClass.COUNTED)

    def test_drain_window_is_inclusive(self):
        window = self.protocol.drain_window
        self.assertIs(self._classify(ended_at=RUN_END - window, outcome=AttemptOutcome.CANCELLED),
                      e3_model.AttemptClass.DRAIN_CENSORED)
        self.assertIs(
            self._classify(ended_at=RUN_END - window - timedelta(seconds=1), outcome=AttemptOutcome.CANCELLED),
            e3_model.AttemptClass.COUNTED)

    def test_unfinished_attempt_in_an_ended_run_is_drain_censored(self):
        self.assertIs(self._classify(ended_at=None), e3_model.AttemptClass.DRAIN_CENSORED)

    def test_unfinished_attempt_in_a_live_run_is_in_flight(self):
        self.assertIs(self._classify(ended_at=None, run_ended_at=None),
                      e3_model.AttemptClass.IN_FLIGHT)

    def test_other_outcomes_always_count(self):
        for outcome in [o for o in AttemptOutcome if o is not AttemptOutcome.CANCELLED]:
            for distance in (timedelta(seconds=10), timedelta(hours=3)):
                self.assertIs(
                    self._classify(ended_at=RUN_END - distance, outcome=outcome),
                    e3_model.AttemptClass.COUNTED, (outcome, distance))

    def test_a_crashed_run_counts_as_live_until_the_next_run_starts(self):
        crashed, next_run = at(10, 12), at(10, 14)
        runs = e3_sources.RunsData(
            attempts=(attempt(task_id=1102, started_at=at(10, 9), run_ended_at=crashed,
                              run_end_known_at=next_run),),
            invocations=(), escalations=())
        record = fx.task_snapshot(self.protocol).task(1102)

        def drained_and_in_flight(as_of):
            ledger = e3_model.task_ledger(record, runs, (), as_of, self.protocol)
            return ledger.attempts_drain_censored, ledger.attempts_in_flight

        self.assertEqual(drained_and_in_flight(at(10, 13)), (0, 1))
        self.assertEqual(drained_and_in_flight(next_run), (1, 0))

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


def esc(task_id, escalation_id, created_at, severity=Severity.BLOCKING, level=None):
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
                   outcome=AttemptOutcome.CANCELLED, run_ended_at=ended_at + timedelta(seconds=20))


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
                closed(101, pre, AttemptOutcome.REQUEUED), closed(101, post, AttemptOutcome.DONE), drained(101, post),
                closed(101, late, AttemptOutcome.DONE),
                closed(102, pre, AttemptOutcome.BLOCKED),
                drained(203, at(27, 9, month=8)),
                closed(1101, at(27, 8, month=8), AttemptOutcome.DONE), drained(1101, at(2, 1)),
                closed(1102, at(20, 9), AttemptOutcome.DONE),
                drained(1103, at(21, 9)),
                closed(150, at(22, 9), AttemptOutcome.DONE),
                attempt(task_id=202, started_at=AS_OF - timedelta(hours=2),
                        ended_at=AS_OF + timedelta(hours=1), outcome=AttemptOutcome.DONE,
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
                esc(101, "esc-101-4", post, severity=Severity.INFO), esc(101, "esc-101-5", late),
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
        self.assertIs(frozen.excluded, e3_model.Exclusion.FREEZE_WINDOW)
        self.assertEqual(frozen.ledger.landed_at, at(5, 9))
        self.assertFalse(frozen.landed_eligible)
        self.assertTrue(self.outcomes[1102].landed_eligible)
        self.assertIsNone(self.outcomes[1102].excluded)

    def test_era_by_attempt_starts(self):
        self.assertIs(self.outcomes[101].era, e3_model.Era.STRADDLE)
        self.assertIs(self.outcomes[102].era, e3_model.Era.PRE)
        self.assertIs(self.outcomes[1102].era, e3_model.Era.POST)
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
                          landed.attempts_counted), (TaskStatus.DONE, at(23, 9), 40, 1.5, 1))
        self.assertEqual((untouched.status, untouched.landed_at, untouched.cost,
                          untouched.attempts_counted), (TaskStatus.PENDING, None, 0.0, 0))
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
                                      for era in e3_model.Era})
        straddle = cells[(Arm.COARSE, e3_model.Era.STRADDLE)]
        self.assertEqual((straddle.n_tasks, straddle.usd_per_kloc, straddle.attempts_per_task,
                          straddle.blocking_esc_per_task), (1, 20.0, 2.0, 2.0))
        post = cells[(Arm.COARSE, e3_model.Era.POST)]
        self.assertEqual((post.n_tasks, post.usd_per_kloc, post.attempts_per_task), (1, 20.0, 0.0))
        standard_post = cells[(Arm.STANDARD, e3_model.Era.POST)]
        self.assertEqual((standard_post.n_tasks, standard_post.attempts_per_task), (1, 1.0))
        empty = cells[(Arm.COARSE, e3_model.Era.PRE)]
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


if __name__ == "__main__":
    unittest.main()
