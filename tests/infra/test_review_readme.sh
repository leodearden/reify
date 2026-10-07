#!/usr/bin/env bash
# Infrastructure test for task 7790 (scripts/review-readme.sh, the timer-driven
# README/getting-started reviewer that commits to the main checkout unwatched).
#
# This wrapper is the member run_all.sh actually discovers: it globs
# `test_*.sh` only, mirrored by classification_discovered_set in
# run-all-classification-lib.sh. The real assertions live in the sibling .py;
# without this file they would be silently never run, reading as coverage
# while asserting nothing.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_review_readme.py (stdlib unittest) exits 0
#   3. scripts/review-readme.sh parses (bash -n smoke)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_review_readme ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── Unit tests ────────────────────────────────────────────────────────────
assert "tests/infra/test_review_readme.py exits 0" \
    python3 "$SCRIPT_DIR/test_review_readme.py"

# ── Parse smoke ───────────────────────────────────────────────────────────
assert "scripts/review-readme.sh parses (bash -n)" \
    bash -n "$ROOT/scripts/review-readme.sh"

test_summary
