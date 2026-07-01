#!/usr/bin/env bash
# tests/infra/load_tolerance_lib.sh — load-tolerant poll budget helper.
#
# WHY THIS LIB (task 4585 / esc-4224-39 / esc-4580-49):
# The post-merge gate runs tests/infra/run_all.sh under DF_VERIFY_ROLE=merge,
# which is deliberately EXEMPT from psi_gate() and the held-slot test semaphore
# (PRD §2 D5 — the merge gate must never wait behind a task slot).  Under heavy
# concurrent task-verify load (host 32c, orchestrator concurrency 24) the
# load-sensitive polling loops in test_portable_timeout.sh Test 16a-class used a
# FIXED 60×200ms (~12s) window — already bumped 3× (5→15→30→60) as whack-a-mole.
#
# This lib ends the per-test-bump cycle by scaling poll budgets off a measured
# signal (/proc/loadavg[1min] ÷ nproc → integer factor) rather than a fixed
# window.  The window only GROWS under load, never shrinks below the
# historically-tuned base; on idle hosts factor=1 so existing assertions are
# byte-for-byte preserved.
#
# USAGE (in a caller like test_portable_timeout.sh):
#   source "$SCRIPT_DIR/load_tolerance_lib.sh"
#   _POLL_ATTEMPTS=$(load_tolerant_attempts 60)   # 60 × factor
#   for ((_a=1; _a<=_POLL_ATTEMPTS; _a++)); do ...done
#
# DESIGN (mirrors psi_gate() + occt_flock_gate_lib.sh conventions):
#   load_tolerance_factor  → echo integer in [1, CAP]
#   load_tolerant_attempts BASE → echo BASE × factor (or BASE if invalid)
#
# ENV KNOBS (all optional; override for unit tests or per-call tuning):
#   REIFY_LOAD_TOLERANCE_FACTOR   — force exact factor (positive int); clamped to CAP
#   REIFY_LOAD_TOLERANCE_LOADAVG  — synthetic 1-min load avg (overrides /proc/loadavg)
#   REIFY_LOAD_TOLERANCE_NPROC    — synthetic nproc (overrides `nproc`)
#   REIFY_LOAD_TOLERANCE_CAP      — ceiling for factor (default 8)
#
# FAIL-SAFE: if LA or NP are unreadable/empty/zero/non-numeric, factor=1 (no
# change from base). Matches psi_gate()'s fail-open philosophy for /proc sources.

# Source guard — prevent double-sourcing.
if [ "${_REIFY_LOAD_TOLERANCE_LIB_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_LOAD_TOLERANCE_LIB_SH_SOURCED=1

# load_tolerance_factor
# Echo an integer in [1, CAP] representing the scaling factor for poll budgets.
# CAP defaults to REIFY_LOAD_TOLERANCE_CAP (default 8).
# If REIFY_LOAD_TOLERANCE_FACTOR is a positive integer, echo it (clamped to CAP).
# Otherwise compute factor = clamp(ceil(LA / NP), 1, CAP).
# Fail-safe to 1 if LA or NP are absent, empty, zero, or non-numeric.
load_tolerance_factor() {
    local _cap="${REIFY_LOAD_TOLERANCE_CAP:-8}"
    # Validate cap is a positive integer; fall back to 8 if not.
    case "$_cap" in
        ''|*[!0-9]*) _cap=8 ;;
    esac
    [ "$_cap" -gt 0 ] 2>/dev/null || _cap=8

    # REIFY_LOAD_TOLERANCE_FACTOR override: use verbatim (clamped to cap).
    local _forced="${REIFY_LOAD_TOLERANCE_FACTOR:-}"
    if [ -n "$_forced" ]; then
        case "$_forced" in
            ''|*[!0-9]*) _forced="" ;;
        esac
        if [ -n "$_forced" ] && [ "$_forced" -gt 0 ] 2>/dev/null; then
            if [ "$_forced" -gt "$_cap" ]; then
                echo "$_cap"
            else
                echo "$_forced"
            fi
            return 0
        fi
    fi

    # Read loadavg: env override or field 1 of /proc/loadavg.
    # ${var+set} expands to "set" if var is defined (even empty), "" if unset —
    # safe under set -u. If the override is explicitly set (even to ""), use it
    # directly; empty triggers the fail-safe below. Only fall through to
    # /proc/loadavg when the override is genuinely unset.
    local _la
    if [ -n "${REIFY_LOAD_TOLERANCE_LOADAVG+set}" ]; then
        _la="${REIFY_LOAD_TOLERANCE_LOADAVG:-}"
    elif [ -r /proc/loadavg ]; then
        # /proc/loadavg: "1.23 0.45 0.67 2/512 12345" — take field 1.
        _la="$(awk '{print $1}' /proc/loadavg 2>/dev/null || true)"
    else
        _la=""
    fi

    # Read nproc: env override or `nproc`.
    # Same ${var+set} idiom: if explicitly set (even empty), use it.
    local _np
    if [ -n "${REIFY_LOAD_TOLERANCE_NPROC+set}" ]; then
        _np="${REIFY_LOAD_TOLERANCE_NPROC:-}"
    else
        _np="$(nproc 2>/dev/null || true)"
    fi

    # Fail-safe: empty, non-numeric, or zero nproc → factor 1.
    case "$_np" in
        ''|*[!0-9]*) echo 1; return 0 ;;
    esac
    if [ "$_np" -le 0 ] 2>/dev/null; then
        echo 1; return 0
    fi

    # Fail-safe: empty or non-numeric loadavg → factor 1.
    # loadavg is a float (e.g. "1.23"); validate via awk.
    if [ -z "$_la" ]; then
        echo 1; return 0
    fi
    local _la_valid
    _la_valid="$(printf '%s' "$_la" | awk '{if ($1+0 == $1 && $1 != "") print "ok"}' 2>/dev/null || true)"
    if [ "$_la_valid" != "ok" ]; then
        echo 1; return 0
    fi

    # Compute factor = clamp(ceil(LA / NP), 1, CAP) using awk (locale-safe float → int ceil).
    local _factor
    _factor="$(awk -v la="$_la" -v np="$_np" -v cap="$_cap" 'BEGIN {
        f = la / np
        i = int(f)
        if (f > i) i = i + 1
        if (i < 1) i = 1
        if (i > cap) i = cap
        print i
    }' 2>/dev/null || echo 1)"

    echo "${_factor:-1}"
}

