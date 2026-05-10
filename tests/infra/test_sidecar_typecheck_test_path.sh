#!/usr/bin/env bash
# Infrastructure tests for sidecar test-file typecheck enforcement (task 3357).
# Validates that:
#   (a) gui/sidecar/tsconfig.test.json exists
#   (b) gui/sidecar/package.json defines typecheck:test script
#   (c) tsc --noEmit catches unused @ts-expect-error (TS2578); fixture lives in a
#       temp dir (not live src/) and uses a fixture-only tsconfig for isolation

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== sidecar typecheck test-path enforcement tests ==="

TSCONFIG_TEST="$REPO_ROOT/gui/sidecar/tsconfig.test.json"
SIDECAR_PKG="$REPO_ROOT/gui/sidecar/package.json"

assert "gui/sidecar/tsconfig.test.json exists" \
    test -f "$TSCONFIG_TEST"

assert "gui/sidecar/package.json defines 'typecheck:test' script" \
    bash -c "node -e 'process.exit(typeof require(\"$SIDECAR_PKG\").scripts[\"typecheck:test\"] === \"string\" ? 0 : 1)'"

# -- Behavioral fixture: typecheck:test catches unused @ts-expect-error -------
echo ""
echo "--- Behavioral fixture: typecheck:test catches unused @ts-expect-error ---"

_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do
        rm -rf "$d"
    done
}
trap cleanup EXIT

if [ -f "$REPO_ROOT/gui/sidecar/node_modules/.bin/tsc" ]; then
    # Write fixture to a temp dir (not live src/__tests__/) so it doesn't persist
    # on SIGKILL.  A fixture-only tsconfig isolates the tsc signal to this file.
    TMPDIR_FIXTURE=$(mktemp -d)
    _TMPDIRS+=("$TMPDIR_FIXTURE")

    FIXTURE_FILE="$TMPDIR_FIXTURE/typecheck_fixture.test.ts"
    FIXTURE_CFG="$TMPDIR_FIXTURE/tsconfig.fixture.json"

    # Write a no-op @ts-expect-error on a known-valid assignment — should produce TS2578
    # ("Unused '@ts-expect-error' directive") because the assignment is not a type error.
    # export {} makes the file a module so isolatedModules: true is satisfied.
    cat > "$FIXTURE_FILE" <<'FIXTURE_EOF'
// @ts-expect-error
const _x: number = 1;
export {};
FIXTURE_EOF

    # Temp tsconfig: extend sidecar's tsconfig.test.json, include only the fixture,
    # override rootDir so the fixture path satisfies the rootDir constraint.
    cat > "$FIXTURE_CFG" << TSCONFIG_EOF
{
  "extends": "$TSCONFIG_TEST",
  "compilerOptions": { "rootDir": "$TMPDIR_FIXTURE" },
  "include": ["$FIXTURE_FILE"]
}
TSCONFIG_EOF

    FIXTURE_OUTPUT=$("$REPO_ROOT/gui/sidecar/node_modules/.bin/tsc" --noEmit -p "$FIXTURE_CFG" 2>&1) || true

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

test_summary
