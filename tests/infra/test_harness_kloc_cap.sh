#!/usr/bin/env bash
# tests/infra/test_harness_kloc_cap.sh
#
# B+H integration-gate — the C1 harness-layout CONTRACT + the C2
# anti-re-accretion kLOC-cap DRIFT GUARD (task #5265, PRD
# docs/prds/merge-gate-compile-cost.md §5 C1/C2, decomposition leaf B1).
#
# This guard MUST land before any within-crate test-harness consolidation
# leaf (C-cli / C-syntax / C-occt / EVAL-1..3 / CMP-1..2); every one of those
# leaves is verified against the contract encoded here, and the run-all suite
# runs this guard on every merge_request (classified `pool`).
#
# ===========================================================================
# C1 — the harness-layout contract (documented here; enforced by the guard).
# ===========================================================================
#
# NAMING. A consolidated test-harness compile unit for subsystem <subsystem>
# in crate <c> lives at:
#
#     crates/<c>/tests/harness_<subsystem>.rs
#
# and each former standalone integration binary `crates/<c>/tests/<file>.rs`
# is MOVED under that harness's module directory:
#
#     crates/<c>/tests/harness_<subsystem>/<file>.rs
#
# declared from the harness root with a MANDATORY `#[path]` attribute — the
# form is always:
#
#     #[path = "harness_<subsystem>/<file>.rs"]
#     mod <file>;
#
# `#[path]` is the NORM here, not a fallback for awkward module idents.
# `crates/<c>/tests/harness_<subsystem>.rs` IS an integration-test CRATE ROOT,
# and Rust resolves a bare `mod <file>;` in a crate root against the crate
# root's OWN directory — i.e. `tests/<file>.rs` or `tests/<file>/mod.rs`, never
# `tests/harness_<subsystem>/<file>.rs`. So without `#[path]` the declaration
# either fails to compile (no such file) or silently binds the WRONG file (a
# still-present top-level `tests/<file>.rs` mid-move). Because that second mode
# is SILENT, the rule is not left to prose: Section 6 below ENFORCES it over
# every `crates/*/tests/harness_<subsystem>.rs` root in the live tree, so the
# census is re-derived on every run rather than hand-counted and left to stale.
# (`scripts/check-harness-baseline-registration.sh` states the same rule.)
#
# THE ONE PRINCIPLED EXCEPTION is a module that genuinely still lives as a
# `tests/` SIBLING because it was deliberately NOT moved — the shared `common`
# helper at `crates/<c>/tests/common/mod.rs`. There a bare `mod common;` is
# correct precisely BECAUSE crate-root-relative resolution lands on it
# (harness_cli, harness_occt, harness_fea_solver_e2e do this; harness_langcore
# and harness_patterns spell the equivalent `#[path = "common/mod.rs"]`, and
# harness_topology_selector does the same for `common/differential.rs`). The
# rule is therefore scoped: `#[path]` is mandatory for every former-standalone
# file moved under the harness directory, not for a retained `tests/` sibling.
# Section 6 encodes exactly this scoping — a bare `mod <ident>;` is a violation
# UNLESS crate-root-relative resolution actually lands on a retained sibling
# (`tests/<ident>.rs` or `tests/<ident>/mod.rs`), which is the same condition
# that decides whether the declaration compiles.
#
# Declaring the moved file under its ORIGINAL stem is what gives the merged
# test a stable, predictable module path GOING FORWARD (post-consolidation
# selectors are `<file>::<test_name>`) — it does NOT preserve the pre-
# consolidation id. The files being consolidated declare their `#[test]` fns
# at file top level with no enclosing `mod`, so before consolidation the
# nextest test name has NO module prefix and the binary id is `<crate>::<file>`;
# after, the binary id becomes `<crate>::harness_<subsystem>` and the test name
# gains the `<file>::` prefix. Both halves of the id change (measured,
# task #5283: `reify-compiler::trait_bounds_tests both_bound_check_paths_combined`
# -> `reify-compiler::harness_traits trait_bounds_tests::both_bound_check_paths_combined`
# — space-separated, the form `cargo nextest list` actually prints; `binary$test`
# appears nowhere in this repo). Consolidation IS layout-only in that no `#[test]`
# fn is added or removed (invariant I3) — it is NOT test-id-preserving. A
# hand-written `binary(…)`/`test(=…)` selector naming a former id must be updated
# in the same diff; nothing in-repo does today (no script/nextest-override/
# heavy-filter selects a consolidatable stem) — Section 7 below ENFORCES that
# rather than asserting it in prose (PRD §6 BT-2), and verify.sh's failed-only retry
# is unaffected — it derives `test(=…)` at run time from its own attempt-0 and
# refuses on tree_oid drift (scripts/verify.sh retry_failed_only).
#
# WITHIN-CRATE ONLY (invariant I2). Consolidation never moves a test across a
# crate boundary; `harness_<subsystem>.rs` only ever absorbs `tests/*.rs` from
# its OWN crate. Cross-crate test motion is out of contract.
#
# THE 7 NAMED OVERRIDE BINARIES ARE NEVER CONSOLIDATED (invariant I1). Seven
# standalone integration binaries stay standalone forever (separate compile
# units by deliberate design — distinct harness/#[should_panic]/single-focus
# semantics that a shared harness would break):
#     determinism, analytical_validation, modal_benchmarks   (reify-solver-elastic)
#     buckling_smoke, fea_diagnostics_e2e                     (reify-eval-fea-tests)
#     tensegrity_t0a, representation_within_assertion         (reify-eval)
# They are allow-listed by stem and are NEVER flagged as re-accretion.
#
# ===========================================================================
# C2 — the anti-re-accretion kLOC-cap drift guard (what this script enforces).
# ===========================================================================
#
# For each of the 5 CONSOLIDATABLE crates, enumerate its top-level `tests/*.rs`
# and assert:
#
#   (a) kLOC CAP. Every `harness_<subsystem>.rs` compile unit — the ROOT file
#       PLUS its own `harness_<subsystem>/` module directory PLUS the files it
#       includes from OUTSIDE that directory (NOT the root file alone) — is
#       <= the cap (CAP_LINES, measured as RAW `wc -l` line count summed over
#       the whole unit — simplest and conservative, PRD §11; ~20 kLOC is the
#       upper end of the §7 10-20 kLOC band). A harness that grows past the
#       cap must be SPLIT into a second `harness_<subsystem2>.rs`, never
#       allowed to balloon unbounded — and never accommodated by raising the
#       cap, which would loosen the C2 ratchet to fit its first offender and
#       contradict the ratified §3 W1/§7 band.
#
#       EXTERNAL INCLUDES ARE IN SCOPE. A root may `#[path]`- or bare-`mod`-
#       include a file that escapes its module dir — in this tree the shared
#       `tests/common/` helpers. rustc compiles a SEPARATE COPY of such a file
#       into every including binary, so those lines are real per-unit compile
#       cost and the cap governs them; charging the same helper to two units
#       is two real compilations, not double-counting. Excluding them would
#       leave this ratchet guarding a quantity it does not define, and would
#       be directionally exploitable (move code into `tests/common/`,
#       `#[path]`-include it, cap evaded). The measure is the transitive
#       `mod`-graph closure; the resolution rules live with the measure, in
#       harness-layout-lib.sh's `harness_layout_unit_lines` header.
#
#   (b) NAMING / NO RE-ACCRETION. Every top-level `tests/*.rs` is EITHER a
#       sanctioned `harness_<subsystem>.rs`, OR one of the 7 override binaries,
#       OR grandfathered in the baseline manifest (see the ratchet below).
#       Any OTHER standalone binary is a re-accretion violation: from B1
#       landing, adding a NEW gratuitous standalone test binary to one of
#       these 5 crates requires a conscious baseline edit, so "no new
#       standalone binary silently re-accretes".
#
#   (c) STRUCTURED VERDICT. Every pass/fail is emitted as a machine-parseable
#       token line (NOT a log-scrape) so a failing developer reads the exact
#       offending crate/file/reason directly:
#           HARNESS_KLOC_CAP FAIL crate=<c> file=<path> reason=exceeds-cap lines=<n> cap=<n> root_lines=<n> module_lines=<n> module_files=<n> external_lines=<n> external_files=<n>
#           HARNESS_KLOC_CAP FAIL crate=<c> file=<path> reason=unsanctioned-standalone
#           HARNESS_KLOC_CAP PASS crate=<c>
#           HARNESS_KLOC_CAP SUMMARY crates=<n> violations=<n>
#       On exceeds-cap, `lines=` is the WHOLE-UNIT total, and the four
#       breakdown fields decompose it as
#           lines = root_lines + module_lines + external_lines
#       so an operator reading an ARCHIVED merge-verify log can tell the three
#       remedies apart without re-deriving anything by hand:
#         - module_lines dominates  -> SPLIT the module dir into a second
#                                      harness_<subsystem2>.rs (module_files
#                                      says across how many files);
#         - root_lines dominates    -> trim the root;
#         - external_lines dominates-> the unit is over ONLY because of a
#                                      shared out-of-module-dir include (a
#                                      `tests/common/` helper), so move the
#                                      including submodules — and the include
#                                      with them — into their own harness.
#       The two external fields are APPENDED after `module_files=`, never
#       inserted before `lines=`, so existing unanchored consumers of this
#       grammar keep matching.
#
# ===========================================================================
# THE GRANDFATHER-BASELINE RATCHET (harness-layout-baseline.manifest).
# ===========================================================================
#
# The current pre-consolidation tree holds hundreds of un-consolidated
# standalone `tests/*.rs` across these 5 crates — none are harnesses yet, only
# 2 are overrides. A literal rule (b) would be RED today. The reconciliation
# is a checked-in GRANDFATHER BASELINE: harness-layout-baseline.manifest
# snapshots every currently-sanctioned standalone file, and rule (b) treats a
# baseline-listed file as sanctioned. On the current tree every file is
# grandfathered => the guard is GREEN today; a stray un-grandfathered file is
# flagged immediately.
#
# The baseline is a RATCHET, not a permanent allow-list: each consolidation
# leaf REMOVES its now-consolidated files from the baseline IN THE SAME DIFF
# that consolidates them (mirroring the mandatory same-diff
# run-all-classification.manifest row rule; esc-4914-162), so the baseline
# shrinks leaf-by-leaf and the guard stays green at every intermediate step.
# Baseline shrinking toward empty == consolidation progress.
#
# ===========================================================================
# This is a self-testing guard: hermetic mktemp -d fixtures drive positive
# (must-fire) and negative/allow-list (must-not-fire) self-checks, then a
# final LIVE scan asserts the guard is green on the real 5-crate tree.
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob; classified
# `pool` in run-all-classification.manifest (hermetic: static line counts +
# file reads over tmpdir fixtures, no cargo / CPU-burn / shared state).
# ===========================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

