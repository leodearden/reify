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
#       the parsed count is exactly 7 (task 6368 added the 7th, test-scoped
#       atom), PLUS every ` & test(/^<stem>::/)` sub-clause the lib carries
#       survives VERBATIM into that emitted expression.
#
# WHAT THE DRIFT-GUARD IN THIS FILE DOES AND DOES NOT COVER. Every atom
# parser here -- heavy_atoms(), parse_atoms_from_plan(), _resolve_atoms_ok's
# _atom_re -- matches only the `package(X) & binary(Y)` PREFIX of an atom.
# That is deliberate (it must keep counting test-scoped and whole-binary
# atoms alike), but it makes the count / no-orphan / resolve-to-disk trio
# BINARY-MEMBERSHIP guards, not whole-atom guards: on their own they would
# stay green if the 7th atom's ` & test(...)` clause were silently deleted,
# even though that converts it to whole-binary and evicts all 247 tests in
# harness_fea_solver_e2e from the merge gate to relieve one -- the exact
# outcome esc-6368-2 rescoped AWAY from. Two things close that, at
# different granularities, and neither is this file's parsers:
#   - the EXACT-LITERAL pin on the clause lives in
#     tests/infra/test_heavy_filter_atoms.sh Assertion C (and the
#     resolve-the-stem-to-disk check in its Assertion F). That suite owns
#     the clause's spelling; this one deliberately does not duplicate it.
#   - the clause SURVIVING verify.sh's interpolation into the real emitted
#     plan is what this file adds, in assertion (e) below -- derived from
#     the lib rather than hardcoded, so a legitimate future stem rename does
#     not false-fail here, with a >=1-clause non-vacuity guard so a DELETED
#     clause cannot pass it vacuously.
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

# For nextest_available_ambient (the plan-header availability probe below).
# Sourcing the lib installs no trap and builds no environment — only
# nextest_absent_init does that, and this suite deliberately never calls it.
[ -f "$SCRIPT_DIR/nextest_absent_lib.sh" ] || {
    echo "ERROR: nextest_absent_lib.sh not found at $SCRIPT_DIR/nextest_absent_lib.sh"; exit 1; }
source "$SCRIPT_DIR/nextest_absent_lib.sh"

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
# Detect nextest availability once, via the shared detector in
# tests/infra/nextest_absent_lib.sh (task 5644) -- the same plan-header parse
# seven suites had each open-coded. Still probed directly against real
# verify.sh, so it is always defined, unlike the driver helpers below.
#
# This probe makes its own dedicated --print-plan capture (read by nothing else
# in this file), so it takes the AMBIENT form rather than
# nextest_available_in_plan.
#
# The dropped `env -u REIFY_GATE_EXCLUDE_HEAVY DF_VERIFY_ROLE=task` pin.
# nextest_available_ambient runs verify.sh with no env prefix, so this only
# preserves behaviour if NEXTEST is genuinely role/knob-invariant -- verified in
# the source: verify.sh's `NEXTEST=0; if cargo nextest --version ...` probe
# derives NEXTEST from cargo-nextest resolvability ALONE, reading neither
# DF_VERIFY_ROLE nor REIFY_GATE_EXCLUDE_HEAVY, and the plan header interpolates
# that same $NEXTEST.
#
# UNDER THE NEXTEST-ABSENT HARNESS. This suite is S3 in
# tests/infra/test_verify_nextest_absent_suites.sh, so it also runs as a child
# of nx_run. Its "ambient" env IS the harness env there (HOME/PATH already
# redirected by the parent), and nextest_available_ambient's un-prefixed
# `bash "$verify" --print-plan` inherits it exactly as the old open-coded
# capture did -- reading nextest=0, NEXTEST_AVAILABLE=0, unchanged.
#
# WHAT THE SHARED PATH TRADES -- not a free robustness win. The lib's extractor
# is `|| true`-guarded, so it does not remove the old failure mode, it CONVERTS
# it: where the old unguarded capture aborted the suite under `set -o pipefail`,
# this one answers "not available" and carries on. That moves the failure TOWARD
# vacuous green, not away from it, and dropping the role pin supplies a concrete
# trigger -- an ambient unrecognized role now short-circuits the probe
# (`DF_VERIFY_ROLE=bogus bash scripts/verify.sh test --scope all --print-plan`
# exits 64 with nothing on stdout, measured), where the pinned form was immune.
#
# What makes that acceptable is NOT the guard -- it is the else arm of every
# NEXTEST_AVAILABLE branch below. A false "not available" on a nextest-present
# host routes assertion (b) into `_gate_lacks ... "$NOT_PATTERN"` and assertions
# (c)/(d) into `_offline_lacks '-E "('`, all three of which then fail loudly
# against a plan that DOES carry the -E expression. Those else arms are this
# probe's only detector of a wrong answer: do not delete them as dead weight on
# a nextest-present host.
# ---------------------------------------------------------------------------
NEXTEST_AVAILABLE=0
if nextest_available_ambient "$REPO_ROOT/scripts/verify.sh"; then
    NEXTEST_AVAILABLE=1
