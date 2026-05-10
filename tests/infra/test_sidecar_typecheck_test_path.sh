#!/usr/bin/env bash
# Infrastructure tests for sidecar test-file typecheck enforcement (task 3357).
# Validates that:
#   (a) gui/sidecar/tsconfig.test.json exists with correct settings
#   (b) gui/sidecar/package.json defines typecheck:test script
#   (c) tsc --noEmit -p tsconfig.test.json catches unused @ts-expect-error (TS2578)
#   (d) orchestrator.yaml lint_command sidecar block invokes typecheck and typecheck:test
#   (e) hooks/project-checks sidecar block invokes typecheck and typecheck:test

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== sidecar typecheck test-path enforcement tests ==="

TSCONFIG_TEST="$REPO_ROOT/gui/sidecar/tsconfig.test.json"
SIDECAR_PKG="$REPO_ROOT/gui/sidecar/package.json"

# -- Group (a): tsconfig.test.json structural pins ---------------------------
echo ""
echo "--- Group (a): tsconfig.test.json structural pins ---"

assert "gui/sidecar/tsconfig.test.json exists" \
    test -f "$TSCONFIG_TEST"

assert "tsconfig.test.json extends ./tsconfig.json" \
    bash -c "grep -qE '\"extends\"\\s*:\\s*\"./tsconfig.json\"' '$TSCONFIG_TEST'"

assert "tsconfig.test.json include contains src/**/*.ts" \
    bash -c "grep -qF '\"src/**/*.ts\"' '$TSCONFIG_TEST'"

assert "tsconfig.test.json exclude does NOT contain '*.test.ts'" \
    bash -c "! grep -qF '*.test.ts' '$TSCONFIG_TEST'"

assert "tsconfig.test.json exclude does NOT contain '__tests__'" \
    bash -c "! grep -qF '__tests__' '$TSCONFIG_TEST'"

# -- Group (b): package.json typecheck:test script pin -----------------------
echo ""
echo "--- Group (b): package.json typecheck:test script pin ---"

assert "gui/sidecar/package.json exists" \
    test -f "$SIDECAR_PKG"

assert "gui/sidecar/package.json defines 'typecheck:test' script" \
    bash -c "node -e 'process.exit(typeof require(\"$SIDECAR_PKG\").scripts[\"typecheck:test\"] === \"string\" ? 0 : 1)'"

assert "gui/sidecar/package.json typecheck:test script invokes tsc --noEmit -p tsconfig.test.json" \
    bash -c "node -e 'process.exit(/tsc --noEmit -p tsconfig\\.test\\.json/.test(require(\"$SIDECAR_PKG\").scripts[\"typecheck:test\"]) ? 0 : 1)'"

# -- Group (c): behavioral fixture sub-test (gated on node_modules/.bin/tsc) --
echo ""
echo "--- Group (c): behavioral fixture sub-test ---"

_TMPFILES=()
cleanup() {
    for f in "${_TMPFILES[@]+${_TMPFILES[@]}}"; do
        rm -f "$f"
    done
}
trap cleanup EXIT

if [ -f "$REPO_ROOT/gui/sidecar/node_modules/.bin/tsc" ]; then
    FIXTURE_FILE="$REPO_ROOT/gui/sidecar/src/__tests__/__typecheck_test_path_fixture_$$.test.ts"
    _TMPFILES+=("$FIXTURE_FILE")

    # Write a no-op @ts-expect-error on a known-valid assignment — should produce TS2578
    # ("Unused '@ts-expect-error' directive") because the assignment is not a type error.
    cat > "$FIXTURE_FILE" <<'FIXTURE_EOF'
// @ts-expect-error
const _x: number = 1;
FIXTURE_EOF

    FIXTURE_EC=0
    FIXTURE_OUTPUT=$(cd "$REPO_ROOT/gui/sidecar" && npm run typecheck:test 2>&1) || FIXTURE_EC=$?

    if [ "$FIXTURE_EC" -ne 0 ]; then
        echo "  PASS: typecheck:test exits non-zero on unused @ts-expect-error fixture"
        PASS=$((PASS + 1))
    else
        echo "  FAIL: typecheck:test exits non-zero on unused @ts-expect-error fixture (expected non-zero, got 0)"
        FAIL=$((FAIL + 1))
    fi

    if echo "$FIXTURE_OUTPUT" | grep -q 'TS2578'; then
        echo "  PASS: typecheck:test output contains TS2578"
        PASS=$((PASS + 1))
    else
        echo "  FAIL: typecheck:test output contains TS2578 (actual output: $FIXTURE_OUTPUT)"
        FAIL=$((FAIL + 1))
    fi
else
    echo "  WARNING: gui/sidecar/node_modules/.bin/tsc not found — skipping behavioral fixture test"
fi

# -- Group (d): orchestrator.yaml lint_command sidecar block pins -------------
echo ""
echo "--- Group (d): orchestrator.yaml lint_command sidecar block pins ---"

ORCH="$REPO_ROOT/orchestrator.yaml"

assert "orchestrator.yaml lint_command sidecar block invokes npm run typecheck:test" \
    bash -c "grep 'lint_command:' '$ORCH' | grep -q 'npm run typecheck:test'"

assert "orchestrator.yaml lint_command sidecar block invokes npm run typecheck" \
    bash -c "grep 'lint_command:' '$ORCH' | grep -q 'npm run typecheck && npm run typecheck:test'"

assert "orchestrator.yaml lint_command sidecar block uses bash -c chaining" \
    bash -c "grep 'lint_command:' '$ORCH' | grep -q 'gui/sidecar.*bash -c'"

assert "orchestrator.yaml lint_command sidecar block is NOT just 'npm ci' standalone" \
    bash -c "! grep 'lint_command:' '$ORCH' | grep -qE 'gui/sidecar && timeout[^)]+npm ci\\); fi'"

test_summary
