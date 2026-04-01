#!/usr/bin/env bash
# Shared test helpers for reify shell test files.
# Provides assert() and test_summary() with PASS/FAIL counters.
#
# Usage:  source "$(dirname "${BASH_SOURCE[0]}")/test_helpers.sh"
#   or:   source "$REPO_ROOT/tests/infra/test_helpers.sh"

# Source guard — prevent double-sourcing.
if [ "${_REIFY_TEST_HELPERS_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_TEST_HELPERS_SH_SOURCED=1

PASS=0
FAIL=0

assert() {
    local desc="$1"
    shift
    if "$@" >/dev/null 2>&1; then
        echo "  PASS: $desc"
        PASS=$((PASS + 1))
    else
        echo "  FAIL: $desc"
        FAIL=$((FAIL + 1))
    fi
}

test_summary() {
    echo ""
    echo "Results: $PASS passed, $FAIL failed"
    if [ "$FAIL" -gt 0 ]; then
        exit 1
    fi
}
