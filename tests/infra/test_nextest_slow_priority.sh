#!/usr/bin/env bash
# Infrastructure test for task 4627.
# Validates that .config/nextest.toml declares priority overrides for the 5
# heavy-compute test binaries (LPT scheduling to compress the slow tail), that
# the existing occt test-group block coexists, and that scripts/gen-nextest-config.sh
# preserves all overrides verbatim in the generated temp config consumed by nextest.
#
# Assertions:
# STRUCTURE / PRESERVATION (step-1):
#   A. .config/nextest.toml contains at least one [[profile.default.overrides]]
#      block with a `priority` key for each of the 5 slow binaries:
#        package(reify-eval) & binary(tensegrity_t0a)
#        package(reify-eval) & binary(fea_diagnostics_e2e)
#        package(reify-eval) & binary(representation_within_assertion)
#        package(reify-solver-elastic) & binary(analytical_validation)
#        package(reify-solver-elastic) & binary(determinism)
#   B. The existing occt test-group block is still present (coexistence).
#   C. scripts/gen-nextest-config.sh produces a temp config that still contains
#      every priority override AND the occt group (end-to-end preservation;
#      compile-free — gen-nextest-config.sh does not invoke cargo).
#
# DRIFT-GUARD / LPT-ORDERING (step-3):
#   D. For every priority override, the crates/<pkg>/tests/<binary>.rs source file
#      exists on disk (rejects typo'd / dangling filters silently ignored by nextest).
#   E. The straggler binary tensegrity_t0a carries a priority STRICTLY GREATER than
#      each of the other four (enforces longest-first LPT scheduling tier).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

NEXTEST_TOML="$REPO_ROOT/.config/nextest.toml"
GEN_CFG="$REPO_ROOT/scripts/gen-nextest-config.sh"

echo "=== Nextest slow-priority LPT ordering tests (task 4627) ==="

