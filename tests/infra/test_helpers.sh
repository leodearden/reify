#!/usr/bin/env bash
# Shared test helpers for reify shell test files.
# Provides assert() and test_summary() with PASS/FAIL counters.
#
# Usage:  source "$(dirname "${BASH_SOURCE[0]}")/test_helpers.sh"
#   or:   source "$REPO_ROOT/tests/infra/test_helpers.sh"
#
# Note: tests/infra/test_tree_sitter_pipeline.sh intentionally uses its own richer
# assert API (assert_cmd_success/assert_cmd_fails with output capture to temp
# files, PASS_COUNT/FAIL_COUNT, colored terminal output, test auto-discovery
# via declare -F, trap-based cleanup arrays) and is excluded from this shared
# module. The pipeline's needs are architecturally different from the simple
# boolean assert pattern provided here.

# Source guard — prevent double-sourcing.
if [ "${_REIFY_TEST_HELPERS_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_TEST_HELPERS_SH_SOURCED=1

PASS=0
FAIL=0

assert() {
    local desc="$1"
    shift
    # Per-assert tmpfile redirect (no-subshell tmpfile idiom, esc-4959-57):
    # "$@" runs directly in this shell (redirect only) rather than via a
    # command-substitution subshell (`out="$("$@" 2>&1)"`), because some
    # asserted checker functions mutate parent-shell globals (e.g. the
    # offline suite's _OFFLINE_PLAN_CACHE memoization) and a subshell would
    # silently discard that mutation.
    local _f
    # Guard mktemp failure (e.g. TMPDIR unwritable/full): if _f ends up empty,
    # `"$@" >"$_f" 2>&1` would be an ambiguous/empty redirect that errors
    # before the checker even runs, spuriously FAILing every assert in the
    # suite rather than reporting the real condition. Fall back to the
    # pre-esc-4959-57 `/dev/null` redirect in that case — no captured-output
    # dump is possible, but the checker's actual PASS/FAIL result is preserved.
    _f="$(mktemp "${TMPDIR:-/tmp}/reify-assert.XXXXXX")" || _f=""
    local _target="${_f:-/dev/null}"
    if "$@" >"$_target" 2>&1; then
        echo "  PASS: $desc"
        PASS=$((PASS + 1))
    else
        echo "  FAIL: $desc"
        FAIL=$((FAIL + 1))
        # Dump captured evidence ONLY on FAIL, after the byte-identical
        # "  FAIL: $desc" line, so an all-green suite stays byte-for-byte
        # unchanged while a failing assert preserves the discarded
        # stdout/stderr in the archived verify log (esc-4959-57/esc-4959-56).
        if [ -n "$_f" ] && [ -s "$_f" ]; then
            echo "  ---- assert: captured output (tail -50) ----"
            tail -n 50 "$_f" | sed 's/^/  | /'
            echo "  ---- assert: end captured output ----"
        fi
    fi
    [ -n "$_f" ] && rm -f "$_f"
}

test_summary() {
    echo ""
    echo "Results: $PASS passed, $FAIL failed"
    if [ "$FAIL" -gt 0 ]; then
        exit 1
    fi
}

# ─────────────────────────────────────────────────────────────────────────────
# Warm-lane test isolation (promoted from test_seed_warm_lane.sh Block R,
# tasks 5590/5612)
#
# Opt-in facility for the warm-lane suites (test_seed_warm_lane.sh and its six
# siblings): mint lane fixtures whose dirname(LANE_DIR) is run-private.
#
# WHY it exists: scripts/seed-warm-lane.sh computes RESEED_TRASH_DIR as
# dirname(LANE_DIR)/.reseed-trash and renames a non-empty <lane>/target there
# before re-seeding. A lane created bare under /tmp therefore makes that path
# the machine-shared /tmp/.reseed-trash — shared across every concurrent
# agent/test run on the host. Nesting each lane under its own private parent
# makes dirname(LANE_DIR) unique per lane, so the computed trash dir is
# run-private. (Task 5384 introduced the pattern for individual fixtures; 5590
# factored it out within one suite; 5612 promoted it here.)
#
# SOURCE-TIME FOOTPRINT IS DELIBERATELY INERT. 153 files in this tree source
# test_helpers.sh and only seven use this facility, so nothing below runs at
# source time except these scalar defaults — no mktemp, no state file, no trap.
# init_isolated_lane_root is strictly opt-in and never runs implicitly.
#
# NO EXIT TRAP IS INSTALLED HERE, on purpose: bash EXIT traps do not stack, so
# a `trap` set in this library would clobber (or be clobbered by) each suite's
# own `trap cleanup EXIT` — silently disabling either this cleanup or the
# suite's, including pool's guarded `sudo umount`s and thin/gc's _BGPIDS kill
# loop. Instead init_isolated_lane_root appends its root to the caller's
# existing _TMPDIRS array, the one integration point that composes with every
# existing cleanup() body unmodified.
# ─────────────────────────────────────────────────────────────────────────────

# Inert scalar defaults. _SHARED_TRASH_DIR is the machine-shared path this whole
# facility exists to keep clean; it is a plain variable rather than a constant so
# a positive-control test can redirect it to a run-private trash dir and prove
# the detectors below fire without ever littering the real path. _LANE_ROOT and
# _TRASH_HITS_FILE are set-but-empty so `set -u` consumers can read them before
# init_isolated_lane_root has run.
_SHARED_TRASH_DIR="/tmp/.reseed-trash"
_LANE_ROOT=""
_TRASH_HITS_FILE=""
_SHARED_TRASH_SNAPSHOT=""
_LANE_LITTER_PREFIX=""

# init_isolated_lane_root <stem> — MAIN-SHELL ONLY. Mints the single per-run
# grandparent for every lane this suite creates, and registers it in the
# caller's _TMPDIRS so the caller's own cleanup()/`trap cleanup EXIT` reclaims
# it. Call once, immediately AFTER `trap cleanup EXIT`.
#
# <stem> names the root (and, via it, everything nested inside), so any litter a
# suite does produce stays attributable to that suite by mktemp prefix.
#
# The _TMPDIRS-is-declared check is not decoration: a call placed BEFORE the
# suite's own `_TMPDIRS=()` would register into an array that assignment then
# wipes, leaking the root — and every lane under it — for the whole run. Failing
# loudly turns that ordering mistake into an error instead of a silent leak.
init_isolated_lane_root() {
    local stem="${1:-}"
    if [ -z "$stem" ]; then
        echo "ERROR: init_isolated_lane_root requires a <stem> argument" >&2
        return 1
    fi
    if ! declare -p _TMPDIRS >/dev/null 2>&1; then
        echo "ERROR: init_isolated_lane_root: _TMPDIRS is not declared yet." >&2
        echo "       Call it AFTER the suite's '_TMPDIRS=()' and 'trap cleanup EXIT'," >&2
        echo "       otherwise a later _TMPDIRS=() wipes the registration and leaks the root." >&2
        return 1
    fi
    _LANE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/${stem}-lane-root-XXXXXX")" || return 1
    _TMPDIRS+=("$_LANE_ROOT")
    # Detector state (see the shared-trash detector section below). Nested
    # directly under $_LANE_ROOT — a SIBLING of each lane's own private parent,
    # never inside one — so the caller's existing `rm -rf "$_LANE_ROOT"`
    # reclaims it with no new _TMPDIRS entry and no extra top-level temp entry,
    # and a "the lane's parent contains the lane and nothing else" structural
    # check stays valid.
    _TRASH_HITS_FILE="$_LANE_ROOT/.shared-trash-hits"
    : > "$_TRASH_HITS_FILE" || return 1
    # Litter-guard state (see the litter guard section below): remember the
    # caller's stem, and snapshot the shared trash dir's entry set as it stands
    # RIGHT NOW, so the end-of-suite guard can tell entries this run produced
    # from ones that were already there. An absent trash dir snapshots as empty
    # — that is the normal case, not an error.
    _LANE_LITTER_PREFIX="$stem"
    _SHARED_TRASH_SNAPSHOT="$_LANE_ROOT/.shared-trash-snapshot"
    _list_trash_entries "$_SHARED_TRASH_DIR" > "$_SHARED_TRASH_SNAPSHOT" || return 1
    return 0
}

# make_isolated_lane <prefix> — mktemps a private parent under $_LANE_ROOT and a
# lane dir nested inside it, then echoes the lane path on stdout.
#
# SUBSHELL-SAFE BY CONSTRUCTION: call sites read `X_LANE="$(make_isolated_lane
# p)"`, so this body runs in a command-substitution SUBSHELL. Any
# `_TMPDIRS+=(...)` performed here would be silently discarded once that
# subshell exits, leaking every private parent. That is why registration lives
# in init_isolated_lane_root instead and this function appends to NOTHING:
# cleanup is anchored on the ONE root, which the caller's EXIT trap `rm -rf`s —
# reclaiming every lane, its sibling ${lane}.lock/.ready-marker/.done-marker
# files, and its private .reseed-trash, all in one shot.
make_isolated_lane() {
    local prefix="${1:-}" parent
    if [ -z "${_LANE_ROOT:-}" ]; then
        echo "ERROR: make_isolated_lane: _LANE_ROOT is empty — call init_isolated_lane_root <stem> first" >&2
        return 1
    fi
    if [ -z "$prefix" ]; then
        echo "ERROR: make_isolated_lane requires a <prefix> argument" >&2
        return 1
    fi
    parent="$(mktemp -d "$_LANE_ROOT/${prefix}-XXXXXX")" || return 1
    mktemp -d "$parent/lane-XXXXXX"
}

# ─────────────────────────────────────────────────────────────────────────────
# Shared-trash runtime detector
#
# _note_shared_trash_use is the RECORDER: a warm-lane suite calls it from inside
# its run_helper wrapper after every seed invocation, with the invocation's
# captured stderr in $ERR_OUT. scripts/seed-warm-lane.sh logs its
# rename-into-trash to stderr, so an $ERR_OUT mentioning $_SHARED_TRASH_DIR is
# exact evidence that THIS invocation renamed into that path.
# _assert_no_shared_trash_use is the matching CHECKER, asserted once per suite.
#
# THREE INVARIANTS BELOW ARE LOAD-BEARING:
#
# 1. The trailing `return 0` is MANDATORY. The recorder is called as a bare
#    unguarded statement inside run_helper/run_helper_real, so under
#    `set -euo pipefail` any nonzero return here would abort the ENTIRE suite
#    instead of just failing one assert.
#
# 2. State lives in an append-only FILE ($_TRASH_HITS_FILE), not a bash array.
#    Some run_helper call sites invoke the helper inside a backgrounded
#    ( ... ) & subshell, and a bash array append made there is discarded when
#    the subshell exits — silently blinding the detector on exactly the
#    --fresh-checkout-against-non-empty-target runs most likely to reach seed's
#    rename-into-trash path. A `>>` append performed inside a subshell IS
#    visible to the parent shell, so file-backed state fixes this without
#    changing the call convention at any fixture site. A single-line `printf`
#    append to an O_APPEND file is atomic well below PIPE_BUF (4096), so a
#    background job appending while the main shell asserts cannot corrupt a
#    record.
#
# 3. The variable is QUOTED inside the case pattern (*"$_SHARED_TRASH_DIR"*) so
#    any glob metacharacter in the path is matched literally rather than
#    interpreted as a wildcard — an unquoted pattern would fire on paths seed
#    never touched.
#
# _SHARED_TRASH_DIR stays a plain overridable variable (not a readonly) so a
# suite's positive control can redirect it at an isolated lane's own private
# trash dir and prove the detector fires without littering the real shared path.
# When restoring after such an override, restore FROM A SAVED COPY rather than
# re-typing the literal default: a re-typed literal would silently revert a
# future change to the default above instead of tracking it.
#
# ${ERR_OUT:-} rather than bare $ERR_OUT: this library is sourced by 153 files,
# most of which have no run_helper wrapper and never set ERR_OUT at all, and
# reading an unset variable under `set -u` would abort them.
# ─────────────────────────────────────────────────────────────────────────────

_note_shared_trash_use() {
    case "${ERR_OUT:-}" in
        *"$_SHARED_TRASH_DIR"*) printf '%s\n' "$*" >> "$_TRASH_HITS_FILE" ;;
    esac
    return 0
}

