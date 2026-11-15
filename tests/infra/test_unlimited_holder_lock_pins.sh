#!/usr/bin/env bash
# Infrastructure test for task 9995: the exclusive module_overrides carve-outs
# from dark-factory-orchestrator.yaml's unlimited_holder_locks rule are lock
# keys at lock_depth and pin every ancestor directory key.
#
# This wrapper is the member run_all.sh discovers (it globs `test_*.sh` only)
# and the one that holds the run-all-classification.manifest row. The
# assertions live in the sibling tests/infra/test_unlimited_holder_lock_pins.py.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_unlimited_holder_lock_pins.py (stdlib unittest) exits 0
# PyYAML is the system package, not a hard dependency: without it the suite
# SKIPs, as the sibling yaml-contract tests (test_cpu_governance_config.sh) do.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_unlimited_holder_lock_pins ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── The suite ──────────────────────────────────────────────────────────────
if ! python3 -c 'import yaml' 2>/dev/null; then
    echo "SKIP: python3 'yaml' (PyYAML) not available; skipping the lock-pin suite"
else
    assert "tests/infra/test_unlimited_holder_lock_pins.py exits 0" \
        python3 "$SCRIPT_DIR/test_unlimited_holder_lock_pins.py"
fi

test_summary
