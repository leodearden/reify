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

# -- Helper: _build_posix_fallback_env ----------------------------------------
# Generates an eval-string that creates a rescue dir, symlinks commands, strips
# timeout/gtimeout from PATH, and sources LIB_PORTABLE for POSIX fallback tests.
# Args: $1=extra_cmds  space-separated commands beyond the base set
#                      (sleep kill grep rm mktemp ln)
#       $2=trap_mode   "trap"   — install EXIT trap to clean up rescue_dir
#                      "notrap" — no trap; caller must rm -rf "$rescue_dir"
#       $3=tmpdir_mode "normal" — leave TMPDIR alone
#                      "broken" — set TMPDIR=/dev/null/nope to force mktemp failure
_build_posix_fallback_env() {
    local extra_cmds="$1"
    local trap_mode="$2"
    local tmpdir_mode="$3"
    local all_cmds="sleep kill grep rm mktemp ln${extra_cmds:+ $extra_cmds}"

    printf '    rescue_dir=$(mktemp -d)\n'
    printf '    for cmd in %s; do\n' "$all_cmds"
    printf '        p=$(command -v "$cmd" 2>/dev/null) && ln -sf "$p" "$rescue_dir/$cmd"\n'
    printf '    done\n'
    if [ "$trap_mode" = "trap" ]; then
        printf '    trap "rm -rf \"$rescue_dir\"" EXIT\n'
    fi
    printf '    new_path="$rescue_dir"\n'
    printf '    IFS=: read -ra dirs <<< "$PATH"\n'
    printf '    for d in "${dirs[@]}"; do\n'
    printf '        if [ -x "$d/timeout" ] || [ -x "$d/gtimeout" ]; then\n'
    printf '            continue\n'
    printf '        fi\n'
    printf '        new_path="${new_path:+$new_path:}$d"\n'
    printf '    done\n'
    printf '    export PATH="$new_path"\n'
    if [ "$tmpdir_mode" = "broken" ]; then
        printf '    export TMPDIR=/dev/null/nope\n'
    fi
    printf '    hash -r\n'
    printf '    source "$LIB_PORTABLE"\n'
}

# -- Shared regex: canonical 'kill -- -$cmd_pid' form -------------------------
# Used by Test 22 (structural assertion: no non-comment line in LIB_PORTABLE uses
# this form) and by Test 22b (meta-assertions validating match/reject semantics).
# Single-quoted to preserve the backslash-dollar literal; grep -E sees \$ as a
# literal dollar sign (matching the unquoted $cmd_pid in the shell text).
KILL_CMD_PID_RE='kill[[:space:]]+--[[:space:]]+"?-\$cmd_pid'

echo "=== portable_timeout unit tests ==="

# -- Meta: KILL_CMD_PID_RE shared-setup constant is declared ------------------
# Verifies the shared regex variable is set before any test that uses it.
# KILL_CMD_PID_RE is used by Test 22 (structural assertion against LIB_PORTABLE)
# and by Test 22b (meta-assertions validating the regex's discrimination semantics).
assert "KILL_CMD_PID_RE variable is declared (shared by Test 22 structural assertion and Test 22b meta-assertions)" \
    env KILL_CMD_PID_RE="${KILL_CMD_PID_RE:-}" bash -c '[ -n "$KILL_CMD_PID_RE" ]'

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
MKTEMP_FAIL_SETUP=$(_build_posix_fallback_env "" trap broken)

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
POSIX_FALLBACK_SETUP=$(_build_posix_fallback_env "touch" trap normal)

# POSIX_FALLBACK_SETUP_NO_TRAP — generated by _build_posix_fallback_env.
# Eval-string for tests requiring POSIX fallback WITHOUT an EXIT trap.
# Timer subshells inherit EXIT traps, which would prematurely clean up
# rescue_dir and produce false-positive passes in orphan-detection tests
# (16b, 21b). Uses the superset of commands (ps + bash) so all 4 tests
# (16a/b, 21a/b) share a single helper.
# Callers must clean up rescue_dir explicitly (no EXIT trap to auto-clean).
POSIX_FALLBACK_SETUP_NO_TRAP=$(_build_posix_fallback_env "touch ps bash" notrap normal)

