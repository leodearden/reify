#!/usr/bin/env bash
# tests/infra/test_run_all_pool_lock_host_global.sh
#
# Infrastructure test for task 5131 (PRD docs/prds/run-all-pool-contention-
# tiering-fix.md §9, leaf L3a — "Host-global-lock drift-guard").
#
# CONTRACT UNDER TEST (INV-1 / PRD §8 non-goal, line 170): run_all.sh's H2
# concurrent-pool semaphore lock (run_all.sh:347) must always resolve to a
# HOST-GLOBAL per-uid path (${TMPDIR:-/tmp}/reify-run-all-pool-$(id -u).lock
# by default), NEVER a worktree/per-lane-scoped path. Scoping the lock to a
# worktree/lane would invert the host-global concurrency cap this semaphore
# exists to enforce — M concurrent lanes x N pool workers each — melting the
# host (PRD §8: "the escalations' own repeated suggestion", explicitly
# rejected).
#
# GREEN-ON-ARRIVAL: this is a pure regression/drift guard. The guarded
# behavior (run_all.sh:347's host-global-per-uid default) already exists on
# main (task 4924/H2) — there is no accompanying source fix in this task.
# This guard makes the PRD's G2 signal ("RED if the resolved pool lock base
# is not host-global ... green on the default per-uid host path") an
# EXECUTABLE test rather than a documented premise.
#
# APPROACH (behavioral, not source-grep): drives the REAL run_all.sh against
# an empty `mktemp -d` INFRA_DIR (an optional positional arg, run_all.sh:108-
# 114) so discovery yields zero test_*.sh (fast, hermetic — no test ever
# executes, and the semaphore is never actually acquired), while the
# `INFO: run_all.sh pool: N=... lock=...` stderr line (run_all.sh:369) still
# emits, because it is printed BEFORE discovery. Parsing that line's resolved
# `lock=` value is the behavioral oracle — it exercises run_all.sh:347's
# actual runtime resolution (catching drift in the expression itself, not
# just a hardcoded literal), rather than pattern-matching run_all.sh's source
# text.
#
# Each capture clears ambient REIFY_RUN_ALL_POOL_LOCK / _POOL_DISABLE and
# pins TMPDIR to a test-controlled neutral directory inside the capture
# subshell (ambient-isolation lesson, task 4961 / esc-4906-45 /
# test_run_all_ambient_isolation.sh: a bare per-command prefix assignment
# does not clear an inherited EXPORTED var, so this uses `env -u ...`
# instead). This scopes the guard to run_all.sh:347's resolution contract,
# independent of the orchestrator's own (possibly per-lane) ambient TMPDIR
# provisioning — that provisioning is a separate DF-owned concern (the PRD's
# L3b companion), not a reify-owned source guard.
#
# RECURSION NOTE (mirrors test_run_all_clock_marker_sanitize.sh /
# test_run_all_ambient_isolation.sh): every invocation below points
# run_all.sh at a synthetic EMPTY temp INFRA_DIR, never at the real
# tests/infra/ directory this file itself lives in — so this test can never
# recursively discover and re-run itself.
#
# Deliberately DEFERRED (PRD §10 Q3, out of scope for this task): the
# optional merge single-flight flock around run_all.sh's own invocation.
# This guard covers ONLY the host-global-lock invariant, not merge
# single-flight (see the PRD's L3a decomposition note).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
REAL_RUN_ALL="$REPO_ROOT/tests/infra/run_all.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

[ -f "$REAL_RUN_ALL" ] || { echo "ERROR: run_all.sh not found at $REAL_RUN_ALL"; exit 1; }

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

echo "=== run_all.sh pool-lock host-global drift-guard (task 5131 / PRD run-all-pool-contention-tiering-fix.md §9 L3a) ==="

