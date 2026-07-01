#!/usr/bin/env bash
# Unit tests for tests/infra/load_tolerance_lib.sh.
#
# All assertions use DETERMINISTIC integer arithmetic over synthetic injected
# inputs (REIFY_LOAD_TOLERANCE_LOADAVG / _NPROC / _FACTOR / _CAP) — no real
# load, no sleeps, cannot flake.
#
# Auto-discovered by run_all.sh (matches test_*.sh).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LIB="$SCRIPT_DIR/load_tolerance_lib.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== load_tolerance_lib.sh unit tests ==="

# -- Existence guard: lib must exist before sourcing ---------------------------
echo ""
echo "--- Existence: load_tolerance_lib.sh exists ---"

assert "load_tolerance_lib.sh file exists" \
    test -f "$LIB"

# Source the lib (fails the whole test if it doesn't exist).
if ! [ -f "$LIB" ]; then
    echo "FATAL: load_tolerance_lib.sh not found at $LIB — skipping remaining tests"
    test_summary
fi
source "$LIB"

# -- Test 1: load_tolerance_factor — idle floor (loadavg=1, nproc=32) ----------
echo ""
echo "--- Test 1: load_tolerance_factor — idle floor ---"

assert "factor=1 at loadavg=1 nproc=32 (idle)" \
    env REIFY_LOAD_TOLERANCE_LOADAVG=1 REIFY_LOAD_TOLERANCE_NPROC=32 \
    bash -c 'source "$1" && f=$(load_tolerance_factor) && [ "$f" -eq 1 ]' _ "$LIB"

# -- Test 2: load_tolerance_factor — factor 2 at double load ------------------
echo ""
echo "--- Test 2: load_tolerance_factor — factor 2 at loadavg=64, nproc=32 ---"

assert "factor=2 at loadavg=64 nproc=32" \
    env REIFY_LOAD_TOLERANCE_LOADAVG=64 REIFY_LOAD_TOLERANCE_NPROC=32 \
    bash -c 'source "$1" && f=$(load_tolerance_factor) && [ "$f" -eq 2 ]' _ "$LIB"

# -- Test 3: load_tolerance_factor — factor 3 at triple load ------------------
echo ""
echo "--- Test 3: load_tolerance_factor — factor 3 at loadavg=96, nproc=32 ---"

assert "factor=3 at loadavg=96 nproc=32" \
    env REIFY_LOAD_TOLERANCE_LOADAVG=96 REIFY_LOAD_TOLERANCE_NPROC=32 \
    bash -c 'source "$1" && f=$(load_tolerance_factor) && [ "$f" -eq 3 ]' _ "$LIB"

# -- Test 4: load_tolerance_factor — capped at CAP=8 at extreme load ----------
echo ""
echo "--- Test 4: load_tolerance_factor — capped at CAP=8 at extreme load ---"

assert "factor=8 (cap) at loadavg=1000 nproc=32" \
    env REIFY_LOAD_TOLERANCE_LOADAVG=1000 REIFY_LOAD_TOLERANCE_NPROC=32 \
    bash -c 'source "$1" && f=$(load_tolerance_factor) && [ "$f" -eq 8 ]' _ "$LIB"

# -- Test 5: load_tolerance_factor — ceil (rounds UP) -------------------------
echo ""
echo "--- Test 5: load_tolerance_factor — ceil rounds up (loadavg=65, nproc=32) ---"
# 65 / 32 = 2.03125 → ceil → 3

assert "factor=3 at loadavg=65 nproc=32 (ceil)" \
    env REIFY_LOAD_TOLERANCE_LOADAVG=65 REIFY_LOAD_TOLERANCE_NPROC=32 \
    bash -c 'source "$1" && f=$(load_tolerance_factor) && [ "$f" -eq 3 ]' _ "$LIB"

# -- Test 6: REIFY_LOAD_TOLERANCE_FACTOR override (bypass loadavg/nproc) ------
echo ""
echo "--- Test 6: REIFY_LOAD_TOLERANCE_FACTOR override forces exact factor ---"

