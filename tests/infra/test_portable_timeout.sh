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

# -- Summary ------------------------------------------------------------------
test_summary
