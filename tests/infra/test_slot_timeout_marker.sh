#!/usr/bin/env bash
# tests/infra/test_slot_timeout_marker.sh
#
# Guard for task 6024 (the esc-5848-2 / esc-5893-3 infra-hold class): a
# finite-WAIT slot_acquire deadline must be POSITIVELY identifiable in a verify
# log, so dark-factory classifies it as SEMAPHORE_TIMEOUT instead of guessing
# from a loose whole-output co-occurrence. DF's detector is already deployed and
# line-anchored (`^[ \t]*` + the sentinel, DF task 3679); this suite pins the
# reify-side emission contract that detector depends on.
#
# CONTRACT UNDER TEST (scripts/lib_slot_acquire.sh):
#   On the finite-WAIT rc=75 branch -- and ONLY there -- slot_acquire emits
#   exactly ONE line to stderr whose FIRST TOKEN is at COLUMN 0:
#     <SENTINEL> reason=<R> slots=<N> waited=<secs> disposition=<D> lock=<LOCK_BASE>
#   where <SENTINEL> is the '@@REIFY_SLOT_' prefix followed by 'TIMEOUT@@'
#   (assembled at runtime below -- see SELF-POLLUTION DISCIPLINE). It is never
#   emitted under WAIT=unlimited, never on an uncontended fast-path acquire, and
#   never folded into the existing human-readable deadline messages -- those are
#   dark-factory's OTHER grounded anchors and must stay verbatim.
#
#   Two field-level properties are pinned as their own cases because both are
#   invisible in the happy path:
#     - lock= is TERMINAL. It is the one operator-controlled field, so it is the
#       one that can carry whitespace; last position means a space-bearing path
#       extends the line instead of shifting reason=/slots=/waited=/disposition=
#       out from under a field-position parser (A9).
#     - disposition= distinguishes "the caller aborted with 75" (fatal -- the
#       three wrapper paths) from "the deadline passed and the work ran anyway"
#       (soft -- run_all.sh's pool worker, C). Without it the wire cannot tell a
#       degraded-but-fine pool from a genuine starvation abort, which would
#       recreate the misclassification this task exists to remove, inverted.
#
# SELF-POLLUTION DISCIPLINE (mandatory -- this test must not become the very
# incident it prevents):
#   This file's own stdout/stderr is captured into <n>.out and RE-EMITTED by the
#   outer run_all.sh into the merge-gate verify log -- exactly the stream
#   dark-factory classifies. A bare column-0 sentinel leaking from here would
#   make DF classify the ENTIRE merge verify as SEMAPHORE_TIMEOUT, recreating the
#   infra-hold class this task exists to fix, from inside the fix. DF's anchor
#   tolerates leading whitespace, so INDENTING IS NOT A DEFENCE. Therefore:
#     - the token is assembled at runtime from SP, so it is never contiguous in
#       this file's source nor in any assert description;
#     - every captured marker stays in a mktemp file and is never cat/echoed;
#     - all assert checkers are QUIET grep predicates, so assert's on-FAIL
#       captured-output dump (tests/infra/test_helpers.sh:52-56) stays empty.
#   Same idiom as tests/infra/test_run_all_clock_marker_sanitize.sh:38-42 (CP=)
#   for the @@REIFY_CLOCK_* family; the identical hazard there is a recorded
#   incident (tests/infra/test_test_run_semaphore.sh:61-69, esc-3940-81).
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob; declared
# `pool` in tests/infra/run-all-classification.manifest (hermetic mktemp
# fixtures, private lock paths, no cargo and no CPU burn).
#
# Section F, near the end of this file, is a separate standing guard: the
# roster of suites that are deadline-capable through a DIRECT wrapper call
# site is DERIVED from tests/infra/test_*.sh, not hand-maintained, and
# Section F proves the DECLARATION still matches that derivation. Its scope
# boundary -- notably the TRANSITIVE route it does not see -- is stated in
# full in that section's own preamble.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPTS_DIR="$REPO_ROOT/scripts"
RUN_ALL="$SCRIPT_DIR/run_all.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

# The sentinel under test, assembled at runtime (see SELF-POLLUTION DISCIPLINE).
# SP is the live prefix; SENTINEL is the whole token, which exists only as a
# runtime value and never as a literal in this file.
SP='@@REIFY_SLOT_'
SENTINEL="${SP}TIMEOUT@@"

_TMPDIRS=()
_HOLDERS=()
cleanup() {
    for _p in "${_HOLDERS[@]+${_HOLDERS[@]}}"; do
        kill "$_p" 2>/dev/null || true
        wait "$_p" 2>/dev/null || true
    done
    for _d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$_d"; done
}
trap cleanup EXIT

# --- Quiet grep predicates, used via assert (functions, so assert's
# no-subshell direct-call path runs them in this shell -- esc-4959-57).
# QUIET IS LOAD-BEARING, not style: assert dumps a FAILING checker's captured
# output, and a live column-0 sentinel in that dump is precisely the leak this
# file must never cause. `-- "$2"` end-of-options guard for needles that could
# otherwise be parsed as options.
_has_line()   { grep -qE -- "$2" "$1"; }     # <file> <ERE>     -> 0 if a line matches
_has_text()   { grep -qF -- "$2" "$1"; }     # <file> <literal> -> 0 if present
_lacks_text() { ! grep -qF -- "$2" "$1"; }   # <file> <literal> -> 0 if ABSENT

# --- Background slot holder + race-free ready handshake.
# Preferred over a bare `sleep 0.2` (the T20 idiom) because the ready marker
# CAUSALLY guarantees the holder owns `flock -x` before the contended acquire
# starts -- without it the exactly-one-sentinel assertions can go green-on-a-lie
# under host load. Modelled on tests/infra/test_verify_semaphore_e2e.sh:1119-1123.
# The 45s hold deliberately outlives every outer `timeout` below.
_HOLDER_PID=""
_hold_slot() {  # <lock-base> <slot-n>
    local _lock="$1" _slot="$2" _ready="$1.ready-$2" _i=0
    rm -f "$_ready"
    ( flock -x 9; : > "$_ready"; sleep 45 ) 9>>"${_lock}.slot-${_slot}" &
    _HOLDER_PID=$!
    _HOLDERS+=("$_HOLDER_PID")
    while [ ! -e "$_ready" ]; do
        _i=$((_i + 1))
        if [ "$_i" -ge 1200 ]; then
            echo "ERROR: slot holder never signalled ready for ${_lock}.slot-${_slot}" >&2
            return 1
        fi
        sleep 0.05
    done
    return 0
}

_reap_slot() {  # <lock-base> <slot-n>
    if [ -n "$_HOLDER_PID" ]; then
        kill "$_HOLDER_PID" 2>/dev/null || true
        wait "$_HOLDER_PID" 2>/dev/null || true
        _HOLDER_PID=""
    fi
    rm -f "${1}.slot-${2}" "${1}.ready-${2}" "$1"
}

TMPA="$(mktemp -d)"; _TMPDIRS+=("$TMPA")

echo "=== A: slot_acquire finite-WAIT deadline emits the slot-timeout sentinel ==="

# --- A1/A2/A3: the emission itself, driven directly against the shared lib.
# 5-arg call (the target signature): empty 4th REASON so clock-stop accounting
# is untouched, explicit 5th TIMEOUT_REASON. Extra positional args are ignored
# by a bash function, so this call shape is valid against BOTH the pre- and
# post-fix signature -- what makes it a true RED probe rather than a syntax error.
A_LOCK="$TMPA/a.lock"
A_ERR="$TMPA/a.err"

_hold_slot "$A_LOCK" 1

A_EXIT=0
timeout 30 bash -c '
    source "$1/lib_slot_acquire.sh"
    slot_acquire "$2" 1 2 "" "occt_slot_starvation"
' _ "$SCRIPTS_DIR" "$A_LOCK" 2>"$A_ERR" || A_EXIT=$?

_reap_slot "$A_LOCK" 1

# Counted, not just probed: a future double-emit is as much a contract break as
# no emit (DF would see the same category twice from one wait).
A_ANY_COUNT="$(grep -cF -- "$SENTINEL" "$A_ERR" || true)"
A_COL0_COUNT="$(grep -cE -- "^${SP}TIMEOUT@@" "$A_ERR" || true)"

assert "A1a: contended finite WAIT=2 returns 75 (EX_TEMPFAIL; got $A_EXIT)" \
    test "$A_EXIT" -eq 75
assert "A1b: stderr carries EXACTLY ONE slot-timeout sentinel line (got $A_ANY_COUNT)" \
    test "$A_ANY_COUNT" -eq 1
assert "A3: the sentinel sits at COLUMN 0 -- dark-factory's line anchor (got $A_COL0_COUNT)" \
    test "$A_COL0_COUNT" -eq 1
assert "A2a: sentinel carries reason=/slots=/waited=/disposition=/lock= in that order, waited= is digits" \
    _has_line "$A_ERR" "^${SP}TIMEOUT@@ reason=occt_slot_starvation slots=1 waited=[0-9]+ disposition=fatal lock=.+$"
# A2a's trailing `$` already pins lock= as the LAST field; this pins its VALUE,
# literally (grep -F), since a lock path is full of ERE metacharacters.
assert "A2b: the lock= field carries the LOCK_BASE actually passed" \
    _has_text "$A_ERR" "lock=${A_LOCK}"