assert "FACTOR=5 override forces factor=5" \
    env REIFY_LOAD_TOLERANCE_FACTOR=5 REIFY_LOAD_TOLERANCE_LOADAVG=1 REIFY_LOAD_TOLERANCE_NPROC=32 \
    bash -c 'source "$1" && f=$(load_tolerance_factor) && [ "$f" -eq 5 ]' _ "$LIB"

# -- Test 7: load_tolerant_attempts — BASE × factor ---------------------------
echo ""
echo "--- Test 7: load_tolerant_attempts — BASE × factor ---"

assert "load_tolerant_attempts 60 = 60 at idle (factor=1)" \
    env REIFY_LOAD_TOLERANCE_LOADAVG=1 REIFY_LOAD_TOLERANCE_NPROC=32 \
    bash -c 'source "$1" && a=$(load_tolerant_attempts 60) && [ "$a" -eq 60 ]' _ "$LIB"

assert "load_tolerant_attempts 60 = 120 at loadavg=64/nproc=32 (factor=2)" \
    env REIFY_LOAD_TOLERANCE_LOADAVG=64 REIFY_LOAD_TOLERANCE_NPROC=32 \
    bash -c 'source "$1" && a=$(load_tolerant_attempts 60) && [ "$a" -eq 120 ]' _ "$LIB"

assert "load_tolerant_attempts 60 = 300 with FACTOR=5 override" \
    env REIFY_LOAD_TOLERANCE_FACTOR=5 \
    bash -c 'source "$1" && a=$(load_tolerant_attempts 60) && [ "$a" -eq 300 ]' _ "$LIB"

# -- Test 8: fail-safe — empty/unreadable loadavg → factor=1, attempts=BASE --
echo ""
echo "--- Test 8: fail-safe — empty loadavg or nproc=0 → factor=1 ---"

assert "factor=1 when REIFY_LOAD_TOLERANCE_LOADAVG is empty" \
    env REIFY_LOAD_TOLERANCE_LOADAVG="" REIFY_LOAD_TOLERANCE_NPROC=32 \
    bash -c 'source "$1" && f=$(load_tolerance_factor) && [ "$f" -eq 1 ]' _ "$LIB"

assert "factor=1 when REIFY_LOAD_TOLERANCE_NPROC is empty" \
    env REIFY_LOAD_TOLERANCE_LOADAVG=64 REIFY_LOAD_TOLERANCE_NPROC="" \
    bash -c 'source "$1" && f=$(load_tolerance_factor) && [ "$f" -eq 1 ]' _ "$LIB"

assert "factor=1 when REIFY_LOAD_TOLERANCE_NPROC=0" \
    env REIFY_LOAD_TOLERANCE_LOADAVG=64 REIFY_LOAD_TOLERANCE_NPROC=0 \
    bash -c 'source "$1" && f=$(load_tolerance_factor) && [ "$f" -eq 1 ]' _ "$LIB"

assert "load_tolerant_attempts 60 = 60 (BASE) when nproc=0 (fail-safe)" \
    env REIFY_LOAD_TOLERANCE_LOADAVG=64 REIFY_LOAD_TOLERANCE_NPROC=0 \
    bash -c 'source "$1" && a=$(load_tolerant_attempts 60) && [ "$a" -eq 60 ]' _ "$LIB"

# -- Test 9: REIFY_LOAD_TOLERANCE_CAP override ---------------------------------
echo ""
echo "--- Test 9: REIFY_LOAD_TOLERANCE_CAP override changes ceiling ---"

assert "CAP=4 caps factor at 4 even at extreme load" \
    env REIFY_LOAD_TOLERANCE_LOADAVG=1000 REIFY_LOAD_TOLERANCE_NPROC=32 REIFY_LOAD_TOLERANCE_CAP=4 \
    bash -c 'source "$1" && f=$(load_tolerance_factor) && [ "$f" -eq 4 ]' _ "$LIB"

