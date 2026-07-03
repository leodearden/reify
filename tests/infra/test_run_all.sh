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

    # T9a specifically proves overlap (pool max-concurrency >= 2), so its
    # ARRIVED>=2 barrier uses a much larger poll base than the shared default
    # -- in practice a deadlock backstop rather than a race the
    # first-arriving mock could lose on a severely descheduled host (overlap
    # should be near-guaranteed, not merely probabilistic; the bound stays
    # finite so a genuine bug still fails the test instead of hanging it).
    # This override applies ONLY to the T9a invocation below (env-prefixed on
    # that one command) -- T9b intentionally keeps the small shared default:
    # with REIFY_RUN_ALL_POOL_CONCURRENCY=1 its sole running pool member can
    # never observe ARRIVED>=2 (no sibling runs concurrently), so it always
    # burns the full poll budget serially, and inflating that budget would
    # only add wall-clock to T9b with no proof-strength benefit.
    H2_T9_POLL_BASE_T9A=100

    # -- 9a: REIFY_RUN_ALL_POOL_CONCURRENCY=4 (4 slots, 3 pool members -- all
    # admitted concurrently) ------------------------------------------------
    echo 0 > "$H2_T9_POOL_CUR"; echo 0 > "$H2_T9_POOL_MAX"; echo 0 > "$H2_T9_POOL_ARRIVED"
    echo 0 > "$H2_T9_SERIAL_CUR"; echo 0 > "$H2_T9_SERIAL_MAX"
    LOCK_T9A="$TMPDIR_T9/pool-semaphore-a.lock"

    t9a_rc=0
    t9a_out="$(env -u REIFY_RUN_ALL_EXCLUDE_HOST_INFRA \
        RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T9" \
        REIFY_RUN_ALL_POOL_LOCK="$LOCK_T9A" \
        REIFY_RUN_ALL_POOL_CONCURRENCY=4 \
        REIFY_RUN_ALL_POOL_PSI_DISABLE=1 \
        H2_T9_POLL_BASE="$H2_T9_POLL_BASE_T9A" \
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

    t9a_headers="$(echo "$t9a_out" | grep -E '^--- Running: ' | sed -E 's/^--- Running: (.*) ---$/\1/')" || true
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
    t9b_out="$(env -u REIFY_RUN_ALL_EXCLUDE_HOST_INFRA \
        RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T9" \
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

# -- Test 10: H2 pool N observability (INFO line + knob echo) -------------------
# Proves run_all.sh reports its resolved concurrency bound N via an
# `INFO:`-prefixed stderr line (mirrors cargo-test-occt-gated.sh's own
# `INFO: ... N=` idiom) -- both when the knob is set explicitly and when it
# falls back to the nproc-derived default.
echo ""
echo "--- Test 10: H2 pool N observability (INFO line) ---"

if [ -f "$RUN_ALL" ] && [ -f "$LOAD_TOLERANCE_LIB_T9" ]; then
    TMPDIR_T10="$(mktemp -d)"
    _TMPDIRS+=("$TMPDIR_T10")
    MANIFEST_T10="$TMPDIR_T10/classification.manifest"
    printf 'test_pool_only.sh pool\n' > "$MANIFEST_T10"
    printf '#!/usr/bin/env bash\nexit 0\n' > "$TMPDIR_T10/test_pool_only.sh"
    chmod +x "$TMPDIR_T10/test_pool_only.sh"

    # 10a: explicit REIFY_RUN_ALL_POOL_CONCURRENCY=7 -- INFO line echoes N=7.
    LOCK_T10A="$TMPDIR_T10/pool-a.lock"
    t10a_rc=0
    t10a_out="$(RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T10" \
        REIFY_RUN_ALL_POOL_LOCK="$LOCK_T10A" \
        REIFY_RUN_ALL_POOL_CONCURRENCY=7 \
        REIFY_RUN_ALL_POOL_PSI_DISABLE=1 \
        bash "$RUN_ALL" "$TMPDIR_T10" 2>&1)" || t10a_rc=$?

    if [[ "$t10a_out" == *"INFO:"*"N=7"* ]]; then
        assert "T10a: INFO line reports N=7 with REIFY_RUN_ALL_POOL_CONCURRENCY=7" true
    else
        assert "T10a: INFO line reports N=7 with REIFY_RUN_ALL_POOL_CONCURRENCY=7 (got: $t10a_out)" false
    fi

    # 10b: knob unset -- INFO line still reports a positive-integer default N.
    LOCK_T10B="$TMPDIR_T10/pool-b.lock"
    t10b_rc=0
    t10b_out="$(RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T10" \
        REIFY_RUN_ALL_POOL_LOCK="$LOCK_T10B" \
        REIFY_RUN_ALL_POOL_PSI_DISABLE=1 \
        bash "$RUN_ALL" "$TMPDIR_T10" 2>&1)" || t10b_rc=$?

    t10b_n="$(echo "$t10b_out" | grep -oE 'N=[0-9]+' | head -1 | cut -d= -f2)" || true
    if [ -n "$t10b_n" ] && [ "$t10b_n" -ge 1 ] 2>/dev/null; then
        assert "T10b: INFO line reports a positive-integer default N (got: $t10b_out)" true
    else
        assert "T10b: INFO line reports a positive-integer default N (got: $t10b_out)" false
    fi
else
    assert "T10a: INFO line reports N=7 with REIFY_RUN_ALL_POOL_CONCURRENCY=7 (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T10b: INFO line reports a positive-integer default N (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
fi

# -- Test 11: H2 PSI soft-gate (non-blocking + fail-open) -----------------------
# Proves the pool's PSI gate is SOFT (paces spawns under sustained pressure
# but always admits -- never skips a test, never hangs) and fail-open (an
# unreadable PSI source disables gating entirely rather than erroring).
echo ""
echo "--- Test 11: H2 PSI soft-gate ---"

