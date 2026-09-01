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
#   harness_layout_baseline_rows [baseline]
#                                          print the DATA ROWS of [baseline]
#                                          (default: harness_layout_baseline_path)
#                                          one per line — every line that is
#                                          neither a comment nor blank. THE
#                                          single definition of "a data row of
#                                          the baseline"; a missing baseline
#                                          prints nothing and returns 0 (each
#                                          caller decides what that means); an
#                                          UNREADABLE baseline returns grep's
#                                          error status (>= 2), never a vacuous
#                                          "zero rows".
#   harness_layout_baseline_contains <p> [baseline]
#                                          exit 0 iff <p> is a data row of
#                                          [baseline] (default:
#                                          harness_layout_baseline_path), i.e. a
#                                          non-comment, non-blank line. Same
#                                          comment/blank stripping as
#                                          run-all-classification-lib.sh; exact
#                                          full-line match. O(1) after the first
#                                          call for a given baseline (memoized —
#                                          see the MEMO block below).
#   harness_layout_baseline_cache_reset [baseline]
#                                          drop the memoized rows for [baseline]
#                                          (or for every baseline when called
#                                          with no argument), so the next
#                                          rows/contains call re-reads from
#                                          disk. Only needed by a caller that
#                                          REWRITES a baseline in place at a
#                                          path it has already queried.
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
#   harness_layout_declared_members <root-harness-rs>
#                                          print, one per line, the
#                                          `_harness_layout_norm_path`-normalized
#                                          path of every file in the
#                                          TRANSITIVE `mod`-graph closure
#                                          reachable from <root-harness-rs>
#                                          (the same walk harness_layout_unit_lines
#                                          runs, via the shared private
#                                          `_harness_layout_walk_unit`) that
#                                          lands under <root-harness-rs>'s own
#                                          module dir (${root%.rs}). "member"
#                                          means "a file under the root's own
#                                          harness_<subsystem>/ module
#                                          directory". Consumed by rule (d) in
#                                          test_harness_kloc_cap.sh to detect a
#                                          module-dir file with no reachable
#                                          `mod` declaration.
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

# _HL_CRATES — THE single definition (G7) of the 5 crates whose top-level
# tests/*.rs are subject to the C1 layout contract. reify-solver-elastic /
# reify-eval-fea-tests are deliberately NOT here (they host only override +
# permanently-standalone binaries — out of the consolidation contract's
# scope). harness_layout_consolidatable_crates (below) prints it verbatim;
# the MEMO block further down builds _HL_CRATE_SET from it once at source
# time.
declare -ga _HL_CRATES=(reify-cli reify-syntax reify-kernel-occt reify-eval reify-compiler)

