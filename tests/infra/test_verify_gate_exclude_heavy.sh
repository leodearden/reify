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
# Detect nextest availability once. NEXTEST is computed in verify.sh before
# any role/knob logic runs and is role/knob-invariant; the plan header
# `nextest=$NEXTEST` reports it directly. Positive assertions (below) only
# make sense on the nextest path — the cargo-test fallback has no -E support.
# ---------------------------------------------------------------------------
_PROBE_HEADER="$(env -u REIFY_GATE_EXCLUDE_HEAVY DF_VERIFY_ROLE=task \
    bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan | grep '^# verify.sh plan')"
NEXTEST_AVAILABLE=0
case "$_PROBE_HEADER" in
    *"nextest=1"*) NEXTEST_AVAILABLE=1 ;;
esac
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

test_summary
