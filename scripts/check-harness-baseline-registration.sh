#!/usr/bin/env bash
# scripts/check-harness-baseline-registration.sh — diff-scoped harness-layout
# baseline-registration drift gate (task 5300).
#
# WHY (the drift this closes). tests/infra/harness-layout-baseline.manifest
# (task 5265's C2 anti-re-accretion grandfather baseline) drifted when task 4370
# landed 3 new standalone crates/<c>/tests/<f>.rs files WITHOUT matching baseline
# rows. The existing guard tests/infra/test_harness_kloc_cap.sh Section 5 is a
# WHOLE-TREE live scan: once that drift was on main it went RED for EVERY
# unrelated task whose post-merge verify merely rebased onto the drifted tip —
# repeated merge thrash (5260/5266/5288), each re-diagnosing the same root cause.
# A whole-tree guard cannot tell "your diff introduced the drift" from "you
# rebased onto an already-drifted main," so it re-fires on innocent downstream
# tasks.
#
# WHAT (diff-scoped, fires at the source). This gate considers ONLY files ADDED
# in THIS diff (args / stdin, or --from-git self-derivation). A downstream
# rebaser's own diff adds no test file -> it stays GREEN even if main is
# momentarily drifted -> no thrash. The offending diff (adds file, omits row) ->
# RED -> blocked at source. Membership is checked against the CURRENT on-disk
# baseline: a newly-added file's canonical path crates/<c>/tests/<f>.rs is
# present there ONLY if the same diff added the row, so current-baseline
# membership is exactly "row landed in the same diff" (no fragile +/- manifest
# line-diff).
#
# Companion to task 5256's diff-consuming scripts/verify-pipeline-guard.sh
# (mirrors its args-or-stdin input contract + leading-./ normalization) and
# structured like task 5252's scripts/check-infra-classification-manifest.sh (a
# cheap pure-bash early verify gate). Shares the 5 consolidatable crates, 7
# override stems, and baseline predicates with test_harness_kloc_cap.sh via
# tests/infra/harness-layout-lib.sh (single source of truth — cannot diverge).
#
# INPUT
#   positional args   repo-relative added-file paths (git diff --name-only form)
#   stdin             if no args, newline-separated repo-relative paths
#   --from-git        self-derive the added-file set from git (see step-6)
#
# OUTPUT (structured verdict grammar, rule-(c) style — machine-parseable, not a
# log-scrape):
#   HARNESS_BASELINE_REG FAIL crate=<c> file=<path> reason=unregistered-standalone
#   HARNESS_BASELINE_REG PASS
#   HARNESS_BASELINE_REG SUMMARY added=<n> violations=<v>
#
# EXIT: 0 when clean (v==0), 1 when any violation (v>0). Honors
# REIFY_HARNESS_LAYOUT_BASELINE (via the lib) for testability.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LIB="$REPO_ROOT/tests/infra/harness-layout-lib.sh"

# Fail-open if our own shared lib is somehow absent — never a false RED. Emit a
# well-formed empty SUMMARY so downstream parsers still see a verdict.
if [ ! -f "$LIB" ]; then
    printf '[error] harness-layout-lib.sh not found at %s\n' "$LIB" >&2
    printf 'HARNESS_BASELINE_REG SUMMARY added=0 violations=0\n'
    exit 0
fi
# shellcheck source=tests/infra/harness-layout-lib.sh
source "$LIB"

# _emit <VERDICT> <field>... — one canonical structured verdict line (own tag,
# distinct from HARNESS_KLOC_CAP so consumers/logs stay unambiguous).
_emit() {
    local verdict="$1"
    shift
    printf 'HARNESS_BASELINE_REG %s %s\n' "$verdict" "$*"
}

# _check_candidates <path>... — the PURE core. Classify each candidate
# repo-relative added path; emit a structured FAIL per unregistered in-scope
# standalone; end with a PASS (when clean) and always a SUMMARY. Returns 0 iff
# no violations. On-disk existence + baseline membership are resolved relative
# to the current working directory / the lib's baseline path (verify.sh cds to
# REPO_ROOT before running plan entries; the tests cd into their fixture tree).
_check_candidates() {
    local added=0 violations=0
    local raw path crate rest
    for raw in "$@"; do
        [ -n "$raw" ] || continue
        # Defensive leading-./ normalization (git diff --name-only emits clean
        # paths; matches verify-pipeline-guard.sh's input contract).
        path="${raw#./}"
        added=$((added + 1))
        # Only files that EXIST on disk are candidates — skipping non-existent
        # paths transparently handles deletes / renames-away in the diff.
        [ -e "$path" ] || continue
        # In-scope standalone? crates/<one-of-5>/tests/<base>.rs, top-level,
        # base not harness_*, stem not one of the 7 overrides.
        harness_layout_in_scope_standalone "$path" || continue
        # Registered in the CURRENT baseline (== row landed in the same diff)?
        if harness_layout_baseline_contains "$path"; then
            continue
        fi
        rest="${path#crates/}"
        crate="${rest%%/*}"
        _emit FAIL "crate=$crate" "file=$path" "reason=unregistered-standalone"
        violations=$((violations + 1))
    done

    if [ "$violations" -eq 0 ]; then
        _emit PASS
    fi
    _emit SUMMARY "added=$added" "violations=$violations"
    [ "$violations" -eq 0 ]
}

