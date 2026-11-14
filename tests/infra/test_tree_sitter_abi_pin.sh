#!/usr/bin/env bash
# Infrastructure test for task 9727 (GitHub issue leodearden/reify#1): both
# tree-sitter generators -- scripts/tree-sitter-generate.sh and
# tree-sitter-reify/build.rs -- emit a parser at the ABI the workspace's
# tree-sitter 0.24 runtime loads, even under a CLI that defaults to ABI 15.
#
# This wrapper is the member run_all.sh discovers (it globs `test_*.sh` only)
# and the one that holds the run-all-classification.manifest row. The
# assertions live in the sibling tests/infra/test_tree_sitter_abi_pin.py.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_tree_sitter_abi_pin.py (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

# tree-sitter and cargo live in ~/.cargo/bin; without this the suite would
# skip on a PATH that lacks them rather than test anything.
[ -f "${HOME:-~}/.cargo/env" ] && . "${HOME:-~}/.cargo/env" || true

echo "=== test_tree_sitter_abi_pin ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── The suite ──────────────────────────────────────────────────────────────
assert "tests/infra/test_tree_sitter_abi_pin.py exits 0" \
    python3 "$SCRIPT_DIR/test_tree_sitter_abi_pin.py"

test_summary
