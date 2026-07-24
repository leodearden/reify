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

# ===========================================================================
# DERIVE block: _derive_merge_delta git-derivation coverage (both branches).
#
# The SCENARIO block above drives the skip DECISION only through the hermetic
# REIFY_AFFECTED_CRATES_OVERRIDE path (no git) and the underivable unborn-HEAD
# fail-open path (S5). The actual git delta-derivation that feeds the decision
# on a REAL merge — verify.sh's _derive_merge_delta — was untested, yet
# UNDER-approximating that delta is the one dangerous failure mode (a
# release-sensitive change silently skipping the gate). Both derivable branches
# are pinned here directly:
#   - merge commit (DF lane): first-parent diff `HEAD^1 HEAD` => the topic-
#     introduced files ONLY (NOT files that merely advanced on main) — proves
#     first-parent semantics, not a merge-base or second-parent diff.
#   - MERGE_HEAD present (hook path): two-dot `HEAD MERGE_HEAD` => a fail-WIDE
#     over-approximation (topic files PLUS files main changed). Over-approx is
#     the SAFE direction; that safety property is asserted explicitly here.
#
# _derive_merge_delta lives inside scripts/verify.sh (which runs its pipeline at
# top level and so cannot be sourced for one function), so its REAL definition
# is extracted verbatim from verify.sh and eval'd. It depends only on git and
# $REPO_ROOT — no other verify.sh helper — so extraction-in-isolation is
# faithful; a rename/move or a new intra-verify.sh dependency fails this block
# loudly rather than silently dropping coverage.
# ===========================================================================
echo ""
echo "--- DERIVE: _derive_merge_delta first-parent & MERGE_HEAD branches ---"

# Extract the real _derive_merge_delta from verify.sh (its header line through
# the column-0 closing brace) and define it in this shell.
_DERIVE_FN_SRC="$(awk '/^_derive_merge_delta\(\) \{/{f=1} f{print} f&&/^\}/{exit}' "$REPO_ROOT/scripts/verify.sh")"
[ -n "$_DERIVE_FN_SRC" ] || { echo "ERROR: could not extract _derive_merge_delta from scripts/verify.sh"; exit 1; }
eval "$_DERIVE_FN_SRC"

assert "_derive_merge_delta extracted from verify.sh and defined" \
    declare -F _derive_merge_delta

# Paths introduced by the topic vs. by main, shared across the merge fixtures.
_TOPIC_FILE="crates/reify-expr/src/rider3_probe.rs"   # a real release-sensitive crate path
_BASE_FILE="rider3_main_only.txt"                      # only main advances this

# make_merge_fixture VARNAME — a workspace-less git repo with: a base commit, a
# `topic` branch that adds $_TOPIC_FILE, and an advanced base (adds $_BASE_FILE)
# so a merge is a true non-fast-forward 2-parent commit. The caller completes or
# stages the merge. No verify.sh source closure needed — _derive_merge_delta
# only needs git + a REPO_ROOT override.
make_merge_fixture() {
    local _var="$1" dir base
    dir="$(mktemp -d)"
    _TMPDIRS+=("$dir")
    git -C "$dir" init -q
    git -C "$dir" config user.email "test@test.com"
    git -C "$dir" config user.name "Test"
    base="$(git -C "$dir" symbolic-ref --short HEAD)"
    echo base > "$dir/base.txt"
    git -C "$dir" add -A
    git -C "$dir" commit -qm "A: base"
    git -C "$dir" checkout -q -b topic
    mkdir -p "$dir/$(dirname "$_TOPIC_FILE")"
    echo 'pub fn probe() {}' > "$dir/$_TOPIC_FILE"
    git -C "$dir" add -A
    git -C "$dir" commit -qm "T: topic adds a reify-expr file"
    git -C "$dir" checkout -q "$base"
    echo main-only > "$dir/$_BASE_FILE"
    git -C "$dir" add -A
    git -C "$dir" commit -qm "A2: base advances (diverge)"
    printf -v "$_var" '%s' "$dir"
}