assert "FACTOR=10 clamped to CAP=4 when CAP override is set" \
    env REIFY_LOAD_TOLERANCE_FACTOR=10 REIFY_LOAD_TOLERANCE_CAP=4 \
    bash -c 'source "$1" && f=$(load_tolerance_factor) && [ "$f" -eq 4 ]' _ "$LIB"

# -- Test 10: bad BASE for load_tolerant_attempts — echoes BASE unchanged -----
echo ""
echo "--- Test 10: load_tolerant_attempts with bad BASE is safe ---"

assert "load_tolerant_attempts with non-integer BASE echoes BASE unchanged" \
    env REIFY_LOAD_TOLERANCE_FACTOR=2 \
    bash -c 'source "$1" && a=$(load_tolerant_attempts "notanint") && [ "$a" = "notanint" ]' _ "$LIB"

assert "load_tolerant_attempts with empty BASE echoes empty or 0 safely" \
    env REIFY_LOAD_TOLERANCE_FACTOR=2 \
    bash -c 'source "$1" && a=$(load_tolerant_attempts "") && [ -z "$a" ] || [ "$a" = "0" ]' _ "$LIB"

# -- Test 11: source guard — double-source is a no-op -------------------------
echo ""
echo "--- Test 11: source guard _REIFY_LOAD_TOLERANCE_LIB_SH_SOURCED ---"

assert "double-sourcing load_tolerance_lib.sh is a no-op (guard works)" \
    bash -c 'source "$1" && source "$1" && declare -f load_tolerance_factor >/dev/null' _ "$LIB"

# -- Test 12: fractional loadavg — mirrors real /proc/loadavg input -----------
# /proc/loadavg always emits a float like "2.50"; Tests 1-5 only inject integers.
# These assertions exercise the awk float-division/ceil path and the _la_valid
# numeric validator ($1+0 == $1) with values that cannot appear in a pure-integer
# test: the intermediate ratio is non-integer AND the dividend itself is a float.
#   48.5 / 32 = 1.515625 → ceil → 2
#   31.9 / 32 = 0.996875 → ceil → 1  (below 1, clamped to 1)
echo ""
echo "--- Test 12: fractional loadavg — real /proc format (floats) ---"

assert "factor=2 at loadavg=48.5 nproc=32 (float input, ceil(1.515)=2)" \
    env REIFY_LOAD_TOLERANCE_LOADAVG=48.5 REIFY_LOAD_TOLERANCE_NPROC=32 \
    bash -c 'source "$1" && f=$(load_tolerance_factor) && [ "$f" -eq 2 ]' _ "$LIB"

assert "factor=1 at loadavg=31.9 nproc=32 (float input, ceil(0.997)=1)" \
    env REIFY_LOAD_TOLERANCE_LOADAVG=31.9 REIFY_LOAD_TOLERANCE_NPROC=32 \
    bash -c 'source "$1" && f=$(load_tolerance_factor) && [ "$f" -eq 1 ]' _ "$LIB"

# -- Test 13: quiet_box_met — deterministic quiet-box precondition predicate --
# All cases use synthetic inputs (no real load, no /proc reads) — cannot flake.
# The exact-exit-code check (`[ "$rc" -eq N ]`) means an undefined function
# (rc=127) fails every assertion — a genuine RED before quiet_box_met exists.
echo ""
echo "--- Test 13: quiet_box_met — quiet-box precondition predicate ---"

assert "quiet_box_met 5 20 -> rc 0 (quiet: avg10 < ceiling -> proceed)" \
    bash -c 'source "$1"; quiet_box_met 5 20; rc=$?; [ "$rc" -eq 0 ]' _ "$LIB"

assert "quiet_box_met 25 20 -> rc 1 (hot: avg10 >= ceiling -> caller SKIPs)" \
    bash -c 'source "$1"; quiet_box_met 25 20; rc=$?; [ "$rc" -eq 1 ]' _ "$LIB"

assert "quiet_box_met 20 20 -> rc 1 (boundary: NOT strictly below -> hot)" \
    bash -c 'source "$1"; quiet_box_met 20 20; rc=$?; [ "$rc" -eq 1 ]' _ "$LIB"

