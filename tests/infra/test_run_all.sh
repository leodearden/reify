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

    # Use bash-native substring matching (`[[ == *substr* ]]`) rather than
    # `echo "$t3_output" | grep -q`: the pipe-to-grep form forks a subshell and
    # a grep reading from a pipe, and under heavy concurrent test load that grep
    # can transiently fail (broken pipe / EINTR) and return non-zero EVEN WHEN
    # the content matches — silently flipping the check to its else branch and
    # producing a spurious FAIL (esc-4574-42 / esc-4707-64: the got: output
    # plainly contained the expected string yet grep "missed" it). Native
    # matching does no fork and no pipe, so the assertion is purely a function
    # of $t3_output.
    if [[ "$t3_output" == *"test_portable_sha256"* ]]; then
        assert "test_portable_sha256.sh is discovered" true
    else
        assert "test_portable_sha256.sh is discovered (got: $t3_output)" false
    fi

    if [[ "$t3_output" == *"test_test_helpers"* ]]; then
        assert "test_test_helpers.sh is discovered" true
    else
        assert "test_test_helpers.sh is discovered (got: $t3_output)" false
    fi

    # Use "Running: test_helpers.sh" not "Running.*test_helpers.sh" —
    # the latter would also match "test_test_helpers.sh" as a suffix.
    if [[ "$t3_output" != *"Running: test_helpers.sh"* ]]; then
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

# -- Test 7: failed-test diagnostic naming in summary --------------------------
echo ""
echo "--- Test 7: failed-test diagnostic naming in summary ---"

if [ -f "$RUN_ALL" ]; then
    # 7a: mixed pass+fail — FAILED line names the failing test, not the passing one
    TMPDIR_T7A="$(mktemp -d)"
    _TMPDIRS+=("$TMPDIR_T7A")
    printf '#!/usr/bin/env bash\nexit 0\n' > "$TMPDIR_T7A/test_pass.sh"
    printf '#!/usr/bin/env bash\nexit 1\n' > "$TMPDIR_T7A/test_boom.sh"
    chmod +x "$TMPDIR_T7A/test_pass.sh" "$TMPDIR_T7A/test_boom.sh"
    t7a_out="$(bash "$RUN_ALL" "$TMPDIR_T7A" 2>&1)" || true
    rm -rf "$TMPDIR_T7A"

    if echo "$t7a_out" | grep -qE '^=== FAILED:'; then
        assert "=== FAILED: line is emitted on partial failure" true
    else
        assert "=== FAILED: line is emitted on partial failure (got: $t7a_out)" false
    fi

    if echo "$t7a_out" | grep -E '^=== FAILED:' | grep -q 'test_boom'; then
        assert "=== FAILED: line names the failing test (test_boom.sh)" true
    else
        assert "=== FAILED: line names the failing test test_boom.sh (got: $t7a_out)" false
    fi

    if ! echo "$t7a_out" | grep -E '^=== FAILED:' | grep -q 'test_pass'; then
        assert "=== FAILED: line does NOT name the passing test (test_pass.sh)" true
    else
        assert "=== FAILED: line must NOT name the passing test test_pass.sh (got: $t7a_out)" false
    fi

    # 7b: all-pass — no === FAILED: line emitted
    TMPDIR_T7B="$(mktemp -d)"
    _TMPDIRS+=("$TMPDIR_T7B")
    printf '#!/usr/bin/env bash\nexit 0\n' > "$TMPDIR_T7B/test_alpha.sh"
    printf '#!/usr/bin/env bash\nexit 0\n' > "$TMPDIR_T7B/test_beta.sh"
    chmod +x "$TMPDIR_T7B/test_alpha.sh" "$TMPDIR_T7B/test_beta.sh"
    t7b_out="$(bash "$RUN_ALL" "$TMPDIR_T7B" 2>&1)" || true
    rm -rf "$TMPDIR_T7B"

    if ! echo "$t7b_out" | grep -qE '^=== FAILED:'; then
        assert "no === FAILED: line emitted on all-pass" true
    else
        assert "no === FAILED: line emitted on all-pass (got: $t7b_out)" false
    fi

    # 7c: regression guard — Summary line still present after the FAILED addition
    TMPDIR_T7C="$(mktemp -d)"
    _TMPDIRS+=("$TMPDIR_T7C")
    printf '#!/usr/bin/env bash\nexit 1\n' > "$TMPDIR_T7C/test_fail2.sh"
    chmod +x "$TMPDIR_T7C/test_fail2.sh"
    t7c_out="$(bash "$RUN_ALL" "$TMPDIR_T7C" 2>&1)" || true
    rm -rf "$TMPDIR_T7C"

    if echo "$t7c_out" | grep -qE '^=== Summary:'; then
        assert "=== Summary: line still present in run_all.sh output" true
    else
        assert "=== Summary: line still present in run_all.sh output (got: $t7c_out)" false
    fi
