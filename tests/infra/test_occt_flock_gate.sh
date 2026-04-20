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
echo "--- Test 6: default lock path is user-scoped ---"

assert "default lock path is user-scoped via 'id -u'" \
    grep -q 'reify-occt-$(id -u)' "$WRAPPER"

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


# -- Orchestrator integration tests --------------------------------------------
ORCH="$REPO_ROOT/orchestrator.yaml"

echo ""
echo "--- Test 10: debug pass is gated by cargo-test-occt-gated.sh ---"

assert "test_command contains './scripts/cargo-test-occt-gated.sh cargo test --workspace -- --test-threads=1'" \
    bash -c "grep 'test_command:' '$ORCH' | grep -qF './scripts/cargo-test-occt-gated.sh cargo test --workspace -- --test-threads=1'"

echo ""
echo "--- Test 11: release pass is gated by cargo-test-occt-gated.sh ---"

assert "test_command contains './scripts/cargo-test-occt-gated.sh cargo test --workspace --release -- --test-threads=1'" \
    bash -c "grep 'test_command:' '$ORCH' | grep -qF './scripts/cargo-test-occt-gated.sh cargo test --workspace --release -- --test-threads=1'"

echo ""
echo "--- Test 12: no bare ungated 'cargo test --workspace' in test_command ---"

