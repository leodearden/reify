#!/usr/bin/env bash
# tests/infra/run-all-classification-lib.sh — shared run_all.sh test-classification logic.
#
# This library is the SINGLE implementation of "which bucket (pool /
# intra-run-serial / host-exclusive) does each tests/infra/test_*.sh belong
# to". It is sourced by:
#   - tests/infra/test_run_all_classification.sh  (drift catcher)
# and its classification_discovered_set function mirrors the discovery
# predicate tests/infra/run_all.sh's own for-loop uses, so the guard's
# discovered set and run_all.sh's discovered set cannot drift apart by
# construction (H2 will refactor run_all.sh's discovery to consume this
# function directly).
#
# Designed to be sourced, not executed directly:
#   source "$(dirname "${BASH_SOURCE[0]}")/run-all-classification-lib.sh"
#
# Provides:
#   classification_all_buckets       echoes the 3 valid bucket tokens, one
#                                     per line: pool, intra-run-serial,
#                                     host-exclusive. Single source for the
#                                     bucket enum.
#   classification_manifest_path     prints the default manifest path
#                                     (RUN_ALL_CLASSIFICATION_MANIFEST).
#   classification_infra_dir         prints the default infra dir (the
#                                     directory containing this library).
#   classification_bucket <b> [m]    prints the test basenames declared in
#                                     bucket <b> (one per line), reading
#                                     manifest [m] (default: the real
#                                     manifest) with comments/blank lines
#                                     stripped.
#   classification_declared_union [m]  prints the sorted-unique union of
#                                     test basenames across the three VALID
#                                     buckets only, from manifest [m]
#                                     (default: the real manifest). A row
#                                     whose bucket token is not valid is
#                                     thereby excluded from the union and
#                                     surfaces as coverage drift instead of
#                                     being silently accepted.
#   classification_discovered_set [d]  prints the sorted set of test_*.sh
#                                     basenames that tests/infra/run_all.sh
#                                     would discover in dir [d] (default:
#                                     this library's own directory): mirrors
#                                     run_all.sh's for-loop exactly (iterate
#                                     test_*.sh, [ -f ] guard, exclude
#                                     test_helpers.sh by basename).
#   classification_coverage_diff [m] [d]  prints the diff between the
#                                     declared union (from manifest [m]) and
#                                     the discovered set (from dir [d])
#                                     (empty = no drift).
#   classification_overlap [m]       prints any test basename declared in
#                                     more than one bucket in manifest [m]
#                                     (default: the real manifest) (empty =
#                                     no overlap).
#   classification_malformed_rows [m]  prints each malformed ROW (the raw
#                                     line, verbatim) of manifest [m]
#                                     (default: the real manifest): a row is
#                                     malformed when it does not have exactly
#                                     2 fields, OR its bucket field (2nd) is
#                                     not one of classification_all_buckets.
#                                     Empty output = every row is
#                                     well-formed. A SINGLE-awk pass over the
#                                     whole manifest (comment/blank lines
#                                     skipped in-awk, valid-bucket set built
#                                     from classification_all_buckets) — no
#                                     per-row forking, unlike a
#                                     read-loop-plus-per-row-assert idiom.
#   classification_stable_empty BASE ACCESSOR [args...]  retry-until-clean
#                                     wrapper: runs ACCESSOR [args...] up to
#                                     load_tolerant_attempts(BASE) times
#                                     (falls back to BASE when
#                                     load_tolerant_attempts is not in scope),
#                                     tolerating a non-zero exit each attempt.
#                                     Returns the CLEAN (empty output, rc 0)
#                                     verdict the first time any attempt is
#                                     clean (prints "", rc 0); otherwise
#                                     prints the LAST (stable) non-empty
#                                     output and returns rc 1. Because the
#                                     wrapped accessors are deterministic
#                                     reads of static files, a
#                                     non-reproducing non-empty result is a
#                                     transient shell hiccup (masked by
#                                     retry), while a STABLE non-empty result
#                                     is genuine (never masked — guard
#                                     integrity is preserved).
#
# All [m]/[d] arguments are OPTIONAL — every accessor defaults to the real
# manifest/dir when omitted, via classification_manifest_path /
# classification_infra_dir. Passing an explicit path (e.g. a synthetic
# fixture manifest) overrides the default for that call only — this is what
# lets test_run_all_classification.sh's non-vacuity self-check exercise the
# parse/diff/overlap logic against injected-drift and injected-overlap
# fixtures without mutating the real manifest.
#
# Environment:
#   RUN_ALL_CLASSIFICATION_MANIFEST  Override the manifest path. Defaults to
#                                     run-all-classification.manifest next to
#                                     this library.
#
# Manifest format: `<test_basename> <bucket>` rows, comment lines (^\s*#) and
# blank lines ignored.
#
# Graceful degradation: every accessor below degrades to empty output (exit
# 0) when its manifest is absent, mirroring scripts/verify.sh's
# `[ -f ] || return 0` idiom. This lets the drift-guard reach its own
# existence/coverage assertions instead of aborting under `set -euo pipefail`.