assert "A2c: an un-specified disposition defaults to fatal (pre-existing callers unchanged)" \
    _has_line "$A_ERR" " disposition=fatal "

echo ""
echo "--- A7/A8: the TIMEOUT_REASON fallback chain, both branches ---"

# No in-repo caller takes either branch after task 6024 -- every call site passes
# an explicit 5th arg. That is exactly why they are pinned HERE: the fallback is
# still reachable by any future or out-of-tree caller, and whatever token it
# produces lands on a CROSS-REPO wire. Untested-but-observable is the gap.

# A7: 3-arg call -> the literal `slot_acquire` fallback token.
A7_LOCK="$TMPA/a7.lock"
A7_ERR="$TMPA/a7.err"
_hold_slot "$A7_LOCK" 1
A7_EXIT=0
timeout 30 bash -c '
    source "$1/lib_slot_acquire.sh"
    slot_acquire "$2" 1 1
' _ "$SCRIPTS_DIR" "$A7_LOCK" 2>"$A7_ERR" || A7_EXIT=$?
_reap_slot "$A7_LOCK" 1

assert "A7a: a bare 3-arg call still deadlines with 75 (got $A7_EXIT)" \
    test "$A7_EXIT" -eq 75
assert "A7b: with neither REASON nor TIMEOUT_REASON the wire carries reason=slot_acquire" \
    _has_line "$A7_ERR" "^${SP}TIMEOUT@@ reason=slot_acquire slots=1 waited=[0-9]+ disposition=fatal lock=.+$"

# A8: 4-arg call -> TIMEOUT_REASON inherits the clock REASON.
A8_LOCK="$TMPA/a8.lock"
A8_ERR="$TMPA/a8.err"
_hold_slot "$A8_LOCK" 1
A8_EXIT=0
REIFY_CLOCK_HEARTBEAT_SECS=3600 timeout 30 bash -c '
    source "$1/lib_slot_acquire.sh"
    slot_acquire "$2" 1 1 "a8_inherited_probe"
' _ "$SCRIPTS_DIR" "$A8_LOCK" 2>"$A8_ERR" || A8_EXIT=$?
_reap_slot "$A8_LOCK" 1

assert "A8a: a 4-arg call still deadlines with 75 (got $A8_EXIT)" \
    test "$A8_EXIT" -eq 75
assert "A8b: with no TIMEOUT_REASON the clock REASON is inherited onto the sentinel" \
    _has_line "$A8_ERR" "^${SP}TIMEOUT@@ reason=a8_inherited_probe slots=1 waited=[0-9]+ disposition=fatal lock=.+$"

echo ""
echo "--- A9: a whitespace-bearing LOCK_BASE cannot shift any other field ---"

# The guard on the terminal-lock= ordering choice stated in this file's header
# (rationale in full: slot_emit_timeout's header in scripts/lib_slot_acquire.sh).
# Move lock= back into the middle and the shape assertion below goes RED.
A9_DIR="$TMPA/a9 dir"
mkdir -p "$A9_DIR"
A9_LOCK="$A9_DIR/a9 lock.lock"
A9_ERR="$TMPA/a9.err"
_hold_slot "$A9_LOCK" 1
A9_EXIT=0
timeout 30 bash -c '
    source "$1/lib_slot_acquire.sh"
    slot_acquire "$2" 1 1 "" "occt_slot_starvation"
' _ "$SCRIPTS_DIR" "$A9_LOCK" 2>"$A9_ERR" || A9_EXIT=$?
_reap_slot "$A9_LOCK" 1

assert "A9a: a space-bearing lock base still deadlines with 75 (got $A9_EXIT)" \
    test "$A9_EXIT" -eq 75
assert "A9b: every fixed field is intact and unshifted despite the spaces" \
    _has_line "$A9_ERR" "^${SP}TIMEOUT@@ reason=occt_slot_starvation slots=1 waited=[0-9]+ disposition=fatal lock=.+$"
assert "A9c: the whole space-bearing path survives verbatim in the terminal lock= field" \
    _has_text "$A9_ERR" "lock=${A9_LOCK}"

echo ""
echo "--- A4: uncontended fast-path acquire is silent (negative control) ---"

A4_LOCK="$TMPA/a4.lock"
A4_ERR="$TMPA/a4.err"
A4_EXIT=0
timeout 30 bash -c '
    source "$1/lib_slot_acquire.sh"
    slot_acquire "$2" 1 2 "" "occt_slot_starvation"
' _ "$SCRIPTS_DIR" "$A4_LOCK" 2>"$A4_ERR" || A4_EXIT=$?
rm -f "$A4_LOCK" "${A4_LOCK}.slot-1"

assert "A4a: uncontended acquire returns 0 (got $A4_EXIT)" \
    test "$A4_EXIT" -eq 0
assert "A4b: uncontended fast path emits NO sentinel" \
    _lacks_text "$A4_ERR" "$SENTINEL"

echo ""
echo "--- A5: WAIT=unlimited can never emit the sentinel (structural, not conditional) ---"

A5_LOCK="$TMPA/a5.lock"
A5_ERR="$TMPA/a5.err"
_hold_slot "$A5_LOCK" 1
A5_EXIT=0
timeout 3 bash -c '
    source "$1/lib_slot_acquire.sh"
    slot_acquire "$2" 1 unlimited "" "occt_slot_starvation"
' _ "$SCRIPTS_DIR" "$A5_LOCK" 2>"$A5_ERR" || A5_EXIT=$?
_reap_slot "$A5_LOCK" 1

assert "A5a: WAIT=unlimited against a held slot never returns; the outer timeout fires (124; got $A5_EXIT)" \
    test "$A5_EXIT" -eq 124
assert "A5b: WAIT=unlimited emits NO sentinel (the deadline branch is unreachable)" \
    _lacks_text "$A5_ERR" "$SENTINEL"

echo ""
echo "--- A6: the sentinel survives run_all.sh's clock sanitizer (drift guard) ---"

# EXTRACTED from run_all.sh, never hardcoded: that is what makes this a real
# guard. Survival is true today only because _RA_CLOCK_SANITIZE is PREFIX-scoped
# to `@@REIFY_CLOCK_`; broadening it to `s/@@REIFY_/.../` later would silently
# neuter the cross-repo seam, and turns this assertion RED instead.
RA_SANITIZE_EXPR="$(sed -n "s/^_RA_CLOCK_SANITIZE='\(.*\)'\$/\1/p" "$RUN_ALL" | head -1)"
A6_OUT="$TMPA/a6.out"
printf '%sTIMEOUT@@ reason=run_all_pool_starvation slots=1 waited=3 disposition=soft lock=/tmp/x.lock\n' "$SP" \
    | sed "${RA_SANITIZE_EXPR:-s/^//}" > "$A6_OUT"

assert "A6a: run_all.sh's clock-sanitizer expression was extracted (non-vacuity)" \
    test -n "$RA_SANITIZE_EXPR"
assert "A6b: the sentinel survives that sanitizer unrewritten, still at column 0" \
    _has_line "$A6_OUT" "^${SP}TIMEOUT@@ "

echo ""
echo "=== B: each wrapper declares its OWN timeout reason, additively ==="

# Driven BEHAVIOURALLY (a real contended acquire through each wrapper), never by
# a source grep: the reason token is a cross-repo wire value, so what matters is
# what actually reaches stderr.
#
# B3 is the load-bearing REGRESSION half. The two human-readable deadline
# messages below are dark-factory's OTHER grounded anchors -- its
# _SLOT_ACQUIRE_DEADLINE_RE pins the script basename, the optional `ERROR: `
# prefix and the `within <N>s` shape, each "grounded in a line reify emits
# TODAY". Rewording them, or folding the sentinel into them, would silently
# delete three working detectors while adding one. B3 pins them with ^-anchored
# patterns and B4 pins that the sentinel is a SEPARATE line, so a later refactor
# cannot quietly merge the two.

# --- B0: scripts/lib_test_semaphore.sh -> reason=test_slot_starvation
# The highest-traffic call site (every verify test phase goes through it), and
# the one whose 5th arg is a deliberate literal duplicate of its 4th -- so a
# future edit that drops it would fall silently through the A7/A8 fallback chain
# and still emit a plausible-looking token. Covered behaviourally like B1/B2 so
# that edit goes RED here instead of being discovered on the DF side.
B0_LOCK="$TMPA/b0.lock"
B0_ERR="$TMPA/b0.err"
B0_MSGS="$TMPA/b0.msgs"
B0_MSG_RE='^lib_test_semaphore\.sh: failed to acquire test slot within 1s \(LOCK='

_hold_slot "$B0_LOCK" 1

B0_EXIT=0
# DISABLE explicitly emptied: an ambient REIFY_TEST_SEMAPHORE_DISABLE=1 would
# short-circuit the acquire entirely and make every assertion below vacuous.
REIFY_TEST_SEMAPHORE_LOCK="$B0_LOCK" REIFY_TEST_SEMAPHORE_CONCURRENCY=1 \
    REIFY_TEST_SEMAPHORE_WAIT=1 REIFY_TEST_SEMAPHORE_DISABLE= \
    REIFY_CLOCK_HEARTBEAT_SECS=3600 \
    timeout 30 bash -c '
        source "$1/lib_test_semaphore.sh"
        test_semaphore_acquire
    ' _ "$SCRIPTS_DIR" 2>"$B0_ERR" || B0_EXIT=$?

