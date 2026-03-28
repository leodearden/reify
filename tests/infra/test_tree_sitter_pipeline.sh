#!/usr/bin/env bash
# Infrastructure tests for tree-sitter generation pipeline.
# Validates that generated files are properly managed:
#   - generation script exists and is executable
#   - .gitignore excludes generated files
#   - generated files are not tracked by git
#   - full regeneration-to-build pipeline works
#   - orchestrator and hook configs include generation steps

set -euo pipefail

# Resolve repo root (two levels up from tests/infra/).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Ensure parser.c is restored on exit (even on failure).
trap '"$ROOT/scripts/tree-sitter-generate.sh" >/dev/null 2>&1 || true' EXIT

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

echo "=== Tree-sitter pipeline infrastructure tests ==="

# ── Step 1: Generation script exists and is executable ──────────────
assert "scripts/tree-sitter-generate.sh exists" \
    test -f "$ROOT/scripts/tree-sitter-generate.sh"

assert "scripts/tree-sitter-generate.sh is executable" \
    test -x "$ROOT/scripts/tree-sitter-generate.sh"

# ── Step 2: Generation script produces expected output files ────────
# Run the generation script and verify outputs.
assert "tree-sitter-generate.sh runs successfully" \
    "$ROOT/scripts/tree-sitter-generate.sh"

assert "parser.c was generated" \
    test -f "$ROOT/tree-sitter-reify/src/parser.c"

assert "grammar.json was generated" \
    test -f "$ROOT/tree-sitter-reify/src/grammar.json"

assert "node-types.json was generated" \
    test -f "$ROOT/tree-sitter-reify/src/node-types.json"

# parser.c should be non-trivial (>100KB).
parser_size=$(wc -c < "$ROOT/tree-sitter-reify/src/parser.c" 2>/dev/null || echo 0)
assert "parser.c is non-trivial (>100KB, got ${parser_size} bytes)" \
    test "$parser_size" -gt 102400

# ── Step 3: .gitignore excludes generated files ────────────────────
assert ".gitignore contains tree-sitter-reify/src/parser.c" \
    grep -qF "tree-sitter-reify/src/parser.c" "$ROOT/.gitignore"

assert ".gitignore contains tree-sitter-reify/src/grammar.json" \
    grep -qF "tree-sitter-reify/src/grammar.json" "$ROOT/.gitignore"

assert ".gitignore contains tree-sitter-reify/src/node-types.json" \
    grep -qF "tree-sitter-reify/src/node-types.json" "$ROOT/.gitignore"

# ── Step 4: Generated files are NOT tracked by git ─────────────────
# git ls-files returns empty for untracked files.
assert_not_tracked() {
    local f="$1"
    [ -z "$(cd "$ROOT" && git ls-files "$f")" ]
}

assert "parser.c is not tracked by git" \
    assert_not_tracked "tree-sitter-reify/src/parser.c"

assert "grammar.json is not tracked by git" \
    assert_not_tracked "tree-sitter-reify/src/grammar.json"

assert "node-types.json is not tracked by git" \
    assert_not_tracked "tree-sitter-reify/src/node-types.json"

# ── Step 5: Full regeneration-to-build pipeline ────────────────────
# Delete parser.c, regenerate, then verify cargo check succeeds.
rm -f "$ROOT/tree-sitter-reify/src/parser.c"

assert "tree-sitter-generate.sh regenerates after deletion" \
    "$ROOT/scripts/tree-sitter-generate.sh"

assert "parser.c exists after regeneration" \
    test -f "$ROOT/tree-sitter-reify/src/parser.c"

assert "cargo check -p tree-sitter-reify succeeds after regeneration" \
    cargo check -p tree-sitter-reify

# ── Step 6: Orchestrator verification commands include generation ───
# Check that tree-sitter generation appears in each verification command.
assert "test_command includes tree-sitter generation" \
    bash -c "grep '^test_command:' '$ROOT/orchestrator.yaml' | grep -q 'tree-sitter-generate'"

assert "lint_command includes tree-sitter generation" \
    bash -c "grep '^lint_command:' '$ROOT/orchestrator.yaml' | grep -q 'tree-sitter-generate'"

assert "type_check_command includes tree-sitter generation" \
    bash -c "grep '^type_check_command:' '$ROOT/orchestrator.yaml' | grep -q 'tree-sitter-generate'"

# ── Step 7: hooks/project-checks includes tree-sitter generation ───
assert "hooks/project-checks includes tree-sitter generation" \
    grep -q "tree-sitter-generate" "$ROOT/hooks/project-checks"

# ── Step 8: Install guidance recommends npm, not cargo ────────────
assert_not_contains() {
    local file="$1"
    local pattern="$2"
    ! grep -qF "$pattern" "$file"
}

assert "generation script does NOT recommend 'cargo install tree-sitter-cli'" \
    assert_not_contains "$ROOT/scripts/tree-sitter-generate.sh" "cargo install tree-sitter-cli"

assert "generation script recommends 'npm install -g tree-sitter-cli'" \
    grep -qF "npm install -g tree-sitter-cli" "$ROOT/scripts/tree-sitter-generate.sh"

# ── Summary ─────────────────────────────────────────────────────────
echo ""
echo "Results: $PASS passed, $FAIL failed"
if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
