#!/usr/bin/env bash
# tests/infra/test_warm_lane_pool.sh
# End-to-end integration gate for the warm-lane CoW pool mechanism.
# Task: #4662
#
# Architecture — two layers:
#
#   ALWAYS-RUN layer (no substrate needed, runs everywhere):
#     Block A  — script-presence / CLI-stability preconditions for all 4
#                warm-lane scripts (provision/seed/refresh/preflight).
#     Block FC — fail-closed wiring (B2 non-reflink-loud, B5 RUSTFLAGS-mismatch,
#                B5 preflight against unmounted mount) via the PATH-stub idiom.
#
#   SUBSTRATE-GATED real end-to-end layer (skips gracefully when no reflink
#   substrate or no cargo; runs on the provisioned host or with opt-in):
#     Block B3+B4 — warm-skip + path-independence (heavy dep fresh:true, B4 fresh
#                   count equality, B3 wall direction).
#     Block PS    — identical test pass-set warm vs cold.
#     Block B7    — reset-in-place stability over K cycles.
#     Block B6+B1 — lifecycle: in-flight clone independence + provision idempotency.
#
# Env knobs:
#   REIFY_WARM_LANE_MOUNT        — pre-existing XFS-reflink mount to use as
#                                  substrate (skips provision step).
#   REIFY_RUN_WARM_LANE_GATE     — set to 1 to opt-in to provisioning a small
#                                  ephemeral loopback via provision-warm-lane-fs.sh
#                                  when no mount is available.
#   REIFY_WARM_LANE_GATE_DEP_FNS — number of trivial fns in the heavy dep crate
#                                  (default: 200; tune for timing signal).
#   REIFY_WARM_LANE_GATE_RESET_CYCLES — number of reset-in-place cycles for B7
#                                  (default: 3).
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== warm-lane pool end-to-end integration gate (task #4662) ==="

# ─────────────────────────────────────────────────────────────────────────────
# Resolved paths for the four warm-lane scripts (systems-under-test; read-only)
# ─────────────────────────────────────────────────────────────────────────────
PROVISION_SCRIPT="$REPO_ROOT/scripts/provision-warm-lane-fs.sh"
SEED_SCRIPT="$REPO_ROOT/scripts/seed-warm-lane.sh"
REFRESH_SCRIPT="$REPO_ROOT/scripts/refresh-warm-base.sh"
PREFLIGHT_SCRIPT="$REPO_ROOT/scripts/warm-lane-preflight.sh"

# ─────────────────────────────────────────────────────────────────────────────
# Shared temp state + cleanup trap
# ─────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

# ─────────────────────────────────────────────────────────────────────────────
test_summary