_reap_slot "$B0_LOCK" 1

{ grep -E -- "$B0_MSG_RE" "$B0_ERR" || true; } > "$B0_MSGS"

assert "B0a: test_semaphore_acquire on a held slot returns 75 (got $B0_EXIT)" \
    test "$B0_EXIT" -eq 75
assert "B0b: test-semaphore deadline sentinel carries reason=test_slot_starvation, at column 0" \
    _has_line "$B0_ERR" "^${SP}TIMEOUT@@ reason=test_slot_starvation slots=1 waited=[0-9]+ disposition=fatal lock=.+$"
assert "B0c: test-semaphore human deadline message is unchanged (DF's grounded anchor)" \
    test -s "$B0_MSGS"
assert "B0d: the test-semaphore sentinel is a SEPARATE line, not appended to that message" \
    _lacks_text "$B0_MSGS" "$SENTINEL"

echo ""
echo "--- B1: scripts/lib_lane_x_flock.sh -> reason=lane_x_slot_starvation ---"

# --- B1: scripts/lib_lane_x_flock.sh -> reason=lane_x_slot_starvation
B1_LOCK="$TMPA/b1.lock"
B1_ERR="$TMPA/b1.err"
B1_MSGS="$TMPA/b1.msgs"
# ^-anchored, matching dark-factory's own anchor shape for this line.
B1_MSG_RE='^lib_lane_x_flock\.sh: failed to acquire Lane-X lock within 1s \(LOCK='

_hold_slot "$B1_LOCK" 1

B1_EXIT=0
REIFY_LANE_X_FLOCK_LOCK="$B1_LOCK" REIFY_LANE_X_FLOCK_WAIT=1 \
    timeout 30 bash -c '
        source "$1/lib_lane_x_flock.sh"
        lane_x_flock_acquire
    ' _ "$SCRIPTS_DIR" 2>"$B1_ERR" || B1_EXIT=$?

_reap_slot "$B1_LOCK" 1

# Extract the human-message line(s) to their own file so B4 can assert on them
# with a quiet predicate; B3's `test -s` is what makes B4 non-vacuous.
{ grep -E -- "$B1_MSG_RE" "$B1_ERR" || true; } > "$B1_MSGS"

assert "B1a: lane_x_flock_acquire on a held lock returns 75 (got $B1_EXIT)" \
    test "$B1_EXIT" -eq 75
assert "B1b: lane-x deadline sentinel carries reason=lane_x_slot_starvation, at column 0" \
    _has_line "$B1_ERR" "^${SP}TIMEOUT@@ reason=lane_x_slot_starvation slots=1 waited=[0-9]+ disposition=fatal lock=.+$"
assert "B3a: lane-x human deadline message is unchanged (DF's grounded anchor)" \
    test -s "$B1_MSGS"
assert "B4a: the lane-x sentinel is a SEPARATE line, not appended to that message" \
    _lacks_text "$B1_MSGS" "$SENTINEL"

echo ""
echo "--- B2: scripts/cargo-test-occt-gated.sh -> reason=occt_slot_starvation ---"

B2_LOCK="$TMPA/b2.lock"
B2_ERR="$TMPA/b2.err"
B2_MSGS="$TMPA/b2.msgs"
B2_MSG_RE='^ERROR: cargo-test-occt-gated\.sh: failed to acquire OCCT slot within 1s \(LOCK='

_hold_slot "$B2_LOCK" 1

B2_EXIT=0
REIFY_OCCT_LOCK="$B2_LOCK" REIFY_OCCT_CONCURRENCY=1 REIFY_OCCT_LOCK_WAIT=1 \
    timeout 30 bash "$SCRIPTS_DIR/cargo-test-occt-gated.sh" true 2>"$B2_ERR" || B2_EXIT=$?

_reap_slot "$B2_LOCK" 1

{ grep -E -- "$B2_MSG_RE" "$B2_ERR" || true; } > "$B2_MSGS"

assert "B2a: the OCCT wrapper on a held slot exits 75 (got $B2_EXIT)" \
    test "$B2_EXIT" -eq 75
assert "B2b: OCCT deadline sentinel carries reason=occt_slot_starvation, at column 0" \
    _has_line "$B2_ERR" "^${SP}TIMEOUT@@ reason=occt_slot_starvation slots=1 waited=[0-9]+ disposition=fatal lock=.+$"
assert "B3b: OCCT human deadline message is unchanged, ERROR: prefix intact (DF's grounded anchor)" \
    test -s "$B2_MSGS"
assert "B4b: the OCCT sentinel is a SEPARATE line, not appended to that message" \
    _lacks_text "$B2_MSGS" "$SENTINEL"

echo ""
echo "=== C: run_all.sh's pool wait reaches the PARENT's stderr, unsanitized ==="

# THE HIGHEST-VALUE CASE. run_all.sh's pool wait is the one finite-WAIT path
# absent from dark-factory's three-basename grounded allowlist
# (lib_test_semaphore | cargo-test-occt-gated | lib_lane_x_flock), so the
# sentinel is the ONLY route by which a starved pool wait can ever be
# classified. Everything else in this suite hardens a path DF could already
# see; this section covers the one it could not.
#
# Two invisible facts make it work, so both are pinned here behaviourally
# (A6 pins the first synthetically as well):
#   1. _RA_CLOCK_SANITIZE is prefix-scoped to `@@REIFY_CLOCK_`, so it cannot
#      rewrite this family; and
#   2. the pool worker's slot_acquire writes to the INHERITED parent fd 2 --
#      the `> .out 2>&1` redirect is scoped to the member `bash` command only --
#      so the marker never enters the sanitized re-emission path at all.
#
# RECURSION NOTE: run_all.sh is driven against a TEMP fixture dir only, never
# the real tests/infra/ (this file is itself auto-discovered by the outer
# run_all).

if [ ! -f "$RUN_ALL" ]; then
    assert "run_all.sh present (skipped - pool substrate missing)" false
    test_summary
    exit 0
fi

TMPC="$(mktemp -d)"; _TMPDIRS+=("$TMPC")

# One trivial passing fixture + a PRIVATE classification manifest, so discovery
# sees exactly this member and nothing from the real suite.
cat > "$TMPC/test_marker_member.sh" <<'MEMBEREOF'
#!/usr/bin/env bash
echo "marker member fixture ran"
exit 0
MEMBEREOF
chmod +x "$TMPC/test_marker_member.sh"
cat > "$TMPC/classification.manifest" <<'EOF'
test_marker_member.sh pool
EOF

C_POOL_LOCK="$TMPC/pool.lock"
RA_OUT="$TMPC/ra_out.txt"

_hold_slot "$C_POOL_LOCK" 1

# POOL_WAIT=1 makes the worker's slot_acquire deadline in ~1s. The pool is a
# SOFT admission, so the member still runs unslotted and run_all still exits 0 --
# the sentinel is the ONLY observable, which is exactly the point. C4 pins that
# exit-0 half rather than leaving it as prose: C1/C3 are all emitted BEFORE the
# member runs, so every one of them would still pass if a pool-slot deadline
# became fatal (the regression run_all.sh's own soft-admission comment guards).
C_RC=0
RUN_ALL_CLASSIFICATION_MANIFEST="$TMPC/classification.manifest" \
    REIFY_RUN_ALL_POOL_LOCK="$C_POOL_LOCK" \
    REIFY_RUN_ALL_POOL_CONCURRENCY=1 \
    REIFY_RUN_ALL_POOL_WAIT=1 \
    REIFY_RUN_ALL_POOL_PSI_DISABLE=1 \
    timeout 300 bash "$RUN_ALL" "$TMPC" > "$RA_OUT" 2>&1 || C_RC=$?

_reap_slot "$C_POOL_LOCK" 1

# $RA_OUT deliberately HOLDS a live column-0 sentinel -- it is the assertion
# target. It must stay in this temp file: never cat it, never let it reach this
# test's own stdout, or the outer run_all re-emits it into the merge-gate verify
# log and dark-factory misclassifies the whole verify. Quiet checkers only.

# C3 FIRST: without these, C1/C2 could pass vacuously via the legacy serial
# fallback (which has no pool wait at all, hence no sentinel to find or rewrite).
assert "C3a: the pool path was actually taken (INFO: run_all.sh pool: N= present)" \
    _has_text "$RA_OUT" "INFO: run_all.sh pool: N="
assert "C3b: the fixture member actually ran" \
    _has_text "$RA_OUT" "--- Running: test_marker_member.sh ---"

assert "C1: pool deadline sentinel reaches run_all's own output with reason=run_all_pool_starvation, at column 0" \
    _has_line "$RA_OUT" "^${SP}TIMEOUT@@ reason=run_all_pool_starvation slots=1 waited=[0-9]+ disposition=soft lock=.+$"
assert "C2: that sentinel is never rewritten to the QUOTED form (it rode the inherited parent fd 2)" \
    _lacks_text "$RA_OUT" "@@REIFY_QUOTED_SLOT_"
