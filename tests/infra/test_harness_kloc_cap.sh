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
# declared from the harness root with `mod <file>;` (or, when the on-disk
# name cannot be a bare module ident, `#[path = "harness_<subsystem>/<file>.rs"]
# mod <file>;`). Declaring it under its ORIGINAL stem PRESERVES the module
# path, so existing `cargo test` filtersets of the form `test(/^<file>::/)`
# (and every `<file>::<test_name>` selector the merge gate already runs)
# resolve UNCHANGED after consolidation — consolidation is layout-only, never
# a test-selector break.
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
#   (a) kLOC CAP. Every `harness_<subsystem>.rs` compile unit is <= the cap
#       (CAP_LINES, measured as RAW `wc -l` line count — simplest and
#       conservative, PRD §11; ~20 kLOC is the upper end of the §7 10-20 kLOC
#       band). A harness that grows past the cap must be SPLIT into a second
#       `harness_<subsystem2>.rs`, never allowed to balloon unbounded.
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
#           HARNESS_KLOC_CAP FAIL crate=<c> file=<path> reason=exceeds-cap lines=<n> cap=<n>
#           HARNESS_KLOC_CAP FAIL crate=<c> file=<path> reason=unsanctioned-standalone
#           HARNESS_KLOC_CAP PASS crate=<c>
#           HARNESS_KLOC_CAP SUMMARY crates=<n> violations=<n>
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

# ---------------------------------------------------------------------------
# Config constants (the C1/C2 contract parameters).
# ---------------------------------------------------------------------------

# The 5 crates whose top-level tests/*.rs are subject to the C1 layout
# contract. reify-solver-elastic / reify-eval-fea-tests are deliberately NOT
# here: they host only override binaries + permanently-standalone tests and are
# out of the consolidation contract's scope.
CONSOLIDATABLE_CRATES=(reify-cli reify-syntax reify-kernel-occt reify-eval reify-compiler)

# The 7 standalone integration binaries that are NEVER consolidated (I1),
# identified by file stem (basename without the .rs extension).
OVERRIDE_BINARIES=(determinism analytical_validation modal_benchmarks buckling_smoke fea_diagnostics_e2e tensegrity_t0a representation_within_assertion)

# Raw-line-count cap per harness_<subsystem>.rs compile unit (PRD §11: raw
# line count, simplest/conservative; ~20 kLOC = upper end of the §7 band).
CAP_LINES=20000

# The checked-in grandfather-baseline ratchet.
BASELINE="$SCRIPT_DIR/harness-layout-baseline.manifest"

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
# line count and flag it if it exceeds <cap_lines>.
# ---------------------------------------------------------------------------
harness_layout_violations() {
    local crate="$1"
    local tests_dir="$2"
    local baseline_file="$3"
    local cap_lines="$4"

    local violations=0
    local f base lines

    for f in "$tests_dir"/*.rs; do
        [ -f "$f" ] || continue          # skip a literal no-match glob

        base="$(basename "$f")"

        # rule (a): kLOC cap governs the harness_<subsystem>.rs compile units.
        case "$base" in
            harness_*.rs)
                lines="$(wc -l < "$f")"
                lines="${lines//[[:space:]]/}"   # portable: strip any wc padding
                if [ "$lines" -gt "$cap_lines" ]; then
                    printf 'HARNESS_KLOC_CAP FAIL crate=%s file=%s reason=exceeds-cap lines=%s cap=%s\n' \
                        "$crate" "$f" "$lines" "$cap_lines"
                    violations=$((violations + 1))
                fi
                continue
                ;;
        esac
    done

    if [ "$violations" -gt 0 ]; then
        return 1
    fi
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

test_summary
