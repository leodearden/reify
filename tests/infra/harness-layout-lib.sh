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
#                                          <module_lines> <module_files>
#                                          <external_lines> <external_files>"
#                                          for the COMPILE UNIT rooted at
#                                          <root-harness-rs>: <root_lines> is
#                                          the root file's own `wc -l` (0 if
#                                          absent); <module_lines>/<module_files>
#                                          sum `wc -l`/count over the files in
#                                          its own harness_<subsystem>/ module
#                                          directory (0/0 if that dir does not
#                                          exist — the single-file-harness
#                                          case); <external_lines>/<external_files>
#                                          sum `wc -l`/count over the files the
#                                          unit includes from OUTSIDE that
#                                          module directory (the shared
#                                          tests/common/ helpers); <total> =
#                                          <root_lines> + <module_lines> +
#                                          <external_lines>. See the function's
#                                          own header for the resolution rules.
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

# _harness_layout_norm_path <path> — print <path> with `.`, `..` and repeated
# `/` segments collapsed lexically (NO disk access, NO symlink resolution —
# symlinked module files are out of contract, same posture as the plain
# `find -type f` walk below).
#
# Needed because the "is this include inside the module dir?" test below is a
# path-prefix test: a `#[path = "../shared/x.rs"]` written from inside
# `harness_<subsystem>/` produces `…/harness_<subsystem>/../shared/x.rs`, which
# a naive prefix test would misread as INSIDE the module dir and silently drop
# from the measure — precisely the undercount this attribution exists to close.
_harness_layout_norm_path() {
    local p="$1" leading="" out="" seg
    case "$p" in /*) leading="/"; p="${p#/}" ;; esac
    while [ -n "$p" ]; do
        seg="${p%%/*}"
        if [ "$seg" = "$p" ]; then p=""; else p="${p#*/}"; fi
        case "$seg" in
            ''|'.') ;;                       # empty (`//`) or `.`: no-op
            '..')
                case "$out" in
                    ''|'..'|*/'..')
                        # Nothing poppable. Keep the `..` only on a RELATIVE
                        # path; an absolute path's root has no parent.
                        [ -n "$leading" ] || out="${out:+$out/}.."
                        ;;
                    */*) out="${out%/*}" ;;
                    *)   out="" ;;
                esac
                ;;
            *) out="${out:+$out/}$seg" ;;
        esac
    done
    printf '%s\n' "$leading$out"
}

# _harness_layout_mod_decls <file> — print `<kind>|<value>` for every OUT-OF-LINE
# module declaration in <file>:
#   path|<P>       for `#[path = "<P>"] mod <ident>;`
#   bare|<ident>   for a `mod <ident>;` carrying no `#[path]`
#
# Attribute binding matches _bare_mod_decls in test_harness_kloc_cap.sh:
# intervening blank lines and `//` comments preserve a pending `#[path]`; any
# other line clears it. The attribute and its `mod` on the SAME line are handled
# too. Only the `mod <ident>;` FORM is considered — an inline `mod x { … }`
# block declares no out-of-line file and so pulls in no separate source file.
_harness_layout_mod_decls() {
    [ -f "$1" ] || return 0
    awk '
        /^[[:space:]]*$/    { next }   # blank: a pending attribute still binds
        /^[[:space:]]*\/\// { next }   # comment: a pending attribute still binds
        /^[[:space:]]*#\[path[[:space:]]*=/ {
            p = $0
            sub(/^[^"]*"/, "", p)
            sub(/".*$/, "", p)
            if ($0 ~ /\][[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[[:space:]]*;/) {
                printf "path|%s\n", p       # attribute and `mod` on one line
                pathline = 0
            } else {
                pathval = p
                pathline = 1
            }
            next
        }
        /^[[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[[:space:]]*;/ {
            if (pathline) {
                printf "path|%s\n", pathval
            } else {
                ident = $0
                sub(/^[[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+/, "", ident)
                sub(/[[:space:]]*;.*$/, "", ident)
                printf "bare|%s\n", ident
            }
            pathline = 0
            next
        }
        { pathline = 0 }
    ' "$1"
}

# harness_layout_unit_lines <root-harness-rs> — print "<total> <root_lines>
# <module_lines> <module_files> <external_lines> <external_files>"
# (space-separated) for the compile unit rooted at <root-harness-rs>.
#
# <root_lines>  = `wc -l` of the root file itself (0 if the root is absent).
# module dir    = ${root%.rs}, i.e. the harness_<subsystem>/ directory next
#                 to the root.
# <module_lines>/<module_files> = sum of `wc -l` / count over every *.rs
#                 file at ANY DEPTH under the module dir (0/0 when the dir
#                 does not exist — the single-file-harness case — or when it
#                 has no *.rs entries; see the recursive-walk / .rs-only
#                 rationale below).
# <external_lines>/<external_files> = sum of `wc -l` / count over the files
#                 the unit includes from OUTSIDE the module dir (see the
#                 EXTERNAL ATTRIBUTION block below).
# <total>       = <root_lines> + <module_lines> + <external_lines>.
#
# EXTERNAL ATTRIBUTION. A harness root may pull in a file that does NOT live
# under its own module dir — in this tree, the shared `tests/common/` helpers,
# via `#[path = "common/x.rs"]` or the retained-sibling bare `mod common;`.
# Those lines ARE part of the compile unit and are attributed to it: rustc
# compiles a SEPARATE COPY of such a helper into every including binary, so
# charging the same file to two units is not double-counting — it is two real
# compilations, and per-unit compile cost is exactly what rule (a)'s cap
# governs. (Leaving them unattributed would also leave the C2 anti-re-accretion
# ratchet guarding a quantity it does not define, and be directionally
# exploitable: move code into `tests/common/`, `#[path]`-include it, cap evaded.)
#
# The measure is the TRANSITIVE `mod`-graph closure reachable from the root —
# transitive because `tests/common/mod.rs` itself declares `pub mod
# alloc_counter;` / `pub mod as_printed;`, so a one-hop walk would miss them.
# Targets are resolved the way rustc resolves them:
#
#   `#[path = "P"] mod X;`  in file F  ->  dirname(F)/P
#   bare `mod X;` where F is the crate ROOT or F's basename is `mod.rs`
#                                      ->  dirname(F)/X.rs, else dirname(F)/X/mod.rs
#   bare `mod X;` in any other module file F
#                                      ->  ${F%.rs}/X.rs,   else ${F%.rs}/X/mod.rs
#
# A visited set keyed by the lexically-normalized resolved path counts each
# file AT MOST ONCE, so a helper reached by two declarations is charged once.
# A resolved target UNDER the module dir is TRAVERSED (its own escaping
# includes still count) but is NOT added to external_lines/external_files —
# the `find` walk below already counted it. An unresolvable target contributes
# nothing: it would not compile, and Section 6 of test_harness_kloc_cap.sh is
# what flags that class of declaration.
#
# Per-file `wc -l` is summed rather than `cat`-ing every file through a
# single `wc -l` because per-file counting is what also yields
# <module_files>, and it matches the existing root-file counting call shape
# (`wc -l < "$root"`) exactly — NOT because the two approaches would ever
# disagree on the total: `wc -l` counts newline characters, and
# concatenation neither adds nor removes them, so summing per-file counts
# and counting the single `cat`-ed stream always agree exactly, even when an
# interior file's last line is unterminated (PRD
# docs/prds/merge-gate-compile-cost.md §5 C2 settles raw line count as the
# measure).
#
# Recursive walk over the module dir via `find <moddir> -type f -name
# '*.rs'` (plain `-type f`, no `-L`: symlinked module files are out of
# contract), counting every `.rs` file at any depth — NOT just the module
# dir's direct entries. `.rs`-only because rule (a) caps a COMPILE UNIT (PRD
# docs/prds/merge-gate-compile-cost.md §5 C2 wording) and only `.rs` is
# compiled into it: a colocated fixture (`.ri`/`.json`/golden output, etc.)
# must not be able to push a harness over a hard merge-gate cap. This is a
# live risk, not hypothetical — crates/reify-syntax/tests/ already colocates
# `fixtures/` and `common/` directories with its tests.
#
# `find ... -print0` feeds a `while IFS= read -r -d ''` loop via process
# substitution (not a pipe) that drains the stream to completion — never an
# early-closing consumer, which under the callers' `set -o pipefail` would
# reproduce the esc-5172-1 SIGPIPE-141 hazard (harness_layout_baseline_contains
# above, and test_harness_kloc_cap.sh's Section-1 fixture generator, hit the
# same class of hazard). `-print0` + `read -r -d ''` keeps the walk correct
# for any path a plain glob would mangle.
harness_layout_unit_lines() {
    local root="$1"
    local root_lines=0 module_lines=0 module_files=0 n f
    local external_lines=0 external_files=0
    local moddir="${root%.rs}"

    if [ -f "$root" ]; then
        root_lines="$(wc -l < "$root")"
        root_lines="${root_lines//[[:space:]]/}"   # portable: strip any wc padding
    fi

    # Guard `[ -d "$moddir" ]` before invoking find: a missing module dir is
    # the single-file-harness case (0/0), and `find` on a non-existent path
    # exits non-zero, which would otherwise abort the sourcing script under
    # `set -e`.
    if [ -d "$moddir" ]; then
        while IFS= read -r -d '' f; do
            n="$(wc -l < "$f")"
            n="${n//[[:space:]]/}"   # portable: strip any wc padding
            module_lines=$((module_lines + n))
            module_files=$((module_files + 1))
        done < <(find "$moddir" -type f -name '*.rs' -print0)
    fi

    # Transitive `mod`-graph walk for the EXTERNAL attribution (see the header
    # block above). An INDEX-walked worklist, not a `while read` over a
    # producer, because the walk both consumes and appends within one pass.
    local -A _visited=()
    local -a _queue=()
    local _cur _dir _base _kind _val _cand _target _i=0
    local _moddir_prefix
    _moddir_prefix="$(_harness_layout_norm_path "$moddir")/"

    if [ -f "$root" ]; then
        _visited["$(_harness_layout_norm_path "$root")"]=1
        _queue+=("$root")
    fi

    while [ "$_i" -lt "${#_queue[@]}" ]; do
        _cur="${_queue[$_i]}"
        _i=$((_i + 1))
        case "$_cur" in
            */*) _dir="${_cur%/*}" ;;
            *)   _dir="." ;;
        esac
        # Process substitution (not a pipe) feeding a loop that DRAINS to
        # completion — never an early-closing consumer, which under the
        # callers' `set -o pipefail` would reproduce the esc-5172-1 SIGPIPE-141
        # hazard (harness_layout_baseline_contains above hits the same class).
        while IFS='|' read -r _kind _val; do
            [ -n "$_kind" ] || continue
            _target=""
            case "$_kind" in
                path)
                    _cand="$(_harness_layout_norm_path "$_dir/$_val")"
                    [ -f "$_cand" ] && _target="$_cand"
                    ;;
                bare)
                    # rustc: a bare `mod X;` in the crate ROOT or in a `mod.rs`
                    # resolves against that file's OWN directory; in any other
                    # module file it resolves against the file's stem directory.
                    if [ "$_cur" = "$root" ] || [ "${_cur##*/}" = "mod.rs" ]; then
                        _base="$_dir"
                    else
                        _base="${_cur%.rs}"
                    fi
                    for _cand in "$_base/$_val.rs" "$_base/$_val/mod.rs"; do
                        _cand="$(_harness_layout_norm_path "$_cand")"
                        if [ -f "$_cand" ]; then _target="$_cand"; break; fi
                    done
                    ;;
            esac
            # Unresolvable target: contributes 0, and never aborts a `set -e`
            # caller (it would not compile; Section 6 flags that class).
            [ -n "$_target" ] || continue
            [ -z "${_visited[$_target]:-}" ] || continue
            _visited["$_target"]=1
            _queue+=("$_target")
            # Under the module dir => already counted by the find walk above.
            # Still queued, so its OWN escaping includes are attributed.
            case "$_target" in
                "$_moddir_prefix"*) continue ;;
            esac
            n="$(wc -l < "$_target")"
            n="${n//[[:space:]]/}"   # portable: strip any wc padding
            external_lines=$((external_lines + n))
            external_files=$((external_files + 1))
        done < <(_harness_layout_mod_decls "$_cur")
    done

    printf '%s %s %s %s %s %s\n' \
        "$((root_lines + module_lines + external_lines))" \
        "$root_lines" "$module_lines" "$module_files" \
        "$external_lines" "$external_files"
}
