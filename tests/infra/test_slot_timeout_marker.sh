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
# roster of deadline-capable suites -- those with a DIRECT wrapper call site,
# plus the transitive closure of the suites that INVOKE one -- is DERIVED
# from tests/infra/test_*.sh, not hand-maintained, and Section F proves the
# DECLARATION still matches that derivation. Its scope boundary -- the
# residual routes even the closure does not follow -- is stated in full in
# that section's own preamble.

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
# -- task 6255 made it the BEHAVIOURAL SUBSET of the deadline-capable roster
# Section F derives (D_ROSTER -- direct call sites, plus their transitive
# invocation closure since task 6291). D_ROSTER also lists EIGHT static-only
# members (test_run_all.sh, test_slot_event_log.sh, test_verify_semaphore_e2e.sh
# and the five transitive members 6291 added); their evidence-preservation is
# ASSERTED IN PROSE beside D_ROSTER_MODE -- measured, and for two of the
# transitive members measured NOT clean on the bare-variable echo/description
# channel (recorded there in full) -- but is NOT machine-checked; generalizing
# D4 to cover them is tracked as #6278.
# Section F derives D_ROSTER_MODE's behavioural/static-only split directly
# from D_MEMBERS membership (not a hand-typed parallel array), so the two
# cannot silently diverge -- see Section F for the derivation.
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
echo "=== F: the deadline-capable roster is DERIVED (direct call sites + invocation closure), not hardcoded ==="

# Section D's D_MEMBERS is a hand-maintained list of suites that force a
# finite-WAIT slot_acquire deadline. Task 6255: that list silently missed
# three real deadline-capable suites (test_run_all.sh, test_slot_event_log.sh,
# test_verify_semaphore_e2e.sh) -- discovered only by deriving the set fresh
# from source and diffing it against the declaration. This section is that
# derivation, kept live as a standing drift guard: F1 below is what stops
# the declared list from silently falling out of step with reality again.
#
# SCOPE, stated rather than left implied (D1's own convention, see its
# description above). The derivation is over DIRECT call sites -- a file that
# itself names one of the four acquire wrappers -- AND over the TRANSITIVE
# INVOCATION CLOSURE of those call sites: a file that invokes a capable node
# is capable too, to a fixed point (task 6291; the closure and its edge
# grammar are below, its controls are FC1-FC7). Section F catches a new
# deadline-capable SUITE appearing by EITHER route -- a file with no matching
# entry in D_ROSTER at all. A green F1 is therefore proof that the roster is
# SELF-CONSISTENT over both routes; it is NOT proof that no tests/infra suite
# can reach a deadline by ANY route whatsoever. Four gaps are deliberately
# out of scope:
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
# (2) RESIDUAL ROUTES THE CLOSURE STILL DOES NOT FOLLOW. Task 6291 CLOSED
# what used to stand here: a suite that reaches a deadline only by invoking
# tests/infra/run_all.sh (whose pool worker calls slot_acquire with the finite
# default REIFY_RUN_ALL_POOL_WAIT=1800, run_all.sh:1361), or by invoking
# another suite that does, IS derived now, and five such members are declared
# below. What closed it was a CAPABILITY edge grammar, not a path-MENTION
# one. Adding a bare `run_all` alternation to F_BIND_RE/F_EXEC_RE was measured
# and REJECTED because it got BOTH directions wrong -- it ADMITTED
# test_verify_release_delta_skip.sh, which binds the path (:521) purely as an
# inspection target for `test -f` (:523) and two greps (:530, :536), and it
# still MISSED test_run_all_ambient_isolation.sh's real second-order route,
# matching only its `case`-pattern comparison line (:160). Both measurements
# are now PINNED against a future re-widening -- see F_EDGE_ANCHOR's comment
# below, and the FC6a/FC7b real-tree route pins -- rather than left as prose.
# Three residual routes remain, each stated as MEASURED rather than
# hypothesised:
#
#   (i) THE scripts/ ROUTE. The node set is deliberately tests/infra-only
#   (test_*.sh plus run_all.sh), so a suite that reaches a deadline by
#   exec'ing scripts/verify.sh -- whose --include-infra plan segment runs
#   `bash tests/infra/run_all.sh` (verify.sh:2701 today) -- is not derived
#   through that hop. MEASURED over the tree today this hides NO member: the
#   anchored-verb exec sites for scripts/verify.sh outside this file are
#   test_occt_flock_gate.sh and test_verify_semaphore_e2e.sh -- both ALREADY
#   roster members by their own direct call sites -- and
#   test_verify_nextest_probe.sh, which is not a member and does not need to
#   be: its exec (:207) is reached only through `run_verify`, and every one of
#   that helper's call sites (:261, :280, :305, :327, :346) passes
#   --print-plan via "$@", so verify.sh prints a plan and never runs the
#   pipeline. Two further mentions are not invocations at all --
#   test_verify_pipeline_guard.sh:91 feeds the STRING `scripts/verify.sh` to
#   verify-pipeline-guard.sh as printf DATA, and test_hooks_call_verify.sh:35
#   and :48 inspect the hook files with `bash -c "grep ... verify.sh ..."`.
#   So this is a derivation gap, not a live hole -- but it is the one that
#   would go unseen first, the moment a suite runs verify.sh for real with
#   infra included.
#
#   (ii) POSIX DOT-SOURCE is not in the edge verb set {bash, sh, source}, and
#   the exclusion is measured in BOTH directions: admitting `.` as a verb
#   reads the git PATHSPEC at test_orchestrator_config_canonical_path.sh:64-65
#   (`-- . ':(exclude)<path>'`) as an invocation, and buys nothing, because
#   tests/infra holds exactly TWO real POSIX dot-sources of a .sh path
#   (test_land_script.sh:43, test_verify_semaphore_wiring.sh:334) and both
#   target hooks/main-gate-lib.sh, which is not a node. (test_seed_warm_lane.sh
#   :5137's `. "$1"` is a HEREDOC body, i.e. gap (3), not a live dot-source.)
#   Consistent with (4) below, which records the same omission in the DIRECT
#   predicate.
#
#   (iii) DYNAMICALLY CONSTRUCTED invocations -- a path assembled at run time,
#   or an exec through a variable holding a helper NAME rather than a path --
#   are out of a line-oriented scanner's reach by construction. Unlike (i) and
#   (ii) this is stated STRUCTURALLY, not as a counted absence: no cheap
#   enumeration bounds it, so no claim is made that no suite uses the shape
#   today. The one dynamic hop the derivation DOES follow is the deliberately
#   narrow exec-forwarding-helper rule (`ambient_isolation_check_one
#   "$TARGET"`); its own comment below records why widening it further was
#   declined.
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
#
# (4) GRAMMAR-PRECISION gaps in the direct-call-site predicate itself,
# found during amendment review. F_EXEC_RE/F_CALL_RE are LINE-oriented
# pattern matches, not a shell parser, so they miss idiomatic shapes that
# are semantically equivalent to ones they do match:
#   - POSIX dot-source (`. "$path"`) is not an alternative to `source`/
#     `bash` in F_EXEC_RE -- the same omission the edge grammar makes, for
#     the same measured reason ((2)(ii) above);
#   - a call guarded by a keyword or operator on the same line (e.g.
#     `if test_semaphore_acquire "$SLOT"; then`) is not matched by
#     F_CALL_RE, which requires the call to be the line's first token.
# Two sibling false negatives measured in the same review -- a flag token
# between an exec verb and its path, and a bind line carrying a trailing
# same-line comment -- WERE fixed, in F_EXEC_RE and F_BIND_RE respectively
# (see their own comments below); both were closed-form single-shape
# widenings, verified not to change the derived roster over the real tree
# before landing. The two above were not: safely admitting `if`/`&&`/`||`-
# guarded calls needs statement-boundary awareness this scanner does not
# have, and a naive widening risks matching inside an unrelated quoted
# string instead -- the same class of hazard that sank the run_all
# alternation recorded in (2) above. No suite in tests/infra is known to use either shape
# today (F1's pass is the evidence). Net effect: F2's positive control
# proves the derivation matches its OWN canonical fixture shapes, not the
# full diversity of real bash call syntax, and a green F1 should be read
# accordingly.

# The declared roster: every suite this file currently knows to be
# deadline-capable -- through a DIRECT wrapper call site, or through the
# TRANSITIVE closure of suites that invoke one (see SCOPE (2) above for the
# routes that remain outside even that), sorted
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
    test_run_all_ambient_isolation.sh
    test_run_all_clock_marker_sanitize.sh
    test_run_all_content_skip.sh
    test_run_all_pool_lock_host_global.sh
    test_run_all.sh
    test_slot_event_log.sh
    test_test_run_semaphore.sh
    test_verify_env_ambient_isolation.sh
    test_verify_semaphore_e2e.sh
)

