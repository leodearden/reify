#!/usr/bin/env bash
# Infrastructure test for task 4912 (PRD docs/prds/offline-deep-test-lane.md §3/DA4).
#
# Validates that scripts/heavy-test-filter-lib.sh is the SINGLE SOURCE OF TRUTH
# for the `heavy` nextest filterset -- the offline/gate test partition consumed
# by the `offline` role (A2: `-E "$heavy" --run-ignored all`) and the knob-gated
# gate exclusion (A4: `-E "not ($heavy)"` iff REIFY_GATE_EXCLUDE_HEAVY=1).
#
# Assertions:
#   A. scripts/heavy-test-filter-lib.sh exists on disk.
#   B. REIFY_HEAVY_NEXTEST_FILTER is defined and non-empty once the lib is sourced.
#   C. Each of the 8 expected binary-level atoms is present in the expression
#      (task 6368 added the 7th, a test-scoped atom on harness_fea_solver_e2e).
#   D. Drift-guard: the total count of `package(X) & binary(Y)` atoms parsed out
#      of the expression is exactly 8 (catches silent membership drift -- an
#      extra or missing atom). The parser matches only the `package()`/`binary()`
#      prefix of an atom, so the 7th atom's trailing `& test(...)` clause does
#      not change how it is counted here.
#   E. Resolve-to-disk: every parsed (pkg, bin) atom maps to a real
#      crates/<pkg>/tests/<bin>.rs file on disk (a typo'd/renamed/deleted binary
#      becomes a CI failure here, not a silent coverage hole). Assumes the
#      Cargo package name equals its crate directory name under crates/ (true
#      for every heavy package today); see the inline note at Assertion E.
#   F. Test-scope clause, announce-or-assert: Assertions C-E all stop at the
#      `package(X) & binary(Y)` prefix, so the 7th atom's DISCRIMINATING clause
#      -- `& test(/^<stem>::/)`, the thing that makes it move one test instead
#      of a whole 247-test binary -- is unresolved by any of them. F parses
#      <stem> back out and resolves it one level deeper, to the `#[path]`
#      submodule file. It deliberately does NOT hard-fail on absence: the stem
#      is legitimately absent today (task #4880 is unlanded), so a bare
#      existence assert would be born RED. Instead it ANNOUNCES the inertness
#      on every run while the file is missing, and the moment the file appears
#      it upgrades to a hard assert that the harness root declares
#      `mod <stem>;` -- without that declaration the `<file>::<test>` module
#      path the harness's own doc-comment contract promises does not exist, the
#      filterset matches nothing, and the exclusion is silently dead.
#      Non-vacuity: both branches are exercised against REAL on-disk siblings
#      every run, so the machinery cannot rot while its live atom is inert.
#   G. Gate residency (task #6630): a named set of cheap regression guards must
#      NOT live in any heavy-gated submodule. A test-scoped atom is a
#      SUBMODULE-granularity eviction -- it sweeps every test in that stem off
#      the task/merge gate into the offline-only lane -- so a cheap guard that
#      merely happens to share a file with an expensive test silently loses its
#      gate coverage, which is what #6630 found and fixed. G re-walks the SAME
#      `$_TEST_SCOPED` atom list F parses (so it tracks future atom additions
#      with no edit here) and asserts, for every heavy stem that resolves to a
#      real file, that the file declares NONE of `_GATE_RESIDENT_TESTS` (G1).
#      G2 is the non-vacuity counterweight: each named test must be declared in
#      EXACTLY ONE file under the harness module dir, and that file's stem must
#      be `mod`-declared in the harness root -- without it, deleting or
#      renaming the tests would satisfy G1 vacuously and silently re-open the
#      very coverage hole #6630 closed (an undeclared member compiles into
#      nothing, the same hazard rule (d) of tests/infra/test_harness_kloc_cap.sh
#      mechanizes). G3 guards G1 against iterating an empty name list.
#
# Compile-free -- this test never invokes cargo.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

LIB="$REPO_ROOT/scripts/heavy-test-filter-lib.sh"

echo "=== heavy nextest filterset single-source-of-truth tests (task 4912) ==="

