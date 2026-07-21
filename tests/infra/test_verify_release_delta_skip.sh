#!/usr/bin/env bash
# tests/infra/test_verify_release_delta_skip.sh
# Infrastructure test for task 5279
# (PRD docs/prds/merge-gate-riders.md task ε, rider 3).
#
# Validates the delta-conditional release-pass skip: a SWEEP-GATED, default-OFF
# skip of the merge-gate release nextest pass when the merge delta touches no
# release-sensitive crate.
#
# UNIT block (step-1/step-2): drives the release_delta_requires_pass predicate
# (scripts/release-scope-lib.sh) directly, injecting the affected crate set via
# REIFY_AFFECTED_CRATES_OVERRIDE so the exercise is fully hermetic (no git, no
# cargo — the fixture-less workspace path is never reached).
#   Contract: rc0 = release pass REQUIRED, rc1 = SKIPPABLE (delta-clean).
#     - non-release-sensitive crate               => rc1  (delta-clean, skip)
#     - a release_declared_set member              => rc0  (required)
#     - ALL sentinel (C4 global / C5 unmappable)   => rc0  (fail-wide)
#     - empty / whitespace-only override           => rc1  (empty set, skip)
#     - mixed set containing one sensitive crate   => rc0  (required)
#   The sensitive crate name is read from release_declared_set (the single source
#   of truth), never hard-coded, so this test cannot drift from
#   scripts/release-sensitive-crates.txt.
#
# SCENARIO block (step-3): five hermetic `verify.sh --print-plan` scenarios that
# assert the plan marker `RELEASE-PASS: skipped (delta-clean)` and the presence /
# absence of the release nextest pass under the knob/role/override matrix. Added
# in step-3 once verify.sh is wired (step-4).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

# Source the shared release-scope library. After step-2 this provides the
# release_delta_requires_pass predicate alongside release_declared_set (both are
# single-definition, shared with scripts/verify.sh so the entrypoint and this
# drift test cannot diverge).
[ -f "$REPO_ROOT/scripts/release-scope-lib.sh" ] || { echo "ERROR: release-scope-lib.sh not found at $REPO_ROOT/scripts/release-scope-lib.sh"; exit 1; }
# shellcheck source=scripts/release-scope-lib.sh
source "$REPO_ROOT/scripts/release-scope-lib.sh"

# plan_capture_lib.sh (capture_print_plan + plan_match/plan_capture_complete) and
# copy_list_preflight_lib.sh (assert_source_closure_copied) power the hermetic
# --print-plan SCENARIO block below.
[ -f "$SCRIPT_DIR/plan_capture_lib.sh" ] || { echo "ERROR: plan_capture_lib.sh not found at $SCRIPT_DIR/plan_capture_lib.sh"; exit 1; }
# shellcheck source=tests/infra/plan_capture_lib.sh
source "$SCRIPT_DIR/plan_capture_lib.sh"

[ -f "$SCRIPT_DIR/copy_list_preflight_lib.sh" ] || { echo "ERROR: copy_list_preflight_lib.sh not found at $SCRIPT_DIR/copy_list_preflight_lib.sh"; exit 1; }
# shellcheck source=tests/infra/copy_list_preflight_lib.sh
source "$SCRIPT_DIR/copy_list_preflight_lib.sh"

echo "=== Delta-conditional release-pass skip tests (task 5279) ==="

# ---------------------------------------------------------------------------
# UNIT block: release_delta_requires_pass exit-code contract
# ---------------------------------------------------------------------------
echo ""
echo "--- UNIT: release_delta_requires_pass exit-code contract ---"

# Single source of truth for the crate names below: read the declared
# release-sensitive set from the shared lib and take its first entry as the
# "sensitive" fixture. Hard-coding would let this test drift from
# scripts/release-sensitive-crates.txt.
_DECLARED_SET="$(release_declared_set)"
[ -n "$_DECLARED_SET" ] || { echo "ERROR: release_declared_set is empty — cannot pick a sensitive crate"; exit 1; }
_SENSITIVE_CRATE="${_DECLARED_SET%%$'\n'*}"

# A crate guaranteed NOT release-sensitive. reify-cli is a real workspace member
# with zero release-sensitive tests. Guard against future drift: if reify-cli is
# ever added to release-sensitive-crates.txt this fails loudly rather than
# silently inverting the delta-clean case.
_NONSENSITIVE_CRATE="reify-cli"
if printf '%s\n' "$_DECLARED_SET" | grep -qxF "$_NONSENSITIVE_CRATE"; then
    echo "ERROR: chosen non-sensitive crate '$_NONSENSITIVE_CRATE' is in release_declared_set — pick another"
    exit 1
