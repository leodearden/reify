#!/usr/bin/env bash
# Unit tests for scripts/lib.sh shared library.
# Tests that lib.sh is sourceable and provides expected utilities.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LIB_FILE="$SCRIPT_DIR/lib.sh"

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

echo "=== lib.sh unit tests ==="

# ── Test 1: lib.sh exists ─────────────────────────────────────────
echo ""
echo "--- Test 1: lib.sh exists ---"

assert "lib.sh file exists" \
    test -f "$LIB_FILE"

# ── Test 2: lib.sh is sourceable ──────────────────────────────────
echo ""
echo "--- Test 2: lib.sh is sourceable ---"

assert "lib.sh can be sourced without error" \
    bash -c "source '$LIB_FILE'"

# ── Test 3: compute_sha256 is defined after sourcing ─────────────
echo ""
echo "--- Test 3: compute_sha256 function defined ---"

assert "compute_sha256 function is defined after sourcing lib.sh" \
    bash -c "source '$LIB_FILE' && declare -f compute_sha256 >/dev/null"

# ── Test 4: compute_sha256 produces a 64-char hex hash ───────────
echo ""
echo "--- Test 4: compute_sha256 produces correct output ---"

# Use lib.sh itself as a known test file.
assert "compute_sha256 produces a 64-char hex hash" \
    bash -c "source '$LIB_FILE' && hash=\$(compute_sha256 '$LIB_FILE' | awk '{print \$1}') && [[ \"\$hash\" =~ ^[0-9a-f]{64}$ ]]"

# ── Summary ───────────────────────────────────────────────────────
echo ""
echo "Results: $PASS passed, $FAIL failed"
if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
