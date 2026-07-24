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
# line count and flag it if it exceeds <cap_lines>.
# ---------------------------------------------------------------------------
harness_layout_violations() {
    local crate="$1"
    local tests_dir="$2"
    local baseline_file="$3"
    local cap_lines="$4"

    local violations=0
    local f base lines key

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
                lines="$(wc -l < "$f")"
                lines="${lines//[[:space:]]/}"   # portable: strip any wc padding
                if [ "$lines" -gt "$cap_lines" ]; then
                    _emit FAIL "crate=$crate" "file=$f" "reason=exceeds-cap" \
                        "lines=$lines" "cap=$cap_lines"
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

test_summary
