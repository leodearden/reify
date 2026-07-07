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

_AFFECTED_CRATES_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Shared compile-closure primitive (_reify_compile_closure): _reverse_closure
# below delegates to it instead of carrying its own copy of the
# adj_normal/adj_dev + normal_closure model. occt-scope-lib.sh's own source
# guard makes this a no-op if verify.sh (or another caller) already sourced
# it first.
[ -f "$_AFFECTED_CRATES_LIB_DIR/occt-scope-lib.sh" ] || { echo "affected-crates-lib.sh: ERROR — scripts/occt-scope-lib.sh not found next to affected-crates-lib.sh" >&2; return 1; }
# shellcheck source=scripts/occt-scope-lib.sh
source "$_AFFECTED_CRATES_LIB_DIR/occt-scope-lib.sh"

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
# Matches: docs/** (documentation), gui/src/** (frontend-only), and
# tests/infra/** (shell/python infra test scripts — these run as their own
# verify step and never affect Rust crate compilation or test outcomes, so a
# tests/infra-only diff must narrow to no crates rather than hitting the C5
# fail-wide-to-ALL path via an unmappable path).
_is_noncrate() {
    local path="$1"
    case "$path" in
        docs/*)        return 0 ;;
        gui/src/*)     return 0 ;;
        tests/infra/*) return 0 ;;
    esac
    return 1
}

# _file_to_crate <path> — map a crate-owned path to its crate name, or print
# nothing if the path is not under a known crate location.
# Mapping rules (§5):
#   crates/<name>/**  -> <name>
#   gui/src-tauri/**  -> reify-gui
_file_to_crate() {
    local path="$1"
    case "$path" in
        crates/*/*)
            # Extract the crate name: crates/<name>/...
            local rest="${path#crates/}"
            echo "${rest%%/*}"
            ;;
        gui/src-tauri/*)
            echo "reify-gui"
            ;;
        *)
            # No mapping found.
            ;;
    esac
}

# _reverse_closure — read seed crate names from stdin (one per line), emit the
# cargo-accurate affected workspace-crate set (seeds plus every workspace
# crate whose test-compile-closure pulls in a seed), sorted-unique, one per
# line.
#
# Delegates to _reify_compile_closure (scripts/occt-scope-lib.sh, sourced
# above) — the single shared implementation of the adj_normal/adj_dev +
# normal_closure compile-closure model, also used by occt_touching_set. This
# is the reverse ("which crates pull in a seed") framing of that same model,
# reached here by passing the stdin seed names as the helper's argv instead
# of occt_touching_set's hardcoded seed.
#
# tests/infra/test_affected_crates_lib.sh asserts affected_crates(occt-seed)
# == occt_touching_set as a regression guard: since both now delegate to the
# same helper, this fails loudly if either caller's seed handling regresses.
#
# On any cargo failure or malformed-metadata error from the shared helper,
# prints ALL (C5).
_reverse_closure() {
    local seeds
    seeds="$(cat)"
    [ -n "$seeds" ] || return 0

    # Collect metadata once; guard failure -> ALL.
    local meta
    meta="$(cargo metadata --format-version 1 2>/dev/null)" || { echo ALL; return 0; }
    [ -n "$meta" ] || { echo ALL; return 0; }

    # Convert the newline-separated seeds into a bash array for safe argv
    # expansion into _reify_compile_closure.
    local seed_args=()
    local s
    while IFS= read -r s; do
        [ -n "$s" ] && seed_args+=("$s")
    done <<< "$seeds"

    printf '%s\n' "$meta" | _reify_compile_closure "${seed_args[@]}" 2>/dev/null || { echo ALL; return 0; }
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
    local crate
    for arg in "$@"; do
        if _is_noncrate "$arg"; then
            # Non-crate path: skip, contributes nothing.
            continue
        fi
        crate="$(_file_to_crate "$arg")"
        if [ -n "$crate" ]; then
            direct+=("$crate")
        else
            # C5: unmappable path — fail wide.
            echo ALL
            return 0
        fi
    done

    # If no direct crates were accumulated, print nothing.
    if [ "${#direct[@]}" -eq 0 ]; then
        return 0
    fi

    # Expand the direct crate set through the reverse-dependency closure, then
    # emit sorted-unique (one crate per line).
    printf '%s\n' "${direct[@]}" | _reverse_closure
    return 0
}