if [ -f "$RUN_ALL" ] && [ -f "$LOAD_TOLERANCE_LIB_T9" ]; then
    TMPDIR_T11="$(mktemp -d)"
    _TMPDIRS+=("$TMPDIR_T11")
    MANIFEST_T11="$TMPDIR_T11/classification.manifest"
    printf 'test_pool_p1.sh pool\ntest_pool_p2.sh pool\n' > "$MANIFEST_T11"
    printf '#!/usr/bin/env bash\nexit 0\n' > "$TMPDIR_T11/test_pool_p1.sh"
    printf '#!/usr/bin/env bash\nexit 0\n' > "$TMPDIR_T11/test_pool_p2.sh"
    chmod +x "$TMPDIR_T11/test_pool_p1.sh" "$TMPDIR_T11/test_pool_p2.sh"

    # 11a: hot fixture PSI file (avg10=95, >= threshold 85) -- gate engages
    # (soft) but the run STILL completes with both pool mocks executed, and a
    # yield/backoff note is present as evidence the gate actually engaged.
    PSI_HOT_T11="$TMPDIR_T11/psi_hot"
    printf 'some avg10=95.00 avg60=90.00 avg300=80.00 total=1\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' > "$PSI_HOT_T11"

    LOCK_T11A="$TMPDIR_T11/pool-a.lock"
    t11a_rc=0
    t11a_out="$(RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T11" \
        REIFY_RUN_ALL_POOL_LOCK="$LOCK_T11A" \
        REIFY_RUN_ALL_POOL_CONCURRENCY=4 \
        REIFY_RUN_ALL_POOL_PSI_PROC_PATH="$PSI_HOT_T11" \
        REIFY_RUN_ALL_POOL_PSI_THRESHOLD=85 \
        REIFY_RUN_ALL_POOL_PSI_ATTEMPTS=2 \
        bash "$RUN_ALL" "$TMPDIR_T11" 2>&1)" || t11a_rc=$?

    if [[ "$t11a_out" == *"=== Summary: 2 discovered, 0 failed ==="* ]]; then
        assert "T11a: run completes with both pool mocks under sustained PSI pressure" true
    else
        assert "T11a: run completes with both pool mocks under sustained PSI pressure (got: $t11a_out)" false
    fi

    if [[ "$t11a_out" == *"PSI backoff"* ]]; then
        assert "T11a: a PSI backoff/yield note is present (gate engaged)" true
    else
        assert "T11a: a PSI backoff/yield note is present (gate engaged) (got: $t11a_out)" false
    fi

    # 11b: fail-open -- nonexistent PSI source -> no gating, run completes normally.
    LOCK_T11B="$TMPDIR_T11/pool-b.lock"
    t11b_rc=0
    t11b_out="$(RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T11" \
        REIFY_RUN_ALL_POOL_LOCK="$LOCK_T11B" \
        REIFY_RUN_ALL_POOL_CONCURRENCY=4 \
        REIFY_RUN_ALL_POOL_PSI_PROC_PATH="$TMPDIR_T11/does-not-exist/psi" \
        bash "$RUN_ALL" "$TMPDIR_T11" 2>&1)" || t11b_rc=$?

    if [[ "$t11b_out" == *"=== Summary: 2 discovered, 0 failed ==="* ]]; then
        assert "T11b: run completes normally with an unreadable PSI source (fail-open)" true
    else
        assert "T11b: run completes normally with an unreadable PSI source (fail-open) (got: $t11b_out)" false
    fi

    if [[ "$t11b_out" != *"PSI backoff"* ]]; then
        assert "T11b: no PSI backoff note when source is unreadable (fail-open, no gating)" true
    else
        assert "T11b: no PSI backoff note when source is unreadable (fail-open, no gating) (got: $t11b_out)" false
    fi
else
    assert "T11a: run completes with both pool mocks under sustained PSI pressure (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T11a: a PSI backoff/yield note is present (gate engaged) (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T11b: run completes normally with an unreadable PSI source (fail-open) (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T11b: no PSI backoff note when source is unreadable (fail-open, no gating) (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
fi

# -- Test 12: H2 verify-pipeline full-gate routing ------------------------------
# run_all.sh is the infra-test runner every verify invocation drives, but it
# is neither sourced by verify.sh nor otherwise listed, so a run_all.sh-only
# diff (like this task's own concurrent-pool change) could take the
# merge-worker config fast-path and never actually exercise the new pool
# machinery. Registering it in scripts/verify-pipeline-paths.txt routes
# run_all.sh edits to the full --scope all gate.
echo ""
echo "--- Test 12: verify-pipeline-guard.sh routes run_all.sh to the full gate ---"

GUARD_T12="$REPO_ROOT/scripts/verify-pipeline-guard.sh"
if [ -f "$GUARD_T12" ]; then
    t12_rc=0
    bash "$GUARD_T12" requires-full-gate tests/infra/run_all.sh >/dev/null 2>&1 || t12_rc=$?
    assert "run_all.sh requires the full --scope all gate (exit 0)" \
        test "$t12_rc" -eq 0

    assert "verify-pipeline-guard.sh --list includes tests/infra/run_all.sh" \
        bash -c "bash '$GUARD_T12' --list | grep -qxF 'tests/infra/run_all.sh'"
else
    assert "run_all.sh requires the full --scope all gate (skipped - guard missing)" false
    assert "verify-pipeline-guard.sh --list includes tests/infra/run_all.sh (skipped - guard missing)" false
fi

# -- Test 13: H3 REIFY_RUN_ALL_EXCLUDE_HOST_INFRA exclusion seam (pool path) ----
# Proves the strict-1 flip-seam knob excludes the `host-exclusive` bucket from
# discovery on the H2 pool path, and that any other value (unset/"0"/empty/
# garbage/whitespace-padded) runs the FULL discovered set unchanged (DA1:
# strictly additive default -- a malformed knob must never silently drop
# host-infra coverage).
echo ""
echo "--- Test 13: H3 exclusion seam (pool path) ---"

if [ -f "$RUN_ALL" ] && [ -f "$LOAD_TOLERANCE_LIB_T9" ]; then
    TMPDIR_T13="$(mktemp -d)"
    _TMPDIRS+=("$TMPDIR_T13")

    # Fixture manifest: 3 `pool` + 1 `host-exclusive` (full discovered = 4).
    MANIFEST_T13="$TMPDIR_T13/classification.manifest"
    cat > "$MANIFEST_T13" <<'EOF'