# Classifies each D_ROSTER entry as `behavioural` (a real contended
# acquire, inside Section D's concurrent arm) or `static-only` (its
# deadline-forcing site is checked for evidence-preservation via source
# inspection, never actually contended here). DERIVED from D_MEMBERS
# membership just below, not hand-declared: a hand-typed parallel array
# needed its own bidirectional-equality-plus-index-alignment asserts
# (three asserts, ~40 lines) purely to prove it stayed in lockstep with
# D_MEMBERS -- guarding a self-inflicted duplication, not the system
# under test (amendment review). A value built by testing membership in
# D_MEMBERS cannot itself fall out of step with D_MEMBERS, so that
# guard-the-guard code is gone; nothing is lost by removing it, since
# D_MEMBERS' own members are already proven to be genuine direct call
# sites by F1 (they are members of the roster F1 derives). D_MEMBERS
# itself is UNCHANGED by this task, so Section D's concurrent arm keeps
# its current wall clock.
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
#
#   THE FIVE TRANSITIVE MEMBERS (task 6291). None is in D_MEMBERS, so all
#   five come out static-only and Section D's concurrent arm and its wall
#   clock are untouched. Each is recorded with its derived ROUTE and its
#   MEASURED leak-channel state -- the honest state, not a blanket "clean".
#   test_run_all_clock_marker_sanitize.sh -- bucket pool; route run_all.sh
#   (RUN_ALL bound :31, `bash "$RUN_ALL"` :96). Measured CLEAN on both
#   channels: the child is captured to $RA_OUT_FILE (:92-96) and only
#   grepped, and no capture variable is interpolated into any assert
#   description.
#   test_run_all_content_skip.sh -- bucket pool; route run_all.sh (RUN_ALL
#   bound :25, invoked :86 and :387). Measured CLEAN: captured to $RUN_OUT
#   (:80-87, :380-388) and only grepped; no capture interpolation in any
#   description.
#   test_run_all_pool_lock_host_global.sh -- bucket pool; route run_all.sh
#   (REAL_RUN_ALL bound :69, invoked :122 and :126). Measured CLEAN: captured
#   to $_out (:120-127), then reduced to a single `lock=` token
#   (`_line=...grep 'lock=' | head -n1`; `_lock="${_line##*lock=}"`); the
#   descriptions interpolate only that extracted token, which a sentinel line
#   cannot ride.
#   test_run_all_ambient_isolation.sh -- bucket pool; route test_run_all.sh,
#   second-order (TARGET bound :93, handed to `ambient_isolation_check_one`
#   :366, exec'd at run_all_ambient_isolation_lib.sh:73/:92). The capture at
#   the exec site is correct (`bash "$_target" 2>&1` into $_amb_out), but
#   run_all_ambient_isolation_lib.sh:106 ECHOES the whole $_amb_out on the
#   genuine-bug (red) path. That is a bare-variable dump with no reader
#   token, so it is outside E1's grammar in both directions -- E_CAT_RE is
#   reader-keyed and E1 is assert-keyed, and this is a bare `echo`. Stated as
#   a measured fact, not a defect claim against this task: the fix touches
#   another module and belongs to the guard-widening work #6278 sits in, and
#   is filed separately (ticket tkt_0RSN3SGERQF3E3KD04D71G6W8R, esc-6291-1).
#   test_verify_env_ambient_isolation.sh -- bucket pool; route
#   test_occt_flock_gate.sh (`bash "$SCRIPT_DIR/test_occt_flock_gate.sh"
#   2>&1` :177). The capture at the site is correct (into $amb_out,
#   :172-178), but :189 interpolates the whole $amb_out into an assert
#   DESCRIPTION on the red path -- again a bare variable with no reader to
#   key on, which E1's own preamble names as out of its grammar. Same
#   disposition as the entry above: measured, recorded, filed, not fixed
#   here.
#   test_verify_semaphore_e2e.sh -- bucket intra-run-serial (run-all-
#   classification.manifest:56 -- mutates the invoking lane's own shared
#   state: CoW target/, working-tree parser.c). Section D forks its members
#   CONCURRENTLY (the D1/D3 loop above) and this file is bucket pool, so
#   running it here would violate the run_all classification partition --
#   a CORRECTNESS hazard, not merely the wall clock its nested full-scope
#   verify.sh test pass would add. This is the binding reason the task's
#   option (a), adding it to Section D, was rejected.
# Built by testing each D_ROSTER entry for membership in D_MEMBERS,
# preserving index alignment by construction (one append per D_ROSTER
# iteration -- there is no way for this to end up a different length than
# D_ROSTER). Produces the identical behavioural/static-only split the
# hand-typed array previously declared verbatim: test_lane_x_flock.sh,
# test_occt_flock_gate.sh, test_test_run_semaphore.sh (D_MEMBERS' three
# entries) come out `behavioural`; the other EIGHT -- test_run_all.sh,
# test_slot_event_log.sh, test_verify_semaphore_e2e.sh and the five
# transitive members added by task 6291 -- come out `static-only`, which is
# why growing D_ROSTER needed no edit here and left Section D's concurrent
# arm and its wall clock untouched. See the measured justification for each
# static-only entry in the comment block above.
D_ROSTER_MODE=()
for _frm_entry in "${D_ROSTER[@]}"; do
    _frm_mode="static-only"
    for _frm_member in "${D_MEMBERS[@]}"; do
        [ "$_frm_member" = "$_frm_entry" ] && { _frm_mode="behavioural"; break; }
    done
    D_ROSTER_MODE+=("$_frm_mode")
done

TMPF="$(mktemp -d)"; _TMPDIRS+=("$TMPF")

# Four separately-named EREs, each covering one grammar shape a deadline-
# capable call site can take. Held apart (not inlined) so each stays
# independently greppable/editable and its own rationale stays attached.
#
# VALUE-AGNOSTIC ON PURPOSE: a digits-only `_WAIT=[0-9]+` form was measured
# to MISS test_verify_semaphore_e2e.sh, which assigns
# `export REIFY_TEST_SEMAPHORE_WAIT="$wait"` (a variable, :528).
F_WAIT_RE='REIFY_(TEST_SEMAPHORE|OCCT_LOCK|LANE_X_FLOCK|RUN_ALL_POOL)_WAIT='
# A variable bound to one of the four acquire-wrapper paths. Anchored to
# "quote-or-not, then blank-or-end-of-line", not a bare end-of-line ($):
# a strict $ anchor was MEASURED to silently drop a bind line the moment
# it grew a trailing same-line comment (e.g. `LIB=".../lib_slot_acquire.sh"
# # the wrapper`) -- test_slot_event_log.sh's roster membership rests on
# exactly one such line today, so this was a live gap, not a hypothetical
# one. Comment-stripping upstream only removes FULL-LINE comments, never a
# trailing one, so the anchor itself has to tolerate it.
F_BIND_RE='^[[:blank:]]*[A-Za-z_][A-Za-z0-9_]*=.*/(lib_test_semaphore|cargo-test-occt-gated|lib_lane_x_flock|lib_slot_acquire)\.sh"?([[:blank:]]|$)'
# One of the four wrappers exec'd or sourced by path. Tolerates flag
# tokens between the verb and the path (e.g. `bash -x ".../foo.sh"`): the
# original `"?[^"]*` could not cross the quote that follows a flag, so
# `bash -x "$SCRIPTS_DIR/cargo-test-occt-gated.sh"` was a measured miss.
# Both widenings above were checked against the real tree before landing
# and change no member of the derived roster (see SCOPE (4)).
F_EXEC_RE='(bash|source)[[:blank:]]+([^"[:blank:]]+[[:blank:]]+)*"?[^"]*/(lib_test_semaphore|cargo-test-occt-gated|lib_lane_x_flock|lib_slot_acquire)\.sh'
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
# The four EREs as ONE alternation -- the exact string _f_deadline_capable
# used to pass to grep inline, named so the direct sweep and the closure's
# SEED round cannot drift apart.
F_DIRECT_RE="$F_WAIT_RE|$F_BIND_RE|$F_EXEC_RE|$F_CALL_RE"

# _f_direct_capable_stripped <already-comment-stripped file> -> rc 0 if that
# file has a DIRECT wrapper call site, rc 1 otherwise. This is the ONLY
# direct predicate: it is _f_deadline_capable's old per-file body with the
# comment-strip lifted out of it, because the closure strips every node once
# up front (it needs those stripped copies for the edge scan anyway) and
# re-stripping per node inside the SEED round would double the seed cost for
# nothing. Grepping a FILE rather than a pipe also puts this beyond the
# SIGPIPE/pipefail hazard entirely.
#
# A caller holding an UNSTRIPPED path pipes it itself --
# `grep -vE '^[[:blank:]]*#' <path> | ...` -- rather than reaching for a
# second predicate: an uncalled unstripped twin over the same F_DIRECT_RE
# was shipped by task 6291 and REMOVED in its review amendment, because two
# near-identical predicates with only one of them live is a drift hazard (a
# future tightening of the direct grammar could land on the dead copy and
# look like it had taken effect while the derivation was unchanged).
_f_direct_capable_stripped() {
    local _n
    _n="$(grep -cE -- "$F_DIRECT_RE" "$1" || true)"
    [ "${_n:-0}" -ge 1 ]
}