else
    assert "=== FAILED: line is emitted on partial failure (skipped - run_all.sh missing)" false
    assert "=== FAILED: line names the failing test test_boom.sh (skipped - run_all.sh missing)" false
    assert "=== FAILED: line does NOT name the passing test test_pass.sh (skipped - run_all.sh missing)" false
    assert "no === FAILED: line emitted on all-pass (skipped - run_all.sh missing)" false
    assert "=== Summary: line still present in run_all.sh output (skipped - run_all.sh missing)" false
fi

# -- Test 8: failure-path classifier marker (^FAILED <space-joined names>) ------
echo ""
echo "--- Test 8: failure-path classifier marker ---"

if [ -f "$RUN_ALL" ]; then
    # 8a: forced single failure — classifier marker ^FAILED must be emitted.
    # Matches dark-factory verify.py pattern #7b `^FAILED\s` -> test_failure,
    # ranked before pattern #10 `tree-sitter generate` -> tree_sitter_generate_error.
    TMPDIR_T8A="$(mktemp -d)"
    _TMPDIRS+=("$TMPDIR_T8A")
    printf '#!/usr/bin/env bash\nexit 1\n' > "$TMPDIR_T8A/test_boom.sh"
    chmod +x "$TMPDIR_T8A/test_boom.sh"
    t8a_rc=0
    t8a_out="$(bash "$RUN_ALL" "$TMPDIR_T8A" 2>&1)" || t8a_rc=$?
    rm -rf "$TMPDIR_T8A"

    if echo "$t8a_out" | grep -qE '^FAILED '; then
        assert "^FAILED classifier marker line is emitted on failure" true
    else
        assert "^FAILED classifier marker line is emitted on failure (got: $t8a_out)" false
    fi

    if echo "$t8a_out" | grep -E '^FAILED ' | grep -q 'test_boom'; then
        assert "^FAILED line names the failing suite (test_boom.sh)" true
    else
        assert "^FAILED line names the failing suite test_boom.sh (got: $t8a_out)" false
    fi

    assert "run_all.sh still exits 1 with classifier marker" \
        test "$t8a_rc" -eq 1

    if echo "$t8a_out" | grep -qE '^=== FAILED:'; then
        assert "=== FAILED: human-readable line still present alongside classifier marker" true
    else
        assert "=== FAILED: human-readable line still present alongside classifier marker (got: $t8a_out)" false
    fi

    # 8b: all-pass — NO ^FAILED line emitted (classifier marker is failure-path only)
    TMPDIR_T8B="$(mktemp -d)"
    _TMPDIRS+=("$TMPDIR_T8B")
    printf '#!/usr/bin/env bash\nexit 0\n' > "$TMPDIR_T8B/test_alpha.sh"
    printf '#!/usr/bin/env bash\nexit 0\n' > "$TMPDIR_T8B/test_beta.sh"
    chmod +x "$TMPDIR_T8B/test_alpha.sh" "$TMPDIR_T8B/test_beta.sh"
    t8b_out="$(bash "$RUN_ALL" "$TMPDIR_T8B" 2>&1)" || true
    rm -rf "$TMPDIR_T8B"

    if ! echo "$t8b_out" | grep -qE '^FAILED '; then
        assert "no ^FAILED line emitted on all-pass (failure-path only)" true
    else
        assert "no ^FAILED line emitted on all-pass (failure-path only) (got: $t8b_out)" false
    fi
