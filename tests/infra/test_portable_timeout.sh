#!/usr/bin/env bash
# Unit tests for portable_timeout() from scripts/lib_portable.sh.
# Tests that the function runs commands with a timeout, using the 3-tier
# strategy: GNU timeout -> gtimeout -> background+sleep+kill fallback.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
LIB_PORTABLE="$REPO_ROOT/scripts/lib_portable.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== portable_timeout unit tests ==="

# -- Test 1: portable_timeout is defined after sourcing -----------------------
echo ""
echo "--- Test 1: portable_timeout function defined ---"

assert "portable_timeout function is defined after sourcing" \
    bash -c "source '$LIB_PORTABLE' && declare -f portable_timeout >/dev/null"

# -- Test 2: runs a fast command successfully ---------------------------------
echo ""
echo "--- Test 2: fast command succeeds ---"

assert "portable_timeout 5 true returns 0" \
    bash -c "source '$LIB_PORTABLE' && portable_timeout 5 true"

# -- Test 3: returns non-zero for a failing command ---------------------------
echo ""
echo "--- Test 3: failing command returns non-zero ---"

assert "portable_timeout 5 false returns non-zero" \
    bash -c "source '$LIB_PORTABLE' && ! portable_timeout 5 false"

# -- Test 4: returns 124 when command exceeds timeout -------------------------
echo ""
echo "--- Test 4: timeout returns exit code 124 ---"

assert "portable_timeout 1 sleep 10 returns 124" \
    bash -c "source '$LIB_PORTABLE' && rc=0; portable_timeout 1 sleep 10 || rc=\$?; [ \"\$rc\" -eq 124 ]"

# -- Test 5: preserves command exit code for non-timeout failures -------------
echo ""
echo "--- Test 5: preserves command exit code ---"

# bash -c 'exit 42' should give exit code 42, not 124.
assert "portable_timeout preserves non-timeout exit code" \
    bash -c "source '$LIB_PORTABLE' && rc=0; portable_timeout 5 bash -c 'exit 42' || rc=\$?; [ \"\$rc\" -eq 42 ]"

# -- Test 6: runs a command that produces output ------------------------------
echo ""
echo "--- Test 6: command output is passed through ---"

assert "portable_timeout passes through stdout" \
    bash -c "source '$LIB_PORTABLE' && out=\$(portable_timeout 5 echo hello) && [ \"\$out\" = 'hello' ]"

# -- Test 7: _PORTABLE_TIMEOUT_TIMED_OUT is true after genuine timeout --------
echo ""
echo "--- Test 7: _PORTABLE_TIMEOUT_TIMED_OUT set to true on genuine timeout ---"

assert "_PORTABLE_TIMEOUT_TIMED_OUT is true after genuine timeout" \
    bash -c "source '$LIB_PORTABLE' && portable_timeout 1 sleep 10 || true; [ \"\$_PORTABLE_TIMEOUT_TIMED_OUT\" = 'true' ]"

# -- Test 8: _PORTABLE_TIMEOUT_TIMED_OUT is false for natural exit 124 --------
echo ""
echo "--- Test 8: _PORTABLE_TIMEOUT_TIMED_OUT false on natural exit 124 ---"

# Force POSIX fallback by creating a temp dir with only essential binaries,
# excluding timeout/gtimeout. This is the path where the core misdiagnosis
# occurs (GNU timeout has the same ambiguity, documented limitation).
assert "_PORTABLE_TIMEOUT_TIMED_OUT is false when command exits 124 naturally (POSIX fallback)" \
    env LIB_PORTABLE="$LIB_PORTABLE" bash -c '
        # Build a PATH that excludes timeout and gtimeout.
        # Remove each directory that contains these binaries.
        new_path=""
        IFS=: read -ra dirs <<< "$PATH"
        for d in "${dirs[@]}"; do
            if [ -x "$d/timeout" ] || [ -x "$d/gtimeout" ]; then
                continue
            fi
            new_path="${new_path:+$new_path:}$d"
        done
        export PATH="$new_path"
        hash -r
        source "$LIB_PORTABLE"
        portable_timeout 5 bash -c "exit 124" || true
        [ "$_PORTABLE_TIMEOUT_TIMED_OUT" = "false" ]
    '

