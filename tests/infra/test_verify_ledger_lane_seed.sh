#!/usr/bin/env bash
# Infrastructure test for task 7424 (lane seed stamp + agent cargo start log).
#
# This wrapper is the member run_all.sh actually discovers (it globs
# `test_*.sh` only); the assertions live in the sibling .py, which EXECUTES the
# real scripts/seed-warm-lane.sh on a tempdir fixture lane and the real
# scripts/agent-bin/cargo with a stub real cargo.
#
# CLASSIFIED `pool` like test_seed_warm_lane.sh: every lane, base and stub is
# private to the run, and seed runs with REIFY_WARM_LANE_RERERE_ARM=0 so it
# never touches the shared store's config.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_verify_ledger_lane_seed.py (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_verify_ledger_lane_seed ==="

assert "python3 is available" command -v python3

assert "tests/infra/test_verify_ledger_lane_seed.py exits 0" \
    python3 "$SCRIPT_DIR/test_verify_ledger_lane_seed.py"

test_summary