# _derive_contains <fixture> <needle> — run the extracted _derive_merge_delta with
# REPO_ROOT pointed at <fixture>, in a subshell that FIRST cd's into the fixture
# (so the global REPO_ROOT/CWD are untouched). The cd mirrors verify.sh's real
# invocation context — the decision block runs AFTER `cd "$REPO_ROOT"`
# (verify.sh:650) — which is exactly what makes the hook-path MERGE_HEAD check
# `[ -f "$_mh" ]` (a repo-root-relative `.git/MERGE_HEAD`) resolve. rc0 iff
# derivation succeeds AND its output contains <needle> (exact substring,
# fork-free — no pipe/grep EINTR surface).
_derive_contains() {
    local _fix="$1" _needle="$2" _out
    _out="$( cd "$_fix" && REPO_ROOT="$_fix" _derive_merge_delta )" || return 1
    [[ "$_out" == *"$_needle"* ]]
}
_derive_lacks() { ! _derive_contains "$@"; }

# --- first-parent branch: a completed 2-parent merge commit ---
DERIVE_FP=""
make_merge_fixture DERIVE_FP
git -C "$DERIVE_FP" merge -q --no-ff --no-edit topic     # HEAD gains a 2nd parent
assert "DERIVE first-parent: yields the topic-introduced file ($_TOPIC_FILE)" \
    _derive_contains "$DERIVE_FP" "$_TOPIC_FILE"
assert "DERIVE first-parent: EXCLUDES files only main advanced ($_BASE_FILE) — proves HEAD^1 HEAD, not merge-base/2nd-parent" \
    _derive_lacks "$DERIVE_FP" "$_BASE_FILE"

# --- MERGE_HEAD branch: an in-progress (staged, uncommitted) merge ---
DERIVE_MH=""
make_merge_fixture DERIVE_MH
git -C "$DERIVE_MH" merge -q --no-commit --no-ff topic >/dev/null 2>&1   # leaves MERGE_HEAD (disjoint files: no conflict)
assert "DERIVE MERGE_HEAD: yields the topic-introduced file ($_TOPIC_FILE)" \
    _derive_contains "$DERIVE_MH" "$_TOPIC_FILE"
assert "DERIVE MERGE_HEAD: ALSO includes files main changed ($_BASE_FILE) — two-dot HEAD MERGE_HEAD is fail-WIDE (over-approx, safe)" \
    _derive_contains "$DERIVE_MH" "$_BASE_FILE"

# ===========================================================================
# Scenario 6: knob ON + role=merge + NO override + a REAL 2-parent merge commit
# => release nextest pass PRESENT, marker ABSENT. Exercises verify.sh's OWN
# _derive_merge_delta -> release_delta_requires_pass wiring end-to-end with no
# override (S1–S5 all short-circuit derivation via the override or an unborn
# HEAD). The topic introduces both a release-sensitive crate file AND a C4
# workspace-global file (Cargo.lock); affected_crates fails WIDE to ALL on the
# C4 global BEFORE consulting cargo, so the outcome is deterministic and fully
# hermetic (no cargo in the fixture) — REQUIRED, pass kept. The precise
# file->crate->sensitive resolution is covered by the override-driven UNIT block
# and by test_affected_crates_lib.sh (K8); the exact derived file set by the
# DERIVE block above. Here the point is that a real, non-empty git-derived delta
# reaches the predicate and keeps the pass with no override in play.
# ===========================================================================
echo ""
echo "--- Scenario 6: knob on + role=merge + no override + real merge commit => release pass present (derivation->predicate) ---"

# _scenario_plan_in <fixdir> <outvar> <env-and-flags...> — as _scenario_plan but
# for an arbitrary fixture dir (the S1–S5 helper hardcodes the unborn-HEAD FIX;
# Scenario 6 needs a fixture whose HEAD is a real merge commit).
_scenario_plan_in() {
    local _fix="$1" _outvar="$2"; shift 2
    capture_print_plan "$_outvar" "${REIFY_PLAN_CAPTURE_RETRIES:-3}" \
        "$@" bash -c 'cd "$1" && exec bash scripts/verify.sh test --profile both --scope all --print-plan 2>/dev/null' _ "$_fix" \
        || true
}

FIX6=""
make_fixture FIX6
# Promote the unborn-HEAD source-closure fixture to a repo whose HEAD is a real
# 2-parent merge commit, so verify.sh's own _derive_merge_delta takes the
# first-parent branch (no MERGE_HEAD, no override).
git -C "$FIX6" add -A
git -C "$FIX6" commit -qm "base: verify.sh source closure"
_BASE6="$(git -C "$FIX6" symbolic-ref --short HEAD)"
git -C "$FIX6" checkout -q -b topic6
mkdir -p "$FIX6/$(dirname "$_TOPIC_FILE")"
echo 'pub fn probe() {}' > "$FIX6/$_TOPIC_FILE"
: > "$FIX6/Cargo.lock"                                   # C4 workspace-global => affected_crates ALL (no cargo)
git -C "$FIX6" add -A
git -C "$FIX6" commit -qm "topic6: reify-expr file + C4 global"
git -C "$FIX6" checkout -q "$_BASE6"
echo advance > "$FIX6/rider3_base6_only.txt"
git -C "$FIX6" add -A
git -C "$FIX6" commit -qm "base6 advances (diverge)"
git -C "$FIX6" merge -q --no-ff --no-edit topic6

