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
