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

# Hermetic-harness isolation: normalize DF_VERIFY_ROLE to 'task' for the whole
# suite run. The dark-factory post-merge gate stamps DF_VERIFY_ROLE=merge and
# runs the infra suites as one of its plan lines (verify.sh: bash run_all.sh),
# so without this every child inherits role=merge. Several suites
# (test_verify_throughput.sh, test_verify_scope.sh, test_scope_boundary.sh,
# test_verify_gui_feature_check.sh) are meta-tests that drive their own hermetic
# `verify.sh --scope branch/staged --print-plan` fixtures to assert narrowing
# behavior; under an inherited role=merge, verify.sh's contract-C2 guard
# ("merge gate never narrows") force-rewrites their scope to 'all', collapsing
# every scope=branch assertion (observed: throughput 24/14, scope 72/33).
# Pinning role=task here makes the meta-tests hermetic. Suites that deliberately
# exercise merge-role behavior set `DF_VERIFY_ROLE=merge` inline per command,
# and that per-command assignment still overrides this exported default.
export DF_VERIFY_ROLE=task

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
