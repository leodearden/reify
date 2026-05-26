#!/usr/bin/env bash
# Tests for tests/infra/run_all.sh discovery runner.
# Verifies: existence, executability, exclusion of test_helpers.sh,
# discovery of test_*.sh files, exit-code aggregation, and
# orchestrator.yaml wiring.
#
# IMPORTANT: All tests that exercise run_all.sh use temp dirs with mock
# scripts to avoid infinite recursion (this file is itself auto-discovered
# by run_all.sh when it runs on the real tests/infra/ directory).

set -euo pipefail

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

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

# -- Test 2: test_helpers.sh excluded from discovery ----------------------------
# Use a temp dir containing only test_helpers.sh to verify it is excluded.
echo ""
echo "--- Test 2: test_helpers.sh excluded from discovery ---"

if [ -f "$RUN_ALL" ]; then
    TMPDIR_T2="$(mktemp -d)"
    _TMPDIRS+=("$TMPDIR_T2")
    cp "$SCRIPT_DIR/test_helpers.sh" "$TMPDIR_T2/test_helpers.sh"
    t2_output="$(bash "$RUN_ALL" "$TMPDIR_T2" 2>&1)" || true
    rm -rf "$TMPDIR_T2"

    if ! echo "$t2_output" | grep -q "Running: test_helpers\.sh"; then
        assert "test_helpers.sh not listed as a discovered test" true
    else
        assert "test_helpers.sh not listed as a discovered test (got: $t2_output)" false
    fi
else
    assert "test_helpers.sh not listed as a discovered test (skipped - run_all.sh missing)" \
        false
fi

# -- Test 3: test_*.sh files are discovered, test_helpers.sh excluded -----------
# Use a temp dir with mock test_*.sh scripts and test_helpers.sh to verify
# discovery logic. We do NOT invoke run_all.sh on the real SCRIPT_DIR —
# that would cause infinite recursion since this file is auto-discovered.
echo ""
echo "--- Test 3: test_*.sh discovery (mock dir) ---"

if [ -f "$RUN_ALL" ]; then
    TMPDIR_T3="$(mktemp -d)"
    _TMPDIRS+=("$TMPDIR_T3")
    printf '#!/usr/bin/env bash\nexit 0\n' > "$TMPDIR_T3/test_portable_sha256.sh"
    printf '#!/usr/bin/env bash\nexit 0\n' > "$TMPDIR_T3/test_test_helpers.sh"
    printf '#!/usr/bin/env bash\nexit 0\n' > "$TMPDIR_T3/test_helpers.sh"
    chmod +x "$TMPDIR_T3/test_portable_sha256.sh" \
              "$TMPDIR_T3/test_test_helpers.sh" \
              "$TMPDIR_T3/test_helpers.sh"
    t3_output="$(bash "$RUN_ALL" "$TMPDIR_T3" 2>&1)" || true
    rm -rf "$TMPDIR_T3"

    if echo "$t3_output" | grep -q "test_portable_sha256"; then
        assert "test_portable_sha256.sh is discovered" true
    else
        assert "test_portable_sha256.sh is discovered (got: $t3_output)" false
    fi

    if echo "$t3_output" | grep -q "test_test_helpers"; then
        assert "test_test_helpers.sh is discovered" true
    else
        assert "test_test_helpers.sh is discovered (got: $t3_output)" false
    fi

    # Use "Running: test_helpers.sh" not "Running.*test_helpers.sh" —
    # the latter would also match "test_test_helpers.sh" as a suffix.
    if ! echo "$t3_output" | grep -q "Running: test_helpers\.sh"; then
        assert "test_helpers.sh is NOT in discovered output" true
    else
        assert "test_helpers.sh is NOT in discovered output (got: $t3_output)" false
    fi
else
    assert "test_portable_sha256.sh is discovered (skipped - run_all.sh missing)" false
    assert "test_test_helpers.sh is discovered (skipped - run_all.sh missing)" false
    assert "test_helpers.sh is NOT in discovered output (skipped - run_all.sh missing)" false
fi

# -- Test 4: exit-code aggregation using temp dir mock scripts ------------------
echo ""
echo "--- Test 4: exit-code aggregation ---"

if [ -f "$RUN_ALL" ]; then
    # 4a: all-pass scenario — should exit 0
    TMPDIR_PASS="$(mktemp -d)"
    _TMPDIRS+=("$TMPDIR_PASS")
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
    _TMPDIRS+=("$TMPDIR_FAIL")
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
    _TMPDIRS+=("$TMPDIR_EMPTY")
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

# -- Test 5: verify.sh plan wiring ----------------------------------------------
echo ""
echo "--- Test 5: verify.sh test plan (--include-infra) includes run_all.sh ---"

# Since task 3766 the orchestrator runs scripts/verify.sh; run_all.sh is wired
# into the test-side infra of the verify.sh plan, not orchestrator.yaml directly.
assert "verify.sh test plan references tests/infra/run_all.sh" \
    bash -c "bash '$REPO_ROOT/scripts/verify.sh' test --scope all --include-infra --print-plan | grep -v '^#' | grep -q 'tests/infra/run_all\.sh'"

# -- Test 6: structural self-checks (meta-assertions) ---------------------------
echo ""
echo "--- Test 6: structural self-checks ---"

THIS_FILE="${BASH_SOURCE[0]}"

assert "t2_rc dead variable removed" \
    bash -c "! grep -qE 't2_rc=[0\$]' '$THIS_FILE'"

assert "t3_rc dead variable removed" \
    bash -c "! grep -qE 't3_rc=[0\$]' '$THIS_FILE'"

assert "trap cleanup EXIT is registered" \
    bash -c "grep -Eq '^trap cleanup EXIT' '$THIS_FILE'"

assert "_TMPDIRS array is declared" \
    bash -c "grep -Eq '^_TMPDIRS=\(\)' '$THIS_FILE'"

assert "cleanup() function defined" \
    bash -c "grep -Eq '^cleanup\(\) \{' '$THIS_FILE'"

# -- Summary --------------------------------------------------------------------
test_summary
