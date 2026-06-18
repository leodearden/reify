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
