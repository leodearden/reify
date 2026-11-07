#!/usr/bin/env bash
# Self-launching one-command runner for the task-5098 Y-rail lengthening
# integration gate (PRD docs/prds/v0_6/ai-native-editing.md §7 leaf ζ).
#
# NOT a visual-regression gate, despite the directory it shares: the driver
# captures no screenshot and diffs no baseline (that is gui/test/visual/run.ts).
#
# Usage:
#   bash gui/test/visual/run_rail_lengthening_e2e_smoke.sh
#   # or via npm:
#   npm --prefix gui run test:smoke:rail-lengthening
#
# Fixture: gui/test/fixtures/small_cube.ri
# Driver:  gui/test/visual/smoke_rail_lengthening_e2e.mjs
#
# THE FIXTURE IS NOT THE SUBJECT. It is only what the launcher boots with — a
# small file that opens fast. The driver then copies prj/printer_v01/ to a
# mkdtemp directory and opens the COPY itself, because the gate's on-disk
# assertion needs a file that `reify_set_parameter` may really rewrite, and the
# tracked design must never be that file. See the driver's header.
#
# The whole launch/readiness/teardown lifecycle — port resolution, DISPLAY and
# library-path hygiene, the EXIT/INT/TERM reap trap, the optional pre-build, the
# backgrounded launcher, the readiness+liveness poll loop, the post-driver
# post-mortem and the SIGTERM teardown — lives in gui/test/visual/lib_e2e_smoke.sh,
# shared with the six sibling e2e smoke runners. Read that file for the lifecycle
# contract, including why the bash-side readiness gate and the driver's own
# waitForServer(60_000) are both intentional.
#
# LIVE-ONLY — not CI/verify-gated. The deterministic gates that DO run in CI are
# gui/test/visual/railLengtheningGate.test.ts (this driver's whole decision
# function, as pure data) and
# gui/src-tauri/src/tests/debug_boundary_tests.rs::write_tool_payload_carries_a_flipped_constraint_status
# (that a constraint-status flip survives the write-tool payload, headlessly).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib_e2e_smoke.sh
source "$SCRIPT_DIR/lib_e2e_smoke.sh"

e2e_smoke_run \
    --name run_rail_lengthening_e2e_smoke \
    --fixture gui/test/fixtures/small_cube.ri \
    --driver gui/test/visual/smoke_rail_lengthening_e2e.mjs
