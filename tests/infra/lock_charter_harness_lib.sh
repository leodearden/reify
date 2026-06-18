#!/usr/bin/env bash
# tests/infra/lock_charter_harness_lib.sh — driver lib for test_lock_charter_lifecycle.sh.
#
# Sourced by tests/infra/test_lock_charter_lifecycle.sh (the auto-discovered
# test_*.sh harness); never executed standalone (the *_lib.sh name keeps it
# out of run_all.sh's test_*.sh glob).
#
# This lib provides lcl_* helpers (lock-charter-lifecycle helpers) that drive:
#   - the real α predicate (scripts/lock-charter-guard.sh) for §8 rows 1-3
#   - curl-stub canned MCP responses for §8 rows 4-10 and 13 (hermetic mode)
#   - opt-in live fused-memory MCP calls (REIFY_LOCK_CHARTER_LIVE=1 only)
#
# Source guard — prevents double-sourcing.
if [ "${_REIFY_LOCK_CHARTER_HARNESS_LIB_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_LOCK_CHARTER_HARNESS_LIB_SH_SOURCED=1

# REPO_ROOT must be set by the sourcing harness before this lib is sourced.
# (set by test_lock_charter_lifecycle.sh via the standard SCRIPT_DIR/../.. pattern)

# ──────────────────────────────────────────────────────────────────────────────
# Guard-surface helpers (§8 rows 1-3, always-on)
# Wraps the real α predicate: scripts/lock-charter-guard.sh
# ──────────────────────────────────────────────────────────────────────────────

# Globals populated by lcl_run_guard.
LCL_GUARD_RC=0
LCL_GUARD_OUT=""

# lcl_run_guard <classify|check|--list-extensions> [args...]
#
# Run scripts/lock-charter-guard.sh with the given subcommand and arguments.
# Captures exit code into LCL_GUARD_RC and stdout into LCL_GUARD_OUT.
# Inherits stdin from the caller (needed for 'check </dev/null' pattern).
# Mirrors run_classify/run_check in tests/infra/test_lock_charter_guard.sh.
lcl_run_guard() {
    local _subcmd="${1:-}"
    shift || true
    LCL_GUARD_OUT="$(bash "$REPO_ROOT/scripts/lock-charter-guard.sh" \
        "$_subcmd" "$@" 2>/dev/null)" \
        && LCL_GUARD_RC=$? || LCL_GUARD_RC=$?
}

# lcl_canonical_extensions
#
# Echo the canonical OQ#2 extension allowlist (sorted-unique, one per line).
# This is the shared α/γ test vector (PRD §11 Q1) — byte-identical to the
# output of 'scripts/lock-charter-guard.sh --list-extensions'.
# Pinned here so the row-3 C-P3 no-drift assertion has a stable reference
# independent of the script under test.
lcl_canonical_extensions() {
    cat <<'EXTS_EOF'
c
cc
cjs
cpp
css
cts
cxx
gcode
h
hh
hpp
html
js
json
jsonc
jsx
lock
md
mjs
mts
png
py
ri
rs
scss
service
sh
step
stl
svg
toml
ts
tsx
txt
yaml
yml
EXTS_EOF
}

# ──────────────────────────────────────────────────────────────────────────────
# Live-mode plumbing (§8 rows 4-10, 13 — opt-in REIFY_LOCK_CHARTER_LIVE=1)
# ──────────────────────────────────────────────────────────────────────────────

# lcl_live_enabled
#
# Returns 0 (true) ONLY when ALL of:
#   - REIFY_LOCK_CHARTER_LIVE=1 (explicit opt-in — never auto-enabled by reachability)
#   - curl is on PATH
#   - jq is on PATH
# Returns 1 (false) otherwise, printing a clear SKIP reason to stderr.
lcl_live_enabled() {
    if [ "${REIFY_LOCK_CHARTER_LIVE:-}" != "1" ]; then
        echo "SKIP: live mode disabled (set REIFY_LOCK_CHARTER_LIVE=1 to enable)" >&2
        return 1
    fi
    if ! command -v curl >/dev/null 2>&1; then
        echo "SKIP: curl not on PATH — live mode requires curl" >&2
        return 1
    fi
    if ! command -v jq >/dev/null 2>&1; then
        echo "SKIP: jq not on PATH — live mode requires jq" >&2
        return 1
    fi
    return 0
}

# lcl_mcp_call <tool> <json-args>
#
# POST a JSON-RPC tools/call to the fused-memory MCP endpoint.
# URL: ${REIFY_FUSED_MEMORY_URL:-http://127.0.0.1:8002/mcp}
# Timeout: 5 seconds (-m 5), so it never hangs.
# On curl failure or empty response: returns curl's exit code (never 127).
# On success: prints jq-extracted .result.content[0].text (falling back to .).
lcl_mcp_call() {
    local _tool="$1"
    local _args="$2"
    local _url="${REIFY_FUSED_MEMORY_URL:-http://127.0.0.1:8002/mcp}"

    if ! command -v curl >/dev/null 2>&1; then
        echo "error: curl not found" >&2
        return 1
    fi
    if ! command -v jq >/dev/null 2>&1; then
        echo "error: jq not found" >&2
        return 1
    fi

    local _body
    _body='{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"'"$_tool"'","arguments":'"$_args"'}}'

    local _raw _rc=0
    _raw="$(curl -s -m 5 \
        -H 'Content-Type: application/json' \
        -H 'Accept: application/json, text/event-stream' \
        -d "$_body" \
        "$_url" 2>/dev/null)" && _rc=0 || _rc=$?

    if [ "$_rc" -ne 0 ] || [ -z "$_raw" ]; then
        return "$_rc"
    fi

    echo "$_raw" | jq -r '.result.content[0].text // .' 2>/dev/null || echo "$_raw"
}

# ──────────────────────────────────────────────────────────────────────────────
# Curl-stub + scheduler-observation helpers (§8 rows 4-10, hermetic)
# PATH-stub idiom mirrors test_orchestrator_redeploy_restart.sh (systemctl/systemd-run)
# ──────────────────────────────────────────────────────────────────────────────

# Tracking arrays for cleanup (populated by lcl_make_curl_stub)
_LCL_STUB_DIRS=()
_LCL_STUB_CALL_FILES=()

# lcl_cleanup_stubs — remove all stub dirs and call files; called from EXIT trap
lcl_cleanup_stubs() {
    local _d _f
    for _d in "${_LCL_STUB_DIRS[@]+"${_LCL_STUB_DIRS[@]}"}"; do
        rm -rf "$_d" 2>/dev/null || true
    done
    for _f in "${_LCL_STUB_CALL_FILES[@]+"${_LCL_STUB_CALL_FILES[@]}"}"; do
        rm -f "$_f" 2>/dev/null || true
    done
}

# lcl_make_curl_stub <state-json> <events-json> [submit-json]
#
# Creates a PATH-stub directory containing a fake `curl` that routes responses
# by tool name in the -d POST body:
#   get_scheduler_state  → state-json
#   get_scheduler_events → events-json
#   submit_task          → submit-json (optional 3rd arg; returns {} if omitted)
# Prepends the stub dir to PATH (so lcl_mcp_call picks it up).
# Records all invocations to LCL_STUB_CALLS_FILE.
# Tracks dirs for cleanup via _LCL_STUB_DIRS.
lcl_make_curl_stub() {
    local _state_json="$1"
    local _events_json="$2"
    local _submit_json="${3:-}"

    local _stub_dir _calls_file
    _stub_dir="$(mktemp -d /tmp/test-lcl-curl-stub-XXXXXX)"
    _calls_file="$(mktemp /tmp/test-lcl-curl-calls-XXXXXX)"

    _LCL_STUB_DIRS+=("$_stub_dir")
    _LCL_STUB_CALL_FILES+=("$_calls_file")

    # Write canned responses to files inside the stub dir
    printf '%s' "$_state_json"  > "${_stub_dir}/state.json"
    printf '%s' "$_events_json" > "${_stub_dir}/events.json"
    if [ -n "$_submit_json" ]; then
        printf '%s' "$_submit_json" > "${_stub_dir}/submit.json"
        export LCL_STUB_SUBMIT_FILE="${_stub_dir}/submit.json"
    else
        export LCL_STUB_SUBMIT_FILE=""
    fi

    # Export env vars the stub reads at runtime
    export LCL_STUB_STATE_FILE="${_stub_dir}/state.json"
    export LCL_STUB_EVENTS_FILE="${_stub_dir}/events.json"
    export LCL_STUB_CALLS_FILE="$_calls_file"

    cat > "${_stub_dir}/curl" << 'CURL_STUB_EOF'
#!/usr/bin/env bash
echo "curl $*" >> "${LCL_STUB_CALLS_FILE:-/dev/null}"
# Find the -d argument (POST body)
_body=""
while [ "$#" -gt 0 ]; do
    if [ "$1" = "-d" ]; then shift; _body="$1"; break; fi
    shift
done
# Route response by tool name embedded in the JSON body
case "$_body" in
    *'"name":"get_scheduler_state"'*)
        cat "${LCL_STUB_STATE_FILE:-/dev/null}" ;;
    *'"name":"get_scheduler_events"'*)
        cat "${LCL_STUB_EVENTS_FILE:-/dev/null}" ;;
    *'"name":"submit_task"'*)
        if [ -n "${LCL_STUB_SUBMIT_FILE:-}" ]; then
            cat "${LCL_STUB_SUBMIT_FILE}"
        else
            echo '{"result":{"content":[{"text":"{}"}]}}'
        fi ;;
    *)
        echo '{"result":{"content":[{"text":"{}"}]}}' ;;
