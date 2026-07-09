#!/usr/bin/env bash
# tests/infra/test_run_all_pool_lock_host_global.sh
#
# Infrastructure test for task 5131 (PRD docs/prds/run-all-pool-contention-
# tiering-fix.md §9, leaf L3a — "Host-global-lock drift-guard"), TIGHTENED by
# task 5145 to require TMPDIR-independence.
#
# CONTRACT UNDER TEST (INV-1 / PRD §8 non-goal, line 170; tightened by task
# 5145): run_all.sh's H2 concurrent-pool semaphore lock (run_all.sh:605) must
# always resolve to a FIXED host-global per-uid path —
# /tmp/reify-run-all-pool-$(id -u).lock by default — regardless of TMPDIR.
# NEVER a worktree/per-lane-scoped path, and never keyed off an ambient
# TMPDIR either. Scoping the lock to a worktree/lane would invert the
# host-global concurrency cap this semaphore exists to enforce — M concurrent
# lanes x N pool workers each — melting the host (PRD §8: "the escalations'
# own repeated suggestion", explicitly rejected). Keying the default off
# ${TMPDIR:-/tmp} is the same failure by a different door: any lane/agent/
# harness that exports a private TMPDIR (a common isolation pattern the
# warm-lane orchestrator uses per-worktree) silently forks the lock
# namespace, so two verifies each believe they hold the only slot.
#
# RED-ON-ARRIVAL (task 5145): task 5131's original framing treated per-lane
# TMPDIR provisioning as "a separate DF-owned concern" and asserted the
# resolved lock only against a controlled-but-still-TMPDIR-keyed path
# ($SHARED_TMP/reify-run-all-pool-$(id -u).lock). Task 5145 supersedes that
# stance: TMPDIR-keying the host-global lock IS the bug. This test now pins
# the FIXED /tmp literal as the only correct default (Assertion 2) and adds a
# same-Assertion-3 check that the resolved lock never contains $SHARED_TMP
# (the injected TMPDIR) — proving TMPDIR-independence, not just per-lane-path
# independence. RED until run_all.sh:605 stops consulting TMPDIR.
#
# APPROACH (behavioral, not source-grep): drives the REAL run_all.sh against
# an empty `mktemp -d` INFRA_DIR (an optional positional arg, run_all.sh:189)
# so discovery yields zero test_*.sh (fast, hermetic — no test ever
# executes, and the semaphore is never actually acquired), while the
# `INFO: run_all.sh pool: N=... lock=...` stderr line (run_all.sh:637) still
# emits, because it is printed BEFORE discovery. Parsing that line's resolved
# `lock=` value is the behavioral oracle — it exercises run_all.sh:605's
# actual runtime resolution (catching drift in the expression itself, not
# just a hardcoded literal), rather than pattern-matching run_all.sh's source
# text.
#
# Each capture clears ambient REIFY_RUN_ALL_POOL_LOCK / _POOL_DISABLE and
# pins TMPDIR to a test-controlled neutral directory ($SHARED_TMP) inside the
# capture subshell (ambient-isolation lesson, task 4961 / esc-4906-45 /
# test_run_all_ambient_isolation.sh: a bare per-command prefix assignment
# does not clear an inherited EXPORTED var, so this uses `env -u ...`
# instead). Pinning TMPDIR to a SHARED-but-not-/tmp-literal path is what makes
# Assertion 2 a genuine post-5145 TMPDIR-independence check: if run_all.sh's
# default ever regresses to consulting TMPDIR, the resolved lock would embed
# $SHARED_TMP instead of the fixed /tmp literal, and both Assertion 2 and the
# new $SHARED_TMP-exclusion check in Assertion 3 catch it.
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
#   run_all.sh:605's true default (or the given override) independent of any
#   orchestrator ambient state. Extracts the resolved lock from the
#   "INFO: run_all.sh pool: N=... lock=..." stderr line (run_all.sh:637),
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

EXPECTED_DEFAULT_LOCK="/tmp/reify-run-all-pool-$(id -u).lock"

echo ""
echo "--- Assertion 1: NON-VACUITY GATE (pool path actually taken, not vacuously skipped) ---"
assert "capture A emitted a non-empty INFO ... lock= line (got: '$LOCK_A')" \
    test -n "$LOCK_A"
assert "capture B emitted a non-empty INFO ... lock= line (got: '$LOCK_B')" \
    test -n "$LOCK_B"

echo ""
echo "--- Assertion 2 (G2, tightened by task 5145): GREEN-ON-DEFAULT — resolved lock == FIXED /tmp per-uid host path, independent of TMPDIR ---"
assert "capture A default lock ('$LOCK_A') == /tmp/reify-run-all-pool-\$(id -u).lock ('$EXPECTED_DEFAULT_LOCK')" \
    test "$LOCK_A" = "$EXPECTED_DEFAULT_LOCK"
assert "capture B default lock ('$LOCK_B') == /tmp/reify-run-all-pool-\$(id -u).lock ('$EXPECTED_DEFAULT_LOCK')" \
    test "$LOCK_B" = "$EXPECTED_DEFAULT_LOCK"

echo ""
echo "--- Assertion 3: NOT PER-LANE, NOT TMPDIR-KEYED — default lock embeds neither REPO_ROOT, nor either CWD, nor either empty INFRA_DIR, nor the injected TMPDIR ---"
assert_lock_excludes "capture A" "$LOCK_A" "REPO_ROOT" "$REPO_ROOT"
assert_lock_excludes "capture A" "$LOCK_A" "CWD_A"     "$CWD_A"
assert_lock_excludes "capture A" "$LOCK_A" "CWD_B"     "$CWD_B"
assert_lock_excludes "capture A" "$LOCK_A" "INFRA_A"   "$INFRA_A"
assert_lock_excludes "capture A" "$LOCK_A" "INFRA_B"   "$INFRA_B"
assert_lock_excludes "capture A" "$LOCK_A" "SHARED_TMP (TMPDIR)" "$SHARED_TMP"
assert_lock_excludes "capture B" "$LOCK_B" "REPO_ROOT" "$REPO_ROOT"
assert_lock_excludes "capture B" "$LOCK_B" "CWD_A"     "$CWD_A"
assert_lock_excludes "capture B" "$LOCK_B" "CWD_B"     "$CWD_B"
assert_lock_excludes "capture B" "$LOCK_B" "INFRA_A"   "$INFRA_A"
assert_lock_excludes "capture B" "$LOCK_B" "INFRA_B"   "$INFRA_B"
assert_lock_excludes "capture B" "$LOCK_B" "SHARED_TMP (TMPDIR)" "$SHARED_TMP"

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
