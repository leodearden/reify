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


if __name__ == "__main__":
    unittest.main()
