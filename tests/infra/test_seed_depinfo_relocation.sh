#!/usr/bin/env bash
# Infrastructure test for task 10167 (seeded lanes relocate cargo dep-info input
# paths recorded under the base build root).
#
# This wrapper is the member run_all.sh actually discovers (it globs
# `test_*.sh` only); the assertions live in the sibling .py, which EXECUTES the
# real scripts/seed-warm-lane.sh with real git and real cargo on a tempdir
# fixture workspace, then asks cargo whether the seeded lane's units are fresh.
#
# CLASSIFIED `pool` like test_verify_ledger_lane_seed.sh: every repo, base, lane
# and stub is private to the run, cargo runs with a private CARGO_HOME, and seed
# runs with REIFY_WARM_LANE_RERERE_ARM=0 so it never touches the shared store's
# config. Host load can slow it, never change its answer.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_seed_depinfo_relocation.py (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_seed_depinfo_relocation ==="

assert "python3 is available" command -v python3

assert "tests/infra/test_seed_depinfo_relocation.py exits 0" \
    python3 "$SCRIPT_DIR/test_seed_depinfo_relocation.py"

test_summary