_assert_no_shared_trash_use() {
    [ ! -s "$_TRASH_HITS_FILE" ] && return 0
    printf 'seed invocation wrote into machine-shared %s:\n' "$_SHARED_TRASH_DIR"
    cat "$_TRASH_HITS_FILE"
    return 1
}

# ─────────────────────────────────────────────────────────────────────────────
# Shared-trash LITTER guard
#
# The ERR_OUT recorder above only fires when a suite's run_helper wrapper
# captured seed's stderr into $ERR_OUT. Most suites have no such wrapper — some
# never invoke the real seed at all, driving stub scripts instead — so
# _assert_no_shared_trash_use would read a permanently-empty hits file and pass
# VACUOUSLY in them forever. This guard instead observes the FILESYSTEM: it
# diffs $_SHARED_TRASH_DIR's entry set against a snapshot taken at
# init_isolated_lane_root time, so it sees a leak regardless of how seed was
# invoked or whether its stderr was captured.
#
# ATTRIBUTION IS BY THE SUITE'S OWN mktemp STEM — a deliberate trade-off.
# $_SHARED_TRASH_DIR is machine-shared, so a bare snapshot-diff would fail this
# suite for a concurrent worktree's litter. seed names each trash entry
# "<lane-basename>.<pid>", and every lane this library mints is named from the
# suite's stem, so a prefix test on the entry basename attributes litter
# race-free — the same method that attributed the pre-fix entries forensically.
# The cost: a hypothetical bare-/tmp lane named OUTSIDE its own suite's naming
# convention is not caught. New entries that do not match the stem are therefore
# emitted as an informational line and never fail the suite.
# ─────────────────────────────────────────────────────────────────────────────

