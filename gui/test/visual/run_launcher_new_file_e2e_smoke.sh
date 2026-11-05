#!/usr/bin/env bash
# Self-launching one-command acceptance runner for the task-9730 launcher Start
# view smoke (alpha-spine β) — the new-file scenario: B1 Start view, B3 New… design created, declared, compiled and rendered.
#
# Usage:
#   bash gui/test/visual/run_launcher_new_file_e2e_smoke.sh
#   # or via npm:
#   npm --prefix gui run test:smoke:launcher-new-file
#
# Fixture: none — reify-gui launches with no file, on the Start view.
# Driver:  gui/test/visual/smoke_launcher_e2e.mjs (REIFY_LAUNCHER_SMOKE_SCENARIO=new)
#
# The launch/readiness/teardown lifecycle lives in gui/test/visual/lib_e2e_smoke.sh,
# shared with the sibling e2e smoke runners; read that file for its contract.
#
# LIVE-ONLY — not CI/verify-gated. Waivable when blocked solely on the debug-port
# gap (esc-4202-61). The deterministic gate is gui/test/visual/launcherGate.test.ts
# plus the StartView/launcherActions/App vitest suites and the reify-gui Rust tests.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib_e2e_smoke.sh
source "$SCRIPT_DIR/lib_e2e_smoke.sh"

export REIFY_LAUNCHER_SMOKE_SCENARIO=new

e2e_smoke_run \
    --name run_launcher_new_file_e2e_smoke \
    --no-fixture \
    --driver gui/test/visual/smoke_launcher_e2e.mjs