# C4/C5 are the two halves of "a soft deadline is not a starvation abort". C4 is
# the BEHAVIOUR (the run completed anyway); C5 is that the behaviour is legible
# ON THE WIRE, which is the only half a consumer in another repo can see. The
# three wrapper paths emit disposition=fatal (B0b/B1b/B2b) for the same reason.
assert "C4: soft admission -- run_all still exits 0 despite the pool deadline (got $C_RC)" \
    test "$C_RC" -eq 0
assert "C5: the pool sentinel says disposition=soft, so the wire distinguishes it from an abort" \
    _has_line "$RA_OUT" "^${SP}TIMEOUT@@ .* disposition=soft "

echo ""
echo "=== D: no deadline-forcing infra test leaks a live sentinel into the verify log ==="

# THE FIX MUST NOT BECOME THE INCIDENT. Sections A-C made slot_acquire emit a
# live column-0 sentinel on EVERY finite-WAIT rc=75 -- including the deadlines
# that PRE-EXISTING infra tests force deliberately, on a normal GREEN run. Under
# run_all.sh a member's stdout+stderr land in <n>.out and Phase 3 re-emits them
# UNCONDITIONALLY (run_all.sh:1785/1799) through a sanitizer that is prefix-scoped
# to `@@REIFY_CLOCK_` (:368, pinned by A6) -- so such a sentinel survives verbatim
# at column 0 into the merge-gate verify log, and dark-factory's presence-anchored
# classifier marks the ENTIRE merge verify as SEMAPHORE_TIMEOUT. That is precisely
# the infra-hold misclassification this task exists to remove, reintroduced by the
# fix. Sections A-C hold that discipline for THIS file's own emissions; D extends
# it to the emit-adjacent sites this change turned from latent into live.
#
# D_MEMBERS below is NO LONGER the source of truth for "which suites can leak"
# -- task 6255 made it the BEHAVIOURAL SUBSET of the direct-call-site
# deadline-capable roster Section F derives (D_ROSTER). D_ROSTER also lists
# the static-only members (test_run_all.sh, test_slot_event_log.sh,
# test_verify_semaphore_e2e.sh); their evidence-preservation is ASSERTED IN
# PROSE beside D_ROSTER_MODE and measured clean today, but is NOT
# machine-checked -- generalizing D4 to cover them is tracked as #6278. F4,
# in Section F, is the assertion that keeps this behavioural list and that
# roster's mode table in lockstep -- a RED there means the two have silently
# diverged, not that this section itself is broken.
#
# TWO ARMS cover that BEHAVIOURAL SUBSET, not the whole deadline-capable
# problem -- see Section F's own SCOPE paragraph for what it does and does
# not prove. Within the subset, neither arm suffices alone. D1/D3 are
# BEHAVIOURAL and model run_all's capture exactly (see _d_capture), catching
# any leak that actually happens on this run. D4 is STATIC and covers what D1
# structurally cannot: two of the three members only reach their deadline
# under contention this suite must not manufacture, so their D1 zero would
# look identical with the redirect reverted. See D4's own preamble for the
# per-member reasoning.

TMPD="$(mktemp -d)"; _TMPDIRS+=("$TMPD")

# dark-factory's anchor, semantics verbatim: `^[ \t]*` TOLERATES leading
# whitespace, so INDENTING IS NOT A DEFENCE (D2b pins that). POSIX [[:blank:]] is
# exactly space+tab -- GNU grep -E does not honour `\t` inside a bracket
# expression, so spelling it that way would silently match a literal `t`.
D_ANCHOR="^[[:blank:]]*${SP}TIMEOUT@@"

# Members with a site that can reach slot_acquire's rc=75 branch -- and now its
# sentinel. Only the FIRST reaches it on every green run; the other two reach it
# only under contention this suite must not manufacture (see D4, which is the
# arm that guards them deterministically).
# RECURSION: test_slot_timeout_marker.sh is deliberately ABSENT from this list.
# It is the one file whose captured sentinels are the assertion target, and every
# one of them is confined to a temp file by the discipline at the top of this file.
D_MEMBERS=(test_lane_x_flock.sh test_test_run_semaphore.sh test_occt_flock_gate.sh)
# Index-aligned non-vacuity anchors: the deadline-forcing section header(s) of
# each member, newline-separated, each asserted on its own. Renaming or deleting
# one of those sections turns this guard RED rather than silently green.
D_HEADERS=(
    $'^--- Test 13:\n^--- Test 15:'
    $'^--- Test HG-2:'
    $'^--- Test 15:'
)
# Index-aligned: does this member reach the deadline on EVERY run, or only under
# contention? Purely descriptive -- printed beside D1 so its zero is read for
# what it is, never asserted on (asserting a member's own pass/fail here would
# re-report that suite's flakes as failures of this one, which is exactly what
# _d_capture's ignore-exit-status rule exists to prevent).
D_ALWAYS_DEADLINES=(
    'every run (Test 12/15 hold their own slot)'
    'MERGE GATE ONLY (WAIT=0 vs the REAL host-global semaphore, which verify.sh holds across every test pass)'
    'UNDER LOAD ONLY (WAIT=10 vs its own 6s holder)'
)

_d_capture() {  # <member-basename> <outfile>
    # Combined stdout+stderr into ONE file -- byte-identical to run_all's own
    # member capture (`bash "$INFRA_DIR/$name" > "<n>.out" 2>&1`, run_all.sh:1691),
    # which is the stream Phase 3 re-emits. A nested run_all would add nothing:
    # its sanitizer provably cannot touch this token family (A6), so direct
    # capture is equally faithful at a fraction of the cost.
    #
    # Exit status is IGNORED BY DESIGN: this guard asserts OUTPUT SHAPE only, so
    # it can never double-report another suite's failure (or its load flakes) as
    # a failure of this one. `timeout` here is a pure anti-hang backstop, not a
    # timing assertion -- nothing below compares an elapsed duration.
    timeout 600 bash "$SCRIPT_DIR/$1" > "$2" 2>&1 || true
}

# --- D2 FIRST: positive control on the D1 predicate.
# D1 asserts an ABSENCE, and an absence-assert with a typo'd regex is green
# forever. D2 pins that the exact predicate D1 uses does detect the real shape,
# at column 0 (D2a) and indented (D2b -- the anchor's whitespace tolerance is
# what makes indentation a non-defence, and the reason D1 must scan for it).
D_CTRL0="$TMPD/positive-control-col0.out"
D_CTRL1="$TMPD/positive-control-indented.out"
printf '%sTIMEOUT@@ reason=lane_x_slot_starvation slots=1 waited=1 disposition=fatal lock=/tmp/x.lock\n' \
    "$SP" > "$D_CTRL0"
printf '    %sTIMEOUT@@ reason=lane_x_slot_starvation slots=1 waited=1 disposition=fatal lock=/tmp/x.lock\n' \
    "$SP" > "$D_CTRL1"

D_CTRL0_COUNT="$(grep -cE -- "$D_ANCHOR" "$D_CTRL0" || true)"
D_CTRL1_COUNT="$(grep -cE -- "$D_ANCHOR" "$D_CTRL1" || true)"

assert "D2a: positive control -- D1's own predicate DOES detect a column-0 sentinel (got $D_CTRL0_COUNT)" \
    test "$D_CTRL0_COUNT" -eq 1
assert "D2b: positive control -- it detects an INDENTED one too (indentation is not a defence; got $D_CTRL1_COUNT)" \
    test "$D_CTRL1_COUNT" -eq 1

echo ""
echo "--- D1/D3: each deadline-forcing member, captured exactly as run_all captures it ---"

# Concurrently, so the added wall clock is max(member) rather than sum(member).
_d_pids=()
for _d_i in "${!D_MEMBERS[@]}"; do
    _d_capture "${D_MEMBERS[$_d_i]}" "$TMPD/${D_MEMBERS[$_d_i]}.out" &
    _d_pids+=("$!")
done
for _d_p in "${_d_pids[@]}"; do wait "$_d_p" 2>/dev/null || true; done

for _d_i in "${!D_MEMBERS[@]}"; do
    _d_m="${D_MEMBERS[$_d_i]}"
    _d_f="$TMPD/${_d_m}.out"

    # D3 BEFORE D1: without it D1 would pass vacuously on an empty capture (a
    # member that failed to start, or whose deadline-forcing section was renamed).
    while IFS= read -r _d_h; do
        assert "D3 [$_d_m]: deadline-forcing section '${_d_h#^--- }' actually ran (non-vacuity)" \
            _has_line "$_d_f" "$_d_h"
    done <<< "${D_HEADERS[$_d_i]}"

    # D1 reports the COUNT and the MEMBER NAME only -- never the matched content.
    # `test` as the checker is load-bearing, not style: assert dumps a FAILING
    # checker's captured output, and dumping the offending line would BE the leak
    # this assertion exists to prevent. The count is the actionable half; the
    # member name says where to look.
    # SCOPE, stated in the description rather than left implied: this observes
    # THIS RUN's capture, and two of the three members only deadline under
    # contention -- so a zero from them means "no leak occurred here", not "no
    # leak is possible". D4 below is what makes their redirects load-bearing.
    _d_n="$(grep -cE -- "$D_ANCHOR" "$_d_f" || true)"
    assert "D1 [$_d_m]: leaked ZERO slot-timeout sentinels into its own captured output THIS RUN -- deadline reached: ${D_ALWAYS_DEADLINES[$_d_i]} (got $_d_n)" \
        test "$_d_n" -eq 0
