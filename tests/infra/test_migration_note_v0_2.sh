#!/usr/bin/env bash
# Infrastructure test for task 9734 (docs/prds/alpha-spine.md D8, B10): the
# v0.2-alpha migration note docs/releases/v0.2-alpha.md stays true. Every
# before/after snippet it shows is a committed tests/prd-gate/fixtures/
# migration_v0_2_*.ri file, each before fails `reify check` with exactly the
# error lines the note quotes, each after passes, and its one `Closes:` line
# names the issues scripts/release-alpha.sh will close.
#
# This wrapper is the member run_all.sh discovers (it globs `test_*.sh` only)
# and the one that holds the run-all-classification.manifest row. The
# assertions live in the sibling tests/infra/test_migration_note_v0_2.py.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_migration_note_v0_2.py (stdlib unittest) exits 0
# and prints one "  SKIP:" line per skipped test, because unittest exits 0 on a
# skip and assert shows a member's output only when it FAILs.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_migration_note_v0_2 ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── The suite ──────────────────────────────────────────────────────────────
SUITE_LOG="$(mktemp "${TMPDIR:-/tmp}/test_migration_note_v0_2.XXXXXX")"
trap 'rm -f "$SUITE_LOG"' EXIT

# -v names every skipped test with its reason. On failure the log is replayed
# into assert's capture, which assert dumps.
run_suite() {
    python3 "$SCRIPT_DIR/test_migration_note_v0_2.py" -v >"$SUITE_LOG" 2>&1 || {
        cat "$SUITE_LOG"
        return 1
    }
}

assert "tests/infra/test_migration_note_v0_2.py exits 0" run_suite
sed -n 's/^\(.*\) \.\.\. skipped /  SKIP: \1: /p' "$SUITE_LOG"

test_summary