# ---------------------------------------------------------------------------
# Assertion A: the lib exists on disk
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion A: scripts/heavy-test-filter-lib.sh exists on disk ---"

assert "scripts/heavy-test-filter-lib.sh exists on disk" \
    test -f "$LIB"

# Guarded source -- safe under set -e even when the lib is absent (RED state):
# the left side of the && short-circuits without executing `source`, and a
# failing left-of-&& command does not itself trigger set -e.
[ -f "$LIB" ] && source "$LIB"

# ---------------------------------------------------------------------------
# Assertion B: REIFY_HEAVY_NEXTEST_FILTER is defined and non-empty
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion B: REIFY_HEAVY_NEXTEST_FILTER is defined and non-empty ---"

assert "REIFY_HEAVY_NEXTEST_FILTER is defined and non-empty" \
    test -n "${REIFY_HEAVY_NEXTEST_FILTER:-}"

# ---------------------------------------------------------------------------
# Assertion C: each of the 8 expected atoms is present in the expression
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion C: expected atoms present in the heavy filter ---"

_atom_present() {
    case "${REIFY_HEAVY_NEXTEST_FILTER:-}" in
        *"$1"*) return 0 ;;
        *) return 1 ;;
    esac
}

assert "heavy filter contains package(reify-solver-elastic) & binary(determinism)" \
    _atom_present "package(reify-solver-elastic) & binary(determinism)"

assert "heavy filter contains package(reify-solver-elastic) & binary(analytical_validation)" \
    _atom_present "package(reify-solver-elastic) & binary(analytical_validation)"

assert "heavy filter contains package(reify-solver-elastic) & binary(modal_benchmarks)" \
    _atom_present "package(reify-solver-elastic) & binary(modal_benchmarks)"

assert "heavy filter contains package(reify-eval-fea-tests) & binary(buckling_smoke)" \
    _atom_present "package(reify-eval-fea-tests) & binary(buckling_smoke)"

assert "heavy filter contains package(reify-eval) & binary(tensegrity_t0a)" \
    _atom_present "package(reify-eval) & binary(tensegrity_t0a)"

assert "heavy filter contains package(reify-eval-fea-tests) & binary(fea_diagnostics_e2e)" \
    _atom_present "package(reify-eval-fea-tests) & binary(fea_diagnostics_e2e)"

assert "heavy filter contains package(reify-eval) & binary(harness_fea_solver_e2e) & test(/^fea_in_the_loop_producer::/) (task 6368)" \
    _atom_present "package(reify-eval) & binary(harness_fea_solver_e2e) & test(/^fea_in_the_loop_producer::/)"

assert "heavy filter contains package(reify-eval) & binary(harness_fea_solver_e2e) & test(/^fea_bracket_minimize_mass_e2e::/) (task 2930)" \
    _atom_present "package(reify-eval) & binary(harness_fea_solver_e2e) & test(/^fea_bracket_minimize_mass_e2e::/)"

# ---------------------------------------------------------------------------
# Assertion D: drift-guard -- exactly 8 package(X) & binary(Y) atoms total
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion D: drift-guard -- exactly 8 binary-level atoms total ---"

# Parse every `package(X) & binary(Y)` atom out of the expression. Guarded with
# `|| true` so a zero-match grep (RED state, filter unset/empty) does not trip
# set -e/pipefail before test_summary can report the FAIL tally.
_ATOMS="$(printf '%s' "${REIFY_HEAVY_NEXTEST_FILTER:-}" | grep -oE 'package\([a-z0-9_-]+\) & binary\([a-z0-9_-]+\)')" || true

_ATOM_COUNT=0
if [ -n "$_ATOMS" ]; then
    _ATOM_COUNT="$(printf '%s\n' "$_ATOMS" | wc -l | tr -d '[:space:]')"
fi

assert "exactly 8 package(X) & binary(Y) atoms parsed from heavy filter (count=${_ATOM_COUNT}; no silent membership drift)" \
    test "$_ATOM_COUNT" -eq 8

