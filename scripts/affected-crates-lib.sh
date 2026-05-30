#!/usr/bin/env bash
# scripts/affected-crates-lib.sh — maps a changed-file list to the affected
# workspace-crate set (direct crates ∪ their reverse-dependency closure).
#
# Contract references:
#   docs/prds/verify-scope-contract.md
#     §3  C3 — Reverse-closure completeness
#         C4 — Global changes force ALL
#         C5 — Fail safe, fail wide
#     §5  File→crate mapping table
#     §6  Algorithm
#
# Designed to be sourced, not executed directly:
#   source "$(dirname "${BASH_SOURCE[0]}")/affected-crates-lib.sh"
#
# Provides:
#   affected_crates <file>...  prints the affected workspace crate names
#                              (sorted, one per line), or the literal ALL.
#                              Always returns 0.
#
# Sourced by:
#   scripts/verify.sh           (Phase 2 narrowing)
#   tests/infra/test_affected_crates_lib.sh  (drift catcher)

# Source guard — prevent double-sourcing.
if [ "${_REIFY_AFFECTED_CRATES_LIB_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_AFFECTED_CRATES_LIB_SOURCED=1

set -euo pipefail

_AFFECTED_CRATES_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# _is_global <path> — returns 0 (true) if the path is a C4 workspace-global file.
# Matches: root Cargo.toml, Cargo.lock, .cargo/**, tree-sitter-reify/**,
#          rust-toolchain and rust-toolchain.toml.
_is_global() {
    local path="$1"
    case "$path" in
        Cargo.toml|Cargo.lock) return 0 ;;
        .cargo/*)              return 0 ;;
        tree-sitter-reify/*)   return 0 ;;
        rust-toolchain*)       return 0 ;;
    esac
    return 1
}

# _is_noncrate <path> — returns 0 (true) if the path is a non-crate file that
# contributes no crates and must NOT force ALL.
# Matches: docs/** (documentation) and gui/src/** (frontend-only).
_is_noncrate() {
    local path="$1"
    case "$path" in
        docs/*)    return 0 ;;
        gui/src/*) return 0 ;;
    esac
    return 1
}

# affected_crates <file>... — print the affected workspace crate set, one name
# per line, sorted; or print the literal ALL if any C4/C5 condition fires.
# Always returns 0 so callers are safe under set -e and inside $() capture.
affected_crates() {
    # C4: if any arg is a global file, immediately emit ALL.
    local arg
    for arg in "$@"; do
        if _is_global "$arg"; then
            echo ALL
            return 0
        fi
    done

    # Accumulate the direct crate set from crate-mappable paths.
    local direct=()
    for arg in "$@"; do
        if _is_noncrate "$arg"; then
            # Non-crate path: skip, contributes nothing.
            continue
        fi
        # Crate-mappable / unmappable paths handled in later steps.
        : placeholder
    done

    # If no direct crates were accumulated, print nothing.
    if [ "${#direct[@]}" -eq 0 ]; then
        return 0
    fi

    # Emit direct crates sorted (reverse closure expansion comes in later steps).
    printf '%s\n' "${direct[@]}" | sort -u
    return 0
}
