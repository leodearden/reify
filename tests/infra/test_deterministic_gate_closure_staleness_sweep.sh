#!/usr/bin/env bash
# tests/infra/test_deterministic_gate_closure_staleness_sweep.sh
# Hermetic tests for scripts/deterministic-gate-closure-staleness-sweep.sh.
#
# Task #5321. Every fixture is synthetic: a temp tasks.db built from the
# production DDL, a temp escalation dir, and a temp git repo. The suite NEVER
# reads the live .taskmaster/tasks/tasks.db, the live data/escalations/, or
# the real repo — both because those mutate continuously under the
# orchestrator (flaky, and lock-contending under a `pool` classification) and
# because the four seed instances this design was derived from (5236, 5271,
# 5316, 5373) have already been redispatched/closed, so a live assertion on
# them would be a doomed RED that could never be GREENed. Their recorded
# shapes are encoded as frozen fixtures instead.
#
# run_sweep captures STDOUT, STDERR, and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks:
#   S — scaffolding
#   (additional blocks land in subsequent commits as the script grows: the
#    CLI contract + empty-input degradation, the liveness guard, trigger
#    classes A/B/C, the #5316 corruption suppressors, and --emit-requests +
#    the read-only proof.)
#
# The suite is free of `sleep` / wall-clock upper bounds by construction
# (offset-timestamp fixtures instead), so
# tests/infra/test_no_new_wallclock_upper_bounds.sh stays green.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/deterministic-gate-closure-staleness-sweep.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/deterministic-gate-closure-staleness-sweep.sh hermetic tests (task 5321) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state + cleanup
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

# ──────────────────────────────────────────────────────────────────────────────
# Block S — scaffolding
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block S: scaffolding ---"

assert "S0: SUT is present and executable" test -x "$SCRIPT"

test_summary
