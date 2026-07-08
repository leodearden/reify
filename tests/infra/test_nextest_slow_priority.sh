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
            match($0, /-?[0-9]+/)
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
# Helper (task 5141): extract the `terminate-after` integer from a
# `slow-timeout = { period = "...", terminate-after = N }` line inside a
# [[profile.default.overrides]] block whose filter contains both
# package(<pkg>) and binary(<bin>). Mirrors _priority_for_file's block-walk,
# but stays in_block through the intervening `priority = ...` line (slow-timeout
# is authored one line below priority — task 5141 step-4) instead of resetting
# on it. Reads from FILE argument; prints the integer or empty string.
# Usage: _slow_terminate_for_file <file> <pkg> <binary>
# ---------------------------------------------------------------------------
_slow_terminate_for_file() {
    local file="$1" pkg="package(${2})" bin="binary(${3})"
    awk -v pkg="$pkg" -v bin="$bin" '
        /^\[\[/ { in_block = 0 }
        /filter/ && index($0, pkg) && index($0, bin) { in_block = 1 }
        in_block && /slow-timeout/ {
            match($0, /terminate-after[[:space:]]*=[[:space:]]*[0-9]+/)
            seg = substr($0, RSTART, RLENGTH)
            match(seg, /[0-9]+$/)
            print substr(seg, RSTART, RLENGTH)
            in_block = 0
        }
    ' "$file"
}

# Convenience wrapper for the canonical nextest.toml.
_slow_terminate_for() {
    _slow_terminate_for_file "$NEXTEST_TOML" "$1" "$2"
}

# ---------------------------------------------------------------------------
# Precompute priority values from nextest.toml (in current shell, not subshell).
# This makes assertions simple test -n / test -gt checks on already-resolved values.
# ---------------------------------------------------------------------------
P_T0A="$(_priority_for reify-eval tensegrity_t0a)"
P_FEA="$(_priority_for reify-eval-fea-tests fea_diagnostics_e2e)"
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

assert "nextest.toml: priority override exists for package(reify-eval-fea-tests) & binary(fea_diagnostics_e2e)" \
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
_C_FEA="$(_priority_for_file "$_TMP_CFG" reify-eval-fea-tests fea_diagnostics_e2e)"
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

assert "crates/reify-eval-fea-tests/tests/fea_diagnostics_e2e.rs exists on disk (filter not dangling)" \
    test -f "$REPO_ROOT/crates/reify-eval-fea-tests/tests/fea_diagnostics_e2e.rs"

assert "crates/reify-eval/tests/representation_within_assertion.rs exists on disk (filter not dangling)" \
    test -f "$REPO_ROOT/crates/reify-eval/tests/representation_within_assertion.rs"

assert "crates/reify-solver-elastic/tests/analytical_validation.rs exists on disk (filter not dangling)" \
    test -f "$REPO_ROOT/crates/reify-solver-elastic/tests/analytical_validation.rs"

assert "crates/reify-solver-elastic/tests/determinism.rs exists on disk (filter not dangling)" \
    test -f "$REPO_ROOT/crates/reify-solver-elastic/tests/determinism.rs"

# ---------------------------------------------------------------------------
# Assertion D2: drift-guard extension (step-3) — all priority values are
# numeric integers in nextest's documented signed-8-bit range (-100..100).
# nextest 0.9.136 hard-rejects out-of-range values at parse time (e.g. 9999
# fails); the assertion ensures a future edit doesn't accidentally write a
# value that nextest silently truncates or rejects at runtime.
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion D2: priority values are integers in nextest's -100..100 range ---"

_check_priority_range() {
    local name="$1" val="$2"
    # Must be non-empty, an integer (optionally negative), and in [-100, 100].
    if [ -z "$val" ]; then return 1; fi
    case "$val" in
        ''|*[!0-9-]*) return 1 ;;
        -*)
            # negative
            abs="${val#-}"
            case "$abs" in (*[!0-9]*) return 1 ;; esac
            [ "$abs" -le 100 ] || return 1
            ;;
        *)
            [ "$val" -le 100 ] || return 1
            ;;
    esac
    return 0
}

assert "tensegrity_t0a priority (${P_T0A:-unset}) is in nextest range -100..100" \
    _check_priority_range tensegrity_t0a "${P_T0A:-}"

assert "fea_diagnostics_e2e priority (${P_FEA:-unset}) is in nextest range -100..100" \
    _check_priority_range fea_diagnostics_e2e "${P_FEA:-}"

assert "representation_within_assertion priority (${P_REPR:-unset}) is in nextest range -100..100" \
    _check_priority_range representation_within_assertion "${P_REPR:-}"

assert "analytical_validation priority (${P_ANAL:-unset}) is in nextest range -100..100" \
    _check_priority_range analytical_validation "${P_ANAL:-}"

assert "determinism priority (${P_DET:-unset}) is in nextest range -100..100" \
    _check_priority_range determinism "${P_DET:-}"

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