# _derive_added_from_git — print the ADDED files (--diff-filter=A) of the current
# merge delta, one per line, or return NON-ZERO when the base is underivable (the
# caller then fails open). Mirrors verify.sh _derive_merge_delta, plus a
# merge-base fallback for the linear task-branch tier:
#
# RENAMES: --no-renames is passed on every diff so a file RENAMED into an
# in-scope slot (e.g. git mv crates/<other>/tests/x.rs into
# crates/<consolidatable>/tests/x.rs) surfaces as an ADD of the destination
# (rename -> add+delete decomposition; the delete is then dropped by
# --diff-filter=A) instead of a rename (R). Git's default detection
# (diff.renames=true) would otherwise classify it R and hide it from this
# added-only gate, letting a rename-in introduce an unregistered standalone that
# only the whole-tree backstop catches post-merge. --no-renames closes that
# path at the source too, deterministically (independent of ambient diff.renames).
#   MERGE_HEAD present (hook, merge in progress)  -> git diff HEAD MERGE_HEAD
#   HEAD has >=2 parents (speculative merge landed) -> git diff HEAD^1 HEAD
#   else (linear task branch)                     -> git diff merge-base(main,HEAD) HEAD
# Uses CWD-relative git (NOT -C REPO_ROOT) so it inspects the repo it is run in
# (REPO_ROOT in the verify plan; the fixture repo under test). All git stderr
# suppressed; any failure / unresolvable base / detached-or-unborn HEAD => the
# base is underivable => fail open (never a false RED). AUTHORITATIVE at the
# merge tier (HEAD is always a 2-parent merge / MERGE_HEAD present there — the
# tier that decides landing); best-effort at the linear branch tier.
_derive_added_from_git() {
    local mh
    mh="$(git rev-parse --git-path MERGE_HEAD 2>/dev/null || echo '')"
    if [ -n "$mh" ] && [ -f "$mh" ]; then
        git diff --no-renames --diff-filter=A --name-only HEAD MERGE_HEAD 2>/dev/null || return 1
        return 0
    fi
    local parents arr
    parents="$(git rev-list --parents -n1 HEAD 2>/dev/null)" || return 1
    # "<sha> <parent1> <parent2> ..."; >=3 tokens => a merge commit (>=2 parents).
    # shellcheck disable=SC2206
    arr=($parents)
    if [ "${#arr[@]}" -ge 3 ]; then
        git diff --no-renames --diff-filter=A --name-only HEAD^1 HEAD 2>/dev/null || return 1
        return 0
    fi
    local base
    base="$(git merge-base main HEAD 2>/dev/null)" || return 1
    [ -n "$base" ] || return 1
    git diff --no-renames --diff-filter=A --name-only "$base" HEAD 2>/dev/null || return 1
    return 0
}

# --from-git: self-derive the ADDED-file set from git and feed it into the pure
# core. An underivable base derives nothing and exits 0 (fail open).
if [ "${1:-}" = "--from-git" ]; then
    _added_raw=""
    _rc=0
    if _added_raw="$(_derive_added_from_git)"; then
        _paths=()
        while IFS= read -r _line; do
            [ -n "$_line" ] && _paths+=("$_line")
        done <<< "$_added_raw"
        if [ "${#_paths[@]}" -gt 0 ]; then
            _check_candidates "${_paths[@]}" || _rc=$?
        else
            _check_candidates || _rc=$?
        fi
    else
        _check_candidates || _rc=$?
    fi
    exit "$_rc"
fi

# Input dispatch: positional args, else newline-separated stdin.
if [ "$#" -gt 0 ]; then
    _check_candidates "$@"
else
    _paths=()
    while IFS= read -r _line; do
        [ -n "$_line" ] && _paths+=("$_line")
    done
    if [ "${#_paths[@]}" -gt 0 ]; then
        _check_candidates "${_paths[@]}"
    else
        _check_candidates
    fi
fi
