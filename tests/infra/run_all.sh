#!/usr/bin/env bash
# tests/infra/run_all.sh — discovers and runs all test_*.sh files.
#
# Usage: run_all.sh [INFRA_DIR]
#
#   INFRA_DIR  Directory to search for test_*.sh files.
#              Defaults to the directory containing this script.
#
# Auto-discovery: all files matching test_*.sh in INFRA_DIR are discovered
# and run as subshell invocations. test_helpers.sh is excluded by name
# (it is a shared library, not a test runner).
#
# Exits 0 if all discovered tests pass (or none are found), 1 if any fail.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INFRA_DIR="${1:-$SCRIPT_DIR}"

failures=0
discovered=0

echo "=== Running all infra tests in: $INFRA_DIR ==="

for test_file in "$INFRA_DIR"/test_*.sh; do
    # If glob matches nothing, the literal pattern string is returned — skip it.
    [ -f "$test_file" ] || continue

    # Exclude test_helpers.sh — shared library, not a test runner.
    basename="$(basename "$test_file")"
    if [ "$basename" = "test_helpers.sh" ]; then
        continue
    fi

    discovered=$((discovered + 1))
    echo ""
    echo "--- Running: $basename ---"
    if bash "$test_file"; then
        echo "  RESULT: PASS ($basename)"
    else
        echo "  RESULT: FAIL ($basename)"
        failures=$((failures + 1))
    fi
done

echo ""
echo "=== Summary: $discovered discovered, $failures failed ==="

if [ "$failures" -eq 0 ]; then
    exit 0
else
    exit 1
fi
