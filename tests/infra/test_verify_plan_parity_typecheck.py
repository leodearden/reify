#!/usr/bin/env python3
"""
test_verify_plan_parity_typecheck.py — B1's typecheck grid shard: bash
`scripts/verify.sh typecheck` and Python `scripts/verify.py typecheck` print the
same plan in every cell of PRD D13's grid (S18 γ1, task 9098; PRD
docs/prds/v0_6/verify-structured-plan-and-role-table.md D8, D13).

The grid is role {unset, merge, offline, background} × profile {implicit,
debug, release, both} × scope {all, staged, branch}, each with
--include-infra, in two clone states: clean, and a staged change to a crate
on a started branch. Each cell compares exit code, stdout and stderr byte for
byte, as a subTest; the cells of one state run concurrently, since a
--print-plan never mutates the clone. The grid and the runner live in
tests/infra/plan_parity_harness.py; the core member is
test_verify_plan_parity.py.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its
thin wrapper tests/infra/test_verify_plan_parity_typecheck.sh.
"""

import sys
import unittest
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(INFRA_DIR))
import plan_parity_harness as harness  # noqa: E402


def tearDownModule():
    harness.discard_clone()


class TestTypecheckGridParity(harness.GridParity):
    ACTION = "typecheck"

    def test_clean_tree(self):
        self.assert_grid()

    def test_staged_crate_change(self):
        self.assert_staged_change_grid()


if __name__ == "__main__":
    unittest.main(verbosity=2)
