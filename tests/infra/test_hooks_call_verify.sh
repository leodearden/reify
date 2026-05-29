#!/usr/bin/env bash
# Infrastructure test for task 3766.
# Verifies that the git hooks and orchestrator.yaml delegate to scripts/verify.sh
# with the expected arguments — the structural wiring of the unification. This is
# the counterpart to the --print-plan content tests: it pins HOW verify.sh is
# invoked, while the others pin WHAT the plan contains.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

PROJECT_CHECKS="$REPO_ROOT/hooks/project-checks"
PRE_MERGE="$REPO_ROOT/hooks/pre-merge-commit"
VERIFY="$REPO_ROOT/scripts/verify.sh"
ORCH="$REPO_ROOT/orchestrator.yaml"

echo "=== hooks + orchestrator call verify.sh ==="

# -- verify.sh itself ----------------------------------------------------------
echo ""
echo "--- verify.sh exists and is executable ---"
assert "scripts/verify.sh exists" test -f "$VERIFY"
assert "scripts/verify.sh is executable" test -x "$VERIFY"

# -- hooks/project-checks ------------------------------------------------------
echo ""
echo "--- hooks/project-checks delegates to verify.sh (staged scope) ---"
assert "hooks/project-checks exists" test -f "$PROJECT_CHECKS"
assert "hooks/project-checks is executable" test -x "$PROJECT_CHECKS"
assert "project-checks execs verify.sh all --profile debug --scope staged --include-infra" \
    bash -c "grep -qE 'scripts/verify\.sh\" all --profile debug --scope staged --include-infra' '$PROJECT_CHECKS'"
# The fat per-step logic must be gone (delegation, not duplication).
assert "project-checks no longer runs clippy directly" \
    bash -c "! grep -q 'cargo clippy' '$PROJECT_CHECKS'"
assert "project-checks no longer runs 'npx vitest' directly" \
    bash -c "! grep -q 'npx vitest' '$PROJECT_CHECKS'"

# -- hooks/pre-merge-commit ----------------------------------------------------
echo ""
echo "--- hooks/pre-merge-commit delegates to verify.sh (full scope, main only) ---"
assert "hooks/pre-merge-commit exists" test -f "$PRE_MERGE"
assert "hooks/pre-merge-commit is executable" test -x "$PRE_MERGE"
assert "pre-merge-commit execs verify.sh all --profile both --scope all" \
    bash -c "grep -qE 'scripts/verify\.sh\" all --profile both --scope all' '$PRE_MERGE'"
assert "pre-merge-commit gates main only (branch != main -> exit 0)" \
    bash -c "grep -q 'branch' '$PRE_MERGE' && grep -q '!= \"main\"' '$PRE_MERGE'"

# -- orchestrator.yaml ---------------------------------------------------------
echo ""
echo "--- orchestrator.yaml command keys delegate to verify.sh ---"
assert "test_command calls ./scripts/verify.sh test" \
    bash -c "grep '^test_command:' '$ORCH' | grep -qF './scripts/verify.sh test'"
assert "lint_command calls ./scripts/verify.sh lint" \
    bash -c "grep '^lint_command:' '$ORCH' | grep -qF './scripts/verify.sh lint'"
assert "type_check_command no longer invokes verify.sh typecheck (redundant cargo check dropped; clippy supersets it)" \
    bash -c "! grep '^type_check_command:' '$ORCH' | grep -qF './scripts/verify.sh typecheck'"
assert "type_check_command is the passing no-op 'true'" \
    bash -c "grep -qE '^type_check_command:[[:space:]]+\"?true\"?[[:space:]]*(#.*)?$' '$ORCH'"
# verify_env must remain (verify.sh re-bakes it, but the orchestrator still injects it).
assert "orchestrator.yaml still defines verify_env" \
    bash -c "grep -q '^verify_env:' '$ORCH'"

test_summary
