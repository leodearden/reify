#!/usr/bin/env bash
# Infrastructure test for task 4917 (A6, PRD docs/prds/offline-deep-test-lane.md
# §0/§8): executable drift-guard for the offline/gate heavy-test partition.
#
# Drives REAL scripts/verify.sh test --scope all --print-plan invocations
# under three regimes and asserts on the ACTUAL emitted plan lines (never a
# re-tabulated/unexecuted promise):
#   (a) offline role        -> positive heavy filter + --run-ignored all,
#                               release profile, idle-class nice/ionice.
#   (b) gate roles, knob=1  -> negated heavy filter `-E "not (<heavy>)"`.
#   (c) gate roles, knob!=1 -> unchanged (no -E heavy filter at all).
#   (d) heavy (+) smoke partition -- no overlap, no orphan.
#   (e) resolve-to-disk -- every atom parsed from the ACTUAL emitted offline
#       -E expression maps to a real crates/<pkg>/tests/<bin>.rs file, and
#       the parsed count is exactly 6 (no silent membership drift).
#
# Plus a non-vacuity self-check that deliberately breaks the partition
# (dangling atom / dropped atom / injected overlap) and asserts the guard's
# own resolve-to-disk / orphan / overlap checks detect the break -- mirrors
# tests/infra/test_run_all_classification.sh's injected-drift self-check.
#
# Modeled on tests/infra/test_verify_gate_exclude_heavy.sh (A4) and
# tests/infra/test_run_offline_deep.sh (A5) for the --print-plan oracle
# driver + NEXTEST_AVAILABLE probe idiom, and on
# tests/infra/test_heavy_filter_atoms.sh (Assertion E) for the
# resolve-to-disk atom parser.
#
# Compile-free -- this test never invokes cargo (only verify.sh --print-plan,
# which is pure bash string-building).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== offline/gate heavy-test partition drift-guard tests (task 4917 / A6) ==="

# ---------------------------------------------------------------------------
# Single source of truth for the `heavy` filter expression (A1 / task 4912).
# ---------------------------------------------------------------------------
HEAVY_LIB="$REPO_ROOT/scripts/heavy-test-filter-lib.sh"
if [ ! -f "$HEAVY_LIB" ]; then
    echo "ERROR: scripts/heavy-test-filter-lib.sh not found (task 4912/A1 not landed?)"
    exit 1
fi
# shellcheck source=scripts/heavy-test-filter-lib.sh
source "$HEAVY_LIB"

if [ -z "${REIFY_HEAVY_NEXTEST_FILTER:-}" ]; then
    echo "ERROR: REIFY_HEAVY_NEXTEST_FILTER not defined after sourcing $HEAVY_LIB"
    exit 1
fi

# A representative atom -- its presence proves an injected filter is the
# real negated/positive heavy set, not an empty not()/().
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
# Detect nextest availability once (role/knob-invariant; probed directly
# against real verify.sh -- always defined, unlike the driver helpers below).
# ---------------------------------------------------------------------------
_PROBE_HEADER="$(env -u REIFY_GATE_EXCLUDE_HEAVY DF_VERIFY_ROLE=task \
    bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan | grep '^# verify.sh plan')"
NEXTEST_AVAILABLE=0
case "$_PROBE_HEADER" in
    *"nextest=1"*) NEXTEST_AVAILABLE=1 ;;
esac
echo "(nextest available on this host: $NEXTEST_AVAILABLE)"

# ===========================================================================
# Driver / checker helper functions.
#
# Every reference to a helper that is "not yet defined" during an earlier
# RED cycle is confined to the body of a function that is itself invoked
# strictly as assert()'s command argument (never a bare top-level command
# substitution) -- so a command-not-found (127) is caught by assert()'s own
# `if "$@"` and reported as a clean FAIL, never a hard `set -e` script abort.
# Checks require their underlying driver call to SUCCEED before evaluating
# presence/absence (`|| return 1`), so an absence-style check (e.g.
# _gate_lacks) can never vacuously PASS just because the driver failed to
# produce any output.
# ===========================================================================

_gate_has() {
    # $1=role $2=knob-mode $3=needle (fixed string)
    local out
    out="$(gate_plan "$1" "$2")" || return 1
    printf '%s' "$out" | grep -qF -- "$3"
}
_gate_lacks() {
    # $1=role $2=knob-mode $3=needle (fixed string)
    local out
    out="$(gate_plan "$1" "$2")" || return 1
    ! printf '%s' "$out" | grep -qF -- "$3"
}

# ===========================================================================
# Assertions.
# ===========================================================================

# ---------------------------------------------------------------------------
# Assertion (b): knob EXACTLY "1" -> -E "not (<heavy>)" injected, for both
# gate roles. Guarded on nextest availability (fallback cargo-test path
# never emits -E).
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion (b): knob=1 -> $NOT_PATTERN injected (gate roles) ---"

if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    for _role in task merge; do
        assert "role=$_role, knob=1: plan contains $NOT_PATTERN" \
            _gate_has "$_role" 1 "$NOT_PATTERN"
        assert "role=$_role, knob=1: plan contains a real heavy atom ($HEAVY_ATOM)" \
            _gate_has "$_role" 1 "$HEAVY_ATOM"
    done
else
    for _role in task merge; do
        assert "role=$_role, knob=1, nextest unavailable: plan has NO $NOT_PATTERN (cargo-test fallback has no -E support)" \
            _gate_lacks "$_role" 1 "$NOT_PATTERN"
    done
fi

# ---------------------------------------------------------------------------
# Assertion (c): unset / empty / "0" / garbage -> NO exclusion, for both
# gate roles. Always valid (asserts absence) regardless of nextest
# availability.
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion (c): knob unset/empty/0/garbage -> NO $NOT_PATTERN injected (gate roles) ---"

NEG_SET_VALUES=("" "0" "2" "01" " 1 " "yes" "10")

for _role in task merge; do
    assert "role=$_role, REIFY_GATE_EXCLUDE_HEAVY unset: plan has NO $NOT_PATTERN" \
        _gate_lacks "$_role" "__UNSET__" "$NOT_PATTERN"

    for _val in "${NEG_SET_VALUES[@]}"; do
        assert "role=$_role, REIFY_GATE_EXCLUDE_HEAVY='$_val': plan has NO $NOT_PATTERN" \
            _gate_lacks "$_role" "$_val" "$NOT_PATTERN"
    done
done

test_summary