# -- Test 9: _PORTABLE_TIMEOUT_TIMED_OUT is false for success and failure -----
echo ""
echo "--- Test 9: _PORTABLE_TIMEOUT_TIMED_OUT false on success and normal failure ---"

assert "_PORTABLE_TIMEOUT_TIMED_OUT is false when command succeeds" \
    bash -c "source '$LIB_PORTABLE' && portable_timeout 5 true; [ \"\$_PORTABLE_TIMEOUT_TIMED_OUT\" = 'false' ]"

assert "_PORTABLE_TIMEOUT_TIMED_OUT is false when command fails normally" \
    bash -c "source '$LIB_PORTABLE' && portable_timeout 5 false || true; [ \"\$_PORTABLE_TIMEOUT_TIMED_OUT\" = 'false' ]"

# -- Test 10: mktemp failure + genuine timeout still enforced -----------------
echo ""
echo "--- Test 10: mktemp failure + genuine timeout still enforced ---"

# Force POSIX fallback (exclude timeout/gtimeout from PATH) AND force mktemp
# failure (TMPDIR=/dev/null/nope). The timeout should still fire, kill the
# command, and report _PORTABLE_TIMEOUT_TIMED_OUT=true.
#
# Since timeout and sleep may live in the same directory (/usr/bin), we create
# a rescue directory with symlinks to essential commands before excluding
# directories that contain timeout/gtimeout. Set TMPDIR only AFTER creating
# the rescue dir (so mktemp -d still works for test setup).

# Helper: set up POSIX fallback + broken mktemp environment.
# Exports: PATH (no timeout/gtimeout, with rescue), TMPDIR (broken), LIB_PORTABLE.
# Usage: eval "$MKTEMP_FAIL_SETUP"
MKTEMP_FAIL_SETUP='
    rescue_dir=$(mktemp -d)
    for cmd in sleep kill grep rm mktemp ln; do
        p=$(command -v "$cmd" 2>/dev/null) && ln -sf "$p" "$rescue_dir/$cmd"
    done
    trap "rm -rf $rescue_dir" EXIT
    new_path="$rescue_dir"
    IFS=: read -ra dirs <<< "$PATH"
    for d in "${dirs[@]}"; do
        if [ -x "$d/timeout" ] || [ -x "$d/gtimeout" ]; then
            continue
        fi
        new_path="${new_path:+$new_path:}$d"
    done
    export PATH="$new_path"
    export TMPDIR=/dev/null/nope
    hash -r
    source "$LIB_PORTABLE"
'

assert "mktemp failure: timeout still enforced (exit 124)" \
    env LIB_PORTABLE="$LIB_PORTABLE" MKTEMP_FAIL_SETUP="$MKTEMP_FAIL_SETUP" bash -c '
        eval "$MKTEMP_FAIL_SETUP"
        rc=0; portable_timeout 1 sleep 10 || rc=$?
        [ "$rc" -eq 124 ]
    '

assert "mktemp failure: _PORTABLE_TIMEOUT_TIMED_OUT is true" \
    env LIB_PORTABLE="$LIB_PORTABLE" MKTEMP_FAIL_SETUP="$MKTEMP_FAIL_SETUP" bash -c '
        eval "$MKTEMP_FAIL_SETUP"
        portable_timeout 1 sleep 10 || true
        [ "$_PORTABLE_TIMEOUT_TIMED_OUT" = "true" ]
    '

assert "mktemp failure: stderr contains warning" \
    env LIB_PORTABLE="$LIB_PORTABLE" MKTEMP_FAIL_SETUP="$MKTEMP_FAIL_SETUP" bash -c '
        eval "$MKTEMP_FAIL_SETUP"
        err=$(portable_timeout 1 sleep 10 2>&1 >/dev/null || true)
        echo "$err" | grep -q "mktemp failed"
    '

# -- Test 11: mktemp failure + command succeeds before timeout ----------------
echo ""
echo "--- Test 11: mktemp failure + command succeeds normally ---"

# Force POSIX fallback + mktemp failure. Run a fast command that succeeds.
# Exit code should be 0 and _PORTABLE_TIMEOUT_TIMED_OUT should be false.
assert "mktemp failure: fast command exit code preserved (0)" \
    env LIB_PORTABLE="$LIB_PORTABLE" MKTEMP_FAIL_SETUP="$MKTEMP_FAIL_SETUP" bash -c '
        eval "$MKTEMP_FAIL_SETUP"
        portable_timeout 5 true
    '

