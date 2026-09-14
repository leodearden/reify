#!/usr/bin/env bash
# Infrastructure test for task 4627, broadened by task 6485.
#
# PURPOSE: this is the completeness-and-safety guard over EVERY slow-timeout
# override in .config/nextest.toml — not only the five LPT priority blocks it
# originally covered.
#
# Originally (4627/5141) it validated that .config/nextest.toml declares priority
# overrides for the heavy-compute test binaries (LPT scheduling to compress the
# slow tail), that the occt test-group block coexists, and that
# scripts/gen-nextest-config.sh preserves all overrides verbatim in the generated
# temp config consumed by nextest. Task 6485 added the tiering guards J/K/L.
#
# THREE TIERS of ceiling now live in that file: default 1200s, gate-resident
# 1800s (blocks that still run on the merge gate, bounded by its 3600s wall), and
# heavy 43200s/12h (the 8 members of REIFY_HEAVY_NEXTEST_FILTER, which run only on
# the offline deep lane under its 13h release wall).
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
#
# TIERING / COMPLETENESS (task 6485):
#   J. Every atom of REIFY_HEAVY_NEXTEST_FILTER has its own override block at the
#      43200s (12h) heavy ceiling. Heavy membership is DERIVED from
#      scripts/heavy-test-filter-lib.sh, never restated here, so a 9th atom added
#      to the lib fails immediately with no edit to this file. J-gen pins the same
#      ceilings through gen-nextest-config.sh; J-neg is its non-vacuity self-check.
#   K. TOTAL CLASSIFICATION — every slow-timeout override in the file classifies
#      as exactly one of heavy (=> 43200s) or gate-resident (=> 1800s, under the
#      3600s gate wall), and the two classes PARTITION the file. A block matching
#      neither RED-lights until a human classifies it; a deleted allowlisted block
#      fails too. The gate-resident allowlist lives in THIS FILE, deliberately not
#      in the config, so an override cannot be self-classified in the same edit
#      that adds it.
#   L. REACHABILITY — the offline release wall (grepped from scripts/verify.sh)
#      strictly exceeds the heavy ceiling (parsed from .config/nextest.toml), so
#      nextest attributes-and-kills a hung heavy test BY NAME before the outer
#      wall fires exit 124 naming nothing. Both operands are derived from files,
#      never restated as literals.

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
        in_block && /^[[:space:]]*slow-timeout[[:space:]]*=/ {
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
# Helper (task 5141 amend — reviewer test-quality finding): extract the
# `period` seconds integer from a `slow-timeout = { period = "Ns",
# terminate-after = M }` line inside a [[profile.default.overrides]] block
# whose filter contains both package(<pkg>) and binary(<bin>). Mirrors
# _slow_terminate_for_file's block-walk but pulls the period digits instead
# of terminate-after. Feeds Assertion H below so its 2-tier ordering/wall-
# bound arithmetic is computed FROM parsed file content rather than repeating
# hardcoded literals that could never fail regardless of the file's contents.
# Usage: _slow_period_for_file <file> <pkg> <binary>
# ---------------------------------------------------------------------------
_slow_period_for_file() {
    local file="$1" pkg="package(${2})" bin="binary(${3})"
    awk -v pkg="$pkg" -v bin="$bin" '
        /^\[\[/ { in_block = 0 }
        /filter/ && index($0, pkg) && index($0, bin) { in_block = 1 }
        in_block && /^[[:space:]]*slow-timeout[[:space:]]*=/ {
            match($0, /period[[:space:]]*=[[:space:]]*"[0-9]+s"/)
            seg = substr($0, RSTART, RLENGTH)
            match(seg, /[0-9]+/)
            print substr(seg, RSTART, RLENGTH)
            in_block = 0
        }
    ' "$file"
}

# Convenience wrapper for the canonical nextest.toml.
_slow_period_for() {
    _slow_period_for_file "$NEXTEST_TOML" "$1" "$2"
}

# ---------------------------------------------------------------------------
# Helper (task 5141 amend): compute period-seconds * terminate-after for the
# [profile.default] table's slow-timeout — section-scoped to that table only
# (excludes [[profile.default.overrides]] blocks), mirroring
# test_occt_gated_scope.sh's _DEFAULT_ST_AWK extractor. Prints the product
# (e.g. "1200") or an empty string if [profile.default]'s slow-timeout is
# missing. Feeds Assertion H so the default-tier ceiling it compares against
# is likewise parsed from the file, not a repeated literal.
# Usage: _default_slow_timeout_seconds_for_file <file>
# ---------------------------------------------------------------------------
_default_slow_timeout_seconds_for_file() {
    local file="$1"
    awk '
        /^\[profile\.default\]/ { f = 1; next }
        /^\[/ { f = 0 }
        f && /slow-timeout/ {
            match($0, /period[[:space:]]*=[[:space:]]*"[0-9]+s"/)
            pseg = substr($0, RSTART, RLENGTH)
            match(pseg, /[0-9]+/)
            period = substr(pseg, RSTART, RLENGTH) + 0
            match($0, /terminate-after[[:space:]]*=[[:space:]]*[0-9]+/)
            tseg = substr($0, RSTART, RLENGTH)
            match(tseg, /[0-9]+$/)
            term = substr(tseg, RSTART, RLENGTH) + 0
            print period * term
            exit
        }
    ' "$file"
}

# ---------------------------------------------------------------------------
# Helper (task 5984): extract the [profile.default] table's global test-threads
# pool cap — section-scoped to that table only (the /^\[/ reset excludes the
# [[profile.default.overrides]] blocks that follow), mirroring
# _default_slow_timeout_seconds_for_file above and test_occt_gated_scope.sh's
# _DEFAULT_TT_AWK. Prints the integer (e.g. "32") or an empty string if the key
# is absent. Feeds Assertion I.
# Usage: _default_test_threads_for_file <file>
# ---------------------------------------------------------------------------
_default_test_threads_for_file() {
    local file="$1"
    awk '
        /^\[profile\.default\]/ { f = 1; next }
        /^\[/ { f = 0 }
        f && /^test-threads[[:space:]]*=/ {
            match($0, /[0-9]+/)
            print substr($0, RSTART, RLENGTH)
            exit
        }
    ' "$file"
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
# Assertion F (task 5141; RETARGETED by task 6485): per-block
# slow-timeout/terminate-after values, BY TIER.
#
# All five of these blocks used to carry terminate-after = 15 (1800s). Task 6485
# split them: the four HEAVY binaries moved to the 12h offline ceiling
# (terminate-after = 360), while representation_within_assertion is
# GATE-RESIDENT — it still runs on the merge gate, so it keeps 1800s, strictly
# under the 3600s pass-level wall. Pinning each tier's value separately here is
# what makes a block silently changing tier fail; Assertion K enforces that the
# two tiers exhaust every slow-timeout override in the file.
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion F (task 5141, retargeted 6485): heavy blocks at terminate-after = 360, gate-resident at 15 ---"

ST_T0A="$(_slow_terminate_for reify-eval tensegrity_t0a)"
ST_FEA="$(_slow_terminate_for reify-eval-fea-tests fea_diagnostics_e2e)"
ST_REPR="$(_slow_terminate_for reify-eval representation_within_assertion)"
ST_ANAL="$(_slow_terminate_for reify-solver-elastic analytical_validation)"
ST_DET="$(_slow_terminate_for reify-solver-elastic determinism)"

assert "nextest.toml: tensegrity_t0a override has slow-timeout terminate-after = 360 (heavy tier, 12h)" \
    test "${ST_T0A:-}" = "360"

assert "nextest.toml: fea_diagnostics_e2e override has slow-timeout terminate-after = 360 (heavy tier, 12h)" \
    test "${ST_FEA:-}" = "360"

assert "nextest.toml: representation_within_assertion override has slow-timeout terminate-after = 15 (gate-resident tier, 1800s)" \
    test "${ST_REPR:-}" = "15"

assert "nextest.toml: analytical_validation override has slow-timeout terminate-after = 360 (heavy tier, 12h)" \
    test "${ST_ANAL:-}" = "360"

assert "nextest.toml: determinism override has slow-timeout terminate-after = 360 (heavy tier, 12h)" \
    test "${ST_DET:-}" = "360"

# ---------------------------------------------------------------------------
# Assertion G (task 5141): gen-nextest-config.sh preserves each heavy-tier
# slow-timeout verbatim in the generated temp config (compile-free — the
# generator only runs sed on the occt max-threads line, never cargo/nextest).
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion G (task 5141, retargeted 6485): gen-nextest-config.sh preserves each tier's slow-timeout ---"

_TMP_CFG_ST="$(REIFY_OCCT_NEXTEST_MAX_THREADS=24 bash "$GEN_CFG")"

_GST_T0A="$(_slow_terminate_for_file "$_TMP_CFG_ST" reify-eval tensegrity_t0a)"
_GST_FEA="$(_slow_terminate_for_file "$_TMP_CFG_ST" reify-eval-fea-tests fea_diagnostics_e2e)"
_GST_REPR="$(_slow_terminate_for_file "$_TMP_CFG_ST" reify-eval representation_within_assertion)"
_GST_ANAL="$(_slow_terminate_for_file "$_TMP_CFG_ST" reify-solver-elastic analytical_validation)"
_GST_DET="$(_slow_terminate_for_file "$_TMP_CFG_ST" reify-solver-elastic determinism)"

rm -f "$_TMP_CFG_ST"

assert "gen-nextest-config.sh: tensegrity_t0a slow-timeout terminate-after = 360 preserved in generated config" \
    test "${_GST_T0A:-}" = "360"

assert "gen-nextest-config.sh: fea_diagnostics_e2e slow-timeout terminate-after = 360 preserved in generated config" \
    test "${_GST_FEA:-}" = "360"

assert "gen-nextest-config.sh: representation_within_assertion slow-timeout terminate-after = 15 preserved in generated config" \
    test "${_GST_REPR:-}" = "15"

assert "gen-nextest-config.sh: analytical_validation slow-timeout terminate-after = 360 preserved in generated config" \
    test "${_GST_ANAL:-}" = "360"

assert "gen-nextest-config.sh: determinism slow-timeout terminate-after = 360 preserved in generated config" \
    test "${_GST_DET:-}" = "360"

# ---------------------------------------------------------------------------
# Assertion H (task 5141; amended — reviewer test-quality finding): 2-tier
# ordering + wall bound. For each of the 5 heavy binaries, its ceiling
# (period-seconds * terminate-after — BOTH FACTORS EXTRACTED FROM
# nextest.toml via _slow_period_for/ST_* above, not hardcoded literals) is
# strictly greater than the [profile.default] ceiling (likewise extracted via
# _default_slow_timeout_seconds_for_file; test_occt_gated_scope.sh Test 16c
# performs the equivalent file-derived check of the default ceiling against
# the 3600s wall).
#
# RETARGETED by task 6485 — the `< 3600s` conjunct is now applied ONLY to the
# gate-resident block (representation_within_assertion). The invariant it
# encodes is unchanged and NOT dropped: every ceiling must attribute-and-kill
# before its outer wall fires exit 124 with zero attribution. What changed is
# that the applicable wall now differs BY TIER. A gate-resident block still runs
# on the merge gate, so its wall is 3600s and it is still checked here. The four
# heavy blocks no longer run on any gate path, so their wall is the offline
# lane's 13h release wall — 12x larger than 3600s — and their reachability is
# checked against that wall in Assertion L instead, with both operands derived
# from files. Keeping a `< 3600` check on a heavy block here would assert a
# bound that no path actually imposes on it.
#
# The original form of this assertion (`test $((120*15)) -gt $((120*10))`)
# was pure arithmetic on literals that never read nextest.toml — it could
# never fail regardless of what the file contained. Deriving both operands
# from the parsed config ties the check to actual state, so an edit to any
# heavy block's period/terminate-after (or to [profile.default]'s) that broke
# the ordering/wall invariant would now fail here.
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion H (task 5141, retargeted 6485): every ceiling > default ceiling; gate-resident ceiling < its 3600s wall (values extracted from nextest.toml) ---"

SP_T0A="$(_slow_period_for reify-eval tensegrity_t0a)"
SP_FEA="$(_slow_period_for reify-eval-fea-tests fea_diagnostics_e2e)"
SP_REPR="$(_slow_period_for reify-eval representation_within_assertion)"
SP_ANAL="$(_slow_period_for reify-solver-elastic analytical_validation)"
SP_DET="$(_slow_period_for reify-solver-elastic determinism)"

DEFAULT_SECONDS="$(_default_slow_timeout_seconds_for_file "$NEXTEST_TOML")"

HS_T0A=$(( ${SP_T0A:-0} * ${ST_T0A:-0} ))
HS_FEA=$(( ${SP_FEA:-0} * ${ST_FEA:-0} ))
HS_REPR=$(( ${SP_REPR:-0} * ${ST_REPR:-0} ))
HS_ANAL=$(( ${SP_ANAL:-0} * ${ST_ANAL:-0} ))
HS_DET=$(( ${SP_DET:-0} * ${ST_DET:-0} ))

assert "tensegrity_t0a heavy ceiling (${HS_T0A}s = period ${SP_T0A:-unset}s * terminate-after ${ST_T0A:-unset}) is strictly greater than the default ceiling (${DEFAULT_SECONDS:-unset}s), both extracted from nextest.toml" \
    bash -c "[ -n '${HS_T0A:-}' ] && [ -n '${DEFAULT_SECONDS:-}' ] && [ '${HS_T0A:-0}' -gt '${DEFAULT_SECONDS:-0}' ]"

assert "fea_diagnostics_e2e heavy ceiling (${HS_FEA}s = period ${SP_FEA:-unset}s * terminate-after ${ST_FEA:-unset}) is strictly greater than the default ceiling (${DEFAULT_SECONDS:-unset}s), both extracted from nextest.toml" \
    bash -c "[ -n '${HS_FEA:-}' ] && [ -n '${DEFAULT_SECONDS:-}' ] && [ '${HS_FEA:-0}' -gt '${DEFAULT_SECONDS:-0}' ]"

assert "representation_within_assertion heavy ceiling (${HS_REPR}s = period ${SP_REPR:-unset}s * terminate-after ${ST_REPR:-unset}) is strictly greater than the default ceiling (${DEFAULT_SECONDS:-unset}s), both extracted from nextest.toml" \
    bash -c "[ -n '${HS_REPR:-}' ] && [ -n '${DEFAULT_SECONDS:-}' ] && [ '${HS_REPR:-0}' -gt '${DEFAULT_SECONDS:-0}' ]"

assert "analytical_validation heavy ceiling (${HS_ANAL}s = period ${SP_ANAL:-unset}s * terminate-after ${ST_ANAL:-unset}) is strictly greater than the default ceiling (${DEFAULT_SECONDS:-unset}s), both extracted from nextest.toml" \
    bash -c "[ -n '${HS_ANAL:-}' ] && [ -n '${DEFAULT_SECONDS:-}' ] && [ '${HS_ANAL:-0}' -gt '${DEFAULT_SECONDS:-0}' ]"

assert "determinism heavy ceiling (${HS_DET}s = period ${SP_DET:-unset}s * terminate-after ${ST_DET:-unset}) is strictly greater than the default ceiling (${DEFAULT_SECONDS:-unset}s), both extracted from nextest.toml" \
    bash -c "[ -n '${HS_DET:-}' ] && [ -n '${DEFAULT_SECONDS:-}' ] && [ '${HS_DET:-0}' -gt '${DEFAULT_SECONDS:-0}' ]"



# The one surviving wall check: representation_within_assertion is
# GATE-RESIDENT, so 3600s really is the wall that binds it.
assert "representation_within_assertion gate-resident ceiling (${HS_REPR}s, extracted from nextest.toml) is strictly less than the 3600s (60m) pass-level wall that binds it" \
    bash -c "[ -n '${HS_REPR:-}' ] && [ '${HS_REPR:-0}' -lt 3600 ]"



# ---------------------------------------------------------------------------
# Assertion I (task 5984): CO-PRESERVATION of BOTH sed anchors in ONE generate.
#
# Extends Assertion C's one-generate-many-assertions pattern.  As of task 5984
# gen-nextest-config.sh rewrites TWO anchors in a single sed pass:
#   ^occt = { max-threads = N }$   (the occt test-group cap, task 4503/4621)
#   ^test-threads = N$            (the global [profile.default] pool cap)
#
# Both anchored substitutions fail SAFE by construction: if a future edit breaks
# an anchor the substitution silently no-ops and the generated config keeps the
# in-file literal.  That is the right failure mode at runtime — but it is
# INVISIBLE, and for the global pool cap it would mean the cap quietly stops
# tracking the host.  This assertion is what converts that silent no-op into a
# loud CI failure: ONE generate carrying BOTH env overrides must produce a
# single config in which BOTH rewritten values AND all five priority overrides
# hold together.  A sed edit that rewrites one anchor while dropping or
# clobbering the other cannot pass here.
#
# Compile-free (gen-nextest-config.sh only runs sed, never cargo).
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion I (task 5984): one generate rewrites BOTH the occt cap and the global test-threads cap, preserving the priority overrides ---"

_CO_CFG="$(REIFY_OCCT_NEXTEST_MAX_THREADS=7 REIFY_NEXTEST_TEST_THREADS=5 bash "$GEN_CFG")"

_CO_OCCT="$(awk '/^\[test-groups\]/{f=1;next}/^\[/{f=0}f&&/occt.*max-threads/{match($0,/[0-9]+/);print substr($0,RSTART,RLENGTH);exit}' "$_CO_CFG")"
_CO_TT="$(_default_test_threads_for_file "$_CO_CFG")"
_CO_T0A="$(_priority_for_file "$_CO_CFG" reify-eval tensegrity_t0a)"
_CO_FEA="$(_priority_for_file "$_CO_CFG" reify-eval-fea-tests fea_diagnostics_e2e)"
_CO_REPR="$(_priority_for_file "$_CO_CFG" reify-eval representation_within_assertion)"
_CO_ANAL="$(_priority_for_file "$_CO_CFG" reify-solver-elastic analytical_validation)"
_CO_DET="$(_priority_for_file "$_CO_CFG" reify-solver-elastic determinism)"

assert "gen-nextest-config.sh (MAX_THREADS=7 + TEST_THREADS=5 in ONE generate): [test-groups] occt max-threads = 7 (got '${_CO_OCCT:-unset}')" \
    test "${_CO_OCCT:-}" = "7"

assert "gen-nextest-config.sh (MAX_THREADS=7 + TEST_THREADS=5 in ONE generate): [profile.default] test-threads = 5 (got '${_CO_TT:-unset}') — the co-preservation conjunct" \
    test "${_CO_TT:-}" = "5"

assert "gen-nextest-config.sh (both caps rewritten): tensegrity_t0a priority override still resolves" \
    test -n "${_CO_T0A:-}"

assert "gen-nextest-config.sh (both caps rewritten): fea_diagnostics_e2e priority override still resolves" \
    test -n "${_CO_FEA:-}"

assert "gen-nextest-config.sh (both caps rewritten): representation_within_assertion priority override still resolves" \
    test -n "${_CO_REPR:-}"

assert "gen-nextest-config.sh (both caps rewritten): analytical_validation priority override still resolves" \
    test -n "${_CO_ANAL:-}"

assert "gen-nextest-config.sh (both caps rewritten): determinism priority override still resolves" \
    test -n "${_CO_DET:-}"

rm -f "$_CO_CFG"

# ===========================================================================
# Assertion J (task 6485): every heavy-filter member carries its own
# slow-timeout override at the 12h offline ceiling.
#
# Heavy membership is DERIVED from scripts/heavy-test-filter-lib.sh rather than
# restated here. That is the point of the guard: heavy membership already has
# exactly one definition, and a list repeated here would become a second one
# that drifts. A 9th atom added to the lib fails Assertion J immediately, with
# no edit to this file.
#
# The correspondence checked is exact string equality between an atom (outer
# parentheses stripped) and an override block's `filter =` VALUE. That is why
# the two test-scoped atoms must be authored in .config/nextest.toml
# byte-identically to their lib form.
# ===========================================================================

# ---------------------------------------------------------------------------
# Helper (task 6485): ceiling (period-seconds * terminate-after) for the
# [[profile.default.overrides]] block whose `filter` VALUE equals <filter>
# exactly (whitespace-normalized). Mirrors the _slow_period_for_file /
# _slow_terminate_for_file block-walk above, but keys on the whole filter
# value instead of package+binary.
#
# REQUIRED, not stylistic: the two test-scoped heavy atoms share BOTH package
# (reify-eval) AND binary (harness_fea_solver_e2e), so the package+binary
# helpers above cannot tell them apart -- they would match whichever block
# came first and silently report one ceiling for both.
# Usage: _slow_ceiling_for_filter <file> <filter-value>
# ---------------------------------------------------------------------------
_slow_ceiling_for_filter() {
    local file="$1" want="$2"
    awk -v want="$want" -v q="'" '
        /^\[\[/ { in_block = 0 }
        $0 ~ "^[[:space:]]*filter[[:space:]]*=" {
            in_block = 0
            if (match($0, q "[^" q "]*" q)) {
                val = substr($0, RSTART + 1, RLENGTH - 2)
                gsub(/[[:space:]]+/, " ", val)
                sub(/^ /, "", val)
                sub(/ $/, "", val)
                if (val == want) in_block = 1
            }
        }
        in_block && /^[[:space:]]*slow-timeout[[:space:]]*=/ {
            match($0, /period[[:space:]]*=[[:space:]]*"[0-9]+s"/)
            pseg = substr($0, RSTART, RLENGTH)
            match(pseg, /[0-9]+/)
            period = substr(pseg, RSTART, RLENGTH) + 0
            match($0, /terminate-after[[:space:]]*=[[:space:]]*[0-9]+/)
            tseg = substr($0, RSTART, RLENGTH)
            match(tseg, /[0-9]+$/)
            term = substr(tseg, RSTART, RLENGTH) + 0
            print period * term
            in_block = 0
        }
    ' "$file"
}

# The 12h (43200s = 120s x 360) offline per-test ceiling.
HEAVY_CEILING_SECONDS=43200

# ---------------------------------------------------------------------------
# Parse the heavy atoms out of the single source of truth. Split the
# or-joined expression on its top-level ' | ' joiner and strip each atom's
# outer parentheses, yielding exactly the filter strings the override blocks
# must carry. (test_heavy_filter_atoms.sh's parser deliberately stops at the
# `package(X) & binary(Y)` PREFIX; here the WHOLE atom text is needed,
# including a trailing `& test(...)` clause, because that whole text is what
# an override block's filter value has to equal.)
# ---------------------------------------------------------------------------
# shellcheck source=scripts/heavy-test-filter-lib.sh
source "$REPO_ROOT/scripts/heavy-test-filter-lib.sh"

HEAVY_ATOMS=()
while IFS= read -r _atom; do
    [ -n "$_atom" ] && HEAVY_ATOMS+=("$_atom")
done < <(printf '%s\n' "${REIFY_HEAVY_NEXTEST_FILTER:-}" \
            | sed 's/ | /\n/g' \
            | sed -e 's/^(//' -e 's/)$//')

# ---------------------------------------------------------------------------
# _heavy_ceilings_ok <file> — returns 0 iff EVERY heavy atom resolves to a
# block at HEAVY_CEILING_SECONDS in <file>. Pointed at the canonical config by
# Assertion J, at a generated config by J-gen, and at deliberately-broken
# fixture copies by the non-vacuity self-check below.
# ---------------------------------------------------------------------------
_heavy_ceilings_ok() {
    local file="$1" atom got
    for atom in ${HEAVY_ATOMS+"${HEAVY_ATOMS[@]}"}; do
        got="$(_slow_ceiling_for_filter "$file" "$atom")"
        [ "${got:-}" = "$HEAVY_CEILING_SECONDS" ] || return 1
    done
    return 0
}

# Negation wrapper. assert runs "$@" directly in THIS shell (test_helpers.sh's
# no-subshell idiom), so the rejection cases must be a shell function too --
# a `bash -c '! ...'` child would not inherit _heavy_ceilings_ok or HEAVY_ATOMS
# and would fail for the wrong reason, making the self-check meaningless.
_heavy_ceilings_reject() { ! _heavy_ceilings_ok "$1"; }

echo ""
echo "--- Assertion J (task 6485): every heavy-filter atom has a 12h (${HEAVY_CEILING_SECONDS}s) override block ---"

# Non-vacuity floor for the PARSE itself. Deliberately '>= 1', not '== 8': a
# 9th atom added to the lib must fail in the per-atom checks below (naming the
# offender), never here with an unhelpful count mismatch.
echo "    (parsed ${#HEAVY_ATOMS[@]} heavy atoms from scripts/heavy-test-filter-lib.sh)"
assert "heavy-filter lib parsed into at least one atom (guard is non-vacuous)" \
    test "${#HEAVY_ATOMS[@]}" -ge 1

for _atom in ${HEAVY_ATOMS+"${HEAVY_ATOMS[@]}"}; do
    _got="$(_slow_ceiling_for_filter "$NEXTEST_TOML" "$_atom")"
    assert "nextest.toml: heavy atom [${_atom}] has an override block at the ${HEAVY_CEILING_SECONDS}s (12h) ceiling (got '${_got:-<no block>}')" \
        test "${_got:-}" = "$HEAVY_CEILING_SECONDS"
done

# ---------------------------------------------------------------------------
# Assertion J-gen (task 6485): the same ceilings survive gen-nextest-config.sh
# verbatim, asserted from ONE generate (mirrors Assertion G's
# one-generate-many-assertions pattern). The generator's two sed anchors
# (`^occt = { max-threads = N }$`, `^test-threads = ...$`) do not touch
# slow-timeout lines today; this pins that they never start to.
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion J-gen (task 6485): gen-nextest-config.sh preserves every heavy 12h ceiling ---"

_TMP_CFG_J="$(REIFY_OCCT_NEXTEST_MAX_THREADS=24 bash "$GEN_CFG")"

for _atom in ${HEAVY_ATOMS+"${HEAVY_ATOMS[@]}"}; do
    _gotj="$(_slow_ceiling_for_filter "$_TMP_CFG_J" "$_atom")"
    assert "gen-nextest-config.sh: heavy atom [${_atom}] still at the ${HEAVY_CEILING_SECONDS}s ceiling in the generated config (got '${_gotj:-<no block>}')" \
        test "${_gotj:-}" = "$HEAVY_CEILING_SECONDS"
done

rm -f "$_TMP_CFG_J"

# ---------------------------------------------------------------------------
# Assertion J-neg (task 6485): NON-VACUITY SELF-CHECK. A completeness guard
# that is green on arrival proves nothing unless it is also shown to go RED on
# the drift it exists to catch. Each fixture below is a copy of the real
# nextest.toml broken in one specific way; _heavy_ceilings_ok must reject all
# three. Follows assert_guard_rejects in tests/infra/test_verify_offline_partition.sh.
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion J-neg (task 6485): the heavy-ceiling checker REJECTS each seeded drift ---"

_J_FIX="$(mktemp -d)"
_FIRST_ATOM="${HEAVY_ATOMS[0]}"

# (i) the first heavy atom's whole override block deleted.
awk -v want="$_FIRST_ATOM" -v q="'" '
    function flush(   i) {
        if (!drop) { for (i = 1; i <= n; i++) print b[i] }
        n = 0; drop = 0
    }
    /^\[\[/ { flush() }
    { b[++n] = $0 }
    $0 ~ "^[[:space:]]*filter[[:space:]]*=" {
        if (match($0, q "[^" q "]*" q)) {
            val = substr($0, RSTART + 1, RLENGTH - 2)
            if (val == want) drop = 1
        }
    }
    END { flush() }
' "$NEXTEST_TOML" > "$_J_FIX/deleted.toml"

# (ii) the first heavy atom's block left at the old terminate-after = 15.
awk -v want="$_FIRST_ATOM" -v q="'" '
    /^\[\[/ { hit = 0 }
    $0 ~ "^[[:space:]]*filter[[:space:]]*=" {
        hit = 0
        if (match($0, q "[^" q "]*" q)) {
            val = substr($0, RSTART + 1, RLENGTH - 2)
            if (val == want) hit = 1
        }
    }
    hit && /^[[:space:]]*slow-timeout[[:space:]]*=/ {
        sub(/terminate-after[[:space:]]*=[[:space:]]*[0-9]+/, "terminate-after = 15")
        hit = 0
    }
    { print }
' "$NEXTEST_TOML" > "$_J_FIX/stale.toml"

# (iii) the first heavy atom's filter VALUE typo'd so it no longer equals the
# lib atom (the silent-drift shape: nextest still parses the file happily).
sed "s|${_FIRST_ATOM}|&_typo|" "$NEXTEST_TOML" > "$_J_FIX/typo.toml"

assert "J-neg (i): checker REJECTS a nextest.toml with a heavy atom's override block deleted" \
    _heavy_ceilings_reject "$_J_FIX/deleted.toml"

assert "J-neg (ii): checker REJECTS a nextest.toml with a heavy block left at the old terminate-after = 15" \
    _heavy_ceilings_reject "$_J_FIX/stale.toml"

assert "J-neg (iii): checker REJECTS a nextest.toml with a heavy block's filter value typo'd out of correspondence" \
    _heavy_ceilings_reject "$_J_FIX/typo.toml"

# Positive control: the SAME checker accepts the real file, so the three
# rejections above are attributable to the seeded drift and not to a checker
# that rejects everything.
assert "J-neg control: the same checker ACCEPTS the real .config/nextest.toml" \
    _heavy_ceilings_ok "$NEXTEST_TOML"

rm -rf "$_J_FIX"

# ===========================================================================
# Assertion K (task 6485): TOTAL CLASSIFICATION — no override can silently
# drift out of coverage.
#
# Assertion J above proves every HEAVY atom has a block. K proves the converse:
# every slow-timeout override block in .config/nextest.toml is accounted for by
# exactly one of two classes, so a block can neither appear nor vanish unnoticed.
#
#   heavy          — its filter is one of the atoms parsed from
#                    REIFY_HEAVY_NEXTEST_FILTER => ceiling must be 43200s (12h).
#   gate-resident  — its filter is in GATE_RESIDENT_FILTERS below => ceiling must
#                    be 1800s, and strictly under the 3600s gate wall.
#
# A block matching NEITHER fails, telling the author to classify it.
#
# The allowlist lives HERE, in the test, and deliberately NOT as a marker in
# .config/nextest.toml. A marker in the config would let whoever adds an override
# self-classify it in the same edit — the guard would then be satisfied by
# construction and would stop guarding, which is the exact failure mode this task
# was filed against. Keeping it here means a newly added override matches neither
# class and RED-lights until a human deliberately classifies it.
# ===========================================================================

VERIFY_SH="$REPO_ROOT/scripts/verify.sh"

# The gate-resident tier: overrides that DO still run on the merge gate, so the
# 3600s pass-level wall really binds them and their ceiling must stay under it.
GATE_RESIDENT_FILTERS=(
    'package(reify-eval) & binary(representation_within_assertion)'
    'package(reify-eval) & binary(solve_elastic_static_body_e2e)'
)
GATE_RESIDENT_CEILING_SECONDS=1800
GATE_WALL_SECONDS=3600

# ---------------------------------------------------------------------------
# _slow_timeout_filters_for_file <file> — one line per [[profile.default.overrides]]
# block that carries a slow-timeout key, printing that block's filter VALUE.
# Blocks with no slow-timeout (the occt test-group block) are correctly omitted:
# they set a different SETTING and this guard is about ceilings.
# ---------------------------------------------------------------------------
_slow_timeout_filters_for_file() {
    local file="$1"
    awk -v q="'" '
        /^\[\[/ { cur = "" }
        $0 ~ "^[[:space:]]*filter[[:space:]]*=" {
            cur = ""
            if (match($0, q "[^" q "]*" q)) {
                val = substr($0, RSTART + 1, RLENGTH - 2)
                gsub(/[[:space:]]+/, " ", val)
                sub(/^ /, "", val)
                sub(/ $/, "", val)
                cur = val
            }
        }
        cur != "" && /^[[:space:]]*slow-timeout[[:space:]]*=/ { print cur; cur = "" }
    ' "$file"
}

# _in_list <needle> [item...] — exact string membership.
_in_list() {
    local needle="$1"; shift
    local x
    for x in "$@"; do
        [ "$x" = "$needle" ] && return 0
    done
    return 1
}

# ---------------------------------------------------------------------------
# _classify_overrides_ok <file> — returns 0 iff EVERY slow-timeout override in
# <file> classifies as heavy or gate-resident at its class's required ceiling,
# AND every GATE_RESIDENT_FILTERS entry is actually present (so deleting an
# allowlisted block fails too, not just adding an unclassified one).
# The boolean form exists so the non-vacuity self-check below can point the very
# same checker at deliberately-broken fixtures.
# ---------------------------------------------------------------------------
_classify_overrides_ok() {
    local file="$1" f got
    local -a seen=()
    while IFS= read -r f; do
        [ -n "$f" ] || continue
        got="$(_slow_ceiling_for_filter "$file" "$f")"
        if _in_list "$f" ${HEAVY_ATOMS+"${HEAVY_ATOMS[@]}"}; then
            [ "${got:-}" = "$HEAVY_CEILING_SECONDS" ] || return 1
        elif _in_list "$f" ${GATE_RESIDENT_FILTERS+"${GATE_RESIDENT_FILTERS[@]}"}; then
            [ "${got:-}" = "$GATE_RESIDENT_CEILING_SECONDS" ] || return 1
            [ "$GATE_WALL_SECONDS" -gt "${got:-0}" ] || return 1
        else
            return 1
        fi
        seen+=("$f")
    done < <(_slow_timeout_filters_for_file "$file")
    for f in ${GATE_RESIDENT_FILTERS+"${GATE_RESIDENT_FILTERS[@]}"}; do
        _in_list "$f" ${seen+"${seen[@]}"} || return 1
    done
    return 0
}

_classify_overrides_reject() { ! _classify_overrides_ok "$1"; }

echo ""
echo "--- Assertion K (task 6485): every slow-timeout override classifies as heavy or gate-resident ---"

_K_SEEN=()
while IFS= read -r _f; do
    [ -n "$_f" ] || continue
    _kgot="$(_slow_ceiling_for_filter "$NEXTEST_TOML" "$_f")"
    if _in_list "$_f" ${HEAVY_ATOMS+"${HEAVY_ATOMS[@]}"}; then
        assert "K: [${_f}] classified HEAVY — ceiling is ${HEAVY_CEILING_SECONDS}s (got '${_kgot:-<none>}')" \
            test "${_kgot:-}" = "$HEAVY_CEILING_SECONDS"
    elif _in_list "$_f" ${GATE_RESIDENT_FILTERS+"${GATE_RESIDENT_FILTERS[@]}"}; then
        assert "K: [${_f}] classified GATE-RESIDENT — ceiling is ${GATE_RESIDENT_CEILING_SECONDS}s (got '${_kgot:-<none>}')" \
            test "${_kgot:-}" = "$GATE_RESIDENT_CEILING_SECONDS"
        assert "K: [${_f}] gate-resident ceiling stays under the ${GATE_WALL_SECONDS}s gate wall that binds it" \
            test "$GATE_WALL_SECONDS" -gt "${_kgot:-0}"
    else
        assert "K: [${_f}] is UNCLASSIFIED — add it to the heavy filterset (scripts/heavy-test-filter-lib.sh) or, if it really does still run on the merge gate, to GATE_RESIDENT_FILTERS in this test. Classify it deliberately; do not widen the guard." \
            false
    fi
    _K_SEEN+=("$_f")
done < <(_slow_timeout_filters_for_file "$NEXTEST_TOML")

# PARTITION, reverse direction: every allowlist entry must actually be in the
# file, so DELETING a gate-resident block fails here too.
for _g in ${GATE_RESIDENT_FILTERS+"${GATE_RESIDENT_FILTERS[@]}"}; do
    assert "K: gate-resident allowlist entry [${_g}] is present in .config/nextest.toml (a deleted block fails here)" \
        _in_list "$_g" ${_K_SEEN+"${_K_SEEN[@]}"}
done

# solve_elastic_static_body_e2e is named explicitly: it is the override this task
# was combined to cover (it had no drift-guard at all before task 6485), so it
# must be VISIBLY covered rather than only incidentally covered by the loop above.
assert "K: solve_elastic_static_body_e2e (task 7339's contention-headroom override) is enumerated and gate-resident at ${GATE_RESIDENT_CEILING_SECONDS}s — the previously unguarded block this task closes" \
    bash -c '[ "$1" = "$2" ]' _ \
        "$(_slow_ceiling_for_filter "$NEXTEST_TOML" 'package(reify-eval) & binary(solve_elastic_static_body_e2e)')" \
        "$GATE_RESIDENT_CEILING_SECONDS"

assert "K: the two classes PARTITION the file — every enumerated slow-timeout override is consumed by exactly one class" \
    _classify_overrides_ok "$NEXTEST_TOML"

# ===========================================================================
# Assertion L (task 6485): REACHABILITY — the offline release wall strictly
# exceeds the heavy per-test ceiling.
#
# GREEN relationship guard (following T10's and Test 16d's precedent): it holds
# on arrival and exists to fail if either side drifts. BOTH operands are derived
# from files — the ceiling parsed out of .config/nextest.toml, the wall grepped
# out of scripts/verify.sh — never restated as literals here. That is the
# standard Assertion H was amended to meet: an assertion over two hardcoded
# numbers can never fail regardless of what the files contain.
#
# If the wall does not exceed the ceiling, the ceiling is unreachable and a hung
# heavy test degrades to a bare `timeout` exit 124 naming nothing — the task
# 4877/4878 zero-attribution shape this task exists to remove.
#
# WALLCLOCK-GUARD SAFETY: the comparison is the lower-bound `-gt` (never -le/-lt),
# and the operand vars carry no ELAPSED/_S/_MS/_NS/SECONDS suffix, so
# tests/infra/test_no_new_wallclock_upper_bounds.sh does not fire. These are
# CONFIG CONSTANTS, not measured durations.
# ===========================================================================

# ---------------------------------------------------------------------------
# _offline_wall_secs_for_file <verify.sh> — the offline role's RELEASE wall
# default in seconds, from the `[0-9]+h` default on the offline re-resolution
# line. Prints empty if absent (caught by the non-emptiness assertions below,
# so a broken extractor fails loudly instead of silently comparing zeros).
# ---------------------------------------------------------------------------
_offline_wall_secs_for_file() {
    local file="$1" h
    h="$(grep -oE '_resolve_timeout_knob REIFY_VERIFY_TEST_TIMEOUT_RELEASE [0-9]+h' "$file" \
            | grep -oE '[0-9]+' | head -n1)" || h=""
    [ -n "$h" ] || return 0
    printf '%s' $(( h * 3600 ))
}

_reachability_ok() {
    local toml="$1" vsh="$2" wall atom got
    wall="$(_offline_wall_secs_for_file "$vsh")"
    [ -n "$wall" ] || return 1
    for atom in ${HEAVY_ATOMS+"${HEAVY_ATOMS[@]}"}; do
        got="$(_slow_ceiling_for_filter "$toml" "$atom")"
        [ -n "$got" ] || return 1
        [ "$wall" -gt "$got" ] || return 1
    done
    return 0
}

_reachability_reject() { ! _reachability_ok "$1" "$2"; }

echo ""
echo "--- Assertion L (task 6485): offline release wall > heavy per-test ceiling (both operands derived from files) ---"

_OFFLINE_WALL="$(_offline_wall_secs_for_file "$VERIFY_SH")"

assert "L: offline release wall extracted from scripts/verify.sh (non-empty seconds, got '${_OFFLINE_WALL:-<none>}')" \
    test -n "${_OFFLINE_WALL:-}"

for _atom in ${HEAVY_ATOMS+"${HEAVY_ATOMS[@]}"}; do
    _lgot="$(_slow_ceiling_for_filter "$NEXTEST_TOML" "$_atom")"
    assert "L: heavy ceiling for [${_atom}] extracted from nextest.toml (non-empty seconds, got '${_lgot:-<none>}')" \
        test -n "${_lgot:-}"
    assert "L: offline release wall (${_OFFLINE_WALL:-?}s, from verify.sh) strictly exceeds the heavy ceiling for [${_atom}] (${_lgot:-?}s, from nextest.toml) so nextest kills BY NAME before the wall fires" \
        test "${_OFFLINE_WALL:-0}" -gt "${_lgot:-0}"
done

# ---------------------------------------------------------------------------
# Assertion K/L NON-VACUITY SELF-CHECK. Both K and L are green on arrival, which
# proves nothing on its own: a checker that accepts everything would look
# identical. Each fixture below breaks exactly one thing and the corresponding
# checker must REJECT it.
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion K/L non-vacuity: both checkers REJECT each seeded drift ---"

_KL_FIX="$(mktemp -d)"
_GR_FIRST="${GATE_RESIDENT_FILTERS[0]}"

# (i) an EXTRA override block, belonging to neither class.
cp "$NEXTEST_TOML" "$_KL_FIX/extra.toml"
cat >> "$_KL_FIX/extra.toml" <<'EXTRA'

[[profile.default.overrides]]
filter = 'package(reify-eval) & binary(some_unclassified_newcomer)'
slow-timeout = { period = "120s", terminate-after = 99 }
EXTRA

# (ii) a gate-resident block bumped to the heavy ceiling (the silent-promotion
#      shape: it would then run on the gate with an unreachable 12h ceiling).
awk -v want="$_GR_FIRST" -v q="'" '
    /^\[\[/ { hit = 0 }
    $0 ~ "^[[:space:]]*filter[[:space:]]*=" {
        hit = 0
        if (match($0, q "[^" q "]*" q)) {
            val = substr($0, RSTART + 1, RLENGTH - 2)
            if (val == want) hit = 1
        }
    }
    hit && /^[[:space:]]*slow-timeout[[:space:]]*=/ {
        sub(/terminate-after[[:space:]]*=[[:space:]]*[0-9]+/, "terminate-after = 360")
        hit = 0
    }
    { print }
' "$NEXTEST_TOML" > "$_KL_FIX/promoted.toml"

# (iii) a gate-resident block deleted entirely (the reverse-drift shape).
awk -v want="$_GR_FIRST" -v q="'" '
    function flush(   i) {
        if (!drop) { for (i = 1; i <= n; i++) print b[i] }
        n = 0; drop = 0
    }
    /^\[\[/ { flush() }
    { b[++n] = $0 }
    $0 ~ "^[[:space:]]*filter[[:space:]]*=" {
        if (match($0, q "[^" q "]*" q)) {
            val = substr($0, RSTART + 1, RLENGTH - 2)
            if (val == want) drop = 1
        }
    }
    END { flush() }
' "$NEXTEST_TOML" > "$_KL_FIX/gr-deleted.toml"

# (iv) a verify.sh whose offline default is dropped BELOW the heavy ceiling,
#      making the 12h ceiling unreachable again.
sed 's/_resolve_timeout_knob REIFY_VERIFY_TEST_TIMEOUT_RELEASE [0-9]\+h/_resolve_timeout_knob REIFY_VERIFY_TEST_TIMEOUT_RELEASE 6h/' \
    "$VERIFY_SH" > "$_KL_FIX/verify-6h.sh"

assert "K-neg (i): classifier REJECTS an EXTRA override belonging to neither class (a new override must be classified, not absorbed)" \
    _classify_overrides_reject "$_KL_FIX/extra.toml"

assert "K-neg (ii): classifier REJECTS a gate-resident block silently promoted to the ${HEAVY_CEILING_SECONDS}s heavy ceiling" \
    _classify_overrides_reject "$_KL_FIX/promoted.toml"

assert "K-neg (iii): classifier REJECTS a nextest.toml with an allowlisted gate-resident block deleted" \
    _classify_overrides_reject "$_KL_FIX/gr-deleted.toml"

assert "L-neg (iv): reachability guard REJECTS a verify.sh whose offline wall (6h) no longer exceeds the ${HEAVY_CEILING_SECONDS}s heavy ceiling" \
    _reachability_reject "$NEXTEST_TOML" "$_KL_FIX/verify-6h.sh"

# Positive controls: the SAME checkers accept the real files, so the four
# rejections above are attributable to the seeded drift and not to checkers that
# reject everything.
assert "K-neg control: the same classifier ACCEPTS the real .config/nextest.toml" \
    _classify_overrides_ok "$NEXTEST_TOML"

assert "L-neg control: the same reachability guard ACCEPTS the real nextest.toml + verify.sh pair" \
    _reachability_ok "$NEXTEST_TOML" "$VERIFY_SH"

rm -rf "$_KL_FIX"

test_summary
