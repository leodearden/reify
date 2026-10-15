#!/usr/bin/env bash
# scripts/affected-crates-lib.sh — maps a changed-file list to the affected
# workspace-crate set (direct crates ∪ their reverse-dependency closure).
#
# Contract references:
#   docs/prds/verify-scope-contract.md
#     §3  C3 — Reverse-closure completeness
#         C4 — Global changes force ALL
#         C5 — Fail safe, fail wide
#         File→crate mapping precedence (affected_crates' order over the facts)
#         Reverse-closure definition (the algorithm)
#
# The per-path facts (global, seed crates, unioned crates, inert, tree) and the
# compile closure have ONE home, scripts/reify_path_attribution.py. This lib
# owns only affected_crates' precedence over those facts and its closure call.
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
#                              header. A per-crate manifest touch additionally
#                              contributes the crate-DAG gate's host crate.
#                              Always returns 0.
#   direct_crates_per_path <file>...
#                              prints `<file>\t<route>` per file: the direct
#                              route affected_crates' precedence gives that
#                              file alone, before any closure. Consumed by
#                              the B13 corpus parity check in
#                              tests/infra/test_affected_crates_lib.sh.
#   reify_is_inert_path <path> true iff the module reports the path inert
#                              (docs/**, *.md, *.yaml, *.yml); verify.sh's
#                              decide_scope wrapper until #9056 deletes it.
#
# Unprefixed names are the declared interface. A leading underscore means
# private to this file — do not add a consumer outside it without promoting
# the helper here first.
#
# Sourced by:
#   scripts/verify.sh           (Phase 2 narrowing)
#   tests/infra/test_affected_crates_lib.sh  (drift catcher)
#
# Requires scripts/reify_path_attribution.py and scripts/occt-scope-lib.sh next
# to this file; sourcing fails without either, naming it.

