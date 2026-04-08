#!/usr/bin/env bash
# Unit tests for tree-sitter-generate.sh staleness and timeout features.
# Tests the stamp file, staleness check, timeout wrapping, and --force flag.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TS_DIR="$ROOT/tree-sitter-reify"
GENERATE_SCRIPT="$ROOT/scripts/tree-sitter-generate.sh"
STAMP_FILE="$TS_DIR/src/.grammar_hash.stamp"

# Shared utilities (compute_sha256, etc.)
source "$SCRIPT_DIR/lib.sh"

# This is a test script, not a build script — source shared test helpers from tests/infra/.
[ -f "$ROOT/tests/infra/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found"; exit 1; }
source "$ROOT/tests/infra/test_helpers.sh"

# Ensure parser.c + stamp are restored on exit.
trap '"$GENERATE_SCRIPT" --force >/dev/null 2>&1 || true' EXIT

echo "=== tree-sitter-generate.sh unit tests ==="

# ── Test 1: stamp file is created after successful generation ──────
echo ""
echo "--- Test 1: stamp file created after generation ---"

# Remove stamp if it exists, then run generation.
rm -f "$STAMP_FILE"

output=$("$GENERATE_SCRIPT" --force 2>&1)

assert "stamp file exists after generation" \
    test -f "$STAMP_FILE"

# Stamp should contain a sha256 hex string (64 hex chars).
stamp_content=$(cat "$STAMP_FILE" 2>/dev/null || echo "")
assert "stamp contains a sha256 hash (64 hex chars)" \
    env STAMP="$stamp_content" bash -c '[[ "$STAMP" =~ ^[0-9a-f]{64}$ ]]'

# Stamp should match sha256sum of grammar.js.
expected_hash=$(compute_sha256 "$TS_DIR/grammar.js" | awk '{print $1}')
assert "stamp hash matches grammar.js sha256" \
    test "$stamp_content" = "$expected_hash"

# ── Test 2: staleness check skips generation when up to date ───────
echo ""
echo "--- Test 2: skip generation when grammar.js unchanged ---"

# After Test 1, stamp is fresh and outputs exist. Run again.
output2=$("$GENERATE_SCRIPT" 2>&1 || true)

assert "second run prints 'up to date'" \
    bash -c 'grep -q "up to date"' <<< "$output2"

assert "second run does NOT print 'generated parser files'" \
    bash -c '! grep -q "generated parser files"' <<< "$output2"

# ── Test 3: regenerates when stamp file is missing ─────────────────
echo ""
echo "--- Test 3: regenerate when stamp file missing ---"

rm -f "$STAMP_FILE"

output3=$("$GENERATE_SCRIPT" 2>&1 || true)

assert "regenerates when stamp missing (prints 'generated parser files')" \
    bash -c 'grep -q "generated parser files"' <<< "$output3"

assert "stamp recreated after regeneration" \
    test -f "$STAMP_FILE"

# ── Test 4: regenerates when an output file is missing ─────────────
echo ""
echo "--- Test 4: regenerate when output file missing ---"

# Ensure stamp is fresh first.
rm -f "$TS_DIR/src/parser.c"

output4=$("$GENERATE_SCRIPT" 2>&1 || true)

assert "regenerates when parser.c missing (prints 'generated parser files')" \
    bash -c 'grep -q "generated parser files"' <<< "$output4"

assert "parser.c restored after regeneration" \
    test -f "$TS_DIR/src/parser.c"

# ── Test 5: regenerates when grammar.js content changes ────────────
echo ""
echo "--- Test 5: regenerate when grammar hash differs ---"

# Write a fake hash to stamp (simulates grammar.js change).
echo -n "0000000000000000000000000000000000000000000000000000000000000000" > "$STAMP_FILE"

output5=$("$GENERATE_SCRIPT" 2>&1 || true)

assert "regenerates when hash differs (prints 'generated parser files')" \
    bash -c 'grep -q "generated parser files"' <<< "$output5"

# Stamp should now contain the real hash.
stamp_after=$(cat "$STAMP_FILE" 2>/dev/null || echo "")
real_hash=$(compute_sha256 "$TS_DIR/grammar.js" | awk '{print $1}')
assert "stamp updated to match current grammar.js" \
    test "$stamp_after" = "$real_hash"

# ── Test 6: timeout wrapper in source ──────────────────────────────
echo ""
echo "--- Test 6: timeout wrapper present ---"

