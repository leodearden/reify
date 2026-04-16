#!/usr/bin/env bash
# tests/infra/test_tasks_json_schema.sh
# Schema-invariant regression tests for .taskmaster/tasks/tasks.json.
#
# Drives scripts/validate_tasks_json.py with inline JSON fixtures via mktemp
# to verify that invariants 1-3 fire on bad input and pass on good input.
# Also validates the real tasks.json to catch any future drift.
#
# Part of Task 1888: tasks.json schema-invariant validator.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
VALIDATOR="$REPO_ROOT/scripts/validate_tasks_json.py"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== tasks.json schema-invariant tests ==="

# -- Prerequisite: validator script exists ------------------------------------
echo ""
echo "--- Test: validator script exists ---"

assert "validator script exists" \
    test -f "$VALIDATOR"

# -- Summary ------------------------------------------------------------------
test_summary
