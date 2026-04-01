#!/usr/bin/env bash
# Unit tests for portable_timeout() from scripts/lib_portable.sh.
# Tests that the function runs commands with a timeout, using the 3-tier
# strategy: GNU timeout -> gtimeout -> background+sleep+kill fallback.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
LIB_PORTABLE="$REPO_ROOT/scripts/lib_portable.sh"

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

echo "=== portable_timeout unit tests ==="

# -- Test 1: portable_timeout is defined after sourcing -----------------------
echo ""
echo "--- Test 1: portable_timeout function defined ---"

assert "portable_timeout function is defined after sourcing" \
    bash -c "source '$LIB_PORTABLE' && declare -f portable_timeout >/dev/null"

# -- Test 2: runs a fast command successfully ---------------------------------
echo ""
echo "--- Test 2: fast command succeeds ---"

assert "portable_timeout 5 true returns 0" \
    bash -c "source '$LIB_PORTABLE' && portable_timeout 5 true"

# -- Test 3: returns non-zero for a failing command ---------------------------
echo ""
echo "--- Test 3: failing command returns non-zero ---"

assert "portable_timeout 5 false returns non-zero" \
    bash -c "source '$LIB_PORTABLE' && ! portable_timeout 5 false"

# -- Test 4: returns 124 when command exceeds timeout -------------------------
echo ""
echo "--- Test 4: timeout returns exit code 124 ---"

assert "portable_timeout 1 sleep 10 returns 124" \
    bash -c "source '$LIB_PORTABLE' && rc=0; portable_timeout 1 sleep 10 || rc=\$?; [ \"\$rc\" -eq 124 ]"

# -- Test 5: preserves command exit code for non-timeout failures -------------
echo ""
echo "--- Test 5: preserves command exit code ---"

# bash -c 'exit 42' should give exit code 42, not 124.
assert "portable_timeout preserves non-timeout exit code" \
    bash -c "source '$LIB_PORTABLE' && rc=0; portable_timeout 5 bash -c 'exit 42' || rc=\$?; [ \"\$rc\" -eq 42 ]"

# -- Test 6: runs a command that produces output ------------------------------
echo ""
echo "--- Test 6: command output is passed through ---"

assert "portable_timeout passes through stdout" \
    bash -c "source '$LIB_PORTABLE' && out=\$(portable_timeout 5 echo hello) && [ \"\$out\" = 'hello' ]"

# -- Test 7: _PORTABLE_TIMEOUT_TIMED_OUT is true after genuine timeout --------
echo ""
echo "--- Test 7: _PORTABLE_TIMEOUT_TIMED_OUT set to true on genuine timeout ---"

assert "_PORTABLE_TIMEOUT_TIMED_OUT is true after genuine timeout" \
    bash -c "source '$LIB_PORTABLE' && portable_timeout 1 sleep 10 || true; [ \"\$_PORTABLE_TIMEOUT_TIMED_OUT\" = 'true' ]"

# -- Test 8: _PORTABLE_TIMEOUT_TIMED_OUT is false for natural exit 124 --------
echo ""
echo "--- Test 8: _PORTABLE_TIMEOUT_TIMED_OUT false on natural exit 124 ---"

assert "_PORTABLE_TIMEOUT_TIMED_OUT is false when command exits 124 naturally" \
    bash -c "source '$LIB_PORTABLE' && portable_timeout 5 bash -c 'exit 124' || true; [ \"\$_PORTABLE_TIMEOUT_TIMED_OUT\" = 'false' ]"

# -- Test 9: _PORTABLE_TIMEOUT_TIMED_OUT is false for success and failure -----
echo ""
echo "--- Test 9: _PORTABLE_TIMEOUT_TIMED_OUT false on success and normal failure ---"

assert "_PORTABLE_TIMEOUT_TIMED_OUT is false when command succeeds" \
    bash -c "source '$LIB_PORTABLE' && portable_timeout 5 true; [ \"\$_PORTABLE_TIMEOUT_TIMED_OUT\" = 'false' ]"

assert "_PORTABLE_TIMEOUT_TIMED_OUT is false when command fails normally" \
    bash -c "source '$LIB_PORTABLE' && portable_timeout 5 false || true; [ \"\$_PORTABLE_TIMEOUT_TIMED_OUT\" = 'false' ]"

# -- Summary ------------------------------------------------------------------
echo ""
echo "Results: $PASS passed, $FAIL failed"
if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
