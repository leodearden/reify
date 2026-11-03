#!/usr/bin/env bash
# Self-launching one-command acceptance runner for the task-9731 edges-in-black
# toggle smoke (alpha-spine γ, PRD B6).
#
# Usage:
#   bash gui/test/visual/run_edges_toggle_e2e_smoke.sh
#   # or via npm:
#   npm --prefix gui run test:smoke:edges-toggle
#
# Fixture: examples/m5_geometry.ri (the driver opens REIFY_EDGES_SMOKE_FIXTURE
#          instead when set; REIFY_EDGES_SMOKE_SHOTS_DIR keeps the two PNGs)
# Driver:  gui/test/visual/smoke_edges_toggle_e2e.mjs
#
# The launch/readiness/teardown lifecycle lives in gui/test/visual/lib_e2e_smoke.sh,
# shared with the sibling e2e smoke runners; read that file for its contract.
#
# LIVE-ONLY — not CI/verify-gated. Waivable when blocked solely on the debug-port
# gap (esc-4202-61). The deterministic gate is gui/test/visual/edgesToggleGate.test.ts
# plus the meshManager/Viewport/App vitest suites.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib_e2e_smoke.sh
source "$SCRIPT_DIR/lib_e2e_smoke.sh"

e2e_smoke_run \
    --name run_edges_toggle_e2e_smoke \
    --fixture examples/m5_geometry.ri \
    --driver gui/test/visual/smoke_edges_toggle_e2e.mjs
