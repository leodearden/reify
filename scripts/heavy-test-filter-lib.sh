#!/usr/bin/env bash
# scripts/heavy-test-filter-lib.sh — single source of truth for the `heavy`
# nextest filterset (the offline/gate test partition, PRD
# docs/prds/offline-deep-test-lane.md §3/DA4).
#
# This library is the SINGLE definition of the `heavy` binary-level nextest
# filter expression. It is sourced by:
#   - scripts/verify.sh (A2)   offline role: -E "$REIFY_HEAVY_NEXTEST_FILTER" --run-ignored all
#   - scripts/verify.sh (A4)   gate role, iff REIFY_GATE_EXCLUDE_HEAVY=1:
#                              -E "not ($REIFY_HEAVY_NEXTEST_FILTER)"
#   - tests/infra/test_heavy_filter_atoms.sh (A1)      resolve-to-disk drift-guard
#   - tests/infra/test_verify_offline_partition.sh (A6) partition drift-guard
# so the offline view and the negated-gate view can never drift apart from one
# another — both read this one positive expression; A4 derives the negation at
# the consumer rather than storing a second expression.
#
# Designed to be sourced, not executed directly:
#   source "$(dirname "${BASH_SOURCE[0]}")/heavy-test-filter-lib.sh"
#
# Provides:
#   REIFY_HEAVY_NEXTEST_FILTER   exported string constant — a fully-parenthesized,
#                                or-joined, binary-level nextest filterset
#                                expression (`package(X) & binary(Y)` atoms).
#                                Each atom MUST resolve to a real
#                                crates/<pkg>/tests/<bin>.rs file on disk (see
#                                the A1 drift-guard). The drift-guard's
#                                resolve-to-disk check assumes the Cargo
#                                package name equals its crates/ directory
#                                name -- if a future atom's package lives in a
#                                differently-named directory, see the caveat
#                                note in tests/infra/test_heavy_filter_atoms.sh
#                                (Assertion E) before adding it here.
#
# Safe to source multiple times: this file only defines/exports one constant
# and has no other side effects.

# Source guard — prevent double-sourcing.
if [ "${_REIFY_HEAVY_TEST_FILTER_LIB_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_HEAVY_TEST_FILTER_LIB_SOURCED=1

# The `heavy` set (PRD §3): reify-solver-elastic {determinism,
# analytical_validation, modal_benchmarks} + reify-eval {tensegrity_t0a} +
# reify-eval-fea-tests {buckling_smoke, fea_diagnostics_e2e}. Positive
# expression only — the gate view derives `not ($REIFY_HEAVY_NEXTEST_FILTER)`
# at the A4 consumer, so this is the one place membership can ever change.
export REIFY_HEAVY_NEXTEST_FILTER='(package(reify-solver-elastic) & binary(determinism)) | (package(reify-solver-elastic) & binary(analytical_validation)) | (package(reify-solver-elastic) & binary(modal_benchmarks)) | (package(reify-eval-fea-tests) & binary(buckling_smoke)) | (package(reify-eval) & binary(tensegrity_t0a)) | (package(reify-eval-fea-tests) & binary(fea_diagnostics_e2e))'