done

echo ""
echo "--- D4: each member's own deadline-forcing site still CAPTURES stderr (static) ---"

# WHY A STATIC ARM IS REQUIRED. D1 can only observe a leak that actually
# happened this run, and only test_lane_x_flock.sh reaches its deadline on every
# run. The other two reach theirs only under contention this suite must not
# manufacture:
#   - test_test_run_semaphore.sh HG-2 waits on the REAL host-global semaphore
#     with WAIT=0. It deadlines under the MERGE GATE (verify.sh holds that lock
#     across every test pass -- the one scenario where the leak matters) and
#     acquires instantly otherwise. Holding that lock from here to force it
#     would starve every concurrent verify on this host.
#   - test_occt_flock_gate.sh Test 15 sets REIFY_OCCT_LOCK / _LOCK_WAIT /
#     _CONCURRENCY INLINE, so no ambient env from here can make it deadline.
# So on a green standalone run D1 reports 0 for both WHETHER OR NOT their stderr
# is captured -- it would report the same 0 with the redirects reverted, which
# is precisely the hole. D4 goes RED on that revert: it reads each member's
# SOURCE and asserts the deadline-forcing invocation still captures stderr to a
# file, every run, contention or not.
#
# "The redirect is NECESSARY" and "the redirect is PRESENT" are pinned in
# different sections on purpose: B0/B2 already prove BEHAVIOURALLY that these
# exact entry points emit a column-0 sentinel when contended, so D4 does not
# need to re-manufacture that contention -- only to prove the member site that
# reaches them still swallows it.

# Index-aligned with D_MEMBERS: the acquire entry point each member invokes.
# Deliberately redirect-INDEPENDENT, so "the site vanished" (D4a) and "the site
# stopped capturing" (D4b) stay distinguishable failures.
D_INVOKE=('"\$LIB"' 'test_semaphore_acquire' '"\$WRAPPER"')
# A capture to a FILE (`2>"$VAR"` / `2>$VAR`). `2>&1` deliberately does NOT
# match -- merging the sentinel back into the re-emitted stream is the leak, not
# a fix -- and neither does `2>/dev/null`, which would destroy the evidence a
# failing assert needs.
D_CAPTURE_RE='2>"?\$'

_d_section_cmds() {  # <member-file> <output-header-anchor> -> that section's commands
    # Continuations are joined FIRST so each logical command is ONE line: HG-2
    # puts its `2>` on the continuation line, not on the line naming the acquire,
    # and a line-at-a-time scan would call that unredirected.
    # Then the section is sliced header-to-next-header, and comment / assert
    # lines are dropped -- both name the entry point in PROSE without invoking it
    # (test_test_run_semaphore.sh:859 is exactly that shape).
    local _src
    _src="echo \"${2#^}"
    sed -e :a -e '/\\$/N; s/\\\n//; ta' "$1" \
        | awk -v s="$_src" '
            !inb && index($0, s) { inb = 1; next }
            inb && /^echo "---/ { exit }
            inb { print }
          ' \
        | grep -vE '^[[:blank:]]*(#|assert )' || true
}

_d_unredirected() {  # <cmds-file> <invocation-ERE> -> count of invocations with no capture
    local _n
    _n="$( { grep -E -- "$2" "$1" || true; } | grep -cvE -- "$D_CAPTURE_RE" || true )"
    echo "${_n:-0}"
}

# D4c FIRST: positive control on D4b's predicate, since D4b asserts a ZERO and a
# typo'd predicate would be green forever. Synthetic section, built at runtime.
D_CTRL_SEC="$TMPD/positive-control-section.cmds"
printf 'REIFY_X=1     timeout 5 "$LIB" true || _EXIT=$?\n' > "$D_CTRL_SEC"
D_CTRL_BARE="$(_d_unredirected "$D_CTRL_SEC" '"\$LIB"')"
assert "D4c: positive control -- D4b's own predicate DOES flag an invocation with no stderr capture (got $D_CTRL_BARE)" \
    test "$D_CTRL_BARE" -eq 1

for _d_i in "${!D_MEMBERS[@]}"; do
    _d_m="${D_MEMBERS[$_d_i]}"
    while IFS= read -r _d_h; do
        _d_tag="${_d_h#^--- }"
        _d_sec="$TMPD/${_d_m}.$(printf '%s' "$_d_tag" | tr -cd 'A-Za-z0-9').cmds"
        _d_section_cmds "$SCRIPT_DIR/$_d_m" "$_d_h" > "$_d_sec"

        # D4a before D4b: without it, deleting or renaming the invocation would
        # leave D4b trivially green on an empty set.
        _d_inv="$(grep -cE -- "${D_INVOKE[$_d_i]}" "$_d_sec" || true)"
        assert "D4a [$_d_m ${_d_tag%%:*}]: the deadline-capable invocation is still there (non-vacuity; got ${_d_inv:-0})" \
            test "${_d_inv:-0}" -ge 1

        # Counts only -- like D1, never the matching source line.
        _d_bare="$(_d_unredirected "$_d_sec" "${D_INVOKE[$_d_i]}")"
        assert "D4b [$_d_m ${_d_tag%%:*}]: every one of them still captures stderr to a file (got $_d_bare unredirected)" \
            test "$_d_bare" -eq 0
    done <<< "${D_HEADERS[$_d_i]}"
done

echo ""
echo "=== E: no assert DESCRIPTION dumps a whole capture file (stderr OR stdout) ==="

# THE SECOND, SNEAKIER LEAK CHANNEL. Section D covers a child's stderr reaching
# the console; this covers evidence smuggled through assert's own description.
# assert() (tests/infra/test_helpers.sh:42-57) echoes "  PASS: $desc" /
# "  FAIL: $desc" with NO sanitizing, and indents only the CHECKER's captured
# output (l.54, `sed 's/^/  | /'`). Anything inside $desc prints RAW -- so lines
# 2+ of a multi-line interpolation start at COLUMN 0, on a PASSING assertion,
# and the `$(cat ...)` is evaluated on EVERY run even when only the printing is
# conditional. A captured stderr that carries a live sentinel therefore reaches
# the verify log through a green assert.
#
# Unlike Section D this channel is not observable on demand -- it prints only
# when that specific assert runs, under the contention that produced the
# deadline (test_test_run_semaphore.sh's HG-2 deadlines only when the parent
# verify.sh holds the host-global lock, which is exactly the merge-gate case).
# So the guard is STATIC, in the same genre as the existing
# tests/infra/test_no_new_wallclock_upper_bounds.sh scanner: a machine-checkable
# output-safety property, not a documentation-wording grep.

TMPE="$(mktemp -d)"; _TMPDIRS+=("$TMPE")

# Detector, in two halves so this file never contains the forbidden shape
# itself. A line is a violation iff it is assert-wired AND DUMPS a whole
# capture file into the description via an unfiltered reader (`cat`/`tail`/
# `head`/`$(< f)`), quoted or not, where the variable names a capture:
# _ERR/_STDERR/_OUT/_STDOUT with an optional numeric suffix ($A_ERR1/$A_ERR2
# already exist in tests/infra/test_verify_semaphore_e2e.sh).
#
# STDOUT captures are IN SCOPE, not just stderr ones -- the narrower _ERR-only
# reading of this lint had a demonstrable in-tree miss. run_all.sh's Phase-3
# re-emission writes each member's COMBINED stdout+stderr `.out` capture to its
# OWN stdout (_ra_emit_sanitized, run_all.sh:374-377), so a nested run_all's
# stdout/combined capture carries member STDERR bytes -- a leaked sentinel
# among them.
#
# BOUNDED BY DESIGN, and the assertion name says which bound: this flags a
# whole-file DUMP, not every conceivable interpolation. A FILTERED reader
# ($(sed ...) -- the sanctioned form, E3 -- or $(grep ...)) is out of grammar
# because the filter is where the `  | ` prefix belongs, and a variable holding
# text captured on an earlier line is out of grammar because there is no reader
# to key on. The STRUCTURAL fix that closes every variant at once is to have
# assert() itself pipe $desc through the same `sed 's/^/  | /'` it already
# applies to a failing checker's captured output (tests/infra/test_helpers.sh:
# 44/47/54), which would demote this lint to redundant belt-and-braces; it is
# NOT done here only because test_helpers.sh is outside task 6024's module
# locks. Same-line only, which is what every site in-tree uses; a description
# split across a `\`-continuation is out of the grammar.
E_ASSERT_RE='assert "'
E_CAT_RE='\$\((cat|tail|head|<)[^)]*\$\{?[A-Za-z_][A-Za-z0-9_]*(_ERR|_STDERR|_OUT|_STDOUT)[0-9]*'

_e_scan() {  # <dir> -> prints "basename:lineno" per hit; NEVER the matched content
    local _d="$1" _f
    for _f in "$_d"/*.sh; do
        [ -e "$_f" ] || continue
        # `cut -d: -f1` keeps ONLY grep -n's line number, so the offending line's
        # text can never reach stdout -- printing it would BE the leak.
        { grep -niE -- "$E_CAT_RE" "$_f" \
            | grep -iE -- "$E_ASSERT_RE" \
            | cut -d: -f1 \
            | sed "s|^|${_f##*/}:|"; } || true
    done
}