PLAN_S6=""
_scenario_plan_in "$FIX6" PLAN_S6 env -u REIFY_INFRA_SUITE_ACTIVE -u REIFY_AFFECTED_CRATES_OVERRIDE REIFY_RELEASE_DELTA_SKIP=1 DF_VERIFY_ROLE=merge
assert "S6 knob-on/merge/real-merge/no-override: PLAN_S6 non-empty (verify.sh --print-plan OK)" _nonempty "$PLAN_S6"
assert "S6 knob-on/merge/real-merge/no-override: release nextest pass PRESENT (real derived delta => affected=ALL => fail-wide REQUIRED)" _has_release_pass "$PLAN_S6"
assert "S6 knob-on/merge/real-merge/no-override: delta-clean marker ABSENT" _lacks_skip_marker "$PLAN_S6"

# ===========================================================================
# ACTIVATION block (task ζ / 5280): REIFY_RELEASE_DELTA_SKIP durable verify_env
# wiring.
#   The delta-conditional release-pass skip stays inert until
#   REIFY_RELEASE_DELTA_SKIP=1 is genuinely exported into the merge-gate
#   verify.sh process tree (the role=merge guard at verify.sh:1161 gates on
#   it). Task ζ wires the flag into dark-factory-orchestrator.yaml's
#   verify_env block ONLY once the sweep backstop (PRD §5.3 (a)+(b)) is
#   demonstrably healthy — see task 5280's pre-1 prerequisite for the live
#   re-verification record. This block proves the KEY is wired AND valued
#   exactly "1" (not merely present as a comment mention elsewhere in the
#   yaml), mirroring sibling δ/5276's Section 9 in
#   test_run_all_content_skip.sh. RED until step-2 adds the verify_env line.
# ===========================================================================
echo ""
echo "--- ACTIVATION (task ζ): REIFY_RELEASE_DELTA_SKIP durable verify_env wiring ---"