# _HL_OVERRIDE_STEMS — THE single definition (G7) of the 7 standalone
# integration binaries that are NEVER consolidated (invariant I1), identified
# by file stem (basename without the .rs extension).
# harness_layout_override_stems (below) prints it verbatim; the MEMO block
# further down builds _HL_OVERRIDE_STEM_SET from it once at source time.
#
# NOT A SANCTIONED CAP-OVERFLOW DESTINATION (decision, task #6461). None of
# these 7 stems is a place to park a module whose thematic home is a capped
# `harness_<subsystem>.rs` (test_harness_kloc_cap.sh rule (a)) in order to
# relieve that harness's cap pressure.
#
# SCOPE — the destination, not growth. Each stem still grows freely with its
# OWN thematic content (largest today: analytical_validation); what is
# refused is importing a module FOREIGN to a stem's own focus for cap relief.
#
# WHY:
#   - rule (a) already names its remedy — SPLIT into a second
#     `harness_<subsystem2>.rs`, "never accommodated by raising the cap" —
#     and spilling into an override stem is that same accommodation reached
#     by a different route.
#   - these stems carry NO line limit: rule (a)'s CAP_LINES applies only to
#     `harness_*.rs` roots, and an override stem is excluded from the
#     re-accretion predicate — harness_layout_in_scope_standalone below
#     returns 1 on it, and rule (b) in test_harness_kloc_cap.sh's
#     harness_layout_violations `continue`s on it. 5 of the 7 stems aren't
#     even in one of the 5 _HL_CRATES above, so they never reach rule (b) at
#     all. No rule measures lines parked here, anywhere — that is what makes
#     the spill an evasion of the C2 accounting, not a neutral placement
#     choice.
#   - it contradicts the exemption's own rationale: these 7 are permanently
#     standalone for "distinct harness/#[should_panic]/single-focus
#     semantics that a shared harness would break" (invariant I1,
#     test_harness_kloc_cap.sh). A module admitted for cap relief is by
#     construction thematically foreign, which destroys that single-focus
#     property and defeats a reader filtering `<stem>::`.
#   - precedent: harness_topology_selector came in 7.4% over cap and was
#     resolved by SPLITTING out harness_selective_demand, not by parking the
#     overflow in an override stem (test_harness_kloc_cap.sh Section 5b
#     HEADROOM note).
#
# THE STANDING REMEDY: split the harness (#6121) or recover headroom by
# hoisting duplicated fixtures (#6152). A future exception requires a
# conscious amendment to THIS clause, at the single source of truth —
# mirroring the posture rule (b)'s grandfather ratchet already takes
# ("requires a conscious baseline edit").
#
# NO GUARD. "Thematically foreign to this stem's focus" is not mechanically
# decidable, and the nearest proxy — forbidding inline `mod x { ... }` blocks
# in these 7 — is both over-inclusive (a normal Rust idiom for grouping a
# binary's own tests) and trivially evadable (drop the wrapper, paste the
# fns at top level); `_harness_layout_mod_decls` below deliberately ignores
# inline `mod` blocks anyway (they declare no out-of-line file).
declare -ga _HL_OVERRIDE_STEMS=(
    determinism analytical_validation modal_benchmarks
    buckling_smoke fea_diagnostics_e2e
    tensegrity_t0a representation_within_assertion
)

# harness_layout_consolidatable_crates — see _HL_CRATES above.
harness_layout_consolidatable_crates() {
    printf '%s\n' "${_HL_CRATES[@]}"
}

# harness_layout_override_stems — see _HL_OVERRIDE_STEMS above.
harness_layout_override_stems() {
    printf '%s\n' "${_HL_OVERRIDE_STEMS[@]}"
}

# harness_layout_baseline_path — the grandfather-baseline manifest path. Honors
# REIFY_HARNESS_LAYOUT_BASELINE (testability / operator override); defaults to
# harness-layout-baseline.manifest next to this lib.
harness_layout_baseline_path() {
    printf '%s\n' "${REIFY_HARNESS_LAYOUT_BASELINE:-$_HARNESS_LAYOUT_LIB_DIR/harness-layout-baseline.manifest}"
}

# MEMO for the two static data lists, consumed by
# harness_layout_in_scope_standalone below as O(1) associative-array lookups.
#
# WHY: harness_layout_in_scope_standalone is the membership predicate
# test_harness_kloc_cap.sh's whole-tree scan calls once per candidate file —
# ~495 times against the live tree. Before this MEMO, each call forked a
# process substitution to walk the crate list, plus a SECOND for the override
# list on every path that reached the stem check (i.e. every candidate whose
# crate already matched and whose file was not harness_*.rs) — pure fork
# overhead over five and seven static strings. Measured in isolation: ~4s of
# that overhead across 495 calls.
#
# Built directly from _HL_CRATES / _HL_OVERRIDE_STEMS above by a plain `for`
# loop AT SOURCE TIME — no subshell, no fork, no read loop — and
# unconditionally rather than lazily flag-guarded: populating two 5- and
# 7-entry sets costs nothing measurable, so there is no first-call branch to
# maintain. Nothing in this tree mutates either array mid-shell, so no
# cache_reset escape hatch is provided (unlike
# harness_layout_baseline_cache_reset below, which exists because a caller
# CAN rewrite a baseline file in place).
declare -gA _HL_CRATE_SET=()          # crate name         -> 1
declare -gA _HL_OVERRIDE_STEM_SET=()  # override file stem -> 1
for _hl_static_v in "${_HL_CRATES[@]}"; do
    _HL_CRATE_SET["$_hl_static_v"]=1
done
for _hl_static_v in "${_HL_OVERRIDE_STEMS[@]}"; do
    _HL_OVERRIDE_STEM_SET["$_hl_static_v"]=1
done
unset _hl_static_v