esac
CURL_STUB_EOF
    chmod +x "${_stub_dir}/curl"

    # Prepend stub dir so lcl_mcp_call's curl resolves to the stub
    export PATH="${_stub_dir}:${PATH}"
}

# lcl_held_modules <task>
#
# Echo the sorted JSON array of modules held by <task> from get_scheduler_state.
# Returns curl/jq exit code on failure.
lcl_held_modules() {
    local _task="$1"
    local _state_text _rc=0
    _state_text="$(lcl_mcp_call get_scheduler_state '{}')" && _rc=0 || _rc=$?
    [ "$_rc" -ne 0 ] && return "$_rc"
    echo "$_state_text" | jq -c --arg t "$_task" '.parks[$t].held // [] | sort'
}

# lcl_assert_set_to_plan_release <task> <plan-files-json-array> <waiter>
#
# Returns 0 (PASS) iff all three hold:
#   1. get_scheduler_state held == plan-files-json-array (set equality after sort)
#   2. get_scheduler_events has a lock_released(reason=plan_refinement) for <task>
#   3. get_scheduler_events has a task_started for <waiter>
# Returns 1 (FAIL) with a diagnostic on stderr at the first failed check.
lcl_assert_set_to_plan_release() {
    local _task="$1"
    local _plan_files="$2"
    local _waiter="$3"
    local _rc=0

    # (1) held == plan.files
    local _held _plan_sorted
    _held="$(lcl_held_modules "$_task")" && _rc=0 || _rc=$?
    [ "$_rc" -ne 0 ] && { echo "FAIL: get_scheduler_state error (rc=$_rc)" >&2; return "$_rc"; }
    _plan_sorted="$(echo "$_plan_files" | jq -c '. | sort')"
    if [ "$_held" != "$_plan_sorted" ]; then
        echo "FAIL: held($_task)=$_held ≠ plan=$_plan_sorted" >&2
        return 1
    fi

    # (2) lock_released(plan_refinement) + (3) task_started for waiter — one events call
    local _events_text
    _events_text="$(lcl_mcp_call get_scheduler_events '{}')" && _rc=0 || _rc=$?
    [ "$_rc" -ne 0 ] && { echo "FAIL: get_scheduler_events error (rc=$_rc)" >&2; return "$_rc"; }

    local _released _started
    _released="$(echo "$_events_text" | jq -r \
        --arg e "lock_released" --arg t "$_task" --arg r "plan_refinement" \
        '[.events[] | select(.event_type==$e and .task_id==$t and (.data.reason//"")==$r)] | length > 0')"
    _started="$(echo "$_events_text" | jq -r \
        --arg t "$_waiter" \
        '[.events[] | select(.event_type=="task_started" and .task_id==$t)] | length > 0')"

    if [ "$_released" != "true" ]; then
        echo "FAIL: lock_released(plan_refinement) not fired for $_task" >&2; return 1
    fi
    if [ "$_started" != "true" ]; then
        echo "FAIL: task_started not fired for waiter $_waiter" >&2; return 1
    fi
    return 0
}