# verify_env_exports — parse KEY=VALUE from the yaml verify_env block. Mirrored
# VERBATIM from tests/infra/test_verify_env_ambient_isolation.sh:59-85 (task
# 4966's drift-guard; the established 3-copy convention — no shared lib
# exists) as a reuse-note, NOT extracted to a shared lib, to avoid editing
# that live merge-gate guard file (out of this task's declared scope).
# Refresh all copies together if the awk logic ever changes.
verify_env_exports() {
    local yaml_file="$1"
    awk '
        /^verify_env:[[:space:]]*$/ { in_block = 1; next }
        in_block && /^[^[:space:]#]/ { in_block = 0 }
        !in_block { next }
        /^[[:space:]]*#/ { next }
        /^[[:space:]]*$/ { next }
        /^[[:space:]]+[A-Za-z_][A-Za-z0-9_]*:/ {
            line = $0
            sub(/^[[:space:]]+/, "", line)
            colon = index(line, ":")
            key = substr(line, 1, colon - 1)
            rest = substr(line, colon + 1)
            sub(/^[[:space:]]+/, "", rest)
            if (substr(rest, 1, 1) == "\"") {
                tail = substr(rest, 2)
                q = index(tail, "\"")
                val = (q > 0) ? substr(tail, 1, q - 1) : tail
            } else {
                n = split(rest, toks, /[[:space:]]+/)
                val = (n >= 1) ? toks[1] : ""
            }
            print key "=" val
        }
    ' "$yaml_file"
}

ACT_YAML="$REPO_ROOT/dark-factory-orchestrator.yaml"
assert "ACTIVATION: dark-factory-orchestrator.yaml exists" \
    test -f "$ACT_YAML"

ACT_VERIFY_ENV_KV="$(verify_env_exports "$ACT_YAML")"
assert "ACTIVATION: verify_env block parse is non-empty" \
    test -n "$ACT_VERIFY_ENV_KV"

# Parser sanity: a known-wired sibling must be visible to this parser, so an
# ACTIVATION miss below is a real absence, not a parser-shape regression.
assert "ACTIVATION: verify_env parse sees REIFY_RUN_ALL_SKIP_STATE (sibling wiring sanity)" \
    bash -c 'grep -qE "^REIFY_RUN_ALL_SKIP_STATE=" <<< "$1"' _ "$ACT_VERIFY_ENV_KV"

# (1) KEY presence — task ζ wires REIFY_RELEASE_DELTA_SKIP into verify_env.
#     RED now (the yaml line lands in step-2).
assert "ACTIVATION: REIFY_RELEASE_DELTA_SKIP is wired into the yaml verify_env block (task zeta)" \
    bash -c 'grep -qE "^REIFY_RELEASE_DELTA_SKIP=" <<< "$1"' _ "$ACT_VERIFY_ENV_KV"

# (2) exact VALUE "1" — proves a genuinely ACTIVE knob, not a stray "0"/empty
#     placeholder that would satisfy (1) alone but leave the skip permanently
#     inert (verify.sh:1161 requires the literal string "1").
assert "ACTIVATION: REIFY_RELEASE_DELTA_SKIP value is exactly \"1\" (genuinely active, not a placeholder)" \
    bash -c '
        val="$(grep -E "^REIFY_RELEASE_DELTA_SKIP=" <<< "$1" | head -n1)"
        val="${val#REIFY_RELEASE_DELTA_SKIP=}"
        [ "$val" = "1" ]
    ' _ "$ACT_VERIFY_ENV_KV"

# ===========================================================================
# CONTAINMENT block (task ζ / 5280; esc-5280-6 design ruling): the pool
# chokepoint strip.
#   The ACTIVATION above exports REIFY_RELEASE_DELTA_SKIP=1 into the WHOLE
#   merge-gate verify.sh process tree via the orchestrator verify_env. The
#   knob is meant to be consulted ONCE by the top-level merge verify at
#   plan-build time (verify.sh:1160-1178); its decision is frozen into the
#   PLAN before any plan line runs. Without containment the ambient knob
#   leaks into run_all.sh's pool, where members that re-assert
#   DF_VERIFY_ROLE=merge inline (test_verify_scope.sh MG-B5,
#   test_verify_role_prio.sh C1, the occt/release/semaphore plan-shape
#   meta-tests) would inherit knob=1 and see the release pass wrongly
#   suppressed on a delta-clean fixture — merge gate RED on every
#   delta-clean merge (esc-5280-6). run_all.sh neutralizes the knob for all
#   members with `export REIFY_RELEASE_DELTA_SKIP=0`, mirroring its existing
#   DF_VERIFY_ROLE=task normalization (merge-gate-riders K4: "never ambient
#   in the pool"). This asserts that chokepoint is present so it cannot be
#   silently dropped. Direct end-to-end proof (the victim member re-runs
#   GREEN under an ambient knob=1) is captured by the pool run itself; here
#   we pin the seam against regression.
# ===========================================================================
echo ""
echo "--- CONTAINMENT (task ζ): run_all.sh chokepoint strips the ambient knob ---"

ACT_RUN_ALL="$REPO_ROOT/tests/infra/run_all.sh"
assert "CONTAINMENT: tests/infra/run_all.sh exists" \
    test -f "$ACT_RUN_ALL"

# The chokepoint: run_all.sh must export REIFY_RELEASE_DELTA_SKIP=0 so no pool
# member (even one that re-asserts DF_VERIFY_ROLE=merge inline) ever inherits
# the ambient activation knob. Matches `export REIFY_RELEASE_DELTA_SKIP=0`
# with optional leading indentation; the value must be exactly 0.
assert "CONTAINMENT: run_all.sh neutralizes REIFY_RELEASE_DELTA_SKIP for pool members (=0)" \
    grep -qE '^[[:space:]]*export REIFY_RELEASE_DELTA_SKIP=0([[:space:]]|$)' "$ACT_RUN_ALL"

# Placement sanity: the strip must sit beside the sibling DF_VERIFY_ROLE=task
# normalization (both neutralize ambient merge-gate state for the pool), so a
# future refactor that moves one is prompted to move the other.
assert "CONTAINMENT: run_all.sh still normalizes DF_VERIFY_ROLE=task (sibling seam intact)" \
    grep -qE '^[[:space:]]*export DF_VERIFY_ROLE=task([[:space:]]|$)' "$ACT_RUN_ALL"

test_summary
