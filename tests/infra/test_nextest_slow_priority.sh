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
# 1800s, heavy 2160s (task 7552; 43200s before it). What each tier is for, which
# wall binds which role, and why no role is an accepted residual any more:
# docs/prds/offline-deep-test-lane.md
# DA6 — the normative copy. This file MECHANISES that decision; it does not
# restate it, and a claim about the tiers that is only written down here is
# either a duplicate or a drift.
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
#      heavy ceiling. Heavy membership is DERIVED from
#      scripts/heavy-test-filter-lib.sh, never restated here, so a 9th atom added
#      to the lib fails immediately with no edit to this file. J-gen pins the same
#      ceilings through gen-nextest-config.sh; J-neg is its non-vacuity self-check.
#   K. TOTAL CLASSIFICATION — every slow-timeout override in the file classifies
#      as exactly one of heavy (=> HEAVY_CEILING_SECONDS) or gate-resident (=> 1800s, under the
#      3600s gate wall), and the two classes PARTITION the file. A block matching
#      neither RED-lights until a human classifies it; a deleted allowlisted block
#      fails too. The gate-resident allowlist lives in THIS FILE, deliberately not
#      in the config, so an override cannot be self-classified in the same edit
#      that adds it.
#   L. REACHABILITY — every role that actually runs heavy members either REACHES
#      the ceiling (its binding wall strictly exceeds it, so nextest
#      attributes-and-kills BY NAME before the outer wall fires exit 124 naming
#      nothing) or is an explicitly enumerated residual, allowlisted here AND
#      named in the config's ACCEPTED RESIDUAL paragraph. The heavy-running role
#      set is DERIVED from scripts/verify.sh — accepted roles minus the roles the
#      _GATE_HEAVY_EXCLUDE guard covers — so `background`, which runs the heavy
#      set under the 60m debug wall, is classified rather than overlooked. Every
#      operand comes from a file; none is restated as a literal.

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
# split them: the four HEAVY binaries moved to the heavy ceiling (re-sized twice
# within task 7552; 360 before it), while representation_within_assertion is
# GATE-RESIDENT — it still runs on the merge gate, so it keeps 1800s, strictly
# under the 3600s pass-level wall. Pinning each tier's value separately here is
# what makes a block silently changing tier fail; Assertion K enforces that the
# two tiers exhaust every slow-timeout override in the file.
#
# THE BANNER CARRIES NO TIER VALUE, deliberately. It used to, and it drifted: it
# still announced `terminate-after = 27` after the assertions below had been
# re-pointed at 21, so an agent triaging a red F would have grepped the config
# for a number that appears nowhere in it and concluded the tier had been
# removed rather than re-sized. Each value now has exactly one home in this
# assertion — the literal in the `test`, with its seconds spelled out in the
# description on the line directly above it, where a wrong number is adjacent to
# the thing it is wrong about (heuristic 11).
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion F (task 5141, retargeted 6485/7552): each tier's terminate-after, pinned per block ---"

ST_T0A="$(_slow_terminate_for reify-eval tensegrity_t0a)"
ST_FEA="$(_slow_terminate_for reify-eval-fea-tests fea_diagnostics_e2e)"
ST_REPR="$(_slow_terminate_for reify-eval representation_within_assertion)"
ST_ANAL="$(_slow_terminate_for reify-solver-elastic analytical_validation)"
ST_DET="$(_slow_terminate_for reify-solver-elastic determinism)"

assert "nextest.toml: tensegrity_t0a override has slow-timeout terminate-after = 18 (heavy tier, 2160s)" \
    test "${ST_T0A:-}" = "18"

assert "nextest.toml: fea_diagnostics_e2e override has slow-timeout terminate-after = 18 (heavy tier, 2160s)" \
    test "${ST_FEA:-}" = "18"

assert "nextest.toml: representation_within_assertion override has slow-timeout terminate-after = 15 (gate-resident tier, 1800s)" \
    test "${ST_REPR:-}" = "15"

assert "nextest.toml: analytical_validation override has slow-timeout terminate-after = 18 (heavy tier, 2160s)" \
    test "${ST_ANAL:-}" = "18"

assert "nextest.toml: determinism override has slow-timeout terminate-after = 18 (heavy tier, 2160s)" \
    test "${ST_DET:-}" = "18"

# ---------------------------------------------------------------------------
# Assertion G (task 5141): gen-nextest-config.sh preserves each heavy-tier
# slow-timeout verbatim in the generated temp config (compile-free — the
# generator only runs sed on the occt max-threads line, never cargo/nextest).
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion G (task 5141, retargeted 6485/7552): gen-nextest-config.sh preserves each tier's slow-timeout ---"

_TMP_CFG_ST="$(REIFY_OCCT_NEXTEST_MAX_THREADS=24 bash "$GEN_CFG")"

_GST_T0A="$(_slow_terminate_for_file "$_TMP_CFG_ST" reify-eval tensegrity_t0a)"
_GST_FEA="$(_slow_terminate_for_file "$_TMP_CFG_ST" reify-eval-fea-tests fea_diagnostics_e2e)"
_GST_REPR="$(_slow_terminate_for_file "$_TMP_CFG_ST" reify-eval representation_within_assertion)"
_GST_ANAL="$(_slow_terminate_for_file "$_TMP_CFG_ST" reify-solver-elastic analytical_validation)"
_GST_DET="$(_slow_terminate_for_file "$_TMP_CFG_ST" reify-solver-elastic determinism)"

rm -f "$_TMP_CFG_ST"

assert "gen-nextest-config.sh: tensegrity_t0a slow-timeout terminate-after = 18 preserved in generated config" \
    test "${_GST_T0A:-}" = "18"

assert "gen-nextest-config.sh: fea_diagnostics_e2e slow-timeout terminate-after = 18 preserved in generated config" \
    test "${_GST_FEA:-}" = "18"

assert "gen-nextest-config.sh: representation_within_assertion slow-timeout terminate-after = 15 preserved in generated config" \
    test "${_GST_REPR:-}" = "15"

assert "gen-nextest-config.sh: analytical_validation slow-timeout terminate-after = 18 preserved in generated config" \
    test "${_GST_ANAL:-}" = "18"

