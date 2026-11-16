#!/usr/bin/env bash
# Infrastructure test for task 9732 (docs/prds/alpha-spine.md D6, §5.2, B7):
# the clean-machine proof of the installer. The .deb and the AppImage each
# install into a stock ubuntu:24.04 container (no /opt/reify-deps, no extra apt
# source), `reify check examples/m5_geometry.ri` exits 0 and reify-gui under
# xvfb answers a debug-MCP store_state with at least one mesh.
#
# DEFAULT-SKIPPING: armed only by REIFY_RUN_BUNDLE_CLEAN_INSTALL=1, because it
# costs a cold scripts/build-bundle.sh plus two containers. Classified
# host-exclusive (the merge gate runs run_all.sh with host-exclusive members
# excluded); the precedent is test_seed_lane_lock_release_soak.sh.
#
# This wrapper is the member run_all.sh discovers (it globs `test_*.sh` only)
# and the one that holds the run-all-classification.manifest row. The
# assertions live in the sibling tests/infra/test_bundle_clean_install.py.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_bundle_clean_install.py (stdlib unittest) exits 0
# and prints one "  SKIP:" line per skipped test, because unittest exits 0 on a
# skip and assert shows a member's output only when it FAILs.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_bundle_clean_install ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── The suite ──────────────────────────────────────────────────────────────
SUITE_LOG="$(mktemp "${TMPDIR:-/tmp}/test_bundle_clean_install.XXXXXX")"
trap 'rm -f "$SUITE_LOG"' EXIT

# -v names every skipped test with its reason. On failure the log is replayed
# into assert's capture, which assert dumps.
run_suite() {
    python3 "$SCRIPT_DIR/test_bundle_clean_install.py" -v >"$SUITE_LOG" 2>&1 || {
        cat "$SUITE_LOG"
        return 1
    }
}

assert "tests/infra/test_bundle_clean_install.py exits 0" run_suite
sed -n 's/^\(.*\) \.\.\. skipped /  SKIP: \1: /p' "$SUITE_LOG"

test_summary