test_pool_1.sh pool
test_pool_2.sh pool
test_pool_3.sh pool
test_hostx_1.sh host-exclusive
EOF

    printf '#!/usr/bin/env bash\nexit 0\n' > "$TMPDIR_T13/test_pool_1.sh"
    printf '#!/usr/bin/env bash\nexit 0\n' > "$TMPDIR_T13/test_pool_2.sh"
    printf '#!/usr/bin/env bash\nexit 0\n' > "$TMPDIR_T13/test_pool_3.sh"
    printf '#!/usr/bin/env bash\nexit 0\n' > "$TMPDIR_T13/test_hostx_1.sh"
    chmod +x "$TMPDIR_T13/test_pool_1.sh" "$TMPDIR_T13/test_pool_2.sh" \
              "$TMPDIR_T13/test_pool_3.sh" "$TMPDIR_T13/test_hostx_1.sh"

    # 13a: knob=1 -- excludes the host-exclusive member.
    t13a_rc=0
    t13a_out="$(RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T13" \
        REIFY_RUN_ALL_POOL_LOCK="$TMPDIR_T13/pool-a.lock" \
        REIFY_RUN_ALL_POOL_PSI_DISABLE=1 \
        REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1 \
        bash "$RUN_ALL" "$TMPDIR_T13" 2>&1)" || t13a_rc=$?

    # Guard against false confidence: if the pool substrate were ever absent,
    # run_all.sh silently falls back to the legacy for-loop, which (via the
    # same shared _h3_exclude filter) would also print "3 discovered" --
    # passing 13a below without ever exercising the pool-path filter code
    # (lines under the `if [ "$_H2_POOL_ACTIVE" -eq 1 ]` branch). Assert the
    # pool-path-only "INFO: run_all.sh pool: N=" marker is present so this
    # test fails loudly instead of silently degrading to Test 14's coverage.
    if [[ "$t13a_out" == *"INFO: run_all.sh pool: N="* ]]; then
        assert "T13a: pool path was actually taken (INFO: run_all.sh pool: N= present)" true
    else
        assert "T13a: pool path was actually taken (INFO: run_all.sh pool: N= present) (got: $t13a_out)" false
    fi

    if [[ "$t13a_out" == *"=== Summary: 3 discovered, 0 failed ==="* ]]; then
        assert "T13a: knob=1 excludes host-exclusive member (3 discovered)" true
    else
        assert "T13a: knob=1 excludes host-exclusive member (3 discovered) (got: $t13a_out)" false
    fi

    if [[ "$t13a_out" != *"--- Running: test_hostx_1.sh ---"* ]]; then
        assert "T13a: knob=1 host-exclusive header absent" true
    else
        assert "T13a: knob=1 host-exclusive header absent (got: $t13a_out)" false
    fi

    assert "T13a: knob=1 run_all.sh exits 0" \
        test "$t13a_rc" -eq 0

    # 13b: knob UNSET -- full set runs (strictly additive default).
    t13b_out="$(env -u REIFY_RUN_ALL_EXCLUDE_HOST_INFRA \
        RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T13" \
        REIFY_RUN_ALL_POOL_LOCK="$TMPDIR_T13/pool-b.lock" \
        REIFY_RUN_ALL_POOL_PSI_DISABLE=1 \
        bash "$RUN_ALL" "$TMPDIR_T13" 2>&1)" || true

    if [[ "$t13b_out" == *"=== Summary: 4 discovered, 0 failed ==="* ]]; then
        assert "T13b: knob unset runs full set (4 discovered)" true
    else
        assert "T13b: knob unset runs full set (4 discovered) (got: $t13b_out)" false
    fi

    if [[ "$t13b_out" == *"--- Running: test_hostx_1.sh ---"* ]]; then
        assert "T13b: knob unset host-exclusive header present" true
    else
        assert "T13b: knob unset host-exclusive header present (got: $t13b_out)" false
    fi

    # 13c: knob="0" -- strict-1 negative assertion: full set runs.
    t13c_out="$(RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T13" \
        REIFY_RUN_ALL_POOL_LOCK="$TMPDIR_T13/pool-c.lock" \
        REIFY_RUN_ALL_POOL_PSI_DISABLE=1 \
        REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=0 \
        bash "$RUN_ALL" "$TMPDIR_T13" 2>&1)" || true

    if [[ "$t13c_out" == *"=== Summary: 4 discovered, 0 failed ==="* ]]; then
        assert "T13c: knob=0 runs full set (strict-1 negative)" true
    else
        assert "T13c: knob=0 runs full set (strict-1 negative) (got: $t13c_out)" false
    fi

    # 13d: knob="" (empty string) -- full set runs.
    t13d_out="$(RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T13" \
        REIFY_RUN_ALL_POOL_LOCK="$TMPDIR_T13/pool-d.lock" \
        REIFY_RUN_ALL_POOL_PSI_DISABLE=1 \
        REIFY_RUN_ALL_EXCLUDE_HOST_INFRA="" \
        bash "$RUN_ALL" "$TMPDIR_T13" 2>&1)" || true

    if [[ "$t13d_out" == *"=== Summary: 4 discovered, 0 failed ==="* ]]; then
        assert "T13d: knob=empty runs full set" true
    else
        assert "T13d: knob=empty runs full set (got: $t13d_out)" false
    fi

    # 13e/13f: garbage values ("2", "true") -- full set runs (malformed knob
    # never drops coverage).
    t13e_out="$(RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T13" \
        REIFY_RUN_ALL_POOL_LOCK="$TMPDIR_T13/pool-e.lock" \
        REIFY_RUN_ALL_POOL_PSI_DISABLE=1 \
        REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=2 \
        bash "$RUN_ALL" "$TMPDIR_T13" 2>&1)" || true

    if [[ "$t13e_out" == *"=== Summary: 4 discovered, 0 failed ==="* ]]; then
        assert "T13e: knob=2 (garbage) runs full set" true
    else
        assert "T13e: knob=2 (garbage) runs full set (got: $t13e_out)" false
    fi

    t13f_out="$(RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T13" \
        REIFY_RUN_ALL_POOL_LOCK="$TMPDIR_T13/pool-f.lock" \
        REIFY_RUN_ALL_POOL_PSI_DISABLE=1 \
        REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=true \
        bash "$RUN_ALL" "$TMPDIR_T13" 2>&1)" || true

    if [[ "$t13f_out" == *"=== Summary: 4 discovered, 0 failed ==="* ]]; then
        assert "T13f: knob=true (garbage) runs full set" true
    else
        assert "T13f: knob=true (garbage) runs full set (got: $t13f_out)" false
    fi

    # 13g: knob=" 1 " (whitespace-padded) -- strict equality, not a loose
    # match: full set runs.
    t13g_out="$(RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T13" \
        REIFY_RUN_ALL_POOL_LOCK="$TMPDIR_T13/pool-g.lock" \
        REIFY_RUN_ALL_POOL_PSI_DISABLE=1 \
        REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=" 1 " \
        bash "$RUN_ALL" "$TMPDIR_T13" 2>&1)" || true

    if [[ "$t13g_out" == *"=== Summary: 4 discovered, 0 failed ==="* ]]; then
        assert "T13g: knob=' 1 ' (whitespace) runs full set (strict equality)" true
    else
        assert "T13g: knob=' 1 ' (whitespace) runs full set (got: $t13g_out)" false
    fi
else
    assert "T13a: pool path was actually taken (INFO: run_all.sh pool: N= present) (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T13a: knob=1 excludes host-exclusive member (3 discovered) (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T13a: knob=1 host-exclusive header absent (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T13a: knob=1 run_all.sh exits 0 (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T13b: knob unset runs full set (4 discovered) (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T13b: knob unset host-exclusive header present (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T13c: knob=0 runs full set (strict-1 negative) (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T13d: knob=empty runs full set (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T13e: knob=2 (garbage) runs full set (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T13f: knob=true (garbage) runs full set (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T13g: knob=' 1 ' (whitespace) runs full set (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
fi

# -- Test 14: H3 exclusion also applies on the legacy all-serial fallback ------
# Proves the exclusion seam is not pool-path-only: forcing the legacy
# all-serial fallback via REIFY_RUN_ALL_POOL_DISABLE=1 (break-glass) still
# drops the host-exclusive member from discovery when the knob is exactly
# "1", and still runs the full set for any other knob value.
echo ""
echo "--- Test 14: H3 exclusion seam (legacy fallback path) ---"

if [ -f "$RUN_ALL" ]; then
    TMPDIR_T14="$(mktemp -d)"
    _TMPDIRS+=("$TMPDIR_T14")

    # Fixture manifest: 1 `pool` + 1 `host-exclusive` (full discovered = 2).
    MANIFEST_T14="$TMPDIR_T14/classification.manifest"
    cat > "$MANIFEST_T14" <<'EOF'
