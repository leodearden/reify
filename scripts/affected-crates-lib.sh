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

# _is_inert <path> — returns 0 (true) if the path is documentation or
# configuration: it needs no heavy checks and belongs to no crate of its own.
# Matches: docs/**, *.md, *.yaml, *.yml.
#
# SPOT (task 7427). This is the SINGLE source for that class, consulted by
# BOTH consumers that used to carry their own copy:
#   * _is_noncrate below (the crate-attribution side), and
#   * scripts/verify.sh's decide_scope (the heavy-check side), whose `*)`
#     catch-all defers here instead of matching its own glob list.
# The two lists had drifted — decide_scope matched all four patterns,
# _is_noncrate only docs/**. The drift was invisible on a pure-docs diff
# (RUN_RUST=0 skips the closure entirely) but destroyed narrowing on a MIXED
# one: a top-level *.md riding along with a crate edit was unmappable, so C5
# widened the whole closure to ALL. tests/infra/test_affected_crates_lib.sh's
# INERT-SPOT battery pins the two classifications together.
#
# DELIBERATELY NOT ANCHORED, because BOTH consumers place crate ATTRIBUTION
# ahead of it — decide_scope through its `crates/*)` arm, affected_crates
# through _file_to_crate in its accumulation loop — so a crate-OWNED *.md
# (reify-mcp's include_str!-ed chunks, reify-doc's snapshot fixtures) is
# never reached by this predicate on either side and keeps mapping to its
# owning crate. That shared attribute-first precedence IS the SPOT; it is
# recorded here once rather than restated at each call site.
#
# The same precedence keeps decide_scope's other earlier arms winning over
# this rule too — gui/* (a gui/*.md is GUI work), tests/prd-gate/fixtures/*.ri
# and the docs/gui-event-channels.md carve-out.
_is_inert() {
    local path="$1"
    case "$path" in
        docs/*)                 return 0 ;;
        *.md|*.yaml|*.yml)      return 0 ;;
    esac
    return 1
}

# _is_noncrate <path> — returns 0 (true) if the path is a non-crate file that
# contributes no crates and must NOT force ALL.
# Matches: everything _is_inert covers (documentation/configuration), plus
# gui/src/** (frontend-only) and tests/infra/** (shell/python infra test
# scripts — these run as their own verify step and never affect Rust crate
# compilation or test outcomes, so a tests/infra-only diff must narrow to no
# crates rather than hitting the C5 fail-wide-to-ALL path via an unmappable
# path).
_is_noncrate() {
    local path="$1"
    _is_inert "$path" && return 0
    case "$path" in
        gui/src/*)     return 0 ;;
        tests/infra/*) return 0 ;;
    esac
    return 1
}

# _RI_CORPUS_CRATES — the crates whose COMPILED tests read the examples/ .ri
# corpus, as SEED crates for the reverse closure (task 7427).
#
# HOW MEMBERSHIP IS KEPT HONEST: not by hand. RI-CORPUS-DRIFT in
# tests/infra/test_affected_crates_lib.sh derives the set from the repo's own
# Rust sources — every crate with a non-comment line naming an
# `examples/<path>.ri` literal — and asserts DERIVED ⊆ DECLARED. Re-run that
# test rather than editing this line from memory.
#
# SUBSET, not equality, and the asymmetry is deliberate: an extra DECLARED
# crate only ever WIDENS the closure, which is the direction of error C5
# already blesses. An UNDECLARED reader is the real regression — its tests
# would be narrowed AWAY by an edit to the very fixture they read.
_RI_CORPUS_CRATES="reify-cli reify-compiler reify-eval reify-eval-fea-tests"

# _file_to_crate <path> — map a crate-owned path to its crate name, or print
# nothing if the path is not under a known crate location.
# Mapping rules (§5):
#   crates/<name>/**  -> <name>
#   gui/src-tauri/**  -> reify-gui
#   examples/**/*.ri  -> _RI_CORPUS_CRATES (corpus seeds)
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
        examples/*.ri)
            # A corpus leaf: emit the declared reader crates as ordinary
            # seeds, so affected_crates feeds them through _reverse_closure
            # exactly like any other direct crate.
            #
            # A bash `case` glob's `*` spans `/`, so this one arm covers the
            # nested shapes too (examples/auto/*.ri,
            # examples/ambient_default_material/*.ri, …) — which is most of
            # the tree.
            #
            # Scoped to .ri leaves, NOT to the directory: non-.ri, non-inert
            # content under examples/ (a .gcode datum, a .gitkeep) has no
            # declared reader, so it deliberately falls through to the C5
            # fail-wide arm in affected_crates.
            # Word-split is the point (one seed per line, the contract every
            # other arm honours). Safe unquoted: the declared value is a
            # literal in this file and carries no glob metacharacter.
            # shellcheck disable=SC2086
            printf '%s\n' $_RI_CORPUS_CRATES
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
    #
    # --locked stops cargo from REWRITING Cargo.lock: it refuses to resolve a
    # stale/missing lock instead of silently updating it, closing the
    # tracked-file mid-commit-mutation risk. --offline adds the guarantee
    # --locked does NOT imply — no network I/O at all: even against a valid,
    # unchanged lock, a cold registry/index cache would otherwise let cargo
    # fetch dependency manifests. Together they make this call hermetic,
    # which matters because it now runs on the pre-commit-hook tier and under
    # verify.sh --print-plan, where an unbounded index fetch is a hook-stall
    # hazard. (--frozen is exactly this pair spelled as one flag; the two-flag
    # form is kept so each guarantee stays legible at the call site.)
    #
    # Accepted tradeoff: a genuinely cold registry cache no longer stalls, it
    # fails fast (measured 0.15-0.35s) into the C5 fail-wide ALL path just
    # below. docs/prds/verify-scope-contract.md §3 C5 already specifies ALL as
    # the answer to "cannot compute the affected set", so the failure only
    # ever WIDENS the verify scope and can never produce a false PASS. That is
    # the whole containment argument, and it holds unconditionally.
    #
    # Two weaker claims are deliberately NOT made (both were asserted here and
    # corrected in review). C4 does NOT make the cold case unreachable: it
    # returns ALL only when Cargo.lock is in THIS run's changed-file set, so a
    # dev who pulls a Cargo.lock bump and then commits only a source file
    # reaches here with a cache that is cold relative to the lock, C4 never
    # having fired. And the widening does not self-heal by elapsed time: what
    # repopulates the registry is the `cargo check` / `cargo clippy` passes a
    # RUN_RUST=1 verify goes on to run, neither of which passes --offline. A
    # --print-plan probe or a RUN_RUST=0 docs-tier commit never shells out to
    # a networked cargo, so it keeps reporting ALL — harmlessly, per C5 —
    # until a real build runs.
    local meta
    meta="$(cargo metadata --format-version 1 --locked --offline 2>/dev/null)" || {
        echo "affected-crates-lib.sh: cargo metadata --locked --offline failed (stale/missing Cargo.lock, or a cold registry cache) — falling back to ALL" >&2
        echo ALL
        return 0
    }
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
    # ATTRIBUTION FIRST, then the non-crate classes (see _is_inert's header for
    # why that order is the contract on both sides of the SPOT).
    local direct=()
    local crate
    for arg in "$@"; do
        crate="$(_file_to_crate "$arg")"
        if [ -n "$crate" ]; then
            direct+=("$crate")
        elif _is_noncrate "$arg"; then
            # Non-crate path: skip, contributes nothing.
            continue
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