# mk_tmp <outvar> — mktemp -d anchored at /tmp (NOT the ambient TMPDIR of
# the outer test-runner process, which the orchestrator may provision as a
# per-worktree/per-lane path), register the dir for the EXIT-trap cleanup,
# assign its path to <outvar>. Anchoring outside REPO_ROOT is required so
# that Assertion 3's REPO_ROOT-exclusion check can never spuriously FAIL
# just because ambient TMPDIR happens to be nested under the worktree
# (reviewer finding, task 5131 amendment) — SHARED_TMP feeds directly into
# the resolved lock value, so if it were repo-nested the lock would be too.
mk_tmp() {
    local _var="$1" _d
    _d="$(mktemp -d --tmpdir=/tmp reify-poollock.XXXXXX)"
    _TMPDIRS+=("$_d")
    printf -v "$_var" '%s' "$_d"
}

# _contains/_lacks <haystack> <needle> — fixed-string (grep -F) containment
# predicates on plain string values (not files), mirroring the
# _out_contains/_out_lacks idiom in test_run_all_clock_marker_sanitize.sh.
_contains() { grep -qF -- "$2" <<<"$1"; }
_lacks()    { ! grep -qF -- "$2" <<<"$1"; }

# capture_pool_lock <outvar> <cwd> <infra_dir> [override_lock]
#   Drives the REAL run_all.sh from <cwd> against the empty <infra_dir>
#   (fast, hermetic: discovery yields zero test_*.sh, so no test executes
#   and the semaphore is never acquired) under a controlled neutral TMPDIR,
#   clearing ambient REIFY_RUN_ALL_POOL_LOCK / REIFY_RUN_ALL_POOL_DISABLE
#   inside the capture subshell so the resolved lock reflects
#   run_all.sh:347's true default (or the given override) independent of any
#   orchestrator ambient state. Extracts the resolved lock from the
#   "INFO: run_all.sh pool: N=... lock=..." stderr line (run_all.sh:369),
#   which is emitted BEFORE discovery. Sets <outvar> to the resolved lock
#   path (empty string if the INFO line never appeared — the non-vacuity
#   gate below catches that).
capture_pool_lock() {
    local _var="$1" _cwd="$2" _infra="$3" _override="${4:-}"
    local _out _line _lock

    if [ -n "$_override" ]; then
        _out="$( cd "$_cwd" && env -u REIFY_RUN_ALL_POOL_LOCK -u REIFY_RUN_ALL_POOL_DISABLE \
            TMPDIR="$SHARED_TMP" REIFY_RUN_ALL_POOL_LOCK="$_override" \
            bash "$REAL_RUN_ALL" "$_infra" 2>&1 )" || true
    else
        _out="$( cd "$_cwd" && env -u REIFY_RUN_ALL_POOL_LOCK -u REIFY_RUN_ALL_POOL_DISABLE \
            TMPDIR="$SHARED_TMP" \
            bash "$REAL_RUN_ALL" "$_infra" 2>&1 )" || true
    fi

    _line="$(printf '%s\n' "$_out" | grep 'lock=' | head -n1 || true)"
    _lock="${_line##*lock=}"
    printf -v "$_var" '%s' "$_lock"
}

# assert_lock_excludes <label> <lock_value> <needle_label> <needle_value>
assert_lock_excludes() {
    assert "$1 lock ('$2') does not contain $3 ('$4')" \
        _lacks "$2" "$4"
}

SHARED_TMP=""; mk_tmp SHARED_TMP
CWD_A=""; mk_tmp CWD_A
CWD_B=""; mk_tmp CWD_B
INFRA_A=""; mk_tmp INFRA_A
INFRA_B=""; mk_tmp INFRA_B

# -- Two default (no-override) captures from different CWDs + different
# empty INFRA_DIRs, same TMPDIR + uid --------------------------------------
LOCK_A=""
capture_pool_lock LOCK_A "$CWD_A" "$INFRA_A"

LOCK_B=""
capture_pool_lock LOCK_B "$CWD_B" "$INFRA_B"

