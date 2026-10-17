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
# Header fields and step order have one parser each: plan_header_field reads a
# header field by line kind and exact name, and plan_step_index locates a step in
# the commands block. Read the plan through them rather than through ordered
# substrings or byte offsets, which a reordered or extended plan silently breaks.
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
# Exported because occt_flock_gate_lib.sh's occt_plan_grep_or_dump — itself
# exported so the bounds-file negative unit tests run the REAL helper in a
# `bash -c` child — calls this one. A child that inherited the caller but not
# the callee would fail with command-not-found, which a `! ...` negation reads
# as a correct non-zero return, making every such rejection vacuous.
export -f plan_match

# plan_capture_complete <dump>
#
# Returns 0 iff <dump> is structurally whole, which takes THREE conditions —
# both of the markers verify.sh's --print-plan printer (the
# `if [ "$PRINT_PLAN" -eq 1 ]` block after `build_plan`) unconditionally emits,
# AND at least one non-empty line after the second one:
#   "# verify.sh plan"   — header
#   "# --- commands"     — commands-block marker
#   <a non-empty line after the commands marker>
#
# WHY THE MARKER PAIR ALONE IS INSUFFICIENT: both markers sit in the plan
# HEADER, and every line a caller actually asserts on lies in the command body
# below them. A marker-pair oracle therefore certifies every truncation that
# happens after the header — i.e. all of the region the assertions depend on —
# so capture_print_plan returns without retrying and the missing line surfaces
# as a spurious assertion FAIL rather than as the incomplete capture it is.
#
# The rule is deliberately "a non-empty line", not "a command line": the printer
# emits either real command lines or the comment
# "# (no commands — nothing to verify for this action/scope)",
# so a docs-only scope has a legitimately command-free but complete plan.
# Demanding a non-comment line would make every such plan retry to exhaustion.
#
# Fork-free: [[ == *glob* ]] plus the `while read` + `<<<` idiom used by
# plan_match/plan_strip_comments — no pipe, no subshell, no EINTR surface.
plan_capture_complete() {
    local dump="$1" _line _after_marker=0
    [[ "$dump" == *"# verify.sh plan"* ]] || return 1
    [[ "$dump" == *"# --- commands"* ]] || return 1
    while IFS= read -r _line; do
        if (( _after_marker )); then
            [ -n "$_line" ] && return 0
        elif [[ "$_line" == "# --- commands"* ]]; then
            _after_marker=1
        fi
    done <<< "$dump"
    return 1
}

# plan_narrow_active <dump>
#
# Extracts the NARROW_ACTIVE value from the narrowing header emitted by
# verify.sh's --print-plan printer:
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

# plan_header_field <dump> <line-kind> <NAME>
#
# Reads field NAME of one --print-plan header line, found by its label, never by
# position. <line-kind> is a closed set: plan, scope, narrowing, heavy (labels in
# the case below, exactly as verify.sh's --print-plan printer emits them). The
# first line starting with the label is the header; the non-empty, non-'#' lines
# right after it continue it (today only the narrowing header, whose crate lists
# are newline-separated: PRD §2.4 defect 7, made single-line by ρ #9115).
#
# A `KEY=` token starts a field and any other token extends the current field's
# value, single-space joined. NAME matches exactly and its first occurrence wins,
# so a reordered or extended header reads the same: this replaces ordered
# `RUN_RUST=… RUN_GUI=…` substring greps over the whole dump (PRD fk-d47473e4d8d8).
#
# rc 0: prints the value, possibly empty (`closure=`), with no trailing newline.
# rc 1: the header line or NAME is absent; prints nothing.
# rc 2: unknown line-kind; prints nothing on stdout, the accepted set on stderr.
plan_header_field() {
    local dump="$1" kind="$2" name="$3" label
    case "$kind" in
        plan)      label='# verify.sh plan — ' ;;
        scope)     label='# scope decision — ' ;;
        narrowing) label='# narrowing — ' ;;
        heavy)     label='# heavy partition — ' ;;
        *)
            printf "plan_header_field: unknown line-kind '%s' (expected plan, scope, narrowing or heavy)\n" "$kind" >&2
            return 2 ;;
    esac
    local _line _header="" _found=0
    while IFS= read -r _line; do
        if (( _found )); then
            case "$_line" in '#'* | '') break ;; esac
            _header+=" $_line"
        elif [[ "$_line" == "$label"* ]]; then
            _header="${_line#"$label"}"
            _found=1
        fi
    done <<< "$dump"
    (( _found )) || return 1
    local -a _tokens=()
    IFS=$' \t' read -r -a _tokens <<< "$_header"
    local _field_re='^([A-Za-z_][A-Za-z0-9_]*)=(.*)$' _tok _value="" _matched=0
    for _tok in ${_tokens[@]+"${_tokens[@]}"}; do
        if [[ "$_tok" =~ $_field_re ]]; then
            (( _matched )) && break
            [ "${BASH_REMATCH[1]}" = "$name" ] || continue
            _matched=1
            _value="${BASH_REMATCH[2]}"
        elif (( _matched )); then
            _value+="${_value:+ }$_tok"
        fi
    done
    (( _matched )) || return 1
    printf '%s' "$_value"
}

