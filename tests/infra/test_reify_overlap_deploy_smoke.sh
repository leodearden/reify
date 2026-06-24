#!/usr/bin/env bash
# Deploy-readiness smoke for task 4751 (ξ — two-layer merge queue CAPSTONE).
#
# Asserts the user-observable deploy signal: register_for_reify() under the real
# "reify" project id + real cargo loader causes changesets_overlap() to return
# True for a same-crate/different-file pair of crates/reify-eval/ files, while
# the DEFAULT path detector returns False for the same pair.
#
# This is NOT a re-test of κ/4750's detector logic (covered by
# scripts/test_reify_overlap_detector.py using a synthetic fixture).  The
# deploy-specific signal NOT covered by κ is the end-to-end real path:
#   register_for_reify() under real "reify" id + real `cargo metadata` workspace.
#
# SKIP: if the dark-factory γ seam (orchestrator.overlap_footprint) is not
# importable — bare reify clone without the orchestrator venv.  This smoke
# NEVER goes RED in a plain `cargo test` / `scripts/verify.sh` run on a
# developer workstation without the dark-factory venv.
#
# Green after: orchestrator-reify.service is restarted with ν/dark_factory:1897
# wiring active (register_for_reify() called at orchestrator startup).
#
# NOTE: This smoke is a static registration-contract check — it passes both
# before and after the restart once ν is deployed.  The genuine RED→GREEN
# deploy signal is the live orchestrator heartbeat; see the "Live heartbeat"
# section in docs/architecture-audit/two-layer-merge-queue-reify-deploy.md.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_reify_overlap_deploy_smoke ==="

# ── SKIP-guard: python3 + dark-factory γ seam must be importable ──────────
# Mirrors the idiom in tests/infra/test_reify_overlap_detector.sh:31-35.
# python3 presence is folded into the guard so no FAIL is printed before exit 0
# (avoids a masked-FAIL: assert would fire, increment FAIL, then SKIP exit 0).
if ! command -v python3 >/dev/null 2>&1 || ! python3 -c 'import orchestrator.overlap_footprint' 2>/dev/null; then
    echo "SKIP: orchestrator.overlap_footprint not importable (dark-factory venv absent)"
    echo "      This test only runs in the orchestrator verify environment."
    exit 0
fi

# ── Deploy-readiness assertions ────────────────────────────────────────────
# Each assertion runs as a hermetic python3 -c subprocess — does NOT mutate
# the live orchestrator registry.  Snapshot/restore of ov._DETECTORS mirrors
# scripts/test_reify_overlap_detector.py test_registration_round_trip.
#
# Assertion (i): register_for_reify() + changesets_overlap("reify", ...) → True.
# Two distinct crates/reify-eval/src/*.rs paths share the crate:reify-eval
# footprint member; overlap is True even if cargo metadata fails (fail-wide _ALL
# sentinel), so the assertion is robust to cargo unavailability in CI.
assert "register_for_reify() causes changesets_overlap(reify, ...) to return True for same-crate pair" \
    python3 -c "
import sys
sys.path.insert(0, '$ROOT/scripts')
import reify_overlap_detector as rod
import orchestrator.overlap_footprint as ov

original = dict(ov._DETECTORS)
try:
    rod.register_for_reify()
    result = ov.changesets_overlap(
        'reify',
        ['crates/reify-eval/src/lib.rs'],
        ['crates/reify-eval/src/engine_build.rs'],
    )
    assert result is True, repr(result)
finally:
    ov._DETECTORS.clear()
    ov._DETECTORS.update(original)
"

# Assertion (ii): DEFAULT path detector → False for the same pair.
# Proves the registered CrateGraphOverlapDetector is what makes the difference
# (mirrors scripts/test_reify_overlap_detector.py:382-391).
assert "DEFAULT path detector returns False for same-crate/different-file pair (unregistered project id)" \
    python3 -c "
import sys
sys.path.insert(0, '$ROOT/scripts')
import orchestrator.overlap_footprint as ov

result = ov.changesets_overlap(
    'reify-deploy-smoke-unregistered',
    ['crates/reify-eval/src/lib.rs'],
    ['crates/reify-eval/src/engine_build.rs'],
)
assert result is False, repr(result)
"

test_summary
