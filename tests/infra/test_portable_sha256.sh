#!/usr/bin/env bash
# Unit tests for portable_sha256() from scripts/lib_portable.sh.
# Tests that lib_portable.sh is sourceable and portable_sha256 produces
# correct SHA-256 hashes across platforms (sha256sum / shasum fallback).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
LIB_PORTABLE="$REPO_ROOT/scripts/lib_portable.sh"

source "$SCRIPT_DIR/test_helpers.sh"

echo "=== portable_sha256 unit tests ==="

# -- Test 1: lib_portable.sh exists ------------------------------------------
echo ""
echo "--- Test 1: lib_portable.sh exists ---"

assert "lib_portable.sh file exists" \
    test -f "$LIB_PORTABLE"

# -- Test 2: lib_portable.sh is sourceable ------------------------------------
echo ""
echo "--- Test 2: lib_portable.sh is sourceable ---"

assert "lib_portable.sh can be sourced without error" \
    bash -c "source '$LIB_PORTABLE'"

# -- Test 3: portable_sha256 is defined after sourcing ------------------------
echo ""
echo "--- Test 3: portable_sha256 function defined ---"

assert "portable_sha256 function is defined after sourcing" \
    bash -c "source '$LIB_PORTABLE' && declare -f portable_sha256 >/dev/null"

# -- Test 4: portable_sha256 produces a 64-char hex hash ----------------------
echo ""
echo "--- Test 4: portable_sha256 produces correct output ---"

# Use lib_portable.sh itself as a known test file.
assert "portable_sha256 produces a 64-char hex hash" \
    bash -c "source '$LIB_PORTABLE' && hash=\$(portable_sha256 '$LIB_PORTABLE' | awk '{print \$1}') && [[ \"\$hash\" =~ ^[0-9a-f]{64}$ ]]"

# -- Test 5: hash matches sha256sum/shasum output -----------------------------
echo ""
echo "--- Test 5: hash matches system sha256 tool ---"

# Compute expected hash using whatever system tool is available.
# Guard against lib_portable.sh not existing yet (TDD: test written before impl).
EXPECTED_HASH=""
if [ -f "$LIB_PORTABLE" ]; then
    if command -v sha256sum >/dev/null 2>&1; then
        EXPECTED_HASH=$(sha256sum "$LIB_PORTABLE" | awk '{print $1}')
    elif command -v shasum >/dev/null 2>&1; then
        EXPECTED_HASH=$(shasum -a 256 "$LIB_PORTABLE" | awk '{print $1}')
    else
        echo "  SKIP: no sha256sum or shasum available"
    fi
fi

if [ -n "$EXPECTED_HASH" ]; then
    assert "portable_sha256 hash matches system tool output" \
        bash -c "source '$LIB_PORTABLE' && hash=\$(portable_sha256 '$LIB_PORTABLE' | awk '{print \$1}') && [ \"\$hash\" = '$EXPECTED_HASH' ]"
fi

# -- Test 6: source guard prevents double-sourcing ----------------------------
echo ""
echo "--- Test 6: source guard prevents double-sourcing ---"

assert "sourcing lib_portable.sh twice succeeds (source guard)" \
    bash -c "source '$LIB_PORTABLE' && source '$LIB_PORTABLE'"

# -- Summary ------------------------------------------------------------------
test_summary