EXPECTED_DEFAULT_LOCK="$SHARED_TMP/reify-run-all-pool-$(id -u).lock"

echo ""
echo "--- Assertion 1: NON-VACUITY GATE (pool path actually taken, not vacuously skipped) ---"
assert "capture A emitted a non-empty INFO ... lock= line (got: '$LOCK_A')" \
    test -n "$LOCK_A"
assert "capture B emitted a non-empty INFO ... lock= line (got: '$LOCK_B')" \
    test -n "$LOCK_B"

echo ""
echo "--- Assertion 2 (G2): GREEN-ON-DEFAULT — resolved lock == controlled-TMPDIR per-uid host path ---"
assert "capture A default lock ('$LOCK_A') == \$SHARED_TMP/reify-run-all-pool-\$(id -u).lock ('$EXPECTED_DEFAULT_LOCK')" \
    test "$LOCK_A" = "$EXPECTED_DEFAULT_LOCK"
assert "capture B default lock ('$LOCK_B') == \$SHARED_TMP/reify-run-all-pool-\$(id -u).lock ('$EXPECTED_DEFAULT_LOCK')" \
    test "$LOCK_B" = "$EXPECTED_DEFAULT_LOCK"

echo ""
echo "--- Assertion 3: NOT PER-LANE — default lock embeds neither REPO_ROOT, nor either CWD, nor either empty INFRA_DIR ---"
assert_lock_excludes "capture A" "$LOCK_A" "REPO_ROOT" "$REPO_ROOT"
assert_lock_excludes "capture A" "$LOCK_A" "CWD_A"     "$CWD_A"
assert_lock_excludes "capture A" "$LOCK_A" "CWD_B"     "$CWD_B"
assert_lock_excludes "capture A" "$LOCK_A" "INFRA_A"   "$INFRA_A"
assert_lock_excludes "capture A" "$LOCK_A" "INFRA_B"   "$INFRA_B"
assert_lock_excludes "capture B" "$LOCK_B" "REPO_ROOT" "$REPO_ROOT"
assert_lock_excludes "capture B" "$LOCK_B" "CWD_A"     "$CWD_A"
assert_lock_excludes "capture B" "$LOCK_B" "CWD_B"     "$CWD_B"
assert_lock_excludes "capture B" "$LOCK_B" "INFRA_A"   "$INFRA_A"
assert_lock_excludes "capture B" "$LOCK_B" "INFRA_B"   "$INFRA_B"

echo ""
echo "--- Assertion 4: WORKTREE INVARIANCE — two captures from different CWD+INFRA_DIR, same TMPDIR+uid, yield IDENTICAL locks ---"
assert "capture A ('$LOCK_A') and capture B ('$LOCK_B') resolve to the IDENTICAL lock (host-global, invariant across lanes/worktrees)" \
    test "$LOCK_A" = "$LOCK_B"

echo ""
echo "--- Assertion 5: NON-VACUITY CONTROL — forcing REIFY_RUN_ALL_POOL_LOCK to a per-lane path DOES surface under that lane's CWD (proves Assertion 3's predicate discriminates) ---"
OVERRIDE_LOCK="$CWD_A/reify-run-all-pool.lock"
LOCK_OVERRIDE=""
capture_pool_lock LOCK_OVERRIDE "$CWD_A" "$INFRA_A" "$OVERRIDE_LOCK"

assert "override capture emitted a non-empty INFO ... lock= line (got: '$LOCK_OVERRIDE')" \
    test -n "$LOCK_OVERRIDE"
assert "override capture's lock ('$LOCK_OVERRIDE') == the forced override path ('$OVERRIDE_LOCK')" \
    test "$LOCK_OVERRIDE" = "$OVERRIDE_LOCK"
assert "override capture's lock ('$LOCK_OVERRIDE') DOES contain CWD_A ('$CWD_A') — per-lane detector fires, proving Assertion 3 is non-vacuous" \
    _contains "$LOCK_OVERRIDE" "$CWD_A"

test_summary
