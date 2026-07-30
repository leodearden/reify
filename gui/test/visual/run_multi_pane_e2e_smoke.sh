#!/usr/bin/env bash
# Self-launching one-command acceptance runner for the task-4769 multi-pane ζ smoke.
#
# Usage:
#   bash gui/test/visual/run_multi_pane_e2e_smoke.sh
#   # or via npm:
#   npm --prefix gui run test:smoke:multipane
#
# Fixture: examples/multi_pane_viewport.ri
# Driver:  gui/test/visual/smoke_multi_pane_e2e.mjs (the ζ driver)
#
# The whole launch/readiness/teardown lifecycle — port resolution, DISPLAY and
# library-path hygiene, the EXIT/INT/TERM reap trap, the optional pre-build, the
# backgrounded launcher, the readiness+liveness poll loop, the post-driver
# post-mortem and the SIGTERM teardown — lives in gui/test/visual/lib_e2e_smoke.sh,
# shared with the five sibling e2e smoke runners.  Read that file for the
# lifecycle contract, including the readiness-race fix (design decision 5,
# task 4456 step-5) and why the bash-side readiness gate and the driver's own
# waitForServer(60_000) are both intentional (design decision 2, task 4456).
# It replaces the copy this runner used to carry (task 5596).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib_e2e_smoke.sh
source "$SCRIPT_DIR/lib_e2e_smoke.sh"

e2e_smoke_run \
    --name run_multi_pane_e2e_smoke \
    --fixture examples/multi_pane_viewport.ri \
    --driver gui/test/visual/smoke_multi_pane_e2e.mjs