# _f_excluded_node <basename> -> rc 0 if <basename> is excluded from the node
# set. test_helpers.sh matches the test_*.sh glob but is excluded by run_all's
# own discovery. test_slot_timeout_marker.sh is RECURSION -- this file is
# itself 17+ P3 hits and 4 knob hits of its own (the same exclusion
# D_MEMBERS' own RECURSION comment documents, above). Shared by the direct
# sweep and the closure so ONE list governs both.
_f_excluded_node() {
    case "$1" in
        test_helpers.sh|test_slot_timeout_marker.sh) return 0 ;;
    esac
    return 1
}

# _f_deadline_capable <dir> -> the derived ROSTER: the closure restricted to
# test_*.sh basenames, names only, sorted. run_all.sh is filtered back out
# here, and only here -- it is a capability NODE (three roster members reach
# their deadline only through it) but never a roster ENTRY, because D_ROSTER
# lists SUITES forked under the pool.
#
# `grep -E` is a PRINTING grep, which drains its input, so it is not the
# `-q`/`-l` shape the SIGPIPE/pipefail note above forbids; the trailing
# `|| true` is for a legitimately empty result, not to mask a failure.
_f_deadline_capable() {
    _f_closure "$1" | cut -d' ' -f1 | grep -E '^test_' || true
}

# ---------------------------------------------------------------------------
# THE TRANSITIVE-INVOCATION CLOSURE (task 6291).
#
# A suite that never names an acquire wrapper is still deadline-capable if it
# INVOKES something that is: tests/infra/run_all.sh (direct-capable itself --
# _H2_SLOT_ACQUIRE_LIB bind at run_all.sh:1036, bare slot_acquire call at
# :1692 -- and running its pool worker against the finite
# REIFY_RUN_ALL_POOL_WAIT default), or another suite that is capable. The
# closure is the fixed point of that invocation relation over the node set.
#
# NODE vs ROSTER ENTRY. The node set is tests/infra/test_*.sh PLUS
# tests/infra/run_all.sh, because run_all.sh is the hub three of the
# transitive members reach their deadline through. It participates as a
# capability NODE only; D_ROSTER stays a list of SUITES forked under the
# pool, so _f_deadline_capable filters run_all.sh back out.
#
# _f_node_list <dir> -> one PATH per closure node, unsorted.
_f_node_list() {
    local _d="$1" _f _base
    for _f in "$_d"/test_*.sh "$_d"/run_all.sh; do
        [ -e "$_f" ] || continue
        _base="${_f##*/}"
        if _f_excluded_node "$_base"; then continue; fi
        printf '%s\n' "$_f"
    done
}

# The EDGE grammar: four more separately-named EREs, held apart from each
# other and from the four direct ones for the same reason -- each keeps its
# own rationale attached and stays independently greppable. All four are
# PREFIXES/TEMPLATES to which a node's regex-escaped basename, or a bound
# variable's name, is appended by _f_edge_exists below.
#
# (a) THE COMMAND-BOUNDARY ANCHOR, and the reason this is a capability
# grammar rather than a path-mention one. An exec verb only STARTS a command
# at a command boundary: start-of-line, or after a blank, `(`, `)`, `;`, `|`,
# `&` or a backtick. MEASURED: without the anchor,
# tests/infra/test_run_all_ambient_isolation.sh:160 --
# `*"bash tests/infra/run_all.sh"*)`, a `case` comparison PATTERN, where the
# verb is preceded by a double quote -- reads as an invocation, and that file
# then derives by the WRONG route (via:run_all.sh instead of its real
# via:test_run_all.sh). The same anchor is what rejects a copy-list element:
# in `verify.sh run_all.sh` the `sh` that precedes the blank is the tail of
# `verify.sh`, preceded by `.`, not by a boundary.
F_EDGE_ANCHOR='(^|[[:blank:]();|&`])[[:blank:]]*'
# (b) THE VERB, plus the same flag tolerance F_EXEC_RE already carries
# (`bash -x "$X"`). The verb set is exactly {bash, sh, source}.
# POSIX DOT-SOURCE IS DELIBERATELY EXCLUDED, measured both ways: adding `\.`
# admits a git PATHSPEC -- `-- . ':(exclude)<path>'`, the shape at
# tests/infra/test_orchestrator_config_canonical_path.sh:64-65 -- as an
# invocation, and buys nothing: measured, tests/infra holds exactly TWO real
# POSIX dot-sources of a .sh path (test_land_script.sh:43 and
# test_verify_semaphore_wiring.sh:334) and both target hooks/main-gate-lib.sh,
# which is not a node -- no dot-source in tests/infra targets one. Recorded as
# a scoped gap in Section F's SCOPE paragraph, consistent with the existing
# note that dot-source is unhandled in the direct predicate too.
F_EDGE_VERB_RE="${F_EDGE_ANCHOR}"'(bash|sh|source)[[:blank:]]+([^"[:blank:]]+[[:blank:]]+)*'
# (c) A LITERAL path invocation: the anchored verb, then a path ending in
# `/<node basename>`. The trailing boundary stops `/run_all.sh` from matching
# inside `/run_all.sh.orig`.
F_EDGE_PATH_SUF='([^A-Za-z0-9_]|$)'
F_EDGE_LITERAL_PRE="${F_EDGE_VERB_RE}"'"?[^"[:blank:]]*/'
# (d) A BIND of a variable to a path ending in `/<node basename>`. Used with
# `grep -oE` so the VARIABLE NAME can be recovered -- a bind alone is not an
# edge, it is only phase one of one.
F_EDGE_BIND_PRE='^[[:blank:]]*[A-Za-z_][A-Za-z0-9_]*=[^[:blank:]]*/'
# (e) THE EXEC-POSITION test for a bound variable, appended around the
# variable name: it must appear immediately after the anchored verb, or as
# the line's own first command word. Requiring BOTH (d) and (e) FOR THE SAME
# VARIABLE is precisely what rejects the bind-only inspection shape --
# tests/infra/test_verify_release_delta_skip.sh binds ACT_RUN_ALL (:521) and
# then only `test -f`s (:523) and greps (:530, :536) it. That single measured
# false admission is what sank the `run_all`-alternation fix; this is the
# rule that makes the difference. The trailing boundary keeps `$RA` from
# matching inside `$RAX`.
F_EDGE_VAR_PRE='"?\$\{?'
F_EDGE_VAR_SUF='\}?([^A-Za-z0-9_]|$)'
# (f) THE EXEC-FORWARDING-LIB rule, which is what makes the SECOND-ORDER
# route visible. A sibling `*_lib.sh` is exec-forwarding iff it binds a
# variable from a positional AND execs THAT SAME variable through the
# anchored verb grammar above; every function such a lib defines is then
# treated as a forwarding call, so passing a node-bound variable to one is an
# edge. Same-variable is load-bearing and was measured: the looser reading
# ("binds SOME positional and execs SOME variable") admits SIX libs, among
# them test_affected_crates_lib.sh, which defines a function named `cargo`,
# and test_nextest_absent_lib.sh, which defines `cleanup`.
# MEASURED over tests/infra today, EXACTLY ONE lib qualifies --
# run_all_ambient_isolation_lib.sh. nextest_absent_lib.sh, plan_capture_lib.sh,
# occt_flock_gate_lib.sh, load_tolerance_lib.sh, copy_list_preflight_lib.sh,
# lock_charter_harness_lib.sh and test_helpers.sh all fail the exec half.
# That tightness is the whole point: it is what keeps `assert` and every
# other ubiquitous helper out of the rule. If a future reader finds many
# libs qualifying, the rule has stopped being tight and needs re-narrowing.
F_FWD_POSBIND_RE='^[[:blank:]]*(local[[:blank:]]+)?[A-Za-z_][A-Za-z0-9_]*="\$[0-9]"'
F_FWD_FNDEF_RE='^[[:blank:]]*[A-Za-z_][A-Za-z0-9_]*[[:blank:]]*\(\)'

# F_FWD_LIB_MAP[<lib basename>] -> `|`-separated alternation of the function
# names that exec-forwarding lib defines. Computed ONCE per derivation by
# _f_scan_fwd_libs below, hoisted out of both the per-node and the per-round
# loops, so the whole rule costs a constant handful of greps.
declare -A F_FWD_LIB_MAP=()
declare -A F_FWD_FN_ALT_CACHE=()
F_FWD_FN_ALT=""