# quiet_box_met AVG10 CEILING
# Returns 0 (quiet, proceed) when:
#   - AVG10 is empty or the literal "unavailable" (fail-open — mirrors
#     load_tolerance_factor / psi_gate fail-open philosophy for unreadable
#     /proc sources); OR
#   - AVG10 is non-numeric (fail-open — cannot measure means do not block); OR
#   - AVG10 < CEILING (strictly less than — the box is quiet enough to measure).
# Returns 1 (hot, caller SKIPs) when AVG10 is a valid number AND AVG10 >= CEILING.
#
# Used by ROW4 of test_cpu_load_governance.sh to gate the proportional-share
# assertion behind the quiet-box precondition (PRD §8 row 4: 'others quiet').
# Callers supply the already-sampled avg10 string so this function is pure
# (no I/O) and hermetically testable with synthetic inputs via the caller's
# env-injection idiom (see test_load_tolerance_lib.sh Test 13).
#
# Uses awk for float comparison and the numeric-validity guard, mirroring the
# `$1+0 == $1 && $1 != ""` idiom already used in load_tolerance_factor.
quiet_box_met() {
    local _avg10="${1:-}"
    local _ceiling="${2:-20}"

    # Fail-open: empty or literal "unavailable" → quiet (proceed).
    if [ -z "$_avg10" ] || [ "$_avg10" = "unavailable" ]; then
        return 0
    fi

    # Use awk for float comparison and numeric-validity guard.
    # Non-numeric avg10 → fail-open (proceed).  Valid number >= ceiling → hot.
    # Use awk to validate numeric and compare (mirrors load_tolerance_factor
    # idiom).  Prints "hot" only when avg10 is a valid number and avg10 >=
    # ceiling; "quiet" otherwise (fail-open for non-numeric avg10).
    # Captures output as a string so `|| echo quiet` handles awk-not-found
    # without confusing awk's intentional exit code semantics.
    local _result
    _result="$(awk -v a="$_avg10" -v c="$_ceiling" 'BEGIN {
        if (a+0 == a && a != "") {
            print (a+0 >= c+0) ? "hot" : "quiet"
        } else {
            print "quiet"
        }
    }' 2>/dev/null || echo "quiet")"

    [ "${_result:-quiet}" = "hot" ] && return 1 || return 0
}

# load_tolerant_attempts BASE
# Echo BASE × load_tolerance_factor.
# If BASE is not a positive integer, echo BASE unchanged (safe degradation).
load_tolerant_attempts() {
    local _base="${1:-}"
    # Validate BASE is a non-empty positive integer.
    case "$_base" in
        ''|*[!0-9]*)
            echo "$_base"
            return 0
            ;;
    esac
    if ! [ "$_base" -gt 0 ] 2>/dev/null; then
        echo "$_base"
        return 0
    fi

    local _factor
    _factor="$(load_tolerance_factor)"
    echo $(( _base * _factor ))
}

