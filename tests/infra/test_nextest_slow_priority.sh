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
# Helpers
# ---------------------------------------------------------------------------

# Extract the `priority = N` value from a [[profile.default.overrides]] block
# whose filter contains the given package+binary pattern.
# Prints the integer value, or empty string if not found.
# Usage: _priority_for <pkg> <binary>
_priority_for() {
    local pkg="$1" bin="$2"
    # We search for a block whose filter line contains both package(pkg) and
    # binary(bin).  TOML blocks are separated by blank lines or new [[...]]
    # headers.  We use awk to find a matching filter, then look forward for
    # `priority = N` in the same block (before the next [[...]] or EOF).
    awk -v pkg="package(${pkg})" -v bin="binary(${bin})" '
        /^\[\[/ { in_block = 0 }
        /filter/ && index($0, pkg) && index($0, bin) { in_block = 1 }
        in_block && /^priority[[:space:]]*=/ {
            match($0, /[0-9]+/)
            print substr($0, RSTART, RLENGTH)
            in_block = 0
        }
    ' "$NEXTEST_TOML"
}

# Same as above but on an arbitrary file path (used for gen-config preservation).
_priority_for_file() {
    local file="$1" pkg="$2" bin="$3"
    awk -v pkg="package(${pkg})" -v bin="binary(${bin})" '
        /^\[\[/ { in_block = 0 }
        /filter/ && index($0, pkg) && index($0, bin) { in_block = 1 }
        in_block && /^priority[[:space:]]*=/ {
            match($0, /[0-9]+/)
            print substr($0, RSTART, RLENGTH)
            in_block = 0
        }
    ' "$file"
}

# ---------------------------------------------------------------------------
# Assertion A: priority override present for each of the 5 slow binaries
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion A: priority override present for each slow binary ---"

# reify-eval :: tensegrity_t0a
assert "nextest.toml: priority override exists for package(reify-eval) & binary(tensegrity_t0a)" \
    bash -c "[ -n \"\$(_priority_for reify-eval tensegrity_t0a)\" ]"

# reify-eval :: fea_diagnostics_e2e
assert "nextest.toml: priority override exists for package(reify-eval) & binary(fea_diagnostics_e2e)" \
    bash -c "[ -n \"\$(_priority_for reify-eval fea_diagnostics_e2e)\" ]"

# reify-eval :: representation_within_assertion
assert "nextest.toml: priority override exists for package(reify-eval) & binary(representation_within_assertion)" \
    bash -c "[ -n \"\$(_priority_for reify-eval representation_within_assertion)\" ]"

# reify-solver-elastic :: analytical_validation
assert "nextest.toml: priority override exists for package(reify-solver-elastic) & binary(analytical_validation)" \
    bash -c "[ -n \"\$(_priority_for reify-solver-elastic analytical_validation)\" ]"

# reify-solver-elastic :: determinism
assert "nextest.toml: priority override exists for package(reify-solver-elastic) & binary(determinism)" \
    bash -c "[ -n \"\$(_priority_for reify-solver-elastic determinism)\" ]"

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
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion C: gen-nextest-config.sh preserves priority overrides and occt group ---"

# Generate the temp config (compile-free — gen-nextest-config.sh only does sed).
_TMP_CFG="$(REIFY_OCCT_NEXTEST_MAX_THREADS=24 bash "$GEN_CFG")"

assert "gen-nextest-config.sh: occt test-group still present in generated config" \
    bash -c "grep -qF 'occt = { max-threads = ' '$_TMP_CFG'"

assert "gen-nextest-config.sh: tensegrity_t0a priority override preserved in generated config" \
    bash -c "[ -n \"\$(_priority_for_file '$_TMP_CFG' reify-eval tensegrity_t0a)\" ]"

assert "gen-nextest-config.sh: fea_diagnostics_e2e priority override preserved in generated config" \
    bash -c "[ -n \"\$(_priority_for_file '$_TMP_CFG' reify-eval fea_diagnostics_e2e)\" ]"

assert "gen-nextest-config.sh: representation_within_assertion priority override preserved in generated config" \
    bash -c "[ -n \"\$(_priority_for_file '$_TMP_CFG' reify-eval representation_within_assertion)\" ]"

assert "gen-nextest-config.sh: analytical_validation priority override preserved in generated config" \
    bash -c "[ -n \"\$(_priority_for_file '$_TMP_CFG' reify-solver-elastic analytical_validation)\" ]"

assert "gen-nextest-config.sh: determinism priority override preserved in generated config" \
    bash -c "[ -n \"\$(_priority_for_file '$_TMP_CFG' reify-solver-elastic determinism)\" ]"

rm -f "$_TMP_CFG"

# ---------------------------------------------------------------------------
# Assertion D: drift-guard — each filter maps to a real test file on disk
# (step-3 adds this; stub out here as a forward-declared section)
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
# (step-3 adds this; RED until step-4 differentiates priorities)
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion E: LPT ordering — tensegrity_t0a priority > other four binaries ---"

_P_STRAGGLER="$(_priority_for reify-eval tensegrity_t0a)"
_P_FEA="$(_priority_for reify-eval fea_diagnostics_e2e)"
_P_REPR="$(_priority_for reify-eval representation_within_assertion)"
_P_ANALYTICAL="$(_priority_for reify-solver-elastic analytical_validation)"
_P_DETERMINISM="$(_priority_for reify-solver-elastic determinism)"

assert "tensegrity_t0a priority (${_P_STRAGGLER:-unset}) > fea_diagnostics_e2e priority (${_P_FEA:-unset})" \
    bash -c "[ -n '${_P_STRAGGLER}' ] && [ -n '${_P_FEA}' ] && [ '${_P_STRAGGLER}' -gt '${_P_FEA}' ]"

assert "tensegrity_t0a priority (${_P_STRAGGLER:-unset}) > representation_within_assertion priority (${_P_REPR:-unset})" \
    bash -c "[ -n '${_P_STRAGGLER}' ] && [ -n '${_P_REPR}' ] && [ '${_P_STRAGGLER}' -gt '${_P_REPR}' ]"

assert "tensegrity_t0a priority (${_P_STRAGGLER:-unset}) > analytical_validation priority (${_P_ANALYTICAL:-unset})" \
    bash -c "[ -n '${_P_STRAGGLER}' ] && [ -n '${_P_ANALYTICAL}' ] && [ '${_P_STRAGGLER}' -gt '${_P_ANALYTICAL}' ]"

assert "tensegrity_t0a priority (${_P_STRAGGLER:-unset}) > determinism priority (${_P_DETERMINISM:-unset})" \
    bash -c "[ -n '${_P_STRAGGLER}' ] && [ -n '${_P_DETERMINISM}' ] && [ '${_P_STRAGGLER}' -gt '${_P_DETERMINISM}' ]"

test_summary