# ---------------------------------------------------------------------------
# Assertion E: resolve-to-disk -- every parsed atom maps to a real test file
# ---------------------------------------------------------------------------
# NOTE (maintainability caveat, not a current bug): this lookup assumes the
# Cargo package name is identical to its crate directory name under crates/
# (i.e. `crates/<pkg>/` where <pkg> is exactly the package() atom's
# argument). That holds for every heavy package today (reify-solver-elastic,
# reify-eval) but Cargo does not guarantee it in general -- nextest's
# package() atom matches the *Cargo package name*, not the directory, so a
# future heavy package whose directory diverges from its package name would
# false-fail this assertion even though the nextest filter itself is correct.
# If that ever happens, resolve the path via `cargo metadata --no-deps`
# (manifest_path keyed by package name) instead of assuming crates/<pkg>/.
# This test is deliberately compile-free (see file header), so the
# directory-name assumption is a tradeoff, not an oversight.
echo ""
echo "--- Assertion E: resolve-to-disk -- every parsed atom maps to a real test file ---"

if [ -n "$_ATOMS" ]; then
    _atom_re='^package\(([a-z0-9_-]+)\) & binary\(([a-z0-9_-]+)\)$'
    while IFS= read -r _atom; do
        [ -n "$_atom" ] || continue
        if [[ "$_atom" =~ $_atom_re ]]; then
            _pkg="${BASH_REMATCH[1]}"
            _bin="${BASH_REMATCH[2]}"
            assert "crates/$_pkg/tests/$_bin.rs exists on disk (heavy filter atom not dangling)" \
                test -f "$REPO_ROOT/crates/$_pkg/tests/$_bin.rs"
        fi
    done <<< "$_ATOMS"
else
    echo "  (skipped: no atoms parsed -- see Assertion D)"
fi

# ---------------------------------------------------------------------------
# Assertion F: test-scope clause -- announce-or-assert the submodule stem
# ---------------------------------------------------------------------------
# WHY THIS IS NOT JUST ANOTHER `test -f`. Assertion E resolves an atom only to
# `crates/<pkg>/tests/<bin>.rs`, which for the 7th atom is the CONSOLIDATED
# harness root (harness_fea_solver_e2e.rs, task #5281) -- a file that exists
# and will keep existing no matter what happens to the one submodule the atom
# actually targets. So E is structurally incapable of noticing that the
# discriminating `& test(/^<stem>::/)` clause matches zero tests.
#
# The failure mode that opens: nextest's test() filterset is a REGEX MATCH, and
# a regex that matches nothing is not an error -- it is an empty set. A stem
# that never lands, or lands spelled differently, therefore fails OPEN: the
# heavy set silently loses a member, the ~490s test it was meant to move stays
# on the merge gate, and Assertions C/D/E all stay green because the atom's
# TEXT is still present and its binary still resolves.
#
# So this assertion resolves one level deeper -- to the `#[path]`-included
# submodule file -- and it does so in the only shape that is honest about the
# current state of the tree:
#   file ABSENT  -> ANNOUNCE (not fail). The stem is legitimately absent today
#                   (task #4880 unlanded), so a hard assert would be born RED
#                   and would be silenced rather than fixed. The banner makes
#                   the inertness a fact printed on every run instead of a
#                   claim buried in a comment.
#   file PRESENT -> ASSERT the harness root declares `mod <stem>;`. That is the
#                   harness's own documented contract ("so its <file>::<test>
#                   module path -- and thus every test(/^<file>::/) filterset --
#                   resolves unchanged"), and it is exactly what makes the
#                   filterset non-empty. A submodule file that exists but is
#                   never declared compiles into nothing and matches nothing.
# The branch flips by itself the moment #4880 lands: no edit here required.
echo ""
echo "--- Assertion F: test-scope clause -- announce-or-assert the submodule stem ---"

# _submodule_file_exists <pkg> <bin> <stem> -- 0 iff the `#[path]`-included
# submodule file crates/<pkg>/tests/<bin>/<stem>.rs exists on disk.
_submodule_file_exists() {
    test -f "$REPO_ROOT/crates/$1/tests/$2/$3.rs"
}