# -- Helper self-test: POSIX_FALLBACK_SETUP (with-trap) -----------------------
# Symmetric counterpart to the POSIX_FALLBACK_SETUP_NO_TRAP self-test below.
# Verifies that eval'ing POSIX_FALLBACK_SETUP (a) sets an EXIT trap and
# (b) defines portable_timeout.  Placed before Test 12 so a broken with-trap
# helper surfaces early rather than only through indirect downstream failures.
assert "POSIX_FALLBACK_SETUP helper sets EXIT trap and defines portable_timeout" \
    env LIB_PORTABLE="$LIB_PORTABLE" POSIX_FALLBACK_SETUP="$POSIX_FALLBACK_SETUP" bash -c '
        eval "$POSIX_FALLBACK_SETUP"
        trap_output=$(trap -p EXIT)
        [ -n "$trap_output" ] &&
        declare -f portable_timeout >/dev/null
    '

# -- Helper self-test: POSIX_FALLBACK_SETUP_NO_TRAP ----------------------------------
# Verifies the helper correctly strips timeout/gtimeout from PATH and creates
# rescue_dir. Placed before Test 12 so a broken helper surfaces early.
assert "POSIX_FALLBACK_SETUP_NO_TRAP helper strips timeout from PATH and creates rescue_dir" \
    env LIB_PORTABLE="$LIB_PORTABLE" POSIX_FALLBACK_SETUP_NO_TRAP="$POSIX_FALLBACK_SETUP_NO_TRAP" bash -c '
        eval "$POSIX_FALLBACK_SETUP_NO_TRAP"
        _check_rc=0
        ! command -v timeout >/dev/null 2>&1 &&
        ! command -v gtimeout >/dev/null 2>&1 &&
        [ -d "$rescue_dir" ] &&
        declare -f portable_timeout >/dev/null &&
        [ -z "$(trap -p EXIT)" ] || _check_rc=$?
        # Clean up rescue_dir explicitly (no EXIT trap to avoid subshell inheritance).
        rm -rf "$rescue_dir"
        exit "$_check_rc"
    '

# -- Builder self-test: _build_posix_fallback_env --------------------------------
# Assertion (a) FAILS (red) until step-5 defines the builder function.
# Assertions (b) verify the three generated setup variables have correct
# properties, guarding the builder's output contract after the refactor.
assert "_build_posix_fallback_env builder is defined in the test script" \
    declare -f _build_posix_fallback_env

assert "builder-generated setup variables non-empty with correct trap/TMPDIR properties" \
    env MKTEMP_FAIL_SETUP="$MKTEMP_FAIL_SETUP" \
        POSIX_FALLBACK_SETUP="$POSIX_FALLBACK_SETUP" \
        POSIX_FALLBACK_SETUP_NO_TRAP="$POSIX_FALLBACK_SETUP_NO_TRAP" \
    bash -c '
        [ -n "$MKTEMP_FAIL_SETUP" ] &&
        [ -n "$POSIX_FALLBACK_SETUP" ] &&
        [ -n "$POSIX_FALLBACK_SETUP_NO_TRAP" ] &&
        printf "%s" "$MKTEMP_FAIL_SETUP" | grep -q "TMPDIR=/dev/null/nope" &&
        ! printf "%s" "$POSIX_FALLBACK_SETUP_NO_TRAP" | grep -q "trap.*EXIT" &&
        printf "%s" "$POSIX_FALLBACK_SETUP" | grep -q "trap.*EXIT" &&
        printf "%s" "$MKTEMP_FAIL_SETUP" | grep -q "trap.*EXIT"
    '

# -- Safety-net regression: -E not -qE in while-read kill pipeline -----------
assert "Test 16a safety-net uses -E not -qE (stdout feeds while-read kill-loop)" \
    env TEST_FILE="$0" bash -c '
        _key="kills any"
        _ln=$(grep -n "${_key} sleep 31337 system-wide" "$TEST_FILE" | tail -1 | cut -d: -f1)
        _sn=$(sed -n "${_ln},$((${_ln}+4))p" "$TEST_FILE")
        printf "%s" "$_sn" | grep -q " -E " && ! printf "%s" "$_sn" | grep -q " -qE "
    '