assert "gen-nextest-config.sh: determinism slow-timeout terminate-after = 18 preserved in generated config" \
    test "${_GST_DET:-}" = "18"

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
# slow-timeout override at the heavy ceiling.
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
# _slow_timeout_blocks_for_file <file> — ONE parse, two views (see the two
# wrappers below). Emits one `<ceiling-seconds><TAB><filter-value>` line per
# TOML table that carries BOTH a `filter` and a `slow-timeout` key. Ceiling is
# period-seconds * terminate-after. Tables with a filter but no slow-timeout
# (the occt test-group routing block) are correctly omitted: they set a
# different SETTING and this guard is about ceilings.
#
# BLOCK-BUFFERED AND QUOTE-AGNOSTIC, deliberately. The first form of this parse
# walked line-by-line and only saw a block whose `slow-timeout` line came AFTER
# its `filter` line, with the value single-quoted. TOML imposes no key order and
# permits either quote character, so an override authored the other way round —
# or with "double quotes" — was INVISIBLE here. That mattered asymmetrically:
# Assertion J failed CLOSED on it (no block found => `<no block>` => red), but
# Assertion K's total classification failed OPEN, silently absorbing exactly the
# unclassified newcomer it exists to catch. Buffering the whole table and reading
# both keys out of it regardless of order closes that.
#
# `/^\[/` (any table header, not just `^\[\[`) terminates a block: a following
# `[profile.X]` must not let one table's filter pair with a later table's
# slow-timeout.
# ---------------------------------------------------------------------------
_slow_timeout_blocks_for_file() {
    local file="$1"
    awk '
        function emit(   pseg, tseg, period, term) {
            if (fil != "" && sto != "") {
                period = 0; term = 0
                if (match(sto, /period[[:space:]]*=[[:space:]]*"[0-9]+s"/)) {
                    pseg = substr(sto, RSTART, RLENGTH)
                    match(pseg, /[0-9]+/)
                    period = substr(pseg, RSTART, RLENGTH) + 0
                }
                if (match(sto, /terminate-after[[:space:]]*=[[:space:]]*[0-9]+/)) {
                    tseg = substr(sto, RSTART, RLENGTH)
                    match(tseg, /[0-9]+$/)
                    term = substr(tseg, RSTART, RLENGTH) + 0
                }
                printf "%d\t%s\n", period * term, fil
            }
            fil = ""; sto = ""
        }
        /^\[/ { emit() }
        $0 ~ "^[[:space:]]*filter[[:space:]]*=" {
            s = $0
            sub(/^[[:space:]]*filter[[:space:]]*=[[:space:]]*/, "", s)
            qc = substr(s, 1, 1)
            if (qc == "\047" || qc == "\"") {
                rest = substr(s, 2)
                i = index(rest, qc)
                if (i > 0) {
                    val = substr(rest, 1, i - 1)
                    gsub(/[[:space:]]+/, " ", val)
                    sub(/^ /, "", val)
                    sub(/ $/, "", val)
                    fil = val
                }
            }
        }
        $0 ~ "^[[:space:]]*slow-timeout[[:space:]]*=" { sto = $0 }
        END { emit() }
    ' "$file"
}

# ---------------------------------------------------------------------------
# Helper (task 6485): ceiling (period-seconds * terminate-after) for the
# [[profile.default.overrides]] block whose `filter` VALUE equals <filter>
# exactly (whitespace-normalized). Keys on the whole filter value instead of
# the package+binary pair the _slow_period_for_file / _slow_terminate_for_file
# helpers above use.
#
# REQUIRED, not stylistic: the two test-scoped heavy atoms share BOTH package
# (reify-eval) AND binary (harness_fea_solver_e2e), so the package+binary
# helpers above cannot tell them apart -- they would match whichever block
# came first and silently report one ceiling for both.
# Usage: _slow_ceiling_for_filter <file> <filter-value>
# ---------------------------------------------------------------------------
_slow_ceiling_for_filter() {
    local file="$1" want="$2" ceil fil
    while IFS="$(printf '\t')" read -r ceil fil; do
        [ "$fil" = "$want" ] && { printf '%s' "$ceil"; return 0; }
    done < <(_slow_timeout_blocks_for_file "$file")
    return 0
}

# The heavy per-test ceiling: 2160s = 120s x 18 (task 7552; was 3240s and then
# 2520s within the same task, and 43200s before it).
# Basis, in one line: it is a multiple of the 120s period sitting inside
# `[3 x measured per-test max, binding wall MINUS start-offset budget]` for the
# tightest heavy-running role (background: [1635.6s, 2521.3s]), clearing the
# false-kill floor at 4.0x and the reachability bound by 361s.
# The `0.9 x wall` rule this constant carried for one commit is SUPERSEDED: it
# subtracted a fixed 10% where the quantity that actually has to be subtracted is
# the time the pass takes to reach the test, which is not a fraction of the wall
# and is measured, not assumed.
# NOT THE TOP OF THE WINDOW, deliberately. 2520s — the largest step the bound
# allows — was landed first and left 1.3s of slack against L_START_OFFSET_BUDGET_
# SECONDS below, which is a max over two WARM-target runs on one host and so
# under-measures the build term a cold lane pays. Sitting one step lower keeps
# ~33% margin over that budget while spending only false-kill margin that was
# already ~4x. Derivation, the measurement it rests on and the alternatives
# rejected for it are normative in docs/prds/offline-deep-test-lane.md DA6 — do
# not restate them here.
HEAVY_CEILING_SECONDS=2160

# The START-OFFSET BUDGET: worst-case seconds from the `cargo nextest run`
# invocation to a heavy test's PROCESS start (task 7552 amendment).
#
# WHY IT EXISTS. verify.sh's `timeout` clock starts at PASS start and wraps what
# its own comment calls "one combined build+execution nextest pass per profile";
# nextest's `terminate-after` clock starts at TEST-PROCESS start. The two do not
# start together, so a ceiling merely SMALLER than the wall proves nothing — it
# has to be smaller by more than the pass takes to reach the test. Assertion L
# compared bare wall against bare ceiling until this constant existed, and was
# green on a ceiling `background` could not actually reach.
#
# BASIS. The max over N=2 full debug `--workspace` runs of the LATEST-starting
# heavy atom's offset (1078.7s and 315.5s, rounded up to the whole second), under
# natural host contention. Two terms, both measured: the combined build the outer
# `timeout` also wraps, and the un-prioritised heavy atoms' position in the test
# queue — the LPT `priority` overrides cover only 4 of the 8, so the other three
# start several hundred seconds after the first test. It is a max over two
# observations on one host and a bound in no stronger sense.
#
# WHY IT IS A LITERAL, unlike every other operand here. The start offset is a
# property of the BUILD, not of any config file, so there is nothing in the tree
# to derive it from. It is compared only against a DIFFERENCE of two
# file-derived numbers, never against itself, so no assertion below can pass by
# comparing a literal to itself. Evidence:
# docs/notes/heavy-test-per-test-duration-measurement.md. Derivation and the
# constraints it feeds: docs/prds/offline-deep-test-lane.md DA6.
L_START_OFFSET_BUDGET_SECONDS=1079

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
echo "--- Assertion J (task 6485): every heavy-filter atom has a ${HEAVY_CEILING_SECONDS}s heavy-tier override block ---"

# Non-vacuity floor for the PARSE itself. Deliberately '>= 1', not '== 8': a
# 9th atom added to the lib must fail in the per-atom checks below (naming the
# offender), never here with an unhelpful count mismatch.
echo "    (parsed ${#HEAVY_ATOMS[@]} heavy atoms from scripts/heavy-test-filter-lib.sh)"
assert "heavy-filter lib parsed into at least one atom (guard is non-vacuous)" \
    test "${#HEAVY_ATOMS[@]}" -ge 1

for _atom in ${HEAVY_ATOMS+"${HEAVY_ATOMS[@]}"}; do
    _got="$(_slow_ceiling_for_filter "$NEXTEST_TOML" "$_atom")"
    assert "nextest.toml: heavy atom [${_atom}] has an override block at the ${HEAVY_CEILING_SECONDS}s heavy ceiling (got '${_got:-<no block>}')" \
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
echo "--- Assertion J-gen (task 6485): gen-nextest-config.sh preserves every heavy ceiling ---"

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
#                    REIFY_HEAVY_NEXTEST_FILTER => ceiling must be HEAVY_CEILING_SECONDS.
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

# ---------------------------------------------------------------------------
# WALL EXTRACTORS (task 6485). Each reads ONE specific `_resolve_timeout_knob`
# default out of scripts/verify.sh and converts it to seconds; each prints empty
# when its anchor stops matching, which the non-emptiness assertions turn into a
# loud failure rather than a silent comparison against zero.
#
# EVERY ONE IS WINDOWED to the construct it claims to read, never `head -n1` over
# a file-wide grep. verify.sh mentions these knob names in prose comments as well
# as in code, and the release knob now has TWO defaults (the base one and the
# offline re-resolution). A first-match-anywhere grep would work today purely by
# accident of ordering, and would silently substitute one role's wall for
# another's the moment a second role gained its own default — reporting a role
# REACHABLE against a wall it does not run under. Windowing is the same technique
# _heavy_excluded_roles_for_file uses, for the same reason.
# ---------------------------------------------------------------------------

# _debug_wall_secs_for_file <verify.sh> — the DEBUG (--workspace) pass wall.
# Anchored on the unconditional assignment at column 0, so neither the comment
# quoting this grep nor the `_RELEASE` knob's own line can match.
_debug_wall_secs_for_file() {
    local file="$1" m
    m="$(grep -oE '^_VERIFY_TEST_TIMEOUT="\$\(_resolve_timeout_knob REIFY_VERIFY_TEST_TIMEOUT [0-9]+m' "$file" \
            | grep -oE '[0-9]+m$' | tr -d 'm')" || m=""
    [ -n "$m" ] || return 0
    printf '%s' $(( m * 60 ))
}

# _base_release_wall_secs_for_file <verify.sh> — the RELEASE pass wall that
# applies to every role the offline re-resolution below does NOT cover.
# Anchored on its own unconditional assignment at column 0.
_base_release_wall_secs_for_file() {
    local file="$1" m
    m="$(grep -oE '^_VERIFY_TEST_TIMEOUT_RELEASE="\$\(_resolve_timeout_knob REIFY_VERIFY_TEST_TIMEOUT_RELEASE [0-9]+m' "$file" \
            | grep -oE '[0-9]+m$' | tr -d 'm')" || m=""
    [ -n "$m" ] || return 0
    printf '%s' $(( m * 60 ))
}

# _offline_wall_secs_for_file <verify.sh> — the role-scoped RELEASE wall, read
# from INSIDE the `if [ "${DF_VERIFY_ROLE:-task}" = "<role>" ]` … `fi` block that
# re-resolves it, not from the first `Nh` anywhere in the file.
_offline_wall_secs_for_file() {
    local file="$1" h
    h="$(_release_scope_block_for_file "$file" \
            | grep -oE '_resolve_timeout_knob REIFY_VERIFY_TEST_TIMEOUT_RELEASE [0-9]+h' \
            | grep -oE '[0-9]+h$' | tr -d 'h')" || h=""
    [ -n "$h" ] || return 0
    printf '%s' $(( h * 3600 ))
}

# _release_scope_block_for_file <verify.sh> — the body of the role-scoped
# release re-resolution: the `if` line that tests DF_VERIFY_ROLE and immediately
# assigns _VERIFY_TEST_TIMEOUT_RELEASE, through its closing `fi`. Both the wall
# value and the role it is scoped to are read out of this one window, so the two
# can never be taken from different constructs.
_release_scope_block_for_file() {
    awk '
        /^if \[ "\$\{DF_VERIFY_ROLE:-[a-z]+\}" = "[a-z]+" \]; then$/ { buf = $0; n = 1; win = 1; next }
        win && /^fi$/ { if (hit) print buf ORS $0; win = 0; hit = 0; buf = ""; next }
        win {
            buf = buf ORS $0
            if ($0 ~ /_VERIFY_TEST_TIMEOUT_RELEASE=/) hit = 1
        }
    ' "$1"
}

# _release_scoped_roles_for_file <verify.sh> — the role(s) whose RELEASE wall the
# block above re-scopes. Today: offline. Derived rather than named so that
# re-pointing the scope at a different role moves the wall with it here too.
_release_scoped_roles_for_file() {
    _release_scope_block_for_file "$1" \
        | grep -oE 'DF_VERIFY_ROLE:-[a-z]+\}" = "[a-z]+"' \
        | grep -oE '= "[a-z]+"$' | tr -d '= "'
}

# The gate-resident tier: overrides that DO still run on the merge gate, so the
# pass-level wall really binds them and their ceiling must stay under it.
GATE_RESIDENT_FILTERS=(
    'package(reify-eval) & binary(representation_within_assertion)'
    'package(reify-eval) & binary(solve_elastic_static_body_e2e)'
)
GATE_RESIDENT_CEILING_SECONDS=1800

# DERIVED, not a literal. This is the merge role's binding DEBUG wall — the same
# number Assertion L reads for the background role — and the gate-resident tier
# exists precisely to stay under it. Hardcoding 3600 here would mean lowering
# verify.sh's debug default (to 20m, say) silently left the two gate-resident
# 1800s ceilings unreachable on the BLOCKING gate with nothing going red: the
# zero-attribution shape, reintroduced where it matters most. Assertion H's own
# comment records this same defect being fixed once already.
GATE_WALL_SECONDS="$(_debug_wall_secs_for_file "$VERIFY_SH")"

# ---------------------------------------------------------------------------
# _slow_timeout_filters_for_file <file> — the filter-value view of
# _slow_timeout_blocks_for_file: one line per override block that carries a
# slow-timeout key. Sharing that one parse is what keeps K's enumeration and
# J's lookup agreeing about which blocks exist; two parsers would let a block
# be visible to one and not the other, which is how K came to fail OPEN.
# ---------------------------------------------------------------------------
_slow_timeout_filters_for_file() {
    _slow_timeout_blocks_for_file "$1" | cut -f2-
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

# The gate-resident bound has two operands and BOTH must come from a file. This
# is the one for the wall; the ceiling operand is read from .config/nextest.toml
# per block below.
assert "K: gate wall extracted from scripts/verify.sh (non-empty seconds, got '${GATE_WALL_SECONDS:-<none>}') — the gate-resident bound compares two file-derived numbers, not one number against a literal" \
    test -n "${GATE_WALL_SECONDS:-}"

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

# ---------------------------------------------------------------------------
# K-tier: the two tiers must stay ORDERED — every heavy ceiling strictly exceeds
# every gate-resident ceiling. Both operands are read out of .config/nextest.toml
# through _slow_ceiling_for_filter, never from this script's own constants, which
# would be a literal compared against itself.
#
# NEWLY LOAD-BEARING (task 7552). Until this task the two tiers were 24x apart
# (43200s vs 1800s) and could not plausibly collide, so nothing asserted the
# order. At 2160s they are within 1.2x, and the next re-tune in either direction
# could flatten `heavy` into `gate-resident` — or invert them — while every other
# assertion here stayed green, because K classifies by FILTER and each tier's
# value is only ever compared against its own class.
# ---------------------------------------------------------------------------
_MIN_HEAVY_CEIL=""
for _a in ${HEAVY_ATOMS+"${HEAVY_ATOMS[@]}"}; do
    _c="$(_slow_ceiling_for_filter "$NEXTEST_TOML" "$_a")"
    [ -n "$_c" ] || continue
    if [ -z "$_MIN_HEAVY_CEIL" ] || [ "$_c" -lt "$_MIN_HEAVY_CEIL" ]; then _MIN_HEAVY_CEIL="$_c"; fi
done
_MAX_GR_CEIL=""
for _g in ${GATE_RESIDENT_FILTERS+"${GATE_RESIDENT_FILTERS[@]}"}; do
    _c="$(_slow_ceiling_for_filter "$NEXTEST_TOML" "$_g")"
    [ -n "$_c" ] || continue
    if [ -z "$_MAX_GR_CEIL" ] || [ "$_c" -gt "$_MAX_GR_CEIL" ]; then _MAX_GR_CEIL="$_c"; fi
done

assert "K-tier: both tier ceilings extracted from .config/nextest.toml (heavy min '${_MIN_HEAVY_CEIL:-<none>}', gate-resident max '${_MAX_GR_CEIL:-<none>}') — the ordering below compares two file-derived numbers" \
    bash -c '[ -n "$1" ] && [ -n "$2" ]' _ "${_MIN_HEAVY_CEIL:-}" "${_MAX_GR_CEIL:-}"

assert "K-tier: the smallest HEAVY ceiling (${_MIN_HEAVY_CEIL:-?}s) strictly exceeds the largest GATE-RESIDENT ceiling (${_MAX_GR_CEIL:-?}s) — heavy must remain a strictly higher tier, not merely a differently-named one" \
    test "${_MIN_HEAVY_CEIL:-0}" -gt "${_MAX_GR_CEIL:-0}"

# ===========================================================================
# Assertion L (task 6485): REACHABILITY — every role that actually runs heavy
# members either REACHES the heavy per-test ceiling or is an explicitly
# enumerated, doc-agreed residual.
#
# THE INVARIANT, in one line: a hung heavy test must be SIGTERM'd BY NAME by
# nextest's per-test ceiling before the pass-level `timeout` wall fires exit 124
# attributing nothing (the task 4877/4878 shape) — which needs the binding wall
# to exceed the ceiling BY MORE than the pass takes to reach the test, not
# merely to exceed it — and on any role where that is NOT true, the role must be
# named both in L_RESIDUAL_ROLES below and in .config/nextest.toml's ACCEPTED
# RESIDUAL paragraph, so the gap is a recorded decision rather than an accident.
#
# THAT QUALIFIER IS THE WHOLE OF THE 7552 AMENDMENT, and it is not pedantry: the
# first corrected form of this file asserted the bare `wall > ceiling` and went
# green on a 3240s ceiling under `background`'s 3600s wall, leaving 360s of
# headroom for a pass whose measured build-plus-queue term is 1078.7s. The wrong
# rule was the ASSERTED rule, which is how the same claim survived simultaneous
# review of the config, the PRD and this guard.
#
# WHY THE ROLE SET IS DERIVED, NOT ASSUMED. The first form of this assertion
# covered the offline role alone and missed `background` entirely — a live
# orchestrator-spawned role that runs all 8 heavy members 12x under the ceiling.
# The heavy-running set is therefore computed from scripts/verify.sh (accepted
# roles MINUS heavy-excluded roles), so a role added to either list lands in this
# classification with no edit here.
#
# WHAT THIS COMMENT MUST NOT SAY: "no gate path runs heavy members". That claim
# was false as written — the env var is set for every orchestrator-spawned role,
# but verify.sh scopes its EFFECT to task|merge, so setting it says nothing about
# background or offline. (Mechanism and the accepted residual:
# docs/prds/offline-deep-test-lane.md DA6.)
#
# EVERY OPERAND IS DERIVED FROM A FILE, WITH ONE DELIBERATE EXCEPTION —
# including the role => binding-wall mapping itself. The ceiling comes from
# .config/nextest.toml; from scripts/verify.sh come both role sets, the two wall
# defaults, which role the release wall is re-scoped for, AND which profile each
# role forces (so a role running `both` is judged against the TIGHTER of its two
# walls). Naming any of those instead of reading them re-opens the same hole: an
# assertion whose operands are literals cannot fail whatever the files say, and
# a mapping that is a CONSEQUENCE of verify.sh is a literal in disguise.
#
# THE EXCEPTION is L_START_OFFSET_BUDGET_SECONDS, which is measured and cannot
# be otherwise: the seconds a pass takes to reach a heavy test are a property of
# the BUILD and of nextest's scheduling, and no file in the tree states them. It
# is kept honest by being compared only against a DIFFERENCE of two file-derived
# numbers — so a drifting ceiling or a drifting wall still moves the comparison,
# and no assertion here compares a literal to itself.
#
# WALLCLOCK-GUARD SAFETY: every comparison here is a lower bound (-gt/-ge, never
# -le/-lt) and no description uses an elapsed/duration/within-Ns lexeme. These
# comparisons are between CONFIG CONSTANTS, not between a measured run time and
# a deadline, so tests/infra/test_no_new_wallclock_upper_bounds.sh does not fire.
# L_START_OFFSET_BUDGET_SECONDS does carry a SECONDS suffix and IS measured, so
# keep it on the lower-bound side of every comparison it appears in: written as
# an upper bound it would be indistinguishable to that ratchet from the
# wall-clock flake class it exists to keep out.
# ===========================================================================

# Roles whose binding wall does NOT reach the heavy ceiling and which are
# accepted as such — deliberately, with the gap recorded in .config/nextest.toml.
#
# SELF-CLEANING BY CONSTRUCTION: every entry must STILL be a heavy-running role
# (asserted below). When verify.sh's exclusion is extended to cover one, that
# role leaves the heavy-running set, this entry goes stale and RED-lights —
# forcing the allowlist entry and the config's residual note to be retired
# together instead of one outliving the other.
# EMPTY since task 7552, deliberately and not by omission: the heavy ceiling was
# re-sized BELOW the tightest heavy-running wall, so `background` — the one entry
# this array ever carried — became REACHABLE and its entry was retired together
# with the config's residual note, exactly as the paragraph above prescribes.
# A future role that cannot reach the ceiling still lands here.
L_RESIDUAL_ROLES=()

# (The wall extractors — _debug_wall_secs_for_file, _base_release_wall_secs_for_file,
# _offline_wall_secs_for_file and the release-scope window they share — are
# defined above Assertion K, which needs the debug wall for its gate-resident
# bound.)

# ---------------------------------------------------------------------------
# _heavy_excluded_roles_for_file <verify.sh> — the roles the `-E "not (<heavy>)"`
# fragment is scoped to, one per line, parsed from the _GATE_HEAVY_EXCLUDE
# guard's `[ "$DF_VERIFY_ROLE" = "<role>" ]` operands. Today: task, merge.
#
# The parse is windowed to that guard (its `_GATE_HEAVY_EXCLUDE=""` initializer
# through the closing `fi`) because the same role-equality idiom appears dozens
# of times elsewhere in verify.sh — an unwindowed grep would report every role
# the script mentions.
# ---------------------------------------------------------------------------
_heavy_excluded_roles_for_file() {
    local file="$1"
    awk '
        /^_GATE_HEAVY_EXCLUDE=""$/ { win = 1; next }
        win && /^fi$/ { win = 0 }
        win {
            s = $0
            while (match(s, /DF_VERIFY_ROLE"[[:space:]]*=[[:space:]]*"[a-z_]+"/)) {
                st = RSTART; ln = RLENGTH
                seg = substr(s, st, ln)
                if (match(seg, /"[a-z_]+"$/)) print substr(seg, RSTART + 1, RLENGTH - 2)
                s = substr(s, st + ln)
            }
        }
    ' "$file"
}

# ---------------------------------------------------------------------------
# _all_roles_for_file <verify.sh> — the full accepted role set, one per line,
# split out of the unknown-role error's `want a|b|c` spec. That message is the
# single place verify.sh enumerates its roles, so a new role cannot be added
# without passing through it. Anchored on the error text so a `want ...` phrase
# in unrelated prose cannot be mistaken for the role list.
# ---------------------------------------------------------------------------
_all_roles_for_file() {
    local file="$1" spec
    spec="$(grep -F 'unknown DF_VERIFY_ROLE' "$file" \
            | grep -oE 'want [a-z]+(\|[a-z]+)+' | head -n1)" || spec=""
    [ -n "$spec" ] || return 0
    printf '%s\n' "${spec#want }" | tr '|' '\n'
}

# ---------------------------------------------------------------------------
# _heavy_running_roles_for_file <verify.sh> — accepted roles MINUS heavy-excluded
# roles: exactly the roles on which a heavy test can actually run. Today:
# offline, background.
# ---------------------------------------------------------------------------
_heavy_running_roles_for_file() {
    local file="$1" role
    local -a excluded=()
    while IFS= read -r role; do
        [ -n "$role" ] && excluded+=("$role")
    done < <(_heavy_excluded_roles_for_file "$file")
    while IFS= read -r role; do
        [ -n "$role" ] || continue
        _in_list "$role" ${excluded+"${excluded[@]}"} || printf '%s\n' "$role"
    done < <(_all_roles_for_file "$file")
}

# ---------------------------------------------------------------------------
# _role_profile_for_file <verify.sh> <role> — the profile <role> runs when no
# explicit --profile is given: the value its branch of the role-based PROFILE
# default assigns, or the script's base `PROFILE="..."` initializer for a role
# no branch names.
#
# The parse is windowed to that `if [ "$PROFILE_EXPLICIT" -eq 0 ]` … `fi` block.
# Within it, each if/elif condition names one or more roles and the assignment
# that follows is theirs.
# ---------------------------------------------------------------------------
_role_profile_for_file() {
    local file="$1" role="$2" got
    got="$(awk -v want="$role" '
        /^if \[ "\$PROFILE_EXPLICIT" -eq 0 \]/ { win = 1 }
        win && /^fi$/ { win = 0 }
        win && /^(if|elif) / {
            hit = 0
            s = $0
            while (match(s, /DF_VERIFY_ROLE"[[:space:]]*=[[:space:]]*"[a-z_]+"/)) {
                seg = substr(s, RSTART, RLENGTH)
                if (match(seg, /"[a-z_]+"$/) && substr(seg, RSTART + 1, RLENGTH - 2) == want) hit = 1
                s = substr(s, RSTART + RLENGTH)
            }
        }
        win && hit && match($0, /^[[:space:]]*PROFILE="[a-z]+"$/) {
            match($0, /"[a-z]+"$/)
            print substr($0, RSTART + 1, RLENGTH - 2)
            exit
        }
    ' "$file")"
    if [ -n "$got" ]; then
        printf '%s' "$got"
        return 0
    fi
    grep -oE '^PROFILE="[a-z]+"$' "$file" | head -n1 | grep -oE '"[a-z]+"' | tr -d '"'
}

# ---------------------------------------------------------------------------
# _role_wall_secs <verify.sh> <role> — the wall that BINDS heavy members on
# <role>, in seconds; empty if any wall the role needs could not be extracted
# (which makes the role unclassifiable below, i.e. RED, which is the point).
#
# DERIVED END TO END, deliberately. The first form of this helper named each
# role's profile in a literal case statement — offline=>release, background=>debug
# — which made L's "every operand comes from a file" claim false at its most
# load-bearing joint. That mapping is a CONSEQUENCE of verify.sh's role-based
# PROFILE defaults, one `||` away from changing: had offline's branch flipped to
# PROFILE="both" (the same one-line shape merge and background already have), it
# would run a debug pass under the 60m wall against a ceiling sized for the 13h
# release wall — the exact
# unreachable-ceiling shape L exists to prevent — while a hardcoded map went on
# comparing against the 13h release wall and reported REACHABLE. A false green on
# L's core invariant, covered by nothing else. Fixture (viii) pins it.
#
# A role running BOTH profiles is bound by the TIGHTER of the two walls, since
# the heavy members run in each pass and the first wall to fire ends the run.
# Conservative in the safe direction: a `both` role whose debug pass happened to
# be narrow (no heavy members) would be judged against a wall stricter than the
# one that really binds it, which can only over-report a gap, never hide one.
# ---------------------------------------------------------------------------
_role_wall_secs() {
    local file="$1" role="$2" profile wall min=""
    profile="$(_role_profile_for_file "$file" "$role")"
    [ -n "$profile" ] || return 0
    case "$profile" in
        debug)   set -- debug ;;
        release) set -- release ;;
        both)    set -- debug release ;;
        *)       return 0 ;;
    esac
    for wall in "$@"; do
        case "$wall" in
            debug) wall="$(_debug_wall_secs_for_file "$file")" ;;
            release)
                if _in_list "$role" $(_release_scoped_roles_for_file "$file"); then
                    wall="$(_offline_wall_secs_for_file "$file")"
                else
                    wall="$(_base_release_wall_secs_for_file "$file")"
                fi
                ;;
        esac
        [ -n "$wall" ] || return 0
        if [ -z "$min" ] || [ "$min" -gt "$wall" ]; then min="$wall"; fi
    done
    printf '%s' "$min"
}

