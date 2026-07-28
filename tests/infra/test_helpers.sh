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