# harness_layout_in_scope_standalone <repo-rel-path> — exit 0 iff <repo-rel-path>
# is an in-scope re-accretion candidate: a TOP-LEVEL crates/<crate>/tests/<base>.rs
# where <crate> is one of the 5 consolidatable crates, <base> is NOT a
# harness_*.rs unit, and <base>'s stem is NOT one of the 7 override binaries.
#
# Pure string predicate — NO disk access (the gate layers an on-disk existence
# check on top separately). The explicit component parse (not just the case
# glob) rejects nested / multi-segment forms: a bash `case` glob's `*` matches
# `/`, so `crates/*/tests/*.rs` would otherwise accept crates/c/tests/sub/f.rs.
# The crate/override membership checks below answer from the MEMO above —
# populated once when this file is sourced (an O(1) associative-array
# lookup) — rather than looping the two source functions on every call.
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
    [ -n "${_HL_CRATE_SET["$crate"]:-}" ] || return 1

    # A harness_*.rs compile unit is sanctioned by construction.
    case "$base" in
        harness_*) return 1 ;;
    esac

    # An override binary (by stem) is permanently standalone (I1).
    local _stem="${base%.rs}"
    [ -z "${_HL_OVERRIDE_STEM_SET["$_stem"]:-}" ] || return 1

    return 0
}

# MEMO for the baseline data rows, keyed by baseline PATH. The two accessors
# below answer from these rather than re-reading and re-filtering the file, and
# harness_layout_baseline_contains becomes an O(1) associative-array lookup.
#
# WHY: the callers ask the membership question once per candidate file — ~495
# times per run against the ~495-row live baseline — so the un-memoized shape
# was O(files x rows) plus two grep forks on every single call. Measured on that
# live loop in isolation: ~19-22s wall / ~5.2s CPU un-memoized, ~0.2-0.7s wall /
# ~0.1s CPU memoized. Over the whole kLOC guard, A/B'd back-to-back on one box:
# 18.8s/19.5s wall and 11.3s/12.1s CPU before, 11.2s/12.9s wall and 6.3s/6.6s
# CPU after — in a guard whose parent PRD (docs/prds/merge-gate-compile-cost.md)
# exists to CUT merge-gate cost.
#
# CACHE CONTRACT: keyed by PATH ONLY, and lives for the shell's lifetime. A
# caller that REWRITES a baseline in place at a path it has already queried
# therefore sees the stale rows until it calls
# harness_layout_baseline_cache_reset. That is deliberate — any content-keyed
# alternative needs a stat/read fork per call, which is precisely the cost being
# removed — and safe for every caller today: each fixture baseline gets its own
# fresh `mktemp` path and is written once before use, and the two gates each run
# one baseline per process. test_harness_kloc_cap.sh Section 8 pins both halves
# of that contract (memo is real; reset re-reads).
declare -gA _HL_ROWS_LOADED=()   # baseline path         -> 1 once parsed
declare -gA _HL_ROWS_DATA=()     # baseline path         -> rows, newline-joined
declare -gA _HL_ROWS_MEMBER=()   # "<baseline>\x1f<row>" -> 1

# _harness_layout_baseline_load <baseline-file> — populate the memo for
# <baseline-file> unless it is already loaded. Returns grep's status on a read
# error (>= 2) WITHOUT marking the baseline loaded, so a failure is never cached
# as "this baseline has no rows".
#
# The comment/blank stripping is ONE `grep -v` with two anchored alternatives
# rather than two piped `grep -v`s. Identical semantics — a line is dropped iff
# it matches either branch; verified over the live manifest and over an
# indented-comment / whitespace-only / trailing-space fixture — but it halves
# the forks and, more importantly, yields a SINGLE exit status, so grep's error
# exit 2 can be told apart from its no-lines-matched exit 1 without reaching for
# PIPESTATUS (which the `$(…)` assignment would have hidden anyway).
_harness_layout_baseline_load() {
    local baseline="$1"
    [ -z "${_HL_ROWS_LOADED["$baseline"]:-}" ] || return 0

    local raw rc=0 row
    raw="$(grep -vE '^[[:space:]]*#|^[[:space:]]*$' -- "$baseline")" || rc=$?
    [ "$rc" -le 1 ] || return "$rc"

    _HL_ROWS_DATA["$baseline"]="$raw"
    if [ -n "$raw" ]; then
        while IFS= read -r row; do
            [ -n "$row" ] || continue
            _HL_ROWS_MEMBER["$baseline"$'\x1f'"$row"]=1
        done <<< "$raw"
    fi
    _HL_ROWS_LOADED["$baseline"]=1
    return 0
}