assert "quiet_box_met 19.9 20 -> rc 0 (float quiet)" \
    bash -c 'source "$1"; quiet_box_met 19.9 20; rc=$?; [ "$rc" -eq 0 ]' _ "$LIB"

assert "quiet_box_met unavailable 20 -> rc 0 (fail-open: cannot measure -> proceed)" \
    bash -c 'source "$1"; quiet_box_met unavailable 20; rc=$?; [ "$rc" -eq 0 ]' _ "$LIB"

assert 'quiet_box_met "" 20 -> rc 0 (fail-open: empty avg10 -> proceed)' \
    bash -c 'source "$1"; quiet_box_met "" 20; rc=$?; [ "$rc" -eq 0 ]' _ "$LIB"

assert "quiet_box_met garbage 20 -> rc 0 (fail-open: non-numeric avg10 -> proceed)" \
    bash -c 'source "$1"; quiet_box_met garbage 20; rc=$?; [ "$rc" -eq 0 ]' _ "$LIB"

# -- Test 14: load_tolerant_keepalive / load_tolerant_sleep_tick (FIX #2 core) --
# task 4931 / esc-3002-145: throttled HEARTBEAT-marker keepalive emitter for
# load-scaled poll loops (prevents dark-factory's 180s heartbeat-idle backstop
# from mistaking a long-but-alive poll for a wedge).  RED today: neither
# helper is defined yet.
echo ""
echo "--- Test 14: load_tolerant_keepalive / load_tolerant_sleep_tick (FIX #2 core, task 4931) ---"

assert "load_tolerance_lib.sh defines load_tolerant_keepalive()" \
    bash -c 'grep -qF "load_tolerant_keepalive()" "$1"' _ "$LIB"

assert "load_tolerance_lib.sh defines load_tolerant_sleep_tick()" \
    bash -c 'grep -qF "load_tolerant_sleep_tick()" "$1"' _ "$LIB"

# (a) grammar: the exact HEARTBEAT marker line (reason=<given>, waited=<int>),
# captured via a routed fd to a file — never via the assert() description
# (esc-4789-63 / assert_marker convention: the token rides only in the
# suppressed grep-command argument below).
assert "load_tolerant_keepalive writes the HEARTBEAT marker grammar (reason=<given>, waited=<int>) to the routed fd" \
    bash -c '
        source "$1"
        _cap=$(mktemp)
        exec 4>"$_cap"
        REIFY_KEEPALIVE_FD=4 load_tolerant_keepalive mytestreason
        exec 4>&-
        _rc=1
        grep -qE "^@@REIFY_CLOCK_HEARTBEAT@@ reason=mytestreason waited=[0-9]+\$" "$_cap" && _rc=0
        rm -f "$_cap"
        exit "$_rc"
    ' _ "$LIB"

assert "load_tolerant_keepalive with no reason arg defaults reason to load_scaled_poll" \
    bash -c '
        source "$1"
        _cap=$(mktemp)
        exec 4>"$_cap"
        REIFY_KEEPALIVE_FD=4 load_tolerant_keepalive
        exec 4>&-
        _rc=1
        grep -qE "^@@REIFY_CLOCK_HEARTBEAT@@ reason=load_scaled_poll waited=[0-9]+\$" "$_cap" && _rc=0
        rm -f "$_cap"
        exit "$_rc"
    ' _ "$LIB"

# (b) throttle: two rapid successive calls emit exactly ONE marker; two calls
# spaced > REIFY_CLOCK_HEARTBEAT_SECS apart emit TWO.
assert "load_tolerant_keepalive throttles: two rapid successive calls emit exactly one marker" \
    env REIFY_CLOCK_HEARTBEAT_SECS=100 bash -c '
        source "$1"
        _cap=$(mktemp)
        exec 4>"$_cap"
        REIFY_KEEPALIVE_FD=4 load_tolerant_keepalive throttletest
        REIFY_KEEPALIVE_FD=4 load_tolerant_keepalive throttletest
        exec 4>&-
        _n=$(grep -cE "reason=throttletest" "$_cap")
        rm -f "$_cap"
        [ "$_n" -eq 1 ]
    ' _ "$LIB"

