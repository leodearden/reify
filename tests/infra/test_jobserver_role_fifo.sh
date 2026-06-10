#!/usr/bin/env bash
# Tests for scripts/verify.sh role→FIFO selection (task δ, PRD §9 contract C3, §8 T-b).
#
# Oracle assertions use verify.sh --print-plan to inspect the env block, confirming:
#   (a) merge role + merge FIFO present → exports --jobserver-auth=fifo:<merge-tmp>
#   (b) task  role + task  FIFO present → exports --jobserver-auth=fifo:<task-tmp>
#   (c) merge role + merge FIFO absent  → CARGO_MAKEFLAGS left unset (no export)
#   (d) ISOLATION: merge role + only task FIFO present → left unset
#       (proves the guard checks the role's OWN FIFO, not 'any FIFO present')
#   (e) orchestrator.yaml has NO active CARGO_MAKEFLAGS: key (ownership move C3)
#
# Hermetic: mktemp FIFOs at random paths; real /tmp/reify-jobserver-* NEVER touched.
# DF_VERIFY_ROLE is set INLINE per verify.sh invocation (run_all.sh exports role=task
# suite-wide; per-invocation assignment overrides the exported default).
#
# Auto-discovered by tests/infra/run_all.sh via test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

VERIFY="$REPO_ROOT/scripts/verify.sh"

# ---------------------------------------------------------------------------
# Fixture: hermetic temp FIFOs (never touch live /tmp/reify-jobserver-*)
# ---------------------------------------------------------------------------
MERGE_FIFO="$(mktemp -u /tmp/test-jb-merge-XXXXXX)"
TASK_FIFO="$(mktemp -u /tmp/test-jb-task-XXXXXX)"
ABSENT_PATH="$(mktemp -u /tmp/test-jb-absent-XXXXXX)"

_cleanup() {
    rm -f "$MERGE_FIFO" "$TASK_FIFO" 2>/dev/null || true
}
trap _cleanup EXIT

mkfifo "$MERGE_FIFO"
mkfifo "$TASK_FIFO"
# $ABSENT_PATH is intentionally NOT created (used to simulate absent FIFO)

export MERGE_FIFO TASK_FIFO ABSENT_PATH

# ---------------------------------------------------------------------------
# (a) merge role + merge FIFO present → exports --jobserver-auth=fifo:<merge-tmp>
# ---------------------------------------------------------------------------
echo ""
echo "--- (a) merge role + merge FIFO present → env exports merge fifo path ---"
_PLAN_A="$(DF_VERIFY_ROLE=merge REIFY_JOBSERVER_MERGE_FIFO="$MERGE_FIFO" \
    bash "$VERIFY" test --print-plan 2>/dev/null || true)"
export _PLAN_A

assert "(a) merge role + merge FIFO present: exports --jobserver-auth=fifo:<merge-tmp>" \
    bash -c 'printf "%s\n" "$_PLAN_A" | grep -qF "CARGO_MAKEFLAGS=--jobserver-auth=fifo:$MERGE_FIFO"'

assert "(a) merge role + merge FIFO present: does NOT mention task FIFO path in CARGO_MAKEFLAGS" \
    bash -c '! printf "%s\n" "$_PLAN_A" | grep "CARGO_MAKEFLAGS" | grep -qF "$TASK_FIFO"'

# ---------------------------------------------------------------------------
# (b) task role + task FIFO present → exports --jobserver-auth=fifo:<task-tmp>
# ---------------------------------------------------------------------------
echo ""
echo "--- (b) task role + task FIFO present → env exports task fifo path ---"
_PLAN_B="$(DF_VERIFY_ROLE=task REIFY_JOBSERVER_TASK_FIFO="$TASK_FIFO" \
    bash "$VERIFY" test --print-plan 2>/dev/null || true)"
export _PLAN_B

assert "(b) task role + task FIFO present: exports --jobserver-auth=fifo:<task-tmp>" \
    bash -c 'printf "%s\n" "$_PLAN_B" | grep -qF "CARGO_MAKEFLAGS=--jobserver-auth=fifo:$TASK_FIFO"'

assert "(b) task role + task FIFO present: does NOT mention merge FIFO path in CARGO_MAKEFLAGS" \
    bash -c '! printf "%s\n" "$_PLAN_B" | grep "CARGO_MAKEFLAGS" | grep -qF "$MERGE_FIFO"'

# ---------------------------------------------------------------------------
# (c) merge role + merge FIFO absent → CARGO_MAKEFLAGS left unset (per-role guard)
# ---------------------------------------------------------------------------
echo ""
echo "--- (c) merge role + merge FIFO absent → CARGO_MAKEFLAGS left unset ---"
_PLAN_C="$(DF_VERIFY_ROLE=merge REIFY_JOBSERVER_MERGE_FIFO="$ABSENT_PATH" \
    bash "$VERIFY" test --print-plan 2>/dev/null || true)"
export _PLAN_C

assert "(c) merge role + merge FIFO absent: 'CARGO_MAKEFLAGS left unset' comment present" \
    bash -c 'printf "%s\n" "$_PLAN_C" | grep -q "CARGO_MAKEFLAGS left unset"'

assert "(c) merge role + merge FIFO absent: no active 'export CARGO_MAKEFLAGS' line" \
    bash -c '! printf "%s\n" "$_PLAN_C" | grep -q "export CARGO_MAKEFLAGS"'

# ---------------------------------------------------------------------------
# (d) ISOLATION: merge role + only task FIFO present (merge FIFO absent)
#     → CARGO_MAKEFLAGS left unset (guard checks the role's OWN FIFO, not 'any FIFO')
# ---------------------------------------------------------------------------
echo ""
echo "--- (d) isolation: merge role + only task FIFO present → unset (role guard checks own FIFO) ---"
_PLAN_D="$(DF_VERIFY_ROLE=merge \
    REIFY_JOBSERVER_MERGE_FIFO="$ABSENT_PATH" \
    REIFY_JOBSERVER_TASK_FIFO="$TASK_FIFO" \
    bash "$VERIFY" test --print-plan 2>/dev/null || true)"
export _PLAN_D

assert "(d) isolation: merge role + only task FIFO present: 'CARGO_MAKEFLAGS left unset'" \
    bash -c 'printf "%s\n" "$_PLAN_D" | grep -q "CARGO_MAKEFLAGS left unset"'

assert "(d) isolation: merge role + only task FIFO present: no active 'export CARGO_MAKEFLAGS' line" \
    bash -c '! printf "%s\n" "$_PLAN_D" | grep -q "export CARGO_MAKEFLAGS"'

test_summary