assert "script uses 'timeout 60 tree-sitter generate'" \
    grep -q 'timeout 60 tree-sitter generate' "$GENERATE_SCRIPT"

# ── Test 7: --force flag bypasses staleness ────────────────────────
echo ""
echo "--- Test 7: --force bypasses staleness check ---"

# At this point stamp is fresh and outputs exist. Normal run would skip.
output7=$("$GENERATE_SCRIPT" --force 2>&1 || true)

assert "--force generates even when up to date (prints 'generated parser files')" \
    bash -c 'grep -q "generated parser files"' <<< "$output7"

assert "--force does NOT print 'up to date'" \
    bash -c '! grep -q "up to date"' <<< "$output7"

# ── Test 8: shared lib.sh integration ─────────────────────────────
echo ""
echo "--- Test 8: sources lib.sh and lib_portable.sh available ---"

assert "script sources lib.sh" \
    grep -qE '(source|\.)\s+.*lib\.sh' "$GENERATE_SCRIPT"

assert "script does NOT define compute_sha256 locally" \
    bash -c '! grep -q "^compute_sha256()" '"$GENERATE_SCRIPT"

# lib.sh now sources lib_portable.sh, so portable helpers are available.
assert "lib_portable.sh is available (via lib.sh sourcing chain)" \
    bash -c "source '$SCRIPT_DIR/lib.sh' && declare -f portable_sha256 >/dev/null && declare -f portable_timeout >/dev/null"

# ── Test 9: MAX_WAIT_SECS timeout alignment ──────────────────────
echo ""
echo "--- Test 9: MAX_WAIT_SECS constant and derived limits ---"

assert "MAX_WAIT_SECS is defined in the script" \
    grep -q '^MAX_WAIT_SECS=' "$GENERATE_SCRIPT"

assert "flock uses \$MAX_WAIT_SECS (not hardcoded 120)" \
    grep -q 'flock -x -w \$MAX_WAIT_SECS' "$GENERATE_SCRIPT"

assert "MAX_LOCK_WAIT_SECS constant is defined in the script" \
    grep -q '^MAX_LOCK_WAIT_SECS=' "$GENERATE_SCRIPT"

assert "mkdir loop -ge comparison uses \$MAX_LOCK_WAIT_SECS (not \$MAX_WAIT_SECS)" \
    grep -q '\-ge \$MAX_LOCK_WAIT_SECS' "$GENERATE_SCRIPT"

assert "error message reports lock-wait seconds (not iteration count)" \
    grep -qE 'could not acquire generation lock after \$\{MAX_LOCK_WAIT_SECS\}s' "$GENERATE_SCRIPT"

# ── Test 10: timeout guard via portable_timeout ──────────────────
echo ""
echo "--- Test 10: no bare tree-sitter generate; portable_timeout guard ---"

# Every 'tree-sitter generate' invocation must be guarded by portable_timeout.
# No unguarded invocations should exist.
assert "no bare 'tree-sitter generate' without timeout guard" \
    bash -c '[ -z "$(grep -E "^[[:space:]]+tree-sitter generate" "$1" | grep -vE "(timeout|portable_timeout|&)")" ]' _ "$GENERATE_SCRIPT"

# The kill-based fallback now lives in lib_portable.sh (portable_timeout function),
# not inline in tree-sitter-generate.sh.
assert "kill-based fallback lives in lib_portable.sh" \
    grep -q 'kill.*cmd_pid\|kill.*\$cmd_pid' "$SCRIPT_DIR/lib_portable.sh"

# ── Test 11: portability gate — no Perl-mode grep in tests ───────
echo ""
echo "--- Test 11: test file uses only POSIX-compatible grep ---"

# Perl regex mode is unavailable on macOS BSD grep and silently fails
# (exit 2), causing negated assertions to false-positive. All grep patterns
# in this test file must use POSIX-ERE (grep -E) or basic grep instead.
# Note: [P] character class self-avoids matching this assertion line itself.
_TEST_FILE="${BASH_SOURCE[0]}"
assert "test file does not use Perl-mode grep (non-portable)" \
    bash -c '[ -z "$(grep "grep -[P]" "$1")" ]' _ "$_TEST_FILE"

# ── Test 12: lock file excluded from version control ─────────────
echo ""
echo "--- Test 12: .generate.lock in .gitignore ---"

