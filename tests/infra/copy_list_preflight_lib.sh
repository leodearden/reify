#!/usr/bin/env bash
# tests/infra/copy_list_preflight_lib.sh — shared transitive-closure copy-list
# preflight for the verify.sh sandbox meta-tests (task #5154).
#
# test_verify_scope.sh, test_scope_boundary.sh, and test_verify_throughput.sh
# each build a `--print-plan` sandbox by cp-ing a subset of scripts/ into a
# throwaway git fixture so a copied verify.sh can source its libs. That cp
# list is hand-maintained, so it silently drifts (copy-list-drift class:
# tasks 4525/4626/4625) the moment verify.sh's sourced-lib closure changes —
# a copied verify.sh that sources a not-copied lib fails as an opaque exit-1
# (self-heal libs) or exit-127 (downstream libs), not a clear message.
#
# This lib factors that preflight into ONE shared, unit-testable helper
# (mirroring the direct-source-only preflight test_verify_throughput.sh had
# inline before task 5154, generalized to the TRANSITIVE closure — a lib
# sourced only by an already-copied lib, e.g. lib_slot_acquire.sh via
# lib_test_semaphore.sh, is invisible to a direct-only grep of verify.sh).
#
# FUNCTIONS (defined when sourced):
#   derive_source_closure <scripts_dir> <entry_basename>
#       Prints (sorted, one per line) the basenames of every .sh lib reached
#       by walking `source`/`.` statements transitively, starting from
#       <scripts_dir>/<entry_basename>. <entry_basename> itself is excluded.
#       Recursion always reads <scripts_dir> (the caller must pass the
#       AUTHORITATIVE, always-complete tree — e.g. $REPO_ROOT/scripts — not
#       a possibly-incomplete fixture copy, or a lib missing from the fixture
#       would wrongly appear to have no children). Cycle-safe (visited-set).
#
#   assert_source_closure_copied <repo_scripts_dir> <fixture_scripts_dir> <entry_basename>
#       Computes derive_source_closure over <repo_scripts_dir>, then checks
#       every member exists under <fixture_scripts_dir>. Prints a
#       "copy-list drift: missing <lib> ..." line to stderr per missing lib
#       and returns 1 if any are missing; returns 0 if the fixture is
#       complete. Does NOT call `exit` — callers wrap `|| exit 1` (mirrors
#       test_verify_throughput.sh's pre-task-5154 inline self-heal) so this
#       function's RED/GREEN behavior stays assertable in-process by
#       test_copy_list_preflight.sh.
#
# PARSER GRAMMAR: a line is a source statement only if it starts (after
# optional leading whitespace) with `source` or `.` followed by whitespace —
# this excludes `#`-comment lines (incl. `# shellcheck source=` directives)
# and lines that merely mention the token in a string (task 4525 false-match
# class). Of those, the LAST `/<name>.sh"` substring is captured — this
# covers every real dir-idiom in scripts/ ($SCRIPT_DIR/, the nested-quote
# $(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/ idiom, and a bare $VAR_DIR/
# idiom). The capture requires a literal `"` immediately after `.sh`, so
# ONLY double-quoted source paths are recognized, by design: every real
# source statement in scripts/ today double-quotes its path, so this is
# latent, not live. An unquoted (`source $DIR/foo.sh`) or single-quoted
# (`source '$DIR/foo.sh'`) source line has no capturable `/name.sh"`
# substring and would be silently dropped from the derived closure — if
# scripts/ ever grows one, its target needs adding to the affected fixture's
# copy list by hand (this preflight would not flag that specific lib's
# absence, though it still catches drift among every double-quoted-sourced
# lib). A line whose sourced path is a bare variable (e.g. `source "$v"`,
# used by lib_slot_acquire.sh and cpu-admit.sh to reach lib_clock_stop.sh)
# has no capturable `/name.sh"` substring and is silently skipped — static
# variable-assignment tracing is fragile, and both real call sites already
# carry their own guarded existence check with a directed error message, so
# lib_clock_stop.sh stays hand-copied at those two fixture cp sites
# (documented limitation, not a gap this preflight covers).
#
# TEST-ONLY FAULT-INJECTION SEAM: when REIFY_COPY_LIST_PREFLIGHT_INJECT_PHANTOM
# is set (whitespace-separated lib basenames), assert_source_closure_copied
# treats those names as additional closure members that will never be found
# in any fixture — proving a calling meta-test actually consults this
# preflight at fixture-build time (vs. merely mentioning it in source).
# Unset by default: inert, so pool/merge (`--scope all`) runs are byte-for-
# byte unaffected. Mirrors run_all.sh's existing fault-injection-seam
# convention; this is a test-support lib so a test seam carries no
# production-code smell.
#
# Usage:
#   [ -f "$SCRIPT_DIR/copy_list_preflight_lib.sh" ] || { echo "ERROR: copy_list_preflight_lib.sh not found"; exit 1; }
#   source "$SCRIPT_DIR/copy_list_preflight_lib.sh"
#   assert_source_closure_copied "$REPO_ROOT/scripts" "$dir/scripts" verify.sh || exit 1

# Source guard — prevent double-sourcing.
if [ "${_REIFY_COPY_LIST_PREFLIGHT_LIB_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_COPY_LIST_PREFLIGHT_LIB_SH_SOURCED=1

derive_source_closure() {
    local scripts_dir="$1" entry="$2"
    local -A visited=()
    local -a queue=("$entry")
    local -a closure=()
    local i=0 cur f child

    visited["$entry"]=1

    while [ "$i" -lt "${#queue[@]}" ]; do
        cur="${queue[$i]}"
        i=$((i + 1))
        f="$scripts_dir/$cur"
        [ -f "$f" ] || continue

        while IFS= read -r child; do
            [ -n "$child" ] || continue
            if [ -z "${visited[$child]:-}" ]; then
                visited["$child"]=1
                closure+=("$child")
                queue+=("$child")
            fi
        done < <(grep -E '^[[:space:]]*(source|\.)[[:space:]]' "$f" \
                     | sed -n 's|.*/\([^"/]*\.sh\)".*|\1|p' || true)
    done

    if [ "${#closure[@]}" -gt 0 ]; then
        printf '%s\n' "${closure[@]}" | sort
    fi
}

assert_source_closure_copied() {
    local repo_scripts_dir="$1" fixture_scripts_dir="$2" entry="$3"
    local -a closure=()
    local lib rc=0

    while IFS= read -r lib; do
        [ -n "$lib" ] || continue
        closure+=("$lib")
    done < <(derive_source_closure "$repo_scripts_dir" "$entry")

    if [ -n "${REIFY_COPY_LIST_PREFLIGHT_INJECT_PHANTOM:-}" ]; then
        local phantom
        for phantom in ${REIFY_COPY_LIST_PREFLIGHT_INJECT_PHANTOM}; do
            closure+=("$phantom")
        done
    fi

    for lib in "${closure[@]+${closure[@]}}"; do
        [ -f "$fixture_scripts_dir/$lib" ] && continue
        echo "copy-list drift: missing $lib (transitive source closure of" \
             "$entry); add: cp \"$repo_scripts_dir/$lib\"" \
             "\"$fixture_scripts_dir/$lib\"" >&2
        rc=1
    done

    return "$rc"
}
