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
#   C. Each of the 6 expected binary-level atoms is present in the expression.
#   D. Drift-guard: the total count of `package(X) & binary(Y)` atoms parsed out
#      of the expression is exactly 6 (catches silent membership drift -- an
#      extra or missing atom).
#   E. Resolve-to-disk: every parsed (pkg, bin) atom maps to a real
#      crates/<pkg>/tests/<bin>.rs file on disk (a typo'd/renamed/deleted binary
#      becomes a CI failure here, not a silent coverage hole). Assumes the
#      Cargo package name equals its crate directory name under crates/ (true
#      for every heavy package today); see the inline note at Assertion E.
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
# Assertion C: each of the 6 expected atoms is present in the expression
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

# ---------------------------------------------------------------------------
# Assertion D: drift-guard -- exactly 6 package(X) & binary(Y) atoms total
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion D: drift-guard -- exactly 6 binary-level atoms total ---"

# Parse every `package(X) & binary(Y)` atom out of the expression. Guarded with
# `|| true` so a zero-match grep (RED state, filter unset/empty) does not trip
# set -e/pipefail before test_summary can report the FAIL tally.
_ATOMS="$(printf '%s' "${REIFY_HEAVY_NEXTEST_FILTER:-}" | grep -oE 'package\([a-z0-9_-]+\) & binary\([a-z0-9_-]+\)')" || true

_ATOM_COUNT=0
if [ -n "$_ATOMS" ]; then
    _ATOM_COUNT="$(printf '%s\n' "$_ATOMS" | wc -l | tr -d '[:space:]')"
fi

assert "exactly 6 package(X) & binary(Y) atoms parsed from heavy filter (count=${_ATOM_COUNT}; no silent membership drift)" \
    test "$_ATOM_COUNT" -eq 6

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

test_summary