# ---------------------------------------------------------------------------
# _max_heavy_ceiling_for_file <file> — the largest per-test ceiling across the
# heavy atoms, or empty if any atom has no block at all. Comparing a wall
# against this maximum is equivalent to comparing it against every atom's
# ceiling, and keeps the failure message to one number.
# ---------------------------------------------------------------------------
_max_heavy_ceiling_for_file() {
    local file="$1" atom got max=""
    for atom in ${HEAVY_ATOMS+"${HEAVY_ATOMS[@]}"}; do
        got="$(_slow_ceiling_for_filter "$file" "$atom")"
        [ -n "$got" ] || return 0
        if [ -z "$max" ] || [ "$got" -gt "$max" ]; then max="$got"; fi
    done
    printf '%s' "$max"
}

# ---------------------------------------------------------------------------
# _residual_role_documented <nextest.toml> <role> — true iff the ACCEPTED
# RESIDUAL paragraph of the [profile.default] header comment carries an ENTRY
# LINE for <role>: `#   <role> — <reason>`. The paragraph runs from its opener to
# the end of that contiguous comment block, so a `#`-separated continuation still
# counts.
#
# Requiring the prose AND the allowlist to agree is what stops a residual from
# being allowlisted silently in this test file alone: the gap has to be readable
# by someone opening the config with no knowledge that this guard exists.
#
# WHY AN ENTRY LINE AND NOT A WORD MATCH (task 7552 amendment; heuristic 12 —
# structured data rather than a meaningful string). This checker used to match
# the role name ANYWHERE in the paragraph, so the paragraph's own RETIRED-PATH
# HISTORY satisfied it: the live config says "ACCEPTED RESIDUAL — NONE" and then
# names `background` while explaining that its gap CLOSED, and a word match read
# that sentence as a record that the gap is OPEN. The two-sided contract was
# therefore already half-satisfied by a sentence asserting the opposite, and the
# next role to become unreachable could have been allowlisted here and gone green
# with nobody touching the config at all. An entry line is a declaration and a
# sentence is not; prose about a closed path no longer votes.
#
# Both the opener anchor and the entry separator are SHARED constants, not
# literals repeated in the seeding fixture below. Reworded punctuation after
# `ACCEPTED RESIDUAL` once broke this match; had the fixture carried its own copy
# it would have stopped seeding at the same moment the checker stopped checking,
# and the pair would have gone quietly vacuous together instead of going red.
# `--` is accepted alongside the em dash so an ASCII-typed entry still registers.
# ---------------------------------------------------------------------------
RESIDUAL_PAR_ANCHOR='^#[[:space:]]*ACCEPTED RESIDUAL'
RESIDUAL_ENTRY_SEP='—'

