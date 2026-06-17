#!/usr/bin/env bash
# scripts/cpu-admit.sh — shared PSI-admission core for Reify.
#
# Designed to be sourced by verify.sh (psi_gate / compile_gate wrappers) AND
# directly executable as `cpu-admit.sh <mode>` (agent cargo shim β and tests).
# Structure mirrors scripts/lib_test_semaphore.sh: source-guard + function defs
# + main-guard.
#
# FUNCTIONS (defined when sourced):
#   cpu_admit_read_avg10 <proc_path>
#       Parse avg10 from a /proc/pressure/cpu-formatted file.  Echoes the
#       numeric string (e.g. "42.50") on success; echoes "" on any error.
#       Moved verbatim from verify.sh _psi_read_avg10.
#
#   cpu_admit <mode>
#       Unified PSI-admission gate.  mode must be 'admit' or 'requeue'.
#       Returns 0 on pass, 75 (EX_TEMPFAIL) on requeue-timeout, 64 on bad mode.
#       Caller sets _ca_* variables (see CONTRACT below) before calling.
#
# CALLER CONTRACT (_ca_* variables — set by calling function before cpu_admit):
#   _ca_threshold      avg10 ceiling (numeric %, no nproc constant; host-portable)
#   _ca_max_wait       timeout in seconds
#   _ca_poll           recheck interval in seconds (clamped to >= 1 internally)
#   _ca_proc_path      PSI source path (typically /proc/pressure/cpu)
#   _ca_disable        set to "1" for total bypass (no dispatch touch, no wait)
#   _ca_window         min seconds between dispatches (empty = no window check)
#   _ca_dispatch       dispatch coordination file path (empty = no coordination)
#   _ca_log_prefix     stderr message prefix (e.g. "verify.sh" or "cpu-admit")
#   _ca_gate_name      gate name for messages (e.g. "PSI gate" / "compile-gate" / "")
#   _ca_failopen_txt   phrase in the fail-open WARNING line (e.g. "PSI gate disabled")
#
# BEHAVIOR (PRD §4.1 C-A1..C-A5):
#   C-A1 work-conserving: pass immediately when avg10 < _ca_threshold.
#   C-A3 merge bypass: DF_VERIFY_ROLE=merge → immediate pass (touches dispatch if set).
#   C-A4 fail-open: unreadable _ca_proc_path → pass + warn.
#   C-A5 pressure-reactive only: no fixed-count semaphore (lib_test_semaphore.sh
#        stays scoped to the verify test×test region; _ca_window/_ca_dispatch are
#        optional time-spacing — they do NOT add a fixed cap).
#   admit mode: admit-on-timeout (return 0 + warning) — NEVER exit 75.
#   requeue mode: exit-75-on-timeout (EX_TEMPFAIL → orchestrator requeues).
#
# DIRECT-EXEC KNOBS (CLI / agent path — no window/dispatch; pure pressure-reactive):
#   REIFY_CPU_ADMIT_THRESHOLD   avg10 ceiling (default 50)
#   REIFY_CPU_ADMIT_MAX_WAIT    timeout in seconds (default 300)
#   REIFY_CPU_ADMIT_POLL        recheck interval in seconds (default 5)
#   REIFY_CPU_ADMIT_PROC_PATH   PSI source (default /proc/pressure/cpu)
#   REIFY_CPU_ADMIT_DISABLE     set to 1 for total bypass (break-glass)

# Source guard — prevent double-sourcing.
if [ "${_REIFY_CPU_ADMIT_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_CPU_ADMIT_SH_SOURCED=1

# ---------------------------------------------------------------------------
# cpu_admit_read_avg10 <proc_path>
# Parse the avg10 value from a /proc/pressure/cpu-formatted file.
# Echoes the numeric avg10 string (e.g. "42.50") on success; echoes the empty
# string on parse failure, missing file, or any awk error.
# Moved verbatim from verify.sh _psi_read_avg10 (renamed for cpu-admit.sh scope).
# ---------------------------------------------------------------------------
cpu_admit_read_avg10() {
    awk '/^some/ {
        for (i=1; i<=NF; i++) {
            if ($i ~ /^avg10=/) { v=$i; sub(/^avg10=/, "", v); print v; exit }
        }
    }' "$1" 2>/dev/null || echo ""
}

