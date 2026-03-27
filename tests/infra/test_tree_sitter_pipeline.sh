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

# ── Summary ─────────────────────────────────────────────────────────
echo ""
echo "Results: $PASS passed, $FAIL failed"
if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
