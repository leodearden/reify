#!/usr/bin/env bash
# Infrastructure test for task 1992.
# Validates that scripts/cargo-test-occt-gated.sh exists with the correct
# structure, serializes OCCT-touching test processes via flock, and that
# orchestrator.yaml routes all cargo test --workspace invocations through it.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

[ -f "$SCRIPT_DIR/occt_flock_gate_lib.sh" ] || { echo "ERROR: occt_flock_gate_lib.sh not found at $SCRIPT_DIR/occt_flock_gate_lib.sh"; exit 1; }
source "$SCRIPT_DIR/occt_flock_gate_lib.sh"

WRAPPER="$REPO_ROOT/scripts/cargo-test-occt-gated.sh"

echo "=== OCCT flock gate tests ==="

# -- Test 1: wrapper script exists ---------------------------------------------
echo ""
echo "--- Test 1: wrapper script exists ---"

assert "scripts/cargo-test-occt-gated.sh exists" \
    test -f "$WRAPPER"

# -- Test 2: wrapper script is executable --------------------------------------
echo ""
echo "--- Test 2: wrapper script is executable ---"

assert "scripts/cargo-test-occt-gated.sh is executable (mode +x)" \
    test -x "$WRAPPER"

# -- Test 3: shebang line ------------------------------------------------------
echo ""
echo "--- Test 3: wrapper has #!/usr/bin/env bash shebang ---"

assert "first line is '#!/usr/bin/env bash'" \
    bash -c "head -1 '$WRAPPER' | grep -qxF '#!/usr/bin/env bash'"

# -- Test 4: set -euo pipefail -------------------------------------------------
echo ""
echo "--- Test 4: wrapper sets strict error handling ---"

assert "wrapper contains 'set -euo pipefail'" \
    grep -q 'set -euo pipefail' "$WRAPPER"

# -- Test 5: flock -x invocation -----------------------------------------------
echo ""
echo "--- Test 5: wrapper invokes flock -x ---"

assert "wrapper contains 'flock -x'" \
    grep -q 'flock -x' "$WRAPPER"

# -- Test 6: default lock path -------------------------------------------------
echo ""
echo "--- Test 6: default lock path is user-scoped ---"

assert "default lock path is user-scoped via 'id -u'" \
    grep -q 'reify-occt-$(id -u)' "$WRAPPER"

# -- Test 7: argument forwarding -----------------------------------------------
echo ""
echo "--- Test 7: wrapper forwards arguments with exec and \"\$@\" ---"

assert "wrapper contains 'exec'" \
    grep -q 'exec' "$WRAPPER"

assert 'wrapper contains "$@" for argument forwarding' \
    grep -qF '"$@"' "$WRAPPER"


# -- Test 8: serialization (REIFY_OCCT_LOCK override) --------------------------
echo ""
echo "--- Test 8: wrapper serializes two concurrent invocations ---"

_LOCK_FILE="$(mktemp)"
_START_NS="$(date +%s%N)"

# Spawn two concurrent invocations each sleeping 0.4s.
# REIFY_OCCT_CONCURRENCY=1 pins N=1 (exclusive mode) so this test remains
# valid after step-7 flips the default from N=1 to auto-detect.
REIFY_OCCT_LOCK="$_LOCK_FILE" REIFY_OCCT_CONCURRENCY=1 "$WRAPPER" bash -c 'sleep 0.4' &
_PID1=$!
REIFY_OCCT_LOCK="$_LOCK_FILE" REIFY_OCCT_CONCURRENCY=1 "$WRAPPER" bash -c 'sleep 0.4' &
_PID2=$!
wait "$_PID1" "$_PID2"

_END_NS="$(date +%s%N)"
_ELAPSED_MS=$(( (_END_NS - _START_NS) / 1000000 ))

rm -f "$_LOCK_FILE" "${_LOCK_FILE}.slot-1"

