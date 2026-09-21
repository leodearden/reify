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
#                              (sorted, one per line), the literal ALL, or
#                              NOTHING — three outcomes, all load-bearing.
#                              An empty print means "this file list provably
#                              touches zero crates" and is a POSITIVE answer,
#                              not a failure to answer; see the function's own
#                              header. Always returns 0.
#   reify_is_inert_path <path> true iff the path is documentation or
#                              configuration (docs/**, *.md, *.yaml, *.yml).
#                              The shared definition of that class; verify.sh's
#                              decide_scope is its second consumer.
#
# Unprefixed names are the declared interface. A leading underscore means
# private to affected_crates — do not add a consumer outside this file without
# promoting the helper here first.
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

# reify_is_inert_path <path> — returns 0 (true) if the path is documentation or
# configuration: it needs no heavy checks and belongs to no crate of its own.
# Matches: docs/**, *.md, *.yaml, *.yml. Contract: §3 "Inert paths are ONE
# list" in docs/prds/verify-scope-contract.md.
#
# SPOT (task 7427): the single definition of that class, for the two consumers
# that used to carry their own drifting copies — _is_noncrate below (crate
# attribution) and scripts/verify.sh's decide_scope (heavy-check selection).
# tests/infra/test_affected_crates_lib.sh's INERT-SPOT battery pins them
# together.
#
# The suffix patterns are DELIBERATELY NOT ANCHORED, which is safe only because
# both consumers place crate ATTRIBUTION ahead of this predicate — decide_scope
# through its `crates/*)` arm, affected_crates through _file_to_crate in its
# accumulation loop — so a crate-OWNED *.md never reaches here and keeps mapping
# to its owning crate. That shared attribute-first precedence is the load-bearing
# half of the SPOT, and is recorded here rather than at each call site; §5 of the
# contract records what it protects.
reify_is_inert_path() {
    local path="$1"
    case "$path" in
        docs/*)                 return 0 ;;
        *.md|*.yaml|*.yml)      return 0 ;;
    esac
    return 1
}

# _is_noncrate <path> — returns 0 (true) if the path is a non-crate file that
# contributes no crates and must NOT force ALL.
# Matches: everything reify_is_inert_path covers (documentation/configuration),
# plus gui/src/** (frontend-only) and tests/infra/** (shell/python infra test
# scripts — these run as their own verify step and never affect Rust crate
# compilation or test outcomes, so a tests/infra-only diff must narrow to no
# crates rather than hitting the C5 fail-wide-to-ALL path via an unmappable
# path).
_is_noncrate() {
    local path="$1"
    reify_is_inert_path "$path" && return 0
    case "$path" in
        gui/src/*)     return 0 ;;
        tests/infra/*) return 0 ;;
    esac
    return 1
}

# _RI_CORPUS_CRATES — the crates whose COMPILED tests read the examples/ .ri
# corpus, as SEED crates for the reverse closure (task 7427). Contract: §3
# "Corpus mapping" in docs/prds/verify-scope-contract.md.
#
# HOW MEMBERSHIP IS KEPT HONEST: not by hand. RI-CORPUS-DRIFT in
# tests/infra/test_affected_crates_lib.sh sweeps every workspace member's Rust
# sources and asserts DERIVED ⊆ DECLARED; that block owns the reader shapes it
# recognises and why the subset direction is the safe one. Re-run it rather than
# editing this line from memory. Each crate is declared in its own right, never
# left to arrive transitively through another seed's dep edge.
_RI_CORPUS_CRATES="reify-cli reify-compiler reify-eval reify-eval-fea-tests reify-gui"

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
            # A corpus leaf: emit the declared reader crates as ordinary seeds,
            # fed through _reverse_closure like any other direct crate. A bash
            # `case` glob's `*` spans `/`, so nested corpus dirs land here too;
            # scoped to .ri LEAVES, so non-.ri content under examples/ (a
            # .gcode datum, a .gitkeep) keeps falling to the C5 fail-wide arm.
            # Word-split is the point (one seed per line) and is safe: the
            # declared value is a literal here with no glob metacharacter.
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

    # SEEDS IN, NOTHING OUT is a FAILURE to attribute, not an answer — so it is
    # C5, not an empty print. _reify_compile_closure resolves seed NAMES through
    # the metadata's name->id map and silently skips a name that maps to no
    # package (`name_to_ids.get(sn, [])`), exiting 0 with no output. A RESOLVABLE
    # workspace seed is always a member of its own compile closure, so an empty
    # result from a non-empty seed list can only mean a seed did not resolve.
    # Reachable shapes: a file added under a typo'd or not-yet-declared crate
    # directory, a crate directory whose package name differs from the directory
    # name (nothing pins dir == package name), a path under a crate whose
    # `members` entry was already removed so C4 never fires.
    #
    # This is what keeps affected_crates()' empty print SINGLE-SOURCED at the
    # `${#direct[@]} -eq 0` early return — the property its header claims and
    # verify.sh's computed-empty arm relies on. Without it a crate-attributed
    # path whose crate did not resolve would arrive at that consumer wearing the
    # from-diff licence and be read as "provably zero crates".
    local closure
    closure="$(printf '%s\n' "$meta" | _reify_compile_closure "${seed_args[@]}" 2>/dev/null)" || { echo ALL; return 0; }
    if [ -z "$closure" ]; then
        echo "affected-crates-lib.sh: seed crate(s) resolved to no workspace package — falling back to ALL" >&2
        echo ALL
        return 0
    fi
    printf '%s\n' "$closure"
}

# affected_crates <file>... — print the affected workspace crate set, one name
# per line, sorted; or print the literal ALL if any C4/C5 condition fires; or
# print NOTHING if every path is crate-unmappable-but-known (the non-crate
# classes: docs/**, *.md, *.yaml/yml, gui/src/**, tests/infra/**).
# Always returns 0 so callers are safe under set -e and inside $() capture.
#
# THE EMPTY PRINT IS AN ANSWER, NOT A SHRUG (task 6268). Its meaning is exact:
# every path was classified, none mapped to a crate, and an unmappable path
# would have gone wide via C5 instead. It has exactly ONE producer — the
# `${#direct[@]} -eq 0` early return below, which short-circuits BEFORE
# _reverse_closure, so it never shells out to `cargo metadata` and is
# reproducible in a workspace-less fixture. _reverse_closure holds up the other
# half of that single-sourcing: a non-empty seed list that yields no closure is
# C5, never an empty print (see its own header).
#
# A caller that must distinguish this from "affected_crates was never called"
# carries that bit itself: see AFFECTED_CLOSURE_FROM_DIFF in scripts/verify.sh.
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
    # ATTRIBUTION FIRST, then the non-crate classes (see reify_is_inert_path's
    # header for why that order is the contract on both sides of the SPOT).
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

    # No direct crates: print nothing. Load-bearing — "provably zero crates",
    # not "could not tell". See this function's header.
    if [ "${#direct[@]}" -eq 0 ]; then
        return 0
    fi

    # Expand the direct crate set through the reverse-dependency closure, then
    # emit sorted-unique (one crate per line).
    printf '%s\n' "${direct[@]}" | _reverse_closure
    return 0
}