# ──────────────────────────────────────────────────────────────────────────────
# BRE ordering helpers (§8 rows 6-7 — C-S2/C-K1, OBSERVED)
# ──────────────────────────────────────────────────────────────────────────────

# _lcl_ts_to_int <timestamp>
#
# Normalise a scheduler event timestamp to a plain integer for ordering
# comparisons.  Hermetic canned fixtures use plain integers (100, 200, …).
# In live mode the scheduler may emit ISO-8601 / RFC-3339 strings; these are
# converted to epoch-seconds via 'date -d' (GNU coreutils, present on the
# orchestrator host).  Emits a WARNING to stderr for unrecognised formats and
# returns 0 so comparisons degrade gracefully rather than silently.
_lcl_ts_to_int() {
    local _ts="$1"
    case "$_ts" in
        '' | *[!0-9]*)
            # Non-integer (empty string, ISO-8601, or other) — try date -d
            if command -v date >/dev/null 2>&1; then
                local _epoch
                if _epoch="$(date -d "$_ts" +%s 2>/dev/null)" && [ -n "$_epoch" ]; then
                    printf '%s\n' "$_epoch"
                    return 0
                fi
            fi
            printf 'WARNING: _lcl_ts_to_int: unrecognised timestamp %s; ordering result may be wrong\n' \
                "$_ts" >&2
            printf '0\n' ;;
        *)
            # Plain non-negative integer — use as-is
            printf '%s\n' "$_ts" ;;
    esac
}

