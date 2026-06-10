#!/usr/bin/env bash
# Unit tests for portable_mtime() from scripts/lib_portable.sh.
# Tests that lib_portable.sh is sourceable and portable_mtime returns
# the file's modification time as a Unix epoch integer, portably
# (GNU stat -c %Y / BSD stat -f %m).
#
# Mirrors the structure of tests/infra/test_portable_sha256.sh.
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
LIB_PORTABLE="$REPO_ROOT/scripts/lib_portable.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== portable_mtime unit tests ==="

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

# -- Test 3: portable_mtime is defined after sourcing -------------------------
echo ""
echo "--- Test 3: portable_mtime function defined ---"

assert "portable_mtime function is defined after sourcing" \
    bash -c "source '$LIB_PORTABLE' && declare -f portable_mtime >/dev/null"

# -- Test 4: portable_mtime produces a positive integer epoch -----------------
echo ""
echo "--- Test 4: portable_mtime produces a positive integer epoch ---"

# Use lib_portable.sh itself as a known test file.
assert "portable_mtime produces a positive integer epoch" \
    bash -c "source '$LIB_PORTABLE' && mtime=\$(portable_mtime '$LIB_PORTABLE') && [[ \"\$mtime\" =~ ^[0-9]+\$ ]] && [ \"\$mtime\" -gt 0 ]"

# -- Test 5: mtime matches GNU stat -c %Y output (skip if BSD only) -----------
echo ""
echo "--- Test 5: mtime matches system stat output (GNU path) ---"

if stat -c %Y "$LIB_PORTABLE" >/dev/null 2>&1; then
    EXPECTED_MTIME=$(stat -c %Y "$LIB_PORTABLE")
    assert "portable_mtime matches GNU stat -c %Y output" \
        bash -c "source '$LIB_PORTABLE' && mtime=\$(portable_mtime '$LIB_PORTABLE') && [ \"\$mtime\" = '$EXPECTED_MTIME' ]"
else
    echo "  SKIP: GNU stat -c %Y not available (BSD stat detected)"
fi

# -- Test 6: portable_mtime returns non-zero for a missing file ---------------
echo ""
echo "--- Test 6: portable_mtime returns non-zero for a missing file ---"

assert "portable_mtime returns non-zero for a missing file" \
    bash -c "source '$LIB_PORTABLE' && ! portable_mtime '/tmp/nonexistent-file-portable-mtime-test-$$' 2>/dev/null"

# -- Test 7: source guard prevents double-sourcing ----------------------------
echo ""
echo "--- Test 7: source guard prevents double-sourcing ---"

assert "sourcing lib_portable.sh twice succeeds (source guard)" \
    bash -c "source '$LIB_PORTABLE' && source '$LIB_PORTABLE'"

# -- Summary ------------------------------------------------------------------
test_summary