# --- E2/E3 FIRST: the detector's own controls, on synthetic fixtures.
# E1 asserts an ABSENCE, so a typo'd regex would make it green forever (E2), and
# a detector that also flagged the sanctioned remediation would make Section E
# unsatisfiable (E3). Fixtures are BUILT AT RUNTIME in mktemp dirs so this file
# carries no literal that E1's own repo-wide scan could trip over.
E_POS_DIR="$TMPE/fx-pos"; mkdir -p "$E_POS_DIR"
E_NEG_DIR="$TMPE/fx-neg"; mkdir -p "$E_NEG_DIR"

# EVERY reader token is assembled from a variable, so no probe's forbidden shape
# is ever contiguous in THIS file's source -- E1's sweep includes this file, and
# a literal probe would make Section E self-flagging and unsatisfiable.
_E_CAT='cat'
_E_TAIL='tail'
_E_LT='<'
# One probe per grammar variant the detector must cover. Each is a shape that
# reaches the same place -- a whole capture file dumped RAW into a description --
# so a narrowing edit to E_CAT_RE turns E2 RED with the count naming how many
# variants it stopped seeing.
{
    printf 'assert "E-probe 1 (got stderr: $(%s "$_PROBE_ERR"))" false\n' "$_E_CAT"
    # unquoted variable
    printf 'assert "E-probe 2 (got stderr: $(%s $_PROBE_STDERR))" false\n' "$_E_CAT"
    # a reader taking arguments, on a numeric-suffixed capture name
    printf 'assert "E-probe 3 (got stderr: $(%s -n 50 "$_PROBE_ERR2"))" false\n' "$_E_TAIL"
    # the $(< f) reader, on a stdout/combined capture -- in scope because
    # run_all Phase 3 re-emits a member's combined .out on its own stdout
    printf 'assert "E-probe 4 (got: $(%s "$_PROBE_OUT"))" false\n' "$_E_LT"
} > "$E_POS_DIR/test_e_positive_probe.sh"
E_POS_VARIANTS=4
# The sanctioned escape hatch: `  | ` is a NON-whitespace prefix, which is what
# actually defeats dark-factory's `^[ \t]*` anchor (indentation alone does not).
# Same filter assert itself uses on FAIL, and _dump_captured_stderr at
# tests/infra/test_verify_semaphore_e2e.sh:436.
cat > "$E_NEG_DIR/test_e_filtered_probe.sh" <<'ENEGEOF'
assert "E-probe: something went wrong (got stderr: $(sed 's/^/  | /' "$_PROBE_ERR"))" false
ENEGEOF

E2_COUNT="$(_e_scan "$E_POS_DIR" | wc -l | tr -d ' ')"
E3_COUNT="$(_e_scan "$E_NEG_DIR" | wc -l | tr -d ' ')"

assert "E2: positive control -- the detector flags ALL $E_POS_VARIANTS raw-dump variants in an assert description (quoted/unquoted/reader-with-args/\$(< f), _ERR and _OUT alike; got $E2_COUNT)" \
    test "$E2_COUNT" -eq "$E_POS_VARIANTS"
assert "E3: the prefix-filtered form is the sanctioned escape hatch and is NOT flagged (got $E3_COUNT)" \
    test "$E3_COUNT" -eq 0

echo ""
echo "--- E1: repo-wide sweep of tests/infra/*.sh ---"

E_HITS="$TMPE/e1-hits.txt"
_e_scan "$SCRIPT_DIR" > "$E_HITS"
E1_COUNT="$(wc -l < "$E_HITS" | tr -d ' ')"
# Precomputed into a plain variable (not a $(...) inside the description) so this
# assert can never itself become an instance of what it forbids. The list is
# basename:lineno only.
E1_HIT_LIST="$(tr '\n' ' ' < "$E_HITS")"

assert "E1: no assert description DUMPS a whole capture file (cat/tail/head/\$(< f) of a *_ERR/*_STDERR/*_OUT/*_STDOUT var) into its text (got $E1_COUNT: $E1_HIT_LIST)" \
    test "$E1_COUNT" -eq 0

echo ""
echo "=== F: the DIRECT-call-site deadline-capable roster is DERIVED, not hardcoded ==="

# Section D's D_MEMBERS is a hand-maintained list of suites that force a
# finite-WAIT slot_acquire deadline. Task 6255: that list silently missed
# three real deadline-capable suites (test_run_all.sh, test_slot_event_log.sh,
# test_verify_semaphore_e2e.sh) -- discovered only by deriving the set fresh
# from source and diffing it against the declaration. This section is that
# derivation, kept live as a standing drift guard: F1 below is what stops
# the declared list from silently falling out of step with reality again.
#
# SCOPE, stated rather than left implied (D1's own convention, see its
# description above). The derivation is over DIRECT call sites ONLY: a file
# that itself names one of the four acquire wrappers. Section F catches a
# new DIRECTLY-deadline-capable SUITE appearing -- a file with no matching
# entry in D_ROSTER at all. A green F1 is therefore proof that the
# direct-call-site roster is SELF-CONSISTENT; it is NOT proof that no other
# tests/infra suite can reach a deadline by some other route. Three gaps
# are deliberately out of scope:
#
# (1) A new unredirected deadline SITE added inside an existing static-only
# roster member -- D4's per-site stderr-capture check (D4a/D4b) still covers
# only the three behavioural D_MEMBERS. Generalizing D4 to the static-only
# members was declined here because it needs subshell-block analysis D4's
# line-oriented section slicer (_d_section_cmds) cannot do:
# test_verify_semaphore_e2e.sh captures at `) 2>"$C_ERR"`, on the subshell's
# closing line, several lines after the invocation it guards -- not on the
# invoking line itself, which is the only shape D4's slicer can see.
# Generalizing D4 to cover it is tracked as #6278.
#
# (2) TRANSITIVE capability -- a suite that reaches a deadline only by
# invoking tests/infra/run_all.sh (whose pool worker calls slot_acquire with
# the finite default REIFY_RUN_ALL_POOL_WAIT=1800, run_all.sh:1361), or by
# invoking another suite that does. Adding `run_all` to F_BIND_RE/F_EXEC_RE
# was MEASURED and REJECTED: against the real tree it is a path-MENTION
# predicate, not a capability predicate, and it gets BOTH directions wrong.
#   - It ADMITS a file that is not deadline-capable at all:
#     test_verify_release_delta_skip.sh binds ACT_RUN_ALL (:521) purely as a
#     grep TARGET -- used only by `test -f` (:523) and two source-inspection
#     greps (:530, :536) -- and never invokes it. That is exactly the "path
#     appears only as DATA" class F3's negative control exists to reject.
#   - It still MISSES the real route for a file that IS capable:
#     test_run_all_ambient_isolation.sh matches only a `case`-pattern
#     comparison line (:160), while its actual capability is second-order --
#     `bash "$TARGET"` with TARGET=test_run_all.sh (:93, :366) -- which no
#     run_all alternation can see.
#   Admitting those would force roster entries whose measured justification
#   reads "not actually deadline-capable", emptying D_ROSTER's meaning.
#   The three GENUINE transitive members are measured CLEAN today: each
#   redirects the child's stderr into a file or a captured variable --
#   test_run_all_clock_marker_sanitize.sh (:96), test_run_all_content_skip.sh
#   (:86, :387), test_run_all_pool_lock_host_global.sh (:122, :126) -- and
#   Section E's repo-wide scanner already covers the assert-description
#   channel for all of them. So this is a drift-coverage gap, not a live
#   leak. Closing it properly needs a real transitive-closure derivation (a
#   fixed point over suite->suite invocation edges), which is out of this
#   task's scope; filed as a follow-up during esc-6255-1 remediation.
#
# (3) HEREDOC-EMBEDDED invocations. The derivation scans LINES, not shell
# syntax -- it has no heredoc-state tracking, so a wrapper call written as
# the BODY of a heredoc (a fixture-writing suite building another script's
# content -- this file's own F2/F3 fixtures below are exactly that shape)
# reads identically to a live invocation and is FLAGGED, wrongly. False
# POSITIVE only (it can add a spurious roster entry, never hide a real
# one), and it is why this file excludes itself from the derivation by
# basename rather than relying on the predicate to see through its own
# fixtures. No other tests/infra/test_*.sh file trips it today -- F1's pass
# is the evidence, since F1 would go RED the moment one did. Pinned as a
# known, accepted gap (not "fixed" by a real heredoc-state parser, which
# this line-oriented scanner deliberately is not) by the addendum assert
# just after F3 below.