# Source guard — prevent double-sourcing.
if [ "${_REIFY_AFFECTED_CRATES_LIB_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_AFFECTED_CRATES_LIB_SOURCED=1

_AFFECTED_CRATES_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
_AFFECTED_CRATES_ATTRIBUTION="$_AFFECTED_CRATES_LIB_DIR/reify_path_attribution.py"
[ -f "$_AFFECTED_CRATES_ATTRIBUTION" ] || { echo "affected-crates-lib.sh: ERROR — scripts/reify_path_attribution.py not found next to affected-crates-lib.sh" >&2; return 1; }

# The compile closure's entry point (_reify_compile_closure), which
# _reverse_closure below calls. occt-scope-lib.sh's own source guard makes
# this a no-op if verify.sh (or another caller) already sourced it first.
[ -f "$_AFFECTED_CRATES_LIB_DIR/occt-scope-lib.sh" ] || { echo "affected-crates-lib.sh: ERROR — scripts/occt-scope-lib.sh not found next to affected-crates-lib.sh" >&2; return 1; }
# shellcheck source=scripts/occt-scope-lib.sh
source "$_AFFECTED_CRATES_LIB_DIR/occt-scope-lib.sh" || return 1

# _load_attribution_records <array-name> <path>... — fill <array-name> with
# the module's `attribute` TSV record for each path, in argv order, from ONE
# process. Returns non-zero if the CLI fails or the record count is not one
# per path. stdin is always the piped path list, never the caller's: verify.sh
# calls reify_is_inert_path inside a `while read` loop on its own stdin, which
# an inheriting child would drain.
_load_attribution_records() {
    local -n _lar_records="$1"
    shift
    local out
    _lar_records=()
    out="$(printf '%s\n' "$@" | python3 -S "$_AFFECTED_CRATES_ATTRIBUTION" attribute --format tsv)" || return 1
    if [ -n "$out" ]; then
        mapfile -t _lar_records <<<"$out"
    fi
    [ "${#_lar_records[@]}" -eq "$#" ]
}

# _routed_record <record> <route-var> <facts-var> — the one reader of an
# `attribute` TSV record. Fills the associative array <facts-var> with the
# record's fields: global, inert, tree, seeds, unioned and path. The two crate
# sets are sorted, comma-joined, and `-` when empty. It also sets <route-var>
# to affected_crates' route for the record: ALL, SEEDS or NONE. The first
# matching rule wins (contract §3, "File→crate mapping precedence"):
#   global                                  -> ALL   (C4)
#   any seed crate                          -> SEEDS (attribution outranks inert)
#   inert, or tree gui_src / tests_infra    -> NONE  (a known non-crate path)
#   anything else                           -> ALL   (C5: unmappable)
# Fork-free, so callers run it once per record.
_routed_record() {
    local -n _rr_route="$2" _rr_facts="$3"
    local global inert tree seeds unioned path
    IFS=$'\t' read -r global inert tree seeds unioned path <<<"$1"
    _rr_facts=([global]="$global" [inert]="$inert" [tree]="$tree"
               [seeds]="$seeds" [unioned]="$unioned" [path]="$path")
    if [ "$global" = 1 ]; then
        _rr_route=ALL
    elif [ "$seeds" != - ]; then
        _rr_route=SEEDS
    elif [ "$inert" = 1 ] || [ "$tree" = gui_src ] || [ "$tree" = tests_infra ]; then
        _rr_route=NONE
    else
        _rr_route=ALL
    fi
}

# _sorted_union <out-var> <crates> <crates> — the union of two of the module's
# crate sets (sorted, comma-joined, `-` when empty) as one bytewise-sorted,
# space-joined list: direct_crates_per_path's crate column, sorted like the
# corpus rows it is compared with. A fork-free merge, because B13 runs it over
# every tracked path.
_sorted_union() {
    local -n _su_out="$1"
    local LC_ALL=C IFS=' '
    local -a left=() right=() merged=()
    [ "$2" = - ] || IFS=, read -r -a left <<<"$2"
    [ "$3" = - ] || IFS=, read -r -a right <<<"$3"
    local i=0 j=0
    while [ "$i" -lt "${#left[@]}" ] && [ "$j" -lt "${#right[@]}" ]; do
        if [[ "${left[i]}" < "${right[j]}" ]]; then
            merged+=("${left[i]}"); i=$((i + 1))
        elif [[ "${right[j]}" < "${left[i]}" ]]; then
            merged+=("${right[j]}"); j=$((j + 1))
        else
            merged+=("${left[i]}"); i=$((i + 1)); j=$((j + 1))
        fi
    done
    merged+=("${left[@]:i}" "${right[@]:j}")
    _su_out="${merged[*]}"
}

# _reverse_closure — read seed crate names from stdin (one per line), emit the
# cargo-accurate affected workspace-crate set (seeds plus every workspace
# crate whose test-compile-closure pulls in a seed), sorted-unique, one per
# line.
#
# Delegates to _reify_compile_closure (scripts/occt-scope-lib.sh, sourced
# above) — the entry point to compile_closure in
# scripts/reify_path_attribution.py, the one implementation of the
# normal/build vs dev-edge compile-closure model, also used by
# occt_touching_set. This is the reverse ("which crates pull in a seed")
# framing of that same model,
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

# _emit_affected <value>... — affected_crates' single stdout writer, ALL
# sentinel included; prints sorted-unique. Must stay a pipeline ending in an
# external command, so a closed caller pipe (`grep -q` under pipefail) kills
# `sort`, not this shell; `|| true` keeps affected_crates' always-return-0.
_emit_affected() {
    printf '%s\n' "$@" | sort -u || true
}

# direct_crates_per_path <file>... — print `<file>\t<route>` per file, in argv
# order: the route affected_crates' precedence (_routed_record) gives that
# file on its own — ALL, empty, or the file's seed crates ∪ unioned crates
# (bytewise-sorted, space-joined) — before any reverse closure. One module
# call for the whole list. Returns non-zero, printing nothing, if the module
# cannot classify the list.
#
# Its consumer is the B13 parity check in tests/infra/test_affected_crates_lib.sh,
# which compares it with the pinned corpus tests/infra/affected-crates-direct-sets.tsv.
# It is public so that check reads the precedence through the lib's interface
# and never through _routed_record.
direct_crates_per_path() {
    [ "$#" -gt 0 ] || return 0
    local -a records
    _load_attribution_records records "$@" || {
        echo "affected-crates-lib.sh: reify_path_attribution.py attribute failed or returned a record count other than $#" >&2
        return 1
    }
    local -A facts=()
    local rec route crates
    for rec in "${records[@]}"; do
        _routed_record "$rec" route facts
        case "$route" in
            ALL)   crates=ALL ;;
            NONE)  crates="" ;;
            SEEDS) _sorted_union crates "${facts[seeds]}" "${facts[unioned]}" ;;
        esac
        printf '%s\t%s\n' "${facts[path]}" "$crates"
    done
}

