#!/usr/bin/env bash
# tests/infra/test_harness_baseline_registration_gate.sh
#
# Guards the DIFF-SCOPED harness-layout baseline-registration drift gate
# (task #5300). Companion to tests/infra/test_harness_kloc_cap.sh (task 5265,
# the WHOLE-TREE anti-re-accretion kLOC-cap guard) and structured like
# tests/infra/test_infra_classification_manifest_gate.sh (task 5252, the early
# classification-manifest gate).
#
# WHY A SECOND, DIFF-SCOPED GATE. test_harness_kloc_cap.sh Section 5 is a
# WHOLE-TREE live scan: once a new standalone crates/<c>/tests/<f>.rs lands on
# main WITHOUT a matching harness-layout-baseline.manifest row (the task 4370
# drift), that scan goes RED for EVERY unrelated downstream task whose
# post-merge verify merely rebased onto the drifted tip — repeated merge thrash
# (5260/5266/5288), each re-diagnosing the same root cause. A whole-tree guard
# cannot tell "your diff introduced the drift" from "you rebased onto an
# already-drifted main". This gate closes the gap AT ITS SOURCE: it considers
# ONLY files ADDED in THIS diff, so a rebaser (whose diff adds no test file)
# stays GREEN while the offending diff (adds file, omits row) is blocked.
#
# WHAT IS TESTED (built up across the task's TDD steps):
#   - the shared lib tests/infra/harness-layout-lib.sh: the 5 consolidatable
#     crates, 7 override stems, the in-scope-standalone predicate, and the
#     baseline-membership predicate (this file, step-1);
#   - the gate scripts/check-harness-baseline-registration.sh in args/stdin
#     input mode (step-3) and --from-git self-derivation mode (step-5);
#   - verify.sh plan-shape: the gate is emitted early under RUN_RUST=1 (step-7).
#
# Hermetic: pure bash + filesystem (+ throwaway `git init` temp repos for the
# --from-git cases); never runs cargo/npm; never mutates the real baseline or
# the real manifest. Auto-discovered by tests/infra/run_all.sh via the
# test_*.sh glob; classified `pool` in run-all-classification.manifest.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

# The shared lib + the gate script under test. Deliberately NOT sourced at the
# top level: this file is RED (lib absent) before the impl step, and a top-level
# `source` of a missing file would abort under `set -e` before any assert runs.
# Every lib interaction below therefore happens inside a `bash -c 'source ...'`
# child shell, so a missing lib fails just THAT assert (mirrors
# test_infra_classification_manifest_gate.sh section (a)).
LIB="$SCRIPT_DIR/harness-layout-lib.sh"
GATE="$REPO_ROOT/scripts/check-harness-baseline-registration.sh"

echo "=== harness-layout baseline-registration drift gate tests (task 5300) ==="

# Single EXIT-trap over an array of fixtures (the test_harness_kloc_cap.sh
# idiom): individual `trap ... EXIT` calls replace one another, so one handler
# over an array removes every fixture regardless of which section adds the last.
_TMPDIRS=()
trap '[ "${#_TMPDIRS[@]}" -gt 0 ] && rm -rf "${_TMPDIRS[@]}"' EXIT

# ===========================================================================
# Section A: the shared lib exists and is sourceable.
# ===========================================================================
echo ""
echo "--- Section A: harness-layout-lib.sh exists and is sourceable ---"

assert "A: tests/infra/harness-layout-lib.sh exists" \
    test -f "$LIB"
assert "A: harness-layout-lib.sh sources cleanly (rc 0)" \
    bash -c 'source "$1"' _ "$LIB"

# ===========================================================================
# Section B: harness_layout_consolidatable_crates prints EXACTLY the 5 crates.
# ===========================================================================
echo ""
echo "--- Section B: consolidatable crates (exactly the 5) ---"

# Expected set, sorted+comma-joined for an order-independent EXACT comparison.
_B_EXPECT="reify-cli,reify-compiler,reify-eval,reify-kernel-occt,reify-syntax,"

assert "B: consolidatable crates set is EXACTLY the 5" \
    bash -c '
        source "$1"
        got="$(harness_layout_consolidatable_crates | sort | tr "\n" ",")"
        [ "$got" = "$2" ]
    ' _ "$LIB" "$_B_EXPECT"