fi
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

# plan_for <role> <knob-mode> — the shared --print-plan oracle driver for
# gate roles. <knob-mode> is the literal sentinel "__UNSET__" for a
# genuinely-unset REIFY_GATE_EXCLUDE_HEAVY (env -u), or any other string to
# set REIFY_GATE_EXCLUDE_HEAVY to that literal value. Emits the FULL raw
# plan (header + commands) on stdout.
plan_for() {
    local _role="$1" _knob="$2"
    if [ "$_knob" = "__UNSET__" ]; then
        env -u REIFY_GATE_EXCLUDE_HEAVY DF_VERIFY_ROLE="$_role" \
            bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan
    else
        DF_VERIFY_ROLE="$_role" REIFY_GATE_EXCLUDE_HEAVY="$_knob" \
            bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan
    fi
}

# _dump_plan_evidence <label> <raw-plan-text> <rc> — writes the FULL raw
# plan (header included -- carries the diagnostic nextest=0/1 field) plus
# the driver's exit code to STDERR under a stable marker, so a failing
# oracle check preserves its evidence instead of returning 1 silently
# (esc-4959-57/esc-4959-56). When the checker runs as assert()'s own
# command argument, assert's tmpfile capture (test_helpers.sh) picks this
# up and dumps it right after the "  FAIL:" line in the archived verify log.
_dump_plan_evidence() {
    local label="$1" raw="$2" rc="$3"
    echo "  [PLAN-DUMP] $label: driver rc=$rc; full captured plan follows:" >&2
    printf '%s\n' "$raw" >&2
}

_gate_has() {
    # $1=role $2=knob-mode $3=needle (fixed string)
    local raw rc=0
    raw="$(plan_for "$1" "$2")" || rc=$?
    if [ "$rc" -eq 0 ] && printf '%s\n' "$raw" | grep -v '^#' | grep -qF -- "$3"; then
        return 0
    fi
    _dump_plan_evidence "_gate_has $1 $2 '$3'" "$raw" "$rc"
    return 1
}
_gate_lacks() {
    # $1=role $2=knob-mode $3=needle (fixed string)
    local raw rc=0
    raw="$(plan_for "$1" "$2")" || rc=$?
    if [ "$rc" -eq 0 ] && ! printf '%s\n' "$raw" | grep -v '^#' | grep -qF -- "$3"; then
        return 0
    fi
    _dump_plan_evidence "_gate_lacks $1 $2 '$3'" "$raw" "$rc"
    return 1
}

# offline_plan — memoized offline (DF_VERIFY_ROLE=offline) --print-plan
# capture (FULL raw plan: header + commands). Memoized because assertion
# groups (a)/(d)/(e) each query it multiple times and it's otherwise a
# fresh verify.sh subprocess per call.
_OFFLINE_PLAN_CACHE=""
_OFFLINE_PLAN_CACHED=0
offline_plan() {
    if [ "$_OFFLINE_PLAN_CACHED" -eq 0 ]; then
        _OFFLINE_PLAN_CACHE="$(env -u REIFY_GATE_EXCLUDE_HEAVY DF_VERIFY_ROLE=offline \
            bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan)" || return 1
        _OFFLINE_PLAN_CACHED=1
    fi
    printf '%s' "$_OFFLINE_PLAN_CACHE"
}

