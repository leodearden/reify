#!/usr/bin/env bash
# Self-launching one-command acceptance runner for the task-4202 Find-uses smoke.
#
# Usage:
#   bash gui/test/visual/run_find_uses_smoke.sh
#   # or via npm:
#   npm --prefix gui run test:smoke:find-uses
#
# Fixture: gui/test/fixtures/find_uses_smoke.ri
# Driver:  gui/test/visual/smoke_find_uses.mjs (the 4202 drive-only driver)
#
# The whole launch/readiness/teardown lifecycle — port resolution, DISPLAY and
# library-path hygiene, the EXIT/INT/TERM reap trap, the optional pre-build, the
# backgrounded launcher, the readiness+liveness poll loop and the SIGTERM
# teardown — lives in gui/test/visual/lib_e2e_smoke.sh, shared with the five
# sibling e2e smoke runners.  Read that file for the lifecycle contract,
# including the readiness-race fix (design decision 5, task 4456 step-5).
#
# NOTE (task 5596): this runner previously carried its own copy of the lifecycle
# and, being the oldest of the six, had drifted — it lacked the
# /opt/reify-deps/lib LD_LIBRARY_PATH block and the WebKit DMABuf disable that
# its five siblings already had.  Sourcing the shared library gives it that env
# hygiene.  Both blocks are conditional/defaulted, so neither can regress a host
# where they were previously absent.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib_e2e_smoke.sh
source "$SCRIPT_DIR/lib_e2e_smoke.sh"

e2e_smoke_run \
    --name run_find_uses_smoke \
    --fixture gui/test/fixtures/find_uses_smoke.ri \
    --driver gui/test/visual/smoke_find_uses.mjs