assert "mktemp failure: _PORTABLE_TIMEOUT_TIMED_OUT is false on success" \
    env LIB_PORTABLE="$LIB_PORTABLE" MKTEMP_FAIL_SETUP="$MKTEMP_FAIL_SETUP" bash -c '
        eval "$MKTEMP_FAIL_SETUP"
        portable_timeout 5 true
        [ "$_PORTABLE_TIMEOUT_TIMED_OUT" = "false" ]
    '

# -- Test 12: POSIX flag-file genuine timeout (happy path) --------------------
echo ""
echo "--- Test 12: POSIX flag-file genuine timeout (happy path) ---"

# Force POSIX fallback: exclude directories containing timeout/gtimeout from PATH.
# Because timeout, sleep, and mktemp often share the same directory (/usr/bin on
# Linux), we first create a rescue dir with symlinks to essential commands so they
# remain available after stripping that directory.  Unlike MKTEMP_FAIL_SETUP we do
# NOT override TMPDIR, so mktemp succeeds and the flag-file happy-path is exercised.
POSIX_FALLBACK_SETUP='
    rescue_dir=$(mktemp -d)
    for cmd in sleep kill grep rm mktemp ln touch; do
        p=$(command -v "$cmd" 2>/dev/null) && ln -sf "$p" "$rescue_dir/$cmd"
    done
    trap "rm -rf \"$rescue_dir\"" EXIT
    new_path="$rescue_dir"
    IFS=: read -ra dirs <<< "$PATH"
    for d in "${dirs[@]}"; do
        if [ -x "$d/timeout" ] || [ -x "$d/gtimeout" ]; then
            continue
        fi
        new_path="${new_path:+$new_path:}$d"
    done
    export PATH="$new_path"
    hash -r
    source "$LIB_PORTABLE"
'

# SETUP_POSIX_FALLBACK_ENV — eval-string helper for tests that require POSIX fallback
# WITHOUT an EXIT trap. Timer subshells inherit EXIT traps, which would prematurely
# clean up rescue_dir and produce false-positive passes in orphan-detection tests
# (16b, 18b). Uses the superset of commands (ps + bash) so all 4 tests (16a/b, 18a/b)
# share a single helper. Callers must clean up rescue_dir explicitly.
SETUP_POSIX_FALLBACK_ENV='
    rescue_dir=$(mktemp -d)
    for cmd in sleep kill grep rm mktemp ln touch ps bash; do
        p=$(command -v "$cmd" 2>/dev/null) && ln -sf "$p" "$rescue_dir/$cmd"
    done
    new_path="$rescue_dir"
    IFS=: read -ra dirs <<< "$PATH"
    for d in "${dirs[@]}"; do
        if [ -x "$d/timeout" ] || [ -x "$d/gtimeout" ]; then
            continue
        fi
        new_path="${new_path:+$new_path:}$d"
    done
    export PATH="$new_path"
    hash -r
    source "$LIB_PORTABLE"
'

# -- Helper self-test: SETUP_POSIX_FALLBACK_ENV ----------------------------------
# Verifies the helper correctly strips timeout/gtimeout from PATH and creates
# rescue_dir. Placed before Test 12 so a broken helper surfaces early.
assert "SETUP_POSIX_FALLBACK_ENV helper strips timeout from PATH and creates rescue_dir" \
    env LIB_PORTABLE="$LIB_PORTABLE" SETUP_POSIX_FALLBACK_ENV="$SETUP_POSIX_FALLBACK_ENV" bash -c '
        eval "$SETUP_POSIX_FALLBACK_ENV"
        ! command -v timeout >/dev/null 2>&1 &&
        ! command -v gtimeout >/dev/null 2>&1 &&
        [ -d "$rescue_dir" ]
    '

assert "POSIX fallback: sleep 10 with 1s timeout exits 124" \
    env LIB_PORTABLE="$LIB_PORTABLE" POSIX_FALLBACK_SETUP="$POSIX_FALLBACK_SETUP" bash -c '
        eval "$POSIX_FALLBACK_SETUP"
        rc=0; portable_timeout 1 sleep 10 || rc=$?
        [ "$rc" -eq 124 ]
    '

