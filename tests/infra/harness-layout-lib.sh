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
#   harness_layout_unit_lines <root-harness-rs>
#                                          print "<total> <root_lines>
#                                          <module_lines> <module_files>" for
#                                          the COMPILE UNIT rooted at
#                                          <root-harness-rs>: <root_lines> is
#                                          the root file's own `wc -l` (0 if
#                                          absent); <module_lines>/<module_files>
#                                          sum `wc -l`/count over the files in
#                                          its own harness_<subsystem>/ module
#                                          directory (0/0 if that dir does not
#                                          exist — the single-file-harness
#                                          case); <total> = <root_lines> +
#                                          <module_lines>. KNOWN, BOUNDED
#                                          LIMITATION: files pulled in from
#                                          OUTSIDE the module dir (a shared
#                                          tests/common/ helper via
#                                          `#[path = "common/x.rs"]` or a bare
#                                          `mod common;`) are deliberately NOT
#                                          attributed to the unit — measured,
#                                          e.g. attributing
#                                          crates/reify-eval/tests/harness_topology_selector.rs's
#                                          `#[path = "common/differential.rs"]`
#                                          include (2128 lines) would put it at
#                                          21470 lines, 7.4% over CAP_LINES —
#                                          a cap/split call for the PRD owner,
#                                          not a measurement fix.
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

# harness_layout_unit_lines <root-harness-rs> — print "<total> <root_lines>
# <module_lines> <module_files>" (space-separated) for the compile unit
# rooted at <root-harness-rs>.
#
# <root_lines>  = `wc -l` of the root file itself (0 if the root is absent).
# module dir    = ${root%.rs}, i.e. the harness_<subsystem>/ directory next
#                 to the root.
# <module_lines>/<module_files> = sum of `wc -l` / count over the files
#                 directly inside the module dir (0/0 when the dir does not
#                 exist — the single-file-harness case).
# <total>       = <root_lines> + <module_lines>.
#
# Per-file `wc -l` is summed rather than `cat`-ing the files through a single
# `wc -l`: it matches the existing root-file counting semantics exactly and
# avoids `cat` merging one file's unterminated last line into the next file's
# first line, which would silently undercount (PRD
# docs/prds/merge-gate-compile-cost.md §5 C2 settles raw line count as the
# measure).
#
# Flat, non-recursive walk over the module dir's direct entries, filtered to
# regular files only (`[ -f "$f" ] || continue`) — it does not descend into
# nested subdirectories and does not filter by extension.
harness_layout_unit_lines() {
    local root="$1"
    local root_lines=0 module_lines=0 module_files=0 n f
    local moddir="${root%.rs}"

    if [ -f "$root" ]; then
        root_lines="$(wc -l < "$root")"
        root_lines="${root_lines//[[:space:]]/}"   # portable: strip any wc padding
    fi

    if [ -d "$moddir" ]; then
        for f in "$moddir"/*; do
            [ -f "$f" ] || continue
            n="$(wc -l < "$f")"
            n="${n//[[:space:]]/}"   # portable: strip any wc padding
            module_lines=$((module_lines + n))
            module_files=$((module_files + 1))
        done
    fi

    printf '%s %s %s %s\n' \
        "$((root_lines + module_lines))" "$root_lines" "$module_lines" "$module_files"
}
