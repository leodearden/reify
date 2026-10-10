#!/usr/bin/env python3
"""
test_e3_analysis.py — stdlib unittest for the pre-registered E3 §7 analysis
(docs/prds/v0_6/e3-granularity/analysis/: e3_protocol, e3_stats, e3_sources,
e3_model, e3_analysis).

WHAT RUNS THIS: not the gate directly. run_all.sh discovers `test_*.sh` only,
so the discovered member is the thin wrapper tests/infra/test_e3_analysis.sh,
which invokes this file. A bare .py here would be silently never run.

IMPORT IDIOM: tests/infra/ (for the hermetic fixture builders in
e3_analysis_fixtures.py) and the analysis directory are prepended to sys.path,
so the modules import by their plain names without installation.

Every fixture lives under a tempfile dir: no network, no shared state, no
live runs.db, task store or main ref.
"""

import sys
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
INFRA_DIR = REPO_ROOT / "tests" / "infra"
ANALYSIS_DIR = REPO_ROOT / "docs" / "prds" / "v0_6" / "e3-granularity" / "analysis"

sys.path.insert(0, str(INFRA_DIR))
sys.path.insert(0, str(ANALYSIS_DIR))


if __name__ == "__main__":
    unittest.main()