# harness_layout_baseline_rows [baseline-file] — print the DATA ROWS of
# [baseline-file] (default: harness_layout_baseline_path), one per line: every
# line that is neither a comment (`#`, optionally indented) nor blank. Same
# comment/blank stripping style as run-all-classification-lib.sh.
#
# THE SINGLE DEFINITION of "a data row of the baseline" (G7), spelled out in
# _harness_layout_baseline_load above and consumed by every reader through this
# accessor or the membership predicate below. The rule used to be written out
# three times — inside harness_layout_baseline_contains, inline in
# test_harness_kloc_cap.sh's Section 5 guard-integrity check, and (would have
# been) inside its orphan-row detector. A divergence between any two of those
# would mean the membership predicate and the row enumerator disagree about
# which lines of this file are rows, which is exactly the class of silent drift
# this lib exists to prevent — and the memo makes that structural: both now read
# the SAME parsed result, not merely the same rule.
#
# A MISSING baseline prints nothing and returns 0 — deliberately NOT an error
# here, because the two callers want different things from it:
# harness_layout_baseline_contains keeps its own `[ -f ]` guard and returns 1
# ("not a member"), while the orphan-row detector reports an explicit
# `reason=missing-baseline` FAIL. Deciding for them here would force one of the
# two to unpick the decision.
#
# An UNREADABLE baseline (grep exit >= 2) propagates that status. It must NOT
# collapse into the same answer as "no data rows": a blanket `|| true` used to
# swallow grep's permission-denied exit 2 alongside its no-lines-matched exit 1,
# so the orphan-row detector reported a clean `rows=0` PASS on a baseline it
# could not read at all — exactly the vacuous pass its own missing-baseline
# branch exists to forbid. Exit 1 IS still swallowed (returned as 0 with no
# output): an all-comment / empty baseline is a legitimate state of a ratchet
# that shrinks toward empty, not an error, and a caller running under `set -e`
# must not abort on it.
harness_layout_baseline_rows() {
    local baseline="${1:-$(harness_layout_baseline_path)}"
    [ -f "$baseline" ] || return 0
    _harness_layout_baseline_load "$baseline" || return $?
    local data="${_HL_ROWS_DATA["$baseline"]:-}"
    # `printf '%s\n' ""` would emit one blank line, i.e. manufacture a phantom
    # row out of an empty baseline. No rows => no output.
    [ -n "$data" ] || return 0
    printf '%s\n' "$data"
}

# harness_layout_baseline_contains <repo-rel-path> [baseline-file] — exit 0 iff
# <repo-rel-path> is a data row of [baseline-file] (default:
# harness_layout_baseline_path), i.e. a non-comment, non-blank line. Exact
# full-line match.
#
# A missing baseline is NOT a member (return 1) — the same "unknown => flag it"
# posture the callers rely on (a non-member added file is a violation). An
# UNREADABLE baseline is likewise not a member, which is fail-CLOSED: every
# candidate is flagged, loudly, rather than silently grandfathered.
#
# Answered from the memo (see above) as a single associative-array lookup — no
# subshell, no fork, no re-read. That also retires the esc-5172-1 SIGPIPE hazard
# this predicate used to carry by construction rather than by comment: there is
# no pipeline left to early-close, so no upstream stage can be killed to 141
# under the callers' `set -o pipefail` and report "not a member" for every
# grandfathered file.
harness_layout_baseline_contains() {
    local path="$1"
    local baseline="${2:-$(harness_layout_baseline_path)}"
    [ -f "$baseline" ] || return 1
    _harness_layout_baseline_load "$baseline" || return 1
    [ -n "${_HL_ROWS_MEMBER["$baseline"$'\x1f'"$path"]:-}" ]
}