assert "safety-net pipeline actually kills a deliberately leaked sleep 31337 sentinel" \
    bash -c '
        sleep 31337 & _victim=$!
        trap "kill -9 $_victim 2>/dev/null || true" EXIT
        sleep 0.1
        ps -A -o pid,args 2>/dev/null \
            | grep -E "[[:space:]]sleep 31337$" \
            | while read -r _spid _rest; do kill "$_spid" 2>/dev/null || true; done
        sleep 0.2
        ! kill -0 "$_victim" 2>/dev/null
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
# is meaningful.  Runs portable_timeout in the background, 5 checks separated
# by 4×200ms sleeps (~0.8s worst case) for the sentinel sleep to appear, then
# asserts it was found.
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
    env LIB_PORTABLE="$LIB_PORTABLE" POSIX_FALLBACK_SETUP_NO_TRAP="$POSIX_FALLBACK_SETUP_NO_TRAP" bash -c '
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_grep=$(command -v grep)

        eval "$POSIX_FALLBACK_SETUP_NO_TRAP"

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

        # SAFETY_NET_GREP_LINE — Assumes no parallel test runs on the same host: kills any sleep 31337 system-wide.
        # Safety-net: kill any lingering sentinel sleep 31337 processes.
        "$_abs_ps" -A -o pid,args 2>/dev/null \
            | "$_abs_grep" -E "[[:space:]]sleep 31337$" \
            | while read -r _spid _rest; do kill "$_spid" 2>/dev/null || true; done

        # Clean up rescue_dir explicitly (no EXIT trap to avoid subshell inheritance).
        rm -rf "$rescue_dir"
        exit "$found"
    '

assert "POSIX fallback: timer cleanup leaves no orphan sleep after early-exit command" \
    env LIB_PORTABLE="$LIB_PORTABLE" POSIX_FALLBACK_SETUP_NO_TRAP="$POSIX_FALLBACK_SETUP_NO_TRAP" bash -c '
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_grep=$(command -v grep)
        _abs_kill=$(command -v kill)
        _abs_awk=$(command -v awk)

        # Kill stale "sleep 31337" orphans left by Test 16a, prior runs, or
        # concurrent verify pipelines on the same host so they do not cause
        # a false negative for THIS invocation. Mirrors the stabilization
        # pattern applied to Test 21b in cbeeb5a81 / 14f9287f2.
        "$_abs_ps" -A -o pid,args 2>/dev/null \
            | "$_abs_grep" -E "[[:space:]]sleep 31337$" \
            | "$_abs_awk" "{print \$1}" \
            | while read _pid; do "$_abs_kill" -9 "$_pid" 2>/dev/null; done
        "$_abs_sleep" 0.5

        eval "$POSIX_FALLBACK_SETUP_NO_TRAP"

        # Run a 0.3s command under a long timeout (31337s sentinel).
        # Using sleep 0.3 instead of "true" gives the timer time to spawn
        # its inner "sleep 31337" before the command exits.
        portable_timeout 31337 "$_abs_sleep" 0.3 || true

        # Poll up to 5s for the orphan sleep 31337 to be reaped.
        # A single fixed sleep races against kernel process-table cleanup
        # on busy systems, so retry a few times before declaring failure.
        _check_rc=0
        for _try in 1 2 3 4 5; do
            _check_rc=0
            ! "$_abs_ps" -A -o pid,args 2>/dev/null | "$_abs_grep" -E "[[:space:]]sleep 31337$" || _check_rc=$?
            [ "$_check_rc" -eq 0 ] && break
            "$_abs_sleep" 1
        done
        # Clean up rescue_dir explicitly (no EXIT trap to avoid subshell inheritance).
        rm -rf "$rescue_dir"
        exit "$_check_rc"
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
    env LIB_PORTABLE="$LIB_PORTABLE" POSIX_FALLBACK_SETUP_NO_TRAP="$POSIX_FALLBACK_SETUP_NO_TRAP" bash -c '
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_grep=$(command -v grep)
        _abs_bash=$(command -v bash)
        _abs_kill=$(command -v kill)
        _abs_awk=$(command -v awk)

        # Kill stale "sleep 31339" orphans left by prior runs or concurrent verify
        # pipelines so they do not cause resource contention / timing interference
        # for this invocation.  Mirrors the stabilization applied to Test 21b in
        # cbeeb5a81 / 14f9287f2 and to Test 16b in f72ef337e.
        "$_abs_ps" -A -o pid,args 2>/dev/null \
            | "$_abs_grep" -E "[[:space:]]sleep 31339$" \
            | "$_abs_awk" "{print \$1}" \
            | while read _pid; do "$_abs_kill" -9 "$_pid" 2>/dev/null; done
        "$_abs_sleep" 0.5

        eval "$POSIX_FALLBACK_SETUP_NO_TRAP"

        # Command ignores SIGTERM; timer must escalate to SIGKILL after 2s grace.
        rc=0
        portable_timeout 1 "$_abs_bash" -c '"'"'trap "" TERM; sleep 31339'"'"' || rc=$?
        _check_rc=0
        [ "$rc" -eq 124 ] || _check_rc=$?
        # Clean up rescue_dir explicitly (no EXIT trap to avoid subshell inheritance).
        rm -rf "$rescue_dir"
        exit "$_check_rc"
    '