# _submodule_mod_declared <pkg> <bin> <stem> -- 0 iff the harness root
# crates/<pkg>/tests/<bin>.rs declares `mod <stem>;`. Anchored at column 0
# because that is how the harness root writes them (the `#[path = "..."]`
# attribute sits on the preceding line); a nested/indented `mod` inside some
# other module would NOT produce the top-level `<stem>::` path prefix the
# filterset needs, so matching it would be a false PASS. <stem> comes from
# this file's own [a-z0-9_]+ parser, so it carries no regex metacharacters.
_submodule_mod_declared() {
    grep -qE "^mod ${3};[[:space:]]*$" "$REPO_ROOT/crates/$1/tests/$2.rs"
}

# Parse the full (pkg, bin, stem) triple out of every TEST-SCOPED atom.
# Deliberately a separate parse from Assertion D's: that one matches the
# `package()/binary()` prefix of ALL atoms (and so must stay stem-blind),
# this one matches ONLY atoms that carry a trailing test() clause.
_TEST_SCOPED="$(printf '%s' "${REIFY_HEAVY_NEXTEST_FILTER:-}" | grep -oE 'package\([a-z0-9_-]+\) & binary\([a-z0-9_-]+\) & test\(/\^[a-z0-9_]+::/\)')" || true

# Non-vacuity guard. Without this, deleting the ` & test(...)` clause from the
# lib -- which converts the atom to whole-binary and evicts all 247 tests from
# the gate to relieve one, the exact outcome esc-6368-2 rescoped AWAY from --
# would leave this assertion silently iterating over an empty list and
# reporting nothing at all.
assert "at least 1 test-scoped atom parsed from the heavy filter (a dropped '& test(...)' clause would silently widen an atom to its whole binary)" \
    test -n "$_TEST_SCOPED"

if [ -n "$_TEST_SCOPED" ]; then
    _ts_re='^package\(([a-z0-9_-]+)\) & binary\(([a-z0-9_-]+)\) & test\(/\^([a-z0-9_]+)::/\)$'
    while IFS= read -r _ts; do
        [ -n "$_ts" ] || continue
        if [[ "$_ts" =~ $_ts_re ]]; then
            _pkg="${BASH_REMATCH[1]}"
            _bin="${BASH_REMATCH[2]}"
            _stem="${BASH_REMATCH[3]}"
            if _submodule_file_exists "$_pkg" "$_bin" "$_stem"; then
                assert "crates/$_pkg/tests/$_bin.rs declares 'mod $_stem;' (so test(/^$_stem::/) resolves to a real module path, not the empty set)" \
                    _submodule_mod_declared "$_pkg" "$_bin" "$_stem"
            else
                echo "  ANNOUNCE: atom 'package($_pkg) & binary($_bin) & test(/^$_stem::/)' is INERT."
                echo "  ANNOUNCE:   crates/$_pkg/tests/$_bin/$_stem.rs is ABSENT on disk (pending task #4880)."
                echo "  ANNOUNCE:   The clause matches ZERO tests today, so the ~490s producer test it targets"
                echo "  ANNOUNCE:   is NOT yet excluded from the merge gate. This is expected and is not a"
                echo "  ANNOUNCE:   failure. When #4880 lands, this banner MUST disappear and the 'mod $_stem;'"
                echo "  ANNOUNCE:   assertion MUST take its place. A banner that survives #4880 landing means"
                echo "  ANNOUNCE:   the stem drifted and the exclusion is silently dead -- fix the clause in"
                echo "  ANNOUNCE:   scripts/heavy-test-filter-lib.sh, do not silence this line."
            fi
        fi
    done <<< "$_TEST_SCOPED"
fi