# heavy_atoms — the 7 `package(X) & binary(Y)` atoms parsed directly out of
# REIFY_HEAVY_NEXTEST_FILTER (the lib source-of-truth, A1), one per line.
# PREFIX-ONLY by design: a test-scoped atom's trailing ` & test(...)` clause
# is not captured here, so this (and everything built on it) is a
# binary-membership view. See the header's coverage note; the clause itself
# is covered by heavy_test_scope_clauses() below plus
# tests/infra/test_heavy_filter_atoms.sh Assertions C/F.
heavy_atoms() {
    printf '%s' "$REIFY_HEAVY_NEXTEST_FILTER" | grep -oE 'package\([a-z0-9_-]+\) & binary\([a-z0-9_-]+\)'
}

# heavy_test_scope_clauses — the ` & test(/^<stem>::/)` sub-clauses carried by
# REIFY_HEAVY_NEXTEST_FILTER (the lib source-of-truth, A1), one per line.
# The complement of heavy_atoms(): that one sees only the prefix every atom
# shares, this one sees only the suffix that narrows an atom below whole-binary.
heavy_test_scope_clauses() {
    printf '%s' "$REIFY_HEAVY_NEXTEST_FILTER" | grep -oE '& test\(/\^[a-z0-9_]+::/\)'
}

# parse_atoms_from_plan [text] — the `package(X) & binary(Y)` atoms parsed
# directly out of the ACTUAL emitted offline plan's command lines (default:
# offline_plan's command lines), or an explicit override (used by the
# non-vacuity self-check below), one per line. Reused from
# test_heavy_filter_atoms.sh's Assertion E parser, but sourced from the
# ACTUAL emitted -E expression rather than REIFY_HEAVY_NEXTEST_FILTER
# directly -- proving verify.sh's real output resolves to disk (assertion e),
# not merely the lib source-of-truth (which heavy_atoms() already covers).
# PREFIX-ONLY, exactly like heavy_atoms(): the emitted-plan counterpart for
# test-scope clauses is _test_scope_clauses_survive_ok below, not this.
parse_atoms_from_plan() {
    local cmds
    if [ "$#" -eq 0 ]; then
        cmds="$(offline_plan | grep -v '^#')" || return 1
    else
        cmds="$1"
    fi
    printf '%s\n' "$cmds" | grep -oE 'package\([a-z0-9_-]+\) & binary\([a-z0-9_-]+\)'
}

_offline_header_has() {
    local plan rc=0
    plan="$(offline_plan)" || rc=$?
    if [ "$rc" -eq 0 ] && printf '%s\n' "$plan" | grep '^# verify.sh plan' | grep -qF -- "$1"; then
        return 0
    fi
    _dump_plan_evidence "_offline_header_has '$1'" "$plan" "$rc"
    return 1
}
_offline_cmds_has() {
    local plan rc=0
    plan="$(offline_plan)" || rc=$?
    if [ "$rc" -eq 0 ] && printf '%s\n' "$plan" | grep -v '^#' | grep -qF -- "$1"; then
        return 0
    fi
    _dump_plan_evidence "_offline_cmds_has '$1'" "$plan" "$rc"
    return 1
}
_offline_lacks() {
    local plan rc=0
    plan="$(offline_plan)" || rc=$?
    if [ "$rc" -eq 0 ] && ! printf '%s\n' "$plan" | grep -qF -- "$1"; then
        return 0
    fi
    _dump_plan_evidence "_offline_lacks '$1'" "$plan" "$rc"
    return 1
}
_offline_has_cargo_line() {
    local plan cmds n rc=0
    plan="$(offline_plan)" || rc=$?
    if [ "$rc" -eq 0 ]; then
        cmds="$(printf '%s\n' "$plan" | grep -v '^#')"
        n="$(printf '%s\n' "$cmds" | grep -cE '(^| )cargo ' || true)"
        if [ "${n:-0}" -ge 1 ]; then
            return 0
        fi
    fi
    _dump_plan_evidence "_offline_has_cargo_line" "$plan" "$rc"
    return 1
}
_offline_all_cargo_lines_idle_class() {
    local plan cmds rc=0
    plan="$(offline_plan)" || rc=$?
    if [ "$rc" -eq 0 ]; then
        cmds="$(printf '%s\n' "$plan" | grep -v '^#')"
        if ! printf '%s\n' "$cmds" | grep -E '(^| )cargo ' | grep -vq 'nice -n 19 ionice -c3 cargo'; then
            return 0
        fi
    fi
    _dump_plan_evidence "_offline_all_cargo_lines_idle_class" "$plan" "$rc"
    return 1
}