assert "POSIX fallback: _PORTABLE_TIMEOUT_TIMED_OUT true on genuine timeout" \
    env LIB_PORTABLE="$LIB_PORTABLE" POSIX_FALLBACK_SETUP="$POSIX_FALLBACK_SETUP" bash -c '
        eval "$POSIX_FALLBACK_SETUP"
        portable_timeout 1 sleep 10 || true
        [ "$_PORTABLE_TIMEOUT_TIMED_OUT" = "true" ]
    '

# -- Test 13: reset-semantics -------------------------------------------------
echo ""
echo "--- Test 13: _PORTABLE_TIMEOUT_TIMED_OUT resets between consecutive calls ---"

# In a single shell session: first call genuinely times out (flag → true),
# second call completes successfully (flag → false).  Verifies the reset at
# the top of portable_timeout() works across back-to-back invocations.
assert "POSIX fallback: flag resets to false after timeout then fast success" \
    env LIB_PORTABLE="$LIB_PORTABLE" POSIX_FALLBACK_SETUP="$POSIX_FALLBACK_SETUP" bash -c '
        eval "$POSIX_FALLBACK_SETUP"
        # First call: genuine timeout → true
        portable_timeout 1 sleep 10 || true
        [ "$_PORTABLE_TIMEOUT_TIMED_OUT" = "true" ] || { echo "expected true after timeout"; exit 1; }
        # Second call: fast success → reset to false
        portable_timeout 5 true
        [ "$_PORTABLE_TIMEOUT_TIMED_OUT" = "false" ]
    '

# -- Test 14: tree-sitter-generate guard: natural exit 124 not misdiagnosed ---
echo ""
echo "--- Test 14: tree-sitter-generate guard distinguishes natural exit 124 ---"

# Replicate the guard logic from scripts/tree-sitter-generate.sh:
#   if [ "$GEN_EXIT" -eq 124 ] && [ "${_PORTABLE_TIMEOUT_TIMED_OUT:-false}" = "true" ]; then
#       echo "ERROR: tree-sitter generate timed out" >&2
#   fi
# Under POSIX fallback, a command that naturally exits 124 must NOT trigger the
# timeout error path.  The stub sets tree-sitter() to exit 124 without sleeping.
assert "guard: natural exit 124 does not emit timeout error (POSIX fallback)" \
    env LIB_PORTABLE="$LIB_PORTABLE" POSIX_FALLBACK_SETUP="$POSIX_FALLBACK_SETUP" bash -c '
        eval "$POSIX_FALLBACK_SETUP"
        # Stub tree-sitter to exit 124 immediately (not a real timeout).
        tree-sitter() { return 124; }
        # Replicate guard logic from tree-sitter-generate.sh lines 141-148.
        GEN_EXIT=0
        portable_timeout 60 tree-sitter generate || GEN_EXIT=$?
        timeout_error=""
        if [ "$GEN_EXIT" -eq 124 ] && [ "${_PORTABLE_TIMEOUT_TIMED_OUT:-false}" = "true" ]; then
            timeout_error="ERROR: tree-sitter generate timed out after 60s"
        fi
        # The timeout error must NOT have been set.
        [ -z "$timeout_error" ]
    '

# -- Test 15: structural: timer cleanup uses process-group kill ----------------
echo ""
echo "--- Test 15: lib_portable.sh timer cleanup uses process-group kill ---"

# The POSIX fallback timer subshell starts an inner 'sleep $seconds' process.
# Killing only the subshell PID leaves the inner sleep as an orphan.
# Correct cleanup uses 'kill -- -$timer_pid' (process-group kill) so all
# children of the timer subshell are terminated atomically.
assert "lib_portable.sh timer cleanup uses process-group kill (kill -- -)" \
    grep -qE 'kill -- -\$timer_pid' "$LIB_PORTABLE"

# -- Test 16: behavioral: no orphan sleep after fast-exit command --------------
echo ""
echo "--- Test 16: POSIX fallback: no orphan sleep after fast-exit command ---"

