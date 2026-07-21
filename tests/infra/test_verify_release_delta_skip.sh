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

test_summary
