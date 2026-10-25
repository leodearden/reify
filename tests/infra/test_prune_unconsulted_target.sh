#!/usr/bin/env bash
# Infrastructure test for task 9246 (scripts/prune-unconsulted-target.sh, the
# fingerprint-consult liveness trim shared by refresh-warm-base.sh and the
# leo-laptop _merge-verify trim timer).
#
# This wrapper is the member run_all.sh actually discovers: it globs
# `test_*.sh` only. The real assertions live in the sibling .py; without this
# file they would be silently never run, reading as coverage while asserting
# nothing.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_prune_unconsulted_target.py (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_prune_unconsulted_target ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── Unit tests ────────────────────────────────────────────────────────────
assert "tests/infra/test_prune_unconsulted_target.py exits 0" \
    python3 "$SCRIPT_DIR/test_prune_unconsulted_target.py"

test_summary