test_pool_1.sh pool
test_hostx_1.sh host-exclusive
EOF

    printf '#!/usr/bin/env bash\nexit 0\n' > "$TMPDIR_T14/test_pool_1.sh"
    printf '#!/usr/bin/env bash\nexit 0\n' > "$TMPDIR_T14/test_hostx_1.sh"
    chmod +x "$TMPDIR_T14/test_pool_1.sh" "$TMPDIR_T14/test_hostx_1.sh"

    # 14a: knob=1 + legacy fallback -- excludes the host-exclusive member.
    t14a_rc=0
    t14a_out="$(RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T14" \
        REIFY_RUN_ALL_POOL_DISABLE=1 \
        REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1 \
        bash "$RUN_ALL" "$TMPDIR_T14" 2>&1)" || t14a_rc=$?

    if [[ "$t14a_out" == *"=== Summary: 1 discovered, 0 failed ==="* ]]; then
        assert "T14a: knob=1 + legacy fallback excludes host-exclusive member (1 discovered)" true
    else
        assert "T14a: knob=1 + legacy fallback excludes host-exclusive member (1 discovered) (got: $t14a_out)" false
    fi

    if [[ "$t14a_out" != *"--- Running: test_hostx_1.sh ---"* ]]; then
        assert "T14a: knob=1 + legacy fallback host-exclusive header absent" true
    else
        assert "T14a: knob=1 + legacy fallback host-exclusive header absent (got: $t14a_out)" false
    fi

    assert "T14a: knob=1 + legacy fallback run_all.sh exits 0" \
        test "$t14a_rc" -eq 0

    # 14b: knob UNSET + legacy fallback -- full set runs.
    t14b_out="$(env -u REIFY_RUN_ALL_EXCLUDE_HOST_INFRA \
        RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T14" \
        REIFY_RUN_ALL_POOL_DISABLE=1 \
        bash "$RUN_ALL" "$TMPDIR_T14" 2>&1)" || true

    if [[ "$t14b_out" == *"=== Summary: 2 discovered, 0 failed ==="* ]]; then
        assert "T14b: knob unset + legacy fallback runs full set (2 discovered)" true
    else
        assert "T14b: knob unset + legacy fallback runs full set (2 discovered) (got: $t14b_out)" false
    fi

    if [[ "$t14b_out" == *"--- Running: test_hostx_1.sh ---"* ]]; then
        assert "T14b: knob unset + legacy fallback host-exclusive header present" true
    else
        assert "T14b: knob unset + legacy fallback host-exclusive header present (got: $t14b_out)" false
    fi

    # 14c: knob="0" + legacy fallback -- strict-1 negative assertion: full set runs.
    t14c_out="$(RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T14" \
        REIFY_RUN_ALL_POOL_DISABLE=1 \
        REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=0 \
        bash "$RUN_ALL" "$TMPDIR_T14" 2>&1)" || true

    if [[ "$t14c_out" == *"=== Summary: 2 discovered, 0 failed ==="* ]]; then
        assert "T14c: knob=0 + legacy fallback runs full set (strict-1 negative)" true
    else
        assert "T14c: knob=0 + legacy fallback runs full set (got: $t14c_out)" false
    fi
else
    assert "T14a: knob=1 + legacy fallback excludes host-exclusive member (1 discovered) (skipped - run_all.sh missing)" false
    assert "T14a: knob=1 + legacy fallback host-exclusive header absent (skipped - run_all.sh missing)" false
    assert "T14a: knob=1 + legacy fallback run_all.sh exits 0 (skipped - run_all.sh missing)" false
    assert "T14b: knob unset + legacy fallback runs full set (2 discovered) (skipped - run_all.sh missing)" false
    assert "T14b: knob unset + legacy fallback host-exclusive header present (skipped - run_all.sh missing)" false
    assert "T14c: knob=0 + legacy fallback runs full set (skipped - run_all.sh missing)" false
fi

# -- H9 shared fixture helpers (Tests 15-17) -------------------------------------
# _mk_test_mock <path> <exit_code>: writes a minimal test_*.sh mock and
# chmods it executable.
_mk_test_mock() {
    local _path="$1" _exit_code="${2:-0}"
    printf '#!/usr/bin/env bash\nexit %s\n' "$_exit_code" > "$_path"
    chmod +x "$_path"
}

# _mk_hostinfra_fixture_3x2x2 <dir>: writes the standard H9 fixture (3 pool +
# 2 intra-run-serial + 2 host-exclusive, all exit 0) as
# <dir>/classification.manifest plus matching mock files. Tests 15 and 17
# deliberately share this EXACT fixture shape (T17's exactly-once-partition
# assertion needs the same 7 members T15 already proved the host-infra
# behavior against), so a change to the fixture shape is a single edit
# instead of two.
_mk_hostinfra_fixture_3x2x2() {
    local _dir="$1" _name
    cat > "$_dir/classification.manifest" <<'EOF'
test_pool_1.sh pool
test_pool_2.sh pool
test_pool_3.sh pool
test_serial_1.sh intra-run-serial
test_serial_2.sh intra-run-serial
test_hostx_1.sh host-exclusive
test_hostx_2.sh host-exclusive
EOF
    for _name in test_pool_1 test_pool_2 test_pool_3 test_serial_1 test_serial_2 test_hostx_1 test_hostx_2; do
        _mk_test_mock "$_dir/${_name}.sh" 0
    done
}

# -- Test 15: H9 --scope host-infra runs exactly the host-exclusive set ---------
# Proves the new `--scope host-infra` run mode runs EXACTLY the declared
# host-exclusive members (no pool/serial members), preserves the byte-exact
# Summary contract, exits 0, and — being the INVERSE of H3's exclusion —
# ignores REIFY_RUN_ALL_EXCLUDE_HOST_INFRA entirely (a knob=1 hot-path run
# must not also suppress the runner meant to catch the excluded residue).
echo ""
echo "--- Test 15: H9 --scope host-infra (core: exact set, contract, knob-ignore) ---"

