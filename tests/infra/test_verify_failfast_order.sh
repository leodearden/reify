#!/usr/bin/env bash
# Infrastructure test for task 4448.
# Validates fail-fast ordering in verify.sh build_plan():
#
#   (1) Incident/#4446 deliverable: `DF_VERIFY_ROLE=merge verify.sh test --scope all`
#       has `npm run typecheck` BEFORE `./scripts/verify.sh psi-gate` (the expensive pole).
#   (2) `verify.sh all --scope all --include-infra` has npm run typecheck AND infra
#       checks BEFORE the psi-gate.
#   (3) Preservation: plan still contains all expected components (content unchanged).
#
# The bounded node||cargo overlap assertions (step-3) will be appended to this file
# once the fail-fast reorder lands (step-2).
#
# Oracle: verify.sh --print-plan (hermetic, never runs cargo/npm).
# Index technique mirrors test_release_mode_in_test_command.sh:60-63.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== verify.sh fail-fast ordering tests (task 4448) ==="

# Capture plan outputs. Strip comment lines (^#).
# DF_VERIFY_ROLE=merge test --scope all defaults to --profile both (both debug + release passes).
MERGE_TEST_PLAN="$(DF_VERIFY_ROLE=merge bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan | grep -v '^#')"
ALL_PLAN="$(bash "$REPO_ROOT/scripts/verify.sh" all --scope all --include-infra --print-plan | grep -v '^#')"
PLAIN_TEST_PLAN="$(bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan | grep -v '^#')"
export MERGE_TEST_PLAN ALL_PLAN PLAIN_TEST_PLAN

# ===========================================================================
# Test 1: #4446 deliverable — merge-role test plan: npm typecheck BEFORE psi-gate
# ===========================================================================
echo ""
echo "--- Test 1: merge-role test/scope=all: npm run typecheck ordered BEFORE psi-gate ---"

assert "merge test plan: npm run typecheck index < psi-gate index" \
    bash -c '
        NPM_IDX=$(printf "%s\n" "$1" | grep -n "npm run typecheck" | head -1 | cut -d: -f1)
        PSI_IDX=$(printf "%s\n" "$1" | grep -n "\./scripts/verify\.sh psi-gate" | head -1 | cut -d: -f1)
        [ -n "$NPM_IDX" ] && [ -n "$PSI_IDX" ] && [ "$NPM_IDX" -lt "$PSI_IDX" ]
    ' _ "$MERGE_TEST_PLAN"

# plain task-role test plan: same ordering guarantee
assert "plain test plan (task role): npm run typecheck index < psi-gate index" \
    bash -c '
        NPM_IDX=$(printf "%s\n" "$1" | grep -n "npm run typecheck" | head -1 | cut -d: -f1)
        PSI_IDX=$(printf "%s\n" "$1" | grep -n "\./scripts/verify\.sh psi-gate" | head -1 | cut -d: -f1)
        [ -n "$NPM_IDX" ] && [ -n "$PSI_IDX" ] && [ "$NPM_IDX" -lt "$PSI_IDX" ]
    ' _ "$PLAIN_TEST_PLAN"

# ===========================================================================
# Test 2: all --scope all --include-infra: npm AND infra BEFORE psi-gate
# ===========================================================================
echo ""
echo "--- Test 2: all/scope=all/include-infra: npm + infra ordered BEFORE psi-gate ---"

assert "all plan: npm run typecheck index < psi-gate index" \
    bash -c '
        NPM_IDX=$(printf "%s\n" "$1" | grep -n "npm run typecheck" | head -1 | cut -d: -f1)
        PSI_IDX=$(printf "%s\n" "$1" | grep -n "\./scripts/verify\.sh psi-gate" | head -1 | cut -d: -f1)
        [ -n "$NPM_IDX" ] && [ -n "$PSI_IDX" ] && [ "$NPM_IDX" -lt "$PSI_IDX" ]
    ' _ "$ALL_PLAN"

assert "all plan: check_event_inventory.sh index < psi-gate index" \
    bash -c '
        INFRA_IDX=$(printf "%s\n" "$1" | grep -n "check_event_inventory\.sh" | head -1 | cut -d: -f1)
        PSI_IDX=$(printf "%s\n" "$1" | grep -n "\./scripts/verify\.sh psi-gate" | head -1 | cut -d: -f1)
        [ -n "$INFRA_IDX" ] && [ -n "$PSI_IDX" ] && [ "$INFRA_IDX" -lt "$PSI_IDX" ]
    ' _ "$ALL_PLAN"

# ===========================================================================
# Test 3: Preservation — plan still contains all expected components
# ===========================================================================
echo ""
echo "--- Test 3: preservation — all expected components still present ---"

assert "merge test plan: contains check-manifold-deps.sh" \
    bash -c 'printf "%s\n" "$1" | grep -q "check-manifold-deps\.sh"' _ "$MERGE_TEST_PLAN"

assert "merge test plan: contains tree-sitter-generate.sh" \
    bash -c 'printf "%s\n" "$1" | grep -q "tree-sitter-generate\.sh"' _ "$MERGE_TEST_PLAN"

assert "merge test plan: contains npm ci && npm run typecheck && npm test (gui chain intact)" \
    bash -c 'printf "%s\n" "$1" | grep -q "npm ci && npm run typecheck && npm test"' _ "$MERGE_TEST_PLAN"

assert "merge test plan: contains cargo-test-occt-gated.sh (gated pass preserved)" \
    bash -c 'printf "%s\n" "$1" | grep -q "cargo-test-occt-gated\.sh"' _ "$MERGE_TEST_PLAN"

assert "all plan: contains cargo clippy (rust lint gate for action=all)" \
    bash -c 'printf "%s\n" "$1" | grep -q "cargo clippy"' _ "$ALL_PLAN"

assert "all plan: contains check-manifold-deps.sh" \
    bash -c 'printf "%s\n" "$1" | grep -q "check-manifold-deps\.sh"' _ "$ALL_PLAN"

assert "all plan: contains tree-sitter-generate.sh" \
    bash -c 'printf "%s\n" "$1" | grep -q "tree-sitter-generate\.sh"' _ "$ALL_PLAN"

assert "all plan: contains npm ci && npm run typecheck && npm test (gui chain intact)" \
    bash -c 'printf "%s\n" "$1" | grep -q "npm ci && npm run typecheck && npm test"' _ "$ALL_PLAN"

assert "all plan: contains cargo-test-occt-gated.sh (gated pass preserved)" \
    bash -c 'printf "%s\n" "$1" | grep -q "cargo-test-occt-gated\.sh"' _ "$ALL_PLAN"

test_summary
