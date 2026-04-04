#!/usr/bin/env bash
# Test: Both copies of sanitize_value carry SYNC cross-reference comments.
#
# sanitize_value exists in two crates — reify-expr and reify-stdlib.
# Each copy must have a `// SYNC:` marker so that `grep SYNC` finds both
# sites when Value variants change.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXPR_FILE="$REPO_ROOT/crates/reify-expr/src/lib.rs"
STDLIB_FILE="$REPO_ROOT/crates/reify-stdlib/src/lib.rs"

PASS=0
FAIL=0

check() {
    local desc="$1"
    local file="$2"
    local pattern="$3"
    if grep -q "$pattern" "$file"; then
        echo "PASS: $desc"
        PASS=$((PASS + 1))
    else
        echo "FAIL: $desc"
        echo "      Expected pattern '$pattern' not found in $file"
        FAIL=$((FAIL + 1))
    fi
}

# reify-expr's copy must reference reify-stdlib::sanitize_value
check \
    "reify-expr/src/lib.rs has SYNC marker referencing reify-stdlib::sanitize_value" \
    "$EXPR_FILE" \
    "SYNC:.*reify-stdlib::sanitize_value"

# reify-stdlib's copy must reference reify-expr::sanitize_value
check \
    "reify-stdlib/src/lib.rs has SYNC marker referencing reify-expr::sanitize_value" \
    "$STDLIB_FILE" \
    "SYNC:.*reify-expr::sanitize_value"

echo ""
echo "Results: $PASS passed, $FAIL failed"

if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