fi

# Helpers: run the predicate with the affected crate set injected verbatim via
# REIFY_AFFECTED_CRATES_OVERRIDE. The subshell scopes the env assignment so it
# never leaks between asserts; no git / no cargo is reached.
_predicate_requires() {  # asserts rc0 — release pass REQUIRED
    ( REIFY_AFFECTED_CRATES_OVERRIDE="$1" release_delta_requires_pass )
}
_predicate_skips() {     # asserts rc1 — SKIPPABLE (negated for assert)
    ! ( REIFY_AFFECTED_CRATES_OVERRIDE="$1" release_delta_requires_pass )
}

# Clean RED signal in step-1: the predicate does not exist until step-2.
assert "release_delta_requires_pass is defined" \
    declare -F release_delta_requires_pass

assert "non-release-sensitive crate ($_NONSENSITIVE_CRATE) => SKIPPABLE (rc1, delta-clean)" \
    _predicate_skips "$_NONSENSITIVE_CRATE"

assert "release-sensitive crate ($_SENSITIVE_CRATE) => REQUIRED (rc0)" \
    _predicate_requires "$_SENSITIVE_CRATE"

assert "ALL sentinel => REQUIRED (rc0, fail-wide)" \
    _predicate_requires "ALL"

assert "empty override => SKIPPABLE (rc1, empty affected set)" \
    _predicate_skips ""

assert "whitespace-only override => SKIPPABLE (rc1, word-splits to nothing)" \
    _predicate_skips "   "

assert "mixed set with one sensitive crate ($_NONSENSITIVE_CRATE $_SENSITIVE_CRATE) => REQUIRED (rc0)" \
    _predicate_requires "$_NONSENSITIVE_CRATE $_SENSITIVE_CRATE"

# ===========================================================================
# SCENARIO block: five hermetic verify.sh --print-plan scenarios (§5.2).
#
# Fixture: a workspace-less throwaway git repo carrying verify.sh + its
# transitive source closure and NO commits (unborn HEAD). `cargo metadata`
# fails there, so every non-ALL affected set is injected via
# REIFY_AFFECTED_CRATES_OVERRIDE (hermetic — no cargo), and the underivable
# scenario (5) simply omits the override: the unborn HEAD makes verify.sh's
# _derive_merge_delta fail => the predicate is never consulted => fail-open RUN.
#
# Env is managed EXPLICITLY per scenario (never inherits ambient): each run
# clears REIFY_INFRA_SUITE_ACTIVE (so the role-gated reify-cli release prebuild
# at verify.sh:1542 is present for scenario 2's assertion) and sets exactly the
# DF_VERIFY_ROLE / REIFY_RELEASE_DELTA_SKIP / REIFY_AFFECTED_CRATES_OVERRIDE the
# scenario needs — an ambient knob must never leak in.
#
# Release-pass detection: a `cargo nextest run ... --release` plan line (the
# `cargo test ... --release` fallback for hosts without nextest is matched too).
# The `cargo build --release -p reify-cli` prebuild is neither a `cargo nextest
# run` nor a `cargo test` line, so it never trips the release-pass detector —
# that is exactly what lets scenario 2 assert "release pass absent, reify-cli
# release prebuild present" at once.
# ===========================================================================
echo ""
echo "--- SCENARIO: hermetic verify.sh --print-plan (knob/role/override matrix) ---"

_TMPDIRS=()
_cleanup_fixtures() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap _cleanup_fixtures EXIT