# ---------------------------------------------------------------------------
# _cpu_admit_psi_should_pass <timestamp>
# Helper for cpu_admit's flock-coordinated path.
# Returns 0 if both PSI and window conditions are satisfied (safe to dispatch),
# or 1 otherwise.  Reads _ca_proc_path, _ca_threshold, _ca_window, _ca_dispatch
# from the calling scope (bash dynamic scoping; locals visible to callees).
# Moved verbatim from verify.sh _psi_should_pass (variable names updated).
# ---------------------------------------------------------------------------
_cpu_admit_psi_should_pass() {
    local _ts="$1" _mtime _age _avg10
    _mtime=$(stat -c %Y "$_ca_dispatch" 2>/dev/null || echo 0)
    _age=$(( _ts - _mtime ))
    _avg10="$(cpu_admit_read_avg10 "$_ca_proc_path")"
    [ -n "$_avg10" ] && \
        awk -v p="$_avg10" -v t="$_ca_threshold" 'BEGIN{exit !(p<t)}' && \
        [ "$_age" -ge "$_ca_window" ]
}

# ---------------------------------------------------------------------------
# cpu_admit <mode>
# Unified PSI-admission gate.  Caller sets _ca_* variables before calling;
# see CALLER CONTRACT in the header above.
# ---------------------------------------------------------------------------
cpu_admit() {
    local _mode="$1"

    # Validate mode
    case "$_mode" in
        admit|requeue) ;;
        *)
            echo "${_ca_log_prefix:-cpu-admit}: ERROR — unknown mode '${_mode}' (want admit|requeue)" >&2
            return 64
            ;;
    esac

    # Clamp POLL to a sane minimum: sleep 0 (or an invalid value) causes a
    # tight busy-spin hammering date + cpu_admit_read_avg10 for up to MAX_WAIT.
    local _poll="${_ca_poll:-5}"
    [ "$_poll" -ge 1 ] 2>/dev/null || _poll=1

    # Build a per-message gate tag for consistent prefixing:
    #   non-empty _ca_gate_name → "verify.sh: PSI gate" prefix family
    #   empty _ca_gate_name     → "cpu-admit:" prefix (CLI / agent path)
    local _gate_tag=""
    [ -n "${_ca_gate_name:-}" ] && _gate_tag="${_ca_gate_name} "

    # (1) Break-glass bypass — total bypass: no PSI read, no dispatch touch, no wait.
    if [ "${_ca_disable:-}" = "1" ]; then
        echo "${_ca_log_prefix:-cpu-admit}: ${_gate_tag}disabled" >&2
        return 0
    fi

    # (2) Merge bypass: skip wait + touch dispatch (if set) so the next task backs off.
    # DF_VERIFY_ROLE=merge bypass is enforced here; callers (psi_gate/compile_gate)
    # document this delegation with a comment referencing DF_VERIFY_ROLE=merge.
    if [ "${DF_VERIFY_ROLE:-task}" = "merge" ]; then
        if [ -n "${_ca_dispatch:-}" ]; then
            touch "$_ca_dispatch"
            echo "${_ca_log_prefix:-cpu-admit}: ${_gate_tag}bypass (role=merge) — timestamp bumped" >&2
        else
            echo "${_ca_log_prefix:-cpu-admit}: ${_gate_tag}bypass (role=merge)" >&2
        fi
        return 0
    fi

    # (3) Fail-open on missing/unreadable PSI source (older kernels / non-Linux).
    # Touch the dispatch file (if set) so cross-process coordination stays consistent.
    if [ ! -r "${_ca_proc_path:-/proc/pressure/cpu}" ]; then
        echo "${_ca_log_prefix:-cpu-admit}: WARNING — ${_ca_failopen_txt:-fail-open} — kernel lacks ${_ca_proc_path:-/proc/pressure/cpu}" >&2
        [ -n "${_ca_dispatch:-}" ] && touch "$_ca_dispatch"
        return 0
    fi

    # (4) Poll loop: wait for admission conditions to be satisfied.
    local _deadline
    _deadline=$(( $(date +%s) + ${_ca_max_wait:-300} ))

    while true; do
        local _now _flock_rc
        _now=$(date +%s)
        _flock_rc=10  # not-yet (default: condition not met)

        if [ -n "${_ca_dispatch:-}" ] && [ -n "${_ca_window:-}" ]; then
            # Flock-coordinated path: WINDOW spacing + dispatch-file touch.
            # (psi_gate mode: window + dispatch ON)
            # The read-mtime / compare / touch critical section is wrapped in a
            # flock so concurrent waiters pass one-at-a-time and each pass
            # re-touches — guaranteeing consecutive passes are >= _ca_window apart.
            # Relocated verbatim from verify.sh psi_gate loop (preserving
            # concurrent-burst atomicity; see test_psi_gate.sh Cycle 2).
            if command -v flock >/dev/null 2>&1; then
                # Atomic check-and-touch inside a flock subshell.
                # Exit codes: 0=pass, 9=lock-timeout, 10=not-yet.
                # The subshell exits immediately so the FD is not inherited by
                # long-lived children (no cargo/sccache FD-9-inheritance hazard).
                _flock_rc=0
                (
                    flock -w 5 9 || exit 9
                    _ts=$(date +%s)
                    if _cpu_admit_psi_should_pass "$_ts"; then
                        touch "$_ca_dispatch"
                        exit 0
                    fi
                    exit 10
                ) 9>"${_ca_dispatch}.lock" || _flock_rc=$?
                # ${_ca_dispatch}.lock is a single fixed-name file — one lockfile
                # per coordination point, does not accumulate.
            else
                # lock-free best-effort fallback (flock not available)
                local _ts
                _ts=$(date +%s)
                if _cpu_admit_psi_should_pass "$_ts"; then
                    touch "$_ca_dispatch"
                    _flock_rc=0
                fi
            fi
        else
            # Simple pressure-only check (compile_gate mode: no window/dispatch).
            # Admit immediately if: PSI unreadable/unparseable OR avg10 < threshold.
            local _avg10
            _avg10="$(cpu_admit_read_avg10 "${_ca_proc_path:-/proc/pressure/cpu}")"
            if [ -z "$_avg10" ] || \
               awk -v p="$_avg10" -v t="${_ca_threshold:-50}" 'BEGIN{exit !(p<t)}'; then
                _flock_rc=0
            fi
        fi

        if [ "$_flock_rc" -eq 0 ]; then
            return 0
        fi

        # Re-sample now: the flock attempt above may have blocked up to 5s,
        # so the value captured at the top of the loop can be stale.
        _now=$(date +%s)

        # Deadline reached: admit or requeue depending on mode.
        if [ "$_now" -ge "$_deadline" ]; then
            case "$_mode" in
                admit)
                    # Fairness floor: admit anyway with a warning — NEVER exit 75.
                    # Compile admission is soft backpressure; it can delay/stagger
                    # a compile start but NEVER requeues a task (storm-proof).
                    local _avg10_final
                    _avg10_final="$(cpu_admit_read_avg10 "${_ca_proc_path:-/proc/pressure/cpu}")"
                    echo "${_ca_log_prefix:-cpu-admit}: ${_gate_tag}admitting under sustained pressure (fairness floor; avg10=${_avg10_final} >= ${_ca_threshold:-50} for ${_ca_max_wait:-300}s)" >&2
                    return 0
                    ;;
                requeue)
                    echo "${_ca_log_prefix:-cpu-admit}: ${_gate_tag}gave up after ${_ca_max_wait:-300}s waiting for CPU headroom" >&2
                    return 75
                    ;;
            esac
        fi

        sleep "$_poll"
    done
}

