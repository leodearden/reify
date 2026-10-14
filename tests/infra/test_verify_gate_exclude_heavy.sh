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
# Task 7912: the plan header `# heavy partition — HEAVY=excluded|only|included`
# reports the RESOLVED partition for the role, derived from the same fragments
# the nextest passes receive — so it is pinned both per role × knob and against
# the plan's own command lines. It is emitted iff the plan carries test passes.
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
# computed after both): verify.sh's `NEXTEST=0; if cargo nextest --version ...`
# probe derives NEXTEST from cargo-nextest resolvability ALONE, reading neither
# DF_VERIFY_ROLE nor REIFY_GATE_EXCLUDE_HEAVY, and the plan header interpolates
# that same $NEXTEST.
#
# WHAT THE SHARED PATH TRADES — not a free robustness win. The lib's extractor
# is `|| true`-guarded, so it does not remove the old failure mode, it CONVERTS
# it: where the old unguarded capture aborted the suite under `set -o pipefail`,
# this one answers "not available" and carries on. That moves the failure TOWARD
# vacuous green, not away from it, and dropping the role pin supplies a concrete
# trigger — an ambient unrecognized role now short-circuits the probe
# (`DF_VERIFY_ROLE=bogus bash scripts/verify.sh test --scope all --print-plan`
# exits 64 with nothing on stdout, measured), where the pinned form was immune.
#
# What makes that acceptable is NOT the guard — it is the else branch below. A
# false "not available" on a nextest-present host takes the fallback arm, whose
# "role=..., knob=1, nextest unavailable: plan has NO ..." assert then fails
# loudly against a plan that DOES carry the -E exclusion. So that arm is this
# probe's only detector of a wrong answer: do not delete it as dead weight on a
# nextest-present host.
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

# ---------------------------------------------------------------------------
# heavy partition header (task 7912): the plan states the RESOLVED effect of
# the knob for the role, never the env value — the knob being set is not the
# knob deciding anything. Each plan is captured WITH its comment lines, and
# every one is also checked against its own command lines, so the header can
# never contradict what the nextest passes actually receive.
# ---------------------------------------------------------------------------
echo ""
echo "--- heavy partition header (task 7912): the plan states the RESOLVED effect, not the env ---"

HEAVY_HEADER_PREFIX='# heavy partition — '
# A test pass on either path: nextest, or the cargo-test fallback.
TEST_PASS_PATTERN='cargo (nextest run|test) '

_heavy_header() {
    printf '%s\n' "$1" | grep -m1 -- "^$HEAVY_HEADER_PREFIX" || true
}

# Non-vacuous by construction: fails when the header is absent, and fails when
# the plan has no test pass for the header to describe (else `included` —
# neither fragment present — would also match a plan with no tests at all).
_heavy_header_matches_commands() {
    local header value commands has_not=0 has_pos=0
    header="$(_heavy_header "$1")"
    [ -n "$header" ] || return 1
    value="${header##*HEAVY=}"
    commands="$(printf '%s\n' "$1" | grep -v '^#' || true)"
    printf '%s\n' "$commands" | grep -qE -- "$TEST_PASS_PATTERN" || return 1
    printf '%s\n' "$commands" | grep -qF -- "$NOT_PATTERN" && has_not=1
    printf '%s\n' "$commands" | grep -qF -- "$POSITIVE_PATTERN" && has_pos=1
    case "$value" in
        excluded) [ "$has_not" -eq 1 ] && [ "$has_pos" -eq 0 ] ;;
        only)     [ "$has_not" -eq 0 ] && [ "$has_pos" -eq 1 ] ;;
        included) [ "$has_not" -eq 0 ] && [ "$has_pos" -eq 0 ] ;;
        *)        return 1 ;;
    esac
}

_assert_heavy_header() {
    local desc="$1" plan="$2" want="$3"
    assert "$desc" \
        bash -c 'printf "%s\n" "$1" | grep -qF -- "HEAVY=$2"' \
        _ "$(_heavy_header "$plan")" "$want"
}

_capture_full_plan() {
    bash "$REPO_ROOT/scripts/verify.sh" "$1" --scope all --print-plan || true
}

# Case labels and their plans, kept in step so the consistency pass (5)
# covers every plan captured by cases 1-4.
HEAVY_CASE_LABELS=()
HEAVY_CASE_PLANS=()

_plan="$(DF_VERIFY_ROLE=background REIFY_GATE_EXCLUDE_HEAVY=1 _capture_full_plan test)"
_assert_heavy_header \
    "role=background, knob=1: header says HEAVY=included (env set, effect none — the knob is role-scoped)" \
    "$_plan" included