assert "POSIX fallback: SIGKILL escalation leaves no orphan sleep 31339" \
    env LIB_PORTABLE="$LIB_PORTABLE" POSIX_FALLBACK_SETUP_NO_TRAP="$POSIX_FALLBACK_SETUP_NO_TRAP" bash -c '
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

        eval "$POSIX_FALLBACK_SETUP_NO_TRAP"

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

# Regex matches the canonical 'kill -- -$cmd_pid' form (quoted or unquoted).
# 'kill -9 -- ...' is excluded because [[:space:]]+ between 'kill' and '--'
# cannot match the intervening '-9' — no prefix anchor is needed.
# Comment lines are excluded first to avoid false matches on inline documentation.
assert "no line uses the canonical kill -- -\$cmd_pid form (quoted or unquoted)" \
    env KILL_CMD_PID_RE="$KILL_CMD_PID_RE" bash -c '! grep -v '"'"'^[[:space:]]*#'"'"' "$1" | grep -qE "$KILL_CMD_PID_RE"' _ "$LIB_PORTABLE"

# -- Test 22b (meta): simplified kill regex discrimination semantics -----------
echo ""
echo "--- Test 22b (meta): simplified kill regex matches/rejects correctly ---"

# Verifies the simplified regex (without the dead '(^|[^-9])' prefix) correctly
# matches 'kill -- -$cmd_pid' and correctly rejects 'kill -9 -- -$cmd_pid'.
# Self-contained; does not depend on lib_portable.sh content.
assert "simplified kill regex matches canonical kill -- -\$cmd_pid" \
    env KILL_CMD_PID_RE="$KILL_CMD_PID_RE" bash -c 'printf "%s\n" "kill -- -\$cmd_pid" | grep -qE "$KILL_CMD_PID_RE"'

assert "simplified kill regex rejects kill -9 -- -\$cmd_pid" \
    env KILL_CMD_PID_RE="$KILL_CMD_PID_RE" bash -c '! printf "%s\n" "kill -9 -- -\$cmd_pid" | grep -qE "$KILL_CMD_PID_RE"'

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

# -- Test 24b (behavioral): _pt_kill_grace override flows to timer -----------
echo ""
echo "--- Test 24b (behavioral): _pt_kill_grace override flows to timer ---"

# Verifies that overriding _pt_kill_grace inside portable_timeout changes the
# actual wall-clock grace period observed by the timer.  A SIGTERM-ignoring
# command forces the SIGKILL escalation path; elapsed time must reflect the
# override value (5s) rather than the default (2s).
#
# Timing arithmetic: 1s timer + 5s grace ~= 6s wall-clock.  With date +%s
# (1-second integer resolution), the measured gap can be as low as 5.
# Asserting gap >= 5 excludes the 3s default-grace regression bucket by a
# robust 2-second margin, making this CI-safe under moderate load.
#
# The substitution is anchored on 'local' to avoid silently rewriting a
# stray '_pt_kill_grace=2' in a comment or string literal.
#
# Sentinel duration 999 is distinct from 31337/31339 to avoid ps conflicts.
assert "behavioral: _pt_kill_grace=5 override causes >=5s elapsed (SIGKILL path)" \
    env LIB_PORTABLE="$LIB_PORTABLE" POSIX_FALLBACK_SETUP_NO_TRAP="$POSIX_FALLBACK_SETUP_NO_TRAP" bash -c '
        _abs_bash=$(command -v bash)
        _abs_date=$(command -v date)

        eval "$POSIX_FALLBACK_SETUP_NO_TRAP"
        trap '"'"'rm -rf "$rescue_dir"'"'"' EXIT

        func_text=$(declare -f portable_timeout)
        case "$func_text" in
            *"local _pt_kill_grace=2"*) ;;
            *) exit 2 ;;  # sanity: local _pt_kill_grace=2 missing from portable_timeout
        esac
        # Count-exactly-1 check: bash ${var/pat/repl} replaces only the first
        # occurrence, so a second local _pt_kill_grace=2 in portable_timeout
        # would silently leave the default in place at runtime (the later
        # local declaration overrides the first).  Fail loudly here with a
        # clear count instead of a mysterious timing failure below.
        _pt_grace_count=$(grep -cF "local _pt_kill_grace=2" <<< "$func_text")
        [ "$_pt_grace_count" -eq 1 ] || exit 2  # sanity: expected exactly 1 occurrence of local _pt_kill_grace=2
        eval "${func_text/local _pt_kill_grace=2/local _pt_kill_grace=5}"

        t_start=$("$_abs_date" +%s)
        portable_timeout 1 "$_abs_bash" -c '"'"'trap "" TERM; sleep 999'"'"' || true
        t_end=$("$_abs_date" +%s)
        gap=$((t_end - t_start))
        [ "$gap" -ge 5 ]
    '