# harness_layout_baseline_cache_reset [baseline-file] — drop the memoized rows
# for [baseline-file], or for EVERY baseline when called with no argument, so
# the next rows/contains call re-reads from disk. Only a caller that rewrites a
# baseline in place at an already-queried path needs this (see the MEMO block).
harness_layout_baseline_cache_reset() {
    local baseline="${1:-}"
    if [ -z "$baseline" ]; then
        _HL_ROWS_LOADED=()
        _HL_ROWS_DATA=()
        _HL_ROWS_MEMBER=()
        return 0
    fi
    unset '_HL_ROWS_LOADED["$baseline"]' '_HL_ROWS_DATA["$baseline"]'
    local k
    for k in "${!_HL_ROWS_MEMBER[@]}"; do
        case "$k" in
            "$baseline"$'\x1f'*) unset '_HL_ROWS_MEMBER["$k"]' ;;
        esac
    done
    return 0
}

# _harness_layout_norm_path <path> — set the global `_HL_NORM_OUT` to <path>
# with `.`, `..` and repeated `/` segments collapsed lexically (NO disk access,
# NO symlink resolution — symlinked module files are out of contract, same
# posture as the plain `find -type f` walk below).
#
# Needed because the "is this include inside the module dir?" test below is a
# path-prefix test: a `#[path = "../shared/x.rs"]` written from inside
# `harness_<subsystem>/` produces `…/harness_<subsystem>/../shared/x.rs`, which
# a naive prefix test would misread as INSIDE the module dir and silently drop
# from the measure — precisely the undercount this attribution exists to close.
#
# RETURNS VIA A GLOBAL, NOT stdout, deliberately: it is called one-to-two times
# per module declaration, and `$(…)` would fork a subshell for each of those —
# pure overhead for a function that is already pure bash and touches no disk.
# Measured over the 14 live harness units, dropping that fork took ~12% off the
# transitive walk; batching the awk fork (see `_harness_layout_mod_decls`) took
# the rest (task #5620 review). Callers read `$_HL_NORM_OUT` immediately after
# the call.
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
    _HL_NORM_OUT="$leading$out"
}