# Source guard — prevent double-sourcing.
if [ "${_REIFY_RUN_ALL_CLASSIFICATION_LIB_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_RUN_ALL_CLASSIFICATION_LIB_SOURCED=1

_RUN_ALL_CLASSIFICATION_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUN_ALL_CLASSIFICATION_MANIFEST="${RUN_ALL_CLASSIFICATION_MANIFEST:-$_RUN_ALL_CLASSIFICATION_LIB_DIR/run-all-classification.manifest}"

# classification_all_buckets — the single source of truth for the 3-valued
# bucket enum.
classification_all_buckets() {
    printf '%s\n' pool intra-run-serial host-exclusive
}

# classification_manifest_path — the default manifest path.
classification_manifest_path() {
    printf '%s\n' "$RUN_ALL_CLASSIFICATION_MANIFEST"
}

# classification_infra_dir — the default infra directory (this library's own
# directory, the same directory run_all.sh lives in).
classification_infra_dir() {
    printf '%s\n' "$_RUN_ALL_CLASSIFICATION_LIB_DIR"
}

# classification_bucket <bucket> [manifest] — print the test basenames
# declared in <bucket>, one per line, reading [manifest] (default: the real
# manifest).
classification_bucket() {
    local _bucket="$1"
    local _manifest="${2:-$(classification_manifest_path)}"
    [ -f "$_manifest" ] || return 0
    grep -v '^[[:space:]]*#' "$_manifest" \
        | grep -v '^[[:space:]]*$' \
        | awk -v b="$_bucket" '$2 == b { print $1 }'
}

# classification_declared_union [manifest] — sorted-unique union of test
# basenames across the three VALID buckets only, from [manifest] (default:
# the real manifest). A row whose bucket token is not one of
# classification_all_buckets is thereby excluded from the union, so a
# typo'd bucket surfaces as coverage drift rather than being silently
# accepted.
classification_declared_union() {
    local _manifest="${1:-$(classification_manifest_path)}"
    [ -f "$_manifest" ] || return 0
    local _b
    while IFS= read -r _b; do
        classification_bucket "$_b" "$_manifest"
    done < <(classification_all_buckets) | sort -u
}

# classification_discovered_set [dir] — the set of test_*.sh basenames that
# tests/infra/run_all.sh discovers in [dir] (default: this library's own
# directory). Mirrors run_all.sh's for-loop EXACTLY: iterate test_*.sh in the
# infra dir, [ -f ]-guard (skip a literal no-match glob), exclude
# test_helpers.sh by basename. Sorted output.
classification_discovered_set() {
    local _dir="${1:-$(classification_infra_dir)}"
    local _f _base
    for _f in "$_dir"/test_*.sh; do
        [ -f "$_f" ] || continue
        _base="$(basename "$_f")"
        [ "$_base" = "test_helpers.sh" ] && continue
        printf '%s\n' "$_base"
    done | sort
}

# classification_coverage_diff [manifest] [dir] — diff of the declared union
# (from [manifest]) vs the discovered set (from [dir]) (both sorted; each
# defaults to the real manifest/dir when omitted). Empty output = no drift
# (full coverage, no orphans).
classification_coverage_diff() {
    local _manifest="${1:-$(classification_manifest_path)}"
    local _dir="${2:-$(classification_infra_dir)}"
    local _declared_tmp _discovered_tmp _diff_out
    _declared_tmp="$(mktemp)"
    _discovered_tmp="$(mktemp)"
    classification_declared_union "$_manifest" > "$_declared_tmp"
    classification_discovered_set "$_dir" > "$_discovered_tmp"
    _diff_out="$(diff "$_declared_tmp" "$_discovered_tmp" 2>&1 || true)"
    rm -f "$_declared_tmp" "$_discovered_tmp"
    printf '%s' "$_diff_out"
}

# classification_overlap [manifest] — any test basename declared in MORE
# THAN ONE bucket in [manifest] (default: the real manifest). Empty output =
# no overlap.
classification_overlap() {
    local _manifest="${1:-$(classification_manifest_path)}"
    [ -f "$_manifest" ] || return 0
    grep -v '^[[:space:]]*#' "$_manifest" \
        | grep -v '^[[:space:]]*$' \
        | awk '{print $1}' \
        | sort | uniq -d
}

# classification_malformed_rows [manifest] — print each malformed row (the
# raw line, verbatim) of [manifest] (default: the real manifest). A row is
# malformed when it does not have exactly 2 fields, or its bucket field (2nd
# field) is not one of classification_all_buckets. Empty output = every row
# is well-formed.
#
# SINGLE-awk validator (comment/blank lines skipped IN-awk, not via a
# separate grep -v | grep -v pre-filter): this replaces a per-row
# read-loop-plus-per-row-assert idiom (which forks 2-3 subprocesses PER ROW)
# with one awk invocation over the whole file, collapsing the dominant
# fork/pipe surface for a manifest with many rows.
classification_malformed_rows() {
    local _manifest="${1:-$(classification_manifest_path)}"
    [ -f "$_manifest" ] || return 0
    local _buckets
    _buckets="$(classification_all_buckets | tr '\n' ' ')"
    awk -v buckets="$_buckets" '
        BEGIN {
            n = split(buckets, arr, " ")
            for (i = 1; i <= n; i++) valid[arr[i]] = 1
        }
        /^[[:space:]]*#/ { next }
        /^[[:space:]]*$/ { next }
        { if (NF != 2 || !($2 in valid)) print }
    ' "$_manifest"
}

# classification_stable_empty BASE ACCESSOR [args...] — retry-until-clean
# wrapper. Runs `ACCESSOR [args...]` up to load_tolerant_attempts(BASE)
# attempts (BASE itself when load_tolerant_attempts is not in scope),
# tolerating a non-zero exit each attempt (never aborts the caller under
# `set -e`). Returns the CLEAN (empty output, rc 0) verdict the first time
# any attempt is clean; otherwise returns the LAST (stable) non-empty output
# with rc 1. A fixed (not load-scaled) short yield separates attempts on the
# non-clean path only.
classification_stable_empty() {
    local _base="$1"
    shift

    local _attempts
    if declare -F load_tolerant_attempts >/dev/null 2>&1; then
        _attempts="$(load_tolerant_attempts "$_base")"
    else
        _attempts="$_base"
    fi
    # Validate to a positive integer: empty/non-numeric -> fall back to
    # BASE; if BASE is ALSO empty/non-numeric -> floor to 1. Mirrors the
    # load_tolerance_lib BASE-validation idiom, but (unlike
    # load_tolerant_attempts, which may just echo a raw BASE back) this loop
    # bound must end up a real positive integer.
    case "$_attempts" in
        ''|*[!0-9]*) _attempts="$_base" ;;
    esac
    case "$_attempts" in
        ''|*[!0-9]*) _attempts=1 ;;
    esac
    [ "$_attempts" -gt 0 ] 2>/dev/null || _attempts=1

    local _out="" _rc _attempt
    for ((_attempt = 1; _attempt <= _attempts; _attempt++)); do
        _rc=0
        _out="$("$@" 2>/dev/null)" || _rc=$?
        if [ "$_rc" -eq 0 ] && [ -z "$_out" ]; then
            printf ''
            return 0
        fi
        if [ "$_attempt" -lt "$_attempts" ]; then
            sleep 0.1 2>/dev/null || true
        fi
    done
    printf '%s' "$_out"
    return 1
}
