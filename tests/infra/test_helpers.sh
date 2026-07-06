#!/usr/bin/env bash
# Shared test helpers for reify shell test files.
# Provides assert() and test_summary() with PASS/FAIL counters.
#
# Usage:  source "$(dirname "${BASH_SOURCE[0]}")/test_helpers.sh"
#   or:   source "$REPO_ROOT/tests/infra/test_helpers.sh"
#
# Note: tests/infra/test_tree_sitter_pipeline.sh intentionally uses its own richer
# assert API (assert_cmd_success/assert_cmd_fails with output capture to temp
# files, PASS_COUNT/FAIL_COUNT, colored terminal output, test auto-discovery
# via declare -F, trap-based cleanup arrays) and is excluded from this shared
# module. The pipeline's needs are architecturally different from the simple
# boolean assert pattern provided here.

# Source guard — prevent double-sourcing.
if [ "${_REIFY_TEST_HELPERS_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_TEST_HELPERS_SH_SOURCED=1

PASS=0
FAIL=0

assert() {
    local desc="$1"
    shift
    # Per-assert tmpfile redirect (no-subshell tmpfile idiom, esc-4959-57):
    # "$@" runs directly in this shell (redirect only) rather than via a
    # command-substitution subshell (`out="$("$@" 2>&1)"`), because some
    # asserted checker functions mutate parent-shell globals (e.g. the
    # offline suite's _OFFLINE_PLAN_CACHE memoization) and a subshell would
    # silently discard that mutation.
    local _f
    # Guard mktemp failure (e.g. TMPDIR unwritable/full): if _f ends up empty,
    # `"$@" >"$_f" 2>&1` would be an ambiguous/empty redirect that errors
    # before the checker even runs, spuriously FAILing every assert in the
    # suite rather than reporting the real condition. Fall back to the
    # pre-esc-4959-57 `/dev/null` redirect in that case — no captured-output
    # dump is possible, but the checker's actual PASS/FAIL result is preserved.
    _f="$(mktemp "${TMPDIR:-/tmp}/reify-assert.XXXXXX")" || _f=""
    local _target="${_f:-/dev/null}"
    if "$@" >"$_target" 2>&1; then
        echo "  PASS: $desc"
        PASS=$((PASS + 1))
    else
        echo "  FAIL: $desc"
        FAIL=$((FAIL + 1))
        # Dump captured evidence ONLY on FAIL, after the byte-identical
        # "  FAIL: $desc" line, so an all-green suite stays byte-for-byte
        # unchanged while a failing assert preserves the discarded
        # stdout/stderr in the archived verify log (esc-4959-57/esc-4959-56).
        if [ -n "$_f" ] && [ -s "$_f" ]; then
            echo "  ---- assert: captured output (tail -50) ----"
            tail -n 50 "$_f" | sed 's/^/  | /'
            echo "  ---- assert: end captured output ----"
        fi
    fi
    [ -n "$_f" ] && rm -f "$_f"
}

test_summary() {
    echo ""
    echo "Results: $PASS passed, $FAIL failed"
    if [ "$FAIL" -gt 0 ]; then
        exit 1
    fi
}