# lcl_acquire_precedes_edit <task>
#
# Returns 0 (PASS) iff get_scheduler_events shows that the lock_acquired event
# for <task> has a strictly smaller timestamp than the implementation_started
# event for <task> (BRE acquired before the edit phase started).
# Returns 1 (FAIL) with a diagnostic on stderr if the ordering is wrong or
# either event is missing.
lcl_acquire_precedes_edit() {
    local _task="$1"
    local _events_text _rc=0
    _events_text="$(lcl_mcp_call get_scheduler_events '{}')" && _rc=0 || _rc=$?
    [ "$_rc" -ne 0 ] && { echo "FAIL: get_scheduler_events error (rc=$_rc)" >&2; return "$_rc"; }

    local _acquire_ts _edit_ts
    _acquire_ts="$(echo "$_events_text" | jq -r \
        --arg t "$_task" \
        'first(.events[] | select(.event_type=="lock_acquired" and .task_id==$t) | .timestamp) // empty')"
    _edit_ts="$(echo "$_events_text" | jq -r \
        --arg t "$_task" \
        'first(.events[] | select(.event_type=="implementation_started" and .task_id==$t) | .timestamp) // empty')"

    if [ -z "$_acquire_ts" ] || [ -z "$_edit_ts" ]; then
        echo "FAIL: missing lock_acquired or implementation_started event for $_task" >&2
        return 1
    fi

    local _a_int _e_int _result
    _a_int="$(_lcl_ts_to_int "$_acquire_ts")"
    _e_int="$(_lcl_ts_to_int "$_edit_ts")"
    _result="$(awk -v a="$_a_int" -v e="$_e_int" \
        'BEGIN { print (a+0 < e+0) ? "ok" : "fail" }')"
    if [ "$_result" = "ok" ]; then
        return 0
    else
        echo "FAIL: lock_acquired ts ($_acquire_ts) does not precede implementation_started ts ($_edit_ts)" >&2
        return 1
    fi
}

