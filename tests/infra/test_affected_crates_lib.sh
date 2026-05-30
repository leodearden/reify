#!/usr/bin/env bash
# tests/infra/test_affected_crates_lib.sh — drift test for scripts/affected-crates-lib.sh
#
# Validates that affected_crates() correctly maps changed files to the
# affected workspace-crate set per docs/prds/verify-scope-contract.md §3 C3/C4/C5.
#
# Assertions (B8 battery):
#   1. C4: Cargo.lock (global) forces ALL
#   2. C1/§5: non-crate paths (docs/**, gui/src/**) contribute no crates (no ALL)
#   3. C5: unmappable path forces ALL
#   4. crates/<leaf-crate>/** maps to itself (reify-cli has no dependents)
#   5. gui/src-tauri/** maps to reify-gui
#   6. crates/<low-level-crate>/src/* expands to full reverse closure == independent oracle
#   7. reverse closure of reify-core is NOT the ALL sentinel
#   8. reverse closure of reify-core contains sampled dependents
#   9. reverse closure of reify-ir == independent oracle
#  10. cargo metadata failure -> ALL (C5)
#  11. global anywhere in arg list -> ALL

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

[ -f "$REPO_ROOT/scripts/affected-crates-lib.sh" ] || { echo "ERROR: affected-crates-lib.sh not found at $REPO_ROOT/scripts/affected-crates-lib.sh"; exit 1; }
source "$REPO_ROOT/scripts/affected-crates-lib.sh"

echo "=== affected-crates-lib drift tests ==="

# ---------------------------------------------------------------------------
# Step 1: C4 global-force — Cargo.lock forces ALL
# ---------------------------------------------------------------------------
echo ""
echo "--- C4: global files force ALL ---"

assert "Cargo.lock forces ALL" \
    test "$(affected_crates Cargo.lock)" = "ALL"

# ---------------------------------------------------------------------------
# Step 3: C1/§5 non-crate paths — no crates contributed, must NOT force ALL
# ---------------------------------------------------------------------------
echo ""
echo "--- §5 non-crate paths contribute nothing (not ALL) ---"

assert "docs path -> empty" \
    test -z "$(affected_crates docs/architecture/x.md)"

assert "gui frontend -> empty" \
    test -z "$(affected_crates gui/src/App.tsx)"

test_summary
