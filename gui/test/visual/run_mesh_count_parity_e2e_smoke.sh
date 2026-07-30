#!/usr/bin/env bash
# Self-launching one-command runner for the task-5367 cross-layer mesh-count
# parity smoke (follow-up from task 5348).
#
# NOT a visual-regression gate, despite the directory it shares: the driver
# captures no screenshot and diffs no baseline (that is gui/test/visual/run.ts).
# Its only assertion is the three-way numeric parity check.
#
# Usage:
#   bash gui/test/visual/run_mesh_count_parity_e2e_smoke.sh
#   # or via npm:
#   npm --prefix gui run test:smoke:mesh-count-parity
#
# Fixture: gui/test/fixtures/large_assembly.ri
# Driver:  gui/test/visual/smoke_mesh_count_parity_e2e.mjs (the parity driver)
#
# The whole launch/readiness/teardown lifecycle — port resolution, DISPLAY and
# library-path hygiene, the EXIT/INT/TERM reap trap, the optional pre-build, the
# backgrounded launcher, the readiness+liveness poll loop, the post-driver
# post-mortem and the SIGTERM teardown — lives in gui/test/visual/lib_e2e_smoke.sh,
# shared with the five sibling e2e smoke runners.  Read that file for the
# lifecycle contract, including the readiness-race fix (design decision 5,
# task 4456 step-5) and why the bash-side readiness gate and the driver's own
# waitForServer(60_000) are both intentional.  It replaces the copy this runner
# used to carry (task 5596).
#
# The launcher-death liveness contract is pinned behaviorally by
# tests/infra/test_mesh_count_parity_smoke_runner.sh, which drives this script
# through REIFY_SMOKE_SKIP_PREBUILD / REIFY_SMOKE_LAUNCHER / REIFY_SMOKE_WAIT_MS.
# That guard is deliberately left UNCHANGED across the 5596 consolidation: the
# shared library retains the exact `launcher exited early (rc=` substring it
# greps for, so its continued pass is byte-level evidence that extracting the
# lifecycle regressed nothing here.
#
# LIVE-ONLY — not CI/verify-gated.  Waivable when blocked solely on the debug-port
# gap (esc-4202-61).  The deterministic gate is gui/test/visual/meshCountParity.test.ts
# (the whole decision function) plus 5348's commands_tests.rs full-scene tests.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib_e2e_smoke.sh
source "$SCRIPT_DIR/lib_e2e_smoke.sh"

e2e_smoke_run \
    --name run_mesh_count_parity_e2e_smoke \
    --fixture gui/test/fixtures/large_assembly.ri \
    --driver gui/test/visual/smoke_mesh_count_parity_e2e.mjs
