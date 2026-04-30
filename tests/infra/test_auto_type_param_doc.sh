#!/usr/bin/env bash
# Sentinel smoke test for the auto type-param resolution explainer doc and
# cross-references added to the language spec and stdlib reference.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.
#
# These assertions pin *load-bearing anchors* (file existence, algorithm tag,
# cap-of-10 constant, v0.2 deferral, cross-refs) — NOT prose wording.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

EXPLAINER="$REPO_ROOT/docs/auto-type-param-resolution.md"
SPEC="$REPO_ROOT/docs/reify-language-spec.md"
STDLIB_REF="$REPO_ROOT/docs/reify-stdlib-reference.md"

echo "=== auto type-param doc sentinel tests ==="

# ------------------------------------------------------------------
# Check 1: explainer doc exists
# ------------------------------------------------------------------
echo ""
echo "--- Check 1: explainer doc exists ---"

assert "docs/auto-type-param-resolution.md exists" \
    test -f "$EXPLAINER"

# ------------------------------------------------------------------
# Check 2: algorithm tag-line present in explainer
# ------------------------------------------------------------------
echo ""
echo "--- Check 2: 'per-parameter BFS' algorithm tag present ---"

assert "explainer mentions 'per-parameter BFS'" \
    grep -qi "per-parameter BFS" "$EXPLAINER"

# ------------------------------------------------------------------
# Check 3: cap-of-10 anchor present in explainer
# ------------------------------------------------------------------
echo ""
echo "--- Check 3: cap-of-10 anchor present ---"

assert "explainer mentions cap value '10'" \
    grep -qi "cap" "$EXPLAINER"

assert "explainer contains the number '10' (as cap constant)" \
    grep -q "10" "$EXPLAINER"

# ------------------------------------------------------------------
# Check 4: v0.2 deferral mention present in explainer
# ------------------------------------------------------------------
echo ""
echo "--- Check 4: v0.2 deferral note present ---"

assert "explainer mentions 'v0.2' (deferral anchor)" \
    grep -q "v0\.2" "$EXPLAINER"

# ------------------------------------------------------------------
# Check 5: lexicographic tiebreak by FQN anchor present
# ------------------------------------------------------------------
echo ""
echo "--- Check 5: lexicographic tiebreak by FQN anchor ---"

assert "explainer mentions 'lexicographic'" \
    grep -qi "lexicographic" "$EXPLAINER"

assert "explainer mentions 'FQN' or 'fully qualified name'" \
    bash -c "grep -qi 'FQN\|fully qualified name' '$EXPLAINER'"

# ------------------------------------------------------------------
# Check 6: language spec cross-references the explainer
# ------------------------------------------------------------------
echo ""
echo "--- Check 6: language spec cross-references explainer ---"

assert "docs/reify-language-spec.md references 'auto-type-param-resolution.md'" \
    grep -q "auto-type-param-resolution\.md" "$SPEC"

# ------------------------------------------------------------------
# Check 7: stdlib reference cross-references the explainer
# ------------------------------------------------------------------
echo ""
echo "--- Check 7: stdlib reference cross-references explainer ---"

assert "docs/reify-stdlib-reference.md references 'auto-type-param-resolution.md'" \
    grep -q "auto-type-param-resolution\.md" "$STDLIB_REF"

# ------------------------------------------------------------------
# Summary
# ------------------------------------------------------------------
test_summary