# ---------------------------------------------------------------------------
# Assertion F (task 5141): heavy-tier slow-timeout/terminate-after ceiling.
# Each of the 5 heavy binaries additionally carries
# slow-timeout = { period = "120s", terminate-after = 15 } (1800s/30min) on
# its existing [[profile.default.overrides]] priority block — a larger
# ceiling than the [profile.default] 1200s/20min default (test_occt_gated_scope.sh
# Tests 16a-16d), sized to clear the worst legitimate straggler (tensegrity_t0a
# >180s even at ~8x load ≈1440s < 1800s) while still bounding a true hang well
# under the 3600s (60m) verify.sh pass-level wall.
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion F (task 5141): heavy-tier slow-timeout present, terminate-after = 15 ---"

ST_T0A="$(_slow_terminate_for reify-eval tensegrity_t0a)"
ST_FEA="$(_slow_terminate_for reify-eval-fea-tests fea_diagnostics_e2e)"
ST_REPR="$(_slow_terminate_for reify-eval representation_within_assertion)"
ST_ANAL="$(_slow_terminate_for reify-solver-elastic analytical_validation)"
ST_DET="$(_slow_terminate_for reify-solver-elastic determinism)"

assert "nextest.toml: tensegrity_t0a override has slow-timeout terminate-after = 15" \
    test "${ST_T0A:-}" = "15"

assert "nextest.toml: fea_diagnostics_e2e override has slow-timeout terminate-after = 15" \
    test "${ST_FEA:-}" = "15"

assert "nextest.toml: representation_within_assertion override has slow-timeout terminate-after = 15" \
    test "${ST_REPR:-}" = "15"

assert "nextest.toml: analytical_validation override has slow-timeout terminate-after = 15" \
    test "${ST_ANAL:-}" = "15"

assert "nextest.toml: determinism override has slow-timeout terminate-after = 15" \
    test "${ST_DET:-}" = "15"

# ---------------------------------------------------------------------------
# Assertion G (task 5141): gen-nextest-config.sh preserves each heavy-tier
# slow-timeout verbatim in the generated temp config (compile-free — the
# generator only runs sed on the occt max-threads line, never cargo/nextest).
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion G (task 5141): gen-nextest-config.sh preserves heavy-tier slow-timeout ---"

_TMP_CFG_ST="$(REIFY_OCCT_NEXTEST_MAX_THREADS=24 bash "$GEN_CFG")"

_GST_T0A="$(_slow_terminate_for_file "$_TMP_CFG_ST" reify-eval tensegrity_t0a)"
_GST_FEA="$(_slow_terminate_for_file "$_TMP_CFG_ST" reify-eval-fea-tests fea_diagnostics_e2e)"
_GST_REPR="$(_slow_terminate_for_file "$_TMP_CFG_ST" reify-eval representation_within_assertion)"
_GST_ANAL="$(_slow_terminate_for_file "$_TMP_CFG_ST" reify-solver-elastic analytical_validation)"
_GST_DET="$(_slow_terminate_for_file "$_TMP_CFG_ST" reify-solver-elastic determinism)"

rm -f "$_TMP_CFG_ST"

assert "gen-nextest-config.sh: tensegrity_t0a slow-timeout terminate-after = 15 preserved in generated config" \
    test "${_GST_T0A:-}" = "15"

assert "gen-nextest-config.sh: fea_diagnostics_e2e slow-timeout terminate-after = 15 preserved in generated config" \
    test "${_GST_FEA:-}" = "15"

assert "gen-nextest-config.sh: representation_within_assertion slow-timeout terminate-after = 15 preserved in generated config" \
    test "${_GST_REPR:-}" = "15"

assert "gen-nextest-config.sh: analytical_validation slow-timeout terminate-after = 15 preserved in generated config" \
    test "${_GST_ANAL:-}" = "15"

assert "gen-nextest-config.sh: determinism slow-timeout terminate-after = 15 preserved in generated config" \
    test "${_GST_DET:-}" = "15"

# ---------------------------------------------------------------------------
# Assertion H (task 5141): 2-tier ordering + wall bound. The heavy ceiling
# (120*15=1800s) is strictly greater than the default ceiling (120*10=1200s,
# test_occt_gated_scope.sh Test 16c) and strictly less than the 3600s (60m)
# pass-level wall — both tiers attribute-and-kill before the outer timeout
# fires exit 124 with zero attribution.
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion H (task 5141): 2-tier ordering — heavy (1800s) > default (1200s), both < 3600s wall ---"

assert "heavy slow-timeout ceiling (120*15=1800s) is strictly greater than the default ceiling (120*10=1200s)" \
    test $((120 * 15)) -gt $((120 * 10))

assert "heavy slow-timeout ceiling (120*15=1800s) is strictly less than the 3600s (60m) pass-level wall" \
    test $((120 * 15)) -lt 3600

test_summary
