#!/usr/bin/env bash
# Infrastructure test for task 8685: the REIFY_VERIFY_EVENT_LOG verify event
# channel (PRD docs/prds/v0_6/run-all-runner-infra-harness.md §4).
#
# This wrapper is the member run_all.sh actually discovers (it globs
# `test_*.sh` only); the assertions live in the sibling .py, which drives the
# reader tests/infra/verify_events.py, the encoder scripts/lib_event_log.sh,
# slot_acquire's deadline callers and the real tests/infra/run_all.sh on
# tempdir fixtures.
#
# CLASSIFIED `pool`: every lock, manifest, ledger and channel file it uses is
# private to its own tempdirs, like test_run_all_timing.sh.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_verify_events.py (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_verify_events ==="

assert "python3 is available" command -v python3

assert "tests/infra/test_verify_events.py exits 0" \
    python3 "$SCRIPT_DIR/test_verify_events.py"

test_summary
