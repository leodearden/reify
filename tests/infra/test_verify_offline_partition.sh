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

# gate_plan <role> <knob-mode> — command lines only (header stripped) for a
# gate role (task/merge), via plan_for.
gate_plan() {
    plan_for "$1" "$2" | grep -v '^#'
}

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

# heavy_atoms — the 6 `package(X) & binary(Y)` atoms parsed directly out of
# REIFY_HEAVY_NEXTEST_FILTER (the lib source-of-truth, A1), one per line.
heavy_atoms() {
    printf '%s' "$REIFY_HEAVY_NEXTEST_FILTER" | grep -oE 'package\([a-z0-9_-]+\) & binary\([a-z0-9_-]+\)'
}

# parse_atoms_from_plan [text] — the `package(X) & binary(Y)` atoms parsed
# directly out of the ACTUAL emitted offline plan's command lines (default:
# offline_plan's command lines), or an explicit override (used by the
# non-vacuity self-check below), one per line. Reused from
# test_heavy_filter_atoms.sh's Assertion E parser, but sourced from the
# ACTUAL emitted -E expression rather than REIFY_HEAVY_NEXTEST_FILTER
# directly -- proving verify.sh's real output resolves to disk (assertion e),
# not merely the lib source-of-truth (which heavy_atoms() already covers).
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
    local plan
    plan="$(offline_plan)" || return 1
    printf '%s\n' "$plan" | grep '^# verify.sh plan' | grep -qF -- "$1"
}
_offline_cmds_has() {
    local plan
    plan="$(offline_plan)" || return 1
    printf '%s\n' "$plan" | grep -v '^#' | grep -qF -- "$1"
}
_offline_lacks() {
    local plan
    plan="$(offline_plan)" || return 1
    ! printf '%s\n' "$plan" | grep -qF -- "$1"
}
_offline_has_cargo_line() {
    local plan cmds n
    plan="$(offline_plan)" || return 1
    cmds="$(printf '%s\n' "$plan" | grep -v '^#')"
    n="$(printf '%s\n' "$cmds" | grep -cE '(^| )cargo ' || true)"
    [ "${n:-0}" -ge 1 ]
}
_offline_all_cargo_lines_idle_class() {
    local plan cmds
    plan="$(offline_plan)" || return 1
    cmds="$(printf '%s\n' "$plan" | grep -v '^#')"
    ! printf '%s\n' "$cmds" | grep -E '(^| )cargo ' | grep -vq 'nice -n 19 ionice -c3 cargo'
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

_atom_count_is_6() {
    local atoms n
    atoms="$(parse_atoms_from_plan)" || return 1
    n=0
    [ -n "$atoms" ] && n="$(printf '%s\n' "$atoms" | wc -l | tr -d '[:space:]')"
    [ "$n" -eq 6 ]
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
# resolve-to-disk / orphan / overlap checks actually DETECT a deliberately
# broken partition (dangling atom / dropped atom / injected overlap), and
# still ACCEPT the real, unmodified partition (the guard is green on truth,
# not merely unconditionally red). Mirrors
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

    # (4) sanity -- the REAL, unmodified partition must still be ACCEPTED by
    # all three checks (default args -- actual offline plan / actual lib).
    _resolve_atoms_ok || return 1
    _no_orphan_ok || return 1
    _no_overlap_ok || return 1

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

assert "offline plan: no orphan -- every heavy atom is present in the emitted -E expression" \
    _no_orphan_ok

assert "REIFY_HEAVY_NEXTEST_FILTER has no overlap with solver_gate_smoke" \
    _no_overlap_ok

assert "crates/reify-solver-elastic/tests/solver_gate_smoke.rs exists on disk (real, distinct, lighter gate-smoke binary)" \
    test -f "$REPO_ROOT/crates/reify-solver-elastic/tests/solver_gate_smoke.rs"

# ---------------------------------------------------------------------------
# Assertion (e): resolve-to-disk -- every atom parsed from the ACTUAL
# emitted offline -E expression maps to a real test file, and the parsed
# count is exactly 6 (no silent membership drift).
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertion (e): resolve-to-disk -- ACTUAL emitted offline plan atoms ---"

assert "offline plan atoms: exactly 6 parsed (no silent membership drift)" \
    _atom_count_is_6

assert "offline plan atoms: every parsed atom resolves to a real crates/<pkg>/tests/<bin>.rs file" \
    _resolve_atoms_ok

# ---------------------------------------------------------------------------
# Non-vacuity self-check: the guard's own resolve-to-disk / orphan / overlap
# checks must REJECT a deliberately-broken partition, not just pass
# vacuously on the current (correct) one.
# ---------------------------------------------------------------------------
echo ""
echo "--- Non-vacuity self-check: guard detects an injected partition break ---"

assert "guard checks reject a deliberately-broken partition (dangling atom / dropped atom / injected overlap), and still accept the real one" \
    assert_guard_rejects

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