# Non-vacuity self-check for the machinery ABOVE. Today the live atom takes the
# ANNOUNCE branch, which asserts nothing -- so without this, both resolvers
# could be broken (wrong path shape, wrong anchor, inverted return) and this
# whole assertion would still look healthy, right up until #4880 lands and
# quietly took the wrong branch. So drive the SAME two helpers against real
# on-disk data: a genuine sibling submodule of the same harness (which must
# resolve AND be declared) and a stem that cannot exist (which must not).
# Mirrors this suite's sibling guard tests/infra/test_verify_offline_partition.sh
# (assert_guard_rejects) and tests/infra/test_run_all_classification.sh.
_submodule_resolvers_work() {
    local _pkg="$1" _bin="$2" _real
    # First real sibling submodule, by name, of the same consolidated harness.
    _real="$(ls -1 "$REPO_ROOT/crates/$_pkg/tests/$_bin/" 2>/dev/null \
        | grep -E '^[a-z0-9_]+\.rs$' | LC_ALL=C sort | head -n1)" || return 1
    [ -n "$_real" ] || return 1
    _real="${_real%.rs}"
    # (1) a REAL sibling must be seen as present AND as declared -- proves the
    #     path shape and the `^mod <stem>;` anchor both match reality.
    _submodule_file_exists "$_pkg" "$_bin" "$_real" || return 1
    _submodule_mod_declared "$_pkg" "$_bin" "$_real" || return 1
    # (2) a stem that cannot exist must be seen as neither -- proves the
    #     ANNOUNCE branch is reached by absence, not by a broken resolver, and
    #     that a bogus stem could never false-PASS the mod-declaration assert.
    if _submodule_file_exists "$_pkg" "$_bin" "nonexistent_stem_zzz"; then return 1; fi
    if _submodule_mod_declared "$_pkg" "$_bin" "nonexistent_stem_zzz"; then return 1; fi
    return 0
}

if [ -n "$_TEST_SCOPED" ] && [[ "$(printf '%s\n' "$_TEST_SCOPED" | head -n1)" =~ $_ts_re ]]; then
    assert "self-check: the submodule resolvers accept a REAL sibling of crates/${BASH_REMATCH[1]}/tests/${BASH_REMATCH[2]}/ and reject a nonexistent stem (the ANNOUNCE branch above is inertness, not a broken resolver)" \
        _submodule_resolvers_work "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}"
else
    echo "  (self-check skipped: no test-scoped atom parsed -- see the assertion above)"
fi

# ---------------------------------------------------------------------------
# Assertion G: gate residency of the named regression guards (task #6630)
# ---------------------------------------------------------------------------
# Assertion F proves a test-scoped atom RESOLVES. This one is about what that
# resolution COSTS the tests it catches by association.
#
# A test-scoped atom is submodule-granular, not test-granular: `test(/^<stem>::/)`
# evicts EVERY test in that stem from the task/merge gate (verify.sh:740's
# `-E "not ($heavy)"` under REIFY_GATE_EXCLUDE_HEAVY=1) and re-homes it on the
# asynchronous offline lane (verify.sh:751). That is correct and deliberate for
# the expensive test the atom was written for -- but any CHEAP test that merely
# shares the file rides along and silently loses its gate coverage. That is not
# hypothetical: task #5025's two ~0.7s edit-path dispatch guards sat inside
# `fea_in_the_loop_producer` alongside the ~490s producer, so the 7th atom swept
# them off the gate; `nextest list -E "not ($heavy)"` matched ZERO of them.
# Task #6630 moved them to a sibling stem outside the atom. This assertion is
# the standing guard that keeps them there -- re-homing them back into any
# heavy-gated stem must be a FAILURE here, not a silent regression.
echo ""
echo "--- Assertion G: gate residency of the named regression guards (task #6630) ---"

# The tests that MUST stay on the task/merge gate, by exact fn name. Explicit
# and commented so adding a future one is a single-line edit. Membership rule:
# a test belongs here if it is cheap enough to afford on the gate AND guards a
# regression whose feedback is worthless if it only arrives on the async lane.
_GATE_RESIDENT_TESTS=(
    # Task #5025 edit-path optimized-compute dispatch guards; 1.554s / 1.864s
    # measured 2026-09-04 against a 233-test binary whose per-test mean is
    # ~2.25s -- i.e. cheaper than average, hence unambiguously gate-affordable.
    edit_source_dispatches_optimized_compute_into_solver_cost_loop
    edit_param_dispatches_optimized_compute_into_solver_cost_loop
)

# _declares_test_fn <file> <fn> -- 0 iff <file> declares `fn <fn>(` at column 0.
# Anchored at column 0 for the same reason _submodule_mod_declared is: that is
# how the tree writes free test fns (the `#[test]` attribute sits on the
# PRECEDING line, exactly as `#[path = "..."]` precedes `mod`). Anchoring also
# keeps the many DOC-COMMENT mentions of these names -- `//!`/`///` lines that
# cross-reference them -- from false-matching, which an unanchored grep would.
# <fn> comes from the literal array above and carries no regex metacharacters.
_declares_test_fn() {
    grep -qE "^fn ${2}\(" "$1"
}