assert "B: consolidatable crates count is exactly 5" \
    bash -c 'source "$1"; [ "$(harness_layout_consolidatable_crates | grep -c .)" -eq 5 ]' _ "$LIB"

# ===========================================================================
# Section C: harness_layout_override_stems prints EXACTLY the 7 override stems.
# ===========================================================================
echo ""
echo "--- Section C: override stems (exactly the 7) ---"

_C_EXPECT="analytical_validation,buckling_smoke,determinism,fea_diagnostics_e2e,modal_benchmarks,representation_within_assertion,tensegrity_t0a,"

assert "C: override stems set is EXACTLY the 7" \
    bash -c '
        source "$1"
        got="$(harness_layout_override_stems | sort | tr "\n" ",")"
        [ "$got" = "$2" ]
    ' _ "$LIB" "$_C_EXPECT"
assert "C: override stems count is exactly 7" \
    bash -c 'source "$1"; [ "$(harness_layout_override_stems | grep -c .)" -eq 7 ]' _ "$LIB"

# ===========================================================================
# Section D: harness_layout_in_scope_standalone predicate.
# In-scope iff crates/<one-of-5>/tests/<base>.rs, top-level, base not harness_*
# and stem not an override.
# ===========================================================================
echo ""
echo "--- Section D: in-scope-standalone predicate ---"

assert "D: a plain standalone in a consolidatable crate is IN scope (rc 0)" \
    bash -c 'source "$1"; harness_layout_in_scope_standalone "crates/reify-eval/tests/foo.rs"' _ "$LIB"
assert "D: a harness_*.rs unit is NOT in scope (sanctioned harness)" \
    bash -c '! { source "$1"; harness_layout_in_scope_standalone "crates/reify-eval/tests/harness_x.rs"; }' _ "$LIB"
assert "D: an override stem (tensegrity_t0a) is NOT in scope" \
    bash -c '! { source "$1"; harness_layout_in_scope_standalone "crates/reify-eval/tests/tensegrity_t0a.rs"; }' _ "$LIB"
assert "D: a non-consolidatable crate (reify-solver-elastic) is NOT in scope" \
    bash -c '! { source "$1"; harness_layout_in_scope_standalone "crates/reify-solver-elastic/tests/foo.rs"; }' _ "$LIB"
assert "D: a NESTED (non-top-level) tests path is NOT in scope" \
    bash -c '! { source "$1"; harness_layout_in_scope_standalone "crates/reify-eval/tests/sub/foo.rs"; }' _ "$LIB"
assert "D: a non-tests path (src/) is NOT in scope" \
    bash -c '! { source "$1"; harness_layout_in_scope_standalone "crates/reify-eval/src/foo.rs"; }' _ "$LIB"

# ===========================================================================
# Section E: harness_layout_baseline_contains predicate (comment/blank stripped
# exact-line membership against a fixture baseline).
# ===========================================================================
echo ""
echo "--- Section E: baseline-membership predicate ---"

_E_BASELINE="$(mktemp)"; _TMPDIRS+=("$_E_BASELINE")
{
    echo "# a comment line — must be ignored"
    echo ""
    echo "crates/reify-eval/tests/listed.rs"
    echo "crates/reify-cli/tests/other.rs"
} > "$_E_BASELINE"

assert "E: a listed path is a member (rc 0)" \
    bash -c 'source "$1"; harness_layout_baseline_contains "crates/reify-eval/tests/listed.rs" "$2"' _ "$LIB" "$_E_BASELINE"
assert "E: an unlisted path is NOT a member (rc non-zero)" \
    bash -c '! { source "$1"; harness_layout_baseline_contains "crates/reify-eval/tests/missing.rs" "$2"; }' _ "$LIB" "$_E_BASELINE"
# A path that appears ONLY inside a comment line must NOT count as a member
# (proves comment stripping, not a raw substring/grep-anywhere match).
assert "E: a path present only as a comment is NOT a member" \
    bash -c '! { source "$1"; harness_layout_baseline_contains "a comment line — must be ignored" "$2"; }' _ "$LIB" "$_E_BASELINE"

test_summary
