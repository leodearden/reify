#!/usr/bin/env bash
# Self-launching one-command runner for the task-7680 slider/edit-box write-back
# gate (INV-GUI-3, the live half of task 5099 η).
#
# Usage:
#   bash gui/test/visual/run_slider_write_back_e2e_smoke.sh
#   # or via npm:
#   npm --prefix gui run test:smoke:slider-write-back
#
# Fixture: gui/test/fixtures/small_cube.ri
# Driver:  gui/test/visual/smoke_slider_write_back_e2e.mjs
#
# THE FIXTURE IS NOT THE SUBJECT. It is only what the launcher boots with. The
# driver copies gui/test/fixtures/slider_write_back.ri to a mkdtemp directory
# and opens the COPY itself, because the gate rewrites the subject on disk and
# the tracked fixture must never be that file.
#
# The launch/readiness/teardown lifecycle lives in gui/test/visual/lib_e2e_smoke.sh,
# shared with the sibling e2e smoke runners.
#
# LIVE-ONLY — not CI/verify-gated. The halves that DO run in CI are
# gui/test/visual/sliderWriteBackGate.test.ts (this driver's whole decision
# function, as pure data) and gui/src/__tests__/debugFormControl.test.tsx (the
# scrub_range_input / edit_text_input tools against the real components).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib_e2e_smoke.sh
source "$SCRIPT_DIR/lib_e2e_smoke.sh"

e2e_smoke_run \
    --name run_slider_write_back_e2e_smoke \
    --fixture gui/test/fixtures/small_cube.ri \
    --driver gui/test/visual/smoke_slider_write_back_e2e.mjs
