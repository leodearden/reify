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
# harness_selective_demand does the same for `common/differential.rs`). The
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
#       contradict the ratified §3 W1/§7 band. Parking the overflow in one of
#       the 7 override binaries instead is likewise refused — see the clause
#       beside `_HL_OVERRIDE_STEMS` in harness-layout-lib.sh (task #6461).
#
#       SUBJECT: harness compile units ONLY. Every OTHER top-level
#       `tests/*.rs` in these 5 crates — the 7 override binaries and the 348
#       grandfathered baseline standalones — is UNCAPPED BY DESIGN, not by
#       omission: the cap is a consolidation-SIZING parameter, and neither is
#       a consolidation product. Rationale and measured headroom: the clause
#       beside `_HL_OVERRIDE_STEMS` in harness-layout-lib.sh (task #7004).
#       Pinned non-vacuously by Section 3c below.
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
#           HARNESS_KLOC_CAP FAIL crate=<c> file=<path> reason=undeclared-member member=<harness_sub/file.rs>
#           HARNESS_KLOC_CAP PASS crate=<c> scan=undeclared-members roots=<n> members=<n>
#       (the last two lines are rule (d)'s, APPENDED here rather than inserted
#       among the four above, so existing unanchored consumers keep matching —
#       the same append-don't-insert discipline this rule list already states
#       for the external breakdown fields, below.)
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
#   (d) NO UNDECLARED MEMBER. Rule (c)'s converse direction, not a fourth
#       independent axis. A `.rs` file physically present under a harness
#       root's own `crates/<c>/tests/harness_<subsystem>/` module directory,
#       with no `mod` declaration reachable anywhere in the root's
#       transitive mod-graph, is never compiled by rustc: its `#[test]` fns
#       vanish silently — no compile error, no link error, no test-count
#       signal — and the tree stays green with strictly LESS coverage than
#       a reader of the source tree would assume. Section 6 below walks
#       declaration -> file (does every bare `mod` resolve correctly); rule
#       (d) walks the CONVERSE direction, file -> declaration (does every
#       file under the module dir have a reachable declaration at all).
#       Neither direction subsumes the other: Section 6's own principled
#       exception — a bare `mod x;` that correctly resolves to a retained
#       `tests/` sibling — is EXACTLY where a module-dir file of the same
#       stem (`tests/harness_<subsystem>/x.rs`, left behind mid-move) goes
#       silently dead, because rustc's crate-root-relative resolution binds
#       the sibling and never looks at the module-dir file at all. This
#       mechanizes a hazard already named in a commit message —
#       46a65f5181: "an unregistered file is never compiled, so its tests
#       would be silently absent... a vacuous pass" — but never before
#       enforced by a guard.
#
#       REMEDY for a genuine FAIL here, so a developer who hits one is not
#       left choosing between "delete the file" and reverse-engineering the
#       parser: either (i) DELETE the orphaned file, if it should never have
#       been compiled (a stale leftover from a move); or (ii) DECLARE it —
#           #[cfg(any())]
#           #[path = "harness_<subsystem>/<file>.rs"]
#           mod <file>;
#       — which this rule accepts as declared regardless of cfg state, since
#       the shared parser (_harness_layout_mod_decls) does not evaluate
#       `#[cfg(...)]` at all (Section 10's must-not-fire fixture for a
#       `#[cfg(target_os = "linux")]`-gated member pins exactly this). Use
#       (ii) for a file that must stay in the module dir but is deliberately
#       never compiled (e.g. a colocated fixture meant to be read via
#       `include_str!`, not `mod`-ed in) — a permanently-false `cfg(any())`
#       is deliberate over some other silent exclusion: it keeps the file's
#       existence at least VISIBLE to a reader of the root, rather than
#       structurally invisible to both this rule and rustc.
#
#       LANDING PRECONDITION. Like Sections 6-9, rule (d) is a WHOLE-TREE
#       LIVE scan, not diff-scoped — unlike
#       scripts/check-harness-baseline-registration.sh's rule (b) analogue,
#       which is diff-scoped precisely because a whole-tree live scan
#       re-fires on every innocent downstream rebaser once such drift is on
#       main (the 5260/5266/5288 thrash scripts/verify.sh's own 5300-gate
#       comment records). That tradeoff is only sound because the
#       precondition was actually MEASURED, not assumed: on 4a9f2d6d4c the
#       live tree held 31 harness roots and 585 module-dir member files
#       across the 5 consolidatable crates, with 0 undeclared and every
#       module dir flat (no nested subdirs). Should such drift ever land on
#       main regardless, this whole-tree scan will re-fire on every
#       innocent rebaser until it is fixed — the remedy there is to fix the
#       drift, not to loosen this rule.
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
    # re-accretion violations. Reaching the `continue` below via this set
    # also means rule (a)'s cap arm above was never reached for this file at
    # all (its base didn't match `harness_*.rs)`) — so an override stem is
    # never cap-checked, not merely found under threshold. That is a
    # deliberate exemption (task #7004; rationale beside `_HL_OVERRIDE_STEMS`
    # in harness-layout-lib.sh), pinned non-vacuously by Section 3c below.
    # Build the lookup set once per call.
    local -A _override_set=()
    local _ov
    for _ov in "${OVERRIDE_BINARIES[@]}"; do
        _override_set["$_ov"]=1
    done

    for f in "$tests_dir"/*.rs; do
        [ -f "$f" ] || continue          # skip a literal no-match glob

        base="$(basename "$f")"

        # rule (a): kLOC cap governs the harness_<subsystem>.rs compile units.
        # The `harness_*.rs)` pattern below is the cap's ONLY gate: a file
        # whose base does not match it skips this arm entirely and is never
        # measured against cap_lines, however large it is. The arm's own
        # `continue` then confines rule (b) below to non-harness files — a
        # harness root, once cap-checked here, is never also re-accretion-
        # checked. Section 3c pins the arm's complement non-vacuously: an
        # over-cap OVERRIDE stem, which never reaches this arm, contributes
        # zero cap violations in the same scan where an over-cap harness
        # fires.
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
# A THIN FILTER over the shared `_harness_layout_mod_decls` parser in
# harness-layout-lib.sh (which emits `<kind>|<lineno>|<value>` for BOTH kinds),
# not a second copy of it. The attribute-binding rule — blank lines and `//`
# comments preserve a pending `#[path]`, any other line clears it, and only the
# `mod <ident>;` form counts — is non-trivial and is what makes this C1 mandate
# and the C2 kLOC measure agree about which declarations are `#[path]`-covered.
# Two copies of it would desynchronise silently on the first fix applied to
# either (block comments, `#[cfg_attr]`, `#[cfg(…)] mod x;`), so there is one.
#
# No `grep -q`-style early close on the pipeline: awk drains its input to
# completion, so the upstream parser never takes a SIGPIPE under this script's
# `set -o pipefail` (the esc-5172-1 hazard the lib documents).
# ---------------------------------------------------------------------------
_bare_mod_decls() {
    # Shared parser emits `<file>|<kind>|<lineno>|<value>`; this detector is
    # single-file and reports `<lineno>|<ident>`, so drop the file and the kind.
    _harness_layout_mod_decls "$1" \
        | awk -F'|' '$2 == "bare" { print $3 "|" $4 }'
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
# The PARSER half of this predicate is already hoisted: `_bare_mod_decls` above
# is a filter over harness-layout-lib.sh's `_harness_layout_mod_decls`, so the
# attribute-binding rule is shared with the C2 kLOC measure (task #5620).
#
# NOTE (deferred, out of this task's lock scope): the RESOLUTION half — this
# function's retained-sibling test — is still local. The sibling diff-scoped
# gate scripts/check-harness-baseline-registration.sh states the same `#[path]`
# rule in human-facing guidance text only; making both guards execute one copy
# (the G7 no-lockstep-duplication shape the lib exists for) needs a write to
# that sibling script, which this task does not hold — filed as follow-up.
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
# harness_undeclared_member_violations <crate> <tests_dir>
#
# Rule (d) — NO UNDECLARED MEMBER. The CONVERSE of harness_path_attr_violations
# above: that detector walks declaration -> file (does every bare `mod`
# resolve correctly); this one walks file -> declaration (does every file
# physically present under a harness root's own module dir have a reachable
# `mod` declaration anywhere in the root's mod-graph at all). A `.rs` file
# under `crates/<c>/tests/harness_<subsystem>/` with no reachable declaration
# is never compiled by rustc, so its `#[test]` fns vanish silently — no
# compile error, no link error, no test-count signal. Neither direction
# subsumes the other.
#
# WHY THIS IS A GUARD AND NOT A COMMENT (mirrors harness_path_attr_violations'
# own header framing). harness_path_attr_violations walks declarations -> files
# and, BY DESIGN, passes a bare `mod x;` whose crate-root-relative resolution
# lands on a retained `tests/` sibling (Section 6's principled exception) —
# that is correct FOR THAT DETECTOR'S QUESTION. But it means a module-dir file
# of the SAME STEM (`tests/harness_<subsystem>/x.rs`, left behind mid-move, or
# never removed) is structurally INVISIBLE to it: rustc also resolves the bare
# `mod x;` to the sibling, so the module-dir file is silently never compiled —
# no compile error, no link error, no test-count signal, exactly the silent
# mode that makes prose alone insufficient here too.
#
# For every <tests_dir>/harness_<subsystem>.rs root that has a module dir
# (${root%.rs}; a single-file harness with no module dir has nothing to
# scan), compares every *.rs file found under that module dir at any depth
# against harness-layout-lib.sh's harness_layout_declared_members, and flags
# any file absent from that set. Both sides of the comparison are run through
# `_harness_layout_norm_path` — the declared set already comes normalized
# (harness_layout_declared_members' contract), and the `find` result and the
# <tests_dir> prefix used to derive `member=` are normalized here — so a
# `#[path]` value written as `./harness_sub/a.rs` or a `..`-round-trip cannot
# compare unequal to its canonical form and false-fire (the same moddir-prefix
# hazard harness-layout-lib.sh's header calls out). `member=` is always the
# canonical, <tests_dir>-relative form (`harness_sub/a.rs`) a developer can
# paste straight into `#[path = "…"]`, never a `./`- or `..`-bearing echo of
# whatever form the caller's <tests_dir> or the file's declaration happened to
# use.
#
# Prints one structured FAIL line per violation and returns 1 if <crate> has
# any. On a clean crate emits ONE aggregate `PASS crate=<c>
# scan=undeclared-members roots=<n> members=<n>` line and returns 0 — the
# SAME `crate=<c>` field Section 6's bare per-crate `PASS crate=<c>` carries
# (task #7042 amendment: an earlier cut of this rule dropped it, which left
# the archived merge-verify log with N indistinguishable
# `scan=undeclared-members` lines per run and no way to tell which crate
# contributed which counts — exactly the log-forensics use case the counts
# below exist for), PLUS the counts Section 6 has no equivalent of
# (harness_path_attr_violations reports no comparable per-root/per-file
# totals). Here the member count is this rule's own non-vacuity witness: a
# detector that visited every root but silently compared zero member files
# (a broken find glob, a moddir guard that always `continue`s, an over-broad
# declared set) would otherwise return 0 just as vacuously as the
# silent-coverage-loss failure mode this whole rule exists to close, one
# level up — so the count is emitted into the structured grammar (and hence
# the archived merge-verify log) rather than left as an internal bash
# variable a future reader would have to re-derive. `roots=` counts every
# `harness_*.rs` glob match in <tests_dir> (including a single-file harness
# with no module dir, which contributes 0 to `members=`); `members=` counts
# every *.rs file compared under a module dir at any depth, declared or not.
# Parameterized on <tests_dir> so hermetic fixtures drive it exactly as live.
#
# KNOWN LIMITATION (documented in harness-layout-lib.sh's
# _harness_layout_walk_unit header, not fixed here): the shared walk this
# rule's declared-member set is built from resolves a decl by the declaring
# FILE's directory only, with no awareness of inline `mod outer { ... }`
# block nesting — a decl textually inside such a block reads as declared
# even where rustc would resolve it under a subdirectory this walk never
# looks in. No live harness root uses that shape today, so this is a
# documented latent gap, not a current miscount.
# ---------------------------------------------------------------------------
harness_undeclared_member_violations() {
    local crate="$1"
    local tests_dir="$2"

    local violations=0 roots=0 members=0
    local root moddir f member decl tests_dir_norm

    if [ ! -d "$tests_dir" ]; then
        _emit FAIL "crate=$crate" "dir=$tests_dir" "reason=missing-tests-dir"
        return 1
    fi

    _harness_layout_norm_path "$tests_dir"; tests_dir_norm="$_HL_NORM_OUT"

    for root in "$tests_dir"/harness_*.rs; do
        [ -f "$root" ] || continue          # skip a literal no-match glob
        roots=$((roots + 1))
        moddir="${root%.rs}"
        [ -d "$moddir" ] || continue        # single-file harness: nothing to scan

        # Rebuilt fresh per root: membership is root-scoped, not crate-scoped.
        # Keys are already normalized (harness_layout_declared_members' own
        # contract), so only the `find` side below needs normalizing to match.
        local -A _declared=()
        while IFS= read -r decl; do
            [ -n "$decl" ] || continue
            _declared["$decl"]=1
        done < <(harness_layout_declared_members "$root")

        # Process substitution (not a pipe) draining to completion — the same
        # esc-5172-1 SIGPIPE-141 posture harness_layout_unit_lines documents.
        while IFS= read -r -d '' f; do
            _harness_layout_norm_path "$f"
            f="$_HL_NORM_OUT"
            members=$((members + 1))
            [ -n "${_declared["$f"]:-}" ] && continue
            member="${f#"$tests_dir_norm"/}"
            _emit FAIL "crate=$crate" "file=$root" "reason=undeclared-member" "member=$member"
            violations=$((violations + 1))
        done < <(find "$moddir" -type f -name '*.rs' -print0)
    done

    if [ "$violations" -gt 0 ]; then
        return 1
    fi
    _emit PASS "crate=$crate" "scan=undeclared-members" "roots=$roots" "members=$members"
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

# ---------------------------------------------------------------------------
# _harness_layout_row_exists <row> <root_dir> — predicate for
# harness_layout_orphan_rows: exit 0 iff <row> names a file that EXISTS under
# <root_dir> (rule (b)'s own on-disk enumeration, in reverse).
# ---------------------------------------------------------------------------
_harness_layout_row_exists() {
    [ -e "${2}/${1}" ]
}

# ---------------------------------------------------------------------------
# _harness_layout_row_in_scope <row> <_unused> — predicate for
# harness_layout_malformed_rows: exit 0 iff <row> is a well-formed in-scope
# standalone (harness_layout_in_scope_standalone, harness-layout-lib.sh) —
# pure string, no disk access. Takes the same <row> <arg> shape as
# _harness_layout_row_exists so _harness_layout_scan_baseline_rows below can
# dispatch to either predicate identically; the second argument is simply
# unused here.
# ---------------------------------------------------------------------------
_harness_layout_row_in_scope() {
    harness_layout_in_scope_standalone "$1"
}

# ---------------------------------------------------------------------------
# _harness_layout_scan_baseline_rows <scan_token> <reason_token>
#                                     <baseline_file> <predicate_fn> [predicate_arg]
#
# Shared row-scanning skeleton for harness_layout_orphan_rows and
# harness_layout_malformed_rows below. The two started as near-verbatim
# copies of this scaffolding — same missing/unreadable-baseline guards, same
# herestring loop, same violation tally, same crate-derivation-and-emit shape
# — differing only in what makes a row a violation and in two literal
# tokens (task #5983 review: nothing enforced that a fix to one copy, e.g.
# tightening the unreadable-baseline check, would not silently leave the
# other behind). Hoisted here so there is exactly one place left to get it
# right; each caller below is now a thin wrapper supplying its own predicate
# and tokens.
#
# <predicate_fn> is invoked as `"$predicate_fn" <row> <predicate_arg>` — a
# function name carried in a variable and invoked via `"$@"`-style expansion,
# the same idiom test_helpers.sh's assert() already uses for every check in
# this suite — and must return 0 for a well-formed row, 1 for a violation.
# <predicate_arg> is passed through unexamined, empty when the predicate does
# not need one (_harness_layout_row_in_scope).
#
# A missing baseline is an explicit FAIL, never a silent pass — the same
# graceful-degradation posture harness_layout_violations takes for a missing
# tests dir. (A vacuous pass here would be the worst outcome: the guard would
# report clean precisely when it can see nothing.) A baseline that EXISTS but
# cannot be READ is likewise an explicit FAIL, via a distinct
# reason=unreadable-baseline token, checked on the enumerator's own exit
# status: `rows -eq 0` is NOT a usable proxy for it, because a baseline
# holding only its header comment is the legitimate END STATE of a ratchet
# that shrinks toward empty — treating zero rows as an error would RED the
# merge gate as a reward for FINISHING the consolidation this guard protects
# (the same landmine the Section 8/9 notes decline a `rows >= N` floor over).
#
# The loop consumes a HERESTRING over the already-materialized rows, NOT a
# pipe: it must not early-close the grep inside harness_layout_baseline_rows
# (the esc-5172-1 SIGPIPE-141 hazard under this script's `set -o pipefail`),
# and the violation tally must survive in THIS shell rather than dying in a
# pipeline subshell. A herestring is used, rather than the process
# substitution harness_layout_unit_lines and _bare_mod_decls use elsewhere,
# because the enumerator's exit status is needed above, which a process
# substitution would discard. `<<< ""` yields ONE empty line, which the
# `[ -n "$row" ]` guard drops — so an empty baseline counts zero rows, not
# one.
#
# Prints one structured FAIL line per violation — crate derived via the
# shared _harness_layout_row_crate, so callers cannot drift on what `crate=`
# means for a non-`crates/` row — and returns 1 on any. On a clean scan,
# prints one PASS line carrying the scanned row count and returns 0.
# ---------------------------------------------------------------------------
_harness_layout_scan_baseline_rows() {
    local scan_token="$1" reason_token="$2" baseline_file="$3" predicate_fn="$4"
    local predicate_arg="${5:-}"

    if [ ! -f "$baseline_file" ]; then
        _emit FAIL "baseline=$baseline_file" "reason=missing-baseline"
        return 1
    fi

    local rows_raw="" rows_rc=0
    rows_raw="$(harness_layout_baseline_rows "$baseline_file")" || rows_rc=$?
    if [ "$rows_rc" -ne 0 ]; then
        _emit FAIL "baseline=$baseline_file" "reason=unreadable-baseline"
        return 1
    fi

    local violations=0 rows=0
    local row crate

    while IFS= read -r row; do
        [ -n "$row" ] || continue
        rows=$((rows + 1))
        if "$predicate_fn" "$row" "$predicate_arg"; then
            continue
        fi
        _harness_layout_row_crate "$row"
        crate="$_HL_ROW_CRATE"
        _emit FAIL "crate=$crate" "file=$row" "reason=$reason_token"
        violations=$((violations + 1))
    done <<< "$rows_raw"

    if [ "$violations" -gt 0 ]; then
        return 1
    fi
    _emit PASS "scan=$scan_token" "rows=$rows"
    return 0
}

# ---------------------------------------------------------------------------
# harness_layout_orphan_rows <root_dir> <baseline_file>
#
# The CONVERSE of rule (b): every data row of the grandfather baseline must name
# a file that EXISTS under <root_dir>. A row whose file is gone is an ORPHAN —
# it grandfathers nothing, and it makes the manifest header's regenerate-and-diff
# recipe report a mismatch that belongs to nobody's diff.
#
# WHY THIS DIRECTION NEEDS ITS OWN DETECTOR — neither existing check can see it:
#   - harness_layout_violations (rule (b), above) iterates `"$tests_dir"/*.rs`.
#     It enumerates files that EXIST and asks "is this one sanctioned?". A row
#     with no file is never visited, so rule (b) is one-directional BY
#     CONSTRUCTION, not by oversight.
#   - scripts/check-harness-baseline-registration.sh is DIFF-SCOPED: it only
#     ever considers files ADDED in the diff under review, and skips any
#     candidate failing its own `[ -e "$path" ]`. A row that no diff touches is
#     never examined.
#
# The concrete witness that this is not hypothetical (task #5938, measured):
# the `crates/reify-eval/tests/concurrent.rs` row was manufactured by MERGING
# two individually-green branches. df2911dff4 (#5265 pre-2) generated the
# baseline on a base where that file still existed, while ffb85f0627 (#5065
# task ο step-4) deleted the file on a parallel branch; neither is an ancestor
# of the other, and neither diff both deletes the file and leaves the row. Only
# a WHOLE-TREE scan can catch that drift class — a diff-scoped gate structurally
# cannot, because the offending state exists in no single diff.
#
# The predicate is on-disk existence `[ -e "$root_dir/$row" ]`, not `git
# ls-files` tracking: it is the exact converse of rule (b)'s own on-disk
# enumeration, it keeps this `pool`-classified guard free of a git dependency,
# and it is drivable from a hermetic mktemp fixture that is not a git repo.
# scripts/check-harness-baseline-registration.sh takes the same `[ -e ]`
# posture. Directly parallels the `classification_orphaned` stale-row accessor
# (tests/infra/run-all-classification-lib.sh) that already solves this same
# "manifest row whose file no longer exists" problem for the sibling
# run-all-classification.manifest — so this is a parity fix, not a novel rule.
#
# RESIDUAL, stated so the next reader does not assume parity: the manifest
# header's regenerate-and-diff recipe enumerates with `git ls-files`, and this
# detector tests the WORKING TREE. The two answers differ for a row naming a
# file that is present on disk but UNTRACKED (or `git rm --cached`-ed and left
# in the tree) — this scan passes it, regenerate-and-diff still reports it. That
# is the deliberate direction of the gap: a stray untracked file is transient
# developer state and must not RED the merge gate, and the lanes the gate
# actually runs in are seeded tracked-files-only + `git clean -xfd`, where the
# two predicates coincide. The gap is one-sided — a row naming a file that is
# neither tracked NOR on disk (the drift class this exists for) is caught by
# both.
#
# Prints one structured FAIL line per orphan; returns 1 on any. <root_dir> and
# <baseline_file> are both parameterized so hermetic fixtures drive it exactly
# as the live tree does. Thin wrapper over _harness_layout_scan_baseline_rows
# above, supplying the on-disk-existence predicate and this detector's own
# scan=/reason= tokens.
# ---------------------------------------------------------------------------
harness_layout_orphan_rows() {
    local root_dir="$1"
    local baseline_file="$2"
    _harness_layout_scan_baseline_rows "baseline-rows" "orphan-baseline-row" \
        "$baseline_file" _harness_layout_row_exists "$root_dir"
}

# ---------------------------------------------------------------------------
# harness_layout_malformed_rows <baseline_file>
#
# Every data row of <baseline_file> must itself be a well-formed in-scope
# standalone (harness_layout_in_scope_standalone) — independent of whether the
# file it names exists on disk, which is the property
# harness_layout_orphan_rows above does NOT check (that detector asks only
# "does this row's file exist?"). A row that is malformed but whose file
# EXISTS (e.g. `crates/reify-eval/src/foo.rs`, or a nested
# `crates/reify-eval/tests/sub/dir/x.rs`) passes harness_layout_orphan_rows
# silently and is structurally unreachable from rule (b)'s detector
# (harness_layout_violations), which enumerates existing FILES, not baseline
# ROWS — so it would otherwise drift into the ratchet as dead weight.
#
# "Well-formed" is deliberately harness_layout_in_scope_standalone
# (harness-layout-lib.sh), NOT a locally re-derived
# `^crates/[^/]+/tests/[^/]+\.rs$`-style regex: that predicate already
# single-sources exactly this rule — it IS the manifest header's own "WHAT IT
# LISTS" / "INTENTIONALLY EXCLUDED" claims (the 5 consolidatable crates,
# top-level only, no harness_*.rs, none of the 7 override stems) — so
# re-deriving it here would be a second, weaker expression of the same rule
# (the G7 no-lockstep-duplication concern that lib's own header exists to
# prevent). The predicate is pure-string (no disk access), which is why this
# detector — unlike harness_layout_orphan_rows — takes NO <root_dir> argument.
#
# Prints one structured FAIL line per malformed row; returns 1 on any.
# <baseline_file> is parameterized so hermetic fixtures drive it exactly as
# the live tree does. Thin wrapper over _harness_layout_scan_baseline_rows
# above, supplying the in-scope-standalone predicate and this detector's own
# scan=/reason= tokens.
# ---------------------------------------------------------------------------
harness_layout_malformed_rows() {
    local baseline_file="$1"
    _harness_layout_scan_baseline_rows "baseline-row-shape" "malformed-baseline-row" \
        "$baseline_file" _harness_layout_row_in_scope
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
#
# Rule (d) (Section 10 below) preserves this pin rather than conflicting
# with it: both harness_layout_violations here and
# harness_undeclared_member_violations loop `for root in
# "$tests_dir"/harness_*.rs`, so an orphan dir has no root and is never
# visited by EITHER rule — there is nothing for rule (d) to compare an
# orphan dir's files against, and reporting them undeclared would just be
# this same silent-ignore behavior restated as a false positive.
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
# pin ALL THREE of rustc's resolution rules (`#[path]`-relative; bare `mod` in
# the crate root / a `mod.rs`, against the containing dir; bare `mod` elsewhere,
# against the file's STEM dir) in BOTH attribute spellings (attribute on its own
# line, and the one-line `#[path = "…"] mod x;`), plus transitivity, visited-set
# dedup, the unresolvable-target case, and the no-double-counting boundary
# against the existing module-dir `find` walk.
#
# Each assert is an arithmetic equality over the whole tuple, so a resolution
# regression cannot hide: it either drops lines or (with the Fixture E decoys)
# binds the wrong file, and both move the total.
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

# --- Fixture D: the SINGLE-LINE `#[path = "…"] mod x;` form is attributed, and
#     it CONSUMES its own attribute — a bare `mod` on the next line must still
#     resolve as bare. If the pending attribute leaked, `mod sibling;` would be
#     re-resolved to the already-visited one_line.rs, dedup would drop it, and
#     sibling.rs's 5 lines would vanish from the measure. ---
_s1e4_dir="$(mktemp -d)"; _TMPDIRS+=("$_s1e4_dir")
mkdir -p "$_s1e4_dir/shared"
{
    printf '#[path = "shared/one_line.rs"] mod one_line;\n'
    printf 'mod sibling;\n'
} > "$_s1e4_dir/harness_oneline.rs"                                # root: 2 lines
awk 'BEGIN { for (i = 0; i < 11; i++) print "// x" }' > "$_s1e4_dir/shared/one_line.rs"
awk 'BEGIN { for (i = 0; i <  5; i++) print "// x" }' > "$_s1e4_dir/sibling.rs"

_s1e4_tuple="$(harness_layout_unit_lines "$_s1e4_dir/harness_oneline.rs")"
assert "1e: a single-line \`#[path = \"…\"] mod x;\` is attributed AND consumes its own attribute, so the next bare \`mod\` still resolves as bare (total=18 root=2 module=0 files=0 external=16 extfiles=2)" \
    test "$_s1e4_tuple" = "18 2 0 0 16 2"

# --- Fixture E: rustc's THIRD resolution rule — a bare `mod X;` in a module
#     file that is neither the crate root nor a `mod.rs` resolves against that
#     file's STEM directory (`${F%.rs}/X.rs`, else `${F%.rs}/X/mod.rs`), not
#     against its containing directory. Both candidate forms are exercised.
#
#     The two DECOYS are what make this discriminating rather than merely
#     present: they sit exactly where a regression that collapsed the stem-dir
#     branch to the containing dir would look. Without them such a regression
#     would only DROP the includes; with them it silently binds the WRONG files
#     (101+103 instead of 23+29) — the same silent-mis-bind failure mode the C1
#     `#[path]` mandate exists to prevent, now caught by an arithmetic assert. ---
_s1e5_dir="$(mktemp -d)"; _TMPDIRS+=("$_s1e5_dir")
mkdir -p "$_s1e5_dir/shared/outer/other" "$_s1e5_dir/shared/other"
{
    printf '#[path = "shared/outer.rs"]\n'
    printf 'mod outer;\n'
} > "$_s1e5_dir/harness_stem.rs"                                   # root: 2 lines
{
    printf 'mod helper;\n'                     # -> shared/outer/helper.rs
    printf 'mod other;\n'                      # -> shared/outer/other/mod.rs
} > "$_s1e5_dir/shared/outer.rs"                                   # 2 lines
awk 'BEGIN { for (i = 0; i < 23; i++) print "// x" }' > "$_s1e5_dir/shared/outer/helper.rs"
awk 'BEGIN { for (i = 0; i < 29; i++) print "// x" }' > "$_s1e5_dir/shared/outer/other/mod.rs"
# Decoys: reachable ONLY by a containing-dir (wrong) resolution. Never counted.
awk 'BEGIN { for (i = 0; i < 101; i++) print "// x" }' > "$_s1e5_dir/shared/helper.rs"
awk 'BEGIN { for (i = 0; i < 103; i++) print "// x" }' > "$_s1e5_dir/shared/other/mod.rs"

_s1e5_tuple="$(harness_layout_unit_lines "$_s1e5_dir/harness_stem.rs")"
assert "1e: a bare \`mod\` in a non-root non-mod.rs file resolves against the file's STEM dir, in both the .rs and the dir/mod.rs form, and does not bind the containing-dir decoys (total=56 root=2 module=0 files=0 external=54 extfiles=3)" \
    test "$_s1e5_tuple" = "56 2 0 0 54 3"

# --- Regression (task #7042 amendment): `_HL_WALK_VISITED` (declared -gA
# inside `_harness_layout_walk_unit`) is RESET at the top of every call — "a
# second call must not inherit the first's visited set" per that function's
# own header. Nothing exercised that reset until now: dropping the `=()`
# would make a second same-shell call on a root that shares an external
# include skip the already-visited file and undercount it, and every fixture
# above either lives in its own tmpdir (no shared path to leak across) or is
# invoked via `$(…)` (which forks a SUBSHELL, so any leaked-or-reset state
# never escapes back to this shell to be observed).
#
# Two roots in ONE dir both `#[path]`-include the SAME external file, called
# via plain output REDIRECTION (`>`), not `$(…)`: a redirected shell-function
# call runs in THIS shell (no fork), so `_HL_WALK_VISITED`'s global state
# genuinely persists — or, if the reset regressed, leaks — between the two
# calls exactly as it would across two `harness_layout_unit_lines` calls in a
# real caller's loop (e.g. run_harness_layout_scan's per-crate loop, or
# Section 5b's _s5bc_scan). ---
_s1e6_dir="$(mktemp -d)"; _TMPDIRS+=("$_s1e6_dir")
mkdir -p "$_s1e6_dir/common"
{
    printf '#[path = "common/shared.rs"]\n'
    printf 'mod shared;\n'
} > "$_s1e6_dir/harness_alpha.rs"                                 # root: 2 lines
{
    printf '#[path = "common/shared.rs"]\n'
    printf 'mod shared;\n'
} > "$_s1e6_dir/harness_beta.rs"                                  # root: 2 lines
awk 'BEGIN { for (i = 0; i < 9; i++) print "// x" }' > "$_s1e6_dir/common/shared.rs"

_s1e6_out1="$(mktemp)"; _TMPDIRS+=("$_s1e6_out1")
_s1e6_out2="$(mktemp)"; _TMPDIRS+=("$_s1e6_out2")
harness_layout_unit_lines "$_s1e6_dir/harness_alpha.rs" > "$_s1e6_out1"
harness_layout_unit_lines "$_s1e6_dir/harness_beta.rs" > "$_s1e6_out2"
_s1e6_tuple1="$(cat "$_s1e6_out1")"
_s1e6_tuple2="$(cat "$_s1e6_out2")"

assert "1e: first same-shell call reports the shared external include (total=11 root=2 module=0 files=0 external=9 extfiles=1)" \
    test "$_s1e6_tuple1" = "11 2 0 0 9 1"
assert "1e: a SECOND same-shell call on a different root sharing the same external include still reports it — _HL_WALK_VISITED did not leak across calls (total=11 root=2 module=0 files=0 external=9 extfiles=1)" \
    test "$_s1e6_tuple2" = "11 2 0 0 9 1"

# --- Coherence: the total is exactly root + module + external in every case. ---
_s1e_total_coherent() {
    local tuple total root mod files ext extfiles
    for tuple in "$_s1e_tuple" "$_s1e2_tuple" "$_s1e3_tuple" "$_s1e4_tuple" "$_s1e5_tuple" "$_s1e6_tuple1" "$_s1e6_tuple2"; do
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
# Section 3c: rule (a)'s override exemption, pinned NON-VACUOUSLY — an
# over-cap OVERRIDE stem contributes ZERO violations in the SAME scan where an
# over-cap harness DOES fire (task #7004). rule (a)'s cap governs
# harness_<subsystem>.rs compile units ONLY; an override binary (I1) is exempt
# BY DESIGN — never a merge product, never a sanctioned cap-overflow
# destination (the clause beside `_HL_OVERRIDE_STEMS` in harness-layout-lib.sh,
# task #6461/#7004).
#
# WHY THIS SECTION EXISTS. Section 3's own override fixture (tensegrity_t0a.rs
# above) is a ONE-LINE file — vacuous against a 20000 cap, since a 1-line file
# passes whether or not rule (a) applies to override stems at all. Without a
# fixture that puts an override stem OVER the cap, silently extending rule (a)
# to the 7 stems would leave every assertion in this suite green. Section 3c
# closes that gap: it proves the cap machinery is live in this exact scan (the
# harness_over.rs assertion below), then proves the over-cap override is
# silent anyway — so the silence is attributable to the exemption, not to a
# dead scan or an under-sized fixture.
# ===========================================================================
echo ""
echo "--- Section 3c: an over-cap override stem is exempt from the kLOC cap (non-vacuous) ---"

_s3c_dir="$(mktemp -d)"; _TMPDIRS+=("$_s3c_dir")
_s3c_baseline="$(mktemp)"; _TMPDIRS+=("$_s3c_baseline")
: > "$_s3c_baseline"   # empty: rule (b) grandfathering cannot account for the
                       # override's silence below — only the I1 stem allow-list can.

# An over-cap harness root in the SAME dir/scan, so the cap machinery is
# demonstrably live at this size (non-vacuity witness for the override's
# silence below — not a restatement of Section 1's own pin).
awk 'BEGIN { for (i = 0; i < 21000; i++) print "// x" }' > "$_s3c_dir/harness_over.rs"
# An over-cap OVERRIDE stem. A different stem from Section 3's
# tensegrity_t0a.rs, so the two sections do not co-vary on one stem.
awk 'BEGIN { for (i = 0; i < 21000; i++) print "// x" }' > "$_s3c_dir/analytical_validation.rs"

_s3c_out="$(mktemp)"; _TMPDIRS+=("$_s3c_out")
_s3c_rc=0
harness_layout_violations synthcrate "$_s3c_dir" "$_s3c_baseline" 20000 \
    > "$_s3c_out" 2>/dev/null || _s3c_rc=$?

assert "3c: the crate's only violation is the over-cap harness (returns 1)" \
    test "$_s3c_rc" -eq 1
assert "3c: NON-VACUITY — the over-cap harness DOES fire (cap machinery is live at this size)" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL crate=synthcrate file=.*harness_over\.rs reason=exceeds-cap lines=21000 cap=20000' "$_s3c_out"
assert "3c: the over-cap override stem emits NO line naming it at all (exempt, not merely under threshold)" \
    bash -c '! grep -qE "analytical_validation" "$1"' _ "$_s3c_out"

# `|| true`: grep -c exits 1 on zero matches, which would otherwise abort this
# script under `set -e` before the assert below gets to judge pass/fail (the
# run_harness_layout_scan / Section 4 idiom, line ~424 / ~1458).
_s3c_fail_count="$(grep -cE '^HARNESS_KLOC_CAP FAIL ' "$_s3c_out" || true)"
assert "3c: EXACTLY ONE FAIL line total — the override contributed zero violations, not a differently-worded one" \
    test "$_s3c_fail_count" -eq 1

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
# (the headline user-observable signal). Also guard integrity: a missing
# baseline must never let the scan vacuously pass.
# ===========================================================================
echo ""
echo "--- Section 5: live scan of the real 5 consolidatable crates ---"

# Guard integrity: a MISSING baseline is never a silent pass (kept here as
# `test -f`, below). Non-emptiness is DELIBERATELY NOT asserted anymore —
# recorded here so a future reader is not tempted to put the floor back:
#
#   1. The manifest header calls this file "A RATCHET, NOT A PERMANENT
#      ALLOW-LIST" whose "shrinking toward empty == consolidation progress".
#      A non-empty floor would RED the merge gate as a REWARD for FINISHING
#      the consolidation (#5693-#5696, plus the reify-cli / reify-syntax /
#      reify-kernel-occt lines) that this guard exists to protect — invisible
#      today at ~495 rows, firing only at the finish line. Same reasoning as
#      Section 8's and Section 9's own declined `rows >= N` floors, and as
#      the sibling ptodo ratchet's crates/reify-audit/tests/ptodo_baseline.rs
#      :: baseline_is_well_formed, which states the identical rule for its own
#      baseline: "An empty baseline PASSES — it is the ... zero residual debt
#      success state, not a failure."
#   2. It was never load-bearing for THIS section's own vacuity anyway: the
#      live rule-(b) scan directly below is fail-CLOSED on an emptied baseline
#      BY CONSTRUCTION — every one of the ~495 grandfathered files would be
#      flagged as an unsanctioned standalone, a loud RED (see the asserts
#      below).
#   3. The properties the deleted assert used to carry — by accident, via its
#      `2>/dev/null || true` swallow, which made an UNREADABLE live baseline
#      masquerade as merely "empty" — now live elsewhere, deliberately:
#      baseline MISSING or UNREADABLE -> Section 8's live orphan-row scan
#      (`reason=missing-baseline` / `reason=unreadable-baseline`, over this
#      same $BASELINE) and Section 9's live row-shape scan (same two
#      reasons); row SHAPE / well-formedness -> Section 9. Section 9's live
#      well-formedness assert is intentionally NOT duplicated here (G7).
assert "5: grandfather baseline exists (guard integrity)" \
    test -f "$BASELINE"

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
#
# ONE live scan feeds BOTH this section and Section 5c: the two loops were
# byte-identical apart from their predicate, and each live unit measured costs
# real merge-gate wall clock in a PRD about cutting merge-gate CPU. The scan
# stops as soon as BOTH witnesses are in hand, so it never costs more than the
# earlier separate loops did — each of those also early-exited on its first
# witness, so the saving here is one unit's measure, not a whole traversal.
#
# The ASSERTS stay separate (one flag each): a module-dir-walk regression and an
# external-walk regression are different causes, and each must report as its own
# failure — the same one-cause-one-failure rule the NOTE below states.
_S5BC_MODULE_WIRED=0
_S5BC_EXTERNAL_WIRED=0
_s5bc_scan() {
    local c f tuple total root mod files ext extfiles
    for c in "${CONSOLIDATABLE_CRATES[@]}"; do
        for f in "$REPO_ROOT/crates/$c/tests"/harness_*.rs; do
            [ "$_S5BC_MODULE_WIRED" -eq 1 ] && [ "$_S5BC_EXTERNAL_WIRED" -eq 1 ] && return 0
            [ -f "$f" ] || continue
            tuple="$(harness_layout_unit_lines "$f" 2>/dev/null)" || continue
            read -r total root mod files ext extfiles <<<"$tuple"
            # Each fact gates on its OWN fields being numeric, exactly as the
            # two separate loops did — a garbled field must not suppress the
            # other witness.
            if [[ "$root" =~ ^[0-9]+$ ]] && [[ "$total" =~ ^[0-9]+$ ]] \
               && [ "$root" -lt 500 ] && [ "$total" -gt 10000 ]; then
                _S5BC_MODULE_WIRED=1
            fi
            if [[ "$ext" =~ ^[0-9]+$ ]] && [[ "$extfiles" =~ ^[0-9]+$ ]] \
               && [ "$extfiles" -gt 0 ] && [ "$ext" -gt 0 ]; then
                _S5BC_EXTERNAL_WIRED=1
            fi
        done
    done
    return 0
}
_s5bc_scan
assert "5b: at least one live harness has root<500 lines yet aggregate>10000 lines (module dir is actually read)" \
    test "$_S5BC_MODULE_WIRED" -eq 1

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
#
# The witness comes from `_s5bc_scan` under Section 5b — one live scan collects
# both non-vacuity facts. This assert reads its own flag, so an external-walk
# regression still fails HERE and alone.
# ===========================================================================
echo ""
echo "--- Section 5c: live non-vacuity of the external attribution ---"

assert "5c: at least one live harness attributes an out-of-module-dir include (external_files>0 — the walk is wired on the real tree)" \
    test "$_S5BC_EXTERNAL_WIRED" -eq 1

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
# crate is still covered. ONE pass feeds BOTH this section's #[path]-mandate
# check AND Section 10's undeclared-member check below (task #7042
# amendment, the Section 5b/5c pattern — see that pair for the precedent):
# the two loops were byte-identical over `crates/*/tests` apart from which
# detector they called and which output they accumulated into, so a single
# traversal now populates this section's $_s6_live_out/$_s6_live_rc AND
# Section 10's $_s10_live_out/$_s10_live_rc, plus the ONE shared
# $_s6s10_live_roots count both sections' non-vacuity asserts read — so the
# two counts can never silently drift apart. Section 10 below runs no loop
# of its own; it only asserts on the globals this loop sets. ---
_s6_live_out="$(mktemp)"; _TMPDIRS+=("$_s6_live_out")
_s6_live_rc=0
_s10_live_out="$(mktemp)"; _TMPDIRS+=("$_s10_live_out")
_s10_live_rc=0
_s6s10_live_roots=0
: > "$_s6_live_out"
: > "$_s10_live_out"
for _d in "$REPO_ROOT"/crates/*/tests; do
    [ -d "$_d" ] || continue
    compgen -G "$_d/harness_*.rs" > /dev/null || continue
    _s6s10_live_roots=$((_s6s10_live_roots + $(compgen -G "$_d/harness_*.rs" | wc -l)))
    _c="$(basename "$(dirname "$_d")")"
    harness_path_attr_violations "$_c" "$_d" >> "$_s6_live_out" 2>/dev/null || _s6_live_rc=1
    harness_undeclared_member_violations "$_c" "$_d" >> "$_s10_live_out" 2>/dev/null || _s10_live_rc=1
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
# future glob/layout change would silently turn the assert above into a
# no-op. Shared with Section 10's own roots assert below — see the fused-loop
# comment above.
assert "6: live scan actually visited harness roots (>= 13 found, non-vacuity)" \
    test "$_s6s10_live_roots" -ge 13

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

# ===========================================================================
# Section 8: the CONVERSE of rule (b) — every grandfather-baseline row must
# name a file that exists on disk. Rule (b)'s detector
# (harness_layout_violations) enumerates `"$tests_dir"/*.rs`, so it only ever
# visits files that EXIST and asks whether each is sanctioned; a row whose file
# is gone is structurally unreachable from it. The sibling gate
# scripts/check-harness-baseline-registration.sh is diff-scoped, so a row that
# no diff touches is never examined either. An orphan row therefore drifts in
# silently and only surfaces as a regenerate-and-diff mismatch belonging to no
# one's diff.
# ===========================================================================
echo ""
echo "--- Section 8: baseline orphan rows (row with no file on disk) ---"

# The shared fixture root. It carries TWO existing files (not one) because the
# must-not-fire case below pins `rows=2`, which needs two rows each backed by a
# real file; `gone.rs` is the one deliberately absent from it. A second on-disk
# file is inert for the must-fire case — the detector enumerates ROWS, not
# files, so a file with no row is simply never visited.
_s8_root="$(mktemp -d)"; _TMPDIRS+=("$_s8_root")
mkdir -p "$_s8_root/crates/reify-eval/tests"
printf 'fn main() {}\n' > "$_s8_root/crates/reify-eval/tests/present.rs"
printf 'fn main() {}\n' > "$_s8_root/crates/reify-eval/tests/present2.rs"

# --- must-fire: a baseline row naming a file that is NOT on disk. ---
_s8_fire_baseline="$(mktemp)"; _TMPDIRS+=("$_s8_fire_baseline")
printf 'crates/reify-eval/tests/present.rs\ncrates/reify-eval/tests/gone.rs\n' \
    > "$_s8_fire_baseline"

_s8_fire_out="$(mktemp)"; _TMPDIRS+=("$_s8_fire_out")
_s8_fire_rc=0
harness_layout_orphan_rows "$_s8_root" "$_s8_fire_baseline" \
    > "$_s8_fire_out" 2>/dev/null || _s8_fire_rc=$?

assert "8: a baseline row whose file is gone fires (returns 1)" \
    test "$_s8_fire_rc" -eq 1
assert "8: orphan row emitted as a structured FAIL line (orphan-baseline-row)" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL crate=reify-eval file=crates/reify-eval/tests/gone\.rs reason=orphan-baseline-row$' \
        "$_s8_fire_out"
# Precision: a row BACKED by a real file is never flagged.
assert "8: a baseline row backed by a real file is NOT flagged (precision)" \
    bash -c '! grep -qE "^HARNESS_KLOC_CAP FAIL .*present\.rs" "$1"' _ "$_s8_fire_out"
# Exactly one FAIL line: a second orphan cannot hide behind the first, and a
# single row is never double-reported.
_s8_fire_fails="$(grep -cE '^HARNESS_KLOC_CAP FAIL ' "$_s8_fire_out" || true)"
assert "8: exactly one FAIL line for one orphan row (no double-report, no masking)" \
    test "$_s8_fire_fails" -eq 1

# --- must-not-fire: every row backed by a file, plus comment / blank /
#     commented-out-path lines, which are NOT data rows. ---
_s8_ok_baseline="$(mktemp)"; _TMPDIRS+=("$_s8_ok_baseline")
{
    printf '# a header comment\n'
    printf '\n'
    printf 'crates/reify-eval/tests/present.rs\n'
    printf '#crates/reify-eval/tests/gone.rs\n'
    printf 'crates/reify-eval/tests/present2.rs\n'
} > "$_s8_ok_baseline"

_s8_ok_out="$(mktemp)"; _TMPDIRS+=("$_s8_ok_out")
_s8_ok_rc=0
harness_layout_orphan_rows "$_s8_root" "$_s8_ok_baseline" \
    > "$_s8_ok_out" 2>/dev/null || _s8_ok_rc=$?

assert "8: a baseline whose every row is backed by a file does NOT fire (returns 0)" \
    test "$_s8_ok_rc" -eq 0
# rows=2 pins that the comment, the blank line and the commented-out path are
# stripped — neither counted nor flagged — i.e. the SAME data-row semantics
# harness_layout_baseline_contains uses.
assert "8: clean scan emits a structured PASS line counting only data rows (rows=2)" \
    grep -Eq '^HARNESS_KLOC_CAP PASS scan=baseline-rows rows=2$' "$_s8_ok_out"
assert "8: clean scan emits no FAIL line (comment/blank/commented-out are not rows)" \
    bash -c '! grep -qE "^HARNESS_KLOC_CAP FAIL" "$1"' _ "$_s8_ok_out"

# --- graceful degradation: a MISSING baseline is an explicit FAIL, never a
#     silent pass (the posture Section 3b pins for missing-tests-dir). ---
_s8_missing_base="$(mktemp -d)"; _TMPDIRS+=("$_s8_missing_base")
_s8_missing_baseline="$_s8_missing_base/does-not-exist.manifest"   # never created

_s8_missing_out="$(mktemp)"; _TMPDIRS+=("$_s8_missing_out")
_s8_missing_rc=0
harness_layout_orphan_rows "$_s8_root" "$_s8_missing_baseline" \
    > "$_s8_missing_out" 2>/dev/null || _s8_missing_rc=$?

assert "8: a missing baseline fires rather than vacuously passing (returns 1)" \
    test "$_s8_missing_rc" -eq 1
assert "8: missing baseline surfaces an explicit FAIL (missing-baseline)" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL baseline=.*does-not-exist.* reason=missing-baseline$' \
        "$_s8_missing_out"

# --- ZERO DATA ROWS is a PASS, not a failure. A baseline holding only its
#     header comment (or nothing at all) is the legitimate END STATE of the
#     ratchet: this file's own header calls it "a RATCHET, not a permanent
#     allow-list" whose "shrinking toward empty == consolidation progress". A
#     row-less baseline grandfathers nothing and therefore orphans nothing, so
#     REDding on it would punish FINISHING the consolidation (#5693-#5696).
#     Pinned explicitly because the neighbouring missing/unreadable branches
#     both FAIL, which makes this the one degenerate input where the intended
#     posture is not guessable from its siblings. ---
_s8_norows_baseline="$(mktemp)"; _TMPDIRS+=("$_s8_norows_baseline")
{
    printf '# every row consolidated away — the ratchet reached empty\n'
    printf '\n'
} > "$_s8_norows_baseline"

_s8_norows_out="$(mktemp)"; _TMPDIRS+=("$_s8_norows_out")
_s8_norows_rc=0
harness_layout_orphan_rows "$_s8_root" "$_s8_norows_baseline" \
    > "$_s8_norows_out" 2>/dev/null || _s8_norows_rc=$?

assert "8: an all-comment baseline (zero data rows) PASSes — the ratchet's end state" \
    test "$_s8_norows_rc" -eq 0
assert "8: zero-data-row baseline reports rows=0, not a phantom row from the blank line" \
    grep -Eq '^HARNESS_KLOC_CAP PASS scan=baseline-rows rows=0$' "$_s8_norows_out"

# --- UNREADABLE baseline: an explicit FAIL with its OWN reason= token, never a
#     vacuous `rows=0` PASS. The distinction matters precisely because zero rows
#     is legitimate (above): "I read it and it has no rows" and "I could not read
#     it" must not collapse into the same verdict, or the guard reports clean at
#     the exact moment it can see nothing. The detector cannot infer this from
#     the row count — it reads harness_layout_baseline_rows's exit status, which
#     is why that function narrowly propagates grep's error exit 2 instead of
#     swallowing it alongside the no-lines-matched exit 1. ---
if [ "$(id -u)" -eq 0 ]; then
    echo "  (skipped: running as uid 0 — chmod 000 does not deny root reads)"
else
    _s8_unread_baseline="$(mktemp)"; _TMPDIRS+=("$_s8_unread_baseline")
    printf 'crates/reify-eval/tests/present.rs\n' > "$_s8_unread_baseline"
    chmod 000 "$_s8_unread_baseline"

    _s8_unread_out="$(mktemp)"; _TMPDIRS+=("$_s8_unread_out")
    _s8_unread_rc=0
    harness_layout_orphan_rows "$_s8_root" "$_s8_unread_baseline" \
        > "$_s8_unread_out" 2>/dev/null || _s8_unread_rc=$?

    assert "8: an unreadable baseline fires rather than vacuously passing (returns 1)" \
        test "$_s8_unread_rc" -eq 1
    assert "8: unreadable baseline surfaces its own FAIL token (unreadable-baseline)" \
        grep -Eq '^HARNESS_KLOC_CAP FAIL baseline=.* reason=unreadable-baseline$' \
            "$_s8_unread_out"
    assert "8: unreadable baseline emits NO rows=0 PASS line (the vacuous-pass regression)" \
        bash -c '! grep -qE "^HARNESS_KLOC_CAP PASS scan=baseline-rows" "$1"' \
            _ "$_s8_unread_out"

    # Restore so the EXIT trap's `rm -rf` is never fighting permissions.
    chmod 644 "$_s8_unread_baseline"
fi

# --- shape: a row that is not `crates/<crate>/...` must still be reported, with
#     crate=- (the same non-crate-scoped field harness_stale_selector_violations
#     emits). A malformed row surfaces loudly instead of crashing the crate
#     derivation or being silently skipped. ---
_s8_shape_baseline="$(mktemp)"; _TMPDIRS+=("$_s8_shape_baseline")
printf 'not/a/crate/path.rs\n' > "$_s8_shape_baseline"

_s8_shape_out="$(mktemp)"; _TMPDIRS+=("$_s8_shape_out")
_s8_shape_rc=0
harness_layout_orphan_rows "$_s8_root" "$_s8_shape_baseline" \
    > "$_s8_shape_out" 2>/dev/null || _s8_shape_rc=$?

assert "8: a non-crates/ orphan row still fires (returns 1)" \
    test "$_s8_shape_rc" -eq 1
assert "8: a non-crates/ orphan row reports crate=- rather than a garbled crate" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL crate=- file=not/a/crate/path\.rs reason=orphan-baseline-row$' \
        "$_s8_shape_out"

# --- MEMO CONTRACT. harness_layout_baseline_contains answers ~495 times per run
#     against the ~495-row live baseline, so the lib memoizes each baseline's
#     rows keyed by PATH for the shell's lifetime (that took the live membership
#     loop from ~19s to ~0.2s). Two halves, both pinned here because neither is
#     visible from the guard's own verdicts:
#       (1) the memo is REAL — a rewrite-in-place at an already-queried path is
#           NOT observed, which is the documented cost of the path-only key and
#           the reason every fixture above uses a fresh mktemp path;
#       (2) harness_layout_baseline_cache_reset is the escape hatch that makes
#           (1) recoverable rather than a trap.
#     Without (1) a future "optimization" could quietly drop the memo and only
#     the wall clock would notice; without (2) the staleness would be permanent.
_s8_memo_baseline="$(mktemp)"; _TMPDIRS+=("$_s8_memo_baseline")
printf 'crates/reify-eval/tests/first.rs\n' > "$_s8_memo_baseline"

_s8_memo_rc1=0
harness_layout_baseline_contains "crates/reify-eval/tests/first.rs" "$_s8_memo_baseline" \
    || _s8_memo_rc1=$?
assert "8: membership predicate finds a row on first (uncached) query" \
    test "$_s8_memo_rc1" -eq 0

# Rewrite in place at the SAME path — the memo must still answer from the rows
# it parsed above.
printf 'crates/reify-eval/tests/second.rs\n' > "$_s8_memo_baseline"
_s8_memo_stale_new=0
harness_layout_baseline_contains "crates/reify-eval/tests/second.rs" "$_s8_memo_baseline" \
    || _s8_memo_stale_new=$?
_s8_memo_stale_old=0
harness_layout_baseline_contains "crates/reify-eval/tests/first.rs" "$_s8_memo_baseline" \
    || _s8_memo_stale_old=$?
assert "8: memo is real — a rewritten baseline's NEW row is not seen before a reset" \
    test "$_s8_memo_stale_new" -ne 0
assert "8: memo is real — the OLD row is still reported after a rewrite-in-place" \
    test "$_s8_memo_stale_old" -eq 0

harness_layout_baseline_cache_reset "$_s8_memo_baseline"
_s8_memo_fresh_new=0
harness_layout_baseline_contains "crates/reify-eval/tests/second.rs" "$_s8_memo_baseline" \
    || _s8_memo_fresh_new=$?
_s8_memo_fresh_old=0
harness_layout_baseline_contains "crates/reify-eval/tests/first.rs" "$_s8_memo_baseline" \
    || _s8_memo_fresh_old=$?
assert "8: cache reset re-reads from disk — the NEW row is found" \
    test "$_s8_memo_fresh_new" -eq 0
assert "8: cache reset drops the stale keys — the OLD row is gone" \
    test "$_s8_memo_fresh_old" -ne 0

# --- LIVE scan over the real checked-in baseline. Uses the already-resolved
#     $BASELINE (from harness_layout_baseline_path), so the
#     REIFY_HARNESS_LAYOUT_BASELINE override is honored identically to the rest
#     of this guard.
#
#     A GREEN here is NOT equivalent to the manifest header's regenerate-and-diff
#     recipe being byte-clean: that recipe enumerates via `git ls-files` and this
#     scan tests the working tree, so a row backed by an untracked file passes
#     here and still shows up there. The gap is deliberate and one-sided — see
#     the RESIDUAL block in harness_layout_orphan_rows's header for why the
#     working-tree predicate is the right one for a merge gate. ---
_s8_live_out="$(mktemp)"; _TMPDIRS+=("$_s8_live_out")
_s8_live_rc=0
harness_layout_orphan_rows "$REPO_ROOT" "$BASELINE" \
    > "$_s8_live_out" 2>/dev/null || _s8_live_rc=$?

# Offender lines to the archived log on failure (the Section 5 idiom, gated on
# failure so a clean run's output stays byte-for-byte unchanged). Not cosmetic:
# `assert` only dumps the stdout of the `test ...` checker it invokes, which is
# always empty, so without this a failing live scan's own `file=<row>` lines
# would never reach the archived merge-verify log — the 2026-07-20 incident the
# Section 5 comment records.
if [ "$_s8_live_rc" -ne 0 ]; then
    echo "  ---- Section 8: live orphan-row scan output (printed on failure) ----"
    cat "$_s8_live_out"
    echo "  ---- Section 8: end live scan output ----"
fi

assert "8: live baseline has no orphan rows (every row names a file that exists on disk)" \
    test "$_s8_live_rc" -eq 0
# Pins the live grammar WITHOUT pinning a row count — see the note below.
assert "8: live orphan scan emits a structured PASS line carrying the data-row count" \
    grep -Eq '^HARNESS_KLOC_CAP PASS scan=baseline-rows rows=[0-9]+$' "$_s8_live_out"

# DELIBERATELY NO `rows >= N` NON-VACUITY FLOOR HERE, unlike Section 6's
# `>= 13 harness roots` and Section 7's `>= 300 stems`. The analogy is tempting
# and it is a landmine: this file's own header states the baseline is "a RATCHET,
# not a permanent allow-list" and that "shrinking toward empty == consolidation
# progress". A `rows >= N` floor would therefore RED the merge gate as a REWARD
# for FINISHING the consolidation (#5693-#5696) that this very guard exists to
# protect — invisible today at ~495 rows, firing only at the finish line.
#
# Non-vacuity comes instead from Section 8's own must-fire fixture above, which
# proves the detector is wired and DOES fire on a synthetic orphan. That is the
# posture Section 3b takes for missing-tests-dir, not Section 6/7's. The live
# PASS line still carries `rows=<n>` for observability, matched by `[0-9]+`
# rather than asserted against a value.

# ===========================================================================
# Section 9: baseline row SHAPE / well-formedness — every grandfather-baseline
# row must itself be a well-formed in-scope standalone
# (harness_layout_in_scope_standalone), independent of whether the file it
# names happens to exist on disk.
#
# WHY THIS IS NOT ALREADY COVERED. Section 8 only asks "does this row's file
# exist?" — a row like `crates/reify-eval/src/foo.rs` or
# `crates/reify-eval/tests/sub/dir/x.rs`, naming a file that EXISTS, passes
# Section 8 silently. Rule (b)'s detector (harness_layout_violations)
# enumerates existing FILES under a tests dir and asks whether each is
# sanctioned — it never visits a row that names no such file, so a malformed
# row is structurally unreachable from it too. A malformed-but-backed row
# therefore drifts into the ratchet as dead weight, caught by neither existing
# check.
#
# "Well-formed" is defined as harness_layout_in_scope_standalone
# (harness-layout-lib.sh) — the same pure-string predicate that already
# encodes the manifest header's own "WHAT IT LISTS" / "INTENTIONALLY
# EXCLUDED" claims — not a locally re-derived regex, which would be a second,
# weaker expression of the same rule (the G7 no-lockstep-duplication concern
# the lib's own header exists to prevent).
# ===========================================================================
echo ""
echo "--- Section 9: baseline row shape / well-formedness ---"

# --- must-fire: a baseline row that is not an in-scope standalone. Six
#     distinct malformed shapes, chosen to name what Section 8 (the
#     orphan-row detector) is structurally blind to whenever the file
#     happens to exist on disk — Section 8 only asks "is the file there?",
#     never "is the row itself well-formed?". This detector is pure-string
#     (no <root_dir>), so none of these rows needs a backing file on disk to
#     exercise it:
#       - crates/reify-eval/src/foo.rs                  exists, but is not
#         under tests/ at all — passes Section 8 silently if it existed.
#       - crates/reify-eval/tests/sub/dir/x.rs           nested below tests/.
#       - crates/reify-eval/tests/harness_something.rs  a harness_*.rs root,
#         sanctioned by construction, never itself a baseline row.
#       - crates/reify-eval/tests/tensegrity_t0a.rs      one of the 7
#         permanently-standalone override stems (invariant I1).
#       - crates/reify-solver-elastic/tests/x.rs         a real crate, but
#         not one of the 5 consolidatable crates.
#       - not/a/crate/path.rs                            not crates/-rooted
#         at all.
#     The first five also exercise the DERIVED half of
#     _harness_layout_row_crate (crate=reify-eval / crate=reify-solver-elastic)
#     — before this fixture grew, only its `-`-fallback branch was pinned. ---
_s9_fire_baseline="$(mktemp)"; _TMPDIRS+=("$_s9_fire_baseline")
printf '%s\n' \
    'crates/reify-eval/tests/present.rs' \
    'crates/reify-eval/src/foo.rs' \
    'crates/reify-eval/tests/sub/dir/x.rs' \
    'crates/reify-eval/tests/harness_something.rs' \
    'crates/reify-eval/tests/tensegrity_t0a.rs' \
    'crates/reify-solver-elastic/tests/x.rs' \
    'not/a/crate/path.rs' \
    > "$_s9_fire_baseline"

_s9_fire_out="$(mktemp)"; _TMPDIRS+=("$_s9_fire_out")
_s9_fire_rc=0
harness_layout_malformed_rows "$_s9_fire_baseline" \
    > "$_s9_fire_out" 2>/dev/null || _s9_fire_rc=$?

assert "9: a baseline row that is not an in-scope standalone fires (returns 1)" \
    test "$_s9_fire_rc" -eq 1
assert "9: malformed row emitted as a structured FAIL line (malformed-baseline-row)" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL crate=- file=not/a/crate/path\.rs reason=malformed-baseline-row$' \
        "$_s9_fire_out"
assert "9: a file that EXISTS but is not under tests/ is caught (crate= derived, not the - fallback)" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL crate=reify-eval file=crates/reify-eval/src/foo\.rs reason=malformed-baseline-row$' \
        "$_s9_fire_out"
assert "9: a row nested below tests/ is rejected" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL crate=reify-eval file=crates/reify-eval/tests/sub/dir/x\.rs reason=malformed-baseline-row$' \
        "$_s9_fire_out"
assert "9: a harness_*.rs root is rejected (sanctioned by construction, never a baseline row)" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL crate=reify-eval file=crates/reify-eval/tests/harness_something\.rs reason=malformed-baseline-row$' \
        "$_s9_fire_out"
assert "9: one of the 7 override stems is rejected (permanently standalone, never a baseline row)" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL crate=reify-eval file=crates/reify-eval/tests/tensegrity_t0a\.rs reason=malformed-baseline-row$' \
        "$_s9_fire_out"
assert "9: a non-consolidatable crate is rejected (its own crate= derived, not skipped)" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL crate=reify-solver-elastic file=crates/reify-solver-elastic/tests/x\.rs reason=malformed-baseline-row$' \
        "$_s9_fire_out"
# Precision: the one row that IS a well-formed in-scope standalone is never
# flagged.
assert "9: a well-formed row is NOT flagged (precision)" \
    bash -c '! grep -qE "^HARNESS_KLOC_CAP FAIL .*present\.rs" "$1"' _ "$_s9_fire_out"
# Exactly one FAIL line per malformed row, six total: none hides behind
# another, and none is double-reported.
_s9_fire_fails="$(grep -cE '^HARNESS_KLOC_CAP FAIL ' "$_s9_fire_out" || true)"
assert "9: exactly one FAIL line per malformed row, six total (no double-report, no masking)" \
    test "$_s9_fire_fails" -eq 6

# --- must-not-fire: every row is a well-formed in-scope standalone, plus
#     comment / blank / commented-out-path lines, which are NOT data rows. ---
_s9_ok_baseline="$(mktemp)"; _TMPDIRS+=("$_s9_ok_baseline")
{
    printf '# a header comment\n'
    printf '\n'
    printf 'crates/reify-eval/tests/present.rs\n'
    printf '#crates/reify-eval/tests/gone.rs\n'
    printf 'crates/reify-compiler/tests/present2.rs\n'
} > "$_s9_ok_baseline"

_s9_ok_out="$(mktemp)"; _TMPDIRS+=("$_s9_ok_out")
_s9_ok_rc=0
harness_layout_malformed_rows "$_s9_ok_baseline" \
    > "$_s9_ok_out" 2>/dev/null || _s9_ok_rc=$?

assert "9: a baseline whose every row is a well-formed in-scope standalone does NOT fire (returns 0)" \
    test "$_s9_ok_rc" -eq 0
# rows=2 pins that the comment, the blank line and the commented-out path are
# stripped — neither counted nor flagged — i.e. the SAME data-row semantics
# harness_layout_baseline_contains uses. The two rows span two DIFFERENT
# consolidatable crates, so the crate-membership half of the predicate is
# exercised for more than one value.
assert "9: clean scan emits a structured PASS line counting only data rows (rows=2)" \
    grep -Eq '^HARNESS_KLOC_CAP PASS scan=baseline-row-shape rows=2$' "$_s9_ok_out"
assert "9: clean scan emits no FAIL line (comment/blank/commented-out are not rows)" \
    bash -c '! grep -qE "^HARNESS_KLOC_CAP FAIL" "$1"' _ "$_s9_ok_out"

# --- ZERO DATA ROWS is a PASS, not a failure — the exact assert this task
#     exists to install. A baseline holding only its header comment (or
#     nothing at all) is the legitimate END STATE of the ratchet: this file's
#     own header calls it "a RATCHET, not a permanent allow-list" whose
#     "shrinking toward empty == consolidation progress", so REDding at zero
#     rows would punish FINISHING the consolidation (#5693-#5696, plus the
#     reify-cli / reify-syntax / reify-kernel-occt lines) that this guard
#     protects. Non-vacuity for THIS section comes from the must-fire fixture
#     above, NEVER from a `rows >= N` floor — the same posture Section 8
#     takes for its own zero-rows fixture. ---
_s9_norows_baseline="$(mktemp)"; _TMPDIRS+=("$_s9_norows_baseline")
{
    printf '# every row consolidated away — the ratchet reached empty\n'
    printf '\n'
} > "$_s9_norows_baseline"

_s9_norows_out="$(mktemp)"; _TMPDIRS+=("$_s9_norows_out")
_s9_norows_rc=0
harness_layout_malformed_rows "$_s9_norows_baseline" \
    > "$_s9_norows_out" 2>/dev/null || _s9_norows_rc=$?

assert "9: an all-comment baseline (zero data rows) PASSes — the ratchet's designed end state, not a failure" \
    test "$_s9_norows_rc" -eq 0
assert "9: zero-data-row baseline reports rows=0, not a phantom row from the herestring's empty line" \
    grep -Eq '^HARNESS_KLOC_CAP PASS scan=baseline-row-shape rows=0$' "$_s9_norows_out"

# --- graceful degradation: a MISSING baseline is an explicit FAIL, never a
#     silent pass (the posture Section 8 pins for its own orphan-row detector).
#     "I read it and it has no rows" (PASS, above) and "I could not read it /
#     it is not there" (FAIL, here) must never collapse into the same verdict,
#     or the guard reports clean at the exact moment it can see nothing. ---
_s9_missing_base="$(mktemp -d)"; _TMPDIRS+=("$_s9_missing_base")
_s9_missing_baseline="$_s9_missing_base/does-not-exist.manifest"   # never created

_s9_missing_out="$(mktemp)"; _TMPDIRS+=("$_s9_missing_out")
_s9_missing_rc=0
harness_layout_malformed_rows "$_s9_missing_baseline" \
    > "$_s9_missing_out" 2>/dev/null || _s9_missing_rc=$?

assert "9: a missing baseline fires rather than vacuously passing (returns 1)" \
    test "$_s9_missing_rc" -eq 1
assert "9: missing baseline surfaces an explicit FAIL (missing-baseline)" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL baseline=.*does-not-exist.* reason=missing-baseline$' \
        "$_s9_missing_out"

# --- UNREADABLE baseline: an explicit FAIL with its own reason= token, never a
#     vacuous `rows=0` PASS. This cannot be inferred from the row count — it is
#     read off harness_layout_baseline_rows's EXIT STATUS, which is exactly why
#     that lib function narrowly propagates grep's error exit (>= 2) instead of
#     swallowing it alongside the no-lines-matched exit 1
#     (tests/infra/harness-layout-lib.sh:252-260). ---
if [ "$(id -u)" -eq 0 ]; then
    echo "  (skipped: running as uid 0 — chmod 000 does not deny root reads)"
else
    _s9_unread_baseline="$(mktemp)"; _TMPDIRS+=("$_s9_unread_baseline")
    printf 'crates/reify-eval/tests/present.rs\n' > "$_s9_unread_baseline"
    chmod 000 "$_s9_unread_baseline"

    _s9_unread_out="$(mktemp)"; _TMPDIRS+=("$_s9_unread_out")
    _s9_unread_rc=0
    harness_layout_malformed_rows "$_s9_unread_baseline" \
        > "$_s9_unread_out" 2>/dev/null || _s9_unread_rc=$?

    assert "9: an unreadable baseline fires rather than vacuously passing (returns 1)" \
        test "$_s9_unread_rc" -eq 1
    assert "9: unreadable baseline surfaces its own FAIL token (unreadable-baseline)" \
        grep -Eq '^HARNESS_KLOC_CAP FAIL baseline=.* reason=unreadable-baseline$' \
            "$_s9_unread_out"
    assert "9: unreadable baseline emits NO rows=0 PASS line (the vacuous-pass regression)" \
        bash -c '! grep -qE "^HARNESS_KLOC_CAP PASS scan=baseline-row-shape" "$1"' \
            _ "$_s9_unread_out"

    # Restore so the EXIT trap's `rm -rf` is never fighting permissions.
    chmod 644 "$_s9_unread_baseline"
fi

# --- LIVE scan over the real checked-in baseline. Uses the already-resolved
#     $BASELINE (from harness_layout_baseline_path), so the
#     REIFY_HARNESS_LAYOUT_BASELINE override is honored identically to the
#     rest of this guard. ---
_s9_live_out="$(mktemp)"; _TMPDIRS+=("$_s9_live_out")
_s9_live_rc=0
harness_layout_malformed_rows "$BASELINE" \
    > "$_s9_live_out" 2>/dev/null || _s9_live_rc=$?

# Offender lines to the archived log on failure (the Section 5/8 idiom, gated
# on failure so a clean run's output stays byte-for-byte unchanged). Not
# cosmetic: `assert` only dumps the stdout of the `test ...` checker it
# invokes, which is always empty, so without this a failing live scan's own
# `file=<row>` lines would never reach the archived merge-verify log — the
# 2026-07-20 incident the Section 5 comment records.
if [ "$_s9_live_rc" -ne 0 ]; then
    echo "  ---- Section 9: live row-shape scan output (printed on failure) ----"
    cat "$_s9_live_out"
    echo "  ---- Section 9: end live scan output ----"
fi

assert "9: live baseline has no malformed rows (every data row is a well-formed in-scope standalone)" \
    test "$_s9_live_rc" -eq 0
# Pins the live grammar WITHOUT pinning a row count — a pinned count here would
# be the same landmine Section 8 declines, in a different section: the count
# is expected to shrink as consolidation proceeds, all the way to 0.
assert "9: live row-shape scan emits a structured PASS line carrying the data-row count" \
    grep -Eq '^HARNESS_KLOC_CAP PASS scan=baseline-row-shape rows=[0-9]+$' "$_s9_live_out"

# ===========================================================================
# Section 10: rule (d) — undeclared member (a file physically present under a
# harness root's own module dir with NO reachable `mod` declaration anywhere
# in the root's transitive mod-graph). rustc never compiles such a file, so
# its `#[test]` fns vanish with no compile error, no link error, and no
# test-count signal — this is the converse of Section 6, which walks
# declaration -> file; this walks file -> declaration, and neither subsumes
# the other.
# ===========================================================================
echo ""
echo "--- Section 10: rule (d) — undeclared member (file present, never declared) ---"

# --- must-fire: a module-dir file with no reachable `mod` declaration ---
_s10_bad_dir="$(mktemp -d)"; _TMPDIRS+=("$_s10_bad_dir")
mkdir -p "$_s10_bad_dir/harness_synth"
{
    printf '#[path = "harness_synth/declared.rs"]\n'
    printf 'mod declared;\n'
} > "$_s10_bad_dir/harness_synth.rs"
printf '#[test]\nfn t() {}\n' > "$_s10_bad_dir/harness_synth/declared.rs"
printf '#[test]\nfn t() {}\n' > "$_s10_bad_dir/harness_synth/orphan.rs"

_s10_bad_out="$(mktemp)"; _TMPDIRS+=("$_s10_bad_out")
_s10_bad_rc=0
harness_undeclared_member_violations synthcrate "$_s10_bad_dir" \
    > "$_s10_bad_out" 2>/dev/null || _s10_bad_rc=$?

assert "10: a module-dir file with no reachable \`mod\` declaration fires (returns 1)" \
    test "$_s10_bad_rc" -eq 1
assert "10: violation emitted as a structured FAIL line (undeclared-member, member named)" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL crate=synthcrate file=.*harness_synth\.rs reason=undeclared-member member=harness_synth/orphan\.rs$' "$_s10_bad_out"
assert "10: the DECLARED member is not named in any FAIL line (precision)" \
    bash -c '! grep -q "member=harness_synth/declared.rs" "$1"' _ "$_s10_bad_out"

# --- must-not-fire: a member declared TRANSITIVELY, from a nested mod.rs
# inside the module dir rather than from the root itself. This is the case
# that rules out a root-only, single-hop declared-members walk: it must not
# contradict harness_layout_unit_lines, which explicitly counts members "at
# ANY depth" (its own Section 1c fixture already builds
# harness_nested/deep/b.rs). rustc resolves a bare `mod` declared in a
# `mod.rs` against THAT FILE'S OWN directory, so `sub/mod.rs`'s `mod deep;`
# reaches `sub/deep.rs` without the root ever naming it directly. ---
_s10_nested_dir="$(mktemp -d)"; _TMPDIRS+=("$_s10_nested_dir")
mkdir -p "$_s10_nested_dir/harness_synth/sub"
{
    printf '#[path = "harness_synth/sub/mod.rs"]\n'
    printf 'mod sub;\n'
} > "$_s10_nested_dir/harness_synth.rs"
printf 'mod deep;\n' > "$_s10_nested_dir/harness_synth/sub/mod.rs"
printf '#[test]\nfn t() {}\n' > "$_s10_nested_dir/harness_synth/sub/deep.rs"

_s10_nested_out="$(mktemp)"; _TMPDIRS+=("$_s10_nested_out")
_s10_nested_rc=0
harness_undeclared_member_violations synthcrate "$_s10_nested_dir" \
    > "$_s10_nested_out" 2>/dev/null || _s10_nested_rc=$?

assert "10: a member declared transitively via a nested mod.rs is NOT flagged (rc 0)" \
    test "$_s10_nested_rc" -eq 0
# roots=1 members=2 pins the count exactly (the Section 8/9 rows=2 idiom):
# 1 root (harness_synth.rs) and 2 module-dir files compared (sub/mod.rs,
# sub/deep.rs) — proving the walk actually descended into sub/ rather than
# vacuously reporting members=0.
assert "10: transitive-member scan emits a structured PASS line (crate=synthcrate roots=1 members=2)" \
    grep -Eq '^HARNESS_KLOC_CAP PASS crate=synthcrate scan=undeclared-members roots=1 members=2$' "$_s10_nested_out"
assert "10: transitive-member scan emits NO FAIL line at all" \
    bash -c '! grep -qE "^HARNESS_KLOC_CAP FAIL" "$1"' _ "$_s10_nested_out"

# --- must-not-fire: NORMALIZATION — non-canonical but valid #[path] values (a
# leading "./" and a "../"-round-trip) must resolve to the same member as
# their canonical form, or a raw-vs-normalized comparison would false-fire. ---
_s10_norm_dir="$(mktemp -d)"; _TMPDIRS+=("$_s10_norm_dir")
mkdir -p "$_s10_norm_dir/harness_synth"
{
    printf '#[path = "./harness_synth/a.rs"]\n'
    printf 'mod a;\n'
    printf '#[path = "harness_synth/../harness_synth/b.rs"]\n'
    printf 'mod b;\n'
} > "$_s10_norm_dir/harness_synth.rs"
printf '#[test]\nfn t() {}\n' > "$_s10_norm_dir/harness_synth/a.rs"
printf '#[test]\nfn t() {}\n' > "$_s10_norm_dir/harness_synth/b.rs"

_s10_norm_out="$(mktemp)"; _TMPDIRS+=("$_s10_norm_out")
_s10_norm_rc=0
harness_undeclared_member_violations synthcrate "$_s10_norm_dir" \
    > "$_s10_norm_out" 2>/dev/null || _s10_norm_rc=$?

assert "10: non-canonical #[path] forms (./ and ../-round-trip) are NOT flagged (rc 0)" \
    test "$_s10_norm_rc" -eq 0
assert "10: normalization scan emits NO FAIL line at all" \
    bash -c '! grep -qE "^HARNESS_KLOC_CAP FAIL" "$1"' _ "$_s10_norm_out"

# --- must-not-fire: MODDIR BOUNDARY + the live #[cfg] shape. A #[cfg]-gated
# member (crates/reify-cli/tests/harness_cli.rs:184-186's #[cfg] -> #[path]
# -> mod ordering) is DECLARED regardless of cfg state, and a bare `mod
# common;` resolving to a retained tests/ sibling is not a member at all — it
# resolves OUTSIDE the module dir. ---
_s10_bound_dir="$(mktemp -d)"; _TMPDIRS+=("$_s10_bound_dir")
mkdir -p "$_s10_bound_dir/harness_synth" "$_s10_bound_dir/common"
{
    printf '#[cfg(target_os = "linux")]\n'
    printf '#[path = "harness_synth/gated.rs"]\n'
    printf 'mod gated;\n'
    printf 'mod common;\n'
} > "$_s10_bound_dir/harness_synth.rs"
printf '#[test]\nfn t() {}\n' > "$_s10_bound_dir/harness_synth/gated.rs"
printf '#[test]\nfn t() {}\n' > "$_s10_bound_dir/common/mod.rs"

_s10_bound_out="$(mktemp)"; _TMPDIRS+=("$_s10_bound_out")
_s10_bound_rc=0
harness_undeclared_member_violations synthcrate "$_s10_bound_dir" \
    > "$_s10_bound_out" 2>/dev/null || _s10_bound_rc=$?

assert "10: a #[cfg]-gated member (#[cfg] then #[path] then mod) is NOT flagged (rc 0)" \
    test "$_s10_bound_rc" -eq 0
assert "10: moddir-boundary scan emits NO FAIL line at all" \
    bash -c '! grep -qE "^HARNESS_KLOC_CAP FAIL" "$1"' _ "$_s10_bound_out"

# --- must-not-fire: SHAPE GUARDS. A single-file harness (no module dir) has
# nothing to scan; and the Section 1c ORPHAN case (a module dir with no
# sibling root) is re-driven through rule (d) to confirm it preserves that
# pin rather than newly firing on it. ---
_s10_single_dir="$(mktemp -d)"; _TMPDIRS+=("$_s10_single_dir")
printf '#[test]\nfn t() {}\n' > "$_s10_single_dir/harness_synth.rs"

_s10_single_out="$(mktemp)"; _TMPDIRS+=("$_s10_single_out")
_s10_single_rc=0
harness_undeclared_member_violations synthcrate "$_s10_single_dir" \
    > "$_s10_single_out" 2>/dev/null || _s10_single_rc=$?

assert "10: a single-file harness (no module dir) is NOT flagged (rc 0)" \
    test "$_s10_single_rc" -eq 0
# roots=1 members=0 pins that the single root is still counted even though
# it contributes nothing to members= (no module dir to compare at all).
assert "10: single-file-harness scan emits a structured PASS line (crate=synthcrate roots=1 members=0)" \
    grep -Eq '^HARNESS_KLOC_CAP PASS crate=synthcrate scan=undeclared-members roots=1 members=0$' "$_s10_single_out"

_s10_orphan_dir="$(mktemp -d)"; _TMPDIRS+=("$_s10_orphan_dir")
mkdir -p "$_s10_orphan_dir/harness_orphan"
awk 'BEGIN { for (i = 0; i < 3; i++) print "// x" }' > "$_s10_orphan_dir/harness_orphan/x.rs"

_s10_orphan_out="$(mktemp)"; _TMPDIRS+=("$_s10_orphan_out")
_s10_orphan_rc=0
harness_undeclared_member_violations synthcrate "$_s10_orphan_dir" \
    > "$_s10_orphan_out" 2>/dev/null || _s10_orphan_rc=$?

assert "10: an orphan module dir with no sibling root is NOT flagged, same pin as 1c (rc 0)" \
    test "$_s10_orphan_rc" -eq 0
assert "10: orphan-dir scan emits NO FAIL line at all" \
    bash -c '! grep -qE "^HARNESS_KLOC_CAP FAIL" "$1"' _ "$_s10_orphan_out"

# --- must-FIRE: SILENT-BIND RESIDUE — the case Section 6 deliberately
# tolerates. root writes bare `mod x;`, and BOTH a retained tests/ sibling
# ($dir/x.rs) AND a module-dir file of the same stem ($dir/harness_synth/x.rs)
# exist. rustc's crate-root-relative resolution binds the SIBLING (Section
# 6's principled exception, unchanged by this rule), which leaves the
# module-dir file silently dead — the concrete witness for why rule (d)
# exists at all. ---
_s10_bind_dir="$(mktemp -d)"; _TMPDIRS+=("$_s10_bind_dir")
mkdir -p "$_s10_bind_dir/harness_synth"
printf 'mod x;\n' > "$_s10_bind_dir/harness_synth.rs"
printf '#[test]\nfn t() {}\n' > "$_s10_bind_dir/x.rs"
printf '#[test]\nfn t() {}\n' > "$_s10_bind_dir/harness_synth/x.rs"

_s10_bind_attr_out="$(mktemp)"; _TMPDIRS+=("$_s10_bind_attr_out")
_s10_bind_attr_rc=0
harness_path_attr_violations synthcrate "$_s10_bind_dir" \
    > "$_s10_bind_attr_out" 2>/dev/null || _s10_bind_attr_rc=$?
assert "10: Section 6's principled sibling exception still holds unchanged (rc 0)" \
    test "$_s10_bind_attr_rc" -eq 0

_s10_bind_out="$(mktemp)"; _TMPDIRS+=("$_s10_bind_out")
_s10_bind_rc=0
harness_undeclared_member_violations synthcrate "$_s10_bind_dir" \
    > "$_s10_bind_out" 2>/dev/null || _s10_bind_rc=$?

assert "10: the silent-bind residue module-dir file DOES fire (returns 1)" \
    test "$_s10_bind_rc" -eq 1
assert "10: silent-bind residue reported as undeclared-member member=harness_synth/x.rs" \
    grep -Eq '^HARNESS_KLOC_CAP FAIL crate=synthcrate file=.*harness_synth\.rs reason=undeclared-member member=harness_synth/x\.rs$' "$_s10_bind_out"

# --- LIVE scan: fused into Section 6's live loop above (task #7042
# amendment, the Section 5b/5c pattern — see that loop's comment) rather than
# a second byte-identical traversal of `crates/*/tests` here. $_s10_live_out,
# $_s10_live_rc and the shared $_s6s10_live_roots are already populated by
# the time this section runs; nothing below re-scans. This is the
# WHOLE-TREE-LIVE landing precondition prerequisite pre-1 measured before any
# code was written (31 roots, 585 module-dir member files, 0 undeclared, on
# 4a9f2d6d4c): a rule (d) that visited every root but silently compared zero
# member files (a broken find glob, a moddir guard that always `continue`s,
# a declared-set that accidentally contains everything) would pass an rc-0 +
# roots-floor assert VACUOUSLY — the exact silent-failure mode this whole
# task exists to close, one level up. The member-count floor below guards
# the guard. ---

# Offender lines to the archived log on failure (the Section 5/6 idiom).
if [ "$_s10_live_rc" -ne 0 ]; then
    echo "  ---- Section 10: live undeclared-member scan output (printed on failure) ----"
    cat "$_s10_live_out"
    echo "  ---- Section 10: end live scan output ----"
fi

assert "10: live tree — every harness root declares every module-dir member (rc 0)" \
    test "$_s10_live_rc" -eq 0
# Non-vacuity (roots): shared with Section 6's own roots assert above — the
# SAME fused scan populated both, so this can no longer silently drift from
# Section 6's count the way two independently-maintained variables could.
assert "10: live scan actually visited harness roots (>= 13 found, non-vacuity)" \
    test "$_s6s10_live_roots" -ge 13

# Non-vacuity (members): roots alone are not enough — see the block comment
# above. harness_undeclared_member_violations emits one aggregate `PASS
# crate=<c> scan=undeclared-members roots=<n> members=<n>` line PER CALL
# (i.e. per crate — the same granularity Section 6 is called at, and now
# carrying the SAME crate= field Section 6's own PASS line does — task #7042
# amendment, so an operator reading the archived merge-verify log can tell
# which crate contributed which counts), so the live loop above accumulates
# one such line per clean crate into $_s10_live_out. Summing their
# `members=` fields across every crate is the whole-tree member count this
# rule actually ratchets (585 today, measured on 4a9f2d6d4c — the count only
# grows as consolidation leaves land, so this is a floor, not a pin,
# mirroring Sections 8/9's own declared-row floors).
assert "10: live scan emits the new aggregate PASS grammar (crate=C scan=undeclared-members roots=N members=N)" \
    grep -Eq '^HARNESS_KLOC_CAP PASS crate=[A-Za-z0-9_-]+ scan=undeclared-members roots=[0-9]+ members=[0-9]+$' "$_s10_live_out"

# `|| true` (the Section 4 idiom, tests/infra/test_harness_kloc_cap.sh:1315):
# under `set -o pipefail` a legitimate zero-match grep exits 1, and awk's
# unconditional `END { print sum + 0 }` never masks that — without `|| true`
# this whole command substitution would abort the script here, before
# test_summary ever runs (the same class of hazard test_helpers.sh's assert()
# guards against at its own `rm -f`/`return 0`).
_s10_live_members="$(grep -E '^HARNESS_KLOC_CAP PASS crate=[A-Za-z0-9_-]+ scan=undeclared-members roots=[0-9]+ members=[0-9]+$' "$_s10_live_out" \
    | awk -F'members=' '{sum += $2} END {print sum + 0}' || true)"
assert "10: live scan actually compared >= 300 module-dir member files across all crates (non-vacuity floor, 585 today)" \
    test "$_s10_live_members" -ge 300

test_summary