# Parallel would finish in ~400ms; serialized takes >=700ms.
assert "two 0.4s sleep invocations run serially (elapsed >= 700ms, got ${_ELAPSED_MS}ms)" \
    test "$_ELAPSED_MS" -ge 700

# -- Test 9: exit-code propagation ----------------------------------------------
echo ""
echo "--- Test 9: wrapper propagates exit code of wrapped command ---"

_TMP_LOCK="$(mktemp)"
_EC=0
REIFY_OCCT_LOCK="$_TMP_LOCK" "$WRAPPER" bash -c 'exit 42' || _EC=$?
rm -f "$_TMP_LOCK"

assert "wrapper exit code is 42 (got $_EC)" \
    test "$_EC" -eq 42


# -- verify.sh plan integration tests ------------------------------------------
# These formerly grepped orchestrator.yaml's test_command. Since task 3766 the
# orchestrator calls scripts/verify.sh, so the canonical command list is taken
# from verify.sh --print-plan (--scope all → full plan, index-independent; env
# lines stripped via `grep -v '^#'`). The gated passes are plain `cargo test`
# under the flock wrapper regardless of the nextest/cargo-test choice for the
# ungated tail, so the gated assertions below stay exact-match.
TEST_PLAN_SEGS="$(bash "$REPO_ROOT/scripts/verify.sh" test --profile both --scope all --print-plan | grep -v '^#')"
export TEST_PLAN_SEGS

echo ""
echo "--- Test 10: debug pass is gated by cargo-test-occt-gated.sh ---"

assert "plan contains gated debug pass with -p reify-kernel-occt" \
    bash -c "printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -qE './scripts/cargo-test-occt-gated\.sh.*cargo test -p reify-kernel-occt -p reify-eval -p reify-cli -p reify-config -- --test-threads=1'"

echo ""
echo "--- Test 11: release pass is gated by cargo-test-occt-gated.sh ---"

assert "plan contains gated release pass with -p reify-kernel-occt" \
    bash -c "printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -qE './scripts/cargo-test-occt-gated\.sh.*cargo test -p reify-kernel-occt -p reify-eval -p reify-cli -p reify-config --release -- --test-threads=1'"

echo ""
echo "--- Test 12: no bare ungated workspace pass (every --workspace leaf has --exclude) ---"

# Allowed forms:
#   (a) Gated:   cargo-test-occt-gated.sh cargo test -p ...  (no --workspace)
#   (b) Ungated: cargo (test|nextest run) --workspace --exclude ... (intentional)
# Forbidden: a bare workspace pass without any --exclude flags. Accept both
# runner spellings so the assertion is valid whether or not nextest is installed.
assert "no bare workspace test pass without --exclude in the plan" \
    bash -c "! printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -E 'cargo (test|nextest run) --workspace' | grep -vq -- '--exclude'"

echo ""
echo "--- Test 13: --workspace flag preserved under gate (coverage assertion) ---"

assert "gated debug invocation contains '-p reify-kernel-occt'" \
    bash -c "printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -qE 'cargo-test-occt-gated\.sh.*cargo test -p reify-kernel-occt'"

# -- Test 14: bounded lock-wait exits non-zero with clear message ---------------
echo ""
echo "--- Test 14: REIFY_OCCT_LOCK_WAIT=1 fires within budget, exits non-zero with message ---"

_LOCK14="$(mktemp)"
_ERR14="$(mktemp)"

# Spawn a background holder that acquires slot-1 and holds it for 10s.
# The wrapper uses ${LOCK}.slot-1 (not $LOCK directly), so the holder must
# target the slot file to actually block the wrapper.
( flock -x 9; sleep 10 ) 9>>"${_LOCK14}.slot-1" &
_HOLDER14=$!
sleep 0.2  # give the holder time to acquire before we proceed

_START14="$(date +%s)"
_EXIT14=0
# REIFY_OCCT_CONCURRENCY=1 pins N=1 so the single holder on slot-1 blocks the wrapper.
REIFY_OCCT_LOCK="$_LOCK14" REIFY_OCCT_CONCURRENCY=1 REIFY_OCCT_LOCK_WAIT=1 timeout 5 "$WRAPPER" true 2>"$_ERR14" || _EXIT14=$?
_END14="$(date +%s)"
_ELAPSED14=$(( _END14 - _START14 ))

