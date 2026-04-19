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


# -- Test 8: serialization (REIFY_OCCT_LOCK override) --------------------------
echo ""
echo "--- Test 8: wrapper serializes two concurrent invocations ---"

_LOCK_FILE="$(mktemp)"
_START_NS="$(date +%s%N)"

# Spawn two concurrent invocations each sleeping 0.4s.
REIFY_OCCT_LOCK="$_LOCK_FILE" "$WRAPPER" bash -c 'sleep 0.4' &
_PID1=$!
REIFY_OCCT_LOCK="$_LOCK_FILE" "$WRAPPER" bash -c 'sleep 0.4' &
_PID2=$!
wait "$_PID1" "$_PID2"

_END_NS="$(date +%s%N)"
_ELAPSED_MS=$(( (_END_NS - _START_NS) / 1000000 ))

rm -f "$_LOCK_FILE"

# Parallel would finish in ~400ms; serialized takes >=700ms.
assert "two 0.4s sleep invocations run serially (elapsed >= 700ms, got ${_ELAPSED_MS}ms)" \
    test "$_ELAPSED_MS" -ge 700

# -- Test 9: exit-code propagation ----------------------------------------------
echo ""
echo "--- Test 9: wrapper propagates exit code of wrapped command ---"

_TMP_LOCK="$(mktemp)"
_EC=0
REIFY_OCCT_LOCK="$_TMP_LOCK" "$WRAPPER" bash -c 'exit 42' || _EC=$?
rm -f "$_TMP_LOCK"

assert "wrapper exit code is 42 (got $_EC)" \
    test "$_EC" -eq 42

test_summary
