#!/usr/bin/env bash
# scripts/occt-scope-lib.sh — shared OCCT-touching crate-set logic, and the
# bash entry point to the shared cargo-accurate compile closure.
#
# This library is the SINGLE implementation of "which workspace crates touch
# reify-kernel-occt". It is sourced by both:
#   - scripts/verify.sh                  (decides whether to run the gated pass)
#   - tests/infra/test_occt_gated_scope.sh (drift catcher)
# so the declared set and the cargo-metadata-derived set each have exactly one
# definition — divergence between the verify entrypoint and the drift test
# becomes impossible by construction.
#
# It also names `_reify_compile_closure`, the bash entry point to the
# cargo-accurate compile-closure model (normal/build vs dev edges), whose one
# home is compile_closure in scripts/reify_path_attribution.py.
# scripts/affected-crates-lib.sh sources this file to reach it, so
# occt_touching_set and affected_crates share that one model.
#
# Designed to be sourced, not executed directly:
#   source "$(dirname "${BASH_SOURCE[0]}")/occt-scope-lib.sh"
#
# Provides:
#   occt_declared_set       prints the declared OCCT-touching crates (one per
#                           line), reading scripts/occt-touching-crates.txt
#                           with comments/blank lines stripped and whitespace
#                           trimmed.
#   occt_touching_set       prints the cargo-metadata-derived OCCT-touching
#                           workspace members (sorted, one per line).
#   _reify_compile_closure  the shared compile closure: reads `cargo
#                           metadata --format-version 1` JSON on stdin and
#                           seed crate NAMES from argv, prints the workspace
#                           member names whose test-compile closure reaches a
#                           seed (sorted-unique, one per line). Exits non-zero
#                           on malformed input (no try/except) — callers
#                           apply their own error policy at the call site.
#
# Environment:
#   OCCT_TOUCHING_CRATES_FILE  Override the declared-list path. Defaults to
#                              occt-touching-crates.txt next to this library.
#
# Requires scripts/reify_path_attribution.py next to this file: sourcing fails
# without it, naming it, rather than leaving a caller that silences
# _reify_compile_closure's stderr to fail opaquely later.

# Source guard — prevent double-sourcing.
if [ "${_REIFY_OCCT_SCOPE_LIB_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_OCCT_SCOPE_LIB_SOURCED=1

_OCCT_SCOPE_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[ -f "$_OCCT_SCOPE_LIB_DIR/reify_path_attribution.py" ] || { echo "occt-scope-lib.sh: ERROR — scripts/reify_path_attribution.py not found next to occt-scope-lib.sh" >&2; return 1; }
OCCT_TOUCHING_CRATES_FILE="${OCCT_TOUCHING_CRATES_FILE:-$_OCCT_SCOPE_LIB_DIR/occt-touching-crates.txt}"

# occt_declared_set — print the declared OCCT-touching crate list, one crate per
# line, with comment lines (^\s*#) and blank lines removed and surrounding
# whitespace trimmed. This mirrors the historical inline computation in
# tests/infra/test_occt_gated_scope.sh.
occt_declared_set() {
    if [ ! -f "$OCCT_TOUCHING_CRATES_FILE" ]; then
        echo "ERROR: occt-touching-crates.txt not found at $OCCT_TOUCHING_CRATES_FILE" >&2
        return 1
    fi
    grep -v '^\s*#' "$OCCT_TOUCHING_CRATES_FILE" \
        | grep -v '^\s*$' \
        | sed 's/^[[:space:]]*//;s/[[:space:]]*$//'
}

# _reify_compile_closure SEED... < metadata.json — see this file's header. It
# inherits the caller's stdin by design: every caller pipes the metadata in.
_reify_compile_closure() { python3 -S "$_OCCT_SCOPE_LIB_DIR/reify_path_attribution.py" closure "$@"; }

# occt_touching_set — derive the ACTUAL OCCT-touching set from a SINGLE
# `cargo metadata` invocation (the workspace-unified resolve graph), via the
# shared _reify_compile_closure primitive above, seeded with the OCCT crate
# itself. Prints the OCCT-touching workspace member names, sorted, one per
# line.
#
# Using the workspace-unified resolve graph is both faster (one cargo process
# instead of one per workspace member) and more accurate: workspace feature
# unification can activate optional deps (e.g. a future crate that enables the
# reify-gui 'gui' feature would pull in reify-kernel-occt via a normal dep, but
# per-crate `cargo tree -p <crate>` only sees each crate's own default features
# and would miss it).
occt_touching_set() {
    cargo metadata --format-version 1 2>/dev/null | _reify_compile_closure reify-kernel-occt
}
