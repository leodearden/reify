#!/usr/bin/env bash
# Self-launching one-command acceptance runner for the task-9730 launcher Start
# view smoke (alpha-spine β) — the open scenario: B1 Start view, B5 Debug mode, B4 coded failed open, B2 open.
#
# Usage:
#   bash gui/test/visual/run_launcher_e2e_smoke.sh
#   # or via npm:
#   npm --prefix gui run test:smoke:launcher
#
# Fixture: none — reify-gui launches with no file, on the Start view.
# Driver:  gui/test/visual/smoke_launcher_e2e.mjs (REIFY_LAUNCHER_SMOKE_SCENARIO=open)
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

# A fresh app-data home per run (Tauri's app_data_dir follows XDG_DATA_HOME):
# the Recent list starts empty, and a smoke never writes into the developer's
# own Recent list or WebKit storage.
XDG_DATA_HOME="$(mktemp -d "${TMPDIR:-/tmp}/reify-launcher-smoke-data.XXXXXX")"
export XDG_DATA_HOME
export REIFY_LAUNCHER_SMOKE_SCENARIO=open

# e2e_smoke_run exits the shell it runs in, so it runs in a subshell here and
# the per-run data home is removed afterwards, whatever the outcome.
rc=0
(
    e2e_smoke_run \
        --name run_launcher_e2e_smoke \
        --no-fixture \
        --driver gui/test/visual/smoke_launcher_e2e.mjs
) || rc=$?
rm -rf "$XDG_DATA_HOME"
exit "$rc"
