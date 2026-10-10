#!/usr/bin/env bash
# Infrastructure test for task 8455 (the pre-registered E3 §7 analysis under
# docs/prds/v0_6/e3-granularity/analysis/: e3_protocol, e3_stats, e3_sources,
# e3_model and the e3_analysis CLI).
#
# This wrapper is the member run_all.sh actually discovers: it globs
# `test_*.sh` only, mirrored by classification_discovered_set in
# run-all-classification-lib.sh. The real assertions live in the sibling .py;
# without this file they would be silently never run, reading as coverage
# while asserting nothing.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_e3_analysis.py (stdlib unittest) exits 0
#   3. docs/prds/v0_6/e3-granularity/analysis/e3_analysis.py --help exits 0
#      (CLI smoke)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_e3_analysis ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── Unit tests ────────────────────────────────────────────────────────────
assert "tests/infra/test_e3_analysis.py exits 0" \
    python3 "$SCRIPT_DIR/test_e3_analysis.py"

# ── CLI smoke ─────────────────────────────────────────────────────────────
assert "docs/prds/v0_6/e3-granularity/analysis/e3_analysis.py --help exits 0" \
    python3 "$ROOT/docs/prds/v0_6/e3-granularity/analysis/e3_analysis.py" --help

test_summary