# This test verifies that the timer's inner 'sleep 31337' (a distinctive
# sentinel duration) is actually spawned and properly cleaned up when the
# command exits before the timeout fires.
#
# Test 16a (positive spawn check): proves the test setup is valid — the timer
# must actually start its inner 'sleep 31337' before Test 16b's cleanup check
# is meaningful.  Runs portable_timeout in the background, polls up to 5×200ms
# (1s total) for the sentinel sleep to appear, then asserts it was found.
#
# Test 16b (orphan cleanup check): verifies the timer's 'sleep 31337' is gone
# after portable_timeout returns.  Uses 'sleep 0.3' (not 'true') as the
# command, giving the timer time to spawn its sentinel before cleanup fires.
# (With 'true', the timer subshell may not have started 'sleep 31337' before
# the main shell kills it, so the orphan check would pass vacuously.)
#
# Subtlety: timer subshells inherit EXIT traps from the calling shell.
# Save absolute paths BEFORE PATH manipulation; use them for post-exit checks.

assert "POSIX fallback: timer actually spawns sentinel sleep 31337 (positive check)" \
    env LIB_PORTABLE="$LIB_PORTABLE" SETUP_POSIX_FALLBACK_ENV="$SETUP_POSIX_FALLBACK_ENV" bash -c '
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_grep=$(command -v grep)

        eval "$SETUP_POSIX_FALLBACK_ENV"

        # Background portable_timeout with a 2s command so the timer has time
        # to spawn its inner "sleep 31337" before we check.
        portable_timeout 31337 sleep 2 &
        pt_pid=$!

        # Poll up to 5×200ms for the sentinel to appear (robust under CI load).
        # On fast systems this returns on the first iteration (~0ms wait).
        found=1
        for _attempt in 1 2 3 4 5; do
            if "$_abs_ps" -A -o pid,args 2>/dev/null \
                    | "$_abs_grep" -qE "[[:space:]]sleep 31337$"; then
                found=0
                break
            fi
            "$_abs_sleep" 0.2
        done

        # Cleanup: kill the background portable_timeout.
        kill "$pt_pid" 2>/dev/null || true
        wait "$pt_pid" 2>/dev/null || true

        # Safety-net: kill any lingering sentinel sleep 31337 processes.
        "$_abs_ps" -A -o pid,args 2>/dev/null \
            | "$_abs_grep" -E "[[:space:]]sleep 31337$" \
            | while read -r _spid _rest; do kill "$_spid" 2>/dev/null || true; done

        exit $found
    '

assert "POSIX fallback: timer cleanup leaves no orphan sleep after early-exit command" \
    env LIB_PORTABLE="$LIB_PORTABLE" SETUP_POSIX_FALLBACK_ENV="$SETUP_POSIX_FALLBACK_ENV" bash -c '
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_grep=$(command -v grep)

        eval "$SETUP_POSIX_FALLBACK_ENV"

        # Run a 0.3s command under a long timeout (31337s sentinel).
        # Using sleep 0.3 instead of "true" gives the timer time to spawn
        # its inner "sleep 31337" before the command exits.
        portable_timeout 31337 "$_abs_sleep" 0.3 || true

        # Use saved absolute paths — rescue_dir may be gone if the timer
        # subshell ran its inherited EXIT trap when killed.
        "$_abs_sleep" 0.5
        ! "$_abs_ps" -A -o pid,args 2>/dev/null | "$_abs_grep" -E "[[:space:]]sleep 31337$"
    '

# -- Test 17: structural: SIGKILL escalation uses PID-reuse-safe process-group kill ----
echo ""
echo "--- Test 17: lib_portable.sh SIGKILL escalation uses process-group kill ---"

# SIGKILL escalation was re-added to the timer subshell using process-group kill
# ('kill -9 -- -$cmd_pid') rather than individual PID kill ('kill -9 $cmd_pid').
# Process-group kill is PID-reuse safe: the SIGKILL fires BEFORE the main shell's
# wait(2) returns (the main shell is blocked precisely because SIGTERM was ignored),
# so cmd_pid cannot have been recycled.  The process-group syntax adds a second
# safety layer: a stale PGID returns ESRCH harmlessly rather than hitting an
# unrelated process.
assert "lib_portable.sh SIGKILL escalation uses process-group kill (PID-reuse safe)" \
    grep -qF 'kill -9 -- -$cmd_pid' "$LIB_PORTABLE"

