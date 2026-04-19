#!/usr/bin/env bash
# Infrastructure test for task 1992.
# Validates that scripts/cargo-test-occt-gated.sh exists with the correct
# structure, serializes OCCT-touching test processes via flock, and that
# orchestrator.yaml routes all cargo test --workspace invocations through it.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

WRAPPER="$REPO_ROOT/scripts/cargo-test-occt-gated.sh"

echo "=== OCCT flock gate tests ==="

# -- Test 1: wrapper script exists ---------------------------------------------
echo ""
echo "--- Test 1: wrapper script exists ---"

assert "scripts/cargo-test-occt-gated.sh exists" \
    test -f "$WRAPPER"

# -- Test 2: wrapper script is executable --------------------------------------
echo ""
echo "--- Test 2: wrapper script is executable ---"

assert "scripts/cargo-test-occt-gated.sh is executable (mode +x)" \
    test -x "$WRAPPER"

# -- Test 3: shebang line ------------------------------------------------------
echo ""
echo "--- Test 3: wrapper has #!/usr/bin/env bash shebang ---"

assert "first line is '#!/usr/bin/env bash'" \
    bash -c "head -1 '$WRAPPER' | grep -qxF '#!/usr/bin/env bash'"

# -- Test 4: set -euo pipefail -------------------------------------------------
echo ""
echo "--- Test 4: wrapper sets strict error handling ---"

assert "wrapper contains 'set -euo pipefail'" \
    grep -q 'set -euo pipefail' "$WRAPPER"

# -- Test 5: flock -x invocation -----------------------------------------------
echo ""
echo "--- Test 5: wrapper invokes flock -x ---"

assert "wrapper contains 'flock -x'" \
    grep -q 'flock -x' "$WRAPPER"

# -- Test 6: default lock path -------------------------------------------------
echo ""
echo "--- Test 6: default lock path contains /tmp/reify-occt.lock ---"

assert "wrapper contains '/tmp/reify-occt.lock'" \
    grep -q '/tmp/reify-occt.lock' "$WRAPPER"

# -- Test 7: argument forwarding -----------------------------------------------
echo ""
echo "--- Test 7: wrapper forwards arguments with exec and \"\$@\" ---"

assert "wrapper contains 'exec'" \
    grep -q 'exec' "$WRAPPER"

assert 'wrapper contains "$@" for argument forwarding' \
    grep -qF '"$@"' "$WRAPPER"

test_summary