# _harness_layout_row_crate <repo-rel-path> — set the global `_HL_ROW_CRATE` to
# the `<c>` of a `crates/<c>/…`-rooted <repo-rel-path>, for the `crate=<c>`
# field of a structured verdict line.
#
# EVERY other shape yields the `-` sentinel — the same "not crate-scoped" value
# the harness guards already emit for a row they cannot attribute to a crate.
# The sentinel is written FIRST, before any branch, so no caller can read the
# global unset under `set -u`.
#
# The guard is the TWO-segment `crates/*/*`, not merely `crates/*`, and that is
# the whole point of routing this through one definition: a bare `crates/<seg>`
# has no crate-plus-remainder shape, and a naive `${p#crates/}` strip would
# report `<seg>` as a phantom crate on a row that names no file within one.
# Likewise a path not rooted at `crates/` yields `-` rather than its own first
# segment. Guards that disagree here emit contradictory `crate=` attributions
# for the same path.
#
# `crates/*/*` alone does NOT establish that shape, which is why the derived
# value is re-checked before it is assigned: bash lets each `*` match the EMPTY
# string, so a doubled-slash row (`crates//foo.rs`) matches the pattern with an
# empty first segment and a bare `${rest%%/*}` would emit `crate=` — an empty
# value in a field this grammar documents as `crate=<c>`. That shape is
# reachable: the kLOC-cap guard's malformed-row detector feeds hand-written
# manifest rows through here, and a typo'd row must degrade to the sentinel
# every other unattributable row already uses, not to a blank.
#
# RETURNS VIA A GLOBAL, NOT stdout, for consistency with
# _harness_layout_norm_path above — explicitly NOT a measured hot-path
# optimization, unlike that helper's fork-avoidance rationale: every caller
# reaches this only AFTER a path has already been classified a violation, so on
# a clean tree it runs zero times. Callers read `$_HL_ROW_CRATE` immediately
# after the call.
_harness_layout_row_crate() {
    local row="$1" rest
    _HL_ROW_CRATE="-"
    case "$row" in
        crates/*/*)
            rest="${row#crates/}"
            case "${rest%%/*}" in
                '') ;;                       # `crates//…`: keep the sentinel
                *) _HL_ROW_CRATE="${rest%%/*}" ;;
            esac
            ;;
    esac
}

# _harness_layout_mod_decls <file>... — print `<file>|<kind>|<lineno>|<value>`
# for every OUT-OF-LINE module declaration in each <file>:
#   <file>|path|<lineno>|<P>       for `#[path = "<P>"] mod <ident>;`
#   <file>|bare|<lineno>|<ident>   for a `mod <ident>;` carrying no `#[path]`
#
# <lineno> is always the line of the `mod` ITEM (not of a preceding attribute),
# which is the line a developer must edit to fix a violation. <file> is echoed
# back verbatim as passed, so a caller can resolve each decl against its own
# file without a second pass.
#
# TAKES MANY FILES IN ONE CALL, and the walk below feeds it a whole BFS wave at
# a time, because an awk fork per file was the dominant cost of the transitive
# measure: over the 32 module files of the largest live harness unit, one fork
# per file costs 0.372s against 0.012s batched (task #5620 review). Non-existent
# arguments are dropped; an empty argument list returns without invoking awk (it
# would otherwise read stdin and hang).
#
# Fields are `|`-separated, so a source path containing a literal `|` is out of
# contract — the same posture as the symlink exclusion below, and no path in
# this tree is affected. A caller must read the VALUE field last (`read -r f k
# ln val`), which gives it the rest of the line, so a `#[path]` value containing
# `|` still survives intact.
#
# THE SINGLE OUT-OF-LINE-`mod` PARSER for both harness-layout guards (G7). The
# C1 `#[path]`-mandate detector in test_harness_kloc_cap.sh consumes it through
# a `bare`-only filter (`_bare_mod_decls`), and the kLOC measure below consumes
# both kinds — so the attribute-binding rule that decides whether a `mod` is
# `#[path]`-covered exists in exactly ONE place. It has to: that rule is what
# makes the C1 mandate and the C2 quantity the mandate exists to bound agree,
# and a fix applied to one of two copies (block comments, `#[cfg_attr]`,
# `#[cfg(…)] mod x;`) would desynchronise them silently.
#
# Attribute binding: intervening blank lines and `//` comments preserve a
# pending `#[path]`; any other line clears it, INCLUDING a one-line
# `#[path = "…"] mod <ident>;` (which consumes its own attribute, so a bare
# `mod` on the next line is correctly reported as bare, not path-covered).
# Only the `mod <ident>;` FORM is considered — an inline `mod x { … }` block
# declares no out-of-line file and so pulls in no separate source file.
_harness_layout_mod_decls() {
    local _f
    local -a _files=()
    for _f in "$@"; do
        [ -f "$_f" ] && _files+=("$_f")
    done
    [ "${#_files[@]}" -gt 0 ] || return 0
    awk '
        # A pending `#[path]` never crosses a file boundary. Keyed on FILENAME
        # rather than on FNR==1 so that an EMPTY file in the batch (which fires
        # no rule at all) cannot let a pending attribute from the file before it
        # bind the first `mod` of the file after it.
        FILENAME != curfile { curfile = FILENAME; pathline = 0 }
        /^[[:space:]]*$/    { next }   # blank: a pending attribute still binds
        /^[[:space:]]*\/\// { next }   # comment: a pending attribute still binds
        /^[[:space:]]*#\[path[[:space:]]*=/ {
            p = $0
            sub(/^[^"]*"/, "", p)
            sub(/".*$/, "", p)
            if ($0 ~ /\][[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[[:space:]]*;/) {
                # attribute and `mod` on one line: it consumes its own attribute
                printf "%s|path|%d|%s\n", FILENAME, FNR, p
                pathline = 0
            } else {
                pathval = p
                pathline = 1
            }
            next
        }
        /^[[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[[:space:]]*;/ {
            if (pathline) {
                printf "%s|path|%d|%s\n", FILENAME, FNR, pathval
            } else {
                ident = $0
                sub(/^[[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+/, "", ident)
                sub(/[[:space:]]*;.*$/, "", ident)
                printf "%s|bare|%d|%s\n", FILENAME, FNR, ident
            }
            pathline = 0
            next
        }
        { pathline = 0 }
    ' "${_files[@]}"
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
# reproduce the esc-5172-1 SIGPIPE-141 hazard (test_harness_kloc_cap.sh's
# Section-1 fixture generator and its orphan-row detector hit the same class;
# harness_layout_baseline_contains above used to, and now sidesteps it by
# construction — its memo left it with no pipeline at all). `-print0` +
# `read -r -d ''` keeps the walk correct for any path a plain glob would mangle.
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
    # block above) — hoisted (task #7042 step 4) into the shared
    # _harness_layout_walk_unit, which harness_layout_declared_members below
    # also drives, so the two can never disagree about which files a harness
    # unit declares. See that function's header for the walk itself (BFS wave
    # rationale, resolution rules, the esc-5172-1 SIGPIPE-safe drain) — only
    # the totals are read back here, into the SAME local names the unchanged
    # printf below already expects.
    _harness_layout_walk_unit "$root"
    external_lines="$_HL_WALK_EXT_LINES"
    external_files="$_HL_WALK_EXT_FILES"

    printf '%s %s %s %s %s %s\n' \
        "$((root_lines + module_lines + external_lines))" \
        "$root_lines" "$module_lines" "$module_files" \
        "$external_lines" "$external_files"
}

# _harness_layout_walk_unit <root-harness-rs> — the TRANSITIVE `mod`-graph BFS
# shared by harness_layout_unit_lines above (EXTERNAL attribution) and
# harness_layout_declared_members below (the declared-member set rule (d) in
# test_harness_kloc_cap.sh consumes) — hoisted out of harness_layout_unit_lines
# (task #7042 step 4) so the rustc resolution rules exist in exactly ONE place
# (G7 no-lockstep-duplication) and the two callers can never disagree about
# which files a harness unit declares.
#
# Sets, and at the top of every call RESETS (a second call must not inherit
# the first's visited set):
#   _HL_WALK_VISITED        assoc array, normalized resolved path -> 1, for
#                            every file reachable from <root>'s transitive
#                            mod-graph, INCLUDING <root> itself.
#   _HL_WALK_EXT_LINES       sum of `wc -l` / count over every visited file
#   _HL_WALK_EXT_FILES       that resolves OUTSIDE <root>'s own module dir
#                            (the EXTERNAL ATTRIBUTION harness_layout_unit_lines'
#                            header above documents).
#   _HL_WALK_MODDIR_PREFIX   the normalized `${root%.rs}/` prefix, so a caller
#                            can classify a visited path as in-module-dir vs
#                            external without re-deriving it.
#
# Below is the walk exactly as it read inline in harness_layout_unit_lines
# before this hoist (see that function for the surrounding <total>/<root_lines>
# contract its own two return values feed).
#
# Transitive `mod`-graph walk for the EXTERNAL attribution (see
# harness_layout_unit_lines' header block above). BFS a WAVE at a time — every
# file discovered at depth N is parsed by ONE `_harness_layout_mod_decls` call
# — because an awk fork per file dominated this walk's cost (see that
# function's header). Each decl carries its own source file back, so batching
# costs no fidelity: a decl is still resolved relative to the file that
# declared it, not to the wave.
#
# KNOWN LIMITATION (task #7042 amendment; documented, not fixed — no live
# harness root uses the shape below today, so this is a latent gap, not a
# current miscount, and closing it would touch `_harness_layout_mod_decls`,
# which Section 6 and rule (a) in test_harness_kloc_cap.sh also depend on —
# out of this amendment's scope). Resolution below is purely FILE-based
# (dirname of the declaring file), with no awareness of inline `mod outer {
# ... }` block nesting WITHIN that file. A decl that textually appears INSIDE
# such a block — e.g. `mod outer { #[path = "harness_s/inner.rs"] mod inner;
# }` in harness_s.rs — is parsed by `_harness_layout_mod_decls` and resolved
# here exactly as if it were top-level (against dirname(harness_s.rs)), never
# against the dirname(harness_s.rs)/outer/ rustc would actually require. For
# `harness_layout_declared_members` (rule (d) in test_harness_kloc_cap.sh)
# this is a FAIL-OPEN: such a member can read as "declared" — and so pass
# rule (d) — while rustc never compiles it, which is the exact
# silent-coverage-loss class rule (d) exists to catch, just not for this one
# shape.
_harness_layout_walk_unit() {
    local root="$1"
    local moddir="${root%.rs}"
    local n
    local -a _wave=() _next=()
    local _cur _dir _base _kind _ln _val _cand _target

    declare -gA _HL_WALK_VISITED=()
    _HL_WALK_EXT_LINES=0
    _HL_WALK_EXT_FILES=0
    _harness_layout_norm_path "$moddir"; _HL_WALK_MODDIR_PREFIX="$_HL_NORM_OUT/"

    if [ -f "$root" ]; then
        _harness_layout_norm_path "$root"
        _HL_WALK_VISITED["$_HL_NORM_OUT"]=1
        _wave=("$root")
    fi

    while [ "${#_wave[@]}" -gt 0 ]; do
        _next=()
        # Process substitution (not a pipe) feeding a loop that DRAINS to
        # completion — never an early-closing consumer, which under the
        # callers' `set -o pipefail` would reproduce the esc-5172-1 SIGPIPE-141
        # hazard (test_harness_kloc_cap.sh's orphan-row detector, which consumes
        # harness_layout_baseline_rows above, hits the same class).
        # `_ln` (the decl's line number) is unused by the measure — it exists
        # for the C1 mandate detector, which reports it to the developer.
        # Reading the VALUE last is required, not stylistic: `read` gives the
        # trailing field the rest of the line, so a `#[path]` value containing
        # a literal `|` lands whole in `_val` instead of being split across it.
        while IFS='|' read -r _cur _kind _ln _val; do
            [ -n "$_kind" ] || continue
            case "$_cur" in
                */*) _dir="${_cur%/*}" ;;
                *)   _dir="." ;;
            esac
            _target=""
            case "$_kind" in
                path)
                    _harness_layout_norm_path "$_dir/$_val"
                    _cand="$_HL_NORM_OUT"
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
                        _harness_layout_norm_path "$_cand"
                        _cand="$_HL_NORM_OUT"
                        if [ -f "$_cand" ]; then _target="$_cand"; break; fi
                    done
                    ;;
            esac
            # Unresolvable target: contributes 0, and never aborts a `set -e`
            # caller (it would not compile; Section 6 flags that class).
            [ -n "$_target" ] || continue
            [ -z "${_HL_WALK_VISITED[$_target]:-}" ] || continue
            _HL_WALK_VISITED["$_target"]=1
            _next+=("$_target")
            # Under the module dir => already counted by the find walk above.
            # Still queued, so its OWN escaping includes are attributed.
            case "$_target" in
                "$_HL_WALK_MODDIR_PREFIX"*) continue ;;
            esac
            n="$(wc -l < "$_target")"
            n="${n//[[:space:]]/}"   # portable: strip any wc padding
            _HL_WALK_EXT_LINES=$((_HL_WALK_EXT_LINES + n))
            _HL_WALK_EXT_FILES=$((_HL_WALK_EXT_FILES + 1))
        done < <(_harness_layout_mod_decls "${_wave[@]}")
        _wave=("${_next[@]}")
    done
}

