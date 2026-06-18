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
# Block A — Script-presence / CLI-stability preconditions (ALWAYS-RUN)
# Each of the 4 warm-lane scripts must exist as an executable, and --help must
# exit 0 and print "usage" or "Usage" on stderr.
# The verify-pipeline-infra-tests.txt map must contain a drift-guard row that
# routes a warm-lane script edit to this gate.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: script-presence / CLI-stability ---"

_VP_INFRA_MAP="$REPO_ROOT/scripts/verify-pipeline-infra-tests.txt"

# ── A1: provision-warm-lane-fs.sh ────────────────────────────────────────────
assert "A1: provision-warm-lane-fs.sh exists and is executable" \
    test -x "$PROVISION_SCRIPT"
_A1_ERR="$(bash "$PROVISION_SCRIPT" --help 2>&1 >/dev/null)" || true
_A1_RC=0; bash "$PROVISION_SCRIPT" --help >/dev/null 2>&1 || _A1_RC=$?
assert "A1: provision-warm-lane-fs.sh --help exits 0" \
    test "$_A1_RC" -eq 0
assert "A1: provision-warm-lane-fs.sh --help prints usage on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$_A1_ERR"

# ── A2: seed-warm-lane.sh ─────────────────────────────────────────────────────
assert "A2: seed-warm-lane.sh exists and is executable" \
    test -x "$SEED_SCRIPT"
_A2_ERR="$(bash "$SEED_SCRIPT" --help 2>&1 >/dev/null)" || true
_A2_RC=0; bash "$SEED_SCRIPT" --help >/dev/null 2>&1 || _A2_RC=$?
assert "A2: seed-warm-lane.sh --help exits 0" \
    test "$_A2_RC" -eq 0
assert "A2: seed-warm-lane.sh --help prints usage on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$_A2_ERR"

# ── A3: refresh-warm-base.sh ──────────────────────────────────────────────────
assert "A3: refresh-warm-base.sh exists and is executable" \
    test -x "$REFRESH_SCRIPT"
_A3_ERR="$(bash "$REFRESH_SCRIPT" --help 2>&1 >/dev/null)" || true
_A3_RC=0; bash "$REFRESH_SCRIPT" --help >/dev/null 2>&1 || _A3_RC=$?
assert "A3: refresh-warm-base.sh --help exits 0" \
    test "$_A3_RC" -eq 0
assert "A3: refresh-warm-base.sh --help prints usage on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$_A3_ERR"

# ── A4: warm-lane-preflight.sh ───────────────────────────────────────────────
assert "A4: warm-lane-preflight.sh exists and is executable" \
    test -x "$PREFLIGHT_SCRIPT"
_A4_ERR="$(bash "$PREFLIGHT_SCRIPT" --help 2>&1 >/dev/null)" || true
_A4_RC=0; bash "$PREFLIGHT_SCRIPT" --help >/dev/null 2>&1 || _A4_RC=$?
assert "A4: warm-lane-preflight.sh --help exits 0" \
    test "$_A4_RC" -eq 0
assert "A4: warm-lane-preflight.sh --help prints usage on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$_A4_ERR"

# ── A5: drift-guard map contains a row for a warm-lane script → this gate ────
# At least one row in verify-pipeline-infra-tests.txt must map a warm-lane
# script artifact to a glob that matches tests/infra/test_warm_lane_pool.sh.
# This ensures that a future edit to provision/seed/refresh/preflight will
# re-exercise this integration gate at task-scope verify time.
assert "A5: verify-pipeline-infra-tests.txt exists" \
    test -f "$_VP_INFRA_MAP"
assert "A5: drift-guard map has a warm-lane-script → test_warm_lane_pool.sh row" \
    bash -c '
        map="$1"
        # Look for any non-comment row whose artifact column matches a warm-lane script
        # and whose test-glob column would fnmatch tests/infra/test_warm_lane_pool.sh.
        while IFS= read -r line; do
            [[ "$line" =~ ^[[:space:]]*# ]] && continue
            [[ -z "${line// }" ]] && continue
            artifact=$(awk "{print \$1}" <<< "$line")
            glob=$(awk "{print \$2}" <<< "$line")
            case "$artifact" in
                scripts/*warm-lane*.sh|scripts/*warm_lane*.sh|scripts/provision-warm-lane-fs.sh|\
scripts/seed-warm-lane.sh|scripts/refresh-warm-base.sh|scripts/warm-lane-preflight.sh) ;;
                *) continue ;;
            esac
            # Check if the glob matches this gate file
            case "tests/infra/test_warm_lane_pool.sh" in
                $glob) exit 0 ;;
            esac
        done < "$map"
        exit 1
    ' _ "$_VP_INFRA_MAP"

# ─────────────────────────────────────────────────────────────────────────────
test_summary
