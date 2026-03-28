#!/usr/bin/env bash
# Unit tests for tree-sitter-generate.sh staleness and timeout features.
# Tests the stamp file, staleness check, timeout wrapping, and --force flag.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TS_DIR="$ROOT/tree-sitter-reify"
GENERATE_SCRIPT="$ROOT/scripts/tree-sitter-generate.sh"
STAMP_FILE="$TS_DIR/src/.grammar_hash.stamp"

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
    bash -c "[[ '$stamp_content' =~ ^[0-9a-f]{64}$ ]]"

# Stamp should match sha256sum of grammar.js.
expected_hash=$(sha256sum "$TS_DIR/grammar.js" | awk '{print $1}')
assert "stamp hash matches grammar.js sha256" \
    test "$stamp_content" = "$expected_hash"

# ── Test 2: staleness check skips generation when up to date ───────
echo ""
echo "--- Test 2: skip generation when grammar.js unchanged ---"

# After Test 1, stamp is fresh and outputs exist. Run again.
output2=$("$GENERATE_SCRIPT" 2>&1)

assert "second run prints 'up to date'" \
    bash -c "echo '$output2' | grep -q 'up to date'"

assert "second run does NOT print 'generated parser files'" \
    bash -c "! echo '$output2' | grep -q 'generated parser files'"

# ── Test 3: regenerates when stamp file is missing ─────────────────
echo ""
echo "--- Test 3: regenerate when stamp file missing ---"

rm -f "$STAMP_FILE"

output3=$("$GENERATE_SCRIPT" 2>&1)

assert "regenerates when stamp missing (prints 'generated parser files')" \
    bash -c "echo '$output3' | grep -q 'generated parser files'"

assert "stamp recreated after regeneration" \
    test -f "$STAMP_FILE"

# ── Test 4: regenerates when an output file is missing ─────────────
echo ""
echo "--- Test 4: regenerate when output file missing ---"

# Ensure stamp is fresh first.
rm -f "$TS_DIR/src/parser.c"

output4=$("$GENERATE_SCRIPT" 2>&1)

assert "regenerates when parser.c missing (prints 'generated parser files')" \
    bash -c "echo '$output4' | grep -q 'generated parser files'"

assert "parser.c restored after regeneration" \
    test -f "$TS_DIR/src/parser.c"

# ── Test 5: regenerates when grammar.js content changes ────────────
echo ""
echo "--- Test 5: regenerate when grammar hash differs ---"

# Write a fake hash to stamp (simulates grammar.js change).
echo -n "0000000000000000000000000000000000000000000000000000000000000000" > "$STAMP_FILE"

output5=$("$GENERATE_SCRIPT" 2>&1)

assert "regenerates when hash differs (prints 'generated parser files')" \
    bash -c "echo '$output5' | grep -q 'generated parser files'"

# Stamp should now contain the real hash.
stamp_after=$(cat "$STAMP_FILE" 2>/dev/null || echo "")
real_hash=$(sha256sum "$TS_DIR/grammar.js" | awk '{print $1}')
assert "stamp updated to match current grammar.js" \
    test "$stamp_after" = "$real_hash"

# ── Test 6: timeout wrapper in source ──────────────────────────────
echo ""
echo "--- Test 6: timeout wrapper present ---"

assert "script uses 'timeout 60 tree-sitter generate'" \
    grep -q 'timeout 60 tree-sitter generate' "$GENERATE_SCRIPT"

# ── Summary ────────────────────────────────────────────────────────
echo ""
echo "Results: $PASS passed, $FAIL failed"
if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