# _no_overlap_ok [expr-text] — 0 iff <expr-text> (default:
# REIFY_HEAVY_NEXTEST_FILTER) does NOT mention the lighter gate-smoke binary
# (solver_gate_smoke). Accepts an explicit override so the non-vacuity
# self-check (step-5/6) can feed a synthetic broken expression through the
# SAME check.
_no_overlap_ok() {
    local expr
    if [ "$#" -eq 0 ]; then
        expr="$REIFY_HEAVY_NEXTEST_FILTER"
    else
        expr="$1"
    fi
    case "$expr" in
        *"solver_gate_smoke"*) return 1 ;;
        *) return 0 ;;
    esac
}

# _no_orphan_ok [expr-text] — 0 iff every atom from heavy_atoms is present
# in <expr-text> (default: the ACTUAL offline plan's command lines).
# Accepts an explicit override for the non-vacuity self-check (step-5/6).
_no_orphan_ok() {
    local expr
    if [ "$#" -eq 0 ]; then
        expr="$(offline_plan | grep -v '^#')" || return 1
    else
        expr="$1"
    fi
    local atoms
    atoms="$(heavy_atoms)" || return 1
    [ -n "$atoms" ] || return 1
    while IFS= read -r _atom; do
        [ -n "$_atom" ] || continue
        printf '%s\n' "$expr" | grep -qF -- "$_atom" || return 1
    done <<< "$atoms"
    return 0
}

# _test_scope_clauses_survive_ok — 0 iff the lib carries AT LEAST ONE
# ` & test(...)` clause AND every one of them appears VERBATIM in the ACTUAL
# emitted offline -E expression. Two distinct defects, one check:
#   - clause DELETED from the lib -> the >=1 non-vacuity guard fires. This is
#     the hole the header's coverage note names: every prefix-only parser in
#     this file (count-7 / no-orphan / resolve-to-disk) stays green through
#     that deletion, because the atom keeps its package()/binary() prefix and
#     its binary keeps resolving -- it has merely widened from one test to all
#     247 in harness_fea_solver_e2e.
#   - clause MANGLED in transit -> the verbatim grep -F fires. Nothing
#     previously asserted that verify.sh interpolates the clause into the
#     emitted plan intact; the plan is built by shell string-concatenation
#     around an expression containing `(`, `/`, `^` and `::`, so "the lib says
#     X" and "the plan carries X" are genuinely separate claims.
# Derived from the lib rather than hardcoded, deliberately: the exact-literal
# pin belongs to tests/infra/test_heavy_filter_atoms.sh Assertion C, so a
# legitimate future stem rename lands as ONE expected failure there rather
# than also false-failing here.
#
# Takes an optional [expr-text] override (default: the ACTUAL offline plan's
# command lines), matching _no_orphan_ok / _no_overlap_ok / _resolve_atoms_ok,
# so the non-vacuity self-check below can feed a synthetic clause-stripped
# expression through the SAME check rather than a re-implementation of it.
_test_scope_clauses_survive_ok() {
    local clauses expr rc=0
    clauses="$(heavy_test_scope_clauses)" || return 1
    [ -n "$clauses" ] || return 1
    if [ "$#" -eq 0 ]; then
        expr="$(offline_plan | grep -v '^#')" || rc=$?
        if [ "$rc" -ne 0 ]; then
            _dump_plan_evidence "_test_scope_clauses_survive_ok (driver failed)" "$expr" "$rc"
            return 1
        fi
    else
        expr="$1"
    fi
    while IFS= read -r _clause; do
        [ -n "$_clause" ] || continue
        if ! printf '%s\n' "$expr" | grep -qF -- "$_clause"; then
            # Only dump when checking the REAL plan -- an override miss is the
            # self-check's expected outcome, not evidence worth archiving.
            if [ "$#" -eq 0 ]; then
                _dump_plan_evidence "_test_scope_clauses_survive_ok (clause '$_clause' absent from emitted plan)" "$expr" "$rc"
            fi
            return 1
        fi
    done <<< "$clauses"
    return 0
}