_residual_role_documented() {
    local file="$1" role="$2"
    awk -v role="$role" -v anchor="$RESIDUAL_PAR_ANCHOR" -v sep="$RESIDUAL_ENTRY_SEP" '
        $0 ~ anchor { par = 1 }
        par && !/^#/ { par = 0 }
        par && $0 ~ ("^#[[:space:]]+" role "[[:space:]]+(" sep "|--)[[:space:]]") { found = 1 }
        END { exit(found ? 0 : 1) }
    ' "$file"
}

# ---------------------------------------------------------------------------
# _role_class <nextest.toml> <verify.sh> <role> — prints REACHABLE or RESIDUAL,
# or returns 1 for a role belonging to neither class. Written once and used by
# both the per-role assertions and the boolean whole-file checker below, so the
# fixtures exercise the very logic the assertions report.
#
# THE PREDICATE LIVES HERE AND NOWHERE ELSE (task 7552 amendment). It was
# `wall > ceiling`, which compares two clocks that do not start together and was
# therefore green on a ceiling no hung test could ever reach under the binding
# wall. The corrected rule is `wall - ceiling > start-offset budget`, and the
# RESIDUAL branch's mislabel guard is its EXACT complement — written as the
# complement deliberately, since a residual is by definition a role the
# REACHABLE branch rejected, and any gap between the two conditions would leave
# a role the caller must classify but neither branch claims.
# ---------------------------------------------------------------------------
_role_class() {
    local toml="$1" vsh="$2" role="$3" wall ceiling
    ceiling="$(_max_heavy_ceiling_for_file "$toml")"
    [ -n "$ceiling" ] || return 1
    wall="$(_role_wall_secs "$vsh" "$role")"
    [ -n "$wall" ] || return 1
    if [ "$(( wall - ceiling ))" -gt "$L_START_OFFSET_BUDGET_SECONDS" ]; then
        printf 'REACHABLE'
        return 0
    fi
    if _in_list "$role" ${L_RESIDUAL_ROLES+"${L_RESIDUAL_ROLES[@]}"} \
        && _residual_role_documented "$toml" "$role" \
        && [ "$(( ceiling + L_START_OFFSET_BUDGET_SECONDS ))" -ge "$wall" ]; then
        printf 'RESIDUAL'
        return 0
    fi
    return 1
}