# make_fixture VARNAME — isolated git repo with verify.sh + its transitive source
# closure and NO commits (unborn HEAD). Models test_verify_scope.sh:make_fixture;
# assert_source_closure_copied fails loudly if a new source line is left un-copied
# (e.g. the step-2 release-scope-lib.sh -> affected-crates-lib.sh edge).
make_fixture() {
    local _var="$1" dir
    dir="$(mktemp -d)"
    _TMPDIRS+=("$dir")
    mkdir -p "$dir/scripts"
    cp "$REPO_ROOT/scripts/verify.sh" "$dir/scripts/verify.sh"
    cp "$REPO_ROOT/scripts/occt-scope-lib.sh" "$dir/scripts/occt-scope-lib.sh"
    cp "$REPO_ROOT/scripts/occt-touching-crates.txt" "$dir/scripts/occt-touching-crates.txt"
    cp "$REPO_ROOT/scripts/release-scope-lib.sh" "$dir/scripts/release-scope-lib.sh"
    cp "$REPO_ROOT/scripts/release-sensitive-crates.txt" "$dir/scripts/release-sensitive-crates.txt"
    cp "$REPO_ROOT/scripts/affected-crates-lib.sh" "$dir/scripts/affected-crates-lib.sh"
    cp "$REPO_ROOT/scripts/lib_test_semaphore.sh" "$dir/scripts/lib_test_semaphore.sh"
    cp "$REPO_ROOT/scripts/lib_slot_acquire.sh" "$dir/scripts/lib_slot_acquire.sh"
    cp "$REPO_ROOT/scripts/lib_clock_stop.sh"   "$dir/scripts/lib_clock_stop.sh"
    cp "$REPO_ROOT/scripts/cpu-admit.sh" "$dir/scripts/cpu-admit.sh"
    cp "$REPO_ROOT/scripts/lib_proc_reaper.sh" "$dir/scripts/lib_proc_reaper.sh"
    cp "$REPO_ROOT/scripts/gen-nextest-config.sh" "$dir/scripts/gen-nextest-config.sh"
    cp "$REPO_ROOT/scripts/heavy-test-filter-lib.sh" "$dir/scripts/heavy-test-filter-lib.sh"
    cp "$REPO_ROOT/scripts/verify-pipeline-infra-tests.txt" "$dir/scripts/verify-pipeline-infra-tests.txt"
    mkdir -p "$dir/.config"
    cp "$REPO_ROOT/.config/nextest.toml" "$dir/.config/nextest.toml"
    chmod +x "$dir/scripts/verify.sh"
    assert_source_closure_copied "$REPO_ROOT/scripts" "$dir/scripts" verify.sh || exit 1
    git -C "$dir" init -q
    git -C "$dir" config user.email "test@test.com"
    git -C "$dir" config user.name "Test"
    printf -v "$_var" '%s' "$dir"
}

FIX=""
make_fixture FIX

# _scenario_plan <out_var> <env-and-flags...> — capture the standard release-gate
# plan in the fixture under a fully-explicit environment. The trailing
# `bash -c ... _ "$FIX"` cd's into the fixture and runs verify.sh --print-plan;
# capture_print_plan retries on a truncated capture. `|| true` so exhaustion
# surfaces as a failed assertion, not a set -e abort.
_scenario_plan() {
    local _outvar="$1"; shift
    capture_print_plan "$_outvar" "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
        "$@" bash -c 'cd "$1" && exec bash scripts/verify.sh test --profile both --scope all --print-plan 2>/dev/null' _ "$FIX" \
        || true
}

# Predicates over a captured plan string (run in THIS shell — never via `bash -c`,
# which would not see these functions or plan_match).
_nonempty()           { [ -n "$1" ]; }
_has_release_pass()   { plan_match "$1" 'cargo (nextest run|test) .*--release'; }
_lacks_release_pass() { ! plan_match "$1" 'cargo (nextest run|test) .*--release'; }
_has_skip_marker()    { plan_match "$1" 'RELEASE-PASS: skipped \(delta-clean\)'; }
_lacks_skip_marker()  { ! plan_match "$1" 'RELEASE-PASS: skipped \(delta-clean\)'; }
_has_cli_prebuild()   { plan_match "$1" 'cargo build --release -p reify-cli'; }

# ---------------------------------------------------------------------------
# Scenario 1: knob OFF + role=merge => release pass RUN, no marker (INERT).
# Proves the Tests 17/17b invariant: knob-off leaves the release pass emitted.
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario 1: knob unset + role=merge => release pass present, no marker ---"
PLAN_S1=""
_scenario_plan PLAN_S1 env -u REIFY_INFRA_SUITE_ACTIVE -u REIFY_RELEASE_DELTA_SKIP -u REIFY_AFFECTED_CRATES_OVERRIDE DF_VERIFY_ROLE=merge
assert "S1 knob-off/merge: PLAN_S1 non-empty (verify.sh --print-plan OK)" _nonempty "$PLAN_S1"
assert "S1 knob-off/merge: release nextest pass PRESENT (knob default off => plan byte-identical)" _has_release_pass "$PLAN_S1"
assert "S1 knob-off/merge: delta-clean marker ABSENT" _lacks_skip_marker "$PLAN_S1"