# lcl_no_release_when_repended <task>
#
# Returns 0 (PASS) iff get_scheduler_events shows:
#   - a REQUEUED event exists for <task> (charter re-pended correctly), AND
#   - no lock_released event exists for <task> (charter NOT released prematurely)
# Returns 1 (FAIL) with a diagnostic on stderr if either condition is violated.
lcl_no_release_when_repended() {
    local _task="$1"
    local _events_text _rc=0
    _events_text="$(lcl_mcp_call get_scheduler_events '{}')" && _rc=0 || _rc=$?
    [ "$_rc" -ne 0 ] && { echo "FAIL: get_scheduler_events error (rc=$_rc)" >&2; return "$_rc"; }

    local _requeued _released
    _requeued="$(echo "$_events_text" | jq -r \
        --arg t "$_task" \
        '[.events[] | select(.event_type=="REQUEUED" and .task_id==$t)] | length > 0')"
    _released="$(echo "$_events_text" | jq -r \
        --arg t "$_task" \
        '[.events[] | select(.event_type=="lock_released" and .task_id==$t)] | length > 0')"

    if [ "$_requeued" != "true" ]; then
        echo "FAIL: no REQUEUED event found for $_task" >&2; return 1
    fi
    if [ "$_released" = "true" ]; then
        echo "FAIL: lock_released fired for $_task despite REQUEUED (charter violated)" >&2; return 1
    fi
    return 0
}

# ──────────────────────────────────────────────────────────────────────────────
# Staleness re-pend + revalidation helper (§8 row 8 — C-K1, OBSERVED)
# ──────────────────────────────────────────────────────────────────────────────

# lcl_assert_repend_revalidate <task>
#
# Returns 0 (PASS) iff get_scheduler_events shows ALL of:
#   1. A REQUEUED event with data._last_block_reason == 'plan_blast_radius_lock_conflict'
#   2. A subsequent revalidation_passed event (timestamp > REQUEUED timestamp)
# Returns 1 (FAIL) with a diagnostic on stderr at the first failed check.
lcl_assert_repend_revalidate() {
    local _task="$1"
    local _events_text _rc=0
    _events_text="$(lcl_mcp_call get_scheduler_events '{}')" && _rc=0 || _rc=$?
    [ "$_rc" -ne 0 ] && { echo "FAIL: get_scheduler_events error (rc=$_rc)" >&2; return "$_rc"; }

    # (1) REQUEUED with the conflict reason
    local _requeued_ts
    _requeued_ts="$(echo "$_events_text" | jq -r \
        --arg t "$_task" --arg r "plan_blast_radius_lock_conflict" \
        'first(.events[] | select(.event_type=="REQUEUED" and .task_id==$t and (.data._last_block_reason//"")==$r) | .timestamp) // empty')"
    if [ -z "$_requeued_ts" ]; then
        echo "FAIL: no REQUEUED(plan_blast_radius_lock_conflict) event for $_task" >&2
        return 1
    fi

    # (2) Subsequent revalidation_passed event
    local _reval_ts
    _reval_ts="$(echo "$_events_text" | jq -r \
        --arg t "$_task" \
        'first(.events[] | select(.event_type=="revalidation_passed" and .task_id==$t) | .timestamp) // empty')"
    if [ -z "$_reval_ts" ]; then
        echo "FAIL: no revalidation_passed event for $_task" >&2
        return 1
    fi

    # revalidation must be subsequent (timestamp > REQUEUED timestamp)
    local _r_int _v_int _order
    _r_int="$(_lcl_ts_to_int "$_requeued_ts")"
    _v_int="$(_lcl_ts_to_int "$_reval_ts")"
    _order="$(awk -v r="$_r_int" -v v="$_v_int" \
        'BEGIN { print (v+0 > r+0) ? "ok" : "fail" }')"
    if [ "$_order" != "ok" ]; then
        echo "FAIL: revalidation_passed ts ($_reval_ts) does not follow REQUEUED ts ($_requeued_ts)" >&2
        return 1
    fi

    return 0
}

# ──────────────────────────────────────────────────────────────────────────────
# Plan-derivation-input helpers (§8 rows 9-10 — C-A1/C-A2)
# These helpers introspect canned JSON payloads (no MCP call needed).
# ──────────────────────────────────────────────────────────────────────────────