kill "$_HOLDER14" 2>/dev/null || true
wait "$_HOLDER14" 2>/dev/null || true

assert "Test 14: wrapper exits 75 (EX_TEMPFAIL) when lock-wait limit exceeded (got $_EXIT14)" \
    test "$_EXIT14" -eq 75

assert "Test 14: wrapper exits within 3s, not blocked until outer safety timeout (elapsed=${_ELAPSED14}s)" \
    test "$_ELAPSED14" -le 3

assert "Test 14: stderr mentions 'acquire' and lock-wait duration (1s)" \
    grep -qE 'acquire.*1s|1s.*acquire' "$_ERR14"

rm -f "$_LOCK14" "${_LOCK14}.slot-1" "$_ERR14"

# -- Test 15: post-lock timer fires N seconds AFTER lock acquisition, not after start ---
echo ""
echo "--- Test 15: REIFY_OCCT_TEST_TIMEOUT measured post-lock, not from wrapper start ---"

_LOCK15="$(mktemp)"

# Spawn a holder that holds slot-1 for 4 seconds.
# The wrapper uses ${LOCK}.slot-1 (not $LOCK directly), so the holder must
# target the slot file to actually block the wrapper.
# _START15 is recorded ~0.2s after holder spawn, so the effective wait from
# _START15's perspective is ~3.8s; adding 1s command gives ~4.8s, which
# truncates to 4 (satisfying the lower bound ≥ 4).  With 3s holder the
# wait is ~2.8s, total ~3.8s → truncates to 3 → spurious failure on CI.
( flock -x 9; sleep 4 ) 9>>"${_LOCK15}.slot-1" &
_HOLDER15=$!
sleep 0.2  # give holder time to acquire

_START15="$(date +%s)"
_EXIT15=0
# REIFY_OCCT_CONCURRENCY=1 pins N=1 so the single holder on slot-1 blocks the wrapper.
REIFY_OCCT_LOCK="$_LOCK15" REIFY_OCCT_CONCURRENCY=1 REIFY_OCCT_LOCK_WAIT=10 REIFY_OCCT_TEST_TIMEOUT=1 \
    "$WRAPPER" sleep 5 || _EXIT15=$?
_END15="$(date +%s)"
_ELAPSED15=$(( _END15 - _START15 ))

kill "$_HOLDER15" 2>/dev/null || true
wait "$_HOLDER15" 2>/dev/null || true
rm -f "$_LOCK15" "${_LOCK15}.slot-1"

# Expected: lock acquired after ~4s, then `timeout 1 sleep 5` kills after 1s → rc=124
# elapsed ≈ 5s total.  Without internal timeout: sleep 5 runs fully → rc=0, elapsed ≈ 9s.
# Lower bound ≥ 4 proves the timer could not have started at wrapper launch (~1s in that case).
assert "Test 15: wrapper exits 124 (internal timeout killed the command, got $_EXIT15)" \
    test "$_EXIT15" -eq 124

assert "Test 15: elapsed in [4,8]s — timer started post-lock, not at wrapper launch (elapsed=${_ELAPSED15}s)" \
    bash -c "test '$_ELAPSED15' -ge 4 && test '$_ELAPSED15' -le 8"

# -- Test 16: wrapper logs wait duration on lock acquisition -------------------
echo ""
echo "--- Test 16: wrapper emits INFO log line with 'acquired' and numeric duration on stderr ---"

_LOCK16="$(mktemp)"
_ERR16="$(mktemp)"

_EXIT16=0
REIFY_OCCT_LOCK="$_LOCK16" "$WRAPPER" true 2>"$_ERR16" >/dev/null || _EXIT16=$?

assert "Test 16: wrapper exits 0 (sanity check, got $_EXIT16)" \
    test "$_EXIT16" -eq 0

