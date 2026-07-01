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