if [ -f "$RUN_ALL" ]; then
    TMPDIR_T15="$(mktemp -d)"
    _TMPDIRS+=("$TMPDIR_T15")

    # Fixture: 3 `pool` + 2 `intra-run-serial` + 2 `host-exclusive` (full
    # discovered = 7; host-infra scope should touch only the 2). Shared
    # helper defined above Test 15 (also used by Test 17).
    MANIFEST_T15="$TMPDIR_T15/classification.manifest"
    _mk_hostinfra_fixture_3x2x2 "$TMPDIR_T15"

    # Every host-infra invocation overrides REIFY_LANE_X_FLOCK_LOCK to a temp
    # path — the lib's default is a FIXED per-uid host path shared with real
    # host-infra runs / concurrent verify lanes, which would be non-hermetic.
    LOCK_T15="$TMPDIR_T15/lane-x.lock"

    t15_rc=0
    t15_out="$(RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T15" \
        REIFY_LANE_X_FLOCK_LOCK="$LOCK_T15" \
        bash "$RUN_ALL" --scope host-infra "$TMPDIR_T15" 2>&1)" || t15_rc=$?
    rm -f "$LOCK_T15" "${LOCK_T15}.slot-1"

    t15_headers="$(echo "$t15_out" | grep -E '^--- Running: ' | sed -E 's/^--- Running: (.*) ---$/\1/')" || true
    t15_expected=$'test_hostx_1.sh\ntest_hostx_2.sh'
    if [ "$t15_headers" = "$t15_expected" ]; then
        assert "T15a: --scope host-infra headers are EXACTLY the host-exclusive set" true
    else
        assert "T15a: --scope host-infra headers are EXACTLY the host-exclusive set (got: $t15_headers)" false
    fi

    if [[ "$t15_out" == *"=== Summary: 2 discovered, 0 failed ==="* ]]; then
        assert "T15b: byte-exact Summary line (2 discovered, 0 failed)" true
    else
        assert "T15b: byte-exact Summary line (2 discovered, 0 failed) (got: $t15_out)" false
    fi

    assert "T15c: --scope host-infra exits 0" \
        test "$t15_rc" -eq 0

    # 15d/e: inverse runner ignores REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1 — the
    # knob that moves host-exclusive OFF the hot path must not also suppress
    # the runner meant to catch it off-path.
    LOCK_T15D="$TMPDIR_T15/lane-x-d.lock"
    t15d_rc=0
    t15d_out="$(RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T15" \
        REIFY_LANE_X_FLOCK_LOCK="$LOCK_T15D" \
        REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1 \
        bash "$RUN_ALL" --scope host-infra "$TMPDIR_T15" 2>&1)" || t15d_rc=$?
    rm -f "$LOCK_T15D" "${LOCK_T15D}.slot-1"

    if [[ "$t15d_out" == *"=== Summary: 2 discovered, 0 failed ==="* ]]; then
        assert "T15d: --scope host-infra ignores REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1 (still 2 discovered)" true
    else
        assert "T15d: --scope host-infra ignores REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1 (still 2 discovered) (got: $t15d_out)" false
    fi

    if [[ "$t15d_out" == *"--- Running: test_hostx_1.sh ---"* ]] && [[ "$t15d_out" == *"--- Running: test_hostx_2.sh ---"* ]]; then
        assert "T15e: --scope host-infra headers present despite exclusion knob=1" true
    else
        assert "T15e: --scope host-infra headers present despite exclusion knob=1 (got: $t15d_out)" false
    fi

    assert "T15f: --scope host-infra + knob=1 still exits 0" \
        test "$t15d_rc" -eq 0
else
    assert "T15a: --scope host-infra headers are EXACTLY the host-exclusive set (skipped - run_all.sh missing)" false
    assert "T15b: byte-exact Summary line (2 discovered, 0 failed) (skipped - run_all.sh missing)" false
    assert "T15c: --scope host-infra exits 0 (skipped - run_all.sh missing)" false
    assert "T15d: --scope host-infra ignores REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1 (still 2 discovered) (skipped - run_all.sh missing)" false
    assert "T15e: --scope host-infra headers present despite exclusion knob=1 (skipped - run_all.sh missing)" false
    assert "T15f: --scope host-infra + knob=1 still exits 0 (skipped - run_all.sh missing)" false
fi

# -- Test 16: H9 --scope host-infra -- failure contract + flock single-flight --
# (a) A failing host-exclusive mock is named by both the bare `^FAILED `
#     classifier marker and the `=== FAILED:` human line, with the byte-exact
#     Summary reflecting the host-exclusive-only discovered count.
# (b) A held Lane-X lock (background holder, WAIT=0) makes `--scope
#     host-infra` exit 75 fast, run NO member, and diagnose on stderr --
#     mirrors test_lane_x_flock.sh's own Test 12 holder pattern.
echo ""
echo "--- Test 16: H9 --scope host-infra (failure contract + flock single-flight) ---"

if [ -f "$RUN_ALL" ]; then
    TMPDIR_T16="$(mktemp -d)"
    _TMPDIRS+=("$TMPDIR_T16")

    # Fixture manifest: 1 `pool` (must NOT run under --scope host-infra) + 2
    # `host-exclusive` (one passes, one fails). Different shape than the
    # T15/T17 3x2x2 fixture (needs a failing member), so it stays inline;
    # mock-file creation reuses the shared _mk_test_mock helper (defined
    # above Test 15).
    MANIFEST_T16="$TMPDIR_T16/classification.manifest"
    cat > "$MANIFEST_T16" <<'EOF'
test_pool_1.sh pool
test_hostx_ok.sh host-exclusive
test_hostx_boom.sh host-exclusive
EOF

    _mk_test_mock "$TMPDIR_T16/test_pool_1.sh" 0
    _mk_test_mock "$TMPDIR_T16/test_hostx_ok.sh" 0
    _mk_test_mock "$TMPDIR_T16/test_hostx_boom.sh" 1

    # -- 16a: failure contract -----------------------------------------------
    LOCK_T16A="$TMPDIR_T16/lane-x-a.lock"
    t16a_rc=0
    t16a_out="$(RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T16" \
        REIFY_LANE_X_FLOCK_LOCK="$LOCK_T16A" \
        bash "$RUN_ALL" --scope host-infra "$TMPDIR_T16" 2>&1)" || t16a_rc=$?
    rm -f "$LOCK_T16A" "${LOCK_T16A}.slot-1"

    assert "T16a: --scope host-infra with a failing member exits 1" \
        test "$t16a_rc" -eq 1

    if echo "$t16a_out" | grep -qE '^FAILED .*test_hostx_boom\.sh'; then
        assert "T16b: ^FAILED classifier marker names test_hostx_boom.sh" true
    else
        assert "T16b: ^FAILED classifier marker names test_hostx_boom.sh (got: $t16a_out)" false
    fi

    if echo "$t16a_out" | grep -qE '^=== FAILED:.*test_hostx_boom\.sh'; then
        assert "T16c: === FAILED: human line names test_hostx_boom.sh" true
    else
        assert "T16c: === FAILED: human line names test_hostx_boom.sh (got: $t16a_out)" false
    fi

    if [[ "$t16a_out" == *"=== Summary: 2 discovered, 1 failed ==="* ]]; then
        assert "T16d: byte-exact Summary line (2 discovered, 1 failed)" true
    else
        assert "T16d: byte-exact Summary line (2 discovered, 1 failed) (got: $t16a_out)" false
    fi

    if [[ "$t16a_out" != *"--- Running: test_pool_1.sh ---"* ]]; then
        assert "T16e: the pool member is NOT run under --scope host-infra" true
    else
        assert "T16e: the pool member is NOT run under --scope host-infra (got: $t16a_out)" false
    fi

    # -- 16b: Lane-X single-flight -- held lock => exit 75 fast, no member runs --
    LOCK_T16B="$TMPDIR_T16/lane-x-b.lock"

    # Background holder: acquire slot-1 and hold it for 45s (exceeds the
    # outer timeout below).
    ( flock -x 9; sleep 45 ) 9>>"${LOCK_T16B}.slot-1" &
    _HOLDER_T16B=$!
    sleep 0.2   # give holder time to acquire

    _ERR_T16B="$(mktemp)"
    t16f_rc=0
    t16f_out="$(RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T16" \
        REIFY_LANE_X_FLOCK_LOCK="$LOCK_T16B" \
        REIFY_LANE_X_FLOCK_WAIT=0 \
        timeout 30 bash "$RUN_ALL" --scope host-infra "$TMPDIR_T16" 2>"$_ERR_T16B")" || t16f_rc=$?

    kill "$_HOLDER_T16B" 2>/dev/null || true
    wait "$_HOLDER_T16B" 2>/dev/null || true
    rm -f "$LOCK_T16B" "${LOCK_T16B}.slot-1"

    assert "T16f: --scope host-infra with a held Lane-X lock exits 75 fast (got $t16f_rc)" \
        test "$t16f_rc" -eq 75

    if [[ "$t16f_out" != *"--- Running:"* ]]; then
        assert "T16g: no member is run when the Lane-X lock is held (no Running: header)" true
    else
        assert "T16g: no member is run when the Lane-X lock is held (got: $t16f_out)" false
    fi

    assert "T16h: stderr contains an 'acquire' diagnostic (case-insensitive)" \
        bash -c 'grep -qi acquire "$1"' -- "$_ERR_T16B"
    assert "T16i: stderr contains a 'Lane-X' diagnostic (case-insensitive)" \
        bash -c 'grep -qi lane-x "$1"' -- "$_ERR_T16B"

    rm -f "$_ERR_T16B"