# _list_trash_entries <dir> — <dir>'s entry set, one basename per line, sorted.
# An absent or unreadable dir emits nothing and is NOT an error: most runs never
# create the trash dir at all. Used by BOTH the snapshot and the comparison, so
# the two formats can never drift apart.
_list_trash_entries() {
    local dir="$1"
    [ -d "$dir" ] || return 0
    ls -A "$dir" 2>/dev/null | sort
    return 0
}

# _assert_no_shared_trash_litter <trash-dir> <snapshot-file> <stem>
#
# Parameterized so the globals wrapper below, a suite's own positive control,
# and the hermetic liveness control all share ONE implementation — there is no
# second copy that could rot out of step with this one.
_assert_no_shared_trash_litter() {
    local trash_dir="$1" snapshot="$2" stem="$3"
    local entry
    local offenders=() unattributed=()

    if [ -z "$stem" ]; then
        printf 'ERROR: _assert_no_shared_trash_litter: empty <stem> would match every entry\n' >&2
        return 1
    fi

    while IFS= read -r entry; do
        [ -n "$entry" ] || continue
        # New since the snapshot?  (grep -Fx: literal, whole-line.)
        if [ -f "$snapshot" ] && grep -Fxq -- "$entry" "$snapshot"; then
            continue
        fi
        # seed names its trash entries "<lane-basename>.<pid>", so the stem test
        # is a PREFIX test on the basename — the entry is not a bare name and
        # does carry a dotted suffix.
        case "$entry" in
            "$stem"*) offenders+=("$entry") ;;
            *)        unattributed+=("$entry") ;;
        esac
    done < <(_list_trash_entries "$trash_dir")

    # Informational only, never a failure: another worktree writing to a
    # machine-shared path must not flake this suite.
    if [ "${#unattributed[@]}" -ne 0 ]; then
        printf 'note: %s gained entries not attributable to stem %s (other worktrees; not a failure): %s\n' \
            "$trash_dir" "$stem" "${unattributed[*]}"
    fi

    [ "${#offenders[@]}" -eq 0 ] && return 0
    printf 'a lane with stem %s littered the machine-shared %s: %s\n' \
        "$stem" "$trash_dir" "${offenders[*]}"
    return 1
}