# The lock file is a runtime artifact created by flock (exec 9>"$LOCK_FILE").
# It should not be tracked in version control.
assert ".generate.lock pattern appears in root .gitignore" \
    grep -q '\.generate\.lock' "$ROOT/.gitignore"

# The mkdir-based lock directory is also a runtime artifact.
assert ".generate.lock.d appears in root .gitignore" \
    grep -q '\.generate\.lock\.d' "$ROOT/.gitignore"

# ── Test 13: uses portable_timeout from lib_portable.sh ──────────
echo ""
echo "--- Test 13: uses portable_timeout instead of inline block ---"

assert "script calls portable_timeout for tree-sitter generate" \
    grep -q 'portable_timeout.*tree-sitter generate' "$GENERATE_SCRIPT"

assert "script does NOT have inline gtimeout fallback (replaced by portable_timeout)" \
    bash -c '! grep -q "gtimeout 60 tree-sitter generate" '"$GENERATE_SCRIPT"

# ── Test 14: timeout check uses _PORTABLE_TIMEOUT_TIMED_OUT ─────
echo ""
echo "--- Test 14: timeout check uses _PORTABLE_TIMEOUT_TIMED_OUT ---"

assert "script checks _PORTABLE_TIMEOUT_TIMED_OUT alongside exit code 124" \
    grep -q '_PORTABLE_TIMEOUT_TIMED_OUT' "$GENERATE_SCRIPT"

# ── Test 15: stale-age comparison uses -ge (inclusive) ───────────
echo ""
echo "--- Test 15: stale-age comparison uses -ge (not -gt) ---"

# The stale-lock detection must use -ge so that a lock exactly MAX_WAIT_SECS
# old is treated as stale.  Using -gt collapses the safety buffer to zero
# when the poll loop also uses MAX_LOCK_WAIT_SECS=75 wall-time seconds.
assert "stale-age check uses -ge (not -gt) for MAX_WAIT_SECS comparison" \
    grep -qE '_lock_age.*-ge.*MAX_WAIT_SECS' "$GENERATE_SCRIPT"

# ── Test 16: timeout path removes partial output files ────────────
echo ""
echo "--- Test 16: timeout path cleans up partial output files ---"

# On timeout, tree-sitter generate may leave partially-written parser.c,
# grammar.json, and node-types.json.  The timeout error path must remove
# these files before exiting so they don't confuse subsequent runs.
# Require all three output files in a single rm -f command.
assert "timeout error path removes all three partial output files (rm -f parser.c grammar.json node-types.json)" \
    grep -qE 'rm -f .*parser\.c .*grammar\.json .*node-types\.json' "$GENERATE_SCRIPT"

# Both error paths (timeout AND non-timeout failure) must call cleanup.
# With the helper-function approach, _cleanup_partial_outputs must be called
# at least twice — once per error branch.
assert "cleanup is called on both error paths (_cleanup_partial_outputs called >= 2 times)" \
    bash -c '[ "$(grep -c "_cleanup_partial_outputs" "$1")" -ge 2 ]' _ "$GENERATE_SCRIPT"

# ── Test 17: non-timeout failure removes partial output files ─────
echo ""
echo "--- Test 17: non-timeout failure cleans up partial output files ---"

# Create a stub tree-sitter that writes partial output then exits 1,
# simulating a grammar compilation error that partially writes files.
_test17_tmpdir=$(mktemp -d)
cat > "$_test17_tmpdir/tree-sitter" << 'STUB'
#!/bin/sh
# Simulate a partial write: create output files before failing.
touch src/parser.c
touch src/grammar.json
exit 1
STUB
chmod +x "$_test17_tmpdir/tree-sitter"

# Remove stamp so the script does not skip generation.
rm -f "$STAMP_FILE"

# Run generate script with stub shadowing the real tree-sitter.
# Capture exit code; expect non-zero (generation failure).
_test17_exit=0
(export PATH="$_test17_tmpdir:$PATH"; "$GENERATE_SCRIPT" >/dev/null 2>&1) || _test17_exit=$?

assert "non-timeout failure: generate script exits non-zero" \
    test "$_test17_exit" -ne 0

assert "non-timeout failure: partial parser.c is cleaned up" \
    test '!' -f "$TS_DIR/src/parser.c"

assert "non-timeout failure: partial grammar.json is cleaned up" \
    test '!' -f "$TS_DIR/src/grammar.json"

# Clean up stub.
rm -rf "$_test17_tmpdir"

# ── Summary ────────────────────────────────────────────────────────
test_summary