# -- Meta: Test 24c block is gone and Test 24b stale cross-reference removed --
# (a) No comment line starting with '# -- Test 24c' (anchored to the header form)
# (b) No comment line with the stale 'Uses the grep -cF <<< idiom' cross-ref
#     (the original was: '# Uses the grep -cF <<< idiom validated by Test 24c.')
assert "Test 24c block absent and stale cross-reference removed from Test 24b" \
    bash -c '! grep -qE '"'"'^# -- Test 24c'"'"' "$1" && ! grep -qE '"'"'^[[:space:]]+# Uses the grep -cF <<< idiom'"'"' "$1"' \
    _ "${BASH_SOURCE[0]}"

# -- Test 24d (structural): all count-grep uses in this file include -cF ------
echo ""
echo "--- Test 24d (structural): count-grep uses include -cF flag ---"

# Task 1605 origin: the review for task 1473 asked for consistency between
# count-grep invocations; the merge resolution (commit 869964c9f) already
# fixed all occurrences.  This guard locks that convention in.
#
# The pattern is assembled at runtime so no substring of this source file
# can be an accidental self-match.  The guard rejects any line where the
# count flag is not immediately followed by F for fixed-string safety.
#
# The regex is split across three printf arguments so no two adjacent args
# produce the flagless count-grep pattern contiguously in source; the self-
# referential scan cannot false-positive on this block.  Invocations with
# -cE (extended-regex) are also caught — this file has none intentionally.
printf -v _24d_regex '%s' 'grep' ' -c' '([^F]|$)'
assert "count-grep uses include -cF flag (no bare count-grep)" \
    bash -c '! grep -nE "$2" "$1"' _ "${BASH_SOURCE[0]}" "$_24d_regex"

# -- Test 24e (meta): validate the Test 24d guard regex discrimination --------
echo ""
echo "--- Test 24e (meta): guard regex discriminates bare vs -cF correctly ---"

# Verifies _24d_regex (assembled above) correctly matches a flagless count-grep
# invocation and correctly rejects count-grep -cF.  Mirrors the Test 22b
# positive/negative meta-assertion shape: feed two synthetic inputs and assert
# the regex discriminates correctly.
#
# Synthetic strings are assembled via printf to avoid placing any source
# substring that the guard regex would detect in this source file.
#
# positive: flagless count-grep should match
assert "Test 24d regex matches flagless count-grep invocation" \
    bash -c 'printf "%s%s\n" "grep" " -c pattern" | grep -qE "$1"' _ "$_24d_regex"

# negative: count-grep -cF should NOT match
assert "Test 24d regex does not match count-grep -cF invocation" \
    bash -c '! printf "%s%s\n" "grep" " -cF pattern" | grep -qE "$1"' _ "$_24d_regex"

# -- Meta: Test 24b sanity failures use a distinct exit code (not the default) -
# Both sanity checks must be updated so a precondition failure is distinguishable
# from a normal assertion failure at the bash-c level. Each sanity call site is
# guarded independently with grep -qF, so reverting either line alone still
# fails loudly. The needle is assembled via bash string concatenation of two
# halves (first half ends in 'ex', second half begins with 'it') so that the
# assertion line itself does not contain the full distinct-code literal and
# cannot self-match.
assert "Test 24b sanity branch 1 (unmatched case) uses exit 2 distinct code" \
    bash -c 'target="ex""it 2 ;;  # sanity: local"; grep -qF "$target" "$1"' _ "${BASH_SOURCE[0]}"
assert "Test 24b sanity branch 2 (count check) uses exit 2 distinct code" \
    bash -c 'target="|| ex""it 2  # sanity: expected"; grep -qF "$target" "$1"' _ "${BASH_SOURCE[0]}"

# -- Test 25a: structural: SAFETY_NET_GREP_LINE marker present ---------------
echo ""
echo "--- Test 25a: structural: SAFETY_NET_GREP_LINE marker is present ---"

# The safety-net cleanup comment (Test 16a, near the critical grep pipeline)
# must carry a stable SAFETY_NET_GREP_LINE marker so meta-tests can locate
# the grep by marker rather than brittle comment prose.
# Use a regex anchored to a comment line (^spaces#space) so the grep command
# itself — which does not start with '#' — is not a self-referential match.
assert "SAFETY_NET_GREP_LINE comment marker exists in file" \
    grep -qE '^[[:space:]]+#[[:space:]]SAFETY_NET_GREP_LINE' "${BASH_SOURCE[0]}"

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
