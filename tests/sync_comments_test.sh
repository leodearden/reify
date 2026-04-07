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

[ -f "$REPO_ROOT/tests/infra/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found"; exit 1; }
source "$REPO_ROOT/tests/infra/test_helpers.sh"

# reify-expr's copy must reference reify-stdlib::sanitize_value
assert \
    "reify-expr/src/lib.rs has SYNC marker referencing reify-stdlib::sanitize_value" \
    grep -q "SYNC:.*reify-stdlib::sanitize_value" "$EXPR_FILE"

# reify-stdlib's copy must reference reify-expr::sanitize_value
assert \
    "reify-stdlib/src/lib.rs has SYNC marker referencing reify-expr::sanitize_value" \
    grep -q "SYNC:.*reify-expr::sanitize_value" "$STDLIB_FILE"

# Extract the function name referenced in reify-expr's SYNC comment and verify
# it actually exists as a function definition in reify-stdlib
_expr_ref_fn=$(grep -oE 'reify-stdlib::[a-z_]+' "$EXPR_FILE" | head -1 | sed 's/.*:://')
assert \
    "fn ${_expr_ref_fn} exists in reify-stdlib/src/lib.rs (as referenced by SYNC in reify-expr)" \
    grep -q "fn ${_expr_ref_fn}" "$STDLIB_FILE"

# Extract the function name referenced in reify-stdlib's SYNC comment and verify
# it actually exists as a function definition in reify-expr
_stdlib_ref_fn=$(grep -oE 'reify-expr::[a-z_]+' "$STDLIB_FILE" | head -1 | sed 's/.*:://')
assert \
    "fn ${_stdlib_ref_fn} exists in reify-expr/src/lib.rs (as referenced by SYNC in reify-stdlib)" \
    grep -q "fn ${_stdlib_ref_fn}" "$EXPR_FILE"

test_summary