else
    assert "^FAILED classifier marker line is emitted on failure (skipped - run_all.sh missing)" false
    assert "^FAILED line names the failing suite test_boom.sh (skipped - run_all.sh missing)" false
    assert "run_all.sh still exits 1 with classifier marker (skipped - run_all.sh missing)" false
    assert "=== FAILED: human-readable line still present alongside classifier marker (skipped - run_all.sh missing)" false
    assert "no ^FAILED line emitted on all-pass (failure-path only) (skipped - run_all.sh missing)" false
fi

# -- Test 9: H2 concurrent pool -- concurrency, partition, contract, order ------
# Proves tests/infra/run_all.sh parallelizes the `pool`-classified bucket under
# a host-global semaphore while keeping `intra-run-serial`/`host-exclusive`
# tests serial, and that the exact output contract (Summary/FAILED/discovered
# order) survives the concurrent buffering + replay.
echo ""
echo "--- Test 9: H2 concurrent pool (concurrency/partition/contract/order) ---"

LOAD_TOLERANCE_LIB_T9="$SCRIPT_DIR/load_tolerance_lib.sh"
if [ -f "$RUN_ALL" ] && [ -f "$LOAD_TOLERANCE_LIB_T9" ]; then
    TMPDIR_T9="$(mktemp -d)"
    _TMPDIRS+=("$TMPDIR_T9")

    # Fixture manifest: 3 `pool` (one fails), 2 `intra-run-serial`, 1 `host-exclusive`.
    MANIFEST_T9="$TMPDIR_T9/classification.manifest"
    cat > "$MANIFEST_T9" <<'EOF'
