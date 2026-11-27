#!/usr/bin/env bash
# Infrastructure test for task 10264: scripts/tree-sitter-generate.sh answers a
# current src/ without a tree-sitter CLI on PATH, and still refuses to generate.
#
# This wrapper is the member run_all.sh discovers (it globs `test_*.sh` only)
# and the one that holds the run-all-classification.manifest row. The
# assertions live in the sibling tests/infra/test_tree_sitter_generate_without_cli.py.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_tree_sitter_generate_without_cli ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── The suite ──────────────────────────────────────────────────────────────
SUITE_LOG="$(mktemp "${TMPDIR:-/tmp}/test_tree_sitter_generate_without_cli.XXXXXX")"
trap 'rm -f "$SUITE_LOG"' EXIT

# -v names every skipped test with its reason. On failure the log is replayed
# into assert's capture, which assert dumps.
run_suite() {
    python3 "$SCRIPT_DIR/test_tree_sitter_generate_without_cli.py" -v >"$SUITE_LOG" 2>&1 || {
        cat "$SUITE_LOG"
        return 1
    }
}

assert "tests/infra/test_tree_sitter_generate_without_cli.py exits 0" run_suite
sed -n 's/^\(.*\) \.\.\. skipped /  SKIP: \1: /p' "$SUITE_LOG"

test_summary