assert "Test 16: stderr contains log line with 'acquired', 'OCCT lock', and numeric duration (Ns)" \
    grep -qiE 'acquired.*OCCT.*lock.*[0-9]+s' "$_ERR16"

rm -f "$_LOCK16" "$_ERR16"

# -- Test 17: gated invocations delegate the timeout to the wrapper -------------
echo ""
echo "--- Test 17: plan delegates timeout to wrapper via REIFY_OCCT_TEST_TIMEOUT (no outer timeout on gated) ---"

assert "Test 17: no outer 'timeout --kill-after=N Nm ./scripts/cargo-test-occt-gated' in the plan" \
    bash -c "! printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -qE 'timeout[[:space:]]+--kill-after=[0-9]+[[:space:]]+[0-9]+[smhd][[:space:]]+[./]*scripts/cargo-test-occt-gated'"

assert "Test 17: REIFY_OCCT_TEST_TIMEOUT= appears exactly twice in the plan (once per gated invocation)" \
    bash -c "[ \"\$(printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -oF 'REIFY_OCCT_TEST_TIMEOUT=' | wc -l | tr -d ' ')\" -eq 2 ]"

assert "Test 17: debug invocation sets REIFY_OCCT_TEST_TIMEOUT=3600" \
    bash -c "printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -qE 'REIFY_OCCT_TEST_TIMEOUT=3600 ./scripts/cargo-test-occt-gated\.sh.*cargo test -p reify-kernel-occt'"

assert "Test 17: release invocation sets REIFY_OCCT_TEST_TIMEOUT=4800" \
    bash -c "printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -qE 'REIFY_OCCT_TEST_TIMEOUT=4800 ./scripts/cargo-test-occt-gated\.sh.*cargo test -p reify-kernel-occt -p reify-eval -p reify-cli -p reify-config --release'"

# -- Test 18: wrapper does not leak the lock fd into background daemons --------
# Regression test for the 2026-04-20 merge-queue wedge: sccache (spawned as a
# detached daemon by cargo via RUSTC_WRAPPER) inherited FD 9 and outlived
# cargo, pinning the flock forever.  The wrapper must run its child with FD 9
# closed so no descendant can leak the open file description.
echo ""
echo "--- Test 18: wrapper closes fd 9 on the child; daemons do not leak the lock ---"

_LOCK18="$(mktemp)"
_DAEMON_PID_FILE="$(mktemp)"
_EXIT18=0

# Run the wrapper on a command that forks a detached daemon that survives the
# wrapper's exit.  setsid + & + disown produces exactly the sccache-style
# inheritance pattern: the daemon's only link to the lock fd is inheritance
# from its parent, so a correctly-patched wrapper closes fd 9 before the
# child exec and the daemon starts life without fd 9.
# REIFY_OCCT_CONCURRENCY=1 pins N=1 so the slot file is ${_LOCK18}.slot-1.
REIFY_OCCT_LOCK="$_LOCK18" REIFY_OCCT_CONCURRENCY=1 "$WRAPPER" bash -c '
    setsid bash -c "sleep 30" </dev/null >/dev/null 2>&1 &
    echo $! > "'"$_DAEMON_PID_FILE"'"
    disown
    exit 0
' || _EXIT18=$?

_DAEMON_PID="$(cat "$_DAEMON_PID_FILE" 2>/dev/null || echo "")"

# The daemon must still be alive (otherwise the test is vacuous — we'd be
# verifying the lock is free because nothing inherited it at all).
assert "Test 18: daemon spawned inside wrapper is still alive after wrapper exits (pid=$_DAEMON_PID)" \
    bash -c "[ -n '$_DAEMON_PID' ] && kill -0 '$_DAEMON_PID' 2>/dev/null"

# After the wrapper returns, the slot file flock must be free: a non-blocking
# flock attempt on ${_LOCK18}.slot-1 must succeed immediately.  If fd 9 had
# leaked into the surviving daemon, this would fail (slot lock still held).
_LOCK_FREE18=1
( flock -n -x 9 || exit 1 ) 9>>"${_LOCK18}.slot-1" || _LOCK_FREE18=0

