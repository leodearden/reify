#!/usr/bin/env bash
# Infrastructure test for task 4915 (A4): REIFY_GATE_EXCLUDE_HEAVY knob-gated
# gate exclusion.
#
# Contract (PRD §6/§8, DA1/DA2 flip-seam): scripts/verify.sh gate roles
# (task/merge) apply the nextest filter `-E "not (<heavy>)"` IFF the env var
# REIFY_GATE_EXCLUDE_HEAVY is EXACTLY the string "1"; any other value
# (unset/empty/"0"/garbage) leaves the gate running the full test set
# unchanged (strictly-additive-on-landing invariant — a malformed knob must
# never silently create a coverage hole).
#
# Modeled on tests/infra/test_verify_role_prio.sh: drives verify.sh via
# --print-plan (hermetic — never builds/tests anything, no cargo invoked).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

# For nextest_available_ambient (the plan-header availability probe below).
# Sourcing the lib installs no trap and builds no environment — only
# nextest_absent_init does that, and this suite deliberately never calls it.
[ -f "$SCRIPT_DIR/nextest_absent_lib.sh" ] || {
    echo "ERROR: nextest_absent_lib.sh not found at $SCRIPT_DIR/nextest_absent_lib.sh"; exit 1; }
source "$SCRIPT_DIR/nextest_absent_lib.sh"

echo "=== REIFY_GATE_EXCLUDE_HEAVY knob-gated gate exclusion tests (task 4915 / A4) ==="

# Single source of truth for the `heavy` filter expression (A1 / task 4912) —
# lets this test assert on a real atom substring instead of hand-duplicating
# the expression, so the fixture can never silently drift from
# scripts/heavy-test-filter-lib.sh.
LIB="$REPO_ROOT/scripts/heavy-test-filter-lib.sh"
if [ ! -f "$LIB" ]; then
    echo "ERROR: scripts/heavy-test-filter-lib.sh not found (task 4912/A1 not landed?)"
    exit 1
fi
# shellcheck source=scripts/heavy-test-filter-lib.sh
source "$LIB"

if [ -z "${REIFY_HEAVY_NEXTEST_FILTER:-}" ]; then
    echo "ERROR: REIFY_HEAVY_NEXTEST_FILTER not defined after sourcing $LIB"
    exit 1
fi

# A representative atom body drawn from the real expression — its presence in
# the plan proves the injected filter is the actual negated heavy set, not an
# empty `not ()`.
HEAVY_ATOM="binary(determinism)"
case "$REIFY_HEAVY_NEXTEST_FILTER" in
    *"$HEAVY_ATOM"*) ;;
    *)
        echo "ERROR: fixture atom '$HEAVY_ATOM' not found in REIFY_HEAVY_NEXTEST_FILTER — this test's fixture has drifted from scripts/heavy-test-filter-lib.sh"
        exit 1
        ;;
esac

NOT_PATTERN='-E "not ('

# ---------------------------------------------------------------------------
# Detect nextest availability once, via the shared detector in
# tests/infra/nextest_absent_lib.sh (task 5644) — the same plan-header parse
# seven suites had each open-coded. Positive assertions (below) only make sense
# on the nextest path; the cargo-test fallback has no -E support.
#
# This probe makes its own dedicated --print-plan capture (read by nothing else
# in this file), so it takes the AMBIENT form rather than nextest_available_in_
# plan.
#
# The dropped `env -u REIFY_GATE_EXCLUDE_HEAVY DF_VERIFY_ROLE=task` pin.
# nextest_available_ambient runs verify.sh with no env prefix, so the migration
# only preserves behaviour if NEXTEST is genuinely role/knob-invariant. It is,
# and for a checkable reason rather than the one the old comment gave (it said
# NEXTEST is computed "before any role/knob logic runs" — it is not; it is
# computed after both): scripts/verify.sh:1509-1544 derives NEXTEST from
# `cargo nextest --version` / `command -v cargo-nextest` ALONE, reading neither
# DF_VERIFY_ROLE (defaulted :616) nor REIFY_GATE_EXCLUDE_HEAVY (read :709). The
# header at :2598 interpolates that same $NEXTEST.
#
# Net robustness gain, too: the old capture had no `|| true`, so a verify.sh
# hiccup aborted the suite under `set -o pipefail` before test_summary. The lib
# path is guarded at nextest_absent_lib.sh:712 and degrades to "not available".
# ---------------------------------------------------------------------------
NEXTEST_AVAILABLE=0
if nextest_available_ambient "$REPO_ROOT/scripts/verify.sh"; then
    NEXTEST_AVAILABLE=1
fi
echo "(nextest available on this host: $NEXTEST_AVAILABLE)"

# ---------------------------------------------------------------------------
# Positive matrix: knob EXACTLY "1" -> -E "not (<heavy>)" injected, for both
# gate roles. Guarded on nextest availability (fallback cargo-test path never
# emits -E, by design — task 4915 plan decision).
# ---------------------------------------------------------------------------
if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    echo ""
    echo "--- knob=1 (nextest available): expect \"$NOT_PATTERN\" + heavy atom injected ---"

    for _role in task merge; do
        _plan="$(DF_VERIFY_ROLE="$_role" REIFY_GATE_EXCLUDE_HEAVY=1 \
            bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan | grep -v '^#')"

        assert "role=$_role, knob=1: plan contains $NOT_PATTERN" \
            bash -c 'printf "%s\n" "$1" | grep -qF -- "$2"' \
            _ "$_plan" "$NOT_PATTERN"

        assert "role=$_role, knob=1: plan contains a real heavy atom ($HEAVY_ATOM)" \
            bash -c 'printf "%s\n" "$1" | grep -qF -- "$2"' \
            _ "$_plan" "$HEAVY_ATOM"
    done