test_pool_1.sh pool
test_pool_2.sh pool
test_pool_3.sh pool
test_serial_1.sh intra-run-serial
test_serial_2.sh intra-run-serial
test_hostx_1.sh host-exclusive
EOF

    CNT_T9="$TMPDIR_T9/counters"
    mkdir -p "$CNT_T9"

    # _h2t9_write_pool_mock <path> <exit_code>
    # Mock that proves concurrent overlap: flock-guarded increment of a
    # shared "current"/"max" counter pair, then a load-tolerant BARRIER
    # (poll until >= 2 siblings have arrived, bounded by
    # load_tolerant_attempts so the wait auto-extends under host load
    # instead of guessing a fixed sleep) before decrementing. All reads use
    # `-ge`/equality only -- no `-le`/`-lt` wall-clock upper bound.
    _h2t9_write_pool_mock() {
        local _path="$1" _exit_code="$2"
        cat > "$_path" <<'MOCKBODY'
#!/usr/bin/env bash
set -euo pipefail
source "$H2_T9_LOAD_LIB"
LOCK="$H2_T9_POOL_LOCK"; CUR="$H2_T9_POOL_CUR"; MAX="$H2_T9_POOL_MAX"; ARRIVED="$H2_T9_POOL_ARRIVED"
(
    flock -x 201
    c=$(( $(cat "$CUR" 2>/dev/null || echo 0) + 1 ))
    echo "$c" > "$CUR"
    m=$(cat "$MAX" 2>/dev/null || echo 0)
    if [ "$c" -gt "$m" ]; then echo "$c" > "$MAX"; fi
    a=$(( $(cat "$ARRIVED" 2>/dev/null || echo 0) + 1 ))
    echo "$a" > "$ARRIVED"
) 201>>"$LOCK"

attempts=$(load_tolerant_attempts "${H2_T9_POLL_BASE:-3}")
i=0
while [ "$i" -lt "$attempts" ]; do
    a=$( ( flock -x 201; cat "$ARRIVED" 2>/dev/null || echo 0 ) 201>>"$LOCK" )
    if [ "$a" -ge "${H2_T9_ARRIVE_THRESHOLD:-2}" ]; then break; fi
    sleep 0.1
    i=$((i + 1))
done

(
    flock -x 201
    c=$(( $(cat "$CUR" 2>/dev/null || echo 0) - 1 ))
    echo "$c" > "$CUR"
) 201>>"$LOCK"
MOCKBODY
        echo "exit $_exit_code" >> "$_path"
        chmod +x "$_path"
    }

    # _h2t9_write_serial_mock <path> <exit_code>
    # No barrier -- just a short flock-guarded pause while holding the
    # counter incremented, so an accidental overlap (regression) is still
    # observable via the max counter.
    _h2t9_write_serial_mock() {
        local _path="$1" _exit_code="$2"
        cat > "$_path" <<'MOCKBODY'
#!/usr/bin/env bash
set -euo pipefail
source "$H2_T9_LOAD_LIB"
LOCK="$H2_T9_SERIAL_LOCK"; CUR="$H2_T9_SERIAL_CUR"; MAX="$H2_T9_SERIAL_MAX"
(
    flock -x 201
    c=$(( $(cat "$CUR" 2>/dev/null || echo 0) + 1 ))
    echo "$c" > "$CUR"
    m=$(cat "$MAX" 2>/dev/null || echo 0)
    if [ "$c" -gt "$m" ]; then echo "$c" > "$MAX"; fi
) 201>>"$LOCK"

pause_attempts=$(load_tolerant_attempts "${H2_T9_SERIAL_PAUSE_BASE:-2}")
j=0
while [ "$j" -lt "$pause_attempts" ]; do
    sleep 0.05
    j=$((j + 1))
done

(
    flock -x 201
    c=$(( $(cat "$CUR" 2>/dev/null || echo 0) - 1 ))
    echo "$c" > "$CUR"
) 201>>"$LOCK"
MOCKBODY
        echo "exit $_exit_code" >> "$_path"
        chmod +x "$_path"
    }

    _h2t9_write_pool_mock "$TMPDIR_T9/test_pool_1.sh" 0
    _h2t9_write_pool_mock "$TMPDIR_T9/test_pool_2.sh" 0
    _h2t9_write_pool_mock "$TMPDIR_T9/test_pool_3.sh" 1
    _h2t9_write_serial_mock "$TMPDIR_T9/test_serial_1.sh" 0
    _h2t9_write_serial_mock "$TMPDIR_T9/test_serial_2.sh" 0
    _h2t9_write_serial_mock "$TMPDIR_T9/test_hostx_1.sh" 0

    export H2_T9_LOAD_LIB="$LOAD_TOLERANCE_LIB_T9"
    export H2_T9_POOL_LOCK="$CNT_T9/pool.lock"
    export H2_T9_POOL_CUR="$CNT_T9/pool.cur"
    export H2_T9_POOL_MAX="$CNT_T9/pool.max"
    export H2_T9_POOL_ARRIVED="$CNT_T9/pool.arrived"
    export H2_T9_SERIAL_LOCK="$CNT_T9/serial.lock"
    export H2_T9_SERIAL_CUR="$CNT_T9/serial.cur"
    export H2_T9_SERIAL_MAX="$CNT_T9/serial.max"
    export H2_T9_POLL_BASE=3
    export H2_T9_ARRIVE_THRESHOLD=2
    export H2_T9_SERIAL_PAUSE_BASE=2

    # -- 9a: REIFY_RUN_ALL_POOL_CONCURRENCY=4 (4 slots, 3 pool members -- all
    # admitted concurrently) ------------------------------------------------
    echo 0 > "$H2_T9_POOL_CUR"; echo 0 > "$H2_T9_POOL_MAX"; echo 0 > "$H2_T9_POOL_ARRIVED"
    echo 0 > "$H2_T9_SERIAL_CUR"; echo 0 > "$H2_T9_SERIAL_MAX"
    LOCK_T9A="$TMPDIR_T9/pool-semaphore-a.lock"

    t9a_rc=0
    t9a_out="$(RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T9" \
        REIFY_RUN_ALL_POOL_LOCK="$LOCK_T9A" \
        REIFY_RUN_ALL_POOL_CONCURRENCY=4 \
        REIFY_RUN_ALL_POOL_PSI_DISABLE=1 \
        bash "$RUN_ALL" "$TMPDIR_T9" 2>&1)" || t9a_rc=$?

    t9a_pool_max="$(cat "$H2_T9_POOL_MAX" 2>/dev/null || echo 0)"
    assert "T9a: pool group max-concurrency >= 2 (got: $t9a_pool_max)" \
        test "$t9a_pool_max" -ge 2

    t9a_serial_max="$(cat "$H2_T9_SERIAL_MAX" 2>/dev/null || echo 0)"
    assert "T9a: serial group max-concurrency == 1 (got: $t9a_serial_max)" \
        test "$t9a_serial_max" -eq 1

    if [[ "$t9a_out" == *"=== Summary: 6 discovered, 1 failed ==="* ]]; then
        assert "T9a: byte-exact Summary line (6 discovered, 1 failed)" true
    else
        assert "T9a: byte-exact Summary line (6 discovered, 1 failed) (got: $t9a_out)" false
    fi

    if echo "$t9a_out" | grep -qE '^FAILED .*test_pool_3\.sh'; then
        assert "T9a: ^FAILED classifier marker names test_pool_3.sh" true
    else
        assert "T9a: ^FAILED classifier marker names test_pool_3.sh (got: $t9a_out)" false
    fi

    if echo "$t9a_out" | grep -qE '^=== FAILED:.*test_pool_3\.sh'; then
        assert "T9a: === FAILED: human line names test_pool_3.sh" true
    else
        assert "T9a: === FAILED: human line names test_pool_3.sh (got: $t9a_out)" false
    fi

    t9a_headers="$(echo "$t9a_out" | grep -E '^--- Running: ' | sed -E 's/^--- Running: (.*) ---$/\1/')"
    t9a_expected=$'test_hostx_1.sh\ntest_pool_1.sh\ntest_pool_2.sh\ntest_pool_3.sh\ntest_serial_1.sh\ntest_serial_2.sh'
    if [ "$t9a_headers" = "$t9a_expected" ]; then
        assert "T9a: discovered-order headers match sorted order" true
    else
        assert "T9a: discovered-order headers match sorted order (got: $t9a_headers)" false
    fi

    assert "T9a: run_all.sh exits 1 (one pool failure)" \
        test "$t9a_rc" -eq 1

    # -- 9b: REIFY_RUN_ALL_POOL_CONCURRENCY=1 (bound honored -- pool serializes) --
    echo 0 > "$H2_T9_POOL_CUR"; echo 0 > "$H2_T9_POOL_MAX"; echo 0 > "$H2_T9_POOL_ARRIVED"
    echo 0 > "$H2_T9_SERIAL_CUR"; echo 0 > "$H2_T9_SERIAL_MAX"
    LOCK_T9B="$TMPDIR_T9/pool-semaphore-b.lock"

    t9b_rc=0
    t9b_out="$(RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T9" \
        REIFY_RUN_ALL_POOL_LOCK="$LOCK_T9B" \
        REIFY_RUN_ALL_POOL_CONCURRENCY=1 \
        REIFY_RUN_ALL_POOL_PSI_DISABLE=1 \
        bash "$RUN_ALL" "$TMPDIR_T9" 2>&1)" || t9b_rc=$?

    t9b_pool_max="$(cat "$H2_T9_POOL_MAX" 2>/dev/null || echo 0)"
    assert "T9b: REIFY_RUN_ALL_POOL_CONCURRENCY=1 forces pool max-concurrency == 1 (got: $t9b_pool_max)" \
        test "$t9b_pool_max" -eq 1
else
    assert "T9a: pool group max-concurrency >= 2 (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T9a: serial group max-concurrency == 1 (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T9a: byte-exact Summary line (6 discovered, 1 failed) (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T9a: ^FAILED classifier marker names test_pool_3.sh (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T9a: === FAILED: human line names test_pool_3.sh (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T9a: discovered-order headers match sorted order (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T9a: run_all.sh exits 1 (one pool failure) (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T9b: REIFY_RUN_ALL_POOL_CONCURRENCY=1 forces pool max-concurrency == 1 (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
fi

# -- Summary --------------------------------------------------------------------
test_summary