# ---------------------------------------------------------------------------
# Keepalive core (task 4931 / esc-3002-145 FIX #2) — throttled
# @@REIFY_CLOCK_HEARTBEAT@@ emission from inside load-scaled poll loops so a
# long-but-alive poll (up to ~240s in test_proc_reaper.sh teardown polls, up
# to ~600s in test_verify_semaphore_e2e.sh's _wait_for_marker) does not trip
# dark-factory's clock-stop heartbeat-idle backstop
# (verify_clock_stop_heartbeat_idle_max=180s per
# docs/prds/verify-admission-wait-clock-stop.md) into a FALSE 'wedged' kill.
#
# load_tolerant_keepalive [reason]
#   Emits a throttled HEARTBEAT marker (reason defaults to `load_scaled_poll`).
#   Call from inside a poll loop; fires at most once per
#   max(1, REIFY_CLOCK_HEARTBEAT_SECS:-30) seconds.  No-op (silent, returns 0)
#   when reason is empty or REIFY_KEEPALIVE_DISABLE=1.  Routed to
#   ${REIFY_KEEPALIVE_FD:-2}, falling back to real stderr (fd 2) if that fd is
#   not open (e.g. the caller never preserved it via `exec N>&2`).
#
# load_tolerant_sleep_tick [reason]
#   Poll-loop tick primitive: `sleep 1` followed by load_tolerant_keepalive.
#   Drop-in replacement for a bare `sleep 1` inside a load-scaled poll loop.
#
# ENV KNOBS:
#   REIFY_CLOCK_HEARTBEAT_SECS  — throttle interval in seconds (default 30;
#                                 shared with scripts/lib_clock_stop.sh)
#   REIFY_KEEPALIVE_FD          — target fd for the marker (default 2 = stderr)
#   REIFY_KEEPALIVE_DISABLE     — set to 1 to silence load_tolerant_keepalive
#
# Grammar reuse: sources scripts/lib_clock_stop.sh (the single source of
# truth for the @@REIFY_CLOCK_HEARTBEAT@@ wire grammar — the two-way contract
# with dark_factory:1916) so the marker text never drifts.  Guarded: tolerates
# absence by falling back to an inline printf of the identical grammar.
_reify_ltl_clock_lib="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/../../scripts/lib_clock_stop.sh"
if [ -f "$_reify_ltl_clock_lib" ]; then
    # shellcheck disable=SC1090,SC1091
    source "$_reify_ltl_clock_lib"
fi
unset _reify_ltl_clock_lib

# Module-scoped throttle state — persists across calls within one shell.
# The source guard at the top of this file prevents a second source() from
# resetting an in-progress throttle window.
_REIFY_KEEPALIVE_START_EPOCH=""
_REIFY_KEEPALIVE_LAST_EMIT_EPOCH=""

# _reify_keepalive_write REASON WAITED
#   Build the HEARTBEAT line (via clock_emit_heartbeat when lib_clock_stop.sh
#   was sourced; else an identical inline printf) and write it to
#   ${REIFY_KEEPALIVE_FD:-2}, guarded so an unopened fd falls back to real
#   stderr instead of erroring.
_reify_keepalive_write() {
    local _reason="$1" _waited="$2" _fd="${REIFY_KEEPALIVE_FD:-2}" _line
    if declare -F clock_emit_heartbeat >/dev/null 2>&1; then
        _line="$(clock_emit_heartbeat "$_reason" "$_waited" 2>&1)"
    else
        _line="$(printf '@@REIFY_CLOCK_HEARTBEAT@@ reason=%s waited=%s' "$_reason" "$_waited")"
    fi
    # fd 2 (real stderr) is the common default and is always open, so write
    # directly — wrapping it in the same guard below would be self-defeating:
    # `{ printf ... >&2; } 2>/dev/null` redirects the GROUP's fd2 to /dev/null
    # BEFORE the inner `>&2` executes, silently swallowing the marker.
    if [ "$_fd" = "2" ]; then
        printf '%s\n' "$_line" >&2
    else
        { printf '%s\n' "$_line" >&"$_fd"; } 2>/dev/null || printf '%s\n' "$_line" >&2
    fi
}

load_tolerant_keepalive() {
    # ${1-default} (no colon): default applies only when $1 is UNSET (no arg
    # passed) — NOT when it is explicitly the empty string. ${1:-default}
    # would collapse an explicit "" into the default too, defeating the
    # empty-reason no-op contract below.
    local _reason="${1-load_scaled_poll}"
    [ -n "$_reason" ] || return 0
    [ "${REIFY_KEEPALIVE_DISABLE:-}" = "1" ] && return 0

    local _interval="${REIFY_CLOCK_HEARTBEAT_SECS:-30}"
    case "$_interval" in
        ''|*[!0-9]*) _interval=30 ;;
    esac
    [ "$_interval" -ge 1 ] 2>/dev/null || _interval=30

    local _now
    _now="$(date +%s)"
    [ -n "$_REIFY_KEEPALIVE_START_EPOCH" ] || _REIFY_KEEPALIVE_START_EPOCH="$_now"
    local _last="${_REIFY_KEEPALIVE_LAST_EMIT_EPOCH:-0}"

    if [ $(( _now - _last )) -ge "$_interval" ]; then
        _reify_keepalive_write "$_reason" "$(( _now - _REIFY_KEEPALIVE_START_EPOCH ))"
        _REIFY_KEEPALIVE_LAST_EMIT_EPOCH="$_now"
    fi
    return 0
}

load_tolerant_sleep_tick() {
    sleep 1
    load_tolerant_keepalive "$@"
}