# Negation helper: `assert` invokes its predicate as a command, so the negative
# half needs a real command that SUCCEEDS on absence rather than a `!` prefix.
_lacks_test_fn() {
    ! _declares_test_fn "$1" "$2"
}

# (G3) Non-vacuity for G1's inner loop. Mirrors Assertion F's own "at least 1
# test-scoped atom parsed" guard: an emptied list would leave G1 silently
# iterating over nothing and reporting a clean bill of health.
assert "at least 1 gate-resident test name is listed (an emptied _GATE_RESIDENT_TESTS would make the residency check below silently vacuous)" \
    test "${#_GATE_RESIDENT_TESTS[@]}" -gt 0

# (G1) NEGATIVE: no heavy-gated submodule file may declare a gate-resident test.
# Derives its file set from `$_TEST_SCOPED` -- the same list Assertion F walks,
# parsed once from $REIFY_HEAVY_NEXTEST_FILTER -- so a future test-scoped atom
# is covered here automatically and the guard cannot drift from the atom list it
# guards. Absent files are skipped (an inert atom evicts nothing; F announces it).
if [ -n "$_TEST_SCOPED" ]; then
    while IFS= read -r _g_ts; do
        [ -n "$_g_ts" ] || continue
        if [[ "$_g_ts" =~ $_ts_re ]]; then
            _g_pkg="${BASH_REMATCH[1]}"
            _g_bin="${BASH_REMATCH[2]}"
            _g_stem="${BASH_REMATCH[3]}"
            _g_file="$REPO_ROOT/crates/$_g_pkg/tests/$_g_bin/$_g_stem.rs"
            [ -f "$_g_file" ] || continue
            for _g_fn in "${_GATE_RESIDENT_TESTS[@]}"; do
                assert "crates/$_g_pkg/tests/$_g_bin/$_g_stem.rs must NOT declare $_g_fn -- the test-scoped heavy atom 'package($_g_pkg) & binary($_g_bin) & test(/^$_g_stem::/)' sweeps that whole stem off the task/merge gate into the offline-only lane (task #6630)" \
                    _lacks_test_fn "$_g_file" "$_g_fn"
            done
        fi
    done <<< "$_TEST_SCOPED"
fi

# (G2) POSITIVE / non-vacuity: each named test must still EXIST, exactly once,
# in a member the harness root actually compiles. Without this, deleting or
# renaming the two tests would satisfy G1 trivially and drop the coverage this
# assertion exists to protect, with the suite still green. Two counts matter:
# exactly-1 declaring file (0 = gone/renamed; >1 = a duplicate that makes "which
# one runs on the gate" ambiguous), and that file being `mod`-declared -- an
# undeclared member compiles into nothing and runs nowhere.
_G_MODULE_DIR="$REPO_ROOT/crates/reify-eval/tests/harness_fea_solver_e2e"
for _g_fn in "${_GATE_RESIDENT_TESTS[@]}"; do
    _g_hits=()
    while IFS= read -r _g_cand; do
        [ -n "$_g_cand" ] || continue
        if _declares_test_fn "$_g_cand" "$_g_fn"; then
            _g_hits+=("$_g_cand")
        fi
    done < <(find "$_G_MODULE_DIR" -maxdepth 1 -type f -name '*.rs' | LC_ALL=C sort)

    assert "exactly 1 file under crates/reify-eval/tests/harness_fea_solver_e2e/ declares $_g_fn (found ${#_g_hits[@]}) -- 0 means the gate-resident test was deleted or renamed, >1 means a duplicate (task #6630)" \
        test "${#_g_hits[@]}" -eq 1

    if [ "${#_g_hits[@]}" -eq 1 ]; then
        _g_home="$(basename "${_g_hits[0]}" .rs)"
        assert "crates/reify-eval/tests/harness_fea_solver_e2e.rs declares 'mod $_g_home;', the submodule that houses $_g_fn (an undeclared member compiles into nothing and runs on no lane at all)" \
            _submodule_mod_declared reify-eval harness_fea_solver_e2e "$_g_home"
    fi
done

test_summary