# ---------------------------------------------------------------------------
# _role_classification_ok <nextest.toml> <verify.sh> — returns 0 iff EVERY
# heavy-running role classifies, AND every L_RESIDUAL_ROLES entry is still a
# heavy-running role (the self-cleaning direction). The boolean form exists so
# the non-vacuity self-check can point the same logic at broken fixture copies.
# ---------------------------------------------------------------------------
_role_classification_ok() {
    local toml="$1" vsh="$2" role
    local -a running=()
    while IFS= read -r role; do
        [ -n "$role" ] && running+=("$role")
    done < <(_heavy_running_roles_for_file "$vsh")
    [ "${#running[@]}" -ge 1 ] || return 1
    for role in "${running[@]}"; do
        _role_class "$toml" "$vsh" "$role" >/dev/null || return 1
    done
    for role in ${L_RESIDUAL_ROLES+"${L_RESIDUAL_ROLES[@]}"}; do
        _in_list "$role" ${running+"${running[@]}"} || return 1
    done
    return 0
}

_role_classification_reject() { ! _role_classification_ok "$1" "$2"; }

# ---------------------------------------------------------------------------
# _with_residuals "<role...>" <command> [args...] — runs any command
# (_role_class, _role_classification_ok) with the residual allowlist REPLACED
# for the duration of the call. bash's dynamic scoping makes the local shadow
# the global that both functions read, so no parameter has to be threaded
# through either.
#
# This exists because the live allowlist is EMPTY (task 7552) and a checker
# whose residual branch is never entered is a checker whose residual branch is
# untested. The synthetic-role fixtures below drive that branch without
# re-introducing a live residual.
# ---------------------------------------------------------------------------
_with_residuals() {
    local -a L_RESIDUAL_ROLES=()
    read -r -a L_RESIDUAL_ROLES <<< "$1"
    shift
    "$@"
}

_role_classification_ok_with_residuals() { _with_residuals "$1" _role_classification_ok "$2" "$3"; }

_role_classification_reject_with_residuals() { ! _role_classification_ok_with_residuals "$@"; }

# _files_differ <a> <b> — non-vacuity of a seeded fixture: a sed/awk anchor that
# stopped matching would otherwise produce a copy identical to the original and
# a rejection test that silently tests nothing.
_files_differ() { ! cmp -s "$1" "$2"; }
# Negation wrapper: `assert` runs "$@" directly, so a leading `!` cannot be
# passed as the command word.
_residual_role_undocumented() { ! _residual_role_documented "$1" "$2"; }

echo ""
echo "--- Assertion L (task 6485): every heavy-running role reaches the ceiling or is an enumerated residual ---"

_OFFLINE_WALL="$(_offline_wall_secs_for_file "$VERIFY_SH")"
_DEBUG_WALL="$(_debug_wall_secs_for_file "$VERIFY_SH")"
_CEILING="$(_max_heavy_ceiling_for_file "$NEXTEST_TOML")"

assert "L: offline release wall extracted from scripts/verify.sh (non-empty seconds, got '${_OFFLINE_WALL:-<none>}')" \
    test -n "${_OFFLINE_WALL:-}"

assert "L: debug pass wall extracted from scripts/verify.sh (non-empty seconds, got '${_DEBUG_WALL:-<none>}')" \
    test -n "${_DEBUG_WALL:-}"

assert "L: heavy per-test ceiling extracted from .config/nextest.toml (non-empty seconds, got '${_CEILING:-<none>}')" \
    test -n "${_CEILING:-}"

# The offline per-atom reachability checks, retained: the whole-role
# classification below compares against the largest heavy ceiling, so these keep
# naming the individual atom whose block drifted.
for _atom in ${HEAVY_ATOMS+"${HEAVY_ATOMS[@]}"}; do
    _lgot="$(_slow_ceiling_for_filter "$NEXTEST_TOML" "$_atom")"
    assert "L: heavy ceiling for [${_atom}] extracted from nextest.toml (non-empty seconds, got '${_lgot:-<none>}')" \
        test -n "${_lgot:-}"
    assert "L: offline release wall (${_OFFLINE_WALL:-?}s, from verify.sh) exceeds the heavy ceiling for [${_atom}] (${_lgot:-?}s, from nextest.toml) by more than the ${L_START_OFFSET_BUDGET_SECONDS}s start-offset budget, so nextest kills BY NAME before the wall fires" \
        test "$(( ${_OFFLINE_WALL:-0} - ${_lgot:-0} ))" -gt "$L_START_OFFSET_BUDGET_SECONDS"
done

_HEAVY_ROLES=()
while IFS= read -r _r; do
    [ -n "$_r" ] && _HEAVY_ROLES+=("$_r")
done < <(_heavy_running_roles_for_file "$VERIFY_SH")

echo "    (heavy-excluded roles from verify.sh: $(_heavy_excluded_roles_for_file "$VERIFY_SH" | tr '\n' ' '))"
echo "    (heavy-RUNNING roles from verify.sh: ${_HEAVY_ROLES[*]:-<none>})"
echo "    (walls from verify.sh: debug=${_DEBUG_WALL:-?}s release=$(_base_release_wall_secs_for_file "$VERIFY_SH")s; release re-scoped for: $(_release_scoped_roles_for_file "$VERIFY_SH" | tr '\n' ' ')=> ${_OFFLINE_WALL:-?}s)"
for _r in ${_HEAVY_ROLES+"${_HEAVY_ROLES[@]}"}; do
    echo "    (role '${_r}': forces profile '$(_role_profile_for_file "$VERIFY_SH" "$_r")' => binding wall $(_role_wall_secs "$VERIFY_SH" "$_r")s)"
done

# Non-vacuity floor for the role derivation itself: an edit that excluded every
# role would empty the set and make the whole classification loop below a no-op.
assert "L: at least one role still runs heavy members (role derivation is non-vacuous)" \
    test "${#_HEAVY_ROLES[@]}" -ge 1