# ---------------------------------------------------------------------------
# Scenario 2: knob ON + role=merge + non-sensitive override (delta-clean)
# => marker PRESENT, release nextest ABSENT, reify-cli release prebuild PRESENT.
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario 2: knob on + role=merge + non-sensitive override => marker, no release pass, prebuild kept ---"
PLAN_S2=""
_scenario_plan PLAN_S2 env -u REIFY_INFRA_SUITE_ACTIVE REIFY_RELEASE_DELTA_SKIP=1 DF_VERIFY_ROLE=merge REIFY_AFFECTED_CRATES_OVERRIDE="$_NONSENSITIVE_CRATE"
assert "S2 knob-on/merge/clean: PLAN_S2 non-empty (verify.sh --print-plan OK)" _nonempty "$PLAN_S2"
assert "S2 knob-on/merge/clean: delta-clean marker PRESENT" _has_skip_marker "$PLAN_S2"
assert "S2 knob-on/merge/clean: release nextest pass ABSENT (skipped)" _lacks_release_pass "$PLAN_S2"
assert "S2 knob-on/merge/clean: reify-cli release prebuild PRESENT (unconditional; run_all consumes target/release/reify)" _has_cli_prebuild "$PLAN_S2"

# ---------------------------------------------------------------------------
# Scenario 3: knob ON + role=merge + release-sensitive override
# => release nextest PRESENT, marker ABSENT (fail-wide: sensitive crate affected).
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario 3: knob on + role=merge + release-sensitive override => release pass present, no marker ---"
PLAN_S3=""
_scenario_plan PLAN_S3 env -u REIFY_INFRA_SUITE_ACTIVE REIFY_RELEASE_DELTA_SKIP=1 DF_VERIFY_ROLE=merge REIFY_AFFECTED_CRATES_OVERRIDE="$_SENSITIVE_CRATE"
assert "S3 knob-on/merge/sensitive: PLAN_S3 non-empty (verify.sh --print-plan OK)" _nonempty "$PLAN_S3"
assert "S3 knob-on/merge/sensitive: release nextest pass PRESENT ($_SENSITIVE_CRATE is release-sensitive)" _has_release_pass "$PLAN_S3"
assert "S3 knob-on/merge/sensitive: delta-clean marker ABSENT" _lacks_skip_marker "$PLAN_S3"

# ---------------------------------------------------------------------------
# Scenario 4: knob ON + role=background + clean override
# => release nextest PRESENT (background NEVER skips — the sweep IS the backstop).
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario 4: knob on + role=background + clean override => release pass present (background never skips) ---"
PLAN_S4=""
_scenario_plan PLAN_S4 env -u REIFY_INFRA_SUITE_ACTIVE REIFY_RELEASE_DELTA_SKIP=1 DF_VERIFY_ROLE=background REIFY_AFFECTED_CRATES_OVERRIDE="$_NONSENSITIVE_CRATE"
assert "S4 knob-on/background/clean: PLAN_S4 non-empty (verify.sh --print-plan OK)" _nonempty "$PLAN_S4"
assert "S4 knob-on/background/clean: release nextest pass PRESENT (role=background never skips)" _has_release_pass "$PLAN_S4"
assert "S4 knob-on/background/clean: delta-clean marker ABSENT" _lacks_skip_marker "$PLAN_S4"

# ---------------------------------------------------------------------------
# Scenario 5: knob ON + role=merge + NO override (underivable delta)
# => release nextest PRESENT (unborn-HEAD fixture => _derive_merge_delta fails
#    => predicate never consulted => fail-open RUN).
# ---------------------------------------------------------------------------
echo ""
echo "--- Scenario 5: knob on + role=merge + no override (underivable) => release pass present (fail-open) ---"
PLAN_S5=""
_scenario_plan PLAN_S5 env -u REIFY_INFRA_SUITE_ACTIVE -u REIFY_AFFECTED_CRATES_OVERRIDE REIFY_RELEASE_DELTA_SKIP=1 DF_VERIFY_ROLE=merge
assert "S5 knob-on/merge/underivable: PLAN_S5 non-empty (verify.sh --print-plan OK)" _nonempty "$PLAN_S5"
assert "S5 knob-on/merge/underivable: release nextest pass PRESENT (fail-open on underivable delta)" _has_release_pass "$PLAN_S5"
assert "S5 knob-on/merge/underivable: delta-clean marker ABSENT" _lacks_skip_marker "$PLAN_S5"

test_summary