assert "lib_portable.sh SIGKILL does NOT use individual PID kill (PID-reuse unsafe)" \
    bash -c '! grep -qE "kill -(9|KILL) [^-]" "$1"' _ "$LIB_PORTABLE"

# -- Test 18: monitor mode (set -m) preserved after POSIX fallback call --------
echo ""
echo "--- Test 18: POSIX fallback: monitor mode (set -m) preserved ---"

# If the caller has job control enabled (set -m), portable_timeout must restore
# it after using set -m internally.  Before the fix, the code unconditionally
# ran set +m after launching the timer subshell, which silently disabled the
# caller's monitor mode.  This test guards against that regression.
assert "POSIX fallback: monitor mode (set -m) preserved after portable_timeout call" \
    env LIB_PORTABLE="$LIB_PORTABLE" POSIX_FALLBACK_SETUP="$POSIX_FALLBACK_SETUP" bash -c '
        eval "$POSIX_FALLBACK_SETUP"
        set -m 2>/dev/null || true
        portable_timeout 5 true
        # $- must still contain "m" (monitor mode active).
        case $- in
            *m*) ;;
            *) echo "monitor mode was clobbered by portable_timeout"; exit 1 ;;
        esac
    '

# -- Test 18b: degraded-path monitor mode (set -m) preserved ------------------
echo ""
echo "--- Test 18b: degraded path (mktemp fails): monitor mode (set -m) preserved ---"

# Symmetric to Test 18 but exercises the degraded path (mktemp failure) so
# that lines 116-120 of lib_portable.sh have explicit coverage.  Completes
# the 2x2 matrix: {normal, degraded} x {monitor-on, monitor-off}.
assert "degraded path: monitor mode (set -m) preserved after portable_timeout call" \
    env LIB_PORTABLE="$LIB_PORTABLE" MKTEMP_FAIL_SETUP="$MKTEMP_FAIL_SETUP" bash -c '
        eval "$MKTEMP_FAIL_SETUP"
        set -m 2>/dev/null || true
        portable_timeout 5 true 2>/dev/null || true
        # $- must still contain "m" (monitor mode active).
        case $- in
            *m*) ;;
            *) echo "monitor mode was clobbered by portable_timeout (degraded path)"; exit 1 ;;
        esac
    '

# -- Test 18c: degraded-path no-monitor mode (set +m) preserved ---------------
echo ""
echo "--- Test 18c: degraded path (mktemp fails): no-monitor mode (set +m) preserved ---"

# Symmetric to Test 19 on the degraded path: when the caller has no job
# control, portable_timeout must leave it disabled after the call.
assert "degraded path: no-monitor mode (set +m) preserved after portable_timeout call" \
    env LIB_PORTABLE="$LIB_PORTABLE" MKTEMP_FAIL_SETUP="$MKTEMP_FAIL_SETUP" bash -c '
        eval "$MKTEMP_FAIL_SETUP"
        set +m 2>/dev/null || true
        portable_timeout 5 true 2>/dev/null || true
        # $- must NOT contain "m" (monitor mode inactive).
        case $- in
            *m*) echo "portable_timeout unexpectedly enabled monitor mode (degraded path)"; exit 1 ;;
        esac
    '

# -- Test 19: no-monitor mode (set +m, default) preserved after POSIX fallback -
echo ""
echo "--- Test 19: POSIX fallback: no-monitor mode (set +m) preserved ---"

# Symmetric case: when the caller did NOT have job control enabled, portable_timeout
# must leave it disabled after the call.  This is the default condition and already
# passes (the bug is only visible when set -m was active), but it provides regression
# coverage so future patches cannot accidentally enable monitor mode for the caller.
assert "POSIX fallback: no-monitor mode (set +m) preserved after portable_timeout call" \
    env LIB_PORTABLE="$LIB_PORTABLE" POSIX_FALLBACK_SETUP="$POSIX_FALLBACK_SETUP" bash -c '
        eval "$POSIX_FALLBACK_SETUP"
        set +m 2>/dev/null || true
        portable_timeout 5 true
        # $- must NOT contain "m" (monitor mode inactive).
        case $- in
            *m*) echo "portable_timeout unexpectedly enabled monitor mode"; exit 1 ;;
        esac
    '

