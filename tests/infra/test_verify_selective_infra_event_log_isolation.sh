#!/usr/bin/env bash
# Infrastructure test for task #8919 (C4 outside run_all: verify.sh's
# selective-infra leaf spawns each member with REIFY_VERIFY_EVENT_LOG unset).
#
# This wrapper is the member run_all.sh actually discovers (it globs
# `test_*.sh` only); the assertions live in the sibling .py, which derives the
# leaf from the real verify.sh --print-plan and executes it against a probe.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_verify_selective_infra_event_log_isolation.py
#      (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_verify_selective_infra_event_log_isolation ==="

assert "python3 is available" command -v python3

assert "tests/infra/test_verify_selective_infra_event_log_isolation.py exits 0" \
    python3 "$SCRIPT_DIR/test_verify_selective_infra_event_log_isolation.py"

test_summary
