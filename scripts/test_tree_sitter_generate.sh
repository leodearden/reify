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

# Portable SHA-256: prefer sha256sum (GNU coreutils), fall back to shasum (macOS).
# Mirrors compute_sha256() from tree-sitter-generate.sh.
compute_hash() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1"
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1"
    else
        echo "ERROR: neither sha256sum nor shasum found on PATH." >&2
        exit 1
    fi
}

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
    env STAMP="$stamp_content" bash -c '[[ "$STAMP" =~ ^[0-9a-f]{64}$ ]]'

# Stamp should match sha256sum of grammar.js.
expected_hash=$(compute_hash "$TS_DIR/grammar.js" | awk '{print $1}')
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
real_hash=$(compute_hash "$TS_DIR/grammar.js" | awk '{print $1}')
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
echo "--- Test 8: sources lib.sh instead of local compute_sha256 ---"

assert "script sources lib.sh" \
    grep -qE '(source|\.)\s+.*lib\.sh' "$GENERATE_SCRIPT"

assert "script does NOT define compute_sha256 locally" \
    bash -c '! grep -q "^compute_sha256()" '"$GENERATE_SCRIPT"

# ── Summary ────────────────────────────────────────────────────────
echo ""
echo "Results: $PASS passed, $FAIL failed"
if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