# Shared harness-layout contract data + predicates (task 5300). This guard and
# the diff-scoped scripts/check-harness-baseline-registration.sh both source
# this lib, so the consolidatable-crate set, the override set, and the
# baseline-membership semantics cannot silently diverge between them.
[ -f "$SCRIPT_DIR/harness-layout-lib.sh" ] || {
    echo "ERROR: harness-layout-lib.sh not found at $SCRIPT_DIR/harness-layout-lib.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/harness-layout-lib.sh"

# ---------------------------------------------------------------------------
# Config constants (the C1/C2 contract parameters).
# ---------------------------------------------------------------------------

# The 5 crates whose top-level tests/*.rs are subject to the C1 layout
# contract, and the 7 standalone integration binaries that are NEVER
# consolidated (I1). Both sets are now sourced from the shared
# harness-layout-lib.sh (single source of truth with the diff-scoped gate) —
# populated into these arrays so the rest of this guard is unchanged.
# reify-solver-elastic / reify-eval-fea-tests are deliberately NOT consolidatable:
# they host only override binaries + permanently-standalone tests.
CONSOLIDATABLE_CRATES=()
while IFS= read -r _c; do CONSOLIDATABLE_CRATES+=("$_c"); done < <(harness_layout_consolidatable_crates)
OVERRIDE_BINARIES=()
while IFS= read -r _ov; do OVERRIDE_BINARIES+=("$_ov"); done < <(harness_layout_override_stems)

# Raw-line-count cap per harness_<subsystem>.rs compile unit (PRD §11: raw
# line count, simplest/conservative; ~20 kLOC = upper end of the §7 band).
CAP_LINES=20000

# The checked-in grandfather-baseline ratchet (resolved via the shared lib so
# the REIFY_HARNESS_LAYOUT_BASELINE override is honored identically by both
# guards; default path is unchanged).
BASELINE="$(harness_layout_baseline_path)"

# ---------------------------------------------------------------------------
# _emit <VERDICT> <field>... — emit one canonical structured verdict line.
#
# Centralizes the `HARNESS_KLOC_CAP <VERDICT> <fields...>` grammar (rule c) so
# every PASS / FAIL / SUMMARY line is produced in exactly ONE place — a
# consumer parses `HARNESS_KLOC_CAP <VERDICT> key=value...` without regexing
# prose.
# ---------------------------------------------------------------------------
_emit() {
    local verdict="$1"
    shift
    printf 'HARNESS_KLOC_CAP %s %s\n' "$verdict" "$*"
}

# ---------------------------------------------------------------------------
# harness_layout_violations <crate> <tests_dir> <baseline_file> <cap_lines>
#
# Enumerate the top-level *.rs compile units in <tests_dir>, classify each
# against the C1/C2 contract, and print a structured verdict line per
# violation to stdout. Returns 1 if <crate> has any violation, else 0.
#
# Parameterized on (tests_dir, baseline_file, cap_lines) so the hermetic
# seeded-fixture self-checks drive synthetic dirs/baselines/caps exactly as the
# live driver drives the real crate tests dirs.
#
# rule (a) — kLOC cap: for each harness_<subsystem>.rs, take the raw `wc -l`
# line count of the WHOLE compile unit (the root file, its own
# harness_<subsystem>/ module directory, and the transitive closure of the
# includes that escape that directory — via harness_layout_unit_lines) and
# flag it if it exceeds <cap_lines>.
# ---------------------------------------------------------------------------
harness_layout_violations() {
    local crate="$1"
    local tests_dir="$2"
    local baseline_file="$3"
    local cap_lines="$4"

    local violations=0
    local f base lines key root_lines module_lines module_files external_lines external_files

    # Graceful degradation: a missing crate tests dir is an explicit FAIL, never
    # a silent pass — a non-existent dir would otherwise glob to zero files and
    # masquerade as a clean crate.
    if [ ! -d "$tests_dir" ]; then
        _emit FAIL "crate=$crate" "dir=$tests_dir" "reason=missing-tests-dir"
        return 1
    fi

    # rule (b) baseline membership is delegated to the shared lib's
    # harness_layout_baseline_contains (single source of truth with the
    # diff-scoped gate; same comment/blank stripping), called per candidate
    # below.

    # The 7 override binaries (I1) are allow-listed by file stem — never
    # re-accretion violations. Build the lookup set once per call.
    local -A _override_set=()
    local _ov
    for _ov in "${OVERRIDE_BINARIES[@]}"; do
        _override_set["$_ov"]=1
    done

    for f in "$tests_dir"/*.rs; do
        [ -f "$f" ] || continue          # skip a literal no-match glob

        base="$(basename "$f")"

        # rule (a): kLOC cap governs the harness_<subsystem>.rs compile units.
        case "$base" in
            harness_*.rs)
                IFS=' ' read -r lines root_lines module_lines module_files external_lines external_files \
                    <<<"$(harness_layout_unit_lines "$f")" || true
                if [ "$lines" -gt "$cap_lines" ]; then
                    _emit FAIL "crate=$crate" "file=$f" "reason=exceeds-cap" \
                        "lines=$lines" "cap=$cap_lines" \
                        "root_lines=$root_lines" "module_lines=$module_lines" "module_files=$module_files" \
                        "external_lines=$external_lines" "external_files=$external_files"
                    violations=$((violations + 1))
                fi
                continue
                ;;
        esac

        # rule (b): re-accretion. A non-harness standalone is sanctioned iff it
        # is one of the 7 override binaries (I1, never consolidated) OR its
        # canonical repo-relative path crates/<crate>/tests/<base> is
        # grandfathered in the baseline.
        if [ -n "${_override_set[${base%.rs}]:-}" ]; then
            continue
        fi
        key="crates/$crate/tests/$base"
        if ! harness_layout_baseline_contains "$key" "$baseline_file"; then
            _emit FAIL "crate=$crate" "file=$f" "reason=unsanctioned-standalone"
            violations=$((violations + 1))
        fi
    done

    if [ "$violations" -gt 0 ]; then
        return 1
    fi
    _emit PASS "crate=$crate"
    return 0
}

# ---------------------------------------------------------------------------
# run_harness_layout_scan <baseline_file> <cap_lines> <crate:dir>...
#
# Aggregating driver: run harness_layout_violations over each `crate:dir` pair,
# pass its structured verdict lines through, tally the TOTAL FAIL verdicts
# across all crates, and emit a single `HARNESS_KLOC_CAP SUMMARY crates=<N>
# violations=<V>` line. Returns non-zero iff V > 0.
#
# The total is counted from the detector's own FAIL verdict lines (the
# structured output is the contract), so V is the true number of violating
# files, not merely the number of violating crates.
# ---------------------------------------------------------------------------
run_harness_layout_scan() {
    local baseline="$1"
    local cap="$2"
    shift 2

    local crate_count=0 total_violations=0
    local pair crate dir crate_out n

    for pair in "$@"; do
        crate="${pair%%:*}"
        dir="${pair#*:}"
        crate_count=$((crate_count + 1))
        # Capture the crate's verdict lines (|| true: the detector returns 1 on
        # a violation, which must not abort the driver under `set -e`), count
        # its FAIL verdicts, then pass the lines through unchanged.
        crate_out="$(harness_layout_violations "$crate" "$dir" "$baseline" "$cap")" || true
        n="$(printf '%s\n' "$crate_out" | grep -cE '^HARNESS_KLOC_CAP FAIL ' || true)"
        total_violations=$((total_violations + n))
        printf '%s\n' "$crate_out"
    done

    _emit SUMMARY "crates=$crate_count" "violations=$total_violations"
    [ "$total_violations" -eq 0 ]
}

# ---------------------------------------------------------------------------
# _bare_mod_decls <file> — print `<lineno>|<ident>` for every `mod <ident>;`
# item declaration in <file> that is NOT carrying a `#[path = "…"]` attribute.
#
# Intervening blank lines and `//` comments between the attribute and the `mod`
# preserve the attribute's binding; any other line clears it. Only the
# `mod <ident>;` FORM is considered — an inline `mod x { … }` block declares no
# out-of-line file and so is outside the C1 `#[path]` mandate entirely.
# ---------------------------------------------------------------------------
_bare_mod_decls() {
    awk '
        /^[[:space:]]*$/                    { next }   # blank: attribute still binds
        /^[[:space:]]*\/\//                 { next }   # comment: attribute still binds
        /^[[:space:]]*#\[path[[:space:]]*=/ { pathline = 1; next }
        /^[[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[[:space:]]*;/ {
            if (!pathline) {
                ident = $0
                sub(/^[[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+/, "", ident)
                sub(/[[:space:]]*;.*$/, "", ident)
                printf "%d|%s\n", NR, ident
            }
            pathline = 0
            next
        }
        { pathline = 0 }
    ' "$1"
}

# ---------------------------------------------------------------------------
# harness_path_attr_violations <crate> <tests_dir>
#
# C1 `#[path]`-MANDATE detector. For every `<tests_dir>/harness_<subsystem>.rs`
# root, flag each bare `mod <ident>;` whose crate-root-relative resolution does
# NOT land on a retained `tests/` sibling.
#
# WHY THIS IS A GUARD AND NOT A COMMENT. A harness root is an integration-test
# CRATE root, so Rust resolves a bare `mod <ident>;` against `tests/<ident>.rs`
# or `tests/<ident>/mod.rs` — never `tests/harness_<subsystem>/<ident>.rs`. For
# a file MOVED under the harness dir that is either a hard compile error or,
# mid-move while a stale top-level `tests/<ident>.rs` still exists, a SILENT
# bind to the wrong file. The silent mode is why prose alone is insufficient.
#
# The predicate is exactly the C1 scoping, not a blunt "always `#[path]`": a
# bare `mod <ident>;` is CORRECT precisely when crate-root-relative resolution
# lands on a real retained sibling (the shared `common` helper), which is the
# same condition that decides whether the declaration compiles at all. So the
# check has no allow-list to drift — it re-derives the exception from disk.
#
# Prints one structured FAIL line per violation; returns 1 if <crate> has any.
# Parameterized on <tests_dir> so hermetic fixtures drive it exactly as live.
#
# NOTE (deferred, out of this task's lock scope): the sibling diff-scoped gate
# scripts/check-harness-baseline-registration.sh states the same `#[path]` rule
# in human-facing guidance text only. Hoisting this predicate into the shared
# tests/infra/harness-layout-lib.sh so both guards execute one copy (the G7
# no-lockstep-duplication shape the lib already exists for) needs a write to
# that lib, which this task does not hold — filed as follow-up.
# ---------------------------------------------------------------------------
harness_path_attr_violations() {
    local crate="$1"
    local tests_dir="$2"

    local violations=0
    local root ident lineno

    if [ ! -d "$tests_dir" ]; then
        _emit FAIL "crate=$crate" "dir=$tests_dir" "reason=missing-tests-dir"
        return 1
    fi

    for root in "$tests_dir"/harness_*.rs; do
        [ -f "$root" ] || continue          # skip a literal no-match glob
        while IFS='|' read -r lineno ident; do
            [ -n "$ident" ] || continue
            # The ONE principled exception: a module that genuinely still lives
            # as a `tests/` sibling, i.e. crate-root-relative resolution really
            # does find it (`common` today).
            if [ -f "$tests_dir/$ident.rs" ] || [ -f "$tests_dir/$ident/mod.rs" ]; then
                continue
            fi
            _emit FAIL "crate=$crate" "file=$root" "reason=missing-path-attr" \
                "module=$ident" "line=$lineno"
            violations=$((violations + 1))
        done < <(_bare_mod_decls "$root")
    done

    if [ "$violations" -gt 0 ]; then
        return 1
    fi
    _emit PASS "crate=$crate"
    return 0
}

# ---------------------------------------------------------------------------
# harness_stale_selector_violations <root_dir> <scan_path>...
#
# PRD §6 BT-2 detector: no repo-resident selector names a PRE-consolidation
# compile-unit id. Derives the set of consolidated stems from disk (every
# `*.rs` under any `crates/*/tests/harness_<subsystem>/` module directory —
# precisely the ids that used to be standalone binaries), then flags any
# `binary(<stem>)` filterset atom or `--test <stem>` cargo/nextest target
# selector in <scan_path>... that names one. Such a selector silently selects
# NOTHING (`binary()`) or hard-errors with `no test target named <stem>`
# (`--test`) — the exact staleness this task fixed by hand in the
# auto-type-param PRD.
#
# DELIBERATE LIMIT (stated, not silently capped): the third breakage form — a
# `test(=<bare_name>)` selector naming a pre-consolidation TEST name, which now
# needs a `<file>::` prefix — is not statically detectable, since test names
# only exist in a compiled `cargo nextest list`. BT-1's #[test]-fn count check
# is what covers that axis. This detector covers the two forms that ARE
# decidable from the tree: compile-unit / target names.
#
# Prints one structured FAIL line per violation; returns 1 on any. <root_dir>
# is parameterized so hermetic fixtures drive it exactly as the live tree does.
# ---------------------------------------------------------------------------
harness_stale_selector_violations() {
    local root_dir="$1"
    shift

    local violations=0
    local stems_file d p atom file stem

    stems_file="$(mktemp)"; _TMPDIRS+=("$stems_file")
    for d in "$root_dir"/crates/*/tests/harness_*/; do
        [ -d "$d" ] || continue
        find "$d" -type f -name '*.rs' 2>/dev/null
    done | sed 's|.*/||; s|\.rs$||' | sort -u > "$stems_file"

    # No consolidated stems yet (pre-W1 tree, or a fixture with none) => nothing
    # can be stale. Not a vacuous pass: the live tree has 300+ stems, and
    # Section 7's non-vacuity assert pins that.
    if [ ! -s "$stems_file" ]; then
        _emit PASS "scan=selectors" "stems=0"
        return 0
    fi

    for p in "$@"; do
        [ -e "$p" ] || continue
        while IFS= read -r atom; do
            [ -n "$atom" ] || continue
            file="${atom%%:*}"
            stem="${atom#*:}"
            # Normalize both accepted atom shapes down to the bare stem.
            stem="${stem#binary(}"
            stem="${stem%)}"
            stem="${stem##--test*[[:space:]]}"
            grep -qxF "$stem" "$stems_file" || continue
            _emit FAIL "crate=-" "file=$file" "reason=stale-selector" "stem=$stem"
            violations=$((violations + 1))
        done < <(grep -rHoE 'binary\([A-Za-z_][A-Za-z0-9_]*\)|--test[[:space:]]+[A-Za-z_][A-Za-z0-9_]*' \
                    "$p" 2>/dev/null || true)
    done

    if [ "$violations" -gt 0 ]; then
        return 1
    fi
    _emit PASS "scan=selectors" "stems=$(wc -l < "$stems_file" | tr -d ' ')"
    return 0
}

echo "=== Harness-layout contract + anti-re-accretion kLOC-cap drift guard ==="

# Collect every mktemp -d / mktemp path for a SINGLE EXIT cleanup (the
# test_no_new_wallclock_upper_bounds.sh idiom): individual `trap ... EXIT`
# calls replace one another, so one handler over an array removes every
# fixture regardless of which section runs last. `rm -rf` covers both the
# tmpdirs and the tmpfiles collected below.
_TMPDIRS=()
trap '[ "${#_TMPDIRS[@]}" -gt 0 ] && rm -rf "${_TMPDIRS[@]}"' EXIT

# ===========================================================================
# Section 1: rule (a) — the kLOC cap fires on an over-cap harness_*.rs.
# ===========================================================================
echo ""
echo "--- Section 1: kLOC cap fires on a 21k-line harness ---"

_s1_dir="$(mktemp -d)"; _TMPDIRS+=("$_s1_dir")
_s1_baseline="$(mktemp)"; _TMPDIRS+=("$_s1_baseline")
: > "$_s1_baseline"   # empty fixture baseline (nothing grandfathered)

# Generate EXACTLY 21000 lines (21000 > CAP 20000 by construction). awk (not
# `yes | head`): under `set -o pipefail` a `head` that closes the pipe SIGPIPEs
# the still-writing `yes` (141), aborting the script under `set -e` — the
# esc-5172-1 hazard. awk runs to completion, no pipe, no SIGPIPE.
awk 'BEGIN { for (i = 0; i < 21000; i++) print "// x" }' > "$_s1_dir/harness_big.rs"

_s1_out="$(mktemp)"; _TMPDIRS+=("$_s1_out")
_s1_rc=0
harness_layout_violations synthcrate "$_s1_dir" "$_s1_baseline" 20000 \
    > "$_s1_out" 2>/dev/null || _s1_rc=$?

assert "1: over-cap harness_big.rs fires the kLOC cap (returns 1)" \
    test "$_s1_rc" -eq 1
assert "1: cap violation emitted as a structured FAIL line (exceeds-cap, lines=21000 cap=20000)" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL crate=synthcrate file=.*harness_big\.rs reason=exceeds-cap lines=21000 cap=20000' "$_s1_out"

# ===========================================================================
# Section 1b: rule (a) — the kLOC cap AGGREGATES the harness root + its own
# harness_<subsystem>/ module directory, not the root file alone.
# ===========================================================================
echo ""
echo "--- Section 1b: kLOC cap aggregates the harness module dir ---"

_s1b_baseline="$(mktemp)"; _TMPDIRS+=("$_s1b_baseline")
: > "$_s1b_baseline"   # empty fixture baseline (rule (a) never consults it)

# Fixture A: a SMALL root (100 lines, innocuous under a root-only measure)
# with a module dir that pushes the aggregate to 20100 > cap 20000.
_s1b_dir="$(mktemp -d)"; _TMPDIRS+=("$_s1b_dir")
mkdir -p "$_s1b_dir/harness_split"
awk 'BEGIN { for (i = 0; i < 100; i++) print "// x" }'  > "$_s1b_dir/harness_split.rs"
awk 'BEGIN { for (i = 0; i < 6667; i++) print "// x" }' > "$_s1b_dir/harness_split/a.rs"
awk 'BEGIN { for (i = 0; i < 6667; i++) print "// x" }' > "$_s1b_dir/harness_split/b.rs"
awk 'BEGIN { for (i = 0; i < 6666; i++) print "// x" }' > "$_s1b_dir/harness_split/c.rs"

_s1b_out="$(mktemp)"; _TMPDIRS+=("$_s1b_out")
_s1b_rc=0
harness_layout_violations synthcrate "$_s1b_dir" "$_s1b_baseline" 20000 \
    > "$_s1b_out" 2>/dev/null || _s1b_rc=$?

assert "1b: a 100-line root with a 20000-line module dir fires the kLOC cap (returns 1, aggregate 20100 > 20000)" \
    test "$_s1b_rc" -eq 1
assert "1b: cap violation reports the AGGREGATE (lines=20100), not the 100-line root" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL crate=synthcrate file=.*harness_split\.rs reason=exceeds-cap lines=20100 cap=20000' "$_s1b_out"

# BOUNDARY PRECISION companion: aggregate EXACTLY AT the cap (1-line root +
# 19999-line module dir = 20000) must NOT fire — pins `-gt` (strictly
# greater), not `-ge`, now that the aggregate (not the root) is the compared
# quantity.
_s1b2_dir="$(mktemp -d)"; _TMPDIRS+=("$_s1b2_dir")
mkdir -p "$_s1b2_dir/harness_boundary"
awk 'BEGIN { for (i = 0; i < 1; i++) print "// x" }'     > "$_s1b2_dir/harness_boundary.rs"
awk 'BEGIN { for (i = 0; i < 19999; i++) print "// x" }' > "$_s1b2_dir/harness_boundary/only.rs"

_s1b2_out="$(mktemp)"; _TMPDIRS+=("$_s1b2_out")
_s1b2_rc=0
harness_layout_violations synthcrate "$_s1b2_dir" "$_s1b_baseline" 20000 \
    > "$_s1b2_out" 2>/dev/null || _s1b2_rc=$?

assert "1b: aggregate EXACTLY at the cap (20000) does not fire (returns 0, boundary is -gt not -ge)" \
    test "$_s1b2_rc" -eq 0
assert "1b: at-boundary crate emits a structured PASS line" \
    grep -Eq '^HARNESS_KLOC_CAP PASS crate=synthcrate' "$_s1b2_out"
assert "1b: at-boundary crate emits no FAIL line" \
    bash -c '! grep -qE "^HARNESS_KLOC_CAP FAIL" "$1"' _ "$_s1b2_out"

# ===========================================================================
# Section 1c: rule (a) — the module-dir walk RECURSES into nested subdirs and
# counts ONLY *.rs files.
# ===========================================================================
echo ""
echo "--- Section 1c: module-dir walk is recursive and .rs-only ---"

_s1c_baseline="$(mktemp)"; _TMPDIRS+=("$_s1c_baseline")
: > "$_s1c_baseline"   # empty fixture baseline (rule (a) never consults it)

_s1c_dir="$(mktemp -d)"; _TMPDIRS+=("$_s1c_dir")
mkdir -p "$_s1c_dir/harness_nested/deep"
awk 'BEGIN { for (i = 0; i < 10; i++) print "// x" }'    > "$_s1c_dir/harness_nested.rs"
awk 'BEGIN { for (i = 0; i < 5; i++) print "// x" }'     > "$_s1c_dir/harness_nested/a.rs"
awk 'BEGIN { for (i = 0; i < 7; i++) print "// x" }'     > "$_s1c_dir/harness_nested/deep/b.rs"
awk 'BEGIN { for (i = 0; i < 50000; i++) print "// x" }' > "$_s1c_dir/harness_nested/notes.txt"

_s1c_tuple="$(harness_layout_unit_lines "$_s1c_dir/harness_nested.rs")"
assert "1c: harness_layout_unit_lines recurses into nested subdirs and counts only *.rs (total=22 root=10 module=12 files=2, no external includes: 0 0)" \
    test "$_s1c_tuple" = "22 10 12 2 0 0"

_s1c_out="$(mktemp)"; _TMPDIRS+=("$_s1c_out")
_s1c_rc=0
harness_layout_violations synthcrate "$_s1c_dir" "$_s1c_baseline" 20000 \
    > "$_s1c_out" 2>/dev/null || _s1c_rc=$?

assert "1c: a 50000-line colocated non-.rs fixture file never REDs the merge gate (returns 0)" \
    test "$_s1c_rc" -eq 0
assert "1c: clean scan emits a structured PASS line" \
    grep -Eq '^HARNESS_KLOC_CAP PASS crate=synthcrate' "$_s1c_out"
assert "1c: clean scan emits no FAIL line" \
    bash -c '! grep -qE "^HARNESS_KLOC_CAP FAIL" "$1"' _ "$_s1c_out"

# Companion precision: an ORPHAN module dir (no sibling harness_orphan.rs
# root) is not a compile unit and must be silently ignored — pins the
# existing `for f in "$tests_dir"/*.rs` glob behavior against a future
# rewrite that starts walking directories directly.
_s1c_orphan_dir="$(mktemp -d)"; _TMPDIRS+=("$_s1c_orphan_dir")
mkdir -p "$_s1c_orphan_dir/harness_orphan"
awk 'BEGIN { for (i = 0; i < 3; i++) print "// x" }' > "$_s1c_orphan_dir/harness_orphan/x.rs"

_s1c_orphan_out="$(mktemp)"; _TMPDIRS+=("$_s1c_orphan_out")
_s1c_orphan_rc=0
harness_layout_violations synthcrate "$_s1c_orphan_dir" "$_s1c_baseline" 20000 \
    > "$_s1c_orphan_out" 2>/dev/null || _s1c_orphan_rc=$?

assert "1c: an orphan module dir with no sibling root is silently ignored (returns 0)" \
    test "$_s1c_orphan_rc" -eq 0
assert "1c: orphan-dir scan emits a structured PASS line" \
    grep -Eq '^HARNESS_KLOC_CAP PASS crate=synthcrate' "$_s1c_orphan_out"
assert "1c: orphan-dir scan emits no FAIL line" \
    bash -c '! grep -qE "^HARNESS_KLOC_CAP FAIL" "$1"' _ "$_s1c_orphan_out"

# ===========================================================================
# Section 1d: rule (c) — the exceeds-cap verdict carries the root/module
# breakdown, not just the aggregate total (diagnosability: a developer
# reading `lines=20100` on a 100-line root file must be told where the lines
# actually are).
# ===========================================================================
echo ""
echo "--- Section 1d: exceeds-cap verdict carries the root/module breakdown ---"

# Reuses Section 1b's already-captured $_s1b_out (100-line root + 20000-line
# module dir, aggregate 20100 > cap 20000) and Section 1's already-captured
# $_s1_out (21000-line single-file harness, no module dir) instead of
# regenerating either ~20k-line fixture and re-scanning: both sections drive
# the same harness_layout_violations code path / _emit call this section
# pins, so their captured output already carries whatever breakdown fields
# rule (a) emits. Section 1/1b's own asserts are NOT `$`-anchored, so they
# pass regardless of whether the breakdown fields are present; the anchored
# asserts below are what actually pin the fields' presence, values, AND that
# they are appended AFTER `cap=` (not inserted before `lines=`, which would
# break Section 1's existing regex).
assert "1d: exceeds-cap verdict appends root_lines/module_lines/module_files AFTER cap= (module-dir case)" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL crate=synthcrate file=.*harness_split\.rs reason=exceeds-cap lines=20100 cap=20000 root_lines=100 module_lines=20000 module_files=3 external_lines=0 external_files=0$' "$_s1b_out"

# Coherence for a single-file harness (no module dir): root_lines equals the
# whole total and module_lines/module_files are both 0.
assert "1d: exceeds-cap verdict reports a coherent breakdown for a single-file harness (no module dir: root_lines=21000 module_lines=0 module_files=0)" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL crate=synthcrate file=.*harness_big\.rs reason=exceeds-cap lines=21000 cap=20000 root_lines=21000 module_lines=0 module_files=0 external_lines=0 external_files=0$' "$_s1_out"

# ===========================================================================
# Section 1e: rule (a) — EXTERNAL (out-of-module-dir) includes are attributed
# to the compile unit.
#
# A harness root may `#[path]`- or bare-`mod`-include a file that lives OUTSIDE
# its own `harness_<subsystem>/` module directory — in this tree, the shared
# `tests/common/` helpers. rustc compiles a separate copy of every such file
# into EVERY including binary, so those lines are real per-unit compile cost and
# rule (a)'s cap governs them. Leaving them unattributed is not a neutral
# simplification: it is a directionally exploitable bypass of the C2 ratchet
# (move code to `tests/common/`, `#[path]`-include it, cap evaded).
#
# `harness_layout_unit_lines` therefore reports a SIX-field tuple
#     "<total> <root_lines> <module_lines> <module_files> <external_lines> <external_files>"
# with total = root_lines + module_lines + external_lines. The fixtures below
# pin the resolution rules (which mirror rustc's), transitivity, visited-set
# dedup, the unresolvable-target case, and the no-double-counting boundary
# against the existing module-dir `find` walk.
# ===========================================================================
echo ""
echo "--- Section 1e: external (out-of-module-dir) includes are attributed ---"

# --- Fixture A: an escaping `#[path]` IS external; an in-module-dir `#[path]`
#     is NOT double-counted (it is already counted by the find walk). ---
_s1e_dir="$(mktemp -d)"; _TMPDIRS+=("$_s1e_dir")
mkdir -p "$_s1e_dir/harness_ext" "$_s1e_dir/shared"
{
    printf '#[path = "shared/helper.rs"]\n'
    printf 'mod helper;\n'
    printf '#[path = "harness_ext/inner.rs"]\n'
    printf 'mod inner;\n'
} > "$_s1e_dir/harness_ext.rs"                                    # root: 4 lines
awk 'BEGIN { for (i = 0; i < 30; i++) print "// x" }' > "$_s1e_dir/harness_ext/inner.rs"
awk 'BEGIN { for (i = 0; i < 40; i++) print "// x" }' > "$_s1e_dir/shared/helper.rs"

_s1e_tuple="$(harness_layout_unit_lines "$_s1e_dir/harness_ext.rs")"
assert "1e: an escaping #[path] include is attributed as external, and an in-module-dir #[path] is NOT double-counted (total=74 root=4 module=30 files=1 external=40 extfiles=1)" \
    test "$_s1e_tuple" = "74 4 30 1 40 1"

# --- Fixture B: a bare `mod common;` resolving to the retained `tests/` sibling
#     is external; the walk is TRANSITIVE through it; a file reachable twice is
#     counted ONCE; an unresolvable target contributes 0 without aborting. ---
_s1e2_dir="$(mktemp -d)"; _TMPDIRS+=("$_s1e2_dir")
mkdir -p "$_s1e2_dir/common"
{
    # (ii) crate-root-relative bare `mod` onto the retained sibling directory
    #      module `tests/common/mod.rs` — the ONE principled C1 exception.
    printf 'mod common;\n'
    # (iv) a SECOND declaration resolving to the very same file: visited-set
    #      dedup must count common/mod.rs exactly once.
    printf '#[path = "common/mod.rs"]\n'
    printf 'mod common_again;\n'
    # (v) a target that does not exist on disk: contributes 0, and must not
    #     abort this `set -euo pipefail` script.
    printf 'mod does_not_exist;\n'
} > "$_s1e2_dir/harness_sib.rs"                                   # root: 4 lines
# (iii) transitivity: common/mod.rs itself declares a submodule (this is the
#       live reify-eval shape — common/mod.rs declares alloc_counter/as_printed).
#       `mod.rs` resolves a bare `mod sub;` against its OWN directory.
{
    printf 'pub mod sub;\n'
    printf 'pub fn helper() {}\n'
} > "$_s1e2_dir/common/mod.rs"                                    # 2 lines
awk 'BEGIN { for (i = 0; i < 13; i++) print "// x" }' > "$_s1e2_dir/common/sub.rs"

_s1e2_rc=0
_s1e2_tuple="$(harness_layout_unit_lines "$_s1e2_dir/harness_sib.rs")" || _s1e2_rc=$?
assert "1e: an unresolvable \`mod\` target neither aborts nor errors the measure (rc 0)" \
    test "$_s1e2_rc" -eq 0
assert "1e: bare-\`mod\` sibling + transitive submodule are external, double-reached file counted once (total=19 root=4 module=0 files=0 external=15 extfiles=2)" \
    test "$_s1e2_tuple" = "19 4 0 0 15 2"

# --- Fixture C: a file INSIDE the module dir is TRAVERSED (its own escaping
#     include is attributed) even though the file itself is not re-counted —
#     the two halves of the in-module-dir rule, pinned together. Also pins that
#     a `../`-relative `#[path]` is resolved, not string-matched: the naive
#     prefix test would misread `harness_deep/../shared/deep.rs` as "inside the
#     module dir" and silently drop it. ---
_s1e3_dir="$(mktemp -d)"; _TMPDIRS+=("$_s1e3_dir")
mkdir -p "$_s1e3_dir/harness_deep" "$_s1e3_dir/shared"
{
    printf '#[path = "harness_deep/inner.rs"]\n'
    printf 'mod inner;\n'
} > "$_s1e3_dir/harness_deep.rs"                                  # root: 2 lines
{
    printf '#[path = "../shared/deep.rs"]\n'
    printf 'mod deep;\n'
    awk 'BEGIN { for (i = 0; i < 8; i++) print "// x" }'
} > "$_s1e3_dir/harness_deep/inner.rs"                            # 10 lines
awk 'BEGIN { for (i = 0; i < 17; i++) print "// x" }' > "$_s1e3_dir/shared/deep.rs"

_s1e3_tuple="$(harness_layout_unit_lines "$_s1e3_dir/harness_deep.rs")"
assert "1e: a module-dir file is traversed so ITS escaping ../ include is attributed, without re-counting the module-dir file (total=29 root=2 module=10 files=1 external=17 extfiles=1)" \
    test "$_s1e3_tuple" = "29 2 10 1 17 1"

# --- Coherence: the total is exactly root + module + external in every case. ---
_s1e_total_coherent() {
    local tuple total root mod files ext extfiles
    for tuple in "$_s1e_tuple" "$_s1e2_tuple" "$_s1e3_tuple"; do
        read -r total root mod files ext extfiles <<<"$tuple"
        [ "$total" -eq $((root + mod + ext)) ] || return 1
    done
    return 0
}
assert "1e: total == root_lines + module_lines + external_lines across every fixture" \
    _s1e_total_coherent

# ===========================================================================
# Section 1f: rules (a)+(c) — the exceeds-cap VERDICT carries the external
# breakdown, and a unit goes over the cap on the strength of its external
# includes alone.
#
# This is the behavioural core of task #5620: before attribution, a harness
# whose root + module dir sit comfortably under the cap could carry unbounded
# extra compile cost through a `#[path]` include that escapes the module dir,
# and the gate would never fire. The fixture below is exactly that shape.
#
# The two new fields are APPENDED after `module_files=` — never inserted before
# `lines=` — so Section 1/1b's unanchored regexes keep passing (the same
# append-don't-insert discipline Section 1d states). They let an operator
# reading an archived merge-verify log tell "split the module dir" from "trim
# the root" from "this unit is over ONLY because of a shared tests/common/
# include" without re-deriving anything by hand.
# ===========================================================================
echo ""
echo "--- Section 1f: exceeds-cap verdict carries the external breakdown ---"

_s1f_baseline="$(mktemp)"; _TMPDIRS+=("$_s1f_baseline")
: > "$_s1f_baseline"   # empty fixture baseline (rule (a) never consults it)

_s1f_dir="$(mktemp -d)"; _TMPDIRS+=("$_s1f_dir")
mkdir -p "$_s1f_dir/harness_extcap" "$_s1f_dir/shared"
# Root + module dir = 15004 lines, comfortably UNDER the cap. The escaping
# `#[path = "shared/big.rs"]` include adds 6000 -> 21004, OVER the cap.
{
    printf '#[path = "shared/big.rs"]\n'
    printf 'mod big;\n'
    printf '#[path = "harness_extcap/local.rs"]\n'
    printf 'mod local;\n'
} > "$_s1f_dir/harness_extcap.rs"                                 # root: 4 lines
awk 'BEGIN { for (i = 0; i < 15000; i++) print "// x" }' > "$_s1f_dir/harness_extcap/local.rs"
awk 'BEGIN { for (i = 0; i < 6000; i++) print "// x" }'  > "$_s1f_dir/shared/big.rs"
# Second unit in the SAME scan, over cap on its own lines with NO external
# include: pins that the external fields are emitted UNCONDITIONALLY and are
# re-derived per file (a stale carry-over from the unit above would show up
# here as a non-zero external_lines).
awk 'BEGIN { for (i = 0; i < 20001; i++) print "// x" }' > "$_s1f_dir/harness_plain.rs"

_s1f_out="$(mktemp)"; _TMPDIRS+=("$_s1f_out")
_s1f_rc=0
harness_layout_violations synthcrate "$_s1f_dir" "$_s1f_baseline" 20000 \
    > "$_s1f_out" 2>/dev/null || _s1f_rc=$?

assert "1f: a harness UNDER cap on root+module (15004) fires once its escaping #[path] include is attributed (returns 1)" \
    test "$_s1f_rc" -eq 1
assert "1f: exceeds-cap verdict appends external_lines/external_files AFTER module_files= (lines=21004, external_lines=6000 external_files=1)" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL crate=synthcrate file=.*harness_extcap\.rs reason=exceeds-cap lines=21004 cap=20000 root_lines=4 module_lines=15000 module_files=1 external_lines=6000 external_files=1$' "$_s1f_out"
assert "1f: a unit with no external includes reports external_lines=0 external_files=0 in the same scan (fields unconditional, no carry-over)" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL crate=synthcrate file=.*harness_plain\.rs reason=exceeds-cap lines=20001 cap=20000 root_lines=20001 module_lines=0 module_files=0 external_lines=0 external_files=0$' "$_s1f_out"

# ===========================================================================
# Section 2: rule (b) — an unsanctioned standalone tests/*.rs fires.
# ===========================================================================
echo ""
echo "--- Section 2: unsanctioned standalone (stray.rs) fires ---"

_s2_dir="$(mktemp -d)"; _TMPDIRS+=("$_s2_dir")
printf 'fn main() {}\n' > "$_s2_dir/stray.rs"   # small, non-harness, non-override

_s2_baseline="$(mktemp)"; _TMPDIRS+=("$_s2_baseline")
# Grandfather only an UNRELATED file: the key for stray.rs is
# crates/synthcrate/tests/stray.rs, which is NOT in this baseline -> flagged.
printf 'crates/synthcrate/tests/other.rs\n' > "$_s2_baseline"

_s2_out="$(mktemp)"; _TMPDIRS+=("$_s2_out")
_s2_rc=0
harness_layout_violations synthcrate "$_s2_dir" "$_s2_baseline" 20000 \
    > "$_s2_out" 2>/dev/null || _s2_rc=$?

assert "2: unsanctioned stray.rs fires (returns 1)" \
    test "$_s2_rc" -eq 1
assert "2: stray flagged with a structured unsanctioned-standalone FAIL line" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL crate=synthcrate file=.*stray\.rs reason=unsanctioned-standalone' "$_s2_out"

# ===========================================================================
# Section 3: precision / non-vacuity — a sanctioned under-cap harness, an
# override binary, AND a grandfathered file are all correctly NOT flagged, and
# a clean crate emits a PASS line. Proves the guard is green-on-truth, not
# vacuously red.
# ===========================================================================
echo ""
echo "--- Section 3: precision (sanctioned harness/override/baseline not flagged) ---"

_s3_dir="$(mktemp -d)"; _TMPDIRS+=("$_s3_dir")
awk 'BEGIN { for (i = 0; i < 100; i++) print "// ok" }' > "$_s3_dir/harness_ok.rs"  # under cap
printf 'fn main() {}\n' > "$_s3_dir/tensegrity_t0a.rs"   # one of the 7 overrides
printf 'fn main() {}\n' > "$_s3_dir/grand.rs"            # grandfathered below

_s3_baseline="$(mktemp)"; _TMPDIRS+=("$_s3_baseline")
printf 'crates/synthcrate/tests/grand.rs\n' > "$_s3_baseline"

_s3_out="$(mktemp)"; _TMPDIRS+=("$_s3_out")
_s3_rc=0
harness_layout_violations synthcrate "$_s3_dir" "$_s3_baseline" 20000 \
    > "$_s3_out" 2>/dev/null || _s3_rc=$?

assert "3: sanctioned harness/override/grandfathered files are NOT flagged (returns 0)" \
    test "$_s3_rc" -eq 0
assert "3: a clean crate emits a structured PASS line" \
    grep -Eq '^HARNESS_KLOC_CAP PASS crate=synthcrate' "$_s3_out"
assert "3: no FAIL line is emitted for a clean crate (precision)" \
    bash -c '! grep -qE "^HARNESS_KLOC_CAP FAIL" "$1"' _ "$_s3_out"

# ===========================================================================
# Section 3b: graceful degradation — a MISSING tests dir is an explicit FAIL,
# never a silent pass. A non-existent dir globs to zero files and would
# otherwise masquerade as a clean crate; this pins the detector's early-return
# FAIL branch against a regression (e.g. flipping it to `continue`/`return 0`),
# exactly as Sections 1/2/3 pin rules (a)/(b) and precision.
# ===========================================================================
echo ""
echo "--- Section 3b: missing tests dir surfaces an explicit FAIL ---"

_s3b_base="$(mktemp -d)"; _TMPDIRS+=("$_s3b_base")
_s3b_missing="$_s3b_base/does-not-exist"   # deliberately NEVER created
_s3b_baseline="$(mktemp)"; _TMPDIRS+=("$_s3b_baseline")
: > "$_s3b_baseline"   # empty baseline: isolates the test to the missing-dir path

_s3b_out="$(mktemp)"; _TMPDIRS+=("$_s3b_out")
_s3b_rc=0
harness_layout_violations synthcrate "$_s3b_missing" "$_s3b_baseline" 20000 \
    > "$_s3b_out" 2>/dev/null || _s3b_rc=$?

assert "3b: a missing tests dir fires (returns 1, never a silent pass)" \
    test "$_s3b_rc" -eq 1
assert "3b: missing dir emitted as a structured FAIL line (reason=missing-tests-dir)" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL crate=synthcrate dir=.*does-not-exist reason=missing-tests-dir' "$_s3b_out"

# ===========================================================================
# Section 4: rule (c) — the aggregate SUMMARY line is machine-parseable, and
# EVERY emitted line obeys the canonical grammar (not a log-scrape). Drive TWO
# synthetic crates (one clean, one with a violation) through the aggregating
# driver run_harness_layout_scan <baseline> <cap> <crate:dir>...
# ===========================================================================
echo ""
echo "--- Section 4: aggregate SUMMARY grammar (rule c) ---"

_s4_clean_dir="$(mktemp -d)"; _TMPDIRS+=("$_s4_clean_dir")
printf 'fn main() {}\n' > "$_s4_clean_dir/grand.rs"      # grandfathered below

_s4_dirty_dir="$(mktemp -d)"; _TMPDIRS+=("$_s4_dirty_dir")
printf 'fn main() {}\n' > "$_s4_dirty_dir/stray.rs"      # NOT grandfathered -> 1 violation

_s4_baseline="$(mktemp)"; _TMPDIRS+=("$_s4_baseline")
printf 'crates/cleancrate/tests/grand.rs\n' > "$_s4_baseline"

_s4_out="$(mktemp)"; _TMPDIRS+=("$_s4_out")
_s4_rc=0
run_harness_layout_scan "$_s4_baseline" 20000 \
    "cleancrate:$_s4_clean_dir" "dirtycrate:$_s4_dirty_dir" \
    > "$_s4_out" 2>/dev/null || _s4_rc=$?

_s4_summary_count="$(grep -cE '^HARNESS_KLOC_CAP SUMMARY crates=2 violations=1$' "$_s4_out" || true)"
assert "4: exactly one structured SUMMARY crates=2 violations=1 line" \
    test "$_s4_summary_count" -eq 1
assert "4: aggregate scan returns non-zero when any crate has a violation (rc 1)" \
    test "$_s4_rc" -eq 1

# Non-blank lines that do NOT match the canonical grammar (`|| true`: the
# clean case is the grep-no-match exit 1).
_s4_bad="$(grep -vE '^[[:space:]]*$' "$_s4_out" | grep -vE '^HARNESS_KLOC_CAP (PASS|FAIL|SUMMARY) ' || true)"
assert "4: every emitted non-empty line matches the canonical HARNESS_KLOC_CAP grammar" \
    test -z "$_s4_bad"

# ===========================================================================
# Section 5: LIVE scan — the guard is GREEN on the real pre-consolidation tree
# (the headline user-observable signal). Also guard integrity: a missing/empty
# baseline must never let the scan vacuously pass.
# ===========================================================================
echo ""
echo "--- Section 5: live scan of the real 5 consolidatable crates ---"

# Guard integrity: a missing / empty baseline is never a silent pass. (An empty
# baseline would also flag all 867 grandfathered files -> a loud RED below —
# these explicit asserts state the intent regardless.)
_baseline_data="$(grep -vE '^[[:space:]]*#' "$BASELINE" 2>/dev/null | grep -vE '^[[:space:]]*$' || true)"
assert "5: grandfather baseline exists (guard integrity)" \
    test -f "$BASELINE"
assert "5: grandfather baseline is non-empty after comment/blank stripping (guard integrity)" \
    test -n "$_baseline_data"

# Wire the live scan over the 5 consolidatable crates' real tests dirs. A
# missing crate dir surfaces as an explicit missing-tests-dir FAIL from the
# detector (graceful degradation, never a silent pass).
_live_args=()
for _c in "${CONSOLIDATABLE_CRATES[@]}"; do
    _live_args+=("$_c:$REPO_ROOT/crates/$_c/tests")
done
_live_rc=0
_live_out="$(run_harness_layout_scan "$BASELINE" "$CAP_LINES" "${_live_args[@]}")" || _live_rc=$?

_live_summary="$(printf '%s\n' "$_live_out" | grep -E '^HARNESS_KLOC_CAP SUMMARY ' || true)"

# On a non-clean live scan, print the captured structured HARNESS_KLOC_CAP
# lines verbatim to stdout BEFORE the asserts below, so the archived
# merge-verify log carries the exact offending crate=/file=/reason= lines —
# not just the assert PASS/FAIL verdicts. Without this, `assert` only dumps
# the stdout/stderr of the `test ...` checker it invokes (which is always
# empty), so a failing live scan's own offender lines never reached the
# archived log (the 2026-07-20 incident: 4 live violations, zero offender
# lines captured, four investigations blocked). Gated on failure so a clean
# run's output is byte-for-byte unchanged.
if [ "$_live_rc" -ne 0 ] || [ "$_live_summary" != "HARNESS_KLOC_CAP SUMMARY crates=5 violations=0" ]; then
    echo "  ---- Section 5: live scan output (captured, printed on failure) ----"
    printf '%s\n' "$_live_out"
    echo "  ---- Section 5: end live scan output ----"
fi

assert "5: live scan is green on the current tree (rc 0, zero violations)" \
    test "$_live_rc" -eq 0
assert "5: live SUMMARY line reads exactly crates=5 violations=0" \
    test "$_live_summary" = "HARNESS_KLOC_CAP SUMMARY crates=5 violations=0"

# ===========================================================================
# Section 5b: live non-vacuity — the live measure actually reads module dirs,
# not just the harness_<subsystem>.rs root file.
# ===========================================================================
echo ""
echo "--- Section 5b: live non-vacuity (measure actually reads module dirs) ---"

# Under a root-only measure the largest live root is 170 lines
# (harness_cli.rs) — no live harness could ever report an aggregate over
# 10000 lines. This is a direct non-vacuity witness that the shared measure
# `harness_layout_unit_lines` is wired to the module dir at all: at least one
# live harness must show a SMALL root (<500 lines) alongside a LARGE aggregate
# (>10000 lines).
_s5b_nonvacuous() {
    local c f tuple total root mod files ext extfiles
    for c in "${CONSOLIDATABLE_CRATES[@]}"; do
        for f in "$REPO_ROOT/crates/$c/tests"/harness_*.rs; do
            [ -f "$f" ] || continue
            tuple="$(harness_layout_unit_lines "$f" 2>/dev/null)" || continue
            read -r total root mod files ext extfiles <<<"$tuple"
            [[ "$root" =~ ^[0-9]+$ ]] || continue
            [[ "$total" =~ ^[0-9]+$ ]] || continue
            if [ "$root" -lt 500 ] && [ "$total" -gt 10000 ]; then
                return 0
            fi
        done
    done
    return 1
}
assert "5b: at least one live harness has root<500 lines yet aggregate>10000 lines (module dir is actually read)" \
    _s5b_nonvacuous

# NOTE: deliberately no second assert re-checking $_live_summary here — it
# would be byte-identical to Section 5's "live SUMMARY line reads exactly
# crates=5 violations=0" assert (same variable, same expected string, no
# re-scan in between), so a real regression would report two failures for
# one cause. The non-vacuity check above is what this section actually
# contributes.
#
# HEADROOM (measured, task #5620, 14 live harness units). Attributing the
# escaping tests/common/ includes (Section 1e) put
# harness_topology_selector at 21470 = 97 root + 19245 module + 2128 external
# (`#[path = "common/differential.rs"]`), 7.4% OVER CAP_LINES=20000. Per rule
# (a)'s own remedy that was resolved by SPLITTING `selective_demand` out into
# harness_selective_demand — NOT by raising the cap, which would have
# contradicted the ratified 10-20 kLOC band of PRD §3 W1/§7 and loosened the
# C2 ratchet to fit its first offender. Post-split the measured max aggregate
# is 19591 (harness_fea_solver_e2e: 106 root + 19103 module + 382 external),
# so the tightest live unit now sits ~2% under the cap.

# ===========================================================================
# Section 5c: live non-vacuity of the EXTERNAL attribution — the out-of-module-
# dir walk is provably wired on the real tree, not silently degraded to a no-op.
#
# Section 1e pins the walk against hermetic fixtures, and Section 5 asserts the
# live scan is green. But a regression that made the walk return 0/0 on real
# paths would leave BOTH of those green while quietly reopening the bypass this
# task closed (move code into tests/common/, `#[path]`-include it, cap evaded).
# The live tree genuinely has such includes — every consolidated harness that
# reaches the shared `tests/common/` helpers — so at least one live unit must
# report external_files > 0.
# ===========================================================================
echo ""
echo "--- Section 5c: live non-vacuity of the external attribution ---"

_s5c_external_wired() {
    local c f tuple total root mod files ext extfiles
    for c in "${CONSOLIDATABLE_CRATES[@]}"; do
        for f in "$REPO_ROOT/crates/$c/tests"/harness_*.rs; do
            [ -f "$f" ] || continue
            tuple="$(harness_layout_unit_lines "$f" 2>/dev/null)" || continue
            read -r total root mod files ext extfiles <<<"$tuple"
            [[ "$extfiles" =~ ^[0-9]+$ ]] || continue
            [[ "$ext" =~ ^[0-9]+$ ]] || continue
            if [ "$extfiles" -gt 0 ] && [ "$ext" -gt 0 ]; then
                return 0
            fi
        done
    done
    return 1
}
assert "5c: at least one live harness attributes an out-of-module-dir include (external_files>0 — the walk is wired on the real tree)" \
    _s5c_external_wired

# ===========================================================================
# Section 6: C1 `#[path]` MANDATE — every `mod <ident>;` in a harness root
# either carries `#[path]` or resolves to a real retained `tests/` sibling.
#
# This closes the header's named SILENT failure mode: a bare `mod <file>;` for
# a file moved under `tests/harness_<subsystem>/` resolves crate-root-relative
# and binds a stale top-level `tests/<file>.rs` instead. Hermetic must-fire /
# must-not-fire fixtures, then a LIVE scan over every harness root on disk.
# ===========================================================================
echo ""
echo "--- Section 6: C1 #[path] mandate (must-fire / must-not-fire / live) ---"

# --- must-fire: a bare `mod` for a file that lives under the harness dir ---
_s6_bad_dir="$(mktemp -d)"; _TMPDIRS+=("$_s6_bad_dir")
mkdir -p "$_s6_bad_dir/harness_synth"
printf 'mod movedfile;\n' > "$_s6_bad_dir/harness_synth.rs"
printf '#[test]\nfn t() {}\n' > "$_s6_bad_dir/harness_synth/movedfile.rs"
# NOTE: deliberately NO "$_s6_bad_dir/movedfile.rs" — crate-root-relative
# resolution finds nothing, which is exactly the violation.

_s6_bad_out="$(mktemp)"; _TMPDIRS+=("$_s6_bad_out")
_s6_bad_rc=0
harness_path_attr_violations synthcrate "$_s6_bad_dir" \
    > "$_s6_bad_out" 2>/dev/null || _s6_bad_rc=$?

assert "6: a bare \`mod\` for a file moved under harness_<subsystem>/ fires (returns 1)" \
    test "$_s6_bad_rc" -eq 1
assert "6: violation emitted as a structured FAIL line (missing-path-attr, module+line named)" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL crate=synthcrate file=.*harness_synth\.rs reason=missing-path-attr module=movedfile line=1$' "$_s6_bad_out"

# --- must-not-fire: the three shapes that are CORRECT under C1 ---
_s6_ok_dir="$(mktemp -d)"; _TMPDIRS+=("$_s6_ok_dir")
mkdir -p "$_s6_ok_dir/harness_synth" "$_s6_ok_dir/common"
{
    # (a) the mandated form for a moved file
    printf '#[path = "harness_synth/movedfile.rs"]\n'
    printf 'mod movedfile;\n'
    # (b) THE principled exception: a bare `mod` onto a retained tests/ sibling
    #     directory module (`tests/common/mod.rs`) — correct precisely BECAUSE
    #     crate-root-relative resolution lands on it.
    printf 'mod common;\n'
    # (c) same, onto a retained tests/<ident>.rs sibling, with an intervening
    #     blank line + comment between a `#[path]` and its `mod` (the attribute
    #     must still bind, or (a)-shaped code would false-fire).
    printf 'mod sibling;\n'
    printf '#[path = "harness_synth/other.rs"]\n\n// still bound to the attribute above\nmod other;\n'
} > "$_s6_ok_dir/harness_synth.rs"
printf '#[test]\nfn t() {}\n' > "$_s6_ok_dir/harness_synth/movedfile.rs"
printf '#[test]\nfn t() {}\n' > "$_s6_ok_dir/harness_synth/other.rs"
printf 'pub fn helper() {}\n'  > "$_s6_ok_dir/common/mod.rs"
printf 'pub fn helper() {}\n'  > "$_s6_ok_dir/sibling.rs"

_s6_ok_out="$(mktemp)"; _TMPDIRS+=("$_s6_ok_out")
_s6_ok_rc=0
harness_path_attr_violations synthcrate "$_s6_ok_dir" \
    > "$_s6_ok_out" 2>/dev/null || _s6_ok_rc=$?

assert "6: mandated #[path] form + retained tests/ siblings (common/, sibling.rs) do NOT fire (returns 0)" \
    test "$_s6_ok_rc" -eq 0
assert "6: clean harness root emits a structured PASS line" \
    grep -Eq '^HARNESS_KLOC_CAP PASS crate=synthcrate' "$_s6_ok_out"
assert "6: clean harness root emits no FAIL line (precision — no blunt \"always #[path]\")" \
    bash -c '! grep -qE "^HARNESS_KLOC_CAP FAIL" "$1"' _ "$_s6_ok_out"

# --- LIVE scan: every crates/*/tests dir holding a harness root, on disk. Not
# scoped to CONSOLIDATABLE_CRATES, so a harness root appearing in any other
# crate is still covered. ---
_s6_live_out="$(mktemp)"; _TMPDIRS+=("$_s6_live_out")
_s6_live_rc=0
_s6_live_roots=0
: > "$_s6_live_out"
for _d in "$REPO_ROOT"/crates/*/tests; do
    [ -d "$_d" ] || continue
    compgen -G "$_d/harness_*.rs" > /dev/null || continue
    _s6_live_roots=$((_s6_live_roots + $(compgen -G "$_d/harness_*.rs" | wc -l)))
    _c="$(basename "$(dirname "$_d")")"
    harness_path_attr_violations "$_c" "$_d" >> "$_s6_live_out" 2>/dev/null || _s6_live_rc=1
done

# Offender lines to the archived log on failure (the Section 5 idiom).
if [ "$_s6_live_rc" -ne 0 ]; then
    echo "  ---- Section 6: live #[path] scan output (printed on failure) ----"
    cat "$_s6_live_out"
    echo "  ---- Section 6: end live scan output ----"
fi

assert "6: live tree honors the C1 #[path] mandate in every harness root (rc 0)" \
    test "$_s6_live_rc" -eq 0
# Non-vacuity: the live scan must actually have visited harness roots, or a
# future glob/layout change would silently turn the assert above into a no-op.
assert "6: live scan actually visited harness roots (>= 13 found, non-vacuity)" \
    test "$_s6_live_roots" -ge 13

# ===========================================================================
# Section 7: PRD §6 BT-2 — no repo-resident selector names a PRE-consolidation
# compile-unit id. Replaces BT-2's former hand-verified prose claim with a
# mechanical check over the real selector surfaces.
# ===========================================================================
echo ""
echo "--- Section 7: BT-2 no repo-resident selector names a consolidated stem ---"

# --- must-fire: a synthetic root whose scanned file selects a stem that now
#     lives under a harness_<subsystem>/ module dir. ---
_s7_bad_root="$(mktemp -d)"; _TMPDIRS+=("$_s7_bad_root")
mkdir -p "$_s7_bad_root/crates/synthcrate/tests/harness_synth" "$_s7_bad_root/cfg"
printf '#[test]\nfn t() {}\n' > "$_s7_bad_root/crates/synthcrate/tests/harness_synth/synthstem.rs"
printf "filter = 'package(synthcrate) & binary(synthstem)'\n" > "$_s7_bad_root/cfg/nextest.toml"
printf 'cargo test -p synthcrate --test synthstem\n'          > "$_s7_bad_root/cfg/run.sh"

_s7_bad_out="$(mktemp)"; _TMPDIRS+=("$_s7_bad_out")
_s7_bad_rc=0
harness_stale_selector_violations "$_s7_bad_root" "$_s7_bad_root/cfg" \
    > "$_s7_bad_out" 2>/dev/null || _s7_bad_rc=$?

assert "7: a binary()/--test selector naming a consolidated stem fires (returns 1)" \
    test "$_s7_bad_rc" -eq 1
assert "7: stale binary() selector emitted as a structured FAIL line (stale-selector)" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL crate=- file=.*nextest\.toml reason=stale-selector stem=synthstem$' "$_s7_bad_out"
assert "7: stale --test target selector is caught too (both decidable atom shapes)" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL crate=- file=.*run\.sh reason=stale-selector stem=synthstem$' "$_s7_bad_out"

# --- must-not-fire: a selector naming a stem that is still a genuine top-level
#     standalone binary (an override, I1) must NEVER be flagged. ---
_s7_ok_root="$(mktemp -d)"; _TMPDIRS+=("$_s7_ok_root")
mkdir -p "$_s7_ok_root/crates/synthcrate/tests/harness_synth" "$_s7_ok_root/cfg"
printf '#[test]\nfn t() {}\n' > "$_s7_ok_root/crates/synthcrate/tests/harness_synth/synthstem.rs"
printf '#[test]\nfn t() {}\n' > "$_s7_ok_root/crates/synthcrate/tests/synthoverride.rs"
printf "filter = 'package(synthcrate) & binary(synthoverride)'\n" > "$_s7_ok_root/cfg/nextest.toml"

_s7_ok_out="$(mktemp)"; _TMPDIRS+=("$_s7_ok_out")
_s7_ok_rc=0
harness_stale_selector_violations "$_s7_ok_root" "$_s7_ok_root/cfg" \
    > "$_s7_ok_out" 2>/dev/null || _s7_ok_rc=$?

assert "7: a selector naming a still-standalone override binary does NOT fire (returns 0, I1 precision)" \
    test "$_s7_ok_rc" -eq 0
assert "7: clean selector scan emits a structured PASS line carrying the stem count" \
    grep -Eq '^HARNESS_KLOC_CAP PASS scan=selectors stems=1$' "$_s7_ok_out"

# --- LIVE scan over the real selector surfaces. ---
_s7_live_out="$(mktemp)"; _TMPDIRS+=("$_s7_live_out")
_s7_live_rc=0
harness_stale_selector_violations "$REPO_ROOT" \
    "$REPO_ROOT/.config/nextest.toml" "$REPO_ROOT/scripts" "$REPO_ROOT/tests/infra" \
    > "$_s7_live_out" 2>/dev/null || _s7_live_rc=$?

if [ "$_s7_live_rc" -ne 0 ]; then
    echo "  ---- Section 7: live selector scan output (printed on failure) ----"
    cat "$_s7_live_out"
    echo "  ---- Section 7: end live scan output ----"
fi

assert "7: no live repo-resident selector names a pre-consolidation id (BT-2, rc 0)" \
    test "$_s7_live_rc" -eq 0

# Non-vacuity: the stem set must be genuinely populated, or the scan above
# would pass by having nothing to compare against. Counted INDEPENDENTLY off
# the tree rather than parsed out of the detector's own PASS line — a detector
# regression must surface as ONE failure here, not two (the Section 5b rule).
_s7_live_stems=0
for _d in "$REPO_ROOT"/crates/*/tests/harness_*/; do
    [ -d "$_d" ] || continue
    _s7_live_stems=$((_s7_live_stems + $(find "$_d" -type f -name '*.rs' | wc -l)))
done
assert "7: live consolidated-stem set is non-empty (>= 300 stems, non-vacuity)" \
    test "$_s7_live_stems" -ge 300

test_summary
