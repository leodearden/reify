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

# plan_is_narrowing_axis_line <line>
#
# Returns 0 iff <line> sits on the NARROWING AXIS — the set of plan lines whose
# crate selector is substituted from verify.sh's $AFFECTED_ALL_FLAGS, i.e. the
# only lines that branch-diff narrowing (or REIFY_AFFECTED_CRATES_OVERRIDE) can
# move. Returns non-zero for every other line.
#
# THE NARROWING AXIS IS EXACTLY THREE verify.sh CONSTRUCTION SITES (#6391):
#   verify.sh:2133 — the DEBUG nextest pass (via emit_nextest_pass, rel="")
#   verify.sh:2414 — `cargo check $AFFECTED_ALL_FLAGS --tests` (typecheck action)
#   verify.sh:2563 — `cargo clippy $AFFECTED_ALL_FLAGS --all-targets`
# Each emits `--workspace` when NARROW_ACTIVE=0 and ` -p <crate>...` when it is 1,
# so BOTH shapes are on-axis; "no ` -p ` on the axis" is what proves no narrowing.
#
# EVERY OTHER ` -p `-bearing plan line is a fixed-crate or independently-scoped
# axis that this function must classify as OFF-axis:
#   - the release-sensitivity nextest pass (verify.sh:2098-2128) — scoped by
#     scripts/release-sensitive-crates.txt, and deliberately NOT narrowed
#     ("NARROW_ACTIVE is intentionally not applied to the release pass");
#   - the fixed `-p reify-gui --features gui` compile-check and nextest pass;
#   - the fixed `cargo build --release -p reify-audit` / `-p reify-cli` pre-builds.
# Conflating those with a narrowing leak is exactly the defect this function
# retires: a blanket whole-plan `grep -qE " -p <crate>"` could not tell
# "appeared via narrowing" from "appeared at all" (it stalled task 5166 in
# infra-hold 2026-07-20 -> 2026-08-20).
#
# Classification is therefore two flag exclusions over a cargo-subcommand
# allowlist: ` --release` excludes the release pass AND both release pre-builds;
# `--features gui` excludes both fixed gui-feature lines. `cargo build` is
# deliberately absent from the allowlist, so the pre-builds are excluded twice
# over.
#
# This function encodes a MODEL of verify.sh, so it can go stale. The BEHAVIOURAL
# drift guard is tests/infra/test_verify_scope.sh Scenario MG-B5-control, which
# captures a real narrowing-ACTIVE plan and fails if the override's crates ever
# reach a line this classifier does not recognise as on-axis. Do not rely on this
# comment alone to keep the model true.
#
# Fork-free — pure bash `case` on a single argument (no pipe, no subshell, no
# external grep), preserving the esc-4574-42 rationale documented above.
plan_is_narrowing_axis_line() {
    local _line="$1"
    # 1. Comment and blank lines are never commands.
    case "$_line" in
        '#'* | '') return 1 ;;
    esac
    # 2/3. Off-axis flag exclusions (release-sensitivity + fixed gui-feature axes).
    case "$_line" in
        *' --release'*)    return 1 ;;
        *'--features gui'*) return 1 ;;
    esac
    # 4. Narrowable cargo subcommands (the three AFFECTED_ALL_FLAGS sites, plus
    #    emit_nextest_pass's NEXTEST=0 `cargo test` fallback twin).
    case "$_line" in
        *'cargo clippy '* | *'cargo check '* | *'cargo test '* | *'cargo nextest run '*)
            return 0 ;;
    esac
    # 5. Everything else (non-cargo tool lines, cargo build, npm blocks, ...).
    return 1
}

# plan_narrowing_axis_match <dump> <ere>
#
# Returns 0 iff <ere> matches a line of <dump> that plan_is_narrowing_axis_line
# classifies as ON the narrowing axis; non-zero otherwise. See that function for
# what the axis IS and why the distinction matters (#6391).
#
# Same per-line REG_NEWLINE semantics as plan_match (documented there), applied
# to the axis subset only. Fork-free — no pipe, no subshell, no external grep.
plan_narrowing_axis_match() {
    local dump="$1" ere="$2" _line
    while IFS= read -r _line; do
        plan_is_narrowing_axis_line "$_line" || continue
        [[ "$_line" =~ $ere ]] && return 0
    done <<< "$dump"
    return 1
}

# plan_offaxis_match <dump> <ere>
#
# The exact complement of plan_narrowing_axis_match: returns 0 iff <ere> matches
# a line that is NOT on the narrowing axis. See plan_is_narrowing_axis_line for
# the axis definition (#6391).
#
# "Off-axis" deliberately includes comment lines and non-cargo command lines, not
# just the fixed-crate cargo axes — that total-complement property is what makes
# this usable as test_verify_scope.sh Scenario MG-B5-control's classifier drift
# guard: a crate that reaches ANY line the classifier does not recognise as
# on-axis shows up here.
#
# Note the two matchers are NOT mutually exclusive over a dump: a pattern present
# on both an axis line and an off-axis line (e.g. ` -p reify-ir`, on the narrowed
# clippy AND on the release-sensitivity pass) makes BOTH return 0. That is the
# point — it is the case a blanket whole-plan grep could not express.
#
# Same per-line REG_NEWLINE semantics as plan_match. Fork-free.
plan_offaxis_match() {
    local dump="$1" ere="$2" _line
    while IFS= read -r _line; do
        plan_is_narrowing_axis_line "$_line" && continue
        [[ "$_line" =~ $ere ]] && return 0
    done <<< "$dump"
    return 1
}

# plan_narrowing_axis_count <dump>
#
# Prints the number of lines in <dump> that sit on the narrowing axis (see
# plan_is_narrowing_axis_line for the definition, #6391). Fork-free.
#
# Used to prove a narrowing-axis ABSENCE assertion is not vacuous: if the axis
# filter matched nothing, "no ` -p ` on the axis" would pass trivially.
plan_narrowing_axis_count() {
    local dump="$1" _n=0 _line
    while IFS= read -r _line; do
        if plan_is_narrowing_axis_line "$_line"; then
            _n=$((_n + 1))
        fi
    done <<< "$dump"
    printf '%s' "$_n"
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
