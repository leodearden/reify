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
# Fork-free ERE matcher. Returns 0 if <ere> matches any line in <dump>;
# non-zero otherwise. Matches per-line (REG_NEWLINE semantics): . does NOT
# match newlines, and patterns must match within a single line — exactly
# equivalent to `printf '%s\n' "$dump" | grep -qE "$ere"`.
#
# Iterates <dump> line-by-line via `read` and applies [[ =~ ]] on each line,
# keeping the fork-free property (no pipe, no subshell) while restoring
# grep -qE per-line semantics. Covers all ERE patterns used by
# test_verify_scope.sh: alternation (a|b), .* (same-line only),
# \. (literal dot), \* (literal star), empty pattern (matches any line).
#
# Rationale: fork-free — no pipe, no subshell. Eliminates the EINTR class
# of spurious failures documented as esc-4574-42.
plan_match() {
    local dump="$1" ere="$2" _line
    while IFS= read -r _line; do
        [[ "$_line" =~ $ere ]] && return 0
    done <<< "$dump"
    return 1
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

# capture_print_plan <out_var> <max_attempts> <cmd...>
#
# Runs <cmd...> up to <max_attempts> times until plan_capture_complete
# certifies a non-truncated capture. On success: assigns the complete dump
# to <out_var> via printf -v and returns 0. On exhaustion: assigns the last
# (possibly incomplete) capture to <out_var> and returns 1.
#
# Always assigns <out_var> even on exhaustion so the caller's assertions
# remain the visible failure surface rather than a set -e abort on rc=1.
# Call sites should use `|| true` to prevent set -euo pipefail from aborting
# the suite on exhaustion:
#   capture_print_plan PLAN_OUT 3 bash scripts/verify.sh ... || true
#
# Defense-in-depth against genuine PLAN_OUT truncation when verify.sh is
# killed or interrupted under load (the fork-free matching in plan_match
# eliminates the EINTR-in-grep class; this wrapper covers the truncation
# class).
capture_print_plan() {
    local _out_var="$1" _max="$2"
    shift 2
    local _cap="" _i
    for (( _i = 0; _i < _max; _i++ )); do
        _cap="$("$@")"
        if plan_capture_complete "$_cap"; then
            printf -v "$_out_var" '%s' "$_cap"
            return 0
        fi
    done
    # Exhausted — assign best-effort last capture and signal failure.
    printf -v "$_out_var" '%s' "$_cap"
    return 1
}

# plan_count_noncomment_lines <dump>
#
# Counts lines in <dump> that do NOT start with '#' and are not empty
# (i.e. command lines in --print-plan output). Prints the count to stdout.
# Fork-free — no pipe, no subshell, no grep.
#
# Equivalent to `printf '%s\n' "$dump" | grep -cE '^[^#]'` but without the
# pipe-to-grep EINTR surface (esc-4574-42).
plan_count_noncomment_lines() {
    local dump="$1" _n=0 _line
    while IFS= read -r _line; do
        case "$_line" in
            '#'* | '') ;;          # skip comment lines and empty lines
            *) _n=$((_n + 1)) ;;  # count non-empty, non-comment lines
        esac
    done <<< "$dump"
    printf '%s' "$_n"
}