for _role in ${_HEAVY_ROLES+"${_HEAVY_ROLES[@]}"}; do
    _rwall="$(_role_wall_secs "$VERIFY_SH" "$_role")"
    _rclass="$(_role_class "$NEXTEST_TOML" "$VERIFY_SH" "$_role" || true)"
    case "$_rclass" in
        REACHABLE)
            assert "L: role '${_role}' is REACHABLE — its binding wall (${_rwall:-?}s, from verify.sh) exceeds the heavy ceiling (${_CEILING:-?}s, from nextest.toml) by more than the ${L_START_OFFSET_BUDGET_SECONDS}s the pass takes to reach a heavy test, so a hung one is killed BY NAME" \
                test "$(( ${_rwall:-0} - ${_CEILING:-0} ))" -gt "$L_START_OFFSET_BUDGET_SECONDS"
            ;;
        RESIDUAL)
            assert "L: role '${_role}' is an ACCEPTED RESIDUAL — allowlisted in L_RESIDUAL_ROLES here AND named in .config/nextest.toml's ACCEPTED RESIDUAL paragraph, so the gap is recorded where a config reader will find it" \
                _residual_role_documented "$NEXTEST_TOML" "$_role"
            assert "L: role '${_role}' really is a residual and not a mislabel — the heavy ceiling (${_CEILING:-?}s) plus the ${L_START_OFFSET_BUDGET_SECONDS}s start-offset budget is at or above its binding wall (${_rwall:-?}s), so raising that wall past their sum must move it to REACHABLE" \
                test "$(( ${_CEILING:-0} + L_START_OFFSET_BUDGET_SECONDS ))" -ge "${_rwall:-0}"
            ;;
        *)
            assert "L: role '${_role}' runs heavy members (verify.sh accepts it and the _GATE_HEAVY_EXCLUDE guard does not cover it) but is NEITHER reachable (its binding wall '${_rwall:-<unmodelled>}' does not clear the ${_CEILING:-?}s ceiling by the ${L_START_OFFSET_BUDGET_SECONDS}s the pass takes to reach a heavy test) NOR an enumerated residual. Classify it: raise its wall past the ceiling plus that budget, lower the ceiling, add it to the exclusion guard, or add it to L_RESIDUAL_ROLES here AND to .config/nextest.toml's ACCEPTED RESIDUAL paragraph. Do not widen the guard." \
                false
            ;;
    esac
done

# Self-cleaning direction: a stale allowlist entry is as much a defect as a
# missing one, because it documents a gap that no longer exists.
for _r in ${L_RESIDUAL_ROLES+"${L_RESIDUAL_ROLES[@]}"}; do
    assert "L: residual allowlist entry '${_r}' is still a heavy-running role — once verify.sh's exclusion covers it, remove this entry and the config's residual note together" \
        _in_list "$_r" ${_HEAVY_ROLES+"${_HEAVY_ROLES[@]}"}
done

assert "L: every heavy-running role classifies, and every allowlisted residual is still heavy-running" \
    _role_classification_ok "$NEXTEST_TOML" "$VERIFY_SH"