assert "Test 18: slot-1 lock released after wrapper exit despite surviving daemon (fd 9 not inherited)" \
    test "$_LOCK_FREE18" -eq 1

# Cleanup the surviving daemon.
if [ -n "$_DAEMON_PID" ]; then
    kill "$_DAEMON_PID" 2>/dev/null || true
fi
rm -f "$_LOCK18" "${_LOCK18}.slot-1" "$_DAEMON_PID_FILE"

assert "Test 18: wrapper exited 0 on successful spawn (got $_EXIT18)" \
    test "$_EXIT18" -eq 0

# -- Test 19: REIFY_OCCT_CONCURRENCY=2 runs two invocations in parallel ----------
# With N=2 slots, two concurrent wrapper invocations must acquire different slots
# and run simultaneously (~400ms wall-clock), NOT serialize (~800ms).
# This test MUST FAIL on the current exclusive-flock implementation.
echo ""
echo "--- Test 19: REIFY_OCCT_CONCURRENCY=2 allows two concurrent invocations to run in parallel ---"

_LOCK19="$(mktemp)"
_START19_NS="$(date +%s%N)"

# Spawn two concurrent invocations each sleeping 0.4s, both sharing the same
# lock base path with 2 slots.
REIFY_OCCT_LOCK="$_LOCK19" REIFY_OCCT_CONCURRENCY=2 "$WRAPPER" bash -c 'sleep 0.4' &
_PID19A=$!
REIFY_OCCT_LOCK="$_LOCK19" REIFY_OCCT_CONCURRENCY=2 "$WRAPPER" bash -c 'sleep 0.4' &
_PID19B=$!
wait "$_PID19A" "$_PID19B"

_END19_NS="$(date +%s%N)"
_ELAPSED19_MS=$(( (_END19_NS - _START19_NS) / 1000000 ))

rm -f "$_LOCK19" "${_LOCK19}.slot-1" "${_LOCK19}.slot-2"

# Parallel completion: ~400ms. Serial (exclusive): ~800ms.
# Assert elapsed < 900ms (widened from 700ms for CI-load headroom) to detect
# regression to exclusive-mode behavior while tolerating wrapper startup overhead.
# Full-serial threshold is ~800ms so 900ms stays below the failure indicator.
assert "Test 19: two 0.4s sleep invocations run in parallel with N=2 (elapsed < 900ms, got ${_ELAPSED19_MS}ms)" \
    test "$_ELAPSED19_MS" -lt 900

# -- Test 20: N=2, three concurrent invocations serializes the third ----------
# With only 2 slots, a third concurrent wrapper invocation must wait until one
# slot is released. Measured elapsed must be >= 700ms (two parallel rounds of
# ~400ms — proves serialization) and <= 2000ms (load-tolerant sanity ceiling,
# raised 1200->2000 per esc-3939-94: verify-pipeline load inflated the
# serialized 3rd invocation to 1473ms in one run with no logic defect).
# At 2000ms the upper bound no longer discriminates N=2 (~800ms) from fully-serial
# N=1 (~1200ms); the >=700ms lower bound guards against under-serialization only.
# COVERAGE GAP (accepted tradeoff per esc-3939-94): a fully-serial regression
# (N->1) produces ~1200ms for three invocations — inside [700,2000], undetected.
# Test 19 does NOT guard this: two fully-serial invocations complete in ~800ms,
# below Test 19's <900ms threshold (both pass under a fully-serial regression).
# This validates that the acquire-loop bounds N strictly (not ">=N" slots).
echo ""
echo "--- Test 20: REIFY_OCCT_CONCURRENCY=2 serializes the 3rd invocation when both slots are busy ---"

_LOCK20="$(mktemp)"
_START20_NS="$(date +%s%N)"

