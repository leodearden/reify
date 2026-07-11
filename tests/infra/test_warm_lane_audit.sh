#!/usr/bin/env bash
# tests/infra/test_warm_lane_audit.sh
# Hermetic tests for scripts/warm-lane-audit.sh.
#
# PRD: docs/prds/warm-lane-pool-sizing-lifecycle.md §9.1 (task α); boundary
# tests B1/B2 (§10). Reuses the task-4749 status-oracle seam
# (REIFY_LANE_LEAK_STATUS_CMD) and the <lane_dir>.lock flock convention from
# scripts/warm-lane-gc.sh / tests/infra/test_warm_lane_gc.sh.
#
# run_helper captures STDOUT, STDERR, and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks:
#   A — CLI guard: --help, unknown flag
#   (additional blocks land in subsequent commits as the script grows: the
#    ASSIGNED/FREE probe + LIVE + headroom, backing-task-status +
#    RECLAIMABLE-via-terminal, recoverable LANDED/PUSHED/ORPHAN,
#    residue-only-dirty, LEAKED + stale, measured disk figures, --format
#    json, the combined B1 fixture + A3 degradation, and B2 read-only.)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/warm-lane-audit.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/warm-lane-audit.sh hermetic tests (task 5172) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state + cleanup
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
_BGPIDS=()
cleanup() {
    for pid in "${_BGPIDS[@]+${_BGPIDS[@]}}"; do
        kill "$pid" 2>/dev/null || true
    done
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

ERR_FILE="$(mktemp /tmp/test-warm-lane-audit-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── run_helper ─────────────────────────────────────────────────────────────────
# Invokes warm-lane-audit.sh, capturing OUT (stdout), ERR_OUT (stderr), RC.
run_helper() {
    local rc=0
    > "$ERR_FILE"
    OUT="$(bash "$SCRIPT" "$@" 2>"$ERR_FILE")" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# ──────────────────────────────────────────────────────────────────────────────
# Block A — CLI guard
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: CLI guard ---"

# A1: --help exits 0 and prints usage on stderr
run_helper --help
assert "A1: --help exits 0" test "$RC" -eq 0
assert "A1: --help prints 'usage' or 'Usage' on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$ERR_OUT"

# A2: unknown flag exits 2
run_helper --nope
assert "A2: unknown flag exits 2" test "$RC" -eq 2

test_summary