# -- Test 20: structural: header documents SIGKILL escalation via process-group kill ----
echo ""
echo "--- Test 20: portable_timeout header documents SIGKILL escalation ---"

# The portable_timeout doc comment must describe the SIGTERM-first,
# SIGKILL-escalation strategy using process-group kill.  The old SIGTERM-only
# language must be replaced.  This test FAILS until step-6 updates the doc comment.
assert "portable_timeout header documents SIGKILL escalation via process-group kill" \
    grep -qiE 'escalat.*SIGKILL.*via.*process.group|SIGKILL.*via.*process.group' "$LIB_PORTABLE"

# -- Test 21: behavioral: SIGKILL escalation — exit 124 and no orphan --------
echo ""
echo "--- Test 21: POSIX fallback: SIGKILL escalation — exit 124 and no orphan ---"

# This test exercises the full SIGKILL escalation path in the POSIX fallback:
#   1. Timer fires after 1s, sends SIGTERM to the command (bash wrapper).
#   2. Command ignores SIGTERM (trap "" TERM).
#   3. Timer escalates after 2s grace: sends SIGKILL to the command (bash).
#   4. Bash wrapper is killed; its child 'sleep 31339' is an orphan candidate.
#
# Asserts: (a) exit code is 124 (genuine timeout recognized despite SIGKILL),
#          (b) no 'sleep 31339' process remains after portable_timeout returns.
#
# This test FAILS against unpatched lib_portable.sh:
#   - Exit code is 137 (128+9) because flag-file check only accepts 143 (SIGTERM).
#   - 'sleep 31339' survives because kill -9 $cmd_pid kills only the bash
#     wrapper, not its child sleep.
# These failures confirm the test catches real SIGKILL path regressions.
#
# Sentinel duration 31339 is distinct from 31337 (Test 16) to avoid ps conflicts.
# Total test time: ~1s (timer fires) + 2s (grace period) = ~3s.
#
# Regression guard (verified manually, step-5): temporarily commenting out the
# 'kill -9' line in lib_portable.sh causes Test 21a to FAIL (exit 143 never
# arrives, process stays alive, flag check never triggers, returns 143 not 124).
# This confirms the test has discriminating power and is not vacuous.

assert "POSIX fallback: SIGKILL escalation returns exit code 124" \
    env LIB_PORTABLE="$LIB_PORTABLE" SETUP_POSIX_FALLBACK_ENV="$SETUP_POSIX_FALLBACK_ENV" bash -c '
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_grep=$(command -v grep)
        _abs_bash=$(command -v bash)

        eval "$SETUP_POSIX_FALLBACK_ENV"

        # Command ignores SIGTERM; timer must escalate to SIGKILL after 2s grace.
        rc=0
        portable_timeout 1 "$_abs_bash" -c '"'"'trap "" TERM; sleep 31339'"'"' || rc=$?
        [ "$rc" -eq 124 ]
    '

assert "POSIX fallback: SIGKILL escalation leaves no orphan sleep 31339" \
    env LIB_PORTABLE="$LIB_PORTABLE" SETUP_POSIX_FALLBACK_ENV="$SETUP_POSIX_FALLBACK_ENV" bash -c '
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_grep=$(command -v grep)
        _abs_bash=$(command -v bash)
        _abs_kill=$(command -v kill)
        _abs_awk=$(command -v awk)

        # Kill stale "sleep 31339" orphans left by Test 21a or prior runs so
        # they do not cause a false negative for THIS invocation.
        "$_abs_ps" -A -o pid,args 2>/dev/null \
            | "$_abs_grep" -E "[[:space:]]sleep 31339$" \
            | "$_abs_awk" "{print \$1}" \
            | while read _pid; do "$_abs_kill" -9 "$_pid" 2>/dev/null; done
        "$_abs_sleep" 0.5

        eval "$SETUP_POSIX_FALLBACK_ENV"

        # Command ignores SIGTERM; timer escalates to SIGKILL.
        portable_timeout 1 "$_abs_bash" -c '"'"'trap "" TERM; sleep 31339'"'"' || true

        # Poll up to 5s for the orphan sleep 31339 to be reaped.
        # A single fixed sleep races against kernel process-table cleanup
        # on busy systems, so retry a few times before declaring failure.
        _check_rc=0
        for _try in 1 2 3 4 5; do
            _check_rc=0
            ! "$_abs_ps" -A -o pid,args 2>/dev/null | "$_abs_grep" -E "[[:space:]]sleep 31339$" || _check_rc=$?
            [ "$_check_rc" -eq 0 ] && break
            "$_abs_sleep" 1
        done
        # Clean up rescue_dir explicitly (no EXIT trap to avoid subshell inheritance).
        rm -rf "$rescue_dir"
        exit "$_check_rc"
    '

