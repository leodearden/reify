#!/usr/bin/env bash
# Tests for scripts/jobserver-acceptance.py — the end-to-end mixed-load
# acceptance gate for the dual-FIFO jobserver priority balancer
# (task η/4521, PRD §9 leaf, docs/prds/jobserver-merge-priority-balancer.md).
#
# ALL tests here are HERMETIC: mktemp FIFOs, importlib-loaded Python stubs,
# PATH-stubbed systemctl where needed.  The real ~tens-of-minutes A/B campaign
# lives behind the harness's `--run` mode (capstone step-13), never here.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

ACCEPT="$REPO_ROOT/scripts/jobserver-acceptance.py"
SETUP_DEV="$REPO_ROOT/scripts/setup-dev.sh"

[ -f "$ACCEPT" ] || { echo "ERROR: $ACCEPT not found"; exit 1; }
[ -f "$SETUP_DEV" ] || { echo "ERROR: $SETUP_DEV not found"; exit 1; }

# Verify the acceptance harness loads without error (importlib + argparse).
assert "jobserver-acceptance.py loads without error (--help exits 0)" \
    python3 "$ACCEPT" --help

# ── Blocks will be added by steps 03, 05, 07, 09, 11 ──────────────────────

test_summary