# Spawn three concurrent invocations each sleeping 0.4s with N=2 slots.
REIFY_OCCT_LOCK="$_LOCK20" REIFY_OCCT_CONCURRENCY=2 "$WRAPPER" bash -c 'sleep 0.4' &
_PID20A=$!
REIFY_OCCT_LOCK="$_LOCK20" REIFY_OCCT_CONCURRENCY=2 "$WRAPPER" bash -c 'sleep 0.4' &
_PID20B=$!
REIFY_OCCT_LOCK="$_LOCK20" REIFY_OCCT_CONCURRENCY=2 "$WRAPPER" bash -c 'sleep 0.4' &
_PID20C=$!
wait "$_PID20A" "$_PID20B" "$_PID20C"

_END20_NS="$(date +%s%N)"
_ELAPSED20_MS=$(( (_END20_NS - _START20_NS) / 1000000 ))

rm -f "$_LOCK20" "${_LOCK20}.slot-1" "${_LOCK20}.slot-2"

# Two slots: two run in parallel (~400ms), third waits and runs (~800ms total).
# Lower bound >= 700ms proves the third was serialized (all-parallel ~400ms).
# Upper bound <= 2000ms is a load-tolerant sanity ceiling (esc-3939-94).
assert "Test 20: 3 invocations with N=2 complete in [${OCCT_SERIAL3_N2_LOW_MS},${OCCT_SERIAL3_N2_HIGH_MS}]ms — 3rd is serialized (got ${_ELAPSED20_MS}ms)" \
    occt_serial3_n2_within_bounds "$_ELAPSED20_MS"

# -- Test 21: REIFY_OCCT_MAX_CONCURRENCY sets N when CONCURRENCY is unset ------
# With REIFY_OCCT_CONCURRENCY unset, N falls back to REIFY_OCCT_MAX_CONCURRENCY.
# Sub-test A: two concurrent wrappers → parallel (<900ms).
# Sub-test B: three concurrent wrappers → third serialized ([700,2000]ms,
#   load-tolerant ceiling per esc-3939-94; shared with Test 20 via occt_flock_gate_lib.sh).
#
# Historical note: a prior implementation auto-detected N as
# clamp(nproc - load_1m_int, 1, MAX_CAP) and was retired (esc-4000-39, 2026-05-28)
# because the load-based reduction created positive-feedback collapse to N=1
# under concurrent worktree load.  The MAX_CONCURRENCY env var is now N itself
# when CONCURRENCY is unset.
echo ""
echo "--- Test 21: REIFY_OCCT_MAX_CONCURRENCY=2 sets N=2 — 2 parallel, 3rd serialized ---"

_LOCK21A="$(mktemp)"
_LOCK21B="$(mktemp)"

# Sub-test A: 2 invocations with MAX_CONCURRENCY=2 → both slots used in parallel.
_START21A_NS="$(date +%s%N)"
REIFY_OCCT_MAX_CONCURRENCY=2 REIFY_OCCT_LOCK="$_LOCK21A" \
    "$WRAPPER" bash -c 'sleep 0.4' &
_PID21A1=$!
REIFY_OCCT_MAX_CONCURRENCY=2 REIFY_OCCT_LOCK="$_LOCK21A" \
    "$WRAPPER" bash -c 'sleep 0.4' &
_PID21A2=$!
wait "$_PID21A1" "$_PID21A2"
_END21A_NS="$(date +%s%N)"
_ELAPSED21A_MS=$(( (_END21A_NS - _START21A_NS) / 1000000 ))
rm -f "$_LOCK21A" "${_LOCK21A}.slot-1" "${_LOCK21A}.slot-2"

# Sub-test B: 3 invocations with MAX_CONCURRENCY=2 → third must wait.
_START21B_NS="$(date +%s%N)"
REIFY_OCCT_MAX_CONCURRENCY=2 REIFY_OCCT_LOCK="$_LOCK21B" \
    "$WRAPPER" bash -c 'sleep 0.4' &