_atom_count_is_7() {
    local atoms n
    atoms="$(parse_atoms_from_plan)" || return 1
    n=0
    [ -n "$atoms" ] && n="$(printf '%s\n' "$atoms" | wc -l | tr -d '[:space:]')"
    [ "$n" -eq 7 ]
}

# _resolve_atoms_ok [atom-list] — 0 iff <atom-list> (default:
# parse_atoms_from_plan) is non-empty AND every atom resolves to a real
# crates/<pkg>/tests/<bin>.rs file on disk. Accepts an explicit override so
# the non-vacuity self-check can feed a synthetic dangling atom through the
# SAME check. Assumes package name == crates/ directory name (see the
# caveat note in tests/infra/test_heavy_filter_atoms.sh Assertion E).
_resolve_atoms_ok() {
    local atoms
    if [ "$#" -eq 0 ]; then
        atoms="$(parse_atoms_from_plan)" || return 1
    else
        atoms="$1"
    fi
    [ -n "$atoms" ] || return 1
    local _atom_re='^package\(([a-z0-9_-]+)\) & binary\(([a-z0-9_-]+)\)$'
    while IFS= read -r _atom; do
        [ -n "$_atom" ] || continue
        if [[ "$_atom" =~ $_atom_re ]]; then
            local _pkg="${BASH_REMATCH[1]}" _bin="${BASH_REMATCH[2]}"
            [ -f "$REPO_ROOT/crates/$_pkg/tests/$_bin.rs" ] || return 1
        else
            return 1
        fi
    done <<< "$atoms"
    return 0
}

