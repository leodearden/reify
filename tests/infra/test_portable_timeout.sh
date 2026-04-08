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
# is meaningful.  Runs portable_timeout in the background, waits 0.5s, and
# asserts the sentinel sleep is visible in ps.
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
    env LIB_PORTABLE="$LIB_PORTABLE" bash -c '
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_grep=$(command -v grep)

        rescue_dir=$(mktemp -d)
        for cmd in sleep kill grep rm mktemp ln touch ps; do
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

        # Background portable_timeout with a 2s command so the timer has time
        # to spawn its inner "sleep 31337" before we check.
        portable_timeout 31337 sleep 2 &
        pt_pid=$!

        # Give the timer subshell time to spawn its inner "sleep 31337".
        "$_abs_sleep" 0.5

        # Capture whether the sentinel is present before cleanup.
        found=1
        "$_abs_ps" -A -o pid,args 2>/dev/null \
            | "$_abs_grep" -qE "[[:space:]]sleep 31337$" && found=0 || true

        # Cleanup: kill the background portable_timeout.
        kill "$pt_pid" 2>/dev/null || true
        wait "$pt_pid" 2>/dev/null || true

        exit $found
    '

assert "POSIX fallback: timer cleanup leaves no orphan sleep after early-exit command" \
    env LIB_PORTABLE="$LIB_PORTABLE" bash -c '
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_grep=$(command -v grep)

        rescue_dir=$(mktemp -d)
        for cmd in sleep kill grep rm mktemp ln touch ps; do
            p=$(command -v "$cmd" 2>/dev/null) && ln -sf "$p" "$rescue_dir/$cmd"
        done
        # NOTE: no EXIT trap on rescue_dir — timer subshells inherit traps and
        # would clean up the rescue dir on exit, causing false-positive passes.
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

        # Run a 0.3s command under a long timeout (31337s sentinel).
        # Using sleep 0.3 instead of "true" gives the timer time to spawn
        # its inner "sleep 31337" before the command exits.
        portable_timeout 31337 "$_abs_sleep" 0.3 || true

        # Use saved absolute paths — rescue_dir may be gone if the timer
        # subshell ran its inherited EXIT trap when killed.
        "$_abs_sleep" 0.5
        ! "$_abs_ps" -A -o pid,args 2>/dev/null | "$_abs_grep" -E "[[:space:]]sleep 31337$"
    '

# -- Test 17: structural: timer subshell does NOT have SIGKILL escalation ------
echo ""
echo "--- Test 17: lib_portable.sh timer subshell does NOT escalate to SIGKILL ---"

# The SIGKILL escalation (kill -9 / kill -KILL) has been removed from the timer
# subshell to eliminate the PID-reuse race: by the time the SIGKILL would run,
# the main shell has already wait(2)ed on cmd_pid and the kernel may have recycled
# that PID to an unrelated process.  The main shell's process-group kill
# (kill -- -$timer_pid) handles cleanup atomically instead.
assert "lib_portable.sh timer subshell does NOT escalate to SIGKILL (PID-reuse safety)" \
    bash -c '! grep -qE "kill -9[[:space:]]|kill -KILL[[:space:]]" "$1"' _ "$LIB_PORTABLE"

# -- Test 18: monitor mode (set -m) preserved after POSIX fallback call --------
echo ""
echo "--- Test 18: POSIX fallback: monitor mode (set -m) preserved ---"

# If the caller has job control enabled (set -m), portable_timeout must restore
# it after using set -m internally.  Current code unconditionally runs set +m
# after launching the timer subshell (lines 98/108), which silently disables
# the caller's monitor mode.  This test FAILS on unpatched code.
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

# -- Test 20: structural: header documents SIGTERM-only termination ------------
echo ""
echo "--- Test 20: portable_timeout header documents SIGTERM-only termination ---"

# S4 requires that the portable_timeout header comment documents that the POSIX
# fallback uses SIGTERM only and does not escalate to SIGKILL (PID-reuse safety).
# This structural test fails until the header is updated in the next step.
assert "portable_timeout header documents SIGTERM-only termination" \
    grep -qiE 'SIGTERM.*only|SIGTERM.*no.*SIGKILL|does not escalate to SIGKILL' "$LIB_PORTABLE"

# -- Test 18: behavioral: SIGKILL escalation — exit 124 and no orphan --------
echo ""
echo "--- Test 18: POSIX fallback: SIGKILL escalation — exit 124 and no orphan ---"

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

assert "POSIX fallback: SIGKILL escalation returns exit code 124" \
    env LIB_PORTABLE="$LIB_PORTABLE" bash -c '
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_grep=$(command -v grep)
        _abs_bash=$(command -v bash)

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

        # Command ignores SIGTERM; timer must escalate to SIGKILL after 2s grace.
        rc=0
        portable_timeout 1 "$_abs_bash" -c '"'"'trap "" TERM; sleep 31339'"'"' || rc=$?
        [ "$rc" -eq 124 ]
    '

assert "POSIX fallback: SIGKILL escalation leaves no orphan sleep 31339" \
    env LIB_PORTABLE="$LIB_PORTABLE" bash -c '
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_grep=$(command -v grep)
        _abs_bash=$(command -v bash)

        rescue_dir=$(mktemp -d)
        for cmd in sleep kill grep rm mktemp ln touch ps bash; do
            p=$(command -v "$cmd" 2>/dev/null) && ln -sf "$p" "$rescue_dir/$cmd"
        done
        # NOTE: no EXIT trap on rescue_dir — timer subshells inherit traps and
        # would clean up the rescue dir on exit, causing false-positive passes.
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

        # Command ignores SIGTERM; timer escalates to SIGKILL.
        portable_timeout 1 "$_abs_bash" -c '"'"'trap "" TERM; sleep 31339'"'"' || true

        # Brief wait then verify no orphan sleep 31339.
        "$_abs_sleep" 0.5
        ! "$_abs_ps" -A -o pid,args 2>/dev/null | "$_abs_grep" -E "[[:space:]]sleep 31339$"
    '

# -- Summary ------------------------------------------------------------------
test_summary