# assert_no_shared_trash_litter — the globals form a suite asserts at the end of
# its run. FAILS LOUDLY on uninitialized state rather than passing vacuously: a
# suite that forgot to call init_isolated_lane_root would otherwise report a
# false all-clear forever, which is exactly the dead instrument this guard
# exists to prevent.
assert_no_shared_trash_litter() {
    if [ -z "${_SHARED_TRASH_SNAPSHOT:-}" ] || [ -z "${_LANE_LITTER_PREFIX:-}" ]; then
        echo "ERROR: assert_no_shared_trash_litter: no snapshot/stem recorded." >&2
        echo "       Call init_isolated_lane_root <stem> once, after 'trap cleanup EXIT'," >&2
        echo "       otherwise this guard would pass vacuously and observe nothing." >&2
        return 1
    fi
    _assert_no_shared_trash_litter \
        "$_SHARED_TRASH_DIR" "$_SHARED_TRASH_SNAPSHOT" "$_LANE_LITTER_PREFIX"
}

# assert_shared_trash_litter_detector_live — per-suite liveness control. Proves
# the checker actually FIRES on a synthetic offender and then passes once it is
# gone, so the guard above cannot silently become a dead instrument.
#
# Fully hermetic: it mktemps its own scratch trash dir and NEVER reads or writes
# $_SHARED_TRASH_DIR. A control that proved itself by littering the very path it
# defends would be self-defeating.
assert_shared_trash_litter_detector_live() {
    local dir snap stem entry out rc=0
    stem="selftest-stem"
    dir="$(mktemp -d "${TMPDIR:-/tmp}/litter-selftest-XXXXXX")" || return 1
    snap="$dir.snapshot"
    entry="$stem-lane-XXXX.4242"

    _list_trash_entries "$dir" > "$snap" || { rm -rf "$dir" "$snap"; return 1; }

    # Must FAIL, and must name the synthetic offender.
    mkdir -p "$dir/$entry" || { rm -rf "$dir" "$snap"; return 1; }
    if out="$(_assert_no_shared_trash_litter "$dir" "$snap" "$stem" 2>&1)"; then
        echo "ERROR: litter detector is DEAD — it passed a trash dir holding $entry" >&2
        rc=1
    else
        case "$out" in
            *"$entry"*) ;;
            *)  echo "ERROR: litter detector fired but its message did not name $entry: $out" >&2
                rc=1 ;;
        esac
    fi

    # ... and must PASS once the offender is gone, so it is a detector and not a
    # constant failure.
    rm -rf "${dir:?}/$entry"
    if ! _assert_no_shared_trash_litter "$dir" "$snap" "$stem" >/dev/null 2>&1; then
        echo "ERROR: litter detector failed a clean trash dir (false positive)" >&2
        rc=1
    fi

    rm -rf "$dir" "$snap"
    return "$rc"
}