else
    echo ""
    echo "--- knob=1 positive assertions SKIPPED (nextest not available on this host) ---"
    echo "--- knob=1 (nextest unavailable): expect fallback cargo-test path NEVER emits $NOT_PATTERN ---"

    for _role in task merge; do
        _plan="$(DF_VERIFY_ROLE="$_role" REIFY_GATE_EXCLUDE_HEAVY=1 \
            bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan | grep -v '^#')"

        assert "role=$_role, knob=1, nextest unavailable: plan has NO $NOT_PATTERN (cargo-test fallback has no -E support)" \
            bash -c '! printf "%s\n" "$1" | grep -qF -- "$2"' \
            _ "$_plan" "$NOT_PATTERN"
    done
fi

# ---------------------------------------------------------------------------
# Negative matrix: unset / empty / "0" / garbage -> NO exclusion, for both
# gate roles. Always valid (asserts absence) regardless of nextest
# availability -- the strict-"1" coverage-hole guard (PRD §8).
# ---------------------------------------------------------------------------
echo ""
echo "--- unset/empty/0/garbage knob values: expect NO $NOT_PATTERN injected ---"

# Values applied via REIFY_GATE_EXCLUDE_HEAVY=<value> (i.e. "set"). The
# genuinely-unset case is handled separately below via `env -u`.
NEG_SET_VALUES=("" "0" "2" "01" " 1 " "yes" "10")

for _role in task merge; do
    _plan="$(env -u REIFY_GATE_EXCLUDE_HEAVY DF_VERIFY_ROLE="$_role" \
        bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan | grep -v '^#')"
    assert "role=$_role, REIFY_GATE_EXCLUDE_HEAVY unset: plan has NO $NOT_PATTERN" \
        bash -c '! printf "%s\n" "$1" | grep -qF -- "$2"' \
        _ "$_plan" "$NOT_PATTERN"

    for _val in "${NEG_SET_VALUES[@]}"; do
        _plan="$(DF_VERIFY_ROLE="$_role" REIFY_GATE_EXCLUDE_HEAVY="$_val" \
            bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan | grep -v '^#')"
        assert "role=$_role, REIFY_GATE_EXCLUDE_HEAVY='$_val': plan has NO $NOT_PATTERN" \
            bash -c '! printf "%s\n" "$1" | grep -qF -- "$2"' \
            _ "$_plan" "$NOT_PATTERN"
    done
done

# ---------------------------------------------------------------------------
# background role (task 5210): a NEGATIVE regardless of the knob value.
# background is not a task/merge gate role (the negated-exclude fragment is
# scoped explicitly to task/merge, PRD §6/§8), so REIFY_GATE_EXCLUDE_HEAVY=1
# must NOT inject $NOT_PATTERN — a main integrity sweep needs full coverage,
# never a heavy-excluded subset. Nor is background the offline role, so it
# must NOT pick up offline's POSITIVE heavy-select fragment ($POSITIVE_PATTERN)
# either — background matches neither guard, so this holds independent of
# nextest availability (unlike the positive matrix above).
# ---------------------------------------------------------------------------
echo ""
echo "--- background role (task 5210): REIFY_GATE_EXCLUDE_HEAVY=1 must have NO effect ---"

POSITIVE_PATTERN='-E "('

# Sanity so the NO-pattern assertions below are non-vacuous (an unrecognized
# role produces NO plan at all, which would vacuously satisfy both negative
# checks for the wrong reason). Confirms DF_VERIFY_ROLE=background is a
# recognized role and plan generation exits 0, so the negative checks below
# exercise a real plan rather than passing vacuously.
assert "role=background, knob=1: verify.sh exits 0 (plan generation succeeds)" \
    bash -c 'DF_VERIFY_ROLE=background REIFY_GATE_EXCLUDE_HEAVY=1 bash "$1/scripts/verify.sh" test --scope all --print-plan >/dev/null 2>&1' \
    _ "$REPO_ROOT"

# Guarded with '|| true' so an as-yet-unrecognized role (RED phase, pre
# task-5210 step-2) reports a clean assertion FAIL above instead of tripping
# this script's own `set -eo pipefail` on the failing verify.sh exit code.
BACKGROUND_HEAVY_PLAN="$(DF_VERIFY_ROLE=background REIFY_GATE_EXCLUDE_HEAVY=1 \
    bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan | grep -v '^#' || true)"

assert "role=background, knob=1: plan has NO $NOT_PATTERN (background is not a task/merge gate role)" \
    bash -c '! printf "%s\n" "$1" | grep -qF -- "$2"' \
    _ "$BACKGROUND_HEAVY_PLAN" "$NOT_PATTERN"

assert "role=background, knob=1: plan has NO $POSITIVE_PATTERN (background is not the offline role)" \
    bash -c '! printf "%s\n" "$1" | grep -qF -- "$2"' \
    _ "$BACKGROUND_HEAVY_PLAN" "$POSITIVE_PATTERN"

test_summary