# plan_step_index <dump> <ERE> [<from>]
#
# The plan's steps are the non-empty, non-'#' lines after the first
# `# --- commands` line (the marker plan_capture_complete certifies), numbered
# from 0. Lines above the marker (a preamble, the retry marker, a narrowing
# continuation line, the environment block) are never steps, and comment lines
# inside the block do not advance the index.
#
# Prints the index of the first step at index >= <from> (default 0) that <ERE>
# matches, per line as in plan_match, and returns 0. Prints -1, rc 0, when there
# is no such step, so `[ -lt ]` comparisons stay well-formed. A <from> that is not
# a non-negative integer prints nothing on stdout, names it on stderr, rc 2.
#
# "X after every Y" is `plan_step_index "$dump" Y "$i_X"` == -1: no Y at or after X.
plan_step_index() {
    local dump="$1" ere="$2" from="${3-0}" _from_re='^[0-9]+$' _line _step=0 _in_commands=0
    if ! [[ "$from" =~ $_from_re ]]; then
        printf "plan_step_index: <from> must be a non-negative integer, got '%s'\n" "$from" >&2
        return 2
    fi
    while IFS= read -r _line; do
        if (( ! _in_commands )); then
            [[ "$_line" == "# --- commands"* ]] && _in_commands=1
            continue
        fi
        case "$_line" in '#'* | '') continue ;; esac
        if (( _step >= 10#$from )) && [[ "$_line" =~ $ere ]]; then
            printf '%s' "$_step"
            return 0
        fi
        (( ++_step ))
    done <<< "$dump"
    printf '%s' -1
}

# plan_is_narrowing_axis_line <line>
#
# Returns 0 iff <line> sits on the NARROWING AXIS — the set of plan lines whose
# crate selector is substituted from verify.sh's $AFFECTED_ALL_FLAGS, i.e. the
# only lines that branch-diff narrowing (or REIFY_AFFECTED_CRATES_OVERRIDE) can
# move. Returns non-zero for every other line.
#
# THE NARROWING AXIS IS EXACTLY THREE verify.sh CONSTRUCTION SITES (#6391):
#   add_test_passes's debug arm — the DEBUG nextest pass
#     (`emit_nextest_pass "$AFFECTED_ALL_FLAGS" …`, rel="")
#   build_plan's typecheck add — `cargo check $AFFECTED_ALL_FLAGS --tests`
#   build_plan's lint add — `cargo clippy $AFFECTED_ALL_FLAGS --all-targets`
# Each emits `--workspace` when NARROW_ACTIVE=0 and ` -p <crate>...` when it is 1,
# so BOTH shapes are on-axis; "no ` -p ` on the axis" is what proves no narrowing.
#
# EVERY OTHER ` -p `-bearing plan line is a fixed-crate or independently-scoped
# axis that this function must classify as OFF-axis:
#   - the release-sensitivity nextest pass (add_test_passes's release arm,
#     `emit_nextest_pass "$_RELEASE_ALL_FLAGS" " --release" …`) — scoped by
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
# ORDER MATTERS, and is a KNOWN LIMITATION: the flag exclusions are applied
# BEFORE the allowlist, so a narrowable subcommand that also carried ` --release`
# would classify OFF-axis. Sound today (AFFECTED_ALL_FLAGS reaches nextest only in
# add_test_passes's debug arm, and the check/clippy sites never take --release),
# but it is an assumption about verify.sh. Pinned as unit case (p) in
# test_plan_capture_lib.sh; the behavioural backstop is the drift guard named in
# the next paragraph.
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
# Runs <cmd...> up to <max_attempts> times until the capture is certified
# whole. On success: assigns the complete dump to <out_var> via printf -v and
# returns 0. On exhaustion: assigns the last (possibly incomplete) capture to
# <out_var> and returns 1.
#
# A capture is whole when BOTH oracles agree: the child EXITED 0, and
# plan_capture_complete certifies the text.
#
# CHILD EXIT STATUS IS THE PRIMARY ORACLE, because it is the only one that
# reports the thing truncation actually is. Command substitution reads the
# child's stdout to EOF, so a producer that reached normal exit necessarily
# wrote its whole plan; a truncated capture requires the producer to DIE, and
# that is precisely what a non-zero status says. A text oracle can only prove
# the producer reached some point in its own output, never that it finished.
#
# plan_capture_complete is retained as the cheap structural cross-check for
# the case rc alone would miss: verify.sh exits non-zero with no plan at all
# when its top-level nextest availability probe (`cargo nextest --version`,
# retried under REIFY_NEXTEST_PROBE_RETRY_SLEEP) hard-fails with "cargo-nextest
# is present on PATH but the availability probe … failed persistently", and a
# caller's assertion reads far better against a capture certified empty than
# against a silent one.
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
    local _cap="" _rc=0 _i
    for (( _i = 0; _i < _max; _i++ )); do
        _cap="$("$@")" && _rc=0 || _rc=$?
        if [ "$_rc" -eq 0 ] && plan_capture_complete "$_cap"; then
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

# plan_strip_comments <dump>
#
# Prints <dump> with comment lines (leading '#') and empty lines removed,
# preserving order. The fork-free equivalent of
# `printf '%s\n' "$dump" | grep -v '^#'` — no pipe, no subshell, no grep, so it
# carries none of the EINTR surface documented at the top of this file
# (esc-4574-42). Fork-free sibling of plan_count_noncomment_lines above, which
# counts exactly the lines this one keeps.
#
# WHY A CALLER WANTS THIS RATHER THAN THE grep POST-FILTER: the structural
# markers plan_capture_complete certifies on (`# verify.sh plan`,
# `# --- commands`) are themselves comment lines, so stripping comments destroys
# the evidence of completeness. A capture that was truncated mid-plan then reads
# as "pattern absent" and fires a misleading assertion failure instead of a
# retry. The two operations compose in one direction only: certify completeness
# on the RAW dump via capture_print_plan / plan_capture_complete, and only then
# reduce it to command lines with this helper.
#
# Only a '#' in COLUMN 1 starts a comment, matching `grep -v '^#'`: real plan
# command lines carry inline '#' (shell comments inside a quoted `bash -c`,
# fragment identifiers in paths) and must survive intact.
plan_strip_comments() {
    local dump="$1" _out="" _line
    while IFS= read -r _line; do
        case "$_line" in
            '#'* | '') ;;                       # drop comment and empty lines
            *) _out+="${_out:+$'\n'}$_line" ;;  # keep, newline-joined in order
        esac
    done <<< "$dump"
    printf '%s' "$_out"
}