# lcl_assert_first_plan_anti_anchored <input-json>
#
# Returns 0 (PASS) iff the given plan-derivation-input JSON:
#   - Does NOT contain a "files" key in "metadata" (charter NOT leaked to architect)
# Returns 1 (FAIL) with a diagnostic on stderr if metadata.files is present
# (anti-anchoring contract violated — queue-time charter was leaked to the architect).
lcl_assert_first_plan_anti_anchored() {
    local _input_json="$1"

    if ! command -v jq >/dev/null 2>&1; then
        echo "FAIL: jq not found — cannot introspect input JSON" >&2
        return 1
    fi

    # metadata.files must be ABSENT (null or missing)
    local _has_files
    _has_files="$(echo "$_input_json" | jq -r \
        'if (.metadata.files != null and (.metadata.files | length) > 0) then "true" else "false" end')"

    if [ "$_has_files" = "true" ]; then
        echo "FAIL: metadata.files present in first-plan input — anti-anchoring contract violated" >&2
        return 1
    fi

    return 0
}

# lcl_assert_revalidation_sees_plan <input-json>
#
# Returns 0 (PASS) iff the given revalidation-input JSON:
#   - Contains metadata.files with at least one entry (prior plan visible to architect)
# Returns 1 (FAIL) with a diagnostic on stderr if metadata.files is absent
# (revalidation contract violated — prior plan was hidden from the architect).
lcl_assert_revalidation_sees_plan() {
    local _input_json="$1"

    if ! command -v jq >/dev/null 2>&1; then
        echo "FAIL: jq not found — cannot introspect input JSON" >&2
        return 1
    fi

    # metadata.files must be present and non-empty
    local _has_files
    _has_files="$(echo "$_input_json" | jq -r \
        'if (.metadata.files != null and (.metadata.files | length) > 0) then "true" else "false" end')"

    if [ "$_has_files" != "true" ]; then
        echo "FAIL: metadata.files absent in revalidation input — prior plan hidden (contract violated)" >&2
        return 1
    fi

    return 0
}

# ──────────────────────────────────────────────────────────────────────────────
# Live submit-site dir-reject helper (§8 row 13 — opt-in REIFY_LOCK_CHARTER_LIVE=1)
# ──────────────────────────────────────────────────────────────────────────────

# lcl_live_submit_rejects_dir
#
# When lcl_live_enabled (REIFY_LOCK_CHARTER_LIVE=1 + curl + jq), submits a
# disposable task with metadata.files=["crates/reify-eval/src/"] via
# lcl_mcp_call submit_task against a temporary project_root, then asserts
# the response is a clear directory-declaration rejection.
#
# When not live: prints a SKIP message to stderr and returns 0 (skip is not a
# failure — the merge gate never sets REIFY_LOCK_CHARTER_LIVE=1).
#
# Returns:
#   0 — PASS (rejection observed) or SKIP (live mode not enabled)
#   1 — FAIL (no rejection; task was accepted — γ submit backstop not enforcing)
lcl_live_submit_rejects_dir() {
    if ! lcl_live_enabled 2>/dev/null; then
        echo "SKIP: live mode not enabled — submit dir-reject smoke skipped (set REIFY_LOCK_CHARTER_LIVE=1 to run)" >&2
        return 0
    fi

    # Build a minimal submit_task payload with a directory in metadata.files.
    # Use a disposable project_root (never the real reify queue).
    local _submit_args
    _submit_args='{"title":"lcl-test-dir-reject","description":"integration gate smoke test","project_root":"/tmp/lcl-submit-test-'"$$"'","metadata":{"files":["crates/reify-eval/src/"]}}'

    local _response _rc=0
    _response="$(lcl_mcp_call submit_task "$_submit_args" 2>/dev/null)" && _rc=0 || _rc=$?

    if [ "$_rc" -ne 0 ] || [ -z "$_response" ]; then
        echo "FAIL: lcl_mcp_call submit_task returned error (rc=$_rc) — cannot observe rejection" >&2
        return 1
    fi

    # The rejection must carry the specific charter-enforcement phrase.
    # Require the canonical "directory declaration not allowed" substring (present
    # in both the canned stub and the real γ backstop); the broad *"Error"*"dir"*
    # fallback is intentionally absent to avoid false-PASS on unrelated errors.
    case "$_response" in
        *"directory declaration not allowed"* | *"directory declaration"*)
            # Rejection observed — γ backstop is enforcing
            return 0 ;;
        *)
            echo "FAIL: submit_task response does not indicate directory rejection — γ backstop not enforcing" >&2
            echo "  response: $_response" >&2
            return 1 ;;
    esac
}