# harness_layout_declared_members <root-harness-rs> — print, one per line, the
# `_harness_layout_norm_path`-normalized path of every file in the TRANSITIVE
# `mod`-graph closure reachable from <root-harness-rs> that lands under its
# own module dir (${root%.rs}). See the header block above for the full
# contract.
#
# Driven by the shared _harness_layout_walk_unit BFS immediately above (task
# #7042 step 4 hoisted it out of harness_layout_unit_lines, this function's
# other consumer) — see that function's header for the resolution rules
# (rustc's `#[path]` / bare-`mod` semantics) and the SIGPIPE-safe walk itself.
# One walk, two consumers: harness_layout_unit_lines and this function can
# never disagree about which files a harness unit declares — in particular,
# this correctly sees a member declared TRANSITIVELY, e.g. from a nested
# `mod.rs` inside the module dir rather than from <root-harness-rs> itself.
#
# "member" means "a file under the root's own harness_<subsystem>/ module
# directory" — a bare `mod common;` resolving to a retained `tests/` sibling
# (Section 6's principled exception in test_harness_kloc_cap.sh) is correctly
# NOT a member, since it resolves OUTSIDE the module dir: the walk still
# visits it (for harness_layout_unit_lines' external attribution) but this
# function filters it out via `_HL_WALK_MODDIR_PREFIX`.
harness_layout_declared_members() {
    local root="$1"
    local _p

    _harness_layout_walk_unit "$root"
    for _p in "${!_HL_WALK_VISITED[@]}"; do
        case "$_p" in
            "$_HL_WALK_MODDIR_PREFIX"*) printf '%s\n' "$_p" ;;
        esac
    done
}