# ---------------------------------------------------------------------------
# Assertion K/L NON-VACUITY SELF-CHECK. Both K and L are green on arrival, which
# proves nothing on its own: a checker that accepts everything would look
# identical. Each fixture below breaks exactly one thing and the corresponding
# checker must REJECT it. Each fixture is also asserted to DIFFER from its
# source, so a seed whose anchor stopped matching fails loudly instead of
# quietly testing the unmodified file.
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
#      shape: it would then run on the gate carrying the heavy tier's ceiling).
awk -v want="$_GR_FIRST" -v q="'" -v heavy="${ST_T0A:-}" '
    /^\[\[/ { hit = 0 }
    $0 ~ "^[[:space:]]*filter[[:space:]]*=" {
        hit = 0
        if (match($0, q "[^" q "]*" q)) {
            val = substr($0, RSTART + 1, RLENGTH - 2)
            if (val == want) hit = 1
        }
    }
    hit && /^[[:space:]]*slow-timeout[[:space:]]*=/ {
        sub(/terminate-after[[:space:]]*=[[:space:]]*[0-9]+/, "terminate-after = " heavy)
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

# ---------------------------------------------------------------------------
# THE SYNTHETIC-RESIDUAL SCAFFOLD (task 7552), on which fixtures (iv), (v),
# (vii) and (viii) are built.
#
# WHY IT HAD TO EXIST. Before this task `background` was a LIVE residual and the
# heavy ceiling (43200s) exceeded every wall verify.sh defines, so those four
# fixtures could produce a gap just by re-pointing a role at an existing wall.
# At the re-sized ceiling the relation inverts: every wall in verify.sh strictly
# EXCEEDS the ceiling, so no such re-pointing produces a gap and all four would
# pass BY VACUITY — silently deleting the residual machinery's negative coverage
# at the exact moment that machinery has no live user left to exercise it.
#
# WHAT IT IS. A verify.sh copy with three mutations, none of which is the thing
# any fixture is testing:
#   1. it accepts a synthetic role, which no real deployment ever sets;
#   2. it drops `background` from the heavy-running set, so the synthetic role is
#      the only role whose classification is under test (background has the
#      tightest wall, so leaving it in would make it the gap in every fixture and
#      every rejection would be attributable to the scaffold instead of the seed);
#   3. it narrows the DEBUG wall below the ceiling, which is what makes a gap
#      EXPRESSIBLE at all — the synthetic role inherits the base PROFILE, so this
#      is the wall that binds it.
# Paired with a nextest.toml copy naming the synthetic role in the ACCEPTED
# RESIDUAL paragraph, and an allowlist supplied per-call rather than globally.
#
# The scaffold ALONE is asserted to classify cleanly (the control below), so
# every rejection that follows is attributable to the ONE further mutation its
# fixture adds.
#
# THE SCAFFOLD'S OWN VACUITY RISK (task 7791). Item 3 above — narrowing the
# DEBUG wall below the ceiling — is exactly the construction task 7552 traded
# the old role-repointing shape for, and _SYNTH_DEBUG_WALL_M is a LITERAL, not
# derived from HEAVY_CEILING_SECONDS or L_START_OFFSET_BUDGET_SECONDS: either
# one re-tuning can leave the narrowed wall clearing the ceiling by MORE than
# the start-offset budget — the same shape task 7552 measured the old
# role-repointing fixtures going vacuous under (HEAD=e49c9ee218, probe log
# /tmp/t7552/probe.log). Were that to happen here, (v), (vi) — which reuses
# this literal on the real verify.sh —, (vii) and (viii) would FAIL LOUDLY,
# and (iv) would still reject through its self-cleaning direction regardless
# of the gap. ONLY the scaffold control would pass silently: through
# _role_class's REACHABLE branch instead of the RESIDUAL branch it exists to
# cover. The detector is the branch pin placed directly after that control —
# it asks _role_class itself, so the predicate is not restated here.
# ---------------------------------------------------------------------------
_SYNTH_ROLE=synthetic
_SYNTH_DEBUG_WALL_M=30

# _seed_exclude_role <src> <dst> <role> — extend verify.sh's _GATE_HEAVY_EXCLUDE
# guard to also cover <role>. Windowed to that guard's own `_GATE_HEAVY_EXCLUDE=""`
# .. `fi` block, exactly as _heavy_excluded_roles_for_file windows its parse: the
# `[ "$DF_VERIFY_ROLE" = "<role>" ]; }` shape also occurs on the PROFILE-default
# and scope-guard lines, which an unwindowed substitution would corrupt.
_seed_exclude_role() {
    sed '/^_GATE_HEAVY_EXCLUDE=""$/,/^fi$/ s/\(\[ "\$DF_VERIFY_ROLE" = "[a-z_]*" \]\); }/\1 || [ "$DF_VERIFY_ROLE" = "'"$3"'" ]; }/' \
        "$1" > "$2"
}

# _seed_accept_role <src> <dst> <role> — add <role> to the `want a|b|c` spec in
# verify.sh's unknown-role error, the one place the accepted role set is written.
_seed_accept_role() {
    sed '/unknown DF_VERIFY_ROLE/ s/(want \([a-z|]*\))/(want \1|'"$3"')/' "$1" > "$2"
}

# _seed_narrow_debug_wall <src> <dst> <minutes> — drop the DEBUG pass wall below
# the heavy ceiling. Anchored at column 0 on the unconditional assignment, the
# same anchor _debug_wall_secs_for_file reads, so the seed and the extractor
# cannot drift apart. The trailing space after the knob name keeps this off the
# `_RELEASE` knob's line.
_seed_narrow_debug_wall() {
    sed 's/^\(_VERIFY_TEST_TIMEOUT="\$(_resolve_timeout_knob REIFY_VERIFY_TEST_TIMEOUT \)[0-9]\+m/\1'"$3"'m/' \
        "$1" > "$2"
}

# _seed_heavy_ceiling <src> <dst> <grain-count> — rewrite EVERY heavy atom's
# override block to a different per-test ceiling, leaving the gate-resident
# blocks and the [profile.default] ceiling untouched. Keyed on each block's
# `filter =` value matching an entry of HEAVY_ATOMS — the same byte-identical
# correspondence Assertion J checks — so this seed cannot drift from the atom
# list the rest of the file derives. Both TOML quote characters are recognised,
# for the same reason K-neg (x) exists.
_seed_heavy_ceiling() {
    local src="$1" dst="$2" n="$3"
    printf '%s\n' ${HEAVY_ATOMS+"${HEAVY_ATOMS[@]}"} \
    | awk -v n="$n" -v q="'" '
        FNR == NR { want[$0] = 1; next }
        /^\[\[/ { hit = 0 }
        $0 ~ "^[[:space:]]*filter[[:space:]]*=" {
            hit = 0
            if (match($0, q "[^" q "]*" q) || match($0, "\"[^\"]*\"")) {
                val = substr($0, RSTART + 1, RLENGTH - 2)
                if (val in want) hit = 1
            }
        }
        hit && /^[[:space:]]*slow-timeout[[:space:]]*=/ {
            sub(/terminate-after[[:space:]]*=[[:space:]]*[0-9]+/, "terminate-after = " n)
            hit = 0
        }
        { print }
    ' - "$src" > "$dst"
}

# _seed_document_residual <src> <dst> <role> — give <role> an ENTRY LINE inside
# the config's ACCEPTED RESIDUAL paragraph, immediately after the shared anchor
# line so the insertion lands inside the contiguous comment block
# _residual_role_documented scans. Both the anchor and the separator come from
# that checker's own constants, so the seed cannot drift into a shape the checker
# no longer recognises — the failure mode that would make every fixture below
# pass vacuously rather than red.
_seed_document_residual() {
    awk -v anchor="$RESIDUAL_PAR_ANCHOR" -v sep="$RESIDUAL_ENTRY_SEP" -v role="$3" '
        { print }
        !done && $0 ~ anchor { print "#   " role " " sep " synthetic fixture role (tests/infra only)"; done = 1 }
    ' "$1" > "$2"
}

# The scaffold itself: accept + exclude-background + narrow-debug-wall, then the
# matching config copy that documents the synthetic residual.
_seed_accept_role "$VERIFY_SH" "$_KL_FIX/verify-synth-a.sh" "$_SYNTH_ROLE"
_seed_exclude_role "$_KL_FIX/verify-synth-a.sh" "$_KL_FIX/verify-synth-b.sh" background
_seed_narrow_debug_wall "$_KL_FIX/verify-synth-b.sh" "$_KL_FIX/verify-synth.sh" "$_SYNTH_DEBUG_WALL_M"
_seed_document_residual "$NEXTEST_TOML" "$_KL_FIX/synth.toml" "$_SYNTH_ROLE"

# (iv) the scaffold's verify.sh with the SYNTHETIC role ALSO excluded: it stops
#      running heavy members, so its allowlist entry is stale and must be retired
#      with the config's residual note. This is the fixture that makes the guard
#      self-cleaning rather than merely tolerant — the direction that forced THIS
#      task to retire `background`'s entry and the config's note together.
_seed_exclude_role "$_KL_FIX/verify-synth.sh" "$_KL_FIX/verify-synth-excluded.sh" "$_SYNTH_ROLE"

# (v) is the scaffold's verify.sh paired with the REAL .config/nextest.toml,
#     which does NOT name the synthetic role in its residual paragraph: the
#     allowlist here and the prose there must agree, so a residual cannot be
#     carried in the test alone. No extra seed file — the disagreement IS the
#     pairing, and $_KL_FIX/synth.toml is its non-vacuity witness.

# (vi) a verify.sh whose DEBUG wall drops below the heavy ceiling, against the
#      real config and the real (empty) allowlist: `background` runs both
#      profiles, so that wall becomes its binding one and it is then neither
#      reachable nor allowlisted — an unrecorded gap. This is the live shape
#      after task 7552: the ceiling is sized UNDER the debug wall, so the debug
#      wall is the operand a future re-tune can invalidate it with. (Before this
#      task the same fixture seeded the OFFLINE RELEASE wall down to 6h; at a
#      re-sized ceiling no whole-hour release wall is small enough to express a
#      gap, and the release walls are no longer what the ceiling is sized
#      against.) This covers the wall-BELOW-ceiling direction only; (vi-b) below
#      covers the wall-above-ceiling-but-inside-the-budget direction, and the
#      two are different shapes rather than one with a different number — the
#      superseded predicate rejected this one and accepted that one.
_seed_narrow_debug_wall "$VERIFY_SH" "$_KL_FIX/verify-narrow-debug.sh" "$_SYNTH_DEBUG_WALL_M"

# (vi-b) the shape the task 7552 amendment exists for, and the one fixture (vi)
#        cannot express: a debug wall that EXCEEDS the heavy ceiling — so the
#        superseded bare wall-over-ceiling rule called it REACHABLE and went
#        green — but not by the start-offset budget, so the pass never reaches a
#        heavy test before the wall fires and the by-name kill never happens.
#        This is the false green itself, seeded. Derived from the ceiling rather
#        than written as a literal so it stays at exactly that shape whatever
#        the ceiling is re-tuned to: one whole minute above it.
_SYNTH_TIGHT_WALL_M=$(( _CEILING / 60 + 1 ))
_seed_narrow_debug_wall "$VERIFY_SH" "$_KL_FIX/verify-tight-debug.sh" "$_SYNTH_TIGHT_WALL_M"

# (xi) THE OPPOSITE DIRECTION: a .config/nextest.toml whose heavy ceiling leaves
#      headroom comfortably clear of the start-offset budget must still classify
#      REACHABLE. Every other L fixture asserts a REJECTION, so without this one
#      a budget that failed to parse — an empty string, a botched substitution —
#      would make the corrected predicate reject every role and look exactly
#      like rigour. Sized to leave twice the budget of headroom, derived from
#      the same two numbers the predicate itself compares.
_ROOMY_CEILING_N=$(( (_DEBUG_WALL - 2 * L_START_OFFSET_BUDGET_SECONDS) / 120 ))
_seed_heavy_ceiling "$NEXTEST_TOML" "$_KL_FIX/roomy.toml" "$_ROOMY_CEILING_N"

# (vii) the scaffold plus a further `experimental` role with no exclusion, no
#       allowlist entry and no residual note: the newcomer shape, which must not
#       be absorbed silently.
_seed_accept_role "$_KL_FIX/verify-synth.sh" "$_KL_FIX/verify-synth-extra-role.sh" experimental

# (viii) the scaffold plus an OFFLINE branch of the role-based PROFILE default
#        that forces `both` instead of `release` — one `||` away from the shape
#        merge and background already have. Offline then also runs a DEBUG pass,
#        so the scaffold's narrowed debug wall becomes its binding one and the
#        ceiling is out of reach, with the 13h release wall left intact to look
#        reassuring. This is the fixture that makes _role_wall_secs' profile
#        DERIVATION load-bearing: against the literal role=>wall map it replaced,
#        this file classified offline REACHABLE and nothing went red.
sed 's/^\([[:space:]]*\)PROFILE="release"$/\1PROFILE="both"/' \
    "$_KL_FIX/verify-synth.sh" > "$_KL_FIX/verify-synth-offline-both.sh"

# (ix) an unclassified override authored slow-timeout BEFORE filter. TOML imposes
#      no key order, so this is a legal way to write the (i) fixture — and the
#      line-ordered parse this file used to carry could not see it at all,
#      absorbing the newcomer silently (K failing OPEN, the one direction a
#      total-classification guard must never fail).
cp "$NEXTEST_TOML" "$_KL_FIX/reordered.toml"
cat >> "$_KL_FIX/reordered.toml" <<'REORDERED'

[[profile.default.overrides]]
slow-timeout = { period = "120s", terminate-after = 99 }
filter = 'package(reify-eval) & binary(reordered_newcomer)'
REORDERED

# (x) an unclassified override whose filter is DOUBLE-quoted. Equally legal TOML,
#     equally invisible to a parse that keys on the single-quote character.
cp "$NEXTEST_TOML" "$_KL_FIX/dquoted.toml"
cat >> "$_KL_FIX/dquoted.toml" <<'DQUOTED'

[[profile.default.overrides]]
filter = "package(reify-eval) & binary(dquoted_newcomer)"
slow-timeout = { period = "120s", terminate-after = 99 }
DQUOTED

assert "K-neg (i): classifier REJECTS an EXTRA override belonging to neither class (a new override must be classified, not absorbed)" \
    _classify_overrides_reject "$_KL_FIX/extra.toml"

assert "K-neg (ii): classifier REJECTS a gate-resident block silently promoted to the ${HEAVY_CEILING_SECONDS}s heavy ceiling" \
    _classify_overrides_reject "$_KL_FIX/promoted.toml"

assert "K-neg (iii): classifier REJECTS a nextest.toml with an allowlisted gate-resident block deleted" \
    _classify_overrides_reject "$_KL_FIX/gr-deleted.toml"

assert "L-neg scaffold is non-vacuous — the synthetic-role seeds really changed scripts/verify.sh" \
    _files_differ "$VERIFY_SH" "$_KL_FIX/verify-synth.sh"

assert "L-neg scaffold is non-vacuous — the residual-note seed really changed .config/nextest.toml" \
    _files_differ "$NEXTEST_TOML" "$_KL_FIX/synth.toml"

# The positive control for the whole scaffold. Without it every rejection below
# could be an artifact of the three scaffold mutations rather than of the one
# mutation its fixture adds. It is also the only assertion that exercises
# _role_class's RESIDUAL branch, which has no live user since the allowlist
# emptied — the branch pin directly below is what proves that branch is the
# one actually taken, not just that the boolean checker returned true.
assert "L-neg scaffold control: the scaffold ALONE classifies — the synthetic role is allowlisted AND documented AND genuinely below its wall (the RESIDUAL branch), and every other heavy-running role reaches the ceiling" \
    _role_classification_ok_with_residuals "$_SYNTH_ROLE" "$_KL_FIX/synth.toml" "$_KL_FIX/verify-synth.sh"

# The control above only asserts _role_classification_ok, which is TRUE
# whichever branch of _role_class the synthetic role takes. If the gap ever
# closes, the control stays green through REACHABLE instead of RESIDUAL. This
# pin asks _role_class directly, where the branch predicate actually lives.
_SYNTH_CLASS="$(_with_residuals "$_SYNTH_ROLE" _role_class "$_KL_FIX/synth.toml" "$_KL_FIX/verify-synth.sh" "$_SYNTH_ROLE" || true)"
assert "L-neg scaffold control takes the RESIDUAL branch (task 7791): _role_class classifies the synthetic role RESIDUAL, not REACHABLE (got '${_SYNTH_CLASS:-<unclassified>}') — its ${_SYNTH_DEBUG_WALL_M}m seeded debug wall does not clear the ${_CEILING:-?}s heavy ceiling by the ${L_START_OFFSET_BUDGET_SECONDS}s start-offset budget. On FAIL the control above is passing through REACHABLE, _role_class's RESIDUAL branch is untested, and (v)/(vi)/(vii)/(viii) fail for the same root cause: re-seed _SYNTH_DEBUG_WALL_M lower" \
    test "${_SYNTH_CLASS:-}" = RESIDUAL

assert "L-neg (iv) fixture is non-vacuous — the synthetic-role exclusion seed really changed the scaffold's verify.sh" \
    _files_differ "$_KL_FIX/verify-synth.sh" "$_KL_FIX/verify-synth-excluded.sh"

assert "L-neg (iv): classifier REJECTS a verify.sh whose exclusion guard covers the allowlisted residual role while the allowlist still claims it runs heavy members (the self-cleaning direction: allowlist entry and config note must retire together)" \
    _role_classification_reject_with_residuals "$_SYNTH_ROLE" "$_KL_FIX/synth.toml" "$_KL_FIX/verify-synth-excluded.sh"

assert "L-neg (v): classifier REJECTS the real .config/nextest.toml — which does NOT name the synthetic role in its ACCEPTED RESIDUAL paragraph — against an allowlist that does (allowlist and prose must agree; a residual cannot be carried in the test alone)" \
    _role_classification_reject_with_residuals "$_SYNTH_ROLE" "$NEXTEST_TOML" "$_KL_FIX/verify-synth.sh"

# (v-b) the amendment to (v), and the one shape (v) cannot express because it
#       uses a role the config has never heard of. A RETIRED path is still
#       DISCUSSED in that paragraph by name: the live config says "ACCEPTED
#       RESIDUAL — NONE" and then names `background` while explaining that its
#       gap CLOSED. Under the superseded word match that sentence COUNTED, so
#       half of the two-sided contract was already satisfied for `background` by
#       prose asserting the opposite, and re-adding it to the allowlist alone
#       would have gone green with the config untouched. The pair below is the
#       discriminator: the same checker, the same paragraph, one file with an
#       entry line and one with only prose.
assert "L-neg (v-b): PROSE naming a role inside the ACCEPTED RESIDUAL paragraph does NOT document it — the real .config/nextest.toml names 'background' there while recording that its gap closed, and only a '#   <role> — <reason>' ENTRY LINE may stand in for a live residual" \
    _residual_role_undocumented "$NEXTEST_TOML" background

assert "L-neg (v-b) positive control: the SAME checker DOES accept an entry line — the seeded synthetic residual in the fixture config (so the rejection above is about the shape of the mention, not a checker that rejects everything)" \
    _residual_role_documented "$_KL_FIX/synth.toml" "$_SYNTH_ROLE"

assert "L-neg (vi) fixture is non-vacuous — the narrowed-debug-wall seed really changed scripts/verify.sh" \
    _files_differ "$VERIFY_SH" "$_KL_FIX/verify-narrow-debug.sh"

assert "L-neg (vi): classifier REJECTS a verify.sh whose DEBUG wall (${_SYNTH_DEBUG_WALL_M}m) no longer exceeds the ${HEAVY_CEILING_SECONDS}s heavy ceiling — the wall the ceiling is SIZED under, leaving the both-profile roles an unrecorded gap" \
    _role_classification_reject "$NEXTEST_TOML" "$_KL_FIX/verify-narrow-debug.sh"

assert "L-neg (vi-b) fixture is non-vacuous — the tight-debug-wall seed really changed scripts/verify.sh" \
    _files_differ "$VERIFY_SH" "$_KL_FIX/verify-tight-debug.sh"

assert "L-neg (vi-b): classifier REJECTS a verify.sh whose DEBUG wall (${_SYNTH_TIGHT_WALL_M}m) EXCEEDS the ${_CEILING:-?}s heavy ceiling but clears it by less than the ${L_START_OFFSET_BUDGET_SECONDS}s start-offset budget — the false green the superseded bare wall-over-ceiling rule produced, which is the whole reason that rule was replaced" \
    _role_classification_reject "$NEXTEST_TOML" "$_KL_FIX/verify-tight-debug.sh"

assert "L-neg (xi): the roomy-ceiling fixture is expressible at all — leaving twice the ${L_START_OFFSET_BUDGET_SECONDS}s budget of headroom under the ${_DEBUG_WALL:-?}s debug wall needs a positive 120s grain count (got ${_ROOMY_CEILING_N:-<none>})" \
    test "${_ROOMY_CEILING_N:-0}" -ge 1

assert "L-neg (xi) fixture is non-vacuous — the roomy-ceiling seed really changed .config/nextest.toml" \
    _files_differ "$NEXTEST_TOML" "$_KL_FIX/roomy.toml"

assert "L-neg (xi): the SAME role classifier ACCEPTS a nextest.toml whose heavy ceiling ($(( _ROOMY_CEILING_N * 120 ))s) leaves every heavy-running role headroom well clear of the budget — the corrected predicate DISCRIMINATES, it does not simply reject" \
    _role_classification_ok "$_KL_FIX/roomy.toml" "$VERIFY_SH"

assert "L-neg (vii) fixture is non-vacuous — the extra-role seed really changed the scaffold's verify.sh" \
    _files_differ "$_KL_FIX/verify-synth.sh" "$_KL_FIX/verify-synth-extra-role.sh"

assert "L-neg (vii): classifier REJECTS a verify.sh that accepts a new unexcluded role, so a newcomer cannot inherit another role's residual by silence" \
    _role_classification_reject_with_residuals "$_SYNTH_ROLE" "$_KL_FIX/synth.toml" "$_KL_FIX/verify-synth-extra-role.sh"

assert "L-neg (viii) fixture is non-vacuous — the offline-forces-both seed really changed the scaffold's verify.sh" \
    _files_differ "$_KL_FIX/verify-synth.sh" "$_KL_FIX/verify-synth-offline-both.sh"

assert "L-neg (viii): classifier REJECTS a verify.sh whose offline branch forces PROFILE=both — offline then runs a debug pass under the tighter wall, so its binding wall is DERIVED as that wall and the ceiling is unreachable, however long the release wall stays" \
    _role_classification_reject_with_residuals "$_SYNTH_ROLE" "$_KL_FIX/synth.toml" "$_KL_FIX/verify-synth-offline-both.sh"

assert "K-neg (ix) fixture is non-vacuous — the reordered-keys seed really changed .config/nextest.toml" \
    _files_differ "$NEXTEST_TOML" "$_KL_FIX/reordered.toml"

assert "K-neg (ix): classifier REJECTS an unclassified override authored slow-timeout BEFORE filter (TOML fixes no key order, so the parse must not either)" \
    _classify_overrides_reject "$_KL_FIX/reordered.toml"

assert "K-neg (x) fixture is non-vacuous — the double-quoted-filter seed really changed .config/nextest.toml" \
    _files_differ "$NEXTEST_TOML" "$_KL_FIX/dquoted.toml"

assert "K-neg (x): classifier REJECTS an unclassified override whose filter is double-quoted (both TOML quote characters must be seen)" \
    _classify_overrides_reject "$_KL_FIX/dquoted.toml"

# Positive controls: the SAME checkers accept the real files, so the rejections
# above are attributable to the seeded drift and not to checkers that reject
# everything.
assert "K-neg control: the same classifier ACCEPTS the real .config/nextest.toml" \
    _classify_overrides_ok "$NEXTEST_TOML"

assert "L-neg control: the same role classifier ACCEPTS the real nextest.toml + verify.sh pair" \
    _role_classification_ok "$NEXTEST_TOML" "$VERIFY_SH"

rm -rf "$_KL_FIX"

test_summary