assert "load_tolerant_keepalive emits two markers when calls are spaced > REIFY_CLOCK_HEARTBEAT_SECS apart" \
    env REIFY_CLOCK_HEARTBEAT_SECS=1 bash -c '
        source "$1"
        _cap=$(mktemp)
        exec 4>"$_cap"
        REIFY_KEEPALIVE_FD=4 load_tolerant_keepalive spacedtest
        sleep 1.2
        REIFY_KEEPALIVE_FD=4 load_tolerant_keepalive spacedtest
        exec 4>&-
        _n=$(grep -cE "reason=spacedtest" "$_cap")
        rm -f "$_cap"
        [ "$_n" -eq 2 ]
    ' _ "$LIB"

# (c) FD routing: an open fd receives the marker; with REIFY_KEEPALIVE_FD
# unset it falls back to real stderr (fd 2).
assert "load_tolerant_keepalive routes the marker to REIFY_KEEPALIVE_FD when it points at an open fd" \
    bash -c '
        source "$1"
        _cap=$(mktemp)
        exec 5>"$_cap"
        REIFY_KEEPALIVE_FD=5 load_tolerant_keepalive fdtest
        exec 5>&-
        _n=$(grep -cE "reason=fdtest" "$_cap")
        rm -f "$_cap"
        [ "$_n" -eq 1 ]
    ' _ "$LIB"

assert "load_tolerant_keepalive with REIFY_KEEPALIVE_FD unset writes the marker to stderr (fd 2)" \
    bash -c '
        source "$1"
        unset REIFY_KEEPALIVE_FD
        _err=$(mktemp)
        load_tolerant_keepalive fdtest2 2>"$_err"
        _rc=1
        grep -qE "reason=fdtest2" "$_err" && _rc=0
        rm -f "$_err"
        exit "$_rc"
    ' _ "$LIB"

# (d) no-op: empty reason, and REIFY_KEEPALIVE_DISABLE=1 — no stderr output.
assert "load_tolerant_keepalive is a no-op (no output) when reason is explicitly empty" \
    bash -c '
        source "$1"
        _err=$(mktemp)
        load_tolerant_keepalive "" 2>"$_err"
        _n=$(wc -l < "$_err" | tr -d " ")
        rm -f "$_err"
        [ "${_n:-0}" -eq 0 ]
    ' _ "$LIB"

assert "load_tolerant_keepalive is a no-op (no output) when REIFY_KEEPALIVE_DISABLE=1" \
    env REIFY_KEEPALIVE_DISABLE=1 bash -c '
        source "$1"
        _err=$(mktemp)
        load_tolerant_keepalive somereason 2>"$_err"
        _n=$(wc -l < "$_err" | tr -d " ")
        rm -f "$_err"
        [ "${_n:-0}" -eq 0 ]
    ' _ "$LIB"

# (e) loop behavior / suppression-escape: a poll loop calling
# load_tolerant_sleep_tick over a wait longer than the interval still emits
# >=1 marker to the routed fd even when the loop body's own stdout+stderr are
# suppressed (mirrors assert()'s own `"$@" >/dev/null 2>&1` wrapping in
# test_helpers.sh) — the property the real wiring (step-5/6) depends on.
assert "load_tolerant_sleep_tick poll loop escapes assert()-style >/dev/null 2>&1 suppression via the routed fd" \
    env REIFY_CLOCK_HEARTBEAT_SECS=1 bash -c '
        source "$1"
        _cap=$(mktemp)
        exec 6>"$_cap"
        (
            export REIFY_KEEPALIVE_FD=6
            load_tolerant_sleep_tick suppresstest
            load_tolerant_sleep_tick suppresstest
        ) >/dev/null 2>&1
        exec 6>&-
        _n=$(grep -cE "reason=suppresstest" "$_cap")
        rm -f "$_cap"
        [ "$_n" -ge 1 ]
    ' _ "$LIB"

# -- Summary -------------------------------------------------------------------
test_summary
