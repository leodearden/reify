#!/usr/bin/env bash
# Infrastructure test for task 4750 (κ — crate-graph-aware OverlapFootprintDetector).
# Verifies that:
#   1. python3 is on PATH
#   2. The dark-factory γ seam (orchestrator.overlap_footprint) is importable;
#      if not, the test SKIPs cleanly (never RED in a bare reify clone without
#      the dark-factory venv).
#   3. scripts/test_reify_overlap_detector.py (stdlib unittest) exits 0.
#   4. Import smoke: reify_overlap_detector is importable as a sibling module.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_reify_overlap_detector ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── SKIP-guard: dark-factory γ seam must be importable ────────────────────
# Mirrors the `import yaml`/`import tomllib` SKIP idiom in:
#   tests/infra/test_cpu_governance_config.sh (import yaml)
#   tests/infra/test_cargo_incremental_lane_decision.sh (import tomllib)
if ! python3 -c 'import orchestrator.overlap_footprint' 2>/dev/null; then
    echo "SKIP: orchestrator.overlap_footprint not importable (dark-factory venv absent)"
    echo "      This test only runs in the orchestrator verify environment."
    exit 0
fi

# ── Unit tests ────────────────────────────────────────────────────────────
assert "scripts/test_reify_overlap_detector.py exits 0" \
    python3 "$ROOT/scripts/test_reify_overlap_detector.py"

# ── Import smoke ──────────────────────────────────────────────────────────
assert "reify_overlap_detector is importable as a sibling module" \
    python3 -c "import sys; sys.path.insert(0, '$ROOT/scripts'); import reify_overlap_detector"

test_summary
