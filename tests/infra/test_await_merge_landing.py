#!/usr/bin/env python3
"""
test_await_merge_landing.py — stdlib unittest for scripts/await-merge-landing.py.

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only; the member is the thin
wrapper tests/infra/test_await_merge_landing.sh.
"""

import importlib.util
import sys
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
TOOL_PATH = REPO_ROOT / "scripts" / "await-merge-landing.py"


def _load_tool():
    spec = importlib.util.spec_from_file_location("await_merge_landing", TOOL_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {TOOL_PATH}")
    module = importlib.util.module_from_spec(spec)
    # Register before exec: @dataclass resolves a field's type through
    # sys.modules[cls.__module__], which is None for an unregistered module.
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


aml = _load_tool()
Verdict = aml.Verdict

LIVE_STATES = ("queued", "verifying", "gate", "finalizing")
EPISTEMIC_STATES = ("unknown", "no_record", "stale_record", "journaled")


class ClassifyMergeStatusTests(unittest.TestCase):
    def test_done_is_landed(self):
        self.assertIs(aml.classify_merge_status({"state": "done"}), Verdict.LANDED)

    def test_conflict_and_blocked_are_blocked(self):
        for state in ("conflict", "blocked"):
            with self.subTest(state=state):
                self.assertIs(aml.classify_merge_status({"state": state}),
                              Verdict.BLOCKED)

    def test_abandoned_and_superseded_are_failed(self):
        for state in ("abandoned", "superseded"):
            with self.subTest(state=state):
                self.assertIs(aml.classify_merge_status({"state": state}),
                              Verdict.FAILED)

    def test_live_states_keep_waiting(self):
        for state in LIVE_STATES:
            with self.subTest(state=state):
                self.assertIsNone(aml.classify_merge_status({"state": state}))

    def test_epistemic_states_keep_waiting(self):
        for state in EPISTEMIC_STATES:
            with self.subTest(state=state):
                self.assertIsNone(aml.classify_merge_status({"state": state}))

    def test_unrecognised_state_fails_open(self):
        self.assertIsNone(aml.classify_merge_status({"state": "brand_new_state"}))

    def test_missing_state_keeps_waiting(self):
        self.assertIsNone(aml.classify_merge_status({"request_id": "mr-1"}))

    def test_verdict_values_are_the_exit_code_contract(self):
        self.assertEqual(Verdict.LANDED, 0)
        self.assertEqual(Verdict.FAILED, 1)
        self.assertEqual(Verdict.BLOCKED, 3)
        self.assertEqual(Verdict.PENDING, 75)
        self.assertNotIn(2, [int(v) for v in Verdict])


class FakeClock:
    """`now` reads the fake time; `sleep` records the request and advances it.
    No real time passes, so nothing here bounds wall-clock."""

    def __init__(self):
        self.t = 0.0
        self.sleeps = []

    def now(self):
        return self.t

    def sleep(self, seconds):
        self.sleeps.append(seconds)
        self.t += seconds


class ScriptedProbe:
    """Returns (or raises, for an exception instance) the scripted items in
    order, then repeats the last one forever."""

    def __init__(self, *items):
        self.items = list(items)
        self.calls = 0

    def __call__(self):
        item = self.items[min(self.calls, len(self.items) - 1)]
        self.calls += 1
        if isinstance(item, BaseException):
            raise item
        return item


class AwaitLandingLoopTests(unittest.TestCase):
    def _await(self, is_landed, merge_status, *, budget=540, interval=30):
        clock = FakeClock()
        outcome = aml.await_landing(
            is_landed, merge_status, budget_seconds=budget,
            interval_seconds=interval, clock=clock.now, sleep=clock.sleep,
        )
        return outcome, clock

    def test_landed_on_first_round_short_circuits_merge_status(self):
        merge_status = ScriptedProbe({"state": "queued"})
        outcome, clock = self._await(ScriptedProbe(True), merge_status)
        self.assertIs(outcome.verdict, Verdict.LANDED)
        self.assertEqual(outcome.source, "git_ancestry")
        self.assertEqual(outcome.polls, 1)
        self.assertEqual(clock.sleeps, [])
        self.assertEqual(merge_status.calls, 0)

    def test_lands_after_two_waiting_rounds(self):
        outcome, clock = self._await(ScriptedProbe(False, False, True),
                                     ScriptedProbe({"state": "verifying"}))
        self.assertIs(outcome.verdict, Verdict.LANDED)
        self.assertEqual(outcome.source, "git_ancestry")
        self.assertEqual(outcome.polls, 3)
        self.assertEqual(clock.sleeps, [30, 30])

    def test_terminal_merge_status_ends_the_wait(self):
        for state, verdict in (("conflict", Verdict.BLOCKED),
                               ("abandoned", Verdict.FAILED)):
            with self.subTest(state=state):
                reply = {"state": state, "request_id": "mr-1"}
                outcome, _ = self._await(ScriptedProbe(False), ScriptedProbe(reply))
                self.assertIs(outcome.verdict, verdict)
                self.assertEqual(outcome.source, "merge_status")
                self.assertEqual(outcome.merge_status, reply)
                self.assertEqual(outcome.polls, 1)

    def test_done_lands_even_when_commit_is_not_an_ancestor(self):
        reply = {"state": "done", "merge_sha": "abc"}
        outcome, _ = self._await(ScriptedProbe(False), ScriptedProbe(reply))
        self.assertIs(outcome.verdict, Verdict.LANDED)
        self.assertEqual(outcome.source, "merge_status")
        self.assertEqual(outcome.merge_status, reply)

    def test_ends_exactly_at_the_budget_and_never_starts_a_round_after_it(self):
        outcome, clock = self._await(ScriptedProbe(False),
                                     ScriptedProbe({"state": "verifying"}),
                                     budget=540, interval=30)
        self.assertIs(outcome.verdict, Verdict.PENDING)
        self.assertIsNone(outcome.source)
        self.assertEqual(clock.t, 540)
        self.assertEqual(outcome.elapsed_seconds, 540)
        self.assertEqual(sum(clock.sleeps), 540)
        self.assertTrue(all(s <= 30 for s in clock.sleeps), clock.sleeps)
        self.assertEqual(outcome.polls, 19)

    def test_final_sleep_is_truncated_to_the_remaining_budget(self):
        outcome, clock = self._await(ScriptedProbe(False),
                                     ScriptedProbe({"state": "verifying"}),
                                     budget=545, interval=30)
        self.assertIs(outcome.verdict, Verdict.PENDING)
        self.assertEqual(clock.sleeps[-1], 5)
        self.assertEqual(clock.t, 545)
        self.assertEqual(outcome.elapsed_seconds, 545)
        self.assertEqual(outcome.polls, 20)

    def test_zero_budget_is_exactly_one_round(self):
        outcome, clock = self._await(ScriptedProbe(False),
                                     ScriptedProbe({"state": "queued"}), budget=0)
        self.assertIs(outcome.verdict, Verdict.PENDING)
        self.assertEqual(outcome.polls, 1)
        self.assertEqual(clock.sleeps, [])
        self.assertEqual(outcome.merge_status, {"state": "queued"})

    def test_unavailable_merge_status_degrades_to_pending(self):
        down = aml.ProbeUnavailable("merge_status: connection refused")
        outcome, _ = self._await(ScriptedProbe(False), ScriptedProbe(down), budget=60)
        self.assertIs(outcome.verdict, Verdict.PENDING)
        self.assertIn("connection refused", outcome.probe_error)
        self.assertIsNone(outcome.merge_status)
        self.assertEqual(outcome.polls, 3)

    def test_transient_probe_error_does_not_end_the_wait(self):
        flaky = ScriptedProbe(aml.ProbeUnavailable("git: index.lock"), True)
        outcome, _ = self._await(flaky, ScriptedProbe({"state": "verifying"}))
        self.assertIs(outcome.verdict, Verdict.LANDED)
        self.assertEqual(outcome.source, "git_ancestry")
        self.assertEqual(outcome.polls, 2)

    def test_unknown_state_is_not_terminal(self):
        outcome, _ = self._await(ScriptedProbe(False),
                                 ScriptedProbe({"state": "unknown"}), budget=90)
        self.assertIs(outcome.verdict, Verdict.PENDING)
        self.assertEqual(outcome.merge_status, {"state": "unknown"})
        self.assertEqual(outcome.polls, 4)


if __name__ == "__main__":
    unittest.main()
