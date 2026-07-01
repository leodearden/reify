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
#   classification_all_buckets     echoes the 3 valid bucket tokens, one per
#                                   line: pool, intra-run-serial,
#                                   host-exclusive. Single source for the
#                                   bucket enum.
#   classification_bucket <b>      prints the test basenames declared in
#                                   bucket <b> (one per line), reading the
#                                   manifest with comments/blank lines
#                                   stripped.
#   classification_declared_union  prints the sorted-unique union of test
#                                   basenames across the three VALID buckets
#                                   only (a mis-typed bucket token is thereby
#                                   excluded from the union and surfaces as
#                                   coverage drift instead of being silently
#                                   accepted).
#   classification_discovered_set  prints the sorted set of test_*.sh
#                                   basenames that tests/infra/run_all.sh
#                                   would discover: mirrors run_all.sh's
#                                   for-loop exactly (iterate test_*.sh,
#                                   [ -f ] guard, exclude test_helpers.sh by
#                                   basename).
#   classification_coverage_diff   prints the diff between the declared
#                                   union and the discovered set (empty = no
#                                   drift).
#   classification_overlap         prints any test basename declared in more
#                                   than one bucket (empty = no overlap).
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
# 0) when the manifest is absent, mirroring scripts/verify.sh's
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

# classification_bucket <bucket> — print the test basenames declared in
# <bucket>, one per line.
classification_bucket() {
    local _bucket="$1"
    [ -f "$RUN_ALL_CLASSIFICATION_MANIFEST" ] || return 0
    grep -v '^[[:space:]]*#' "$RUN_ALL_CLASSIFICATION_MANIFEST" \
        | grep -v '^[[:space:]]*$' \
        | awk -v b="$_bucket" '$2 == b { print $1 }'
}

# classification_declared_union — sorted-unique union of test basenames
# across the three VALID buckets only. A row whose bucket token is not one
# of classification_all_buckets is thereby excluded from the union, so a
# typo'd bucket surfaces as coverage drift rather than being silently
# accepted.
classification_declared_union() {
    [ -f "$RUN_ALL_CLASSIFICATION_MANIFEST" ] || return 0
    local _b
    while IFS= read -r _b; do
        classification_bucket "$_b"
    done < <(classification_all_buckets) | sort -u
}

# classification_discovered_set — the set of test_*.sh basenames that
# tests/infra/run_all.sh discovers. Mirrors run_all.sh's for-loop EXACTLY:
# iterate test_*.sh in the infra dir, [ -f ]-guard (skip a literal no-match
# glob), exclude test_helpers.sh by basename. Sorted output.
classification_discovered_set() {
    local _dir="$_RUN_ALL_CLASSIFICATION_LIB_DIR"
    local _f _base
    for _f in "$_dir"/test_*.sh; do
        [ -f "$_f" ] || continue
        _base="$(basename "$_f")"
        [ "$_base" = "test_helpers.sh" ] && continue
        printf '%s\n' "$_base"
    done | sort
}

# classification_coverage_diff — diff of the declared union vs the
# discovered set (both sorted). Empty output = no drift (full coverage, no
# orphans).
classification_coverage_diff() {
    local _declared_tmp _discovered_tmp _diff_out
    _declared_tmp="$(mktemp)"
    _discovered_tmp="$(mktemp)"
    classification_declared_union > "$_declared_tmp"
    classification_discovered_set > "$_discovered_tmp"
    _diff_out="$(diff "$_declared_tmp" "$_discovered_tmp" 2>&1 || true)"
    rm -f "$_declared_tmp" "$_discovered_tmp"
    printf '%s' "$_diff_out"
}

# classification_overlap — any test basename declared in MORE THAN ONE
# bucket. Empty output = no overlap.
classification_overlap() {
    [ -f "$RUN_ALL_CLASSIFICATION_MANIFEST" ] || return 0
    grep -v '^[[:space:]]*#' "$RUN_ALL_CLASSIFICATION_MANIFEST" \
        | grep -v '^[[:space:]]*$' \
        | awk '{print $1}' \
        | sort | uniq -d
}