_f_scan_fwd_libs() {
    local _d="$1" _sd="$2" _l _base _n _fns _pv _fwd
    local -a _pvars=()
    F_FWD_LIB_MAP=()
    F_FWD_FN_ALT_CACHE=()
    for _l in "$_d"/*_lib.sh; do
        [ -e "$_l" ] || continue
        _base="${_l##*/}"
        grep -vE '^[[:blank:]]*#' "$_l" > "$_sd/$_base" || true
        _pvars=()
        mapfile -t _pvars < <(grep -oE -- "$F_FWD_POSBIND_RE" "$_sd/$_base" \
            | sed -E 's/^[[:blank:]]*(local[[:blank:]]+)?([A-Za-z_][A-Za-z0-9_]*)=.*/\2/' | sort -u)
        if [ "${#_pvars[@]}" -eq 0 ]; then continue; fi
        _fwd=0
        for _pv in "${_pvars[@]}"; do
            [ -n "$_pv" ] || continue
            _n="$(grep -cE -- "${F_EDGE_VERB_RE}${F_EDGE_VAR_PRE}${_pv}${F_EDGE_VAR_SUF}" "$_sd/$_base" || true)"
            if [ "${_n:-0}" -ge 1 ]; then _fwd=1; break; fi
        done
        [ "$_fwd" -eq 1 ] || continue
        _fns="$(grep -oE -- "$F_FWD_FNDEF_RE" "$_sd/$_base" \
            | sed -E 's/^[[:blank:]]*//; s/[[:blank:]]*\(\)$//' | sort -u | tr '\n' '|' | sed 's/|$//' || true)"
        [ -n "$_fns" ] || continue
        F_FWD_LIB_MAP["$_base"]="$_fns"
    done
}