else
    assert "T16a: --scope host-infra with a failing member exits 1 (skipped - run_all.sh missing)" false
    assert "T16b: ^FAILED classifier marker names test_hostx_boom.sh (skipped - run_all.sh missing)" false
    assert "T16c: === FAILED: human line names test_hostx_boom.sh (skipped - run_all.sh missing)" false
    assert "T16d: byte-exact Summary line (2 discovered, 1 failed) (skipped - run_all.sh missing)" false
    assert "T16e: the pool member is NOT run under --scope host-infra (skipped - run_all.sh missing)" false
    assert "T16f: --scope host-infra with a held Lane-X lock exits 75 fast (skipped - run_all.sh missing)" false
    assert "T16g: no member is run when the Lane-X lock is held (skipped - run_all.sh missing)" false
    assert "T16h: stderr contains an 'acquire' diagnostic (case-insensitive) (skipped - run_all.sh missing)" false
    assert "T16i: stderr contains a 'Lane-X' diagnostic (case-insensitive) (skipped - run_all.sh missing)" false
fi

# -- Test 17: H9 exactly-once partition + hot-path-inert + scope validation -----
# (a) The PRD's leaf signal: a knob=1 hot-path run (pool+serial) and a
#     `--scope host-infra` run, over the SAME fixture, together cover the
#     full discovered universe exactly once (disjoint headers, union == all).
# (b) Hot-path-inert: the DEFAULT run_all.sh (no --scope) must complete and
#     run every discovered test even with the Lane-X lock held elsewhere --
#     proving the default path never touches the flock at all (the
#     behavioral form of the retired test_lane_x_flock.sh inert guard).
# (c) --scope value validation: an unrecognized value / a bare --scope with
#     no following value both exit 64 with a stderr diagnostic.
echo ""
echo "--- Test 17: H9 exactly-once partition + hot-path-inert ---"

if [ -f "$RUN_ALL" ] && [ -f "$LOAD_TOLERANCE_LIB_T9" ]; then
    TMPDIR_T17="$(mktemp -d)"
    _TMPDIRS+=("$TMPDIR_T17")

    # Fixture: 3 pool + 2 intra-run-serial + 2 host-exclusive (7 discovered
    # total). Shared helper defined above Test 15 (identical fixture shape
    # to Test 15, deliberately — see the helper's doc comment).
    MANIFEST_T17="$TMPDIR_T17/classification.manifest"
    _mk_hostinfra_fixture_3x2x2 "$TMPDIR_T17"

    # -- 17a/b: exactly-once partition ---------------------------------------
    t17_hot_out="$(RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T17" \
        REIFY_RUN_ALL_POOL_LOCK="$TMPDIR_T17/pool-hot.lock" \
        REIFY_RUN_ALL_POOL_PSI_DISABLE=1 \
        REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1 \
        bash "$RUN_ALL" "$TMPDIR_T17" 2>&1)" || true

    LOCK_T17HI="$TMPDIR_T17/lane-x-hi.lock"
    t17_hi_out="$(RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T17" \
        REIFY_LANE_X_FLOCK_LOCK="$LOCK_T17HI" \
        bash "$RUN_ALL" --scope host-infra "$TMPDIR_T17" 2>&1)" || true
    rm -f "$LOCK_T17HI" "${LOCK_T17HI}.slot-1"

    t17_hot_headers="$(echo "$t17_hot_out" | grep -E '^--- Running: ' | sed -E 's/^--- Running: (.*) ---$/\1/' | sort)"
    t17_hi_headers="$(echo "$t17_hi_out" | grep -E '^--- Running: ' | sed -E 's/^--- Running: (.*) ---$/\1/' | sort)"

    t17_overlap="$(comm -12 <(printf '%s\n' "$t17_hot_headers") <(printf '%s\n' "$t17_hi_headers") 2>/dev/null)" || true
    if [ -z "$t17_overlap" ]; then
        assert "T17a: knob=1 hot-path headers and --scope host-infra headers are disjoint" true
    else
        assert "T17a: knob=1 hot-path headers and --scope host-infra headers are disjoint (got overlap: $t17_overlap)" false
    fi

    t17_union="$(printf '%s\n%s\n' "$t17_hot_headers" "$t17_hi_headers" | sort -u)"
    t17_expected_union=$'test_hostx_1.sh\ntest_hostx_2.sh\ntest_pool_1.sh\ntest_pool_2.sh\ntest_pool_3.sh\ntest_serial_1.sh\ntest_serial_2.sh'
    if [ "$t17_union" = "$t17_expected_union" ]; then
        assert "T17b: union of knob=1 hot-path + --scope host-infra headers == full discovered set (covered exactly once)" true
    else
        assert "T17b: union of knob=1 hot-path + --scope host-infra headers == full discovered set (got: $t17_union)" false
    fi

    # -- 17c/d: hot-path-inert ------------------------------------------------
    LOCK_T17HP="$TMPDIR_T17/lane-x-hotpath.lock"
    ( flock -x 9; sleep 45 ) 9>>"${LOCK_T17HP}.slot-1" &
    _HOLDER_T17HP=$!
    sleep 0.2   # give holder time to acquire

    t17c_rc=0
    t17c_out="$(env -u REIFY_RUN_ALL_EXCLUDE_HOST_INFRA \
        RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T17" \
        REIFY_RUN_ALL_POOL_LOCK="$TMPDIR_T17/pool-hp.lock" \
        REIFY_RUN_ALL_POOL_PSI_DISABLE=1 \
        REIFY_LANE_X_FLOCK_LOCK="$LOCK_T17HP" \
        REIFY_LANE_X_FLOCK_WAIT=0 \
        timeout 30 bash "$RUN_ALL" "$TMPDIR_T17" 2>&1)" || t17c_rc=$?

    kill "$_HOLDER_T17HP" 2>/dev/null || true
    wait "$_HOLDER_T17HP" 2>/dev/null || true
    rm -f "$LOCK_T17HP" "${LOCK_T17HP}.slot-1"

    if [[ "$t17c_out" == *"=== Summary: 7 discovered, 0 failed ==="* ]]; then
        assert "T17c: default run_all.sh (no --scope) completes with full count despite a held Lane-X lock (hot-path-inert)" true
    else
        assert "T17c: default run_all.sh (no --scope) completes with full count despite a held Lane-X lock (got: $t17c_out)" false
    fi

    assert "T17d: default run_all.sh (no --scope) exits 0 despite a held Lane-X lock" \
        test "$t17c_rc" -eq 0