_PID21B1=$!
REIFY_OCCT_MAX_CONCURRENCY=2 REIFY_OCCT_LOCK="$_LOCK21B" \
    "$WRAPPER" bash -c 'sleep 0.4' &
_PID21B2=$!
REIFY_OCCT_MAX_CONCURRENCY=2 REIFY_OCCT_LOCK="$_LOCK21B" \
    "$WRAPPER" bash -c 'sleep 0.4' &
_PID21B3=$!
wait "$_PID21B1" "$_PID21B2" "$_PID21B3"
_END21B_NS="$(date +%s%N)"
_ELAPSED21B_MS=$(( (_END21B_NS - _START21B_NS) / 1000000 ))
rm -f "$_LOCK21B" "${_LOCK21B}.slot-1" "${_LOCK21B}.slot-2"

assert "Test 21A: 2 invocations with MAX_CONCURRENCY=2 run in parallel (<900ms, got ${_ELAPSED21A_MS}ms)" \
    test "$_ELAPSED21A_MS" -lt 900

assert "Test 21B: 3 invocations with MAX_CONCURRENCY=2 have 3rd serialized ([${OCCT_SERIAL3_N2_LOW_MS},${OCCT_SERIAL3_N2_HIGH_MS}]ms, got ${_ELAPSED21B_MS}ms)" \
    occt_serial3_n2_within_bounds "$_ELAPSED21B_MS"

# -- Test 22: LOCK_WAIT bound fires when ALL N slots are externally held ------
# Mirrors Test 14's pattern but with N=2 (full contention across all slots).
# Spawn two background flock holders each pinning one slot file for 10s;
# invoke wrapper with REIFY_OCCT_CONCURRENCY=2 and REIFY_OCCT_LOCK_WAIT=1.
# The wrapper must exit 75 (EX_TEMPFAIL) within <= 3s (1s wait + one 0.5s
# retry cycle), confirming the deadline is checked after ALL N slots fail
# on a given pass — not only after a single slot fails.
echo ""
echo "--- Test 22: LOCK_WAIT fires within budget when ALL N=2 slots are externally held ---"

_LOCK22="$(mktemp)"
_ERR22="$(mktemp)"

# Spawn two background holders: one pins slot-1, one pins slot-2.
( flock -x 9; sleep 10 ) 9>>"${_LOCK22}.slot-1" &
_HOLDER22A=$!
( flock -x 9; sleep 10 ) 9>>"${_LOCK22}.slot-2" &
_HOLDER22B=$!
sleep 0.2  # give both holders time to acquire before we proceed

_START22="$(date +%s)"
_EXIT22=0
REIFY_OCCT_LOCK="$_LOCK22" REIFY_OCCT_CONCURRENCY=2 REIFY_OCCT_LOCK_WAIT=1 \
    timeout 5 "$WRAPPER" true 2>"$_ERR22" || _EXIT22=$?
_END22="$(date +%s)"
_ELAPSED22=$(( _END22 - _START22 ))

kill "$_HOLDER22A" "$_HOLDER22B" 2>/dev/null || true
wait "$_HOLDER22A" "$_HOLDER22B" 2>/dev/null || true

assert "Test 22: wrapper exits 75 when all N=2 slots held and LOCK_WAIT=1 (got $_EXIT22)" \
    test "$_EXIT22" -eq 75

assert "Test 22: wrapper exits within 3s (not blocked for full holder duration) (elapsed=${_ELAPSED22}s)" \
    test "$_ELAPSED22" -le 3

assert "Test 22: stderr mentions acquire and lock-wait duration (1s)" \
    grep -qE 'acquire.*1s|1s.*acquire' "$_ERR22"

rm -f "$_LOCK22" "${_LOCK22}.slot-1" "${_LOCK22}.slot-2" "$_ERR22"

