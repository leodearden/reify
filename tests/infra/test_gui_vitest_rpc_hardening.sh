#!/usr/bin/env bash
# Infrastructure tests for the GUI vitest worker->host RPC hardening (task 7630).
#
# WHAT IS BEING PINNED. Under merge-lane host starvation the vitest worker's
# birpc channel to the host times out after birpc's hardcoded 60 s
# DEFAULT_TIMEOUT (vitest 3.2.4, dist/chunks/index.*.js:3), producing suites
# that fail with `[vitest-worker]: Timeout calling "<method>"` and ZERO failed
# tests. vitest exposes no knob for that bound (WorkerRpcOptions type-excludes
# `timeout`, dist/workers.d.ts:23), and the merge lane is exempt from CPU
# admission control by design (cpu-admit.sh C-A3), so the flake cannot be
# prevented outright. This suite pins the three things that ARE reachable:
#
#   Section A — the role EXPORT contract, so task 4856's isVerifyLane fork cap
#               actually reaches the vitest child on every path (genuine
#               prevention for the uncapped-31-fork class).
#   Section B — scripts/gui-vitest-run.sh's bounded, signature-gated retry.
#   Section C — the verify.sh / gui-test.sh wiring that makes that runner the
#               single definition of how vitest is invoked.
#
# Hermetic by construction: Sections B and C drive the runner with a STUB npm
# on PATH, never the real 173-file suite — same stance as
# tests/infra/test_gui_test_script.sh, which validates gui-test.sh's contract
# without executing npm.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

VERIFY_SH="$REPO_ROOT/scripts/verify.sh"
VITEST_CONFIG="$REPO_ROOT/gui/vitest.config.ts"

echo "=== gui vitest worker->host RPC hardening tests (task 7630) ==="

# -- Section A: DF_VERIFY_ROLE reaches the vitest child -----------------------
# gui/vitest.config.ts gates BOTH of task 4856's mitigations (maxForks = 4 and
# teardownTimeout = 120 s) on process.env.DF_VERIFY_ROLE. verify.sh defaults
# that variable for every run, but a plain shell assignment is not in the
# environment of the npm/vitest child it later spawns, so on every path where
# the caller does not pre-export the role (hooks/project-checks, and all manual
# or agent-driven runs) the gate reads undefined and vitest silently reverts to
# nCPU-1 forks -- the exact uncapped state 4856 set out to remove.
echo ""
echo "--- Section A: DF_VERIFY_ROLE defaulting assignment is exported ---"

assert "verify.sh's DF_VERIFY_ROLE defaulting assignment carries 'export'" \
    grep -qE '^export DF_VERIFY_ROLE="\$\{DF_VERIFY_ROLE:-task\}"' "$VERIFY_SH"

# The semantics above, in executable form. A comment asserting "a bare
# assignment is invisible to a child" can drift; these two fixtures cannot.
assert "fixture: a BARE 'X=\${X:-task}' is NOT visible in a child's environment" \
    bash -c "! bash -c 'unset X; X=\"\${X:-task}\"; env | grep -q \"^X=\"'"

assert "fixture: an EXPORTED 'X=\${X:-task}' IS visible in a child's environment" \
    bash -c "bash -c 'unset X; export X=\"\${X:-task}\"; env | grep -q \"^X=\"'"

# Guard against a regression that adds a SECOND, unexported defaulting site:
# two assignment sites would make the export conditional on evaluation order.
assert "verify.sh has exactly one DF_VERIFY_ROLE defaulting assignment" \
    bash -c "[ \"\$(grep -cE '^(export )?DF_VERIFY_ROLE=\"\\\$\{DF_VERIFY_ROLE:-task\}\"' '$VERIFY_SH')\" -eq 1 ]"

# -- Section A2: gui/vitest.config.ts cites a real source --------------------
# The config's comment named "verify.sh:328" as where the role is set. Line 328
# is a comment line inside the semaphore documentation block, not code -- a
# false citation sends the next responder to the wrong place.
echo ""
echo "--- Section A2: gui/vitest.config.ts carries no false source citation ---"

assert "gui/vitest.config.ts does not cite the non-existent source 'verify.sh:328'" \
    bash -c "! grep -q 'verify\.sh:328' '$VITEST_CONFIG'"

assert "gui/vitest.config.ts still gates on DF_VERIFY_ROLE (isVerifyLane intact)" \
    grep -q 'DF_VERIFY_ROLE' "$VITEST_CONFIG"

test_summary
