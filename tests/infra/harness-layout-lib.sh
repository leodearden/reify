#!/usr/bin/env bash
# tests/infra/harness-layout-lib.sh — shared harness-layout contract data + predicates.
#
# SINGLE SOURCE OF TRUTH for the data that must agree between the two
# harness-layout guards (task #5300):
#   - tests/infra/test_harness_kloc_cap.sh   (task 5265) — the WHOLE-TREE
#     anti-re-accretion kLOC-cap live scan;
#   - scripts/check-harness-baseline-registration.sh (task 5300) — the
#     DIFF-SCOPED baseline-registration drift gate.
# Both source this lib, so the 5 consolidatable crates, the 7 override stems,
# and the baseline-membership semantics cannot silently diverge between the two
# (the G7 no-lockstep-duplication concern). Directly mirrors the
# run-all-classification-lib.sh shared-derivation pattern that task 5252's
# check-infra-classification-manifest.sh uses.
#
# Designed to be sourced, not executed directly:
#   source "$(dirname "${BASH_SOURCE[0]}")/harness-layout-lib.sh"
#
# Provides:
#   harness_layout_consolidatable_crates   the 5 crates subject to the C1
#                                          layout contract, one per line.
#   harness_layout_override_stems          the 7 permanently-standalone override
#                                          binaries (invariant I1), by file stem
#                                          (basename without .rs), one per line.
#   harness_layout_baseline_path           the grandfather-baseline manifest
#                                          path (honors REIFY_HARNESS_LAYOUT_BASELINE;
#                                          defaults to harness-layout-baseline.manifest
#                                          next to this lib).
#   harness_layout_in_scope_standalone <p> exit 0 iff <p> is an in-scope
#                                          re-accretion candidate: a TOP-LEVEL
#                                          crates/<one-of-5>/tests/<base>.rs with
#                                          <base> NOT a harness_*.rs unit and its
#                                          stem NOT one of the 7 overrides. Pure
#                                          string predicate (no disk access).
#   harness_layout_baseline_contains <p> [baseline]
#                                          exit 0 iff <p> is a non-comment,
#                                          non-blank line of [baseline] (default:
#                                          harness_layout_baseline_path). Same
#                                          comment/blank stripping as
#                                          run-all-classification-lib.sh; exact
#                                          full-line fixed-string match.
#
# Environment:
#   REIFY_HARNESS_LAYOUT_BASELINE  Override the baseline manifest path. Defaults
#                                  to harness-layout-baseline.manifest next to
#                                  this library.

# Source guard — prevent double-sourcing.
if [ "${_REIFY_HARNESS_LAYOUT_LIB_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_HARNESS_LAYOUT_LIB_SOURCED=1

_HARNESS_LAYOUT_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# harness_layout_consolidatable_crates — the 5 crates whose top-level tests/*.rs
# are subject to the C1 layout contract. reify-solver-elastic / reify-eval-fea-tests
# are deliberately NOT here (they host only override + permanently-standalone
# binaries — out of the consolidation contract's scope).
harness_layout_consolidatable_crates() {
    printf '%s\n' reify-cli reify-syntax reify-kernel-occt reify-eval reify-compiler
}

# harness_layout_override_stems — the 7 standalone integration binaries that are
# NEVER consolidated (invariant I1), identified by file stem (basename without
# the .rs extension).
harness_layout_override_stems() {
    printf '%s\n' \
        determinism analytical_validation modal_benchmarks \
        buckling_smoke fea_diagnostics_e2e \
        tensegrity_t0a representation_within_assertion
}

# harness_layout_baseline_path — the grandfather-baseline manifest path. Honors
# REIFY_HARNESS_LAYOUT_BASELINE (testability / operator override); defaults to
# harness-layout-baseline.manifest next to this lib.
harness_layout_baseline_path() {
    printf '%s\n' "${REIFY_HARNESS_LAYOUT_BASELINE:-$_HARNESS_LAYOUT_LIB_DIR/harness-layout-baseline.manifest}"
}

# harness_layout_in_scope_standalone <repo-rel-path> — exit 0 iff <repo-rel-path>
# is an in-scope re-accretion candidate: a TOP-LEVEL crates/<crate>/tests/<base>.rs
# where <crate> is one of the 5 consolidatable crates, <base> is NOT a
# harness_*.rs unit, and <base>'s stem is NOT one of the 7 override binaries.
#
# Pure string predicate — NO disk access (the gate layers an on-disk existence
# check on top separately). The explicit component parse (not just the case
# glob) rejects nested / multi-segment forms: a bash `case` glob's `*` matches
# `/`, so `crates/*/tests/*.rs` would otherwise accept crates/c/tests/sub/f.rs.
harness_layout_in_scope_standalone() {
    local path="$1"
    case "$path" in
        crates/*/tests/*.rs) ;;
        *) return 1 ;;
    esac
    local rest="${path#crates/}"     # <crate>/tests/<base>.rs  (or deeper)
    local crate="${rest%%/*}"        # <crate>
    local tail="${rest#*/}"          # tests/<base>.rs          (or deeper)
    # Exactly tests/<base>.rs: reject nesting below tests/, and a <crate>
    # segment that itself contained a slash (which lands here as a non-match).
    case "$tail" in
        tests/*/*) return 1 ;;       # nested below tests/
        tests/*.rs) ;;
        *) return 1 ;;               # not directly under tests/ (e.g. src/…)
    esac
    local base="${tail#tests/}"      # <base>.rs

    # <crate> must be one of the 5 consolidatable crates.
    local _c _crate_ok=0
    while IFS= read -r _c; do
        if [ "$_c" = "$crate" ]; then _crate_ok=1; break; fi
    done < <(harness_layout_consolidatable_crates)
    [ "$_crate_ok" -eq 1 ] || return 1

    # A harness_*.rs compile unit is sanctioned by construction.
    case "$base" in
        harness_*) return 1 ;;
    esac

    # An override binary (by stem) is permanently standalone (I1).
    local _ov _stem="${base%.rs}"
    while IFS= read -r _ov; do
        if [ "$_ov" = "$_stem" ]; then return 1; fi
    done < <(harness_layout_override_stems)

    return 0
}

# harness_layout_baseline_contains <repo-rel-path> [baseline-file] — exit 0 iff
# <repo-rel-path> is a non-comment, non-blank line of [baseline-file] (default:
# harness_layout_baseline_path). Same comment/blank stripping style as
# run-all-classification-lib.sh; exact full-line fixed-string match.
#
# A missing baseline is NOT a member (return 1) — the same "unknown => flag it"
# posture the callers rely on (a non-member added file is a violation).
harness_layout_baseline_contains() {
    local path="$1"
    local baseline="${2:-$(harness_layout_baseline_path)}"
    [ -f "$baseline" ] || return 1
    # Exact full-line match against non-comment/non-blank lines. NOTE: no
    # `grep -q` on the final stage — under `set -o pipefail` (which the callers
    # set) a `-q` early-close SIGPIPEs the upstream `grep -v` (exit 141), making
    # the pipeline report FAILURE despite a match and flagging every
    # grandfathered file (the esc-5172-1 SIGPIPE hazard). Reading the whole
    # stream to /dev/null preserves grep's own 0/1 match exit with no early close.
    grep -vE '^[[:space:]]*#' "$baseline" \
        | grep -vE '^[[:space:]]*$' \
        | grep -xF -- "$path" >/dev/null
}