else
    assert "T17a: knob=1 hot-path headers and --scope host-infra headers are disjoint (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T17b: union of knob=1 hot-path + --scope host-infra headers == full discovered set (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T17c: default run_all.sh (no --scope) completes with full count despite a held Lane-X lock (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T17d: default run_all.sh (no --scope) exits 0 despite a held Lane-X lock (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
fi

# -- Test 17 (continued): --scope value validation ------------------------------
echo ""
echo "--- Test 17 (continued): --scope value validation ---"

if [ -f "$RUN_ALL" ]; then
    TMPDIR_T17V="$(mktemp -d)"
    _TMPDIRS+=("$TMPDIR_T17V")

    # Check for a 'scope'-specific diagnostic (not merely non-empty stderr --
    # the pool path already writes an incidental "INFO: ... pool: N=" line to
    # stderr, which would make a bare non-empty check pass vacuously).
    _ERR_T17E="$(mktemp)"
    t17e_rc=0
    bash "$RUN_ALL" --scope bogus "$TMPDIR_T17V" >/dev/null 2>"$_ERR_T17E" || t17e_rc=$?
    assert "T17e: --scope bogus exits 64" \
        test "$t17e_rc" -eq 64
    assert "T17f: --scope bogus emits a 'scope' diagnostic on stderr" \
        bash -c 'grep -qi scope "$1"' -- "$_ERR_T17E"
    rm -f "$_ERR_T17E"

    _ERR_T17G="$(mktemp)"
    t17g_rc=0
    bash "$RUN_ALL" --scope >/dev/null 2>"$_ERR_T17G" || t17g_rc=$?
    assert "T17g: bare --scope (no value) exits 64" \
        test "$t17g_rc" -eq 64
    assert "T17h: bare --scope (no value) emits a 'scope' diagnostic on stderr" \
        bash -c 'grep -qi scope "$1"' -- "$_ERR_T17G"
    rm -f "$_ERR_T17G"
else
    assert "T17e: --scope bogus exits 64 (skipped - run_all.sh missing)" false
    assert "T17f: --scope bogus emits a 'scope' diagnostic on stderr (skipped - run_all.sh missing)" false
    assert "T17g: bare --scope (no value) exits 64 (skipped - run_all.sh missing)" false
    assert "T17h: bare --scope (no value) emits a 'scope' diagnostic on stderr (skipped - run_all.sh missing)" false
fi

# -- Test 18: serial retry-once of failed pool members + FLAKY ledger ----------
# (deflake, esc-4959-53/56/57 lineage): proves tests/infra/run_all.sh
# self-heals a transient pool-bucket flake -- a pool member that fails on its
# first (concurrent-pool) attempt but passes on a single serial retry does
# NOT fail the run, is ledgered via a `=== FLAKY (passed on serial retry):`
# line, and both attempts are archived under the SAME discovered-order
# `--- Running: ---` header (so T9a's header-list assertion stays intact).
# Deterministic (fail-twice) pool failures and all-pass runs must NOT emit
# the FLAKY line -- the seam must still classify real failures via the bare
# `^FAILED ` marker (18b/18c reuse Test 9's/Test 11's already-captured output
# rather than paying for new concurrent-pool fixtures).
echo ""
echo "--- Test 18: serial retry-once + FLAKY ledger (deflake) ---"

if [ -f "$RUN_ALL" ] && [ -f "$LOAD_TOLERANCE_LIB_T9" ]; then
    TMPDIR_T18="$(mktemp -d)"
    _TMPDIRS+=("$TMPDIR_T18")

    # -- 18a: FLAKY direction -- fails invocation 1, passes invocation 2+ -----
    # Deterministic on tmpfile-counter STATE, not timing/host load (no
    # wall-clock, no host-baked constant): invocation 1 exits 1, invocation
    # 2+ exits 0.
    MANIFEST_T18A="$TMPDIR_T18/classification-flaky.manifest"
    printf 'test_flaky_pool.sh pool\n' > "$MANIFEST_T18A"

    cat > "$TMPDIR_T18/test_flaky_pool.sh" <<'MOCKBODY'
#!/usr/bin/env bash
set -euo pipefail
counter_file="$FLAKY_COUNTER_FILE"
count=$(( $(cat "$counter_file" 2>/dev/null || echo 0) + 1 ))
echo "$count" > "$counter_file"
echo "test_flaky_pool.sh invocation $count"
if [ "$count" -eq 1 ]; then
    exit 1
else
    exit 0