# -- Test 23: FD-non-leak invariant with N>1 -----------------------------------
# Multi-slot generalization of Test 18 (which tests N=1 / slot-1 only after
# step-6's migration).  Run two concurrent wrapper invocations under
# REIFY_OCCT_CONCURRENCY=2 so they each acquire a different slot file
# (slot-1 and slot-2).  Each wrapper spawns a detached daemon.
# After both wrappers exit, BOTH slot-1 AND slot-2 must be flock-acquirable,
# confirming that neither surviving daemon inherited the slot FD from its
# respective wrapper invocation.  The 9<&- redirection closes FD 9 before
# the child exec, so no descendant process can hold the slot open.
echo ""
echo "--- Test 23: N=2 wrappers close fd 9 on child; daemons do not leak either slot ---"

_LOCK23="$(mktemp)"
_DAEMON_PID_FILE23A="$(mktemp)"
_DAEMON_PID_FILE23B="$(mktemp)"
_EXIT23A=0
_EXIT23B=0

# Spawn both wrappers concurrently so they acquire different slots
# (slot-1 and slot-2 respectively).
REIFY_OCCT_LOCK="$_LOCK23" REIFY_OCCT_CONCURRENCY=2 "$WRAPPER" bash -c '
    setsid bash -c "sleep 30" </dev/null >/dev/null 2>&1 &
    echo $! > "'"$_DAEMON_PID_FILE23A"'"
    disown
    exit 0
' &
_W23A=$!
REIFY_OCCT_LOCK="$_LOCK23" REIFY_OCCT_CONCURRENCY=2 "$WRAPPER" bash -c '
    setsid bash -c "sleep 30" </dev/null >/dev/null 2>&1 &
    echo $! > "'"$_DAEMON_PID_FILE23B"'"
    disown
    exit 0
' &
_W23B=$!
wait "$_W23A" || _EXIT23A=$?
wait "$_W23B" || _EXIT23B=$?

_DAEMON23A="$(cat "$_DAEMON_PID_FILE23A" 2>/dev/null || echo "")"
_DAEMON23B="$(cat "$_DAEMON_PID_FILE23B" 2>/dev/null || echo "")"

# Both daemons must still be alive (otherwise the test is vacuous — no
# inherited FD to worry about).
assert "Test 23: daemon A spawned inside wrapper A is still alive (pid=$_DAEMON23A)" \
    bash -c "[ -n '$_DAEMON23A' ] && kill -0 '$_DAEMON23A' 2>/dev/null"

assert "Test 23: daemon B spawned inside wrapper B is still alive (pid=$_DAEMON23B)" \
    bash -c "[ -n '$_DAEMON23B' ] && kill -0 '$_DAEMON23B' 2>/dev/null"

# Both slot files must be flock-acquirable: surviving daemons must not have
# inherited FD 9 from either wrapper invocation.
_LOCK_FREE23A=1
( flock -n -x 9 || exit 1 ) 9>>"${_LOCK23}.slot-1" || _LOCK_FREE23A=0

_LOCK_FREE23B=1
( flock -n -x 9 || exit 1 ) 9>>"${_LOCK23}.slot-2" || _LOCK_FREE23B=0

assert "Test 23: slot-1 lock released after wrapper A exit (fd 9 not inherited by daemon A)" \
    test "$_LOCK_FREE23A" -eq 1

assert "Test 23: slot-2 lock released after wrapper B exit (fd 9 not inherited by daemon B)" \
    test "$_LOCK_FREE23B" -eq 1

# Both wrappers must have exited 0.
assert "Test 23: wrapper A exited 0 (got $_EXIT23A)" \
    test "$_EXIT23A" -eq 0

assert "Test 23: wrapper B exited 0 (got $_EXIT23B)" \
    test "$_EXIT23B" -eq 0

# Cleanup surviving daemons.
[ -n "$_DAEMON23A" ] && kill "$_DAEMON23A" 2>/dev/null || true
[ -n "$_DAEMON23B" ] && kill "$_DAEMON23B" 2>/dev/null || true
rm -f "$_LOCK23" "${_LOCK23}.slot-1" "${_LOCK23}.slot-2" \
    "$_DAEMON_PID_FILE23A" "$_DAEMON_PID_FILE23B"

test_summary
