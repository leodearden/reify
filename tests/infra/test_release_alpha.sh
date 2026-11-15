#!/usr/bin/env bash
# Infrastructure test for task 9732 (docs/prds/alpha-spine.md D7, §5.3):
# scripts/release-alpha.sh's preflight refusals, tag, prerelease, issue closing
# and idempotent re-run, run as a copy inside a throwaway repo with a stub `gh`,
# a stub build-bundle.sh and a local bare remote. Hermetic: no network.
#
# This wrapper is the member run_all.sh discovers (it globs `test_*.sh` only)
# and the one that holds the run-all-classification.manifest row. The
# assertions live in the sibling tests/infra/test_release_alpha.py.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_release_alpha.py (stdlib unittest) exits 0
# and prints one "  SKIP:" line per skipped test, because unittest exits 0 on a
# skip and assert shows a member's output only when it FAILs.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_release_alpha ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── The suite ──────────────────────────────────────────────────────────────
SUITE_LOG="$(mktemp "${TMPDIR:-/tmp}/test_release_alpha.XXXXXX")"
trap 'rm -f "$SUITE_LOG"' EXIT

# -v names every skipped test with its reason. On failure the log is replayed
# into assert's capture, which assert dumps.
run_suite() {
    python3 "$SCRIPT_DIR/test_release_alpha.py" -v >"$SUITE_LOG" 2>&1 || {
        cat "$SUITE_LOG"
        return 1
    }
}

assert "tests/infra/test_release_alpha.py exits 0" run_suite
sed -n 's/^\(.*\) \.\.\. skipped /  SKIP: \1: /p' "$SUITE_LOG"

test_summary