# affected_crates <file>... — print the affected workspace crate set, one name
# per line, sorted; or print the literal ALL if any C4/C5 condition fires; or
# print NOTHING if every path is crate-unmappable-but-known (the non-crate
# classes: inert docs/**, *.md, *.yaml/yml, gui/src/**, tests/infra/**).
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
# ONE module call classifies the whole list (PRD D10: never one per path); a
# failed call, or one that does not return a record per path, cannot vouch for
# any narrowing, so it is C5 too.
#
# A caller that must distinguish this from "affected_crates was never called"
# carries that bit itself: see AFFECTED_CLOSURE_FROM_DIFF in scripts/verify.sh.
affected_crates() {
    [ "$#" -gt 0 ] || return 0

    local -a records
    if ! _load_attribution_records records "$@"; then
        echo "affected-crates-lib.sh: reify_path_attribution.py attribute failed or returned a record count other than $# — falling back to ALL" >&2
        _emit_affected ALL
        return 0
    fi

    # Accumulate the direct crate set and the unioned crates. Any ALL route
    # (C4 global, C5 unmappable) settles the answer.
    local -a direct=() unioned_crates=() crates
    local -A facts=()
    local rec route
    for rec in "${records[@]}"; do
        _routed_record "$rec" route facts
        case "$route" in
            ALL)
                _emit_affected ALL
                return 0
                ;;
            SEEDS)
                IFS=, read -r -a crates <<<"${facts[seeds]}"
                direct+=("${crates[@]}")
                ;;
        esac
        # Independent of the route: a manifest is also attributed to its own
        # crate, like any other file that crate owns.
        if [ "${facts[unioned]}" != - ]; then
            IFS=, read -r -a crates <<<"${facts[unioned]}"
            unioned_crates+=("${crates[@]}")
        fi
    done

    # No direct crates: print nothing. Load-bearing — "provably zero crates",
    # not "could not tell". See this function's header.
    if [ "${#direct[@]}" -eq 0 ]; then
        return 0
    fi

    # Expand the direct crate set through the reverse-dependency closure, then
    # emit sorted-unique (one crate per line).
    local closure
    closure="$(printf '%s\n' "${direct[@]}" | _reverse_closure)"

    # ALL is a sentinel, not a crate name, so a fail-wide closure is emitted
    # unchanged and never reaches the union below. The empty test is a
    # DELIBERATE second enforcement of the header's one-empty-print-producer
    # invariant, unreachable while _reverse_closure keeps its own C5 rule (see
    # its header). Not dead code: it stops a regression there from reaching
    # verify.sh as "provably zero crates".
    if [ -z "$closure" ] || [ "$closure" = "ALL" ]; then
        _emit_affected ALL
        return 0
    fi

    # A manifest touch additionally contributes the crate-DAG gate's host
    # crate, unioned into the RESULT of the closure rather than seeded into it.
    # Rule and rationale: docs/prds/verify-scope-contract.md §3, "Per-crate
    # manifest touches also contribute the crate-DAG gate".
    _emit_affected "$closure" "${unioned_crates[@]}"
    return 0
}

# reify_is_inert_path <path> — true iff scripts/reify_path_attribution.py
# reports <path> inert: documentation or configuration (docs/**, *.md, *.yaml,
# *.yml). Contract: §3 "Inert paths are ONE list" in
# docs/prds/verify-scope-contract.md.
#
# A thin wrapper for scripts/verify.sh's decide_scope. On a module failure it
# answers "not inert", the fail-wide direction for decide_scope.
#
# KNOWN COST: one module process per call. decide_scope calls this once for
# each path that reaches its catch-all, so it pays one interpreter start per
# path. That was measured at about 84 ms a call on an idle host and 104-155 ms
# at load average 80, against about 0 for the bash `case` this replaced. One
# `attribute` call over 2,000 paths takes about 0.1 s. #9056 batches
# decide_scope's catch-all into a single call and deletes this wrapper.
# Neither change happens here, because scripts/verify.sh is outside this
# lib's remit.
reify_is_inert_path() {
    local -a records
    local -A facts=()
    local route
    _load_attribution_records records "$1" || return 1
    _routed_record "${records[0]}" route facts
    [ "${facts[inert]}" = 1 ]
}
