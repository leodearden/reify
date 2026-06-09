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


if __name__ == "__main__":
    unittest.main()