# ---------------------------------------------------------------------------
# Helper: extract `priority = N` integer from a [[profile.default.overrides]]
# block whose filter contains both package(<pkg>) and binary(<bin>).
# Reads from FILE argument; prints the integer or empty string.
# Usage: _priority_for_file <file> <pkg> <binary>
# ---------------------------------------------------------------------------
_priority_for_file() {
    local file="$1" pkg="package(${2})" bin="binary(${3})"
    awk -v pkg="$pkg" -v bin="$bin" '
        /^\[\[/ { in_block = 0 }
        /filter/ && index($0, pkg) && index($0, bin) { in_block = 1 }
        in_block && /^priority[[:space:]]*=/ {
            match($0, /[0-9]+/)
            print substr($0, RSTART, RLENGTH)
            in_block = 0
        }
    ' "$file"
}

# Convenience wrapper for the canonical nextest.toml.
_priority_for() {
    _priority_for_file "$NEXTEST_TOML" "$1" "$2"
}

# ---------------------------------------------------------------------------
# Precompute priority values from nextest.toml (in current shell, not subshell).
# This makes assertions simple test -n / test -gt checks on already-resolved values.
# ---------------------------------------------------------------------------
P_T0A="$(_priority_for reify-eval tensegrity_t0a)"
P_FEA="$(_priority_for reify-eval fea_diagnostics_e2e)"
P_REPR="$(_priority_for reify-eval representation_within_assertion)"
P_ANAL="$(_priority_for reify-solver-elastic analytical_validation)"
P_DET="$(_priority_for reify-solver-elastic determinism)"

# ---------------------------------------------------------------------------
# Assertion A: priority override present for each of the 5 slow binaries
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion A: priority override present for each slow binary ---"

assert "nextest.toml: priority override exists for package(reify-eval) & binary(tensegrity_t0a)" \
    test -n "$P_T0A"

assert "nextest.toml: priority override exists for package(reify-eval) & binary(fea_diagnostics_e2e)" \
    test -n "$P_FEA"

assert "nextest.toml: priority override exists for package(reify-eval) & binary(representation_within_assertion)" \
    test -n "$P_REPR"

assert "nextest.toml: priority override exists for package(reify-solver-elastic) & binary(analytical_validation)" \
    test -n "$P_ANAL"

assert "nextest.toml: priority override exists for package(reify-solver-elastic) & binary(determinism)" \
    test -n "$P_DET"

# ---------------------------------------------------------------------------
# Assertion B: occt test-group block still present (coexistence)
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion B: occt test-group block coexists ---"

assert "nextest.toml: [test-groups] occt block still present (coexistence)" \
    grep -qF 'occt = { max-threads = ' "$NEXTEST_TOML"

assert "nextest.toml: [[profile.default.overrides]] occt test-group filter still present" \
    grep -qF "test-group = 'occt'" "$NEXTEST_TOML"

# ---------------------------------------------------------------------------
# Assertion C: gen-nextest-config.sh preserves all priority overrides and occt group
# (compile-free — gen-nextest-config.sh only runs sed, never cargo)
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion C: gen-nextest-config.sh preserves priority overrides and occt group ---"

# Generate the temp config (compile-free).
_TMP_CFG="$(REIFY_OCCT_NEXTEST_MAX_THREADS=24 bash "$GEN_CFG")"

# Precompute values from the generated config in the current shell.
_C_T0A="$(_priority_for_file "$_TMP_CFG" reify-eval tensegrity_t0a)"
_C_FEA="$(_priority_for_file "$_TMP_CFG" reify-eval fea_diagnostics_e2e)"
_C_REPR="$(_priority_for_file "$_TMP_CFG" reify-eval representation_within_assertion)"
_C_ANAL="$(_priority_for_file "$_TMP_CFG" reify-solver-elastic analytical_validation)"
_C_DET="$(_priority_for_file "$_TMP_CFG" reify-solver-elastic determinism)"

assert "gen-nextest-config.sh: occt test-group still present in generated config" \
    grep -qF 'occt = { max-threads = ' "$_TMP_CFG"

assert "gen-nextest-config.sh: tensegrity_t0a priority override preserved in generated config" \
    test -n "$_C_T0A"

assert "gen-nextest-config.sh: fea_diagnostics_e2e priority override preserved in generated config" \
    test -n "$_C_FEA"

assert "gen-nextest-config.sh: representation_within_assertion priority override preserved in generated config" \
    test -n "$_C_REPR"

assert "gen-nextest-config.sh: analytical_validation priority override preserved in generated config" \
    test -n "$_C_ANAL"

assert "gen-nextest-config.sh: determinism priority override preserved in generated config" \
    test -n "$_C_DET"

rm -f "$_TMP_CFG"

# ---------------------------------------------------------------------------
# Assertion D: drift-guard — each filter package+binary maps to a real test file
# (typo'd/renamed filters would be silent no-ops in nextest; fail here instead)
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion D: drift-guard — filter package+binary names map to real test files ---"

assert "crates/reify-eval/tests/tensegrity_t0a.rs exists on disk (filter not dangling)" \
    test -f "$REPO_ROOT/crates/reify-eval/tests/tensegrity_t0a.rs"

assert "crates/reify-eval/tests/fea_diagnostics_e2e.rs exists on disk (filter not dangling)" \
    test -f "$REPO_ROOT/crates/reify-eval/tests/fea_diagnostics_e2e.rs"

assert "crates/reify-eval/tests/representation_within_assertion.rs exists on disk (filter not dangling)" \
    test -f "$REPO_ROOT/crates/reify-eval/tests/representation_within_assertion.rs"

assert "crates/reify-solver-elastic/tests/analytical_validation.rs exists on disk (filter not dangling)" \
    test -f "$REPO_ROOT/crates/reify-solver-elastic/tests/analytical_validation.rs"

assert "crates/reify-solver-elastic/tests/determinism.rs exists on disk (filter not dangling)" \
    test -f "$REPO_ROOT/crates/reify-solver-elastic/tests/determinism.rs"

# ---------------------------------------------------------------------------
# Assertion E: LPT ordering — tensegrity_t0a priority strictly greater than others
# (enforces longest-first scheduling; straggler must start at t=0)
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion E: LPT ordering — tensegrity_t0a priority > other four binaries ---"

assert "tensegrity_t0a priority (${P_T0A:-unset}) > fea_diagnostics_e2e priority (${P_FEA:-unset})" \
    bash -c "[ -n '${P_T0A:-}' ] && [ -n '${P_FEA:-}' ] && [ '${P_T0A:-0}' -gt '${P_FEA:-0}' ]"

assert "tensegrity_t0a priority (${P_T0A:-unset}) > representation_within_assertion priority (${P_REPR:-unset})" \
    bash -c "[ -n '${P_T0A:-}' ] && [ -n '${P_REPR:-}' ] && [ '${P_T0A:-0}' -gt '${P_REPR:-0}' ]"

assert "tensegrity_t0a priority (${P_T0A:-unset}) > analytical_validation priority (${P_ANAL:-unset})" \
    bash -c "[ -n '${P_T0A:-}' ] && [ -n '${P_ANAL:-}' ] && [ '${P_T0A:-0}' -gt '${P_ANAL:-0}' ]"

assert "tensegrity_t0a priority (${P_T0A:-unset}) > determinism priority (${P_DET:-unset})" \
    bash -c "[ -n '${P_T0A:-}' ] && [ -n '${P_DET:-}' ] && [ '${P_T0A:-0}' -gt '${P_DET:-0}' ]"

test_summary