fi
MOCKBODY
    chmod +x "$TMPDIR_T18/test_flaky_pool.sh"

    t18a_rc=0
    t18a_out="$(RUN_ALL_CLASSIFICATION_MANIFEST="$MANIFEST_T18A" \
        REIFY_RUN_ALL_POOL_LOCK="$TMPDIR_T18/pool-flaky.lock" \
        REIFY_RUN_ALL_POOL_PSI_DISABLE=1 \
        FLAKY_COUNTER_FILE="$TMPDIR_T18/flaky-counter" \
        bash "$RUN_ALL" "$TMPDIR_T18" 2>&1)" || t18a_rc=$?

    assert "T18a: run_all.sh exits 0 when a pool member passes on serial retry" \
        test "$t18a_rc" -eq 0

    if [[ "$t18a_out" == *"=== FLAKY (passed on serial retry):"*"test_flaky_pool.sh"* ]]; then
        assert "T18a: FLAKY ledger line names test_flaky_pool.sh" true
    else
        assert "T18a: FLAKY ledger line names test_flaky_pool.sh (got: $t18a_out)" false
    fi

    if [[ "$t18a_out" == *"--- attempt 1 (concurrent pool) ---"* ]] && [[ "$t18a_out" == *"--- attempt 2 (serial retry) ---"* ]]; then
        assert "T18a: both attempt markers are present" true
    else
        assert "T18a: both attempt markers are present (got: $t18a_out)" false
    fi

    t18a_headers="$(echo "$t18a_out" | grep -E '^--- Running: ' | sed -E 's/^--- Running: (.*) ---$/\1/')" || true
    if [ "$t18a_headers" = "test_flaky_pool.sh" ]; then
        assert "T18a: exactly one discovered-order header for the retried member" true
    else
        assert "T18a: exactly one discovered-order header for the retried member (got: $t18a_headers)" false
    fi

    if ! echo "$t18a_out" | grep -qE '^FAILED[[:space:]]'; then
        assert "T18a: no ^FAILED classifier marker (flake is not misclassified as test_failure)" true
    else
        assert "T18a: no ^FAILED classifier marker (got: $t18a_out)" false
    fi

    if [[ "$t18a_out" == *"=== Summary: 1 discovered, 0 failed"* ]]; then
        assert "T18a: Summary line shows 0 failed" true
    else
        assert "T18a: Summary line shows 0 failed (got: $t18a_out)" false
    fi

    # Truthful summary: the flaky pass is neither reported as a hard failure
    # nor silently swallowed -- the Summary line itself carries the
    # flaky-retried COUNT (byte-exact), not just the separate FLAKY line.
    if [[ "$t18a_out" == *"=== Summary: 1 discovered, 0 failed, 1 flaky-retried ==="* ]]; then
        assert "T18a: Summary line carries the flaky-retried count (byte-exact)" true
    else
        assert "T18a: Summary line carries the flaky-retried count (byte-exact) (got: $t18a_out)" false
    fi

    # -- 18b: determinism direction (guard non-vacuity) -- a pool member that
    # fails BOTH attempts keeps the byte-identical FAILED contract and emits
    # NO FLAKY line. Reuses Test 9a's own output (test_pool_3.sh, exit 1) --
    # T9a already proves the full FAILED/Summary/exit-1 contract; this only
    # adds the two deflake-specific assertions against that same run, instead
    # of paying for a second concurrent-pool fixture.
    #
    # 18b/18c reuse t9a_out (Test 9a, captured above) and t11b_out (Test 11b,
    # captured above) rather than paying for fresh concurrent-pool fixtures --
    # an implicit execution-order coupling on top-level vars populated
    # earlier in this same shell process. Guard explicitly so a reorder or a
    # skipped upstream test surfaces as an explicit failure here, instead of
    # letting the negative-only checks below pass vacuously on an
    # empty/unset capture.
    if [ -n "${t9a_out:-}" ] && [ -n "${t11b_out:-}" ]; then
        assert "T18b/c: upstream captures t9a_out (Test 9a) and t11b_out (Test 11b) are present" true
    else
        assert "T18b/c: upstream captures t9a_out (Test 9a) and t11b_out (Test 11b) are present (t9a_out: $([ -n "${t9a_out:-}" ] && echo present || echo MISSING), t11b_out: $([ -n "${t11b_out:-}" ] && echo present || echo MISSING))" false
    fi

    if [[ "${t9a_out:-}" != *"=== FLAKY"* ]]; then
        assert "T18b: deterministic fail-twice (test_pool_3.sh) emits NO === FLAKY line" true
    else
        assert "T18b: deterministic fail-twice (test_pool_3.sh) emits NO === FLAKY line (got: ${t9a_out:-<unset>})" false
    fi

    if echo "${t9a_out:-}" | grep -qE '^FAILED[[:space:]].*test_pool_3\.sh'; then
        assert "T18b: deterministic fail-twice (test_pool_3.sh) still emits the ^FAILED classifier" true
    else
        assert "T18b: deterministic fail-twice (test_pool_3.sh) still emits the ^FAILED classifier (got: ${t9a_out:-<unset>})" false
    fi

    # Negative regression lock: the Summary augmentation is CONDITIONAL on
    # flaky_names being non-empty -- a deterministic fail-twice run must keep
    # the byte-exact legacy Summary line (no `flaky-retried` clause).
    if [[ "${t9a_out:-}" != *"flaky-retried"* ]]; then
        assert "T18b: deterministic fail-twice (test_pool_3.sh) Summary carries NO flaky-retried clause" true
    else
        assert "T18b: deterministic fail-twice (test_pool_3.sh) Summary carries NO flaky-retried clause (got: ${t9a_out:-<unset>})" false
    fi

    # -- 18c: all-pass direction -- no FLAKY line when nothing ever failed.
    # Reuses Test 11b's own output (fail-open PSI scenario, both pool mocks
    # exit 0) rather than a third concurrent-pool fixture.
    #
    # Non-vacuity anchor: pin t11b_out to a real Test-11b run (via the same
    # byte-exact Summary substring T11b itself asserts) before trusting the
    # negative-only FLAKY checks below -- otherwise an empty/missing capture
    # would make both negative assertions pass vacuously.
    if [[ "${t11b_out:-}" == *"=== Summary: 2 discovered, 0 failed ==="* ]]; then
        assert "T18c: reused output is a genuine Test-11b run (non-vacuity anchor)" true
    else
        assert "T18c: reused output is a genuine Test-11b run (non-vacuity anchor) (got: ${t11b_out:-<unset>})" false
    fi

    if [[ "${t11b_out:-}" != *"=== FLAKY"* ]]; then
        assert "T18c: all-pass run emits NO === FLAKY line" true
    else
        assert "T18c: all-pass run emits NO === FLAKY line (got: ${t11b_out:-<unset>})" false
    fi

    # Negative regression lock: an all-pass run must also keep the byte-exact
    # legacy Summary line (no `flaky-retried` clause).
    if [[ "${t11b_out:-}" != *"flaky-retried"* ]]; then
        assert "T18c: all-pass run Summary carries NO flaky-retried clause" true
    else
        assert "T18c: all-pass run Summary carries NO flaky-retried clause (got: ${t11b_out:-<unset>})" false
    fi
else
    assert "T18a: run_all.sh exits 0 when a pool member passes on serial retry (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T18a: FLAKY ledger line names test_flaky_pool.sh (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T18a: both attempt markers are present (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T18a: exactly one discovered-order header for the retried member (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T18a: no ^FAILED classifier marker (flake is not misclassified as test_failure) (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T18a: Summary line shows 0 failed (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T18a: Summary line carries the flaky-retried count (byte-exact) (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T18b/c: upstream captures t9a_out (Test 9a) and t11b_out (Test 11b) are present (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T18b: deterministic fail-twice (test_pool_3.sh) emits NO === FLAKY line (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T18b: deterministic fail-twice (test_pool_3.sh) still emits the ^FAILED classifier (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T18b: deterministic fail-twice (test_pool_3.sh) Summary carries NO flaky-retried clause (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T18c: reused output is a genuine Test-11b run (non-vacuity anchor) (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T18c: all-pass run emits NO === FLAKY line (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
    assert "T18c: all-pass run Summary carries NO flaky-retried clause (skipped - run_all.sh or load_tolerance_lib.sh missing)" false
fi

# -- Summary --------------------------------------------------------------------
test_summary