# ---------------------------------------------------------------------------
# Main-guard: when executed directly, run the cpu-admit CLI.
# sourceable by verify.sh (source guard skips re-execution) AND directly
# executable as `cpu-admit.sh <mode>` (agent shim β and tests).
# ---------------------------------------------------------------------------
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    set -euo pipefail

    if [ "$#" -eq 0 ]; then
        echo "Usage: $(basename "$0") admit|requeue" >&2
        exit 64
    fi

    # Resolve public REIFY_CPU_ADMIT_* knobs for the direct-exec path.
    # No _ca_window / _ca_dispatch: the CLI path is pure pressure-reactive
    # (C-A1..C-A5; the optional time-spacing is a verify.sh-internal concern).
    _ca_threshold="${REIFY_CPU_ADMIT_THRESHOLD:-50}"
    _ca_max_wait="${REIFY_CPU_ADMIT_MAX_WAIT:-300}"
    _ca_poll="${REIFY_CPU_ADMIT_POLL:-5}"
    _ca_proc_path="${REIFY_CPU_ADMIT_PROC_PATH:-/proc/pressure/cpu}"
    _ca_disable="${REIFY_CPU_ADMIT_DISABLE:-}"
    _ca_window=""
    _ca_dispatch=""
    _ca_log_prefix="cpu-admit"
    _ca_gate_name=""
    _ca_failopen_txt="fail-open"

    cpu_admit "$1"
    exit $?
fi
