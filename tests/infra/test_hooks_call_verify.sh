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
# The orchestrator merge path runs test_command verbatim with DF_VERIFY_ROLE=merge
# injected. verify.sh's role-based default (merge=>both) must govern the profile;
# an explicit --profile in test_command would override it and pin the merge path
# to that profile, defeating the task-4078 fix.
assert "test_command relies on the role-based profile default (no explicit --profile so merge=>both)" \
    bash -c "grep '^test_command:' '$ORCH' | grep -vq -- '--profile'"
assert "lint_command calls ./scripts/verify.sh lint" \
    bash -c "grep '^lint_command:' '$ORCH' | grep -qF './scripts/verify.sh lint'"
# Single positive assertion: requires the key to exist AND hold the value 'true' — cannot
# pass vacuously if the key is deleted (unlike a pure negation). Implicitly covers the
# "no longer invokes verify.sh typecheck" property: if the value is 'true', it is not
# the old './scripts/verify.sh typecheck --scope all' string.
assert "type_check_command is the passing no-op 'true' (redundant cargo check dropped; clippy supersets it)" \
    bash -c "grep -qE '^type_check_command:[[:space:]]+\"?true\"?[[:space:]]*(#.*)?$' '$ORCH'"
# verify_env must remain (verify.sh re-bakes it, but the orchestrator still injects it).
assert "orchestrator.yaml still defines verify_env" \
    bash -c "grep -q '^verify_env:' '$ORCH'"

# -- Fix 2 (main-gate-hardening): exec -> '|| exit $?' + post-success mark -------
# The pre-commit / pre-merge-commit gates used to `exec` their verifier, which
# replaces the shell and can never run code afterwards. Fix 2 changes them to a
# foreground call followed by `|| exit $?`, then marks the main-gate sentinel —
# so hooks/reference-transaction can tell a verified (sanctioned) refs/heads/main
# move from an unsanctioned one. The verifier args must be UNCHANGED (the
# existing assertions above still pin them); these assertions pin the restructure.
echo ""
echo "--- pre-commit / pre-merge-commit: exec->'|| exit \$?' restructure + sentinel mark ---"

PRE_COMMIT="$REPO_ROOT/hooks/pre-commit"
GATE_LIB="$REPO_ROOT/hooks/main-gate-lib.sh"
REF_TXN="$REPO_ROOT/hooks/reference-transaction"

# The new tripwire + shared lib must exist (lib is sourced, not executed).
assert "hooks/main-gate-lib.sh exists" test -f "$GATE_LIB"
assert "hooks/reference-transaction exists" test -f "$REF_TXN"
assert "hooks/reference-transaction is executable" test -x "$REF_TXN"

# pre-commit must still exist + be executable.
assert "hooks/pre-commit exists" test -f "$PRE_COMMIT"
assert "hooks/pre-commit is executable" test -x "$PRE_COMMIT"

# pre-commit calls project-checks in the FOREGROUND with '|| exit $?' (not exec),
# so the post-success mark is reachable.
assert "pre-commit calls project-checks then '|| exit \$?'" \
    bash -c "grep -F 'hooks/project-checks' '$PRE_COMMIT' | grep -qF '|| exit \$?'"
assert "pre-commit has no top-level 'exec' command (post-mark must be reachable)" \
    bash -c "! grep -qE '^[[:space:]]*exec[[:space:]]' '$PRE_COMMIT'"
assert "pre-commit sources hooks/main-gate-lib.sh" \
    bash -c "grep -q 'main-gate-lib.sh' '$PRE_COMMIT'"
assert "pre-commit marks the sentinel on success (main_gate_mark)" \
    bash -c "grep -q 'main_gate_mark' '$PRE_COMMIT'"

# pre-merge-commit: same restructure — run verify, '|| exit $?', then mark.
assert "pre-merge-commit runs verify.sh then '|| exit \$?'" \
    bash -c "grep -F 'scripts/verify.sh' '$PRE_MERGE' | grep -qF '|| exit \$?'"
assert "pre-merge-commit has no top-level 'exec' command (post-mark must be reachable)" \
    bash -c "! grep -qE '^[[:space:]]*exec[[:space:]]' '$PRE_MERGE'"
assert "pre-merge-commit sources hooks/main-gate-lib.sh" \
    bash -c "grep -q 'main-gate-lib.sh' '$PRE_MERGE'"
assert "pre-merge-commit marks the sentinel on success (main_gate_mark)" \
    bash -c "grep -q 'main_gate_mark' '$PRE_MERGE'"

test_summary