# The declared roster: every suite this file currently knows to be
# deadline-capable THROUGH A DIRECT WRAPPER CALL SITE (see SCOPE (2) above
# for the transitive route this deliberately excludes), sorted
# (load-bearing -- F1 compares sorted lists, so a
# sorted declaration needs no re-sort and a human reading a RED sees the
# same ordering the derivation produced). Declared HERE, at the head of
# Section F, rather than beside D_MEMBERS/D_HEADERS/D_INVOKE/
# D_ALWAYS_DEADLINES in Section D: F1 below is what re-derives and checks
# this list, so it lives next to the check rather than next to Section D's
# unrelated behavioural machinery.
D_ROSTER=(
    test_lane_x_flock.sh
    test_occt_flock_gate.sh
    test_run_all.sh
    test_slot_event_log.sh
    test_test_run_semaphore.sh
    test_verify_semaphore_e2e.sh
)

# Index-aligned with D_ROSTER: is this member run behaviourally (a real
# contended acquire, inside Section D's concurrent arm) or only proven
# static-only (its deadline-forcing site is checked for evidence-
# preservation via source inspection, never actually contended here)? The
# `behavioural` entries are exactly today's D_MEMBERS -- D_MEMBERS itself
# is UNCHANGED by this task, so Section D's concurrent arm keeps its
# current wall clock. Trailing comment on every entry names the member so
# a reader never has to count array positions.
#
# MEASURED justification for every static-only entry below -- the artifact
# a future reader needs in order to decide whether the exclusion still
# holds. (The three behavioural entries need none: their justification is
# that they ARE run, inside Section D's concurrent arm.)
#   test_run_all.sh -- bucket pool; DOES force a deadline every green run
#   (Test 24, test_run_all.sh:2235-2320: a 30s holder on slot-1 against
#   REIFY_RUN_ALL_POOL_WAIT=2), but measured clean on both leak channels:
#   its invocation captures stdout/stderr to separate files (T24_STDOUT/
#   T24_STDERR, never 2>&1 or 2>/dev/null), and its assert descriptions
#   route captured output through the sanctioned prefix-filter Section E
#   blesses. Kept static-only to hold Section D's concurrent wall clock
#   flat -- it is a 238-assertion suite. Absence from task 6024's closing
#   sweep is the evidence a hand-maintained roster drifts.
#   test_slot_event_log.sh -- bucket pool; cheap (measured 4.1s, 35
#   assertions, zero sentinel lines), but never REACHES a deadline on a
#   green run -- it relies on the finite wrapper DEFAULTS
#   (REIFY_TEST_SEMAPHORE_WAIT/REIFY_OCCT_LOCK_WAIT=1800) and its longest
#   holder is 0.2s. A behavioural D1 zero from it would be structurally
#   vacuous, exactly the hole D4's own preamble names. Six of its seven
#   invocation sites also route stderr to /dev/null, which D4's
#   deliberately file-only capture grammar (D_CAPTURE_RE) excludes --
#   including it behaviourally would force weakening that grammar for the
#   existing three members.
#   test_verify_semaphore_e2e.sh -- bucket intra-run-serial (run-all-
#   classification.manifest:56 -- mutates the invoking lane's own shared
#   state: CoW target/, working-tree parser.c). Section D forks its members
#   CONCURRENTLY (the D1/D3 loop above) and this file is bucket pool, so
#   running it here would violate the run_all classification partition --
#   a CORRECTNESS hazard, not merely the wall clock its nested full-scope
#   verify.sh test pass would add. This is the binding reason the task's
#   option (a), adding it to Section D, was rejected.
D_ROSTER_MODE=(
    behavioural   # test_lane_x_flock.sh
    behavioural   # test_occt_flock_gate.sh
    static-only   # test_run_all.sh
    static-only   # test_slot_event_log.sh
    behavioural   # test_test_run_semaphore.sh
    static-only   # test_verify_semaphore_e2e.sh
)

TMPF="$(mktemp -d)"; _TMPDIRS+=("$TMPF")

# Four separately-named EREs, each covering one grammar shape a deadline-
# capable call site can take. Held apart (not inlined) so each stays
# independently greppable/editable and its own rationale stays attached.
#
# VALUE-AGNOSTIC ON PURPOSE: a digits-only `_WAIT=[0-9]+` form was measured
# to MISS test_verify_semaphore_e2e.sh, which assigns
# `export REIFY_TEST_SEMAPHORE_WAIT="$wait"` (a variable, :528).
F_WAIT_RE='REIFY_(TEST_SEMAPHORE|OCCT_LOCK|LANE_X_FLOCK|RUN_ALL_POOL)_WAIT='
# A variable bound to one of the four acquire-wrapper paths.
F_BIND_RE='^[[:blank:]]*[A-Za-z_][A-Za-z0-9_]*=.*/(lib_test_semaphore|cargo-test-occt-gated|lib_lane_x_flock|lib_slot_acquire)\.sh"?$'
# One of the four wrappers exec'd or sourced by path.
F_EXEC_RE='(bash|source)[[:blank:]]+"?[^"]*/(lib_test_semaphore|cargo-test-occt-gated|lib_lane_x_flock|lib_slot_acquire)\.sh'
# A bare call to one of the three acquire functions.
# BIND/EXEC/CALL exist because the wrapper defaults are FINITE
# (REIFY_TEST_SEMAPHORE_WAIT=1800 at lib_test_semaphore.sh:100,
# REIFY_OCCT_LOCK_WAIT=1800 at cargo-test-occt-gated.sh:112): a call site
# carrying no explicit knob at all is still deadline-capable, and that is
# the only route by which test_slot_event_log.sh is in scope.
F_CALL_RE='^[[:blank:]]*(test_semaphore_acquire|lane_x_flock_acquire|slot_acquire)([[:blank:]]|$)'

# _f_deadline_capable <dir> -> prints one BASENAME per deadline-capable
# member of <dir>, sorted, one per line. NAMES ONLY -- never a matched
# line (same discipline as `_e_scan` and D1): this
# file's own output is re-emitted into the merge-gate verify log.
# Comment-stripped first (grep -vE '^[[:blank:]]*#'): a token mentioned
# only in a comment does not make a file deadline-capable.
# Parameterized on <dir> so the SAME function drives both the synthetic
# controls below (F2/F3) and the real repo-wide sweep (F1).
#
# `grep -c` on the second (matching) grep, NOT `grep -q`: `-q` exits the
# instant it sees a match, which can race the still-writing upstream
# `grep -v` into SIGPIPE on a large file with an early match -- this file's
# `pipefail` then promotes that SIGPIPE to the pipeline's reported status,
# silently dropping a real positive (measured directly: with `-q`,
# test_verify_semaphore_e2e.sh and test_run_all.sh flake between MATCH/MISS
# across repeated runs of the identical command). `-c` must drain its whole
# input to produce a count, so it can never race the producer that way --
# the same reason D4's `_d_unredirected` already uses `-c`
# rather than `-q`/`-l` here.
_f_deadline_capable() {
    local _d="$1" _f _base _n
    for _f in "$_d"/test_*.sh; do
        [ -e "$_f" ] || continue
        _base="${_f##*/}"
        # test_helpers.sh matches the glob but is excluded by run_all's own
        # discovery. test_slot_timeout_marker.sh is RECURSION -- this file
        # is itself 17+ P3 hits and 4 knob hits of its own (the same
        # exclusion D_MEMBERS' own RECURSION comment documents, above).
        case "$_base" in
            test_helpers.sh|test_slot_timeout_marker.sh) continue ;;
        esac
        _n="$(grep -vE '^[[:blank:]]*#' "$_f" \
            | grep -cE -- "$F_WAIT_RE|$F_BIND_RE|$F_EXEC_RE|$F_CALL_RE" || true)"
        if [ "${_n:-0}" -ge 1 ]; then
            printf '%s\n' "$_base"
        fi
    done | sort
}

echo ""
echo "--- F2/F3: controls on the derivation predicate, before it drives anything ---"

# CONTROLS FIRST, this file's established discipline for every absence/
# equality assert (D2, D4c, E2/E3): F1 asserts an EQUALITY, and a typo'd
# regex on either side of it would be green forever without these.
F_POS_DIR="$TMPF/fx-pos"; mkdir -p "$F_POS_DIR"
F_NEG_DIR="$TMPF/fx-neg"; mkdir -p "$F_NEG_DIR"

# F2 POSITIVE -- one runtime-built fixture per grammar variant, each in ITS
# OWN file so a narrowing edit that stops matching exactly one variant shows
# up as a basename-COUNT drop naming how many variants it stopped seeing
# (the E_POS_VARIANTS idiom), not swallowed by the other four.
cat > "$F_POS_DIR/test_f_pos_wait_literal.sh" <<'WAITLITEOF'
#!/usr/bin/env bash
export REIFY_TEST_SEMAPHORE_WAIT=30
WAITLITEOF
cat > "$F_POS_DIR/test_f_pos_wait_var.sh" <<'WAITVAREOF'
#!/usr/bin/env bash
wait_secs=5
export REIFY_OCCT_LOCK_WAIT="$wait_secs"
WAITVAREOF
cat > "$F_POS_DIR/test_f_pos_bind.sh" <<'BINDEOF'
#!/usr/bin/env bash
LIB="$REPO_ROOT/scripts/lib_slot_acquire.sh"
BINDEOF
cat > "$F_POS_DIR/test_f_pos_exec.sh" <<'EXECEOF'
#!/usr/bin/env bash
bash "$SCRIPTS_DIR/cargo-test-occt-gated.sh" true
EXECEOF
cat > "$F_POS_DIR/test_f_pos_call.sh" <<'CALLEOF'
#!/usr/bin/env bash
slot_acquire "$LOCK" 1 1
CALLEOF
F_POS_VARIANTS=5

