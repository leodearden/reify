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

ORCH="$REPO_ROOT/orchestrator.yaml"

# -- (a): script is referenced in lint_command ---------------------------------
echo ""
echo "--- (a): scripts/check_event_inventory.sh is in lint_command ---"

assert "lint_command contains 'scripts/check_event_inventory.sh'" \
    bash -c "grep 'lint_command:' '$ORCH' | grep -q 'scripts/check_event_inventory.sh'"

# -- (b): if-test-f guard is used ---------------------------------------------
echo ""
echo "--- (b): if test -f guard is used in lint_command ---"

assert "lint_command contains 'if test -f scripts/check_event_inventory.sh'" \
    bash -c "grep 'lint_command:' '$ORCH' | grep -q 'if test -f scripts/check_event_inventory.sh'"

# -- (c): WARNING echo is present for guard-skip branch -----------------------
echo ""
echo "--- (c): WARNING echo for guard-skip branch in lint_command ---"

assert "lint_command has WARNING echo for check_event_inventory.sh skip" \
    bash -c "grep 'lint_command:' '$ORCH' | grep -q 'WARNING.*check_event_inventory'"

# -- (d): warning mode — no --strict, no --bidirectional flags ----------------
echo ""
echo "--- (d): invocation is in warning mode (no --strict, no --bidirectional) ---"

assert "lint_command does NOT invoke check_event_inventory.sh with --strict" \
    bash -c "! grep 'lint_command:' '$ORCH' | grep -qE 'bash scripts/check_event_inventory\.sh[^;|&]*--strict'"

assert "lint_command does NOT invoke check_event_inventory.sh with --bidirectional" \
    bash -c "! grep 'lint_command:' '$ORCH' | grep -qE 'bash scripts/check_event_inventory\.sh[^;|&]*--bidirectional'"

# -- (e): invocation is wrapped with timeout --kill-after=60 ------------------
echo ""
echo "--- (e): timeout --kill-after=60 wraps the invocation in lint_command ---"

# TIMEOUT_PATTERN is the exact-shape regex that matches only the scoped
# timeout wrapping check_event_inventory.sh's own invocation.  It cannot
# span across &&-separated clauses because every segment between
# 'timeout --kill-after=60' and 'check_event_inventory.sh' is a short
# literal/anchored class — no greedy '.*' that would cross clause boundaries.
# (step-3: intentionally left as the OLD loose greedy form so the synthetic-
# negative sub-assertions below fail and demonstrate the bug they catch.)
TIMEOUT_PATTERN='timeout --kill-after=60.*check_event_inventory\.sh'

assert "lint_command wraps check_event_inventory.sh with 'timeout --kill-after=60'" \
    bash -c "grep 'lint_command:' '$ORCH' | grep -qE '$TIMEOUT_PATTERN'"

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

# -- (f): script is NOT in test_command (placement in lint only) ---------------
echo ""
echo "--- (f): scripts/check_event_inventory.sh is NOT in test_command ---"

assert "test_command does NOT reference scripts/check_event_inventory.sh" \
    bash -c "! grep 'test_command:' '$ORCH' | grep -q 'scripts/check_event_inventory.sh'"

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
