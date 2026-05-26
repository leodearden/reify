#!/usr/bin/env bash
# Infrastructure test for task 3666.
# Validates that orchestrator.yaml's lint_command includes a guarded invocation
# of scripts/check_event_inventory.sh in warning mode (no --strict, no
# --bidirectional), following the test_pm_standardization.sh convention.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== check_event_inventory.sh wiring tests ==="

# Since task 3766 the orchestrator runs scripts/verify.sh, so check_event_inventory
# wiring is asserted against the verify.sh lint plan, not orchestrator.yaml.
# --include-infra so the lint-side infra leaf appears; --scope all for the full
# plan; env lines stripped via `grep -v '^#'`.
LINT_PLAN_SEGS="$(bash "$REPO_ROOT/scripts/verify.sh" lint --scope all --include-infra --print-plan | grep -v '^#')"
TEST_PLAN_SEGS="$(bash "$REPO_ROOT/scripts/verify.sh" test --profile both --scope all --include-infra --print-plan | grep -v '^#')"
export LINT_PLAN_SEGS TEST_PLAN_SEGS

# -- (a): script is referenced in the lint plan --------------------------------
echo ""
echo "--- (a): scripts/check_event_inventory.sh is in the lint plan ---"

assert "lint plan contains 'scripts/check_event_inventory.sh'" \
    bash -c "printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -q 'scripts/check_event_inventory.sh'"

# -- (b): if-test-f guard is used ---------------------------------------------
echo ""
echo "--- (b): if test -f guard is used in the lint plan ---"

assert "lint plan contains 'if test -f scripts/check_event_inventory.sh'" \
    bash -c "printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -q 'if test -f scripts/check_event_inventory.sh'"

# -- (c): WARNING echo is present for guard-skip branch -----------------------
echo ""
echo "--- (c): WARNING echo for guard-skip branch in the lint plan ---"

assert "lint plan has WARNING echo for check_event_inventory.sh skip" \
    bash -c "printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -q 'WARNING.*check_event_inventory'"

# -- (d): warning mode — no --strict, no --bidirectional flags ----------------
echo ""
echo "--- (d): invocation is in warning mode (no --strict, no --bidirectional) ---"

assert "lint plan does NOT invoke check_event_inventory.sh with --strict" \
    bash -c "! printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -qE 'bash scripts/check_event_inventory\.sh[^;|&]*--strict'"

assert "lint plan does NOT invoke check_event_inventory.sh with --bidirectional" \
    bash -c "! printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -qE 'bash scripts/check_event_inventory\.sh[^;|&]*--bidirectional'"

# -- (e): invocation is wrapped with timeout --kill-after=60 ------------------
echo ""
echo "--- (e): timeout --kill-after=60 wraps the invocation in lint_command ---"

# TIMEOUT_PATTERN is the exact-shape regex that matches only the scoped
# timeout wrapping check_event_inventory.sh's own invocation.  It cannot
# span across &&-separated clauses because every segment between
# 'timeout --kill-after=60' and 'check_event_inventory.sh' is a short
# literal/anchored class — no greedy '.*' that would cross clause boundaries.
# The exact-shape pattern:
#   timeout --kill-after=60 <digits>m bash scripts/check_event_inventory.sh
# cannot cross clause boundaries because every segment between
# 'timeout --kill-after=60' and 'check_event_inventory.sh' is a short
# literal/anchored class with no greedy '.*'.
TIMEOUT_PATTERN='timeout --kill-after=60 [0-9]+m bash scripts/check_event_inventory\.sh'

assert "lint plan wraps check_event_inventory.sh with 'timeout --kill-after=60'" \
    bash -c "printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -qE '$TIMEOUT_PATTERN'"

# Synthetic-negative: TIMEOUT_PATTERN must NOT match a malformed invocation
# where 'timeout' appears on a *different* clause from check_event_inventory.sh.
# A greedy '.*' regex silently passes this case; the tight pattern rejects it.
assert "TIMEOUT_PATTERN rejects: timeout on different clause than check_event_inventory.sh" \
    bash -c "! echo 'lint_command: timeout --kill-after=60 30m cargo clippy && bash scripts/check_event_inventory.sh' | grep -qE '$TIMEOUT_PATTERN'"

# Synthetic-negative (defense-in-depth): TIMEOUT_PATTERN must NOT match a
# path-only reference where the script is invoked with 'cat' (not 'bash')
# and has no scoped timeout.  Confirms the 'bash scripts/' literal in the
# tight pattern blocks false positives even for path-only matches.
assert "TIMEOUT_PATTERN rejects: path-only reference without scoped timeout (cat instead of bash)" \
    bash -c "! echo 'lint_command: stuff && timeout --kill-after=60 30m cargo clippy && cat scripts/check_event_inventory.sh' | grep -qE '$TIMEOUT_PATTERN'"

# -- (f): script is NOT in the test plan (placement in lint only) --------------
echo ""
echo "--- (f): scripts/check_event_inventory.sh is NOT in the test plan ---"

assert "test plan does NOT reference scripts/check_event_inventory.sh" \
    bash -c "! printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -q 'scripts/check_event_inventory.sh'"

# -- (g): script file exists and is executable on disk -------------------------
echo ""
echo "--- (g): scripts/check_event_inventory.sh exists and is executable ---"

assert "scripts/check_event_inventory.sh exists" \
    test -f "$REPO_ROOT/scripts/check_event_inventory.sh"

assert "scripts/check_event_inventory.sh is executable" \
    test -x "$REPO_ROOT/scripts/check_event_inventory.sh"

# -- (h): script runs cleanly in warning mode against current worktree ---------
echo ""
echo "--- (h): check_event_inventory.sh exits 0 in warning mode ---"

assert "bash scripts/check_event_inventory.sh --repo-root REPO_ROOT exits 0" \
    bash "$REPO_ROOT/scripts/check_event_inventory.sh" --repo-root "$REPO_ROOT"

assert "bash scripts/check_event_inventory.sh exits 0 with CWD=repo root (mirrors lint_command invocation)" \
    bash -c "cd '$REPO_ROOT' && bash scripts/check_event_inventory.sh"

test_summary
