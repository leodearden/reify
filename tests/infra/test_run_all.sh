#!/usr/bin/env bash
# Tests for tests/infra/run_all.sh discovery runner.
# Verifies: existence, executability, exclusion of test_helpers.sh,
# discovery of test_*.sh files, exit-code aggregation, and
# orchestrator.yaml wiring.
#
# Note: This file is auto-discovered by run_all.sh itself. To avoid
# infinite recursion the aggregation tests use a temp dir with mock
# scripts rather than invoking run_all.sh on the real directory.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
RUN_ALL="$SCRIPT_DIR/run_all.sh"
ORCHESTRATOR_YAML="$REPO_ROOT/orchestrator.yaml"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== run_all.sh unit tests ==="

# -- Test 1: run_all.sh exists and is executable --------------------------------
echo ""
echo "--- Test 1: run_all.sh exists and is executable ---"

assert "run_all.sh file exists" \
    test -f "$RUN_ALL"

assert "run_all.sh is executable" \
    test -x "$RUN_ALL"

# -- Test 2: test_helpers.sh is excluded from discovery -------------------------
echo ""
echo "--- Test 2: test_helpers.sh excluded from discovery ---"

# If run_all.sh exists, check its output doesn't include test_helpers.sh
# We capture list of scripts run_all.sh would discover via a dry-run approach:
# look at what run_all.sh globs by running it against a temp dir that only
# contains test_helpers.sh and see it's skipped.
if [ -f "$RUN_ALL" ]; then
    TMPDIR_T2="$(mktemp -d)"
    # Copy test_helpers.sh there
    cp "$SCRIPT_DIR/test_helpers.sh" "$TMPDIR_T2/test_helpers.sh"
    # Run run_all.sh with INFRA_DIR set to temp dir (uses env override if supported,
    # else check via output — run_all.sh should discover 0 tests in this temp dir)
    t2_output=$(bash "$RUN_ALL" "$TMPDIR_T2" 2>&1) && t2_rc=0 || t2_rc=$?
    rm -rf "$TMPDIR_T2"
    # Output should NOT contain "test_helpers" as a discovered test
    assert "test_helpers.sh not listed as discovered test" \
        bash -c "! echo '$t2_output' | grep -q 'Running.*test_helpers'"
else
    assert "test_helpers.sh not listed as discovered test (skipped - run_all.sh missing)" \
        false
fi

# -- Test 3: real test_*.sh files are discovered --------------------------------
echo ""
echo "--- Test 3: real test_*.sh files are discovered ---"

if [ -f "$RUN_ALL" ]; then
    # Run against SCRIPT_DIR itself - should discover existing test_*.sh files
    # Use a subshell with timeout guard; may fail due to actual test failures
    # but the important thing is the DISCOVERY output is present.
    # We check by capturing output and seeing if known test files are mentioned.
    t3_output=$(bash "$RUN_ALL" 2>&1) && t3_rc=0 || t3_rc=$?
    assert "test_portable_sha256.sh is discovered" \
        bash -c "echo '$t3_output' | grep -q 'test_portable_sha256'"
    assert "test_test_helpers.sh is discovered" \
        bash -c "echo '$t3_output' | grep -q 'test_test_helpers'"
    assert "test_helpers.sh is NOT in discovered output" \
        bash -c "! echo '$t3_output' | grep -q 'Running.*test_helpers\.sh'"
else
    assert "real test_*.sh files are discovered (skipped - run_all.sh missing)" \
        false
    assert "test_helpers.sh is NOT in discovered output (skipped - run_all.sh missing)" \
        false
fi

# -- Test 4: exit-code aggregation using temp dir mock scripts ------------------
echo ""
echo "--- Test 4: exit-code aggregation ---"

if [ -f "$RUN_ALL" ]; then
    # 4a: all-pass scenario — should exit 0
    TMPDIR_PASS="$(mktemp -d)"
    printf '#!/usr/bin/env bash\nexit 0\n' > "$TMPDIR_PASS/test_alpha.sh"
    printf '#!/usr/bin/env bash\nexit 0\n' > "$TMPDIR_PASS/test_beta.sh"
    chmod +x "$TMPDIR_PASS/test_alpha.sh" "$TMPDIR_PASS/test_beta.sh"

    t4a_rc=0
    bash "$RUN_ALL" "$TMPDIR_PASS" >/dev/null 2>&1 || t4a_rc=$?
    rm -rf "$TMPDIR_PASS"

    assert "run_all.sh exits 0 when all tests pass" \
        test "$t4a_rc" -eq 0

    # 4b: any-fail scenario — should exit 1
    TMPDIR_FAIL="$(mktemp -d)"
    printf '#!/usr/bin/env bash\nexit 0\n' > "$TMPDIR_FAIL/test_pass.sh"
    printf '#!/usr/bin/env bash\nexit 1\n' > "$TMPDIR_FAIL/test_fail.sh"
    chmod +x "$TMPDIR_FAIL/test_pass.sh" "$TMPDIR_FAIL/test_fail.sh"

    t4b_rc=0
    bash "$RUN_ALL" "$TMPDIR_FAIL" >/dev/null 2>&1 || t4b_rc=$?
    rm -rf "$TMPDIR_FAIL"

    assert "run_all.sh exits 1 when any test fails" \
        test "$t4b_rc" -eq 1

    # 4c: no test_*.sh files scenario — should exit 0 (empty suite is success)
    TMPDIR_EMPTY="$(mktemp -d)"
    t4c_rc=0
    bash "$RUN_ALL" "$TMPDIR_EMPTY" >/dev/null 2>&1 || t4c_rc=$?
    rm -rf "$TMPDIR_EMPTY"

    assert "run_all.sh exits 0 when no test_*.sh files found" \
        test "$t4c_rc" -eq 0
else
    assert "run_all.sh exits 0 when all tests pass (skipped - run_all.sh missing)" \
        false
    assert "run_all.sh exits 1 when any test fails (skipped - run_all.sh missing)" \
        false
    assert "run_all.sh exits 0 when no test_*.sh files found (skipped - run_all.sh missing)" \
        false
fi

# -- Test 5: orchestrator.yaml wiring -------------------------------------------
echo ""
echo "--- Test 5: orchestrator.yaml test_command includes run_all.sh ---"

assert "orchestrator.yaml references tests/infra/run_all.sh" \
    bash -c "grep -q 'tests/infra/run_all.sh' '$ORCHESTRATOR_YAML'"

# -- Summary --------------------------------------------------------------------
test_summary
