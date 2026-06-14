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