# _f_fwd_fn_alt <stripped-node-file> -> sets F_FWD_FN_ALT to the alternation
# of forwarding function names reachable from THAT node, i.e. defined by the
# exec-forwarding libs it actually `source`s (matched with the same anchored
# verb grammar). Restricting to sourced libs is what stops an unsourced
# lib's helper NAME from conferring an edge on a file that merely reuses the
# name. Sets a global rather than printing, so its per-node memo survives:
# a `$(...)` reader would be a subshell and the cache would be discarded.
_f_fwd_fn_alt() {
    local _sf="$1" _lib _n
    if [ -n "${F_FWD_FN_ALT_CACHE[$_sf]+set}" ]; then
        F_FWD_FN_ALT="${F_FWD_FN_ALT_CACHE[$_sf]}"
        return 0
    fi
    F_FWD_FN_ALT=""
    for _lib in "${!F_FWD_LIB_MAP[@]}"; do
        _n="$(grep -cE -- "${F_EDGE_LITERAL_PRE}${_lib//./\\.}${F_EDGE_PATH_SUF}" "$_sf" || true)"
        if [ "${_n:-0}" -ge 1 ]; then
            F_FWD_FN_ALT="${F_FWD_FN_ALT:+$F_FWD_FN_ALT|}${F_FWD_LIB_MAP[$_lib]}"
        fi
    done
    F_FWD_FN_ALT_CACHE["$_sf"]="$F_FWD_FN_ALT"
}

# _f_edge_exists <stripped-node-file> <target-basename> -> rc 0 if the node
# whose comment-stripped text is in <stripped-node-file> INVOKES
# <target-basename>. This is the one place the edge grammar lives, so
# tightening it is a one-function change.
#
# TWO PHASES, because an invocation can name its target either way:
#   A. a LITERAL path after an anchored verb;
#   B. a BIND of the path to a variable, AND that same variable appearing in
#      an EXEC POSITION somewhere in the file. Phase B alone is a mention,
#      not a call -- see F_EDGE_VAR_PRE's comment for the measured shape this
#      two-phase requirement is what rejects.
# Line-oriented and comment-stripped, exactly like the direct predicate; the
# two phases are per-FILE rather than per-LINE because a bind and its exec
# are routinely lines apart (all three real run_all invokers bind at the top
# of the file and exec 50+ lines later).
#
# Grepping a FILE, never downstream of a pipe, and counting rather than
# `-q`/`-l`: doubly beyond the SIGPIPE/pipefail hazard documented above.
_f_edge_exists() {
    local _sf="$1" _esc="${2//./\\.}" _n _v _re
    local -a _vars=()

    _n="$(grep -cE -- "${F_EDGE_LITERAL_PRE}${_esc}${F_EDGE_PATH_SUF}" "$_sf" || true)"
    if [ "${_n:-0}" -ge 1 ]; then return 0; fi

    mapfile -t _vars < <(grep -oE -- "${F_EDGE_BIND_PRE}${_esc}${F_EDGE_PATH_SUF}" "$_sf" \
        | sed -E 's/^[[:blank:]]*([A-Za-z_][A-Za-z0-9_]*)=.*/\1/' | sort -u)
    if [ "${#_vars[@]}" -eq 0 ]; then return 1; fi
    _f_fwd_fn_alt "$_sf"
    for _v in "${_vars[@]}"; do
        [ -n "$_v" ] || continue
        # Exec position: right after an anchored verb, or as the line's own
        # first command word, or -- the second-order route -- on a line whose
        # first command word is an exec-forwarding helper this file sources.
        _re="${F_EDGE_VERB_RE}${F_EDGE_VAR_PRE}${_v}${F_EDGE_VAR_SUF}"
        _re="$_re|^[[:blank:]]*${F_EDGE_VAR_PRE}${_v}${F_EDGE_VAR_SUF}"
        if [ -n "$F_FWD_FN_ALT" ]; then
            _re="$_re|^[[:blank:]]*($F_FWD_FN_ALT)[[:blank:]]+([^\"[:blank:]]+[[:blank:]]+)*${F_EDGE_VAR_PRE}${_v}${F_EDGE_VAR_SUF}"
        fi
        _n="$(grep -cE -- "$_re" "$_sf" || true)"
        if [ "${_n:-0}" -ge 1 ]; then return 0; fi
    done
    return 1
}

# Scratch for the closure: a memo of each directory's derivation, and the
# comment-stripped copy of each node. Under $TMPF, already in _TMPDIRS.
F_CLOSURE_CACHE_DIR="$TMPF/closure-cache"; mkdir -p "$F_CLOSURE_CACHE_DIR"

_f_closure_key() { printf '%s' "$1" | md5sum | cut -d' ' -f1; }

# _f_closure <dir> -> one "<basename> <route>" line per capable node,
# sorted. route is `direct` (a wrapper call site of its own) or
# `via:<basename>` (the capable node it reaches a deadline through).
#
# BASENAMES AND ROUTES ONLY -- never a matched line, same discipline as
# _f_deadline_capable and _e_scan: this file's output is re-emitted into the
# merge-gate verify log.
#
# MEMOIZED THROUGH A FILE, not a shell variable, and that is load-bearing:
# every consumer below reads the derivation inside a `$(...)` command
# substitution, which is a SUBSHELL, so a cache held in a global array would
# be discarded the moment each call returned and the real-tree derivation
# would run once per assert. A file written by the subshell survives it. The
# derivation is a pure function of the directory's contents, and every
# fixture dir is fully written before its first read, so the memo can never
# serve a stale answer within a run; the cache itself lives in a per-run
# mktemp dir, so it cannot survive between runs either.
_f_closure() {
    local _d="$1" _cache
    _cache="$F_CLOSURE_CACHE_DIR/$(_f_closure_key "$_d").closure"
    if [ ! -f "$_cache" ]; then
        _f_closure_compute "$_d" > "$_cache.part"
        mv -f "$_cache.part" "$_cache"
    fi
    cat "$_cache"
}

# _f_closure_compute <dir> -> the derivation proper: a WORKLIST fixed point
# over invocation edges.
#
# Three sets are maintained: CAP (capable node -> route), PEND (nodes not yet
# capable, held as their comment-stripped copies), and DELTA (the nodes that
# became capable in the PREVIOUS round). Each round scans only PEND, and only
# against DELTA -- a node that already failed against the older members
# cannot newly match them, so re-testing them is pure waste. That is the
# difference between this and the naive all-pairs form: measured on the real
# tree, all-pairs cost 8.6s against ~2s for this shape and ~1.4s for the
# seed-only scan that preceded the closure, for identical output.
#
# THREE FILTERS, cheapest first, so the per-round cost is dominated by a
# constant handful of greps rather than by one process per node:
#   1. ONE multi-file `grep -cHE` for the whole round, asking which PEND
#      nodes mention ANY newly-capable node at all. `-c` (never `-q`/`-l`)
#      so every file is drained -- see the SIGPIPE/pipefail note above --
#      and `-H` so the filename prefix is present even when PEND is down to
#      a single file, which is the shape that would otherwise silently
#      reparse a bare count as a path.
#   2. Per survivor, ONE `grep -oE` naming exactly WHICH delta members it
#      mentions, so the edge grammar runs against a shortlist (normally one
#      candidate) instead of the whole of DELTA.
#   3. The edge grammar itself, per shortlisted candidate.
# Filters 1 and 2 are strict supersets of every edge rule (each requires the
# target basename to appear on some line), so they can only ever skip nodes
# that had no edge. The shortlist is sorted, so first-match still picks the
# same route a full sorted DELTA walk would.
#
# TERMINATION, and why the mutually-invoking cycle fixture is not admitted:
# capability propagates only FROM the capable set. A node becomes capable
# only by invoking something ALREADY capable, so a pair that invokes only
# each other never enters CAP, DELTA goes empty, and the loop stops. PEND is
# also strictly non-increasing, which bounds the round count independently.
_f_closure_compute() {
    local _d="$1" _p _base _cand _esc _n _alt _hit _sd _line
    local -A _cap=()
    local -a _pend=() _delta=() _next=() _still=() _short=()

    _sd="$F_CLOSURE_CACHE_DIR/$(_f_closure_key "$_d").stripped"
    rm -rf "$_sd"; mkdir -p "$_sd"

    # SEED round: the UNCHANGED four-ERE direct predicate. Every node is
    # comment-stripped exactly once here, for the whole derivation -- a token
    # mentioned only in a comment neither makes a file deadline-capable nor
    # constitutes an invocation.
    while IFS= read -r _p; do
        [ -n "$_p" ] || continue
        _base="${_p##*/}"
        grep -vE '^[[:blank:]]*#' "$_p" > "$_sd/$_base" || true
        if _f_direct_capable_stripped "$_sd/$_base"; then
            _cap["$_base"]="direct"
        else
            _pend+=("$_sd/$_base")
        fi
    done < <(_f_node_list "$_d")

    # The exec-forwarding-lib classification, hoisted: once per derivation,
    # never per node and never per round.
    _f_scan_fwd_libs "$_d" "$_sd"

    # DELTA starts as the seed set, SORTED. A node reachable from two capable
    # nodes would otherwise report whichever route bash's hash iteration
    # handed back first, making the derived route non-deterministic run to
    # run.
    while IFS= read -r _cand; do
        [ -n "$_cand" ] || continue
        _delta+=("$_cand")
    done < <(printf '%s\n' "${!_cap[@]}" | sort)

    while [ "${#_delta[@]}" -gt 0 ] && [ "${#_pend[@]}" -gt 0 ]; do
        _alt=""
        for _cand in "${_delta[@]}"; do
            _esc="${_cand//./\\.}"
            _alt="${_alt:+$_alt|}$_esc"
        done
        _next=(); _still=()
        while IFS= read -r _line; do
            [ -n "$_line" ] || continue
            _p="${_line%:*}"; _n="${_line##*:}"
            if [ "${_n:-0}" -eq 0 ]; then _still+=("$_p"); continue; fi
            _base="${_p##*/}"
            _short=()
            mapfile -t _short < <(grep -oE -- "($_alt)" "$_p" | sort -u)
            _hit=""
            for _cand in "${_short[@]}"; do
                [ -n "$_cand" ] || continue
                if _f_edge_exists "$_p" "$_cand"; then _hit="$_cand"; break; fi
            done
            if [ -n "$_hit" ]; then
                _cap["$_base"]="via:$_hit"
                _next+=("$_base")
            else
                _still+=("$_p")
            fi
        done < <(grep -cHE -- "($_alt)" "${_pend[@]}" || true)
        _pend=("${_still[@]}")
        _delta=()
        if [ "${#_next[@]}" -gt 0 ]; then
            while IFS= read -r _cand; do
                [ -n "$_cand" ] || continue
                _delta+=("$_cand")
            done < <(printf '%s\n' "${_next[@]}" | sort)
        fi
    done

    for _base in "${!_cap[@]}"; do
        printf '%s %s\n' "$_base" "${_cap[$_base]}"
    done | sort
}

# _f_closure_names <dir> -> the closure's basenames only, sorted. The routes
# are what the route-pinning asserts key on; the plain name list is what the
# count/membership asserts key on.
_f_closure_names() {
    _f_closure "$1" | cut -d' ' -f1
}

# _f_route_of <dir> <basename> -> prints that node's route, or nothing if the
# node is not derived at all. Plain `grep -E` (a PRINTING grep, which drains
# its input) downstream of the pipe, never `-q`/`-l`.
_f_route_of() {
    local _d="$1" _b="$2" _line
    _line="$(_f_closure "$_d" | grep -E -- "^${_b//./\\.} " || true)"
    printf '%s' "${_line#* }"
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
# (the E_POS_VARIANTS idiom), not swallowed by the others. Seven, not five:
# amendment review measured two real false-negative shapes in the original
# five-fixture set (a bind line with a trailing comment; an exec line with
# a flag before the path) and F_BIND_RE/F_EXEC_RE were widened to catch
# them (see their own comments above) -- these two fixtures are what pin
# that widening as a positive control rather than leaving it unverified.
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
cat > "$F_POS_DIR/test_f_pos_bind_trailing_comment.sh" <<'BINDCMTEOF'
#!/usr/bin/env bash
LIB="$REPO_ROOT/scripts/lib_slot_acquire.sh"  # the wrapper
BINDCMTEOF
cat > "$F_POS_DIR/test_f_pos_exec.sh" <<'EXECEOF'
#!/usr/bin/env bash
bash "$SCRIPTS_DIR/cargo-test-occt-gated.sh" true
EXECEOF
cat > "$F_POS_DIR/test_f_pos_exec_flag.sh" <<'EXECFLAGEOF'
#!/usr/bin/env bash
bash -x "$SCRIPTS_DIR/cargo-test-occt-gated.sh" true
EXECFLAGEOF
cat > "$F_POS_DIR/test_f_pos_call.sh" <<'CALLEOF'
#!/usr/bin/env bash
slot_acquire "$LOCK" 1 1
CALLEOF
F_POS_VARIANTS=7

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

assert "F2: positive control -- the derivation flags all $F_POS_VARIANTS grammar variants (WAIT= literal, WAIT= var, bind, bind+trailing-comment, exec, exec+flag, bare call; got $F2_COUNT)" \
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
# Informational ECHO, not a pass/fail assert: this fixture exists to make
# the documented false-positive VISIBLE, not to fail the suite either way.
# An `-eq 1` assert here would invert the moment someone hardens the
# predicate with heredoc-state tracking -- a strict improvement, and the
# natural fix for SCOPE (3) -- since the count would then correctly drop
# to 0 and a pass/fail assert would go RED on a fix, pointing whoever
# lands it at an assert that fails precisely because they fixed the thing
# it documents. Whichever way the count reads, it is reported here so a
# behaviour change is visible without being treated as a regression.
if [ "$F3_HEREDOC_COUNT" -eq 1 ]; then
    echo "F3-known-gap: a wrapper call inside a quoted heredoc body is (mis)flagged today -- accepted, documented false positive (unchanged, got $F3_HEREDOC_COUNT)"
else
    echo "F3-known-gap: heredoc-body false positive no longer reproduces (got $F3_HEREDOC_COUNT, expected 1) -- the predicate's heredoc handling changed; update Section F's SCOPE (3) to match"
fi

echo ""
echo "--- FC1: control on the transitive-closure derivation (one hop) ---"

# CLOSURE CONTROLS, same controls-first discipline F2/F3 hold for the direct
# predicate. Section F's derivation is not only over DIRECT wrapper call
# sites: a suite reaches a finite-WAIT deadline just as surely by INVOKING a
# node that has one -- tests/infra/run_all.sh, whose pool worker calls
# slot_acquire with the finite default REIFY_RUN_ALL_POOL_WAIT=1800
# (run_all.sh:1361), or another suite that is itself capable.
#
# LABELLED FC<n>, not F4+: F4 was a real assert in this section and was
# removed (see the note after F1 below). Reusing the number would make that
# note read as if it described these.
#
# FC1 is the ONE-HOP positive control: a seed node plus three distinct
# invocation-edge shapes. The fixture tree lives under $TMPF, already
# registered in _TMPDIRS, so it is cleaned up with Section F's other
# fixtures.
F_CLOSURE_POS_DIR="$TMPF/fx-closure-pos"; mkdir -p "$F_CLOSURE_POS_DIR"

# (i) SEED suite -- a direct wrapper call site, capable under the four EREs
# above with no closure involved at all.
cat > "$F_CLOSURE_POS_DIR/test_c_seed.sh" <<'C1SEEDEOF'
#!/usr/bin/env bash
slot_acquire "$LOCK" 1 1
C1SEEDEOF
# (ii) run_all.sh -- a seed NODE that is not a test_*.sh suite, and the
# reason invoking run_all.sh is a deadline-capable act. It is a NODE, never a
# roster entry: D_ROSTER lists SUITES forked under the pool.
#
# The body mirrors the route by which the REAL tests/infra/run_all.sh is
# direct-capable, which was MEASURED per-ERE rather than assumed: F_BIND_RE
# at run_all.sh:1036 (_H2_SLOT_ACQUIRE_LIB=...lib_slot_acquire.sh) and
# F_CALL_RE at :1692 (a bare slot_acquire call). NOT F_WAIT_RE: :1361 spells
# `"${REIFY_RUN_ALL_POOL_WAIT:-1800}"`, a `:-` default EXPANSION, and
# F_WAIT_RE requires `_WAIT=`; the only `..._WAIT=` spelling in run_all.sh is
# in a full-line comment at :86, which the derivation strips. The pool-wait
# line is kept below anyway, unmatched, because it is what makes the deadline
# FINITE and a reader looking for it should find it here (esc-6291-3).
cat > "$F_CLOSURE_POS_DIR/run_all.sh" <<'C1RUNALLEOF'
#!/usr/bin/env bash
_H2_SLOT_ACQUIRE_LIB="$_H2_REPO_ROOT/scripts/lib_slot_acquire.sh"
_H2_POOL_WAIT="${REIFY_RUN_ALL_POOL_WAIT:-1800}"
slot_acquire "$_H2_POOL_LOCK" "$_H2_POOL_N" "$_H2_POOL_WAIT"
C1RUNALLEOF
# (iii) EDGE shape 1 -- a node invoked by LITERAL path.
cat > "$F_CLOSURE_POS_DIR/test_c_lit_runall.sh" <<'C1LITEOF'
#!/usr/bin/env bash
bash "$SCRIPT_DIR/run_all.sh" "$D"
C1LITEOF
# (iv) EDGE shape 2 -- a variable BOUND to a node path, then exec'd. The
# real tree's three run_all invokers all take this shape.
cat > "$F_CLOSURE_POS_DIR/test_c_var_runall.sh" <<'C1VAREOF'
#!/usr/bin/env bash
RA="$SCRIPT_DIR/run_all.sh"
bash "$RA" "$D"
C1VAREOF
# (v) EDGE shape 3 -- suite -> SUITE, not suite -> run_all.sh. The real tree
# has this route too (test_verify_env_ambient_isolation.sh invokes
# test_occt_flock_gate.sh), so a run_all-centred closure would miss it.
cat > "$F_CLOSURE_POS_DIR/test_c_suite.sh" <<'C1SUITEEOF'
#!/usr/bin/env bash
bash "$SCRIPT_DIR/test_c_seed.sh"
C1SUITEEOF
# The INVOKER count is held apart from the total so a narrowing edit reports
# how many EDGE VARIANTS it stopped seeing rather than one opaque number
# (the F_POS_VARIANTS/E_POS_VARIANTS idiom above).
F_CLOSURE_POS_VARIANTS=3
F_CLOSURE_POS_EXPECT=$(( F_CLOSURE_POS_VARIANTS + 1 ))

# Counted with the DRAINING `grep -cE ... || true` form, never `grep -q`/`-l`
# downstream of a pipe -- see the SIGPIPE/pipefail note on
# _f_deadline_capable above. Restricted to test_* basenames because
# run_all.sh is a capability NODE, not a roster SUITE. The `|| true` also
# means that while _f_closure_names does not exist yet the count reads 0 and
# this assert FAILS cleanly instead of aborting the suite under `set -e`.
F_FC1_COUNT="$(_f_closure_names "$F_CLOSURE_POS_DIR" | grep -cE '^test_' || true)"

assert "FC1: one-hop closure positive control -- the seed plus all $F_CLOSURE_POS_VARIANTS invocation-edge variants (literal node path, bound-then-exec variable, suite->suite) are derived (expected $F_CLOSURE_POS_EXPECT, got $F_FC1_COUNT)" \
    test "$F_FC1_COUNT" -eq "$F_CLOSURE_POS_EXPECT"

echo ""
echo "--- FC2/FC3: the closure must be a FIXED POINT, and must terminate ---"

# FC1 above proves ONE hop. A real invocation graph is deeper than that
# (test_run_all_ambient_isolation.sh reaches its deadline through
# test_run_all.sh, which reaches it through its own direct call site), and it
# contains cycles. This fixture pins BOTH properties on shapes a single pass
# provably cannot satisfy: a three-edge chain, and a mutually-invoking pair
# that is capable of nothing.
F_CLOSURE_CHAIN_DIR="$TMPF/fx-closure-chain"; mkdir -p "$F_CLOSURE_CHAIN_DIR"

# The chain: seed <- hop1 <- hop2 <- hop3. hop2 deliberately uses the
# bound-then-exec shape rather than a literal path, so the chain also proves
# the bind-plus-exec-position rule composes ACROSS rounds and not merely
# against the direct seeds.
cat > "$F_CLOSURE_CHAIN_DIR/test_c_seed.sh" <<'C2SEEDEOF'
#!/usr/bin/env bash
slot_acquire "$LOCK" 1 1
C2SEEDEOF
cat > "$F_CLOSURE_CHAIN_DIR/test_c_hop1.sh" <<'C2HOP1EOF'
#!/usr/bin/env bash
bash "$SCRIPT_DIR/test_c_seed.sh"
C2HOP1EOF
cat > "$F_CLOSURE_CHAIN_DIR/test_c_hop2.sh" <<'C2HOP2EOF'
#!/usr/bin/env bash
H1="$SCRIPT_DIR/test_c_hop1.sh"
bash "$H1" --quiet
C2HOP2EOF
cat > "$F_CLOSURE_CHAIN_DIR/test_c_hop3.sh" <<'C2HOP3EOF'
#!/usr/bin/env bash
bash "$SCRIPT_DIR/test_c_hop2.sh"
C2HOP3EOF
# The CYCLE: cycA <-> cycB, neither capable of anything. A fixed point that
# propagated on MENTION rather than on CAPABILITY would either loop here or
# wrongly admit both; propagating only FROM the capable set makes this
# terminate with neither admitted.
cat > "$F_CLOSURE_CHAIN_DIR/test_c_cycA.sh" <<'C2CYCAEOF'
#!/usr/bin/env bash
bash "$SCRIPT_DIR/test_c_cycB.sh"
C2CYCAEOF
cat > "$F_CLOSURE_CHAIN_DIR/test_c_cycB.sh" <<'C2CYCBEOF'
#!/usr/bin/env bash
bash "$SCRIPT_DIR/test_c_cycA.sh"
C2CYCBEOF
F_CLOSURE_CHAIN_EXPECT=4

# Routes are PINNED, not just membership: a future edit that resolves a hop
# by the wrong edge -- reaching past its real predecessor to something else
# capable -- would leave membership and the count identical and be invisible
# without this. Precomputed into plain variables, never a $(...) inside a
# description (E1's discipline).
F_FC2_HOP1_ROUTE="$(_f_route_of "$F_CLOSURE_CHAIN_DIR" test_c_hop1.sh)"
F_FC2_HOP2_ROUTE="$(_f_route_of "$F_CLOSURE_CHAIN_DIR" test_c_hop2.sh)"
F_FC2_HOP3_ROUTE="$(_f_route_of "$F_CLOSURE_CHAIN_DIR" test_c_hop3.sh)"
F_FC3_COUNT="$(_f_closure_names "$F_CLOSURE_CHAIN_DIR" | grep -cE '^test_' || true)"

assert "FC2a: hop 1 of the chain is derived through the seed itself (expected via:test_c_seed.sh, got ${F_FC2_HOP1_ROUTE:-<underived>})" \
    test "$F_FC2_HOP1_ROUTE" = "via:test_c_seed.sh"
assert "FC2b: hop 2 is derived through hop 1 -- a second round, and through a BOUND-then-exec edge (expected via:test_c_hop1.sh, got ${F_FC2_HOP2_ROUTE:-<underived>})" \
    test "$F_FC2_HOP2_ROUTE" = "via:test_c_hop1.sh"
assert "FC2c: hop 3 is derived through hop 2 -- a third round, so the derivation is a fixed point and not a bounded number of passes (expected via:test_c_hop2.sh, got ${F_FC2_HOP3_ROUTE:-<underived>})" \
    test "$F_FC2_HOP3_ROUTE" = "via:test_c_hop2.sh"
assert "FC3: the mutually-invoking pair terminates and is NOT admitted -- capability propagates only FROM the capable set, never from a mention (expected $F_CLOSURE_CHAIN_EXPECT derived, got $F_FC3_COUNT)" \
    test "$F_FC3_COUNT" -eq "$F_CLOSURE_CHAIN_EXPECT"

echo ""
echo "--- FC4: the edge predicate must be a CAPABILITY predicate, not a path-MENTION one ---"

# This is F3's discipline extended to the closure, and it is the assert that
# decides whether the derivation is worth having. Every NEGATIVE fixture below
# -- (i) through (v) -- reproduces a shape MEASURED in the real tree, and
# every one of them is admitted by some plausible-looking widening of the edge
# grammar; (vi) is the positive control that keeps their count honest. A
# derivation that admits them would force roster entries whose measured
# justification reads "not actually deadline-capable" -- which empties
# D_ROSTER's meaning, and is exactly why Section F's SCOPE paragraph records
# the `run_all`-alternation fix as measured-and-REJECTED rather than shipped.
F_CLOSURE_NEG_DIR="$TMPF/fx-closure-neg"; mkdir -p "$F_CLOSURE_NEG_DIR"

# The seed NODE, so every fixture below has something real to (not) invoke.
cat > "$F_CLOSURE_NEG_DIR/run_all.sh" <<'C4RUNALLEOF'
#!/usr/bin/env bash
_H2_SLOT_ACQUIRE_LIB="$_H2_REPO_ROOT/scripts/lib_slot_acquire.sh"
_H2_POOL_WAIT="${REIFY_RUN_ALL_POOL_WAIT:-1800}"
slot_acquire "$_H2_POOL_LOCK" "$_H2_POOL_N" "$_H2_POOL_WAIT"
C4RUNALLEOF

# (i) BIND-ONLY -- the node path is bound to a variable and then used only as
# an INSPECTION target. tests/infra/test_verify_release_delta_skip.sh:521 is
# this shape verbatim (ACT_RUN_ALL, used by `test -f` at :523 and two
# source-inspection greps at :530/:536, never invoked), and it is THE
# measured false admission that sank the run_all alternation.
cat > "$F_CLOSURE_NEG_DIR/test_c_neg_bind_only.sh" <<'C4BINDEOF'
#!/usr/bin/env bash
ACT_RUN_ALL="$REPO_ROOT/tests/infra/run_all.sh"
assert "run_all.sh exists" \
    test -f "$ACT_RUN_ALL"
assert "run_all.sh pins the release-delta skip" \
    grep -qE '^[[:space:]]*export REIFY_RELEASE_DELTA_SKIP=0([[:space:]]|$)' "$ACT_RUN_ALL"
C4BINDEOF

# (ii) CASE PATTERN -- an exec verb inside a quoted COMPARISON pattern, which
# is a datum, not a command. tests/infra/test_run_all_ambient_isolation.sh:160
# is this shape verbatim; it is also the measured reason a path-mention
# grammar gets that file's ROUTE wrong (it would report via:run_all.sh, when
# the real route is via:test_run_all.sh through a forwarding helper).
cat > "$F_CLOSURE_NEG_DIR/test_c_neg_case_pattern.sh" <<'C4CASEEOF'
#!/usr/bin/env bash
RUN_ALL_PLAN_LINE=""
while IFS= read -r _line; do
    case "$_line" in
        *"bash tests/infra/run_all.sh"*)
            RUN_ALL_PLAN_LINE="$_line"
            ;;
    esac
done <<< "$PLAN_DUMP"
C4CASEEOF

# (iii) COPY LIST -- the node basename bare in a closure/copy-list loop, the
# same shape F3 already guards for the direct predicate. Note the hazard is
# not the basename but the `sh ` that the PRECEDING list element ends with:
# "verify.sh run_all.sh" contains an unanchored exec verb followed by a
# blank and then the node path.
cat > "$F_CLOSURE_NEG_DIR/test_c_neg_copy_list.sh" <<'C4COPYEOF'
#!/usr/bin/env bash
for _f in verify.sh run_all.sh test_helpers.sh; do
    cp "$SRC/tests/infra/$_f" "$DST/tests/infra/$_f"
done
C4COPYEOF

# (iv) GIT PATHSPEC -- `-- . ':(exclude)<path>'`. The bare `.` here is a git
# pathspec, not a POSIX dot-source, and it is preceded by a blank, so a verb
# set that included `\.` would read this as "source the node". Measured on
# tests/infra/test_orchestrator_config_canonical_path.sh:64-65, which is this
# shape verbatim. It is also a second bind-only instance: `matches` is bound
# from the substitution and then only tested and echoed.
cat > "$F_CLOSURE_NEG_DIR/test_c_neg_pathspec.sh" <<'C4SPECEOF'
#!/usr/bin/env bash
assert_no_legacy_config_refs() {
    local matches
    matches="$(git -C "$REPO_ROOT" grep -nP '(?<!dark-factory-)orchestrator\.yaml' \
        -- . ':(exclude)tests/infra/run_all.sh' || true)"
    if [ -n "$matches" ]; then
        echo "Legacy top-level config references still present (expected: none):"
        return 1
    fi
}
C4SPECEOF

# (v) A REAL INVOCATION OF A NON-CAPABLE NODE. The edge is genuine; the
# target simply has no deadline to confer. Proves the derivation asks
# "capable?" of the TARGET rather than treating any invocation as capability.
cat > "$F_CLOSURE_NEG_DIR/test_c_neg_noncapable_target.sh" <<'C4NCEOF'
#!/usr/bin/env bash
bash "$SCRIPT_DIR/test_c_inert.sh"
C4NCEOF
cat > "$F_CLOSURE_NEG_DIR/test_c_inert.sh" <<'C4INERTEOF'
#!/usr/bin/env bash
echo "inert: this suite reaches no deadline by any route"
C4INERTEOF

# (vi) THE SENTINEL -- one plainly-capable file, so FC4b's count is a
# DISCRIMINATION and not an absence. Without it every fixture in this dir is
# expected to be rejected, and "the grammar correctly rejected all five
# measured shapes" would be indistinguishable from "the derivation returned
# nothing at all here" (an early return out of _f_scan_fwd_libs or
# _f_edge_exists, an empty memo, a mis-set F_CLOSURE_NEG_DIR). Same pairing
# F2/F3 use, and the same reason FC5 keeps its positive and its two negatives
# in ONE fixture dir.
cat > "$F_CLOSURE_NEG_DIR/test_c_neg_control.sh" <<'C4CTRLEOF'
#!/usr/bin/env bash
bash "$SCRIPT_DIR/run_all.sh"
C4CTRLEOF
F_CLOSURE_NEG_EXPECT=1

# Basenames are safe to print; the ADMITTED list is precomputed into a plain
# variable so a RED names exactly which shape got through (E1's discipline).
F_FC4_CTRL_ROUTE="$(_f_route_of "$F_CLOSURE_NEG_DIR" test_c_neg_control.sh)"
F_FC4_COUNT="$(_f_closure_names "$F_CLOSURE_NEG_DIR" | grep -cE '^test_' || true)"
F_FC4_ADMITTED="$(_f_closure_names "$F_CLOSURE_NEG_DIR" | grep -E '^test_' | tr '\n' ' ' | sed 's/ *$//' || true)"

assert "FC4a: positive control -- the one fixture here that DOES invoke the seed node is admitted, so FC4b's count cannot be satisfied by a wholesale derivation failure over this dir (expected via:run_all.sh, got ${F_FC4_CTRL_ROUTE:-<underived>})" \
    test "$F_FC4_CTRL_ROUTE" = "via:run_all.sh"
assert "FC4b: none of the five measured non-invocation shapes is an edge -- bind-only inspection target, exec verb inside a case PATTERN, copy-list element, git pathspec, and a real invocation of a NON-capable node (expected $F_CLOSURE_NEG_EXPECT derived, the sentinel and nothing else, got $F_FC4_COUNT: ${F_FC4_ADMITTED:-<none>})" \
    test "$F_FC4_COUNT" -eq "$F_CLOSURE_NEG_EXPECT"

echo ""
echo "--- FC5/FC6: the second-order route -- a node-bound variable handed to an exec-forwarding helper ---"

# The remaining real route, and the one a per-file two-phase rule cannot see
# on its own. tests/infra/test_run_all_ambient_isolation.sh never execs its
# TARGET (bound to test_run_all.sh at :93); it hands it to
# `ambient_isolation_check_one "$TARGET" ...` (:366), and the
# `bash "$_target" 2>&1` lives in run_all_ambient_isolation_lib.sh:73/:92
# behind `local _target="$1"`.
#
# The rule has to key on the lib ACTUALLY FORWARDING to an exec, not on
# "any argument to any helper" -- otherwise `assert` and every other
# ubiquitous helper becomes an edge. These fixtures are what hold that line:
# a forwarding lib and an inert twin, and a file that sources the forwarding
# lib but only inspects its target.
F_CLOSURE_FWD_DIR="$TMPF/fx-closure-fwd"; mkdir -p "$F_CLOSURE_FWD_DIR"

cat > "$F_CLOSURE_FWD_DIR/test_c_seed.sh" <<'C5SEEDEOF'
#!/usr/bin/env bash
slot_acquire "$LOCK" 1 1
C5SEEDEOF
# The FORWARDING lib: binds a variable from a positional AND execs a
# variable. run_all_ambient_isolation_lib.sh:59/:73 in miniature.
cat > "$F_CLOSURE_FWD_DIR/c_fwd_lib.sh" <<'C5FWDLIBEOF'
#!/usr/bin/env bash
fwd_run_one() {
    local _t="$1" _key="$2"
    local _out
    _out="$(
        export "$_key=1"
        bash "$_t" 2>&1
    )" || return 1
    printf '%s\n' "$_out" > /dev/null
}
C5FWDLIBEOF
# The INERT twin: binds a positional, but never execs it. Must NOT make its
# callers' arguments into edges.
cat > "$F_CLOSURE_FWD_DIR/c_inert_lib.sh" <<'C5INERTLIBEOF'
#!/usr/bin/env bash
inert_check() {
    local _t="$1"
    grep -qE '^#!' "$_t"
}
C5INERTLIBEOF
# POSITIVE: sources the forwarding lib, binds TARGET to the seed, and only
# ever passes it on. test_run_all_ambient_isolation.sh:91/:93/:366 in
# miniature.
cat > "$F_CLOSURE_FWD_DIR/test_c_fwd_pos.sh" <<'C5POSEOF'
#!/usr/bin/env bash
source "$SCRIPT_DIR/c_fwd_lib.sh"
TARGET="$SCRIPT_DIR/test_c_seed.sh"
_check_rc=0
fwd_run_one "$TARGET" "$_key" "$MANIFEST_KEYS" || _check_rc=$?
C5POSEOF
# NEGATIVE: same handoff shape, but to the INERT lib. The rule must key on
# the lib forwarding to an exec, not on the handoff.
cat > "$F_CLOSURE_FWD_DIR/test_c_fwd_neg.sh" <<'C5NEGEOF'
#!/usr/bin/env bash
source "$SCRIPT_DIR/c_inert_lib.sh"
TARGET="$SCRIPT_DIR/test_c_seed.sh"
inert_check "$TARGET"
C5NEGEOF
# NEGATIVE: sources the FORWARDING lib, but never hands it the target --
# only inspects it. Sourcing an exec-forwarding lib is not itself an edge.
cat > "$F_CLOSURE_FWD_DIR/test_c_fwd_neg_inspect.sh" <<'C5NEGINSEOF'
#!/usr/bin/env bash
source "$SCRIPT_DIR/c_fwd_lib.sh"
TARGET="$SCRIPT_DIR/test_c_seed.sh"
test -f "$TARGET"
grep -qE '^#!' "$TARGET"
C5NEGINSEOF
F_CLOSURE_FWD_EXPECT=2

F_FC5_POS_ROUTE="$(_f_route_of "$F_CLOSURE_FWD_DIR" test_c_fwd_pos.sh)"
F_FC5_COUNT="$(_f_closure_names "$F_CLOSURE_FWD_DIR" | grep -cE '^test_' || true)"
F_FC5_DERIVED="$(_f_closure_names "$F_CLOSURE_FWD_DIR" | grep -E '^test_' | tr '\n' ' ' | sed 's/ *$//' || true)"

assert "FC5a: a node-bound variable passed to an EXEC-FORWARDING helper is an edge (expected via:test_c_seed.sh, got ${F_FC5_POS_ROUTE:-<underived>})" \
    test "$F_FC5_POS_ROUTE" = "via:test_c_seed.sh"
assert "FC5b: the same handoff to an INERT lib, and sourcing a forwarding lib without handing it the target, are NOT edges (expected $F_CLOSURE_FWD_EXPECT derived, got $F_FC5_COUNT: ${F_FC5_DERIVED:-<none>})" \
    test "$F_FC5_COUNT" -eq "$F_CLOSURE_FWD_EXPECT"

# REAL-TREE ROUTE PINS. Membership alone would not catch a derivation that
# reached the right file by the wrong edge, and for this file that is not
# hypothetical: a path-mention grammar derives
# test_run_all_ambient_isolation.sh via:run_all.sh off the `case`-pattern
# comparison at :160, which is a datum, not a call. Pinning the route is what
# makes the roster entry's recorded justification checkable.
F_FC6_AMB_ROUTE="$(_f_route_of "$SCRIPT_DIR" test_run_all_ambient_isolation.sh)"
F_FC6_VENV_ROUTE="$(_f_route_of "$SCRIPT_DIR" test_verify_env_ambient_isolation.sh)"

assert "FC6a: test_run_all_ambient_isolation.sh derives by its REAL second-order route, not off the case-pattern line (expected via:test_run_all.sh, got ${F_FC6_AMB_ROUTE:-<underived>})" \
    test "$F_FC6_AMB_ROUTE" = "via:test_run_all.sh"
assert "FC6b: test_verify_env_ambient_isolation.sh derives through the suite it actually invokes (expected via:test_occt_flock_gate.sh, got ${F_FC6_VENV_ROUTE:-<underived>})" \
    test "$F_FC6_VENV_ROUTE" = "via:test_occt_flock_gate.sh"

# REAL-TREE NEGATIVE PIN. This is the one false admission the whole two-phase
# rule exists to prevent, so it is pinned rather than left to F1: F1 would
# also go RED on it, but only as an unexplained extra roster entry, and the
# natural repair -- declaring it -- would silently produce a member whose
# measured justification reads "not actually deadline-capable". Named here,
# a future widening that re-admits it fails against a stated reason.
#
# FC7a BEFORE FC7b, the same ordering and for the same reason as D4a before
# D4b. `_f_route_of` prints the empty string for a node that is derived with
# no route (impossible) AND for a node that is not in the node set AT ALL, so
# FC7b's `-z` alone would go green for the wrong reason the moment
# test_verify_release_delta_skip.sh is renamed, deleted, or simply stops
# binding run_all.sh -- and the file's one real-tree pin against re-admitting
# the bind-only shape would evaporate silently. FC7a keys on the bind line
# itself (measured today at :521, inspected at :523/:530/:536), so any of
# those three goes RED here instead. Counts only, never the matched line
# (D1's discipline); stderr is discarded so a vanished file reports through
# the assert rather than as noise in the merge-gate verify log.
F_FC7_DELTA_BIND="$(grep -cE '^[[:blank:]]*ACT_RUN_ALL=[^[:blank:]]*/run_all\.sh' \
    "$SCRIPT_DIR/test_verify_release_delta_skip.sh" 2>/dev/null || true)"
F_FC7_DELTA_ROUTE="$(_f_route_of "$SCRIPT_DIR" test_verify_release_delta_skip.sh)"
# REAL-TREE NON-VACUITY PIN. run_all.sh being a DIRECT seed is what makes
# three of the five transitive members capable at all. If it silently stopped
# seeding, those three would vanish from the derivation -- and F1 would stay
# green against a correspondingly shrunken declaration, which is exactly the
# failure mode D4a's own non-vacuity assert exists to catch one level down.
F_FC7_RUNALL_ROUTE="$(_f_route_of "$SCRIPT_DIR" run_all.sh)"

assert "FC7a: test_verify_release_delta_skip.sh still BINDS run_all.sh, so FC7b is pinning the bind-only shape and not an absent file (non-vacuity; got ${F_FC7_DELTA_BIND:-0})" \
    test "${F_FC7_DELTA_BIND:-0}" -ge 1
assert "FC7b: test_verify_release_delta_skip.sh is NOT derived -- it binds run_all.sh purely as an inspection target, and admitting it is the measured failure that sank the run_all alternation (expected <underived>, got ${F_FC7_DELTA_ROUTE:-<underived>})" \
    test -z "$F_FC7_DELTA_ROUTE"
assert "FC7c: run_all.sh is derived as a DIRECT seed node, so the three members that reach a deadline only through it are not vacuously absent (expected direct, got ${F_FC7_RUNALL_ROUTE:-<underived>})" \
    test "$F_FC7_RUNALL_ROUTE" = "direct"


echo ""
echo "--- F1: the declared deadline-capable roster must equal the derived one ---"

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
# The derived side now spans DIRECT call sites AND their transitive
# invocation closure, so a green here IS the full roster claim over the node
# set (tests/infra/test_*.sh plus run_all.sh). It is still not completeness
# over the residual routes SCOPE (2) names -- the scripts/ hop, POSIX
# dot-source, and dynamically constructed invocations -- which is why the
# description scopes itself to "over tests/infra/" rather than claiming no
# suite anywhere can reach a deadline undeclared.
assert "F1: the DERIVED roster of deadline-capable suites over tests/infra/ (direct wrapper call sites plus their invocation closure) equals the DECLARED one (unlisted: ${F_UNLISTED:-<none>}) (stale: ${F_STALE:-<none>})" \
    test "$F_DERIVED" = "$F_DECLARED_SORTED"

echo ""
# F4 (bidirectional D_MEMBERS<->D_ROSTER_MODE equality plus index-
# alignment, three asserts) was REMOVED here (amendment, task 6255 review):
# once D_ROSTER_MODE is DERIVED from D_MEMBERS membership (declared above)
# instead of hand-typed, the two cannot diverge by construction -- there is
# nothing left for a lockstep check to catch. The one part of F4's job the
# derivation does not give for free -- that D_MEMBERS' own entries are
# genuine deadline-capable suites, not typos or stale names -- is already
# proved by F1 above, since a D_MEMBERS entry that were NOT derivable at all
# would still need to appear in D_ROSTER for F1 to pass, and D_MEMBERS' three
# members do appear there. Note the exact strength of that: since task 6291
# the derivation admits a suite by a DIRECT call site OR by the invocation
# closure, so F1 proves those three are capable, not that they are capable
# DIRECTLY. Nothing here depends on which -- Section D forks them and
# observes real behaviour -- and their direct sites are what seeds the
# closure in the first place.

test_summary