# assert_guard_rejects — non-vacuity self-check: proves the guard's own
# resolve-to-disk / orphan / overlap / test-scope checks actually DETECT a
# deliberately broken partition (dangling atom / dropped atom / injected
# overlap / stripped test-scope clause), and still ACCEPT the real,
# unmodified partition (the guard is green on truth, not merely
# unconditionally red). Mirrors
# tests/infra/test_run_all_classification.sh's injected-drift self-check.
assert_guard_rejects() {
    local real_atoms first_atom remaining_atoms

    real_atoms="$(parse_atoms_from_plan)" || return 1
    [ -n "$real_atoms" ] || return 1
    first_atom="$(printf '%s\n' "$real_atoms" | head -n1)"
    [ -n "$first_atom" ] || return 1
    remaining_atoms="$(printf '%s\n' "$real_atoms" | tail -n +2)"

    # (1) dangling atom -- append a nonexistent binary to the real atom
    # list; _resolve_atoms_ok must REJECT it (a typo'd/dangling filter must
    # never be silently accepted).
    local dangling_atoms
    dangling_atoms="$(printf '%s\n%s\n' "$real_atoms" \
        'package(reify-solver-elastic) & binary(nonexistent_zzz)')"
    if _resolve_atoms_ok "$dangling_atoms"; then return 1; fi

    # (2) dropped atom -- an expression built from all-but-the-first real
    # atom; _no_orphan_ok must REJECT it (the dropped atom is missing).
    if _no_orphan_ok "$remaining_atoms"; then return 1; fi

    # (3) injected overlap -- fold solver_gate_smoke into the real heavy
    # expression; _no_overlap_ok must REJECT it.
    local overlap_expr="${REIFY_HEAVY_NEXTEST_FILTER} | (package(reify-solver-elastic) & binary(solver_gate_smoke))"
    if _no_overlap_ok "$overlap_expr"; then return 1; fi

    # (4) dropped test-scope clause -- the REAL emitted expression with every
    # ` & test(/^<stem>::/)` clause stripped out, i.e. exactly what a silent
    # widening of a test-scoped atom to its whole binary looks like on the
    # wire. _test_scope_clauses_survive_ok must REJECT it. This is the break
    # that motivated the check: checks (1)-(3) above, and every prefix-only
    # parser in this file, ACCEPT this same input unchanged -- the atoms still
    # count 7, still resolve to disk, still show no orphan and no overlap.
    local stripped_expr
    stripped_expr="$(offline_plan | grep -v '^#' | sed 's/ & test(\/\^[a-z0-9_]*::\/)//g')" || return 1
    [ -n "$stripped_expr" ] || return 1
    # Self-check the fixture itself: the strip must actually have removed
    # something, or (4) would prove nothing.
    if printf '%s\n' "$stripped_expr" | grep -qE '& test\(/\^[a-z0-9_]+::/\)'; then return 1; fi
    if _test_scope_clauses_survive_ok "$stripped_expr"; then return 1; fi
    # ...and the prefix-only checks really are blind to that same break, which
    # is what makes the new check load-bearing rather than redundant.
    _resolve_atoms_ok "$(parse_atoms_from_plan "$stripped_expr")" || return 1

    # (5) sanity -- the REAL, unmodified partition must still be ACCEPTED by
    # all four checks (default args -- actual offline plan / actual lib).
    _resolve_atoms_ok || return 1
    _no_orphan_ok || return 1
    _no_overlap_ok || return 1
    _test_scope_clauses_survive_ok || return 1

    return 0
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

# ---------------------------------------------------------------------------
# Assertion (a): offline plan shape (role/profile/idle-class/no-jobserver,
# positive heavy filter + --run-ignored all).
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion (a): offline plan shape ---"

assert "offline plan: header shows role=offline" \
    _offline_header_has "role=offline"

assert "offline plan: header shows profile=release" \
    _offline_header_has "profile=release"

assert "offline plan: at least 1 cargo command line (sanity)" \
    _offline_has_cargo_line

assert "offline plan: all cargo lines prefixed 'nice -n 19 ionice -c3 cargo' (idle class)" \
    _offline_all_cargo_lines_idle_class

assert "offline plan: has NO 'export CARGO_MAKEFLAGS=' line (off the merge jobserver)" \
    _offline_lacks 'export CARGO_MAKEFLAGS='

if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    assert 'offline plan: contains a positive heavy filter (-E "(")' \
        _offline_cmds_has '-E "('

    assert "offline plan: contains --run-ignored all" \
        _offline_cmds_has '--run-ignored all'
else
    assert 'offline plan, nextest unavailable: plan has NO -E "(" (cargo-test fallback has no -E support)' \
        _offline_lacks '-E "('
fi

# ---------------------------------------------------------------------------
# Assertion (d): heavy (+) smoke partition -- no overlap, no orphan.
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion (d): heavy (+) smoke partition -- no overlap, no orphan ---"

# The orphan check parses the emitted -E expression, which only exists on the
# nextest plan -- the cargo-test fallback (nextest=0, a host without
# cargo-nextest installed) has no -E support at all, so the property is
# genuinely nextest-only and cannot be recovered by widening a grep (task
# 5599). Guarded with the same NEXTEST_AVAILABLE idiom already used for
# assertions (b) and (c-bis) above; guarded by
# tests/infra/test_verify_nextest_absent_suites.sh.
if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    assert "offline plan: no orphan -- every heavy atom is present in the emitted -E expression" \
        _no_orphan_ok
else
    assert 'offline plan, nextest unavailable: no -E expression exists to orphan-check (cargo-test fallback has no -E support)' \
        _offline_lacks '-E "('
fi

# NOT guarded: reads the REIFY_HEAVY_NEXTEST_FILTER env manifest, not the
# emitted plan, so it holds on both hosts.
assert "REIFY_HEAVY_NEXTEST_FILTER has no overlap with solver_gate_smoke" \
    _no_overlap_ok

assert "crates/reify-solver-elastic/tests/solver_gate_smoke.rs exists on disk (real, distinct, lighter gate-smoke binary)" \
    test -f "$REPO_ROOT/crates/reify-solver-elastic/tests/solver_gate_smoke.rs"

# ---------------------------------------------------------------------------
# Assertion (e): resolve-to-disk -- every atom parsed from the ACTUAL
# emitted offline -E expression maps to a real test file, and the parsed
# count is exactly 7 (task 6368 added the 7th, test-scoped atom). Both are
# BINARY-MEMBERSHIP checks (prefix-only parsers, see the header note), so a
# third check covers the part they structurally cannot: that the lib's
# ` & test(...)` clauses survive verbatim into the emitted expression.
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion (e): resolve-to-disk -- ACTUAL emitted offline plan atoms ---"

# All three parse the ACTUAL emitted offline -E expression -- nextest-only,
# same reasoning as assertion (d)'s orphan check above (task 5599).
if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    assert "offline plan atoms: exactly 7 parsed (no silent membership drift)" \
        _atom_count_is_7

    assert "offline plan atoms: every parsed atom resolves to a real crates/<pkg>/tests/<bin>.rs file" \
        _resolve_atoms_ok

    assert "offline plan: >=1 test-scope clause in the lib, and every one survives verbatim into the emitted -E expression (a dropped clause widens its atom to the whole binary; the prefix-only parsers above cannot see that)" \
        _test_scope_clauses_survive_ok
else
    assert 'offline plan atoms, nextest unavailable: no -E expression exists to parse atoms from (cargo-test fallback has no -E support)' \
        _offline_lacks '-E "('
fi

# ---------------------------------------------------------------------------
# Non-vacuity self-check: the guard's own resolve-to-disk / orphan / overlap
# checks must REJECT a deliberately-broken partition, not just pass
# vacuously on the current (correct) one.
# ---------------------------------------------------------------------------
echo ""
echo "--- Non-vacuity self-check: guard detects an injected partition break ---"

# Nextest-only: assert_guard_rejects mutates and re-checks the emitted -E
# expression, which does not exist on the cargo-test fallback (task 5599).
if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    assert "guard checks reject a deliberately-broken partition (dangling atom / dropped atom / injected overlap / stripped test-scope clause), and still accept the real one" \
        assert_guard_rejects
else
    echo "  (skipped: nextest unavailable -- no -E expression to break and re-check)"
fi

# ---------------------------------------------------------------------------
# Dump self-check: a forced oracle miss emits the full raw plan (header incl
# nextest=) + driver rc to stderr (esc-4959-57/esc-4959-56). Mirrors the
# assert_guard_rejects non-vacuity idiom above, but pins the Part-2
# evidence-dump behavior instead of resolve-to-disk/orphan/overlap. RED on
# base: the checkers currently print nothing on a miss.
# ---------------------------------------------------------------------------
echo ""
echo "--- Dump self-check: a forced oracle miss emits the full raw plan (header incl nextest=) + driver rc to stderr ---"

_MISS_NEEDLE='NEEDLE_THAT_CANNOT_APPEAR_ZZZ'

# _probe_gate_miss_dumps -- forces a GUARANTEED _gate_has needle miss (this
# needle can never appear in a real plan) and checks the stderr-only output
# contains the plan-dump marker, the nextest= header token (host-agnostic --
# the header always carries nextest=0/1), and a driver rc= field.
_probe_gate_miss_dumps() {
    local err
    if err="$(_gate_has task 1 "$_MISS_NEEDLE" 2>&1 1>/dev/null)"; then
        return 1
    fi
    case "$err" in *PLAN-DUMP*) ;; *) return 1 ;; esac
    case "$err" in *"nextest="*) ;; *) return 1 ;; esac
    case "$err" in *"rc="*) ;; *) return 1 ;; esac
    return 0
}

# _probe_offline_miss_dumps -- same, driving an _offline_* checker instead
# of the _gate_* family.
_probe_offline_miss_dumps() {
    local err
    if err="$(_offline_cmds_has "$_MISS_NEEDLE" 2>&1 1>/dev/null)"; then
        return 1
    fi
    case "$err" in *PLAN-DUMP*) ;; *) return 1 ;; esac
    case "$err" in *"nextest="*) ;; *) return 1 ;; esac
    case "$err" in *"rc="*) ;; *) return 1 ;; esac
    return 0
}

assert "gate oracle miss dumps full raw plan (header incl nextest=) + rc to stderr" \
    _probe_gate_miss_dumps
assert "offline oracle miss dumps full raw plan (header incl nextest=) + rc to stderr" \
    _probe_offline_miss_dumps

test_summary
