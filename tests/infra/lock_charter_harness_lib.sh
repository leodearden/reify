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