HEAVY_CASE_LABELS+=("role=background, knob=1"); HEAVY_CASE_PLANS+=("$_plan")

if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    _gate_knob_on_want=excluded; _gate_knob_on_why=""
else
    _gate_knob_on_want=included; _gate_knob_on_why=" (the cargo-test fallback has no -E)"
fi
for _role in task merge; do
    _plan="$(DF_VERIFY_ROLE="$_role" REIFY_GATE_EXCLUDE_HEAVY=1 _capture_full_plan test)"
    _assert_heavy_header \
        "role=$_role, knob=1 (nextest=$NEXTEST_AVAILABLE): header says HEAVY=$_gate_knob_on_want$_gate_knob_on_why" \
        "$_plan" "$_gate_knob_on_want"
    HEAVY_CASE_LABELS+=("role=$_role, knob=1"); HEAVY_CASE_PLANS+=("$_plan")
done

_plan="$(env -u REIFY_GATE_EXCLUDE_HEAVY DF_VERIFY_ROLE=task \
    bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan || true)"
_assert_heavy_header "role=task, knob unset: header says HEAVY=included" "$_plan" included
HEAVY_CASE_LABELS+=("role=task, knob unset"); HEAVY_CASE_PLANS+=("$_plan")

_plan="$(DF_VERIFY_ROLE=task REIFY_GATE_EXCLUDE_HEAVY=0 _capture_full_plan test)"
_assert_heavy_header "role=task, knob=0: header says HEAVY=included" "$_plan" included
HEAVY_CASE_LABELS+=("role=task, knob=0"); HEAVY_CASE_PLANS+=("$_plan")

if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    _plan="$(env -u REIFY_GATE_EXCLUDE_HEAVY DF_VERIFY_ROLE=offline \
        bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan || true)"
    _assert_heavy_header "role=offline, knob unset: header says HEAVY=only" "$_plan" only
    HEAVY_CASE_LABELS+=("role=offline, knob unset"); HEAVY_CASE_PLANS+=("$_plan")
else
    echo "--- role=offline HEAVY=only header assertion SKIPPED (nextest not available on this host) ---"
fi

for _i in "${!HEAVY_CASE_PLANS[@]}"; do
    assert "${HEAVY_CASE_LABELS[$_i]}: heavy partition header is present and agrees with the plan's test-pass command lines" \
        _heavy_header_matches_commands "${HEAVY_CASE_PLANS[$_i]}"
done

_plan="$(DF_VERIFY_ROLE=task REIFY_GATE_EXCLUDE_HEAVY=1 _capture_full_plan lint)"
assert "role=task, knob=1, action=lint: plan has NO heavy partition header (no test passes to describe; scoping guard, green on arrival)" \
    bash -c '! printf "%s\n" "$1" | grep -q -- "^$2"' \
    _ "$_plan" "$HEAVY_HEADER_PREFIX"

# A TEST action can still carry no test passes: `--scope staged` on a clean
# index classifies RUN_RUST=0. Captured in a throwaway repo (scripts/ and
# .config/ committed, nothing staged) so neither the host checkout's index nor
# a MERGE_HEAD in it (which forces --scope all) can change that classification.
CLEAN_INDEX_FIX="$(mktemp -d)"
trap 'rm -rf "$CLEAN_INDEX_FIX"' EXIT
cp -R "$REPO_ROOT/scripts" "$REPO_ROOT/.config" "$CLEAN_INDEX_FIX/"
git -C "$CLEAN_INDEX_FIX" init -q
git -C "$CLEAN_INDEX_FIX" add scripts .config
git -C "$CLEAN_INDEX_FIX" -c user.email=test@invalid.local -c user.name=test commit -q -m base

_plan="$(DF_VERIFY_ROLE=task REIFY_GATE_EXCLUDE_HEAVY=1 \
    bash "$CLEAN_INDEX_FIX/scripts/verify.sh" test --scope staged --print-plan || true)"
assert "role=task, knob=1, action=test, --scope staged, clean index: plan classifies RUN_RUST=0 (precondition — a test action with no test passes)" \
    bash -c 'printf "%s\n" "$1" | grep -qF -- "# scope decision — RUN_RUST=0 "' \
    _ "$_plan"
assert "role=task, knob=1, action=test, --scope staged, clean index: plan has NO heavy partition header (it describes test passes, and there are none)" \
    bash -c '! printf "%s\n" "$1" | grep -q -- "^$2"' \
    _ "$_plan" "$HEAVY_HEADER_PREFIX"

test_summary
