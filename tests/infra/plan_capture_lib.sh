#!/usr/bin/env bash
# tests/infra/plan_capture_lib.sh — fork-free plan capture/match helpers
#
# Sourceable library for test_verify_scope.sh and sibling infra tests.
# Provides robust, concurrency-safe helpers for capturing and asserting
# on verify.sh --print-plan output.
#
# Rationale for fork-free matching: pipe-to-grep forks a subshell and a grep
# that read from a pipe; under heavy concurrent test load that grep can
# transiently fail (broken pipe / EINTR) and return non-zero EVEN WHEN the
# content matches — silently flipping assertions to spurious FAILs.
# (Root cause documented as esc-4574-42 in tests/infra/test_test_helpers.sh.)
# bash [[ ]] does no fork and no pipe, so predicates become pure functions
# of the captured string, eliminating this failure surface entirely.
#
# Usage:
#   [ -f "$SCRIPT_DIR/plan_capture_lib.sh" ] || { echo "ERROR: plan_capture_lib.sh not found"; exit 1; }
#   source "$SCRIPT_DIR/plan_capture_lib.sh"

# Source guard — prevent double-sourcing.
if [ "${_REIFY_PLAN_CAPTURE_LIB_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_PLAN_CAPTURE_LIB_SH_SOURCED=1

# plan_match <dump> <ere>
#
# Fork-free ERE matcher. Returns 0 if <ere> matches any line (or substring
# spanning no newline) in <dump>; non-zero otherwise.
#
# Uses bash [[ =~ ]] with the RHS UNQUOTED so bash treats it as an ERE,
# matching grep -qE semantics per line. Semantics verified equivalent to
# `printf '%s\n' "$dump" | grep -qE "$ere"` for all patterns used by
# test_verify_scope.sh: alternation (a|b), .* (same-line), \. (literal dot),
# \* (literal star).
#
# Note: [[ =~ ]] operates on the whole string, NOT per-line. On Linux/glibc,
# bash uses regexec() WITHOUT REG_NEWLINE, so . DOES match newline characters
# (unlike grep -qE which sets REG_NEWLINE and matches per-line). This differs
# from grep semantics for patterns that span newlines, but all patterns used
# by test_verify_scope.sh match same-line content, so in practice the
# behaviour is equivalent (see esc-4708-51). Empty pattern matches any string
# (parity with grep -qE "").
#
# Rationale: fork-free — no pipe, no subshell. Eliminates the EINTR class
# of spurious failures documented as esc-4574-42.
plan_match() {
    local dump="$1" ere="$2"
    [[ "$dump" =~ $ere ]]
}

# plan_capture_complete <dump>
#
# Returns 0 iff <dump> contains BOTH structural markers that verify.sh
# unconditionally emits in every --print-plan invocation:
#   "# verify.sh plan"   — header (verify.sh:1099)
#   "# --- commands"     — commands-block marker (verify.sh:1104)
#
# Their joint presence certifies a non-truncated capture. Fork-free via
# [[ == *glob* ]] (no pipe, no subshell, no EINTR surface).
plan_capture_complete() {
    local dump="$1"
    [[ "$dump" == *"# verify.sh plan"* ]] && [[ "$dump" == *"# --- commands"* ]]
}

# plan_narrow_active <dump>
#
# Extracts the NARROW_ACTIVE value from the --print-plan narrowing header
# emitted by verify.sh:1101:
#   # narrowing — NARROW_ACTIVE=N affected=...
#
# Prints the numeric value (0 or 1) to stdout; prints nothing if the line
# is absent. Fork-free via bash regex engine and BASH_REMATCH (no sed, no
# awk, no pipe, no subshell).
plan_narrow_active() {
    local dump="$1"
    if [[ "$dump" =~ NARROW_ACTIVE=([0-9]+) ]]; then
        printf '%s' "${BASH_REMATCH[1]}"
    fi
}