# F3 NEGATIVE -- fixtures that must NOT be detected. The second shape is
# load-bearing: a naive whole-file token-mention grep was measured to return
# 24 in-tree files on exactly it (closure fixtures in
# test_copy_list_preflight.sh, test_compute_trampoline_registration_wired.sh).
cat > "$F_NEG_DIR/test_f_neg_comment_only.sh" <<'CMTEOF'
#!/usr/bin/env bash
# Historical note (do not resurrect): this suite used to do
#   export REIFY_TEST_SEMAPHORE_WAIT=5
#   LIB="$REPO_ROOT/scripts/lib_slot_acquire.sh"
#   bash "$SCRIPTS_DIR/cargo-test-occt-gated.sh" true
#   slot_acquire "$LOCK" 1 1
# None of that runs anymore -- replaced by a hermetic stub below.
echo "stubbed, no real acquire call"
CMTEOF
cat > "$F_NEG_DIR/test_f_neg_closure_data.sh" <<'DATAEOF'
#!/usr/bin/env bash
# Closure fixture: wrapper basenames appear only as comparison DATA (bare,
# no directory prefix, never invoked) -- the copy-list/closure shape that
# trips a naive token-mention grep but must not trip this derivation.
for _f in verify.sh lib_test_semaphore.sh lib_slot_acquire.sh \
          cargo-test-occt-gated.sh lib_lane_x_flock.sh; do
    cp "$SRC/scripts/$_f" "$DST/scripts/$_f"
done
assert "closure contains lib_slot_acquire.sh" \
    closure_has "$ROOT" verify.sh lib_slot_acquire.sh
DATAEOF

F2_COUNT="$(_f_deadline_capable "$F_POS_DIR" | wc -l | tr -d ' ')"
F3_COUNT="$(_f_deadline_capable "$F_NEG_DIR" | wc -l | tr -d ' ')"

assert "F2: positive control -- the derivation flags all $F_POS_VARIANTS grammar variants (WAIT= literal, WAIT= var, bind, exec, bare call; got $F2_COUNT)" \
    test "$F2_COUNT" -eq "$F_POS_VARIANTS"
assert "F3: comment-only mentions and closure/copy-list DATA are NOT flagged (got $F3_COUNT)" \
    test "$F3_COUNT" -eq 0

# F3 addendum (documented gap, not a control -- see Section F's SCOPE (3)
# above): a wrapper call written as the BODY of a heredoc reads identically
# to a live invocation to this line-oriented derivation, so it is a KNOWN
# false positive. Pinned here rather than left as an unverified claim in
# prose -- if a future change to the predicate silently starts or stops
# matching this shape, this assert is what notices.
F_NEG_HEREDOC_DIR="$TMPF/fx-neg-heredoc"; mkdir -p "$F_NEG_HEREDOC_DIR"
cat > "$F_NEG_HEREDOC_DIR/test_f_neg_heredoc_body.sh" <<'HEREDOCEOF'
#!/usr/bin/env bash
# Fixture-writing suite: the call below is heredoc BODY TEXT building
# another script's content -- it never executes here, exactly like this
# file's own F2/F3 fixtures above.
cat > "$DST/fixture.sh" <<'INNER'
slot_acquire "$LOCK" 1 1
INNER
HEREDOCEOF
F3_HEREDOC_COUNT="$(_f_deadline_capable "$F_NEG_HEREDOC_DIR" | wc -l | tr -d ' ')"
assert "F3-known-gap: a wrapper call inside a quoted heredoc body is (mis)flagged today -- accepted, documented false positive (got $F3_HEREDOC_COUNT)" \
    test "$F3_HEREDOC_COUNT" -eq 1

echo ""
echo "--- F1: the declared DIRECT-call-site roster must equal the derived one ---"

F_DERIVED="$(_f_deadline_capable "$SCRIPT_DIR")"
F_DECLARED_SORTED="$(printf '%s\n' "${D_ROSTER[@]}" | sort)"
F_UNLISTED="$(comm -23 <(printf '%s\n' "$F_DERIVED") <(printf '%s\n' "$F_DECLARED_SORTED") | tr '\n' ' ' | sed 's/ *$//')"
F_STALE="$(comm -13 <(printf '%s\n' "$F_DERIVED") <(printf '%s\n' "$F_DECLARED_SORTED") | tr '\n' ' ' | sed 's/ *$//')"

# unlisted: a new deadline-capable suite appeared but was never declared.
# stale: a declared entry stopped matching the derivation. Basenames are
# safe to print (matched file CONTENT is not); both are precomputed into
# plain variables above, never a $(...) inside the description itself, so
# this assert can never become an instance of what Section E's E1 forbids.
# A RED here means "a human must classify a newly deadline-
# capable suite", not that a leak occurred -- Section D above is the leak
# guard; this is the guard that keeps its own membership list honest.
# Description says DIRECT-CALL-SITE, not bare "deadline-capable": a green
# here must not be misread as completeness over the transitive route this
# derivation cannot see (SCOPE (2) above).
assert "F1: the DERIVED roster of suites with a DIRECT wrapper call site over tests/infra/ equals the DECLARED one (unlisted: ${F_UNLISTED:-<none>}) (stale: ${F_STALE:-<none>})" \
    test "$F_DERIVED" = "$F_DECLARED_SORTED"

echo ""
echo "--- F4: every roster entry declares a MODE; the behavioural subset must equal D_MEMBERS ---"

# F4c FIRST, this file's established discipline for every check that a later
# assert depends on the shape of (cf. D2 before D1, D4c before D4b, E2/E3
# before E1): a short D_ROSTER_MODE must be diagnosed HERE, before the
# F4_BEHAVIOURAL loop below derives anything from it. Otherwise a truncated
# table would surface as an F4a "missing" red pointing at D_MEMBERS drift,
# hiding the real cause. D_ROSTER_MODE is always declared by this point (a
# straight-line script, no conditional path skips it), so no set -u guard is
# needed for the length check itself.
assert "F4c: D_ROSTER_MODE is index-aligned with D_ROSTER (got ${#D_ROSTER_MODE[@]}, want ${#D_ROSTER[@]})" \
    test "${#D_ROSTER_MODE[@]}" -eq "${#D_ROSTER[@]}"

# Per-index access below stays `:-`-defaulted even though D_ROSTER_MODE is
# always DECLARED: assert() does not abort on a FAIL, so if F4c just went
# red (D_ROSTER_MODE shorter than D_ROSTER) this loop still runs, and an
# out-of-range array INDEX is its own unbound-variable reference under
# `set -u` -- which would abort the whole file. The default keeps that a
# clean "not behavioural" read instead of a hard abort.
F4_BEHAVIOURAL=()
for _f4_i in "${!D_ROSTER[@]}"; do
    if [ "${D_ROSTER_MODE[_f4_i]:-}" = "behavioural" ]; then
        F4_BEHAVIOURAL+=("${D_ROSTER[_f4_i]}")
    fi
done

# Bidirectional equality is the actual anti-drift mechanism: it is what
# stops D_MEMBERS (Section D's concurrent behavioural arm) and the
# roster's own mode table from silently diverging again, which is the
# failure mode that produced this task. Two asserts, each naming ITS OWN
# direction, rather than one bundling both: a D_MEMBERS entry can go
# missing from the roster independently of the roster gaining an
# undeclared extra, and collapsing them into one pass/fail would hide
# which direction actually broke.
F4_MISSING=()
for _f4_m in "${D_MEMBERS[@]}"; do
    _f4_found=0
    for _f4_b in "${F4_BEHAVIOURAL[@]+${F4_BEHAVIOURAL[@]}}"; do
        [ "$_f4_b" = "$_f4_m" ] && { _f4_found=1; break; }
    done
    [ "$_f4_found" -eq 0 ] && F4_MISSING+=("$_f4_m")
done
F4_EXTRA=()
for _f4_b in "${F4_BEHAVIOURAL[@]+${F4_BEHAVIOURAL[@]}}"; do
    _f4_found=0
    for _f4_m in "${D_MEMBERS[@]}"; do
        [ "$_f4_b" = "$_f4_m" ] && { _f4_found=1; break; }
    done
    [ "$_f4_found" -eq 0 ] && F4_EXTRA+=("$_f4_b")
done
# "${ARR[*]}" (IFS-joined into ONE string), not `printf '%s '`: printf with
# zero array elements still runs its format ONCE, emitting a bare space
# that is non-empty and defeats the ":-<none>" fallback below. Both arrays
# are always DECLARED (never unset) by this point, so no [@]+[@] guard
# is needed for this join.
F4_MISSING_LIST="${F4_MISSING[*]}"
F4_EXTRA_LIST="${F4_EXTRA[*]}"

assert "F4a: every D_MEMBERS entry is declared behavioural in D_ROSTER_MODE (missing: ${F4_MISSING_LIST:-<none>})" \
    test "${#F4_MISSING[@]}" -eq 0
assert "F4b: every roster entry declared behavioural is in D_MEMBERS (extra: ${F4_EXTRA_LIST:-<none>})" \
    test "${#F4_EXTRA[@]}" -eq 0

test_summary