# All occurrences of 'cargo test --workspace' must be preceded by the gate script.
# Extract the test_command line; replace gated occurrences with a placeholder;
# then assert no bare 'cargo test --workspace' remains.
assert "no bare 'cargo test --workspace' without gate in test_command" \
    bash -c "
        LINE=\$(grep 'test_command:' '$ORCH')
        # Remove gated occurrences; anything left is ungated.
        STRIPPED=\$(echo \"\$LINE\" | sed 's|[^ ]*/cargo-test-occt-gated\.sh cargo test --workspace||g')
        ! echo \"\$STRIPPED\" | grep -q 'cargo test --workspace'
    "

echo ""
echo "--- Test 13: --workspace flag preserved under gate (coverage assertion) ---"

assert "gated debug invocation contains --workspace" \
    bash -c "grep 'test_command:' '$ORCH' | grep -qF 'cargo-test-occt-gated.sh cargo test --workspace'"

# -- Test 14: bounded lock-wait exits non-zero with clear message ---------------
echo ""
echo "--- Test 14: REIFY_OCCT_LOCK_WAIT=1 fires within budget, exits non-zero with message ---"

_LOCK14="$(mktemp)"
_ERR14="$(mktemp)"

# Spawn a background holder that acquires the lock and holds it for 10s.
( flock -x 9; sleep 10 ) 9>>"$_LOCK14" &
_HOLDER14=$!
sleep 0.2  # give the holder time to acquire before we proceed

_START14="$(date +%s)"
_EXIT14=0
REIFY_OCCT_LOCK="$_LOCK14" REIFY_OCCT_LOCK_WAIT=1 timeout 5 "$WRAPPER" true 2>"$_ERR14" || _EXIT14=$?
_END14="$(date +%s)"
_ELAPSED14=$(( _END14 - _START14 ))

kill "$_HOLDER14" 2>/dev/null || true
wait "$_HOLDER14" 2>/dev/null || true

assert "Test 14: wrapper exits 75 (EX_TEMPFAIL) when lock-wait limit exceeded (got $_EXIT14)" \
    test "$_EXIT14" -eq 75

assert "Test 14: wrapper exits within 3s, not blocked until outer safety timeout (elapsed=${_ELAPSED14}s)" \
    test "$_ELAPSED14" -le 3

assert "Test 14: stderr mentions 'acquire' and lock-wait duration (1s)" \
    grep -qE 'acquire.*1s|1s.*acquire' "$_ERR14"

rm -f "$_LOCK14" "$_ERR14"

# -- Test 15: post-lock timer fires N seconds AFTER lock acquisition, not after start ---
echo ""
echo "--- Test 15: REIFY_OCCT_TEST_TIMEOUT measured post-lock, not from wrapper start ---"

_LOCK15="$(mktemp)"

# Spawn a holder that holds the lock for 3 seconds.
# Using 3s (not 2s) to survive date +%s rounding: at 2s the truncated elapsed
# can read as 2 and spuriously fail the lower-bound assertion on a busy CI.
( flock -x 9; sleep 3 ) 9>>"$_LOCK15" &
_HOLDER15=$!
sleep 0.2  # give holder time to acquire

_START15="$(date +%s)"
_EXIT15=0
REIFY_OCCT_LOCK="$_LOCK15" REIFY_OCCT_LOCK_WAIT=10 REIFY_OCCT_TEST_TIMEOUT=1 \
    "$WRAPPER" sleep 5 || _EXIT15=$?
_END15="$(date +%s)"
_ELAPSED15=$(( _END15 - _START15 ))

kill "$_HOLDER15" 2>/dev/null || true
wait "$_HOLDER15" 2>/dev/null || true
rm -f "$_LOCK15"

# Expected: lock acquired after ~3s, then `timeout 1 sleep 5` kills after 1s → rc=124
# elapsed ≈ 4s total.  Without internal timeout: sleep 5 runs fully → rc=0, elapsed ≈ 8s.
# Lower bound ≥ 4 proves the timer could not have started at wrapper launch (~1s in that case).
assert "Test 15: wrapper exits 124 (internal timeout killed the command, got $_EXIT15)" \
    test "$_EXIT15" -eq 124

assert "Test 15: elapsed in [4,8]s — timer started post-lock, not at wrapper launch (elapsed=${_ELAPSED15}s)" \
    bash -c "test '$_ELAPSED15' -ge 4 && test '$_ELAPSED15' -le 8"

# -- Test 16: wrapper logs wait duration on lock acquisition -------------------
echo ""
echo "--- Test 16: wrapper emits INFO log line with 'acquired' and numeric duration on stderr ---"

_LOCK16="$(mktemp)"
_ERR16="$(mktemp)"

_EXIT16=0
REIFY_OCCT_LOCK="$_LOCK16" "$WRAPPER" true 2>"$_ERR16" >/dev/null || _EXIT16=$?

assert "Test 16: wrapper exits 0 (sanity check, got $_EXIT16)" \
    test "$_EXIT16" -eq 0

assert "Test 16: stderr contains log line with 'acquired', 'OCCT lock', and numeric duration (Ns)" \
    grep -qiE 'acquired.*OCCT.*lock.*[0-9]+s' "$_ERR16"

rm -f "$_LOCK16" "$_ERR16"

# -- Test 17: orchestrator.yaml no longer wraps gated invocations with outer timeout -
echo ""
echo "--- Test 17: orchestrator.yaml delegates timeout to wrapper via REIFY_OCCT_TEST_TIMEOUT ---"

assert "Test 17: no outer 'timeout --kill-after=N Nm ./scripts/cargo-test-occt-gated' remains in test_command" \
    bash -c "LINE=\$(grep 'test_command:' '$ORCH'); ! echo \"\$LINE\" | grep -qE 'timeout[[:space:]]+--kill-after=[0-9]+[[:space:]]+[0-9]+[smhd][[:space:]]+[./]*scripts/cargo-test-occt-gated'"

assert "Test 17: REIFY_OCCT_TEST_TIMEOUT= appears exactly twice in test_command (once per gated invocation)" \
    bash -c "[ \"\$(grep 'test_command:' '$ORCH' | grep -oF 'REIFY_OCCT_TEST_TIMEOUT=' | wc -l | tr -d ' ')\" -eq 2 ]"

assert "Test 17: debug invocation sets REIFY_OCCT_TEST_TIMEOUT=2700" \
    bash -c "grep 'test_command:' '$ORCH' | grep -qF 'REIFY_OCCT_TEST_TIMEOUT=2700 ./scripts/cargo-test-occt-gated.sh cargo test --workspace --'"

assert "Test 17: release invocation sets REIFY_OCCT_TEST_TIMEOUT=3600" \
    bash -c "grep 'test_command:' '$ORCH' | grep -qF 'REIFY_OCCT_TEST_TIMEOUT=3600 ./scripts/cargo-test-occt-gated.sh cargo test --workspace --release'"

test_summary