# -- Test 22: structural: post-wait orphan cleanup uses SIGKILL not SIGTERM ---
echo ""
echo "--- Test 22: post-wait orphan cleanup uses SIGKILL not SIGTERM ---"

# After the timer escalates to SIGKILL because the command ignored SIGTERM,
# the post-wait orphan cleanup (line 149) must also use SIGKILL.  Sending
# SIGTERM at that point is internally inconsistent — the whole reason we
# reached this code path is that SIGTERM was ineffective.  The '|| true'
# suffix is unique to this line, distinguishing it from the timer subshell
# SIGKILL lines.
assert "post-wait orphan cleanup uses kill -9 (SIGKILL) not plain kill (SIGTERM)" \
    grep -qF 'kill -9 -- -$cmd_pid 2>/dev/null || true' "$LIB_PORTABLE"

assert "no remaining line sends SIGTERM (plain kill) to the command process group" \
    bash -c '! grep -qF "kill -- -\$cmd_pid 2>/dev/null || true" "$1"' _ "$LIB_PORTABLE"

# -- Test 23: structural: both timer subshells quote the PGID argument (S1) ---
echo ""
echo "--- Test 23: timer subshells use quoted PGID argument for SIGKILL ---"

# Both timer subshells (flag-file branch and degraded branch) must quote the
# PGID argument: 'kill -9 -- "-$cmd_pid"' rather than 'kill -9 -- -$cmd_pid'.
# Defensive quoting prevents word-splitting if the value is ever non-numeric.
# Exactly 2 occurrences are required — one per timer subshell branch.
assert "both timer subshells use quoted PGID in SIGKILL: exactly 2 occurrences" \
    bash -c 'count=$(grep -cF "kill -9 -- \"-\$cmd_pid\"" "$1"); [ "$count" -eq 2 ]' _ "$LIB_PORTABLE"

# -- Test 24: structural: grace period is a named variable (DRY) (S4) ---------
echo ""
echo "--- Test 24: grace period DRY — local _pt_kill_grace variable used in both timer subshells ---"

# The hardcoded 'sleep 2' in both timer subshells must be replaced by a single
# named local variable '_pt_kill_grace=2' declared once in portable_timeout.
# Two assertions: (a) the local declaration exists, (b) exactly 2 uses of
# 'sleep "$_pt_kill_grace"' appear — one per timer subshell branch.
assert "portable_timeout declares local _pt_kill_grace=2" \
    grep -qE 'local[[:space:]]+_pt_kill_grace=2' "$LIB_PORTABLE"

assert "both timer subshells reference \$_pt_kill_grace: exactly 2 occurrences" \
    bash -c 'count=$(grep -cF "sleep \"\$_pt_kill_grace\"" "$1"); [ "$count" -eq 2 ]' _ "$LIB_PORTABLE"

# -- Test 25: structural: Test 16a exit variable is quoted -------------------
echo ""
echo "--- Test 25: structural: Test 16a exit variable is quoted ---"

# Test 16a closes its subshell with 'exit $found'.  The companion orphan-check
# subshell (Test 18b) correctly uses 'exit "$_check_rc"'.  Consistency between
# two structurally identical exit patterns requires both to quote the variable.
# Check the ABSENCE of the unquoted form with 8-space indentation.  The grep
# pattern uses \$found (with backslash) so the assertion line itself is not a
# self-referential match.
assert "Test 16a subshell uses quoted exit \"\$found\" (no unquoted form)" \
    bash -c '! grep -qF "        exit \$found" "$1"' _ "${BASH_SOURCE[0]}"

# -- Summary ------------------------------------------------------------------
test_summary
