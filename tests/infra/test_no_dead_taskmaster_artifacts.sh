#!/usr/bin/env bash
# tests/infra/test_no_dead_taskmaster_artifacts.sh
#
# Regression guard: asserts that no dead taskmaster artifacts remain tracked
# in git after the 2026-05-12 taskmaster removal cleanup (task #3638).
#
# Asserts:
#   1. git ls-files .taskmaster/ is empty (directory deleted)
#   2. git ls-files scripts/normalize_tasks_json.py is empty
#   3. git ls-files scripts/validate_tasks_json.py is empty
#   4. git ls-files scripts/refresh_briefing_known_gaps.py is empty
#   5. git ls-files hooks/post-commit is empty
#   6. git ls-files .gitattributes is empty (file held only dead taskmaster config)
#   7. bash scripts/setup-dev.sh --with-orchestrator-hooks exits non-zero (flag removed)
#   8. bash scripts/setup-dev.sh --help exits 0 and emits output (help path intact)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== Dead taskmaster artifact regression guard ==="

# ==============================================================================
# Check 1-6: No dead taskmaster files remain tracked
# ==============================================================================
echo ""
echo "--- Check 1-6: dead artifact files not tracked by git ---"

assert ".taskmaster/ not tracked by git" \
    bash -c '[ -z "$(git -C "$1" ls-files .taskmaster/)" ]' -- "$REPO_ROOT"

assert "scripts/normalize_tasks_json.py not tracked by git" \
    bash -c '[ -z "$(git -C "$1" ls-files scripts/normalize_tasks_json.py)" ]' -- "$REPO_ROOT"

assert "scripts/validate_tasks_json.py not tracked by git" \
    bash -c '[ -z "$(git -C "$1" ls-files scripts/validate_tasks_json.py)" ]' -- "$REPO_ROOT"

assert "scripts/refresh_briefing_known_gaps.py not tracked by git" \
    bash -c '[ -z "$(git -C "$1" ls-files scripts/refresh_briefing_known_gaps.py)" ]' -- "$REPO_ROOT"

assert "hooks/post-commit not tracked by git" \
    bash -c '[ -z "$(git -C "$1" ls-files hooks/post-commit)" ]' -- "$REPO_ROOT"

assert ".gitattributes not tracked by git" \
    bash -c '[ -z "$(git -C "$1" ls-files .gitattributes)" ]' -- "$REPO_ROOT"

# ==============================================================================
# Check 7-8: setup-dev.sh CLI surface
# ==============================================================================
echo ""
echo "--- Check 7-8: setup-dev.sh CLI surface ---"

# --with-orchestrator-hooks must be rejected (flag removed from the script).
# The case statement exits with code 2 on Unknown flag BEFORE any apt/cargo
# commands, so this invocation is safe to run.
assert "setup-dev.sh --with-orchestrator-hooks exits non-zero" \
    bash -c '! bash "$1/scripts/setup-dev.sh" --with-orchestrator-hooks 2>/dev/null' -- "$REPO_ROOT"

# --help must still exit 0 and emit some output.
assert "setup-dev.sh --help exits 0 and prints output" \
    bash -c 'out=$(bash "$1/scripts/setup-dev.sh" --help 2>&1); [ $? -eq 0 ] && [ -n "$out" ]' -- "$REPO_ROOT"

# ==============================================================================
# Check 9: no in-source .taskmaster/tasks/tasks.json references
# ==============================================================================
# The audit skill files and the reify-audit binary source must not contain the
# dead default path. After task 3731 made --tasks-file a required flag, any
# re-introduction of this string would silently re-introduce the regression.
echo ""
echo "--- Check 9: no .taskmaster/tasks/tasks.json in audit skill files or binary source ---"

assert "no .taskmaster/tasks/tasks.json reference in .claude/skills/audit/SKILL.md" \
    bash -c '! grep -qF ".taskmaster/tasks/tasks.json" "$1/.claude/skills/audit/SKILL.md"' -- "$REPO_ROOT"

assert "no .taskmaster/tasks/tasks.json reference in .claude/skills/audit/references/" \
    bash -c '! grep -rqF ".taskmaster/tasks/tasks.json" "$1/.claude/skills/audit/references/"' -- "$REPO_ROOT"

assert "no .taskmaster/tasks/tasks.json reference in crates/reify-audit/src/bin/reify-audit.rs" \
    bash -c '! grep -qF ".taskmaster/tasks/tasks.json" "$1/crates/reify-audit/src/bin/reify-audit.rs"' -- "$REPO_ROOT"

assert "no .taskmaster/tasks/tasks.json reference in scripts/reify-audit-predone-wrapper.sh" \
    bash -c '! grep -qF ".taskmaster/tasks/tasks.json" "$1/scripts/reify-audit-predone-wrapper.sh"' -- "$REPO_ROOT"

# -- Summary ------------------------------------------------------------------
test_summary
