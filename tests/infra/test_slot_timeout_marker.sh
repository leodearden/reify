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
#       captured-output dump (tests/infra/test_helpers.sh) stays empty.
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

# dark-factory's OTHER slot anchor, the per-wrapper deadline line, as a POSIX
# ERE. Transcribed ONCE, here, because two sections read it: A6d (synthetic,
# over the rules run_all.sh actually applies) and Section H2/H3 (behavioural,
# over real re-emitted output).
#
# TRANSCRIPTION of the live `_SLOT_ACQUIRE_DEADLINE_RE` in
# orchestrator/src/orchestrator/verify_classify.py, with THREE deliberate
# spelling differences -- enumerated in full, because a transcription that
# UNDERSTATES how far it diverges is worse than one that diverges. Each is
# verified to preserve match semantics over all four grounded shapes (bare,
# indented, `ERROR: `-prefixed, `within unlimiteds`):
#   1. `[[:blank:]]*` for `^[ \t]*` -- GNU grep -E does not honour `\t` inside
#      a bracket expression (the same trap Section D's D_ANCHOR records).
#   2. `([^[:alnum:]]|$)` for the trailing `\b` -- if anything LOOSER, which is
#      the safe direction for an assertion that a count is ZERO.
#   3. `.{1,40}` for DF's `.{1,40}?`. POSIX ERE HAS NO LAZY QUANTIFIER, so the
#      `?` cannot be carried across: GNU grep -E parses `{1,40}?` as an
#      OPTIONAL group, `(.{1,40})?`, which also matches ZERO characters -- a
#      silent and much larger looseness than the two above. Dropping it is the
#      FAITHFUL reading, not a shortcut: greedy vs. lazy changes only WHICH
#      span a backtracking engine settles on, never WHETHER a match exists, and
#      Python's `.{1,40}?` requires >= 1 character exactly as `.{1,40}` does.
#      All four grounded shapes carry 9-11 characters in that span.
#
# CROSS-REPO: a transcription, never the source of truth. If either side moves
# -- DF's allowlist growing past three basenames, or a wrapper's message
# changing -- this anchor AND run_all.sh's $_RA_SLOT_BASENAME_SANITIZE must
# both be re-verified against verify_classify.py.
H_DF_ANCHOR='^[[:blank:]]*(ERROR: )?(lib_test_semaphore|cargo-test-occt-gated|lib_lane_x_flock)\.sh: failed to acquire .{1,40} within [^[:space:]]+s([^[:alnum:]]|$)'

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
echo "--- A6: what run_all.sh's re-emission sanitizer does to this family (drift guard) ---"

# DERIVED from run_all.sh, never hardcoded -- that is what makes this a guard
# rather than a restatement. For a given sanitizing FUNCTION it recovers the
# ORDERED list of rule VARIABLE NAMES that function actually applies, then
# resolves each name to its own `^<NAME>='...'` definition. Keying off the
# parsed chain (not a hardcoded name, and not a hardcoded count) is the whole
# point: a FOURTH sibling rule added tomorrow is picked up automatically and
# cannot bypass this guard.
#
# TWO SPELLINGS are resolved, so a refactor between them does not blind this:
# an INLINE `sed -e "$NAME" -e "$NAME"` chain in the function body, and a
# reference to the hoisted `"${_RA_SANITIZE_SED[@]}"` array (whose own
# definition is then parsed for the same names). run_all.sh currently uses the
# hoisted form; the inline form is what it used when this guard was written.
#
# AND BOTH SITES, not one. run_all sanitizes in TWO places -- the per-member
# replay (_ra_emit_sanitized) and the Summary FAILED region
# (_ra_collect_fail_detail, which is what verify.sh / DF's merge-gate block
# reason quote verbatim). Reading only the first left exactly the false green
# this assertion is otherwise designed to catch: a fourth rule wired into
# _ra_emit_sanitized and forgotten at the collector would satisfy an
# emit-only arity check while the collector shipped a gap, and Section H
# asserts only the two SLOT rules' effects. A6e compares the two derived
# lists directly, which is what makes "the two paths cannot drift" a
# machine-checked property rather than a claim in a comment.
#
# WHY THAT MATTERS, historically: this assertion used to extract exactly one
# name, _RA_CLOCK_SANITIZE, and assert the sentinel survived it. Its comment
# warned only about BROADENING that one rule to `s/@@REIFY_/.../`. Task 6389
# then added two SIBLING rules (_RA_SLOT_SANITIZE, _RA_SLOT_BASENAME_SANITIZE),
# which walked straight past the guard: A6 would have gone on asserting "the
# sentinel survives" while _ra_emit_sanitized no longer let it. A6 is the only
# thing in this repo that reads the sanitizer out of source, so a false green
# here is silent.
#
# THE POST-CHANGE TRUTH about the cross-repo seam, which A6 no longer states
# and must not be read as stating: survival of run_all's OWN pool-wait sentinel
# is guaranteed by FD-2 ROUTING -- the pool worker writes to the inherited
# parent fd 2, so it never enters this re-emission path at all -- and NOT by
# the sanitizer's prefix scope. That is pinned behaviourally, on real run_all
# output, by C1/C2/C5. A6 now pins something narrower and still worth pinning:
# exactly WHICH rules run, and what each of them does to this family.

# _a6_rules_of <function-name> -> one rule VARIABLE NAME per line, in the
# order that function applies them. Empty output means "derived nothing",
# which A6a/A6e surface as RED rather than as a silent pass.
_a6_rules_of() {
    local _a6_fn="$1" _a6_src
    _a6_src="$(sed -n "/^${_a6_fn}() {\$/,/^}\$/p" "$RUN_ALL")"
    if printf '%s\n' "$_a6_src" | grep -q -- '_RA_SANITIZE_SED\[@\]'; then
        _a6_src="${_a6_src}
$(sed -n 's/^_RA_SANITIZE_SED=(\(.*\))$/\1/p' "$RUN_ALL" | head -1)"
    fi
    printf '%s\n' "$_a6_src" \
        | grep -oE -- '-e "\$[A-Za-z_][A-Za-z0-9_]*"' \
        | sed 's/^-e "\$//; s/"$//' || true
}

A6_EMIT_NAMES="$(_a6_rules_of _ra_emit_sanitized)"
A6_DETAIL_NAMES="$(_a6_rules_of _ra_collect_fail_detail)"

A6_RULE_NAMES=()
while IFS= read -r _a6_n; do
    [ -n "$_a6_n" ] || continue
    A6_RULE_NAMES+=("$_a6_n")
done < <(printf '%s\n' "$A6_EMIT_NAMES")

# Resolve each parsed NAME to its definition. A name that resolves to nothing
# is dropped here and shows up as an arity mismatch in A6a -- which is exactly
# how an unwired or renamed rule is caught.
A6_EXPRS=()
for _a6_n in "${A6_RULE_NAMES[@]+"${A6_RULE_NAMES[@]}"}"; do
    _a6_e="$(sed -n "s/^${_a6_n}='\(.*\)'\$/\1/p" "$RUN_ALL" | head -1)"
    [ -n "$_a6_e" ] || continue
    A6_EXPRS+=("$_a6_e")
done
A6_N_NAMES="${#A6_RULE_NAMES[@]}"
A6_N_EXPRS="${#A6_EXPRS[@]}"

A6_SED_ARGS=()
for _a6_e in "${A6_EXPRS[@]+"${A6_EXPRS[@]}"}"; do A6_SED_ARGS+=(-e "$_a6_e"); done
# Identity fallback so a derivation failure surfaces as A6a's RED rather than
# as a sed usage error that aborts the suite.
[ "$A6_N_EXPRS" -gt 0 ] || A6_SED_ARGS=(-e 's/^//')

_a6_arity_ok() {  # <resolved> <parsed> <minimum>
    [ "$1" -eq "$2" ] && [ "$1" -ge "$3" ]
}

assert "A6a: every rule _ra_emit_sanitized applies resolved to a definition, and at least the three expected are wired (parsed $A6_N_NAMES, resolved $A6_N_EXPRS)" \
    _a6_arity_ok "$A6_N_EXPRS" "$A6_N_NAMES" 3

# A6e: THE ANTI-DRIFT ASSERTION. Both sanitizing sites must apply the same
# rules in the same order. Non-vacuity is carried by A6a above (the emit list
# is non-empty and >= 3), so an equal-but-empty pair cannot pass this pair of
# assertions together; A6e additionally requires the collector's own list to
# be non-empty so an unparseable collector is RED here and not just equal-to-
# nothing. Counts only in the description, per Section E discipline.
A6_N_DETAIL="$(printf '%s\n' "$A6_DETAIL_NAMES" | grep -c . || true)"
_a6_same_chain() {  # <emit-list> <detail-list>
    [ -n "$1" ] && [ "$1" = "$2" ]
}
assert "A6e: _ra_collect_fail_detail applies the SAME ordered rule chain as _ra_emit_sanitized (emit $A6_N_NAMES, collector $A6_N_DETAIL)" \
    _a6_same_chain "$A6_EMIT_NAMES" "$A6_DETAIL_NAMES"

# A6b, PRESERVED VERBATIM IN INTENT: the CLOCK rule alone, extracted by its
# exact name, still leaves this family untouched at column 0. It is what keeps
# _RA_CLOCK_SANITIZE prefix-scoped to `@@REIFY_CLOCK_`, and it still turns RED
# on the `s/@@REIFY_/.../` broadening its own rationale block warns about --
# a broadening that would collapse three separately-scoped, separately-
# documented rules into one and silently take the basename half with it.
RA_SANITIZE_EXPR="$(sed -n "s/^_RA_CLOCK_SANITIZE='\(.*\)'\$/\1/p" "$RUN_ALL" | head -1)"
A6_OUT="$TMPA/a6.out"
printf '%sTIMEOUT@@ reason=run_all_pool_starvation slots=1 waited=3 disposition=soft lock=/tmp/x.lock\n' "$SP" \
    | sed "${RA_SANITIZE_EXPR:-s/^//}" > "$A6_OUT"

assert "A6b: the CLOCK rule alone leaves the sentinel unrewritten, still at column 0 (its prefix scope)" \
    _has_line "$A6_OUT" "^${SP}TIMEOUT@@ "

# A6c/A6d: the NEW contract, over the FULL derived chain -- both halves of
# dark-factory's slot classification are neutralized by the rules run_all
# actually applies. Synthetic and cheap; Section H proves the same properties
# behaviourally, on real re-emitted output, which is what makes these two a
# fast drift guard rather than the primary evidence.
A6_FULL_OUT="$TMPA/a6-full.out"
printf '%sTIMEOUT@@ reason=run_all_pool_starvation slots=1 waited=3 disposition=soft lock=/tmp/x.lock\n' "$SP" \
    | sed "${A6_SED_ARGS[@]}" > "$A6_FULL_OUT"

A6_DEADLINE_OUT="$TMPA/a6-deadline.out"
printf 'lib_test_semaphore.sh: failed to acquire test slot within 0s (LOCK=/tmp/l, N=1)\n' \
    | sed "${A6_SED_ARGS[@]}" > "$A6_DEADLINE_OUT"
A6D_N="$(grep -acE -- "$H_DF_ANCHOR" "$A6_DEADLINE_OUT" || true)"

assert "A6c: the sentinel piped through the FULL applied chain IS rewritten to the quoted form" \
    _has_text "$A6_FULL_OUT" "@@REIFY_QUOTED_SLOT_TIMEOUT@@"
assert "A6d: a basename deadline line through the same chain no longer matches DF's anchor (got $A6D_N)" \
    test "$A6D_N" -eq 0

echo ""
echo "--- A6f/A6g: run_all's basename allowlist vs. reify's ACTUAL deadline emitters ---"

# $_RA_SLOT_BASENAME_SANITIZE hardcodes dark-factory's THREE-BASENAME allowlist
# into reify. Two sets have to agree for that rule to keep working, and only
# one of them is checkable from here:
#   - DF's allowlist vs. this file's $H_DF_ANCHOR: a CROSS-REPO obligation, and
#     it stays a documented manual re-verification (see $H_DF_ANCHOR's note) --
#     nothing in this repo can read verify_classify.py.
#   - DF's allowlist vs. REIFY's own emitters: checkable, and left unchecked it
#     is the live half of the risk. If a fourth reify wrapper starts emitting
#     `<basename>.sh: failed to acquire ` and DF's allowlist grows to match it,
#     that new emitter walks straight past this rule and reintroduces exactly
#     the esc-5623 misclassification the rule exists to close -- silently,
#     because every existing assertion here is about the three names that DO
#     match. A6f/A6g turn "someone will remember to widen the alternation"
#     into a RED test on the day the fourth emitter lands.
#
# Both sides are DERIVED, never listed: the allowlist is parsed back out of the
# rule's own BRE alternation, and the emitter set out of scripts/*.sh. Scoped
# to scripts/ deliberately -- run_all.sh's own H9 Lane-X line uses basename
# `run_all.sh`, is outside DF's allowlist BY DESIGN, and must not be swept in.
A6F_EXPR="$(sed -n "s/^_RA_SLOT_BASENAME_SANITIZE='\(.*\)'\$/\1/p" "$RUN_ALL" | head -1)"
_a6f_alt="${A6F_EXPR#*'\('}"
_a6f_alt="${_a6f_alt%%'\)'*}"
A6F_ALLOWLIST="$(printf '%s\n' "$_a6f_alt" | sed 's/\\|/\n/g' | sed '/^$/d' | sort -u)"

# The emitter side: a non-comment `echo "<basename>.sh: failed to acquire `,
# with or without the `ERROR: ` prefix cargo-test-occt-gated.sh uses.
A6F_EMITTERS="$(grep -rhoE -- '^[^#]*echo "(ERROR: )?[a-z][a-z0-9_.-]*\.sh: failed to acquire ' "$SCRIPTS_DIR"/*.sh 2>/dev/null \
    | grep -oE -- '[a-z][a-z0-9_.-]*\.sh: failed to acquire' \
    | sed 's/\.sh: failed to acquire$//' | sort -u || true)"

A6F_N_ALLOW="$(printf '%s\n' "$A6F_ALLOWLIST" | grep -c . || true)"
A6F_N_EMIT="$(printf '%s\n' "$A6F_EMITTERS" | grep -c . || true)"

# A6f is the non-vacuity precondition for A6g: two empty sets are equal, so a
# derivation that silently stopped matching would otherwise pass forever.
_a6f_both_derived() {  # <n-allow> <n-emit> <minimum>
    [ "$1" -ge "$3" ] && [ "$2" -ge "$3" ]
}
_a6f_sets_equal() { [ "$1" = "$2" ]; }

assert "A6f: both sides derived non-vacuously (allowlist $A6F_N_ALLOW, scripts/ emitters $A6F_N_EMIT, expected >= 3)" \
    _a6f_both_derived "$A6F_N_ALLOW" "$A6F_N_EMIT" 3
assert "A6g: run_all's basename allowlist is exactly reify's set of deadline-line emitters (allowlist $A6F_N_ALLOW, emitters $A6F_N_EMIT)" \
    _a6f_sets_equal "$A6F_ALLOWLIST" "$A6F_EMITTERS"

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
# ONE invisible fact makes it work, and it is pinned here behaviourally:
#   the pool worker's slot_acquire writes to the INHERITED parent fd 2 -- the
#   `> .out 2>&1` redirect is scoped to the member `bash` command only -- so
#   the marker never enters the sanitized re-emission path at all.
#
# That FD-2 ROUTING is now the SOLE guarantee. Until task #6389 there was a
# second one -- run_all's sanitizer was prefix-scoped to `@@REIFY_CLOCK_` and
# structurally could not rewrite this family -- and that half is GONE:
# $_RA_SLOT_SANITIZE rewrites exactly this prefix in member-captured output.
# So C1/C2/C5 are no longer belt-and-braces; they are the only thing standing
# between a routing change in run_all's pool worker and a silently
# unclassifiable starved pool wait. A6 no longer pins any part of this: it now
# pins WHICH rules run and what they do (including that they DO rewrite this
# family), which is the opposite claim.
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
# UNCONDITIONALLY (run_all.sh:1879/1893) -- and until task #6389 that re-emission
# passed through a sanitizer prefix-scoped to `@@REIFY_CLOCK_`, so such a sentinel
# survived verbatim at column 0 into the merge-gate verify log and dark-factory's
# presence-anchored classifier marked the ENTIRE merge verify as
# SEMAPHORE_TIMEOUT. That is precisely the infra-hold misclassification this task
# exists to remove, reintroduced by the fix.
#
# TWO LAYERS NOW, and this section is the FIRST one. Task #6389 added
# $_RA_SLOT_SANITIZE / $_RA_SLOT_BASENAME_SANITIZE (run_all.sh:404/445, applied
# at both re-emission sites, pinned by Section H), so a member-captured sentinel
# no longer survives re-emission. That is a SYSTEMIC BACKSTOP for members nobody
# has audited -- it is not a reason to stop diverting stderr at source. Per-site
# diversion remains the first line of defence for reasons the backstop cannot
# cover: it keeps the member's OWN capture clean (so its own failure output, and
# any nested run_all it drives, are readable), it is what the F/G roster
# machinery can actually enforce statically, and it does not depend on a
# consumer-shaped rewrite staying in sync with a cross-repo regex. Same
# two-layer framing the clock family uses about tasks 4802/4887/4931.
# Sections A-C hold that discipline for THIS file's own emissions; D extends
# it to the emit-adjacent sites this change turned from latent into live.
#
# D_MEMBERS below is NO LONGER the source of truth for "which suites can leak"
# -- task 6255 made it the BEHAVIOURAL SUBSET of the deadline-capable roster
# Section F derives (D_ROSTER -- direct call sites, plus their transitive
# invocation closure since task 6291). D_ROSTER also lists NINE static-only
# members (test_run_all.sh, test_slot_event_log.sh, test_verify_semaphore_e2e.sh
# and the six transitive members -- five from 6291, one from 7106). Their
# per-SITE stderr diversion IS machine-checked now, by SECTION G at the end of
# this file (task 6278): G1 proves every deadline-capable site in each of the
# nine diverts stderr off the inherited fd 2, G3 that no member passes
# vacuously, and G0 that those nine remain the WHOLE static-only slice as the
# roster grows.
# D4 deliberately stays what it is -- the SECTION-SLICED, EVIDENCE-PRESERVING
# arm over the three behavioural D_MEMBERS. Section G asserts the weaker LEAK
# property over whole files, and Section G's own preamble records why the two
# grammars must stay apart rather than being unified.
# STILL ONLY MEASURED IN PROSE, beside D_ROSTER_MODE: the OTHER channel. Two of
# the transitive members are measured NOT clean on the bare-variable
# echo/description path -- recorded there in full, filed separately, and
# outside what Section G asserts.
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
    # member capture (`bash "$INFRA_DIR/$name" > "<n>.out" 2>&1`, run_all.sh:1785),
    # which is the stream Phase 3 re-emits. Capturing DIRECTLY rather than
    # through a nested run_all is deliberate, and since task #6389 it is also
    # what makes this arm strictly stronger: run_all's sanitizer now DOES
    # rewrite this token family on re-emission, so a nested run_all would mask
    # exactly the leak D1 exists to find. This arm asserts the member's own
    # capture is clean at source -- the property the backstop cannot give.
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

# _d_join_logical <file> -> that file's LOGICAL command lines, one per line.
# THE one logical-line builder, shared by D4's section slicer below and by
# Section G's whole-file scan -- deliberately not two near-identical copies,
# the drift hazard this file already records beside _f_direct_capable_stripped
# (a future tightening could land on the dead copy and look like it had taken
# effect).
#
# Continuations are joined FIRST, then comment / assert lines are dropped. Both
# halves and their ORDER are load-bearing:
#   - joining first is what makes each logical command ONE line: HG-2 puts its
#     `2>` on the continuation line, not on the line naming the acquire, and a
#     line-at-a-time scan would call that unredirected;
#   - stripping after the join is what removes a WHOLE `assert "..." \` +
#     continuation as one unit. Both name an entry point in PROSE without
#     invoking it (test_test_run_semaphore.sh:859 is exactly that shape).
#     MEASURED, and the reason the order is not free to flip: stripping BEFORE
#     the join deletes the `assert` line but leaves its continuation orphaned at
#     top level -- 75 such lines in test_run_all.sh, 81 in
#     test_verify_semaphore_e2e.sh, 27 in test_slot_event_log.sh -- each a
#     candidate false positive for Section G. The mirror hazard of joining first
#     (a full-line comment ending in `\` swallowing the real line after it)
#     occurs ZERO times across every file either arm scans.
_d_join_logical() {  # <file> -> logical command lines
    sed -e :a -e '/\\$/N; s/\\\n//; ta' "$1" \
        | grep -vE '^[[:blank:]]*(#|assert )' || true
}

_d_section_cmds() {  # <member-file> <output-header-anchor> -> that section's commands
    # The logical lines, sliced header-to-next-header. Output is byte-identical
    # to the pre-refactor inline pipeline over every section D4 slices today.
    local _src
    _src="echo \"${2#^}"
    _d_join_logical "$1" \
        | awk -v s="$_src" '
            !inb && index($0, s) { inb = 1; next }
            inb && /^echo "---/ { exit }
            inb { print }
          ' || true
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
# assert() (tests/infra/test_helpers.sh) USED TO echo "  PASS: $desc" /
# "  FAIL: $desc" with NO sanitizing, indenting only the CHECKER's captured
# output in its on-FAIL dump. Anything inside $desc printed RAW -- so lines 2+
# of a multi-line interpolation started at COLUMN 0, on a PASSING assertion, and
# a captured stderr carrying a live sentinel reached the verify log through a
# GREEN assert. That emitter half is now closed structurally -- see THE
# STRUCTURAL FIX below -- and E4 at the end of this section is its behavioural
# pin. (Cited by SYMBOL, not line number: every line-number cite into
# test_helpers.sh this preamble used to carry went stale the moment that fix
# landed. _assert_emit_desc and assert's on-FAIL capture dump survive edits.)
#
# What the fix does NOT change, and what E1 below therefore still exists for:
# a `$(cat ...)` inside a description is EVALUATED on every run even when only
# the printing is conditional, and it is a shape a reviewer should not have to
# reason about at all. The channel is also not observable on demand -- it prints
# only when that specific assert runs, under the contention that produced the
# deadline (test_test_run_semaphore.sh's HG-2 deadlines only when the parent
# verify.sh holds the host-global lock, which is exactly the merge-gate case).
# So E1 is STATIC, in the same genre as the existing
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
# OWN stdout (_ra_emit_sanitized, run_all.sh:455-458), so a nested run_all's
# stdout/combined capture carries member STDERR bytes -- a leaked sentinel
# among them.
#
# BOUNDED BY DESIGN, and the assertion name says which bound: this flags a
# whole-file DUMP, not every conceivable interpolation. A FILTERED reader
# ($(sed ...) -- the sanctioned form, E3 -- or $(grep ...)) is out of grammar
# because the filter is where the `  | ` prefix belongs, and a variable holding
# text captured on an earlier line is out of grammar because there is no reader
# to key on. Same-line only, which is what every site in-tree uses; a
# description split across a `\`-continuation is out of the grammar.
#
# THE STRUCTURAL FIX HAS LANDED (task 6353). assert() itself now emits $desc
# through _assert_emit_desc (tests/infra/test_helpers.sh), which keeps line 1
# byte-identical and gives lines 2+ the same `  | ` prefix it already applies
# to a failing checker's captured output. That closes every variant at once,
# including the bare-variable shapes E1 is structurally unable to see, and
# demotes E1 to exactly the belt-and-braces this preamble predicted it would
# become. E1 is KEPT anyway: it catches the `$(cat …)` shape at AUTHORING time,
# which is a reviewable static signal a behavioural pin cannot give. E4 below
# is the behavioural half of the pair.
#
# OPTION (c) -- widening E1 to bare-variable dumps -- WAS MEASURED AND
# REJECTED, recorded here so it is not re-derived. At the naive grammar (an
# emit-wired line carrying a bare `$VAR` whose name ends _ERR/_STDERR/_OUT/
# _STDOUT) it yields 708 candidate lines across 58 files. Narrowed to "assigned
# from a `$(...)` containing `2>&1` and re-emitted with no reader present" it
# still yields 271 across 32 files, 162 of them in test_run_all.sh alone.
# Narrowing further to "the captured child is deadline-capable" is defeated by
# tests/infra/run_all_ambient_isolation_lib.sh's own `bash "$_target" 2>&1`,
# where the child is a PARAMETER and no path-literal predicate can see it --
# the same second-order indirection Section F's SCOPE (2) already measured and
# rejected for F_EXEC_RE. A repo-wide static lint for that channel is not
# tractable at acceptable false-positive cost; the structural fix above is what
# closes it instead. This is a MEASURED REJECTION, not deferred work.
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
echo "--- E4: assert() sanitizes a multi-line description (structural closure) ---"

# E1 above is a STATIC lint keyed on a READER token (`$(cat …)`), so it cannot
# see the bare-variable shape: a variable holding text captured on an EARLIER
# line has no reader to key on. E4 is the BEHAVIOURAL pin that closes that gap
# from the other side -- it drives the real assert() and checks the emitted
# bytes against D_ANCHOR.
#
# OVERLAP, stated plainly rather than overclaimed. tests/infra/test_test_helpers.sh
# (checks 6353-c1/c2) already drives the real assert() with a multi-line
# sentinel-shaped description and asserts the same absence, spelling the
# `^[[:blank:]]*` anchor literally. E4 is therefore NOT the only pin of that
# property, and the two things it does add are the whole reason it lives in
# THIS file:
#   - E4b is a LIVENESS control for the ANCHOR ITSELF: it rebuilds the pre-6353
#     raw emitter and requires the probe to FLAG it. Its counterpart controls
#     only for the token being PRESENT (6353-c1); nothing there proves the
#     anchored pattern would fire on an unfiltered emitter, so a typo'd anchor
#     would read green forever. E4b is that proof.
#   - E4 evaluates against D_ANCHOR -- this file's SINGLE spelling of
#     dark-factory's classifier anchor, defined once beside the
#     `[[:blank:]]`-not-`\t` gotcha that makes spelling it by hand hazardous.
#     Binding the behavioural claim to that definition means a change to the
#     anchor propagates here automatically, instead of leaving a hand-copied
#     literal in a sibling silently stale.
# A future change that prefixed with plain INDENTATION would satisfy a
# prefix-shaped reading of the contract and still fail here, which is exactly
# the distinction D2b and the SELF-POLLUTION DISCIPLINE at the top of this file
# exist to make.
#
# Hermetic: no lock, no cargo, no host state -- this file is classified `pool`.

E4_DIR="$TMPE/e4"; mkdir -p "$E4_DIR"
# The pre-6353 emitter, rebuilt at runtime, is E4b's positive control. BUILT AT
# RUNTIME, like E2/E3's fixtures, so this file carries no literal that E1's own
# repo-wide scan could trip over.
E4_RAW_EMITTER="$E4_DIR/raw_emitter.sh"
cat > "$E4_RAW_EMITTER" <<'E4RAWEOF'
# Reproduction of the PRE-6353 assert(): $desc echoed RAW, so lines 2+ of a
# multi-line description land at COLUMN 0. E4b drives this and requires the
# probe to FLAG it -- without that, E4a's zero could be a dead instrument
# rather than a real result. Deliberately minimal: only the emitting shape
# matters, so no tmpfile capture and no on-FAIL dump.
PASS=0
FAIL=0
assert() {
    local desc="$1"
    shift
    if "$@" >/dev/null 2>&1; then
        echo "  PASS: $desc"
        PASS=$((PASS + 1))
    else
        echo "  FAIL: $desc"
        FAIL=$((FAIL + 1))
    fi
}
E4RAWEOF

# The description every probe drives: line 1 ordinary, line 2 a sentinel-shaped
# token assembled from SP, so this file still carries no contiguous literal.
E4_DESC="$(printf 'E4 probe desc\n%sTIMEOUT@@ reason=e4_probe slots=1 waited=1 disposition=soft lock=e4' "$SP")"

# _e_desc_capture <emitter-file> -> the bytes that emitter's assert() prints for
# E4_DESC. INTERNAL: every caller reduces this to a COUNT and no caller ever
# prints it -- printing it would BE the leak this section guards.
#
# The nested `bash -c` is load-bearing twice over: it gives the probe's own
# assert a private PASS/FAIL pair (this file's counters must not move --
# idiom: tests/infra/test_test_helpers.sh's assert probes), and it re-sources
# the emitter from scratch past test_helpers.sh's source guard, which is a
# plain (non-exported) shell variable and so does not reach a child shell.
_e_desc_capture() {  # <emitter-file>
    bash -c '
        source "$1"
        assert "$2" true
    ' _ "$1" "$E4_DESC" 2>&1 || true
}

# _e_desc_probe <emitter-file> -> count of D_ANCHOR-matching lines.
# `grep -c`, never `grep -q`: -q exits on first match and can race the still-
# writing producer into SIGPIPE, which this file's `pipefail` then promotes to
# a pipeline failure -- silently dropping a real positive (the hazard measured
# and documented at _f_deadline_capable below). `|| true` because a legal ZERO
# is grep's exit 1.
_e_desc_probe() {  # <emitter-file>
    local _n
    _n="$(_e_desc_capture "$1" | grep -cE -- "$D_ANCHOR" || true)"
    printf '%s\n' "${_n:-0}"
}

# _e_desc_token_count <emitter-file> -> count of lines carrying the token AT
# ALL, anchored or not. This is what makes E4a's zero mean "unanchored" rather
# than "absent".
_e_desc_token_count() {  # <emitter-file>
    local _n
    _n="$(_e_desc_capture "$1" | grep -cF -- "$SENTINEL" || true)"
    printf '%s\n' "${_n:-0}"
}

# --- E4b/E4c FIRST: the pin's own controls. E4a asserts an ABSENCE, so a
# typo'd anchor (E4b) or a probe that stopped emitting the token at all (E4c)
# would leave it green forever -- the same controls-first discipline D2/D4c and
# E2/E3 above already apply to every absence assert in this file.
#
# Every count is precomputed into a PLAIN variable before it reaches an assert
# description, and no probe output is ever interpolated: an inline `$(...)`
# here would make this very assert an instance of what Section E forbids, and
# E1's sweep includes this file.
E4B_COUNT="$(_e_desc_probe "$E4_RAW_EMITTER" 2>/dev/null || true)"
assert "E4b: positive control -- the PRE-fix raw emitter IS flagged by D_ANCHOR, so the probe is live (got ${E4B_COUNT:-0}, want 1)" \
    test "${E4B_COUNT:-0}" -eq 1

E4C_COUNT="$(_e_desc_token_count "$SCRIPT_DIR/test_helpers.sh" 2>/dev/null || true)"
assert "E4c: non-vacuity -- the sentinel token IS present in the real assert()'s output, just unanchored (got ${E4C_COUNT:-0}, want >=1)" \
    test "${E4C_COUNT:-0}" -ge 1

E4A_COUNT="$(_e_desc_probe "$SCRIPT_DIR/test_helpers.sh" 2>/dev/null || true)"
assert "E4a: the REAL assert() emits NO D_ANCHOR-matching line for a multi-line description (got ${E4A_COUNT:-0}, want 0)" \
    test "${E4A_COUNT:-0}" -eq 0

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
# can reach a deadline by ANY route whatsoever. Of the four gaps recorded here,
# (1) is now CLOSED -- by Section G at the end of this file -- and (2), (3) and
# (4) remain deliberately out of scope. The numbering is RETAINED rather than
# compacted because this file cites these gaps by number from several places.
#
# (1) A new unredirected deadline SITE added inside an existing static-only
# roster member. CLOSED by SECTION G (task 6278). What stood here recorded that
# generalizing D4 had been declined because it needs subshell-block analysis
# D4's line-oriented section slicer (_d_section_cmds) cannot do:
# test_verify_semaphore_e2e.sh captures at `) 2>"$C_ERR"`, on the subshell's
# closing line, several lines after the invocation it guards -- not on the
# invoking line itself, which is the only shape D4's slicer can see. Section G
# supplies that analysis: a two-pass block scan over subshell, command-
# substitution and inline `bash -c` bodies, so a new unredirected
# deadline-capable site inside ANY static-only member now goes RED.
# WHAT REMAINS HAND-MAINTAINED, stated so the closure is not over-read: the
# per-member (scan-file, site-target) DECLARATION. Section G does not DERIVE
# which token is a member's deadline-capable site, and for the motivating
# member it could not -- test_verify_semaphore_e2e.sh's site is
# scripts/verify.sh, outside this section's node set by (2)(i), so a derived
# predicate yields ZERO sites for it and would be vacuously green. Two asserts
# bound that declaration instead: G0 checks the declared coverage still equals
# the DERIVED static-only slice, so roster growth is loud rather than silent;
# G3 checks each member's declared site still resolves to at least one real
# site, so a moved invocation is loud too. The scan-file indirection for the
# one second-order member is likewise declared, not derived from F_FWD_LIB_MAP.
#
# (2) RESIDUAL ROUTES THE CLOSURE STILL DOES NOT FOLLOW. Task 6291 CLOSED
# what used to stand here: a suite that reaches a deadline only by invoking
# tests/infra/run_all.sh (whose pool worker calls slot_acquire with the finite
# default REIFY_RUN_ALL_POOL_WAIT=1800, run_all.sh:1361), or by invoking
# another suite that does, IS derived now, and six such members are declared
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
    test_infra_git_env_isolation.sh
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
# READ THESE WITH SECTION G IN HAND (task 6278): every static-only entry's
# SITE-level stderr diversion is now ASSERTED there, not merely measured in
# this prose. What these notes still carry that no assert does is the OTHER
# channel -- the bare-variable echo/description dumps recorded below -- which
# Section G deliberately does not cover.
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
#   THE SIX TRANSITIVE MEMBERS (five from task 6291; the sixth added by task
#   7106 and marked as such below -- the count is kept accurate rather than
#   frozen at the task that introduced the block). None is in D_MEMBERS, so all
#   six come out static-only and Section D's concurrent arm and its wall
#   clock are untouched. Each is recorded with its derived ROUTE and its
#   MEASURED leak-channel state -- the honest state, not a blanket "clean".
#   test_infra_git_env_isolation.sh (task 7106, NOT 6291) -- bucket
#   intra-run-serial (run-all-classification.manifest:106 -- it re-enters the
#   infra harness twice over: arm F3 spawns a nested run_all.sh on a fixture
#   INFRA_DIR, and arm G re-invokes test_host_global_unit_pinning.sh). That
#   classification is the SECOND, independent reason it must stay static-only,
#   and it is the stronger one: exactly as for test_verify_semaphore_e2e.sh
#   below, Section D forks its members CONCURRENTLY while THIS file is bucket
#   pool, so contending an intra-run-serial member from here would violate the
#   run_all classification partition -- a CORRECTNESS hazard, not merely the
#   nested-run_all wall clock it would add.
#   Route run_all.sh (RUN_ALL bound :397, `bash "$RUN_ALL" "$F3_FIX"
#   >/dev/null 2>&1` :444, inside arm F3's hostile-env subshell -- the
#   end-to-end proof that a run_all.sh member spawn really is scrubbed, which
#   F1/F2's counting arms cannot give). Measured CLEAN on the SITE channel: stdout goes to /dev/null
#   and stderr merges into that ALREADY-diverted stdout, so neither reaches the
#   inherited fd 2. Section G scores it 1 site / 0 unredirected, and that zero
#   is MUTATION-TESTED rather than assumed -- dropping the `>/dev/null`,
#   dropping both redirects, and reversing the pair to `2>&1 >/dev/null` each
#   flip it to 1 unredirected, so it is a property of the site and not a blind
#   spot in the scanner (this is the stdout-precondition branch of the merge
#   rule, the same one G2d2/G2d3 pin, reached here via the `>` rather than via
#   an enclosing `$(` or a pipe -- a fourth real shape for it).
#   Measured CLEAN on the DESCRIPTION channel too, and for a structural reason
#   worth stating: run_all.sh's own output is DISCARDED at the site, so no
#   member sentinel is ever in this file's hands. The one capture F3b does
#   interpolate ($_F3_REPORT_TEXT, assigned :448, interpolated :452) is the
#   fixture PROBE's own report file, whose entire vocabulary this file
#   generates itself (`LEAK <var>=<val>` :428 and `PROBE_RAN` :430, both
#   printf'd into the probe). So this member is not a third instance of the
#   still-open bare-variable channel recorded for the two entries below.
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
#   another module. Note the distinction Section G does NOT erase -- G asserts
#   SITE-level stderr diversion, and this site's capture is correct (G1 passes
#   on it, scanning the lib); what leaks is the DESCRIPTION channel one step
#   later, which stays out of scope here and is filed separately (ticket
#   tkt_0RSN3SGERQF3E3KD04D71G6W8R, esc-6291-1).
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
# entries) come out `behavioural`; the other NINE -- test_run_all.sh,
# test_slot_event_log.sh, test_verify_semaphore_e2e.sh and the six
# transitive members (task 6291's five plus task 7106's) -- come out
# `static-only`, which is why growing D_ROSTER needed no edit here and left
# Section D's concurrent arm and its wall clock untouched. See the measured justification for each
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
# BIND/EXEC/CALL exist because a call site carrying no explicit knob at
# all can still be deadline-capable, and that is the only route by which
# test_slot_event_log.sh is in scope.
# AMENDED (task 6393): the rationale used to read "because the wrapper
# defaults are FINITE (REIFY_TEST_SEMAPHORE_WAIT=1800 ...,
# REIFY_OCCT_LOCK_WAIT=1800 ...)". Only the OCCT half is still true:
# lib_test_semaphore.sh now defaults to "unlimited", so a knob-less
# test_semaphore_acquire site is NOT deadline-capable via its default any
# more. It stays in scope through the OTHER two routes, which is why the
# derived roster does not move: (a) REIFY_OCCT_LOCK_WAIT still defaults to
# 1800 at cargo-test-occt-gated.sh, and (b) these EREs derive membership
# from CALL-SITE SHAPE, never from a default's value -- a knob-less site
# becomes deadline-capable the moment any caller or ambient env supplies a
# finite WAIT, which no static scan can rule out. Do not "simplify" this
# by making the scan value-aware: that would silently shrink the roster.
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
# four of the six transitive members capable at all. If it silently stopped
# seeding, those four would vanish from the derivation -- and F1 would stay
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


echo ""
echo "=== G: every deadline-capable SITE inside a static-only roster member diverts its stderr (static) ==="

# THE INVARIANT SECTION G ASSERTS, and how it differs from D4's.
#
# Section F derives the roster of deadline-capable suites; D_ROSTER_MODE splits
# it into the three BEHAVIOURAL members Section D actually forks and contends,
# and the NINE STATIC-ONLY ones it does not. For the behavioural three, D4
# already reads the member's SOURCE and proves the deadline-forcing invocation
# still captures stderr. For the static-only nine, nothing did -- Section F's
# SCOPE (1) recorded that as a deliberate, tracked gap. Section G closes it:
# for each static-only member, EVERY deadline-capable site in the file it
# actually execs from must divert its stderr away from the inherited fd 2.
#
# TWO DELIBERATELY DIFFERENT GRAMMARS, and the difference is not an oversight:
#   D4 (behavioural three) asserts EVIDENCE PRESERVATION. Its D_CAPTURE_RE is
#   file-only ('2>"?\$'): `2>/dev/null` destroys the evidence a failing assert
#   needs, and `2>&1` merges the sentinel back into the stream run_all Phase 3
#   re-emits. Those three members are the ones whose OWN failures this suite
#   must be able to diagnose, so the stricter property is the right one there.
#   Section G (static-only nine) asserts only the LEAK property: the
#   invocation's stderr does not reach the inherited fd 2. `2>/dev/null` and a
#   `2>&1` whose stdout is itself diverted both satisfy that, and both are
#   shapes the real members use -- six of test_slot_event_log.sh's seven sites
#   route to /dev/null (recorded beside D_ROSTER_MODE as the exact reason
#   including it behaviourally would have forced weakening D4's grammar).
#   Applying D4's strict grammar here would have demanded rewriting eight
#   suites to satisfy a property they do not need; applying G's lax grammar
#   there would have silently weakened the three that do. G2e below pins that
#   the two grammars did not leak into each other.
#
# SCOPE, stated rather than left implied. Section G is WHOLE-FILE (D4 is
# section-sliced), and it proves diversion at the SITE. It does not prove the
# member never re-emits a capture through some later channel -- the
# bare-variable description channel measured at
# run_all_ambient_isolation_lib.sh:106 and test_verify_env_ambient_isolation.sh
# :189 is a separate, still-open leak, filed as tkt_0RSN3SGERQF3E3KD04D71G6W8R
# / esc-6291-1 and out of scope here (see the note beside D_ROSTER_MODE).
#
# THE COVERAGE TABLE. Three index-aligned arrays -- G_MEMBERS, G_SITE and
# G_SCAN. G_SITE holds each member's deadline-capable site TARGET token,
# NOT a whole ERE -- the exec-position anchor is prefixed to every entry by the
# real-tree arm, so a target here can never accidentally match a `test -f
# "$RUN_ALL"` or a `case` pattern. Each entry carries the member's derived
# route and why that token IS its deadline-capable invocation.
G_MEMBERS=(
    # Route run_all.sh: RUN_ALL bound :397, `bash "$RUN_ALL" "$F3_FIX"
    # >/dev/null 2>&1` :444 -- arm F3's end-to-end proof that a member spawned
    # by the real run_all.sh comes up with the git env scrubbed. Its capture is
    # the merge shape with stdout diverted by a `>` in the SAME segment, which
    # is the stdout-precondition branch G2d2/G2d3 pin (measured 1 site / 0
    # unredirected; see the mutation test recorded beside D_ROSTER_MODE).
    test_infra_git_env_isolation.sh
    # Second-order route via test_run_all.sh (TARGET bound :93). The site that
    # must divert is NOT in this file -- see G_SCAN below.
    test_run_all_ambient_isolation.sh
    # Route run_all.sh: RUN_ALL bound :31, `bash "$RUN_ALL"` :96, whose pool
    # worker calls slot_acquire against the finite REIFY_RUN_ALL_POOL_WAIT.
    test_run_all_clock_marker_sanitize.sh
    # Route run_all.sh: RUN_ALL bound :25, invoked :86 and :387.
    test_run_all_content_skip.sh
    # Route run_all.sh under a different variable name: REAL_RUN_ALL bound :69
    # (RUN_ALL there names a WRAPPER fixture, not the real thing -- hence the
    # distinct target), invoked :122 and :126.
    test_run_all_pool_lock_host_global.sh
    # Route run_all.sh: RUN_ALL bound :19; Test 24 forces a real pool deadline
    # every green run (30s holder on slot-1 vs REIFY_RUN_ALL_POOL_WAIT=2).
    test_run_all.sh
    # DIRECT route: it execs the acquire wrappers themselves, bound at :17-19
    # (LIB=lib_slot_acquire.sh, SEM=lib_test_semaphore.sh,
    # OCCT=cargo-test-occt-gated.sh), whose WAIT defaults are finite.
    test_slot_event_log.sh
    # Route test_occt_flock_gate.sh -- itself a behavioural D_MEMBERS entry --
    # exec'd at :177 by its literal path.
    test_verify_env_ambient_isolation.sh
    # DECLARED, NOT DERIVABLE, and this is the member Section G exists for. Its
    # capability route is F_WAIT_RE (`export REIFY_TEST_SEMAPHORE_WAIT="$wait"`
    # :528), but its deadline-capable INVOCATION is scripts/verify.sh -- and
    # scripts/ is deliberately outside Section F's node set (SCOPE (2)(i)). A
    # site predicate derived from F's node/edge grammar therefore yields ZERO
    # sites for this file: a vacuously green check on exactly the member whose
    # subshell-scoped capture shape (`) 2>"$C_ERR"`) is what SCOPE (1) named
    # and what this task was filed to cover. G3's per-member non-vacuity assert
    # is what keeps a declared target honest in place of that derivation.
    test_verify_semaphore_e2e.sh
)
# Index-aligned with G_MEMBERS.
G_SITE=(
    '"\$RUN_ALL"'
    '"\$_target"'
    '"\$RUN_ALL"'
    '"\$RUN_ALL"'
    '"\$REAL_RUN_ALL"'
    '"\$RUN_ALL"'
    '"\$(SEM|OCCT|LIB)"'
    '"\$SCRIPT_DIR/test_occt_flock_gate\.sh"'
    '"\$REPO_ROOT/scripts/verify\.sh"'
)
# Index-aligned with G_MEMBERS: the file whose SOURCE actually holds that
# member's deadline-capable invocation. EMPTY means "the member itself", which
# is the case for eight of the nine.
#
# THE ONE INDIRECTION, and why it is not optional.
# test_run_all_ambient_isolation.sh reaches its deadline SECOND-ORDER: TARGET is
# bound at :93, handed to `ambient_isolation_check_one` at :366, and exec'd at
# run_all_ambient_isolation_lib.sh:73/:92 as `bash "$_target" 2>&1` inside an
# `_amb_out="$(` command substitution. The forwarding call in the member
# therefore CORRECTLY carries no redirect -- there is nothing there to divert.
# Asserting on the member's own line would be a false RED that pressures a
# future reader into adding a redundant redirect at the wrong level, which is
# strictly worse than not checking at all. MEASURED both ways: scanning the
# member itself gives 1 site / 1 unredirected; scanning the lib for the site it
# really execs gives 2 sites / 0 unredirected.
#
# THE HONEST LIMITATION: this indirection is DECLARED, not derived from Section
# F's exec-forwarding-lib rule (F_FWD_LIB_MAP), which already knows how to
# recognise such a lib. A new forwarding hop therefore needs a human edit here.
# What keeps that from being SILENT is the pair G0 (a new static-only member
# cannot go uncovered) and G3 (a member whose site moved cannot pass
# vacuously) -- the same two guards that make the whole declared table
# acceptable.
G_SCAN=(
    ''
    run_all_ambient_isolation_lib.sh
    '' '' '' '' '' '' ''
)

# G0 FIRST -- the COMPLETENESS assert, before any per-member check.
#
# Section G's per-member checks below run off a hand-declared table (G_MEMBERS
# and its index-aligned site targets). A hand-declared table over a DERIVED
# roster is exactly the silent-drift shape task 6255 was filed to close, one
# level down: Section F can start deriving a tenth static-only member and
# Section G would simply never look at it, staying green while its coverage
# quietly stopped being total. G0 is what makes that growth LOUD -- it compares
# the members Section G declares coverage for against the static-only slice of
# D_ROSTER, re-derived here from D_ROSTER_MODE (which is itself derived from
# D_MEMBERS membership, so there is no hand-typed classification anywhere in
# this chain). A new static-only member turns G0 RED until a human declares its
# site target, which is the correct outcome: what site is deadline-capable in a
# new suite is a judgement, not something this scanner can guess.
#
# Basenames only in the description, and both difference lists are precomputed
# into plain variables above the assert -- never a $(...) inside the
# description text -- so this assert can never itself become an instance of
# what Section E's E1 forbids (E1 scans SCRIPT_DIR/*.sh, which includes this
# file).
G_DERIVED_STATIC="$(
    for _g_i in "${!D_ROSTER[@]}"; do
        [ "${D_ROSTER_MODE[$_g_i]}" = "static-only" ] || continue
        printf '%s\n' "${D_ROSTER[$_g_i]}"
    done | sort
)"
G_DECLARED_SORTED="$(printf '%s\n' "${G_MEMBERS[@]}" | sort)"
# uncovered: a static-only roster member Section G declares no site for.
# stale: a G_MEMBERS entry that is no longer a static-only roster member
# (renamed, deleted, or promoted into D_MEMBERS and hence behavioural).
G_UNCOVERED="$(comm -23 <(printf '%s\n' "$G_DERIVED_STATIC") <(printf '%s\n' "$G_DECLARED_SORTED") | tr '\n' ' ' | sed 's/ *$//')"
G_STALE="$(comm -13 <(printf '%s\n' "$G_DERIVED_STATIC") <(printf '%s\n' "$G_DECLARED_SORTED") | tr '\n' ' ' | sed 's/ *$//')"

assert "G0: Section G covers EVERY static-only roster member -- the declared coverage list equals the static-only slice of D_ROSTER (uncovered: ${G_UNCOVERED:-<none>}) (stale: ${G_STALE:-<none>})" \
    test "$G_DECLARED_SORTED" = "$G_DERIVED_STATIC"

# --- Section G's two predicates, both over _d_join_logical's output.
#
# G_CAPTURE_RE is the DIVERSION grammar, deliberately laxer than D4's
# D_CAPTURE_RE ('2>"?\$', file-only). `2>[^&]` accepts a capture to a file
# (`2>"$VAR"`) AND to /dev/null, because Section G asserts only that the site's
# stderr does not reach the inherited fd 2 -- the LEAK property. D4 additionally
# asserts EVIDENCE PRESERVATION on the three behavioural members, whose own
# failures this suite must be able to diagnose, which is why /dev/null is
# excluded there and admitted here. The `[^&]` is what still rejects a bare
# `2>&1`: merging stderr into an INHERITED stdout is not a diversion at all.
# G2e below pins that the two grammars stay separate.
G_CAPTURE_RE='2>[^&]'

# _g_scan <logical-lines-file> <site-ERE> -> "<sites> <unredirected>", the ONE
# analysis both predicates below read, so their two counts can never disagree.
#
# WHY AWK AND NOT A GREP. A line-local predicate cannot see either multi-line
# capture shape the real roster members use -- the subshell closer and the
# command-substitution opener sit on DIFFERENT lines from the site, with no
# backslash continuation for _d_join_logical to merge. That is exactly why
# Section F's SCOPE (1) declined to generalize D4's line-oriented slicer, and
# it is what this two-pass block analysis supplies.
#
# THE SITE ERE AND THE CAPTURE ERE REACH AWK THROUGH THE ENVIRONMENT, read with
# ENVIRON[]. NOT `awk -v`, and this is MEASURED, not stylistic: `-v` runs escape
# processing over the value, so `\.` collapses to `.`, `\$` becomes the regex
# END-OF-STRING anchor and `\{`/`\}` become literals. Handing these EREs over
# with -v silently reduced EVERY member to sites=0, with nothing on stderr but
# an `awk: warning: escape sequence ...`. ENVIRON does no escape processing and
# is POSIX, so this is portable. G_CAPTURE_RE is passed rather than re-spelled
# inside the program, so there stays exactly ONE diversion grammar in Section G.
#
# OUTPUT IS COUNTS ONLY -- never a matched line. Same discipline as D1, D4b and
# _e_scan: this file's output is re-emitted verbatim into the merge-gate verify
# log, so printing the offending line would BE the leak. Reading the file by
# NAME rather than through a pipe also puts this beyond the `-q`/SIGPIPE/
# pipefail hazard recorded above _f_deadline_capable: awk drains to EOF by
# construction.
#
# NO APOSTROPHE APPEARS INSIDE THE AWK PROGRAM, comments included -- the whole
# program is a single-quoted shell string, so one would end it. The single
# quote the body rule needs is BUILT (SQ, below) for the same reason.
_g_scan() {  # <logical-lines-file> <site-ERE> -> "<sites> <unredirected>"
    G_AWK_SITE="$2" G_AWK_CAP="$G_CAPTURE_RE" awk '
    BEGIN {
        SITE = ENVIRON["G_AWK_SITE"]
        CAP  = ENVIRON["G_AWK_CAP"]
        SQ = sprintf("%c", 39)   # the single quote, built not written
        DQ = sprintf("%c", 34)   # the double quote, built for symmetry
        n = 0; top = 0; sites = 0; unred = 0
    }

    # ---- COMMAND SEGMENTATION. Capture attribution has to be COMMAND-scoped,
    # not LINE-scoped: a `2>` merely present somewhere on a line says nothing
    # about which of that lines commands it belongs to. Three real shapes are
    # silent greens under a line-scoped rule, and all three are pinned by G2g:
    #   `bash "$P" & echo $! 2>/dev/null`   the redirect is the echos
    #   `) || kill "$pid" 2>/dev/null`      the redirect is the kills
    #   `bash "$P" 2>&1 >"$OUT"`            reversed pair -- fd 2 is aimed at
    #                                       the INHERITED stdout, then fd 1
    #                                       moves; stderr leaks
    #
    # nextsep() walks from `from` to the first UNQUOTED command separator and
    # leaves SEP/SEPLEN describing it. segat() returns the segment HOLDING a
    # position (a site line); segfrom() the segment STARTING at one (a block
    # closer, scanned from just past the closing token). SEP survives the call
    # because a terminating pipe is itself a stdout diversion -- see PASS 2.
    #
    # A REDIRECT AMPERSAND IS NOT A SEPARATOR: `2>&1` and `>&2` carry `>` (or
    # `<`) immediately before the `&`, and bashs `&>` carries `>` immediately
    # after it. Quote tracking is per-line and deliberately simple; an
    # unbalanced quote merely stops the scan, which yields the whole remainder
    # as one segment -- i.e. exactly the old line-scoped behaviour, so it
    # degrades toward the previous rule and never toward hiding a leak.
    function nextsep(s, from,   i, c, p, q, ln) {
        SEP = ""; SEPLEN = 0
        ln = length(s); q = ""
        for (i = from; i <= ln; i++) {
            c = substr(s, i, 1)
            if (q != "") { if (c == q) q = ""; continue }
            if (c == "\\") { i++; continue }
            if (c == SQ || c == DQ) { q = c; continue }
            if (c == ";") { SEP = ";"; SEPLEN = 1; return i }
            if (c == "|") {
                if (substr(s, i + 1, 1) == "|") { SEP = "||"; SEPLEN = 2 }
                else                            { SEP = "|";  SEPLEN = 1 }
                return i
            }
            if (c == "&") {
                p = (i > 1) ? substr(s, i - 1, 1) : ""
                if (p == ">" || p == "<") continue
                if (substr(s, i + 1, 1) == ">") continue
                if (substr(s, i + 1, 1) == "&") { SEP = "&&"; SEPLEN = 2 }
                else                            { SEP = "&";  SEPLEN = 1 }
                return i
            }
        }
        return 0
    }

    function segat(s, pos,   from, k) {
        from = 1
        while (1) {
            k = nextsep(s, from)
            if (k == 0)   return substr(s, from)
            if (pos < k)  return substr(s, from, k - from)
            from = k + SEPLEN
        }
    }

    function segfrom(s, from,   k) {
        k = nextsep(s, from)
        if (k == 0) return substr(s, from)
        return substr(s, from, k - from)
    }

    # ---- PASS 1: block structure. For every line, record the innermost
    # enclosing opener (encl[]); for every opener, record its kind and, once the
    # closer is seen, that closer disposition (stamp[] = "<kind>:<disp>").
    # THREE OPENER KINDS, each a real in-tree shape:
    #   subst    a line ending in $( -- stdout is diverted into a variable,
    #            which is what makes an inner 2>&1 a diversion rather than a
    #            leak. Shape at test_run_all_content_skip.sh:80-87 and :380-388,
    #            and test_verify_env_ambient_isolation.sh:172-178.
    #   body     a line ending in "bash -c" plus a quote -- an inline script
    #            body, closed by a line starting with that quote, whose capture
    #            sits on that closing line. A real in-tree shape
    #            (test_test_run_semaphore.sh) -- but an HONEST NOTE, because the
    #            earlier claim here was measured on a file Section G does not
    #            read: that file is a BEHAVIOURAL D_MEMBERS entry and is never
    #            in G_MEMBERS, so no member scanned today reaches this branch.
    #            MEASURED: stubbing the branch out leaves all eight per-member
    #            counts byte-identical. It is kept because the shape is one a
    #            static-only member can adopt at any time and the alternative
    #            is a false RED (same class as SCOPE (3) of Section F, but
    #            louder), and G2c3/G2c4 below are the live guard that keeps the
    #            branch exercised rather than merely present.
    #   subshell a line ending in a bare ( -- the shape SCOPE (1) of Section F
    #            names, captured on the closing ) line. 8 of the 11 sites in
    #            test_verify_semaphore_e2e.sh are captured only this way.
    # ORDER MATTERS: $( is tested before the bare ( so a command substitution is
    # never misread as a subshell, and $(( arithmetic is excluded explicitly.
    # FAIL-SAFE BY CONSTRUCTION: a closer arriving on an empty stack is ignored,
    # and an opener still unclosed at EOF never gets a stamp, so it counts as
    # NOT captured. Both degrade toward flagging, never toward hiding.
    {
        line[++n] = $0
        isopen = 0; k = ""
        if      ($0 ~ /\$\($/)                                      { isopen = 1; k = "subst" }
        else if ($0 ~ "(bash|sh)[[:blank:]]+-c[[:blank:]]+" SQ "$") { isopen = 1; k = "body" }
        else if ($0 ~ /\($/ && $0 !~ /\$\(\($/)                     { isopen = 1; k = "subshell" }

        isclose = 0
        if (top > 0) {
            ok = stack[top]
            if (bk[ok] == "body") { if ($0 ~ "^[[:blank:]]*" SQ) isclose = 1 }
            else                  { if ($0 ~ /^[[:blank:]]*\)/)  isclose = 1 }
        }

        if (isclose) {
            ok = stack[top--]
            # Only the CLOSERS OWN segment can capture the block. Scan from
            # just past the closing token, stepping over the `"` of a `)"` that
            # closes a `"$( ... )"` so the segment scanner does not read the
            # rest of the line as quoted text.
            if (bk[ok] == "body") match($0, "^[[:blank:]]*" SQ)
            else                  match($0, /^[[:blank:]]*\)/)
            cfrom = RSTART + RLENGTH
            if (substr($0, cfrom, 1) == DQ) cfrom++
            cseg = segfrom($0, cfrom)
            d = (cseg ~ CAP) ? "err" : (bk[ok] == "subst" ? "out" : "none")
            stamp[ok] = bk[ok] ":" d
            encl[n] = (top > 0) ? stack[top] : 0
        } else if (isopen) {
            encl[n] = (top > 0) ? stack[top] : 0
            stack[++top] = n; bk[n] = k
        } else {
            encl[n] = (top > 0) ? stack[top] : 0
        }
    }

    # ---- PASS 2. Everything below is asked of the sites OWN SEGMENT (segat),
    # never of the whole line -- see the segmentation block above. A site is
    # captured iff any of
    #   (a) its segment carries a diversion (CAP: 2> to a file or to /dev/null);
    #   (b) its segment carries a 2>&1 AND its stdout is itself diverted -- by a
    #       `>` EARLIER IN THE SEGMENT that is not part of 2> or >&, by an
    #       inline $( earlier in the segment, by the segment ending in a PIPE,
    #       or by an enclosing subst block. Without that precondition 2>&1
    #       merges the sentinel back into the stream run_all Phase 3 re-emits,
    #       which is the leak and not a fix. THE ORDER TEST IS THE POINT: the
    #       same two tokens reversed (`2>&1 >file`) aim fd 2 at the inherited
    #       stdout and only then move fd 1, so that shape still leaks (G2g3);
    #   (c) an enclosing block whose closer segment carried 2> (stamp ending
    #       :err) -- the subshell and inline-body shapes.
    END {
        for (i = 1; i <= n; i++) {
            if (line[i] !~ SITE) continue
            sites++
            match(line[i], SITE)
            seg = segat(line[i], RSTART)
            sep = SEP
            cap = 0
            if (seg ~ CAP) cap = 1
            else if (match(seg, /2>&1/)) {
                pre = substr(seg, 1, RSTART - 1)
                if (pre ~ /(^|[^2>&])>[^&]/ || pre ~ /\$\(/ || sep == "|") cap = 1
                else for (e = encl[i]; e != 0; e = encl[e]) if (stamp[e] ~ /^subst:/) { cap = 1; break }
            }
            if (!cap) for (e = encl[i]; e != 0; e = encl[e]) if (stamp[e] ~ /:err$/) { cap = 1; break }
            if (!cap) unred++
        }
        printf "%d %d\n", sites, unred
    }
    ' "$1"
}

# _g_sites <logical-lines-file> <site-ERE> -> how many deadline-capable sites
# that file holds. Drives G3's per-member non-vacuity arm.
_g_sites() {
    local _r
    _r="$(_g_scan "$1" "$2")"
    echo "${_r%% *}"
}

# _g_unredirected <logical-lines-file> <site-ERE> -> how many of those sites do
# NOT divert their stderr. Drives G1.
_g_unredirected() {
    local _r
    _r="$(_g_scan "$1" "$2")"
    echo "${_r##* }"
}

echo ""
echo "--- G2: controls on the site/capture predicate, on synthetic fixtures ---"

# CONTROLS FIRST, this file's own convention throughout (D2 before D1, D4c
# before D4b, E2/E3 before E1, FC* before F1). G1 below asserts a ZERO, and an
# absence-assert whose predicate is typo'd is green forever -- G2a is what makes
# G1's zero mean something. G2b is its mirror: a predicate that flagged a
# CORRECTLY captured site would make Section G unsatisfiable.
#
# Fixtures are PRINTF'd into a mktemp dir, never written as a heredoc: Section
# F's SCOPE (3) records that a heredoc body reads as a live invocation to a
# line-oriented scanner. Harmless for F (this file is excluded from its node set
# by _f_excluded_node) but the habit is worth keeping, and the fixtures stay out
# of Section G's own scan by construction since G reads only declared members.
TMPG="$(mktemp -d)"; _TMPDIRS+=("$TMPG")

G_CTRL_BARE="$TMPG/ctrl-same-line-bare.cmds"
G_CTRL_CAP="$TMPG/ctrl-same-line-captured.cmds"
G_CTRL_SITE='"\$G_PROBE"'
printf 'bash "$G_PROBE" --pool || _rc=$?\n' > "$G_CTRL_BARE"
printf 'bash "$G_PROBE" --pool 2>"$G_ERR" || _rc=$?\n' > "$G_CTRL_CAP"

G2A_N="$(_g_unredirected "$G_CTRL_BARE" "$G_CTRL_SITE")"
G2B_N="$(_g_unredirected "$G_CTRL_CAP" "$G_CTRL_SITE")"

# Counts only, never the fixture line itself -- the same rule D1/D4b/_e_scan
# follow, and the reason `test` is the checker here: assert dumps a FAILING
# checker's captured output.
assert "G2a: positive control -- a deadline-capable site with NO redirect at all is flagged (got $G2A_N unredirected)" \
    test "$G2A_N" -eq 1
assert "G2b: the same site capturing to a file is NOT flagged (got $G2B_N unredirected)" \
    test "$G2B_N" -eq 0

# --- G2c/G2d: the BLOCK-SCOPE controls. Both fixtures are shapes the real
# roster members use, and neither is visible to a line-local predicate even
# after continuation-joining -- which is exactly why Section F's SCOPE (1)
# declined to generalize D4 rather than doing it cheaply.
#
# TWO HALVES EACH, and the second half of each pair is what stops block
# tolerance from being BLANKET tolerance: a predicate that simply ignored
# anything inside a `(` or `$(` would pass the first half of both and be
# useless. The negative half pins that an UNCAPTURED enclosing block is still
# flagged.
G_CTRL_SUB_CAP="$TMPG/ctrl-subshell-captured.cmds"
G_CTRL_SUB_BARE="$TMPG/ctrl-subshell-bare.cmds"
G_CTRL_BODY_CAP="$TMPG/ctrl-body-captured.cmds"
G_CTRL_BODY_BARE="$TMPG/ctrl-body-bare.cmds"
G_CTRL_MERGE_BARE="$TMPG/ctrl-merge-stdout-inherited.cmds"
G_CTRL_MERGE_SUBST="$TMPG/ctrl-merge-stdout-diverted.cmds"
G_CTRL_MERGE_PIPE="$TMPG/ctrl-merge-stdout-piped.cmds"

# The shape at test_verify_semaphore_e2e.sh: the invocation is inside a
# subshell and the capture is on the subshell's CLOSING line, several lines
# later. Eight of that member's eleven sites are captured only this way, and
# this is the shape Section F's SCOPE (1) names verbatim.
printf '%s\n' \
    '_C_RC=0' \
    '(' \
    '    bash "$G_PROBE" --pool' \
    ') 2>"$G_ERR" || _C_RC=$?' \
    > "$G_CTRL_SUB_CAP"
printf '%s\n' \
    '_C_RC=0' \
    '(' \
    '    bash "$G_PROBE" --pool' \
    ') || _C_RC=$?' \
    > "$G_CTRL_SUB_BARE"

# The INLINE-BODY block kind, the third opener _g_scan recognises and until now
# the only one with no control of its own. Same closer-stamped shape as the
# subshell pair above (the capture sits on the line that CLOSES the body), so
# the same two halves apply. Double-quoted printf args here because a
# single-quoted shell string cannot contain the very quote the fixture needs.
printf '%s\n' \
    "bash -c '" \
    '    bash "$G_PROBE" --pool' \
    "' 2>\"\$G_ERR\" || _rc=\$?" \
    > "$G_CTRL_BODY_CAP"
printf '%s\n' \
    "bash -c '" \
    '    bash "$G_PROBE" --pool' \
    "' || _rc=\$?" \
    > "$G_CTRL_BODY_BARE"

# The merge rule and its STDOUT PRECONDITION. `2>&1` is a diversion only if
# stdout is itself diverted; with stdout inherited it is the leak, not a fix
# (the same reason D4's grammar rejects it outright). The positive shape is at
# test_run_all_content_skip.sh:80-87 and :380-388 and
# test_verify_env_ambient_isolation.sh:172-178 -- `2>&1` IS on the invocation
# line, but its legitimacy is only knowable from the enclosing `$(` opener,
# and these carry no backslash continuation for the joiner to merge.
printf '%s\n' \
    'bash "$G_PROBE" --pool 2>&1 || _rc=$?' \
    > "$G_CTRL_MERGE_BARE"
printf '%s\n' \
    'G_OUT="$(' \
    '    bash "$G_PROBE" --pool 2>&1' \
    ')" || _rc=$?' \
    > "$G_CTRL_MERGE_SUBST"
# A PIPE is a stdout diversion too, and it is the third way the precondition
# can be met. `cmd 2>&1 | reader` sends both streams into the pipe -- the
# pipeline fds are installed before the command's own redirections -- so the
# site does not leak, and flagging it would be a false RED that pressures a
# reader into adding a redundant redirect. Same argument the G_SCAN preamble
# makes for the forwarding-lib indirection.
printf '%s\n' \
    'bash "$G_PROBE" --pool 2>&1 | grep -q "$G_MARKER"' \
    > "$G_CTRL_MERGE_PIPE"

G2C1_N="$(_g_unredirected "$G_CTRL_SUB_CAP" "$G_CTRL_SITE")"
G2C2_N="$(_g_unredirected "$G_CTRL_SUB_BARE" "$G_CTRL_SITE")"
G2C3_N="$(_g_unredirected "$G_CTRL_BODY_CAP" "$G_CTRL_SITE")"
G2C4_N="$(_g_unredirected "$G_CTRL_BODY_BARE" "$G_CTRL_SITE")"
G2D1_N="$(_g_unredirected "$G_CTRL_MERGE_BARE" "$G_CTRL_SITE")"
G2D2_N="$(_g_unredirected "$G_CTRL_MERGE_SUBST" "$G_CTRL_SITE")"
G2D3_N="$(_g_unredirected "$G_CTRL_MERGE_PIPE" "$G_CTRL_SITE")"

assert "G2c1: a site whose capture is on its enclosing SUBSHELL's closing line is NOT flagged (got $G2C1_N unredirected)" \
    test "$G2C1_N" -eq 0
assert "G2c2: ... but the same site in a subshell whose closer carries NO redirect still IS (block tolerance is not blanket; got $G2C2_N unredirected)" \
    test "$G2C2_N" -eq 1
assert "G2c3: a site whose capture is on its enclosing inline BODY's closing line is NOT flagged (got $G2C3_N unredirected)" \
    test "$G2C3_N" -eq 0
assert "G2c4: ... but the same site in a body whose closer carries NO redirect still IS (got $G2C4_N unredirected)" \
    test "$G2C4_N" -eq 1
assert "G2d1: a bare 2>&1 with stdout INHERITED is flagged -- merging into the re-emitted stream is the leak, not a fix (got $G2D1_N unredirected)" \
    test "$G2D1_N" -eq 1
assert "G2d2: ... but the same 2>&1 inside a multi-line command substitution is NOT, because stdout is diverted there (got $G2D2_N unredirected)" \
    test "$G2D2_N" -eq 0
assert "G2d3: ... and neither is a 2>&1 whose stdout is diverted by a PIPE -- both streams go into the pipe, not to fd 2 (got $G2D3_N unredirected)" \
    test "$G2D3_N" -eq 0

# --- G2e: THE TIER-SEPARATION PIN. Green on arrival by design, kept as a
# standing regression guard: Section G's laxer DIVERSION grammar must not have
# leaked into D4's deliberately file-only D_CAPTURE_RE. A single
# `2>/dev/null` site is the one input that tells the two grammars apart -- G
# accepts it (stderr is diverted, which is all G claims), D4 rejects it (the
# evidence a failing assert needs is destroyed). Asserted as two separate
# checks so "G got lax" and "D4 got lax" stay distinguishable failures.
G_CTRL_DEVNULL="$TMPG/ctrl-devnull.cmds"
printf '%s\n' \
    'bash "$G_PROBE" --pool 2>/dev/null || _rc=$?' \
    > "$G_CTRL_DEVNULL"
G2E_G_N="$(_g_unredirected "$G_CTRL_DEVNULL" "$G_CTRL_SITE")"
G2E_D_N="$(_d_unredirected "$G_CTRL_DEVNULL" "$G_CTRL_SITE")"

assert "G2e1: Section G accepts a 2>/dev/null site -- it asserts the LEAK property, and stderr is diverted (got $G2E_G_N unredirected)" \
    test "$G2E_G_N" -eq 0
assert "G2e2: D4's file-only grammar still REJECTS that same site -- G's laxer diversion rule did not leak into the evidence-preserving tier (got $G2E_D_N unredirected)" \
    test "$G2E_D_N" -eq 1

# THE EXEC-POSITION ANCHOR, prefixed to every G_SITE target. NEW, separately-
# named constants: F_EDGE_ANCHOR is reused READ-ONLY as the command-boundary
# class, but F_EDGE_VERB_RE is deliberately NOT widened in place -- it drives
# F1's derivation, and adding `env`/`timeout` to it could change the derived
# roster. Section G needs a laxer verb set than the closure does precisely
# because it is asking a different question (is this line an exec position?)
# than the closure asks (does this file invoke that node?).
#
# TWO ALTERNATIVES, both measured NECESSARY:
#   VERB form -- the site follows an anchored exec verb, with the same
#   flag tolerance F_EXEC_RE already carries. `env`/`timeout` are in the set
#   here because a real site is written `timeout 600 bash "$RUN_ALL" ...`.
#   COMMAND-WORD form -- the site IS the command word, e.g.
#   `"$WRAPPER" bash -c true` or a bare `ambient_isolation_check_one "$TARGET"`.
#
# THE BOUNDARY ON THE COMMAND-WORD FORM IS COMMAND-START, NOT ANY BLANK, and
# both halves of that choice were measured.
#
#   WHY NOT ANY BLANK (precision). Allowing F_EDGE_ANCHOR here admits `test -f
#   "$RUN_ALL"`, `[ -f "$REAL_RUN_ALL" ]` and `if [ -f "$RUN_ALL" ]; then` as
#   invocations -- in test_run_all.sh alone that is 101 site matches instead of
#   70, and 29 of the 31 extra read as unredirected: 29 false REDs on a file
#   whose real sites are all correctly captured. G2f4 is the standing guard on
#   that number.
#
#   WHY NOT LINE-INITIAL EITHER (recall). A `^`-only form sees a command word
#   only at the start of a logical line. `cd "$d" && "$SITE" ...`, `if "$SITE"
#   ...; then` and `export X=1; "$SITE" ...` are all real exec positions and
#   all read as ZERO sites under it -- and a site the scan cannot see is a site
#   G1 stays green over, the same SILENT class as the quoted-assignment hole
#   G2f1 pins. This matters most for test_slot_event_log.sh, whose declared
#   targets ARE bare command words; its 8 sites are all line-initial TODAY,
#   which is a fact about the file, not a property of the check. G2f3 is the
#   standing guard on that.
#
#   The middle ground is the set below: start of line, immediately after a
#   `&&`/`||`/`;`/`|`/`&`/`(`/`{`, or at the head of an if/then/elif/else/do/
#   while/until clause. A token after a WORD (`-f`, `case`, `grep`) is never a
#   command word, which is exactly what the precision measurement above is
#   about. MEASURED on the real tree: all eight per-member counts are
#   byte-for-byte identical to the `^`-only form, so this is pure recall.
#
# THE KEY=VAL PREFIX TOLERANCE IS EQUALLY LOAD-BEARING, and was measured the
# other way: test_slot_event_log.sh, test_lane_x_flock.sh and
# test_occt_flock_gate.sh all write `DF_VERIFY_ROLE=task REIFY_...=... "$SEM"
# bash -c true`, where the wrapper is not the line's first word. Drop the
# KEY=VAL prefix and test_slot_event_log.sh falls from 7 sites to 0 -- a check
# that is not merely weaker but vacuous, which is exactly what G3 exists to
# catch.
G_EXEC_KV='([A-Za-z_][A-Za-z0-9_]*=[^[:blank:]]*[[:blank:]]+)*'
# THE POST-VERB CLASS ADMITS A QUOTED ASSIGNMENT WORD, and its narrowness is
# itself load-bearing. Three measurements, all taken against the real tree:
#
#   (a) BEFORE/AFTER -- test_slot_event_log.sh goes 7 sites -> 8, and the other
#   seven members are byte-for-byte unchanged (test_run_all.sh 70/0,
#   test_run_all_ambient_isolation.sh 2/0,
#   test_run_all_clock_marker_sanitize.sh 1/0, test_run_all_content_skip.sh
#   2/0, test_run_all_pool_lock_host_global.sh 2/0,
#   test_verify_env_ambient_isolation.sh 1/0, test_verify_semaphore_e2e.sh
#   11/0). 8 is the TRUE total, confirmed by enumerating that file's wrapper
#   invocations directly.
#
#   (b) THE MUTATION TEST, and it is why this is a CORRECTNESS fix rather than
#   a coverage nicety: delete only the ` 2>"$_STDERR_C"` from
#   test_slot_event_log.sh:102 and the pre-fix scan still reports 7 sites / 0
#   unredirected -- G3 AND G1 both stay GREEN on a site whose stderr capture
#   was removed. The fixed scan reports 8 / 1 and G1 goes correctly RED. The
#   hole was SILENT, and no real-tree count could have revealed it, which is
#   precisely what G2f exists to make visible.
#
#   (c) WHY NARROW RATHER THAN BLANKET -- a blanket
#   `([^[:blank:]]+[[:blank:]]+)*` also yields 8/0 on today's roster and is
#   therefore indistinguishable from this fix by ANY real-tree count, but it
#   steps over an intervening quoted COMMAND word: on G2f2's
#   `timeout 600 bash "$G_WRAPPER" "$G_PROBE"` blanket matches (1) while both
#   the old and the fixed class correctly reject it (0). The `"` exclusion is
#   what keeps a wrapper's ARGUMENTS from reading as sites, and this fix
#   preserves it by admitting only KEY="VAL" words, which can never be the
#   command word. G2f2 is the standing guard on that.
#
# The pre-verb (G_EXEC_KV) and post-verb tolerances are deliberately DIFFERENT
# and must stay so: G_EXEC_KV takes the unquoted `[^[:blank:]]*` value form,
# while this class needs the explicit `="[^"]*"` alternative. Do not "unify"
# them -- the unquoted form here would re-admit the blanket hazard in (c).
G_EXEC_VERB_RE="${F_EDGE_ANCHOR}${G_EXEC_KV}"'(env|timeout|bash|sh|source)[[:blank:]]+((([^"[:blank:]]+)|([A-Za-z_][A-Za-z0-9_]*="[^"]*"))[[:blank:]]+)*'
G_EXEC_CMDSTART='(^[[:blank:]]*|[[:blank:]]*(&&|\|\||[;|&({])[[:blank:]]*|'"${F_EDGE_ANCHOR}"'(if|then|elif|else|do|while|until)[[:blank:]]+)'
G_EXEC_FIRST_RE="${G_EXEC_CMDSTART}${G_EXEC_KV}"

echo ""
echo "--- G2f: controls on the exec-position anchor ---"

# G2a-G2e above exercise only the CAPTURE half of the predicate: every one of
# them calls _g_unredirected with a BARE G_CTRL_SITE and never constructs the
# exec anchor at all. A hole in the anchor itself was therefore invisible to
# every control in this section -- the G3/G1 loop below is the only consumer of
# G_EXEC_VERB_RE/G_EXEC_FIRST_RE, and its real-tree counts cannot distinguish
# "this member has N sites" from "this member has N+1 sites and one is
# unreachable". G2f closes that, and asserts with _g_sites rather than
# _g_unredirected because the property under test is site DETECTION, not
# capture. Controls-first still holds: these sit above the arm they guard.
G_CTRL_ENVKV="$TMPG/ctrl-exec-env-quoted-kv.cmds"
G_CTRL_WRAP="$TMPG/ctrl-exec-wrapper-shadowed.cmds"
G_CTRL_CMDSTART="$TMPG/ctrl-exec-command-start.cmds"
G_CTRL_TESTHEAD="$TMPG/ctrl-exec-test-builtin-head.cmds"

# The real in-tree shape, at test_slot_event_log.sh:98-102 (joined logical line
# 59): that section's own forced-acquire control, run at CONCURRENCY=1 against
# a held lock, i.e. a genuine deadline-capable acquire site. What makes it hard
# is that the `env` verb is followed by a QUOTED assignment word before the
# wrapper is reached, and the FIRST-WORD form cannot rescue it because the
# line's first word is `env`, not a KEY=VAL.
printf '%s\n' \
    'env -u REIFY_SLOT_EVENT_LOG DF_VERIFY_ROLE=task REIFY_LOCK="$G_LOCK" "$G_PROBE" bash -c true 2>"$G_ERR" || _rc=$?' \
    > "$G_CTRL_ENVKV"

# THE WRAPPER-SHADOWING PIN. Here the real exec is "$G_WRAPPER" and "$G_PROBE"
# is a mere ARGUMENT, so this line is not a site at all. Green on arrival by
# design, and kept for a specific reason recorded beside G_EXEC_VERB_RE: the
# blanket relaxation `([^[:blank:]]+[[:blank:]]+)*` is INDISTINGUISHABLE from
# the correct fix on today's roster -- both produce the same eight per-member
# counts -- yet it steps over an intervening quoted COMMAND word and turns this
# fixture into a false positive. Without G2f2 a future "simplification" to
# blanket would land silently. The hazard is live in the tree, not
# hypothetical: it is the same wrapper-vs-real distinction that
# test_run_all_pool_lock_host_global.sh's G_SITE entry already exists to encode.
printf '%s\n' \
    'timeout 600 bash "$G_WRAPPER" "$G_PROBE" --pool 2>"$G_ERR" || _rc=$?' \
    > "$G_CTRL_WRAP"

# THE RECALL HALF. A site can be a command word without being the LINE's first
# word and without following an exec verb -- after `&&`/`||`/`;`/`|`/`&`, or as
# the head of an `if`/`while`/`until`/`then`/`do`/`else` clause. All three lines
# below are genuine invocations; a scan that cannot see them stays green while
# a new unredirected site sits in plain sight, which is the same SILENT class
# as the quoted-assignment hole G2f1 pins. Three separate lines, so the count
# also says WHICH boundary class regressed.
printf '%s\n' \
    'cd "$G_DIR" && "$G_PROBE" bash -c true' \
    'if "$G_PROBE" bash -c true; then :; fi' \
    'export G_X=1; "$G_PROBE" bash -c true' \
    > "$G_CTRL_CMDSTART"

# ITS LOAD-BEARING MIRROR, and the reason the boundary set is COMMAND-START
# rather than any blank. `test -f "$X"`, `[ -f "$X" ]`, a `case` subject and a
# `grep` argument all put the token after a WORD, not after a command start, so
# none of them is an exec position. Measured on the real tree when the anchor
# was first written: admitting a bare blank boundary turns test_run_all.sh from
# 70 site matches into 101, and 29 of the 31 extra read as unredirected -- 29
# false REDs on a file whose real sites are all correctly captured. This
# fixture is what keeps any future widening from re-buying that.
printf '%s\n' \
    'test -f "$G_PROBE" && echo yes' \
    'if [ -f "$G_PROBE" ]; then :; fi' \
    '[ -x "$G_PROBE" ] || exit 1' \
    'case "$G_PROBE" in *) :;; esac' \
    'grep -q "$G_PROBE" "$G_LOG"' \
    > "$G_CTRL_TESTHEAD"

# Built exactly as the G3/G1 loop below builds it, so these controls exercise
# the predicate the real arm uses rather than a lookalike.
G_CTRL_EXEC_RE="(${G_EXEC_VERB_RE}|${G_EXEC_FIRST_RE})${G_CTRL_SITE}"
G2F1_N="$(_g_sites "$G_CTRL_ENVKV" "$G_CTRL_EXEC_RE")"
G2F2_N="$(_g_sites "$G_CTRL_WRAP" "$G_CTRL_EXEC_RE")"
G2F3_N="$(_g_sites "$G_CTRL_CMDSTART" "$G_CTRL_EXEC_RE")"
G2F4_N="$(_g_sites "$G_CTRL_TESTHEAD" "$G_CTRL_EXEC_RE")"

# Counts only, precomputed into plain variables, `test` as the checker -- the
# same E1/D1 output-safety discipline as every other assert in this section.
assert "G2f1: an exec verb followed by a QUOTED assignment word still reaches its site (got $G2F1_N sites)" \
    test "$G2F1_N" -eq 1
assert "G2f2: ... but a site SHADOWED by an intervening quoted command word is NOT one (a blanket relaxation would flag it; got $G2F2_N sites)" \
    test "$G2F2_N" -eq 0
assert "G2f3: a site that is the command word after &&, after ;, or at the head of an if clause is still an exec position (got $G2F3_N of 3 sites)" \
    test "$G2F3_N" -eq 3
assert "G2f4: ... but a token after a WORD is not -- test -f, [ -f, a case subject and a grep argument are all non-sites (got $G2F4_N sites)" \
    test "$G2F4_N" -eq 0

echo ""
echo "--- G2g: capture attribution is COMMAND-scoped, not line-scoped ---"

# THE ATTRIBUTION HAZARD, and why it needs controls of its own. G2a-G2d ask
# whether a redirect is RECOGNISED; G2g asks whether it belongs to the command
# under test at all. A `2>` merely PRESENT somewhere on a line says nothing --
# a shell line can hold several commands, and only the segment the site (or the
# block closer) actually sits in can capture it. All three fixtures below are
# genuinely leaking sites, so all three must be FLAGGED; a predicate that reads
# the whole line accepts every one of them and G1 stays green over a real leak.
# That is the same silent-pass class as the exec-anchor hole G2f pins, on the
# other half of the predicate.
G_CTRL_ATTR_SAME="$TMPG/ctrl-attr-same-line.cmds"
G_CTRL_ATTR_CLOSER="$TMPG/ctrl-attr-closer.cmds"
G_CTRL_ATTR_REVERSED="$TMPG/ctrl-attr-reversed-merge.cmds"

# SAME-LINE: the site is backgrounded and the `2>/dev/null` belongs to the
# `echo` after the `&`. The site's own stderr still reaches the inherited fd 2.
printf '%s\n' \
    'bash "$G_PROBE" --pool & echo $! 2>/dev/null' \
    > "$G_CTRL_ATTR_SAME"

# CLOSER LINE: the subshell closes with NO redirect -- the `2>/dev/null` on
# that line belongs to the `kill` in the `||` branch, which runs only after the
# subshell has already written to fd 2. The block-scope rule must not read it
# as the subshell's own capture.
printf '%s\n' \
    '_C_RC=0' \
    '(' \
    '    bash "$G_PROBE" --pool' \
    ') || kill "$G_PID" 2>/dev/null' \
    > "$G_CTRL_ATTR_CLOSER"

# ORDER WITHIN THE SEGMENT: `2>&1 >file` is the classic reversed pair. fd 2 is
# pointed at whatever fd 1 is AT THAT MOMENT -- the inherited stdout -- and only
# then is fd 1 moved to the file. Stderr leaks; the same two tokens in the
# other order (G2d1s mirror, `>file 2>&1`) do not. The merge rule therefore has
# to compare POSITIONS, not merely observe that both tokens are present.
printf '%s\n' \
    'bash "$G_PROBE" --pool 2>&1 >"$G_OUT"' \
    > "$G_CTRL_ATTR_REVERSED"

G2G1_N="$(_g_unredirected "$G_CTRL_ATTR_SAME" "$G_CTRL_SITE")"
G2G2_N="$(_g_unredirected "$G_CTRL_ATTR_CLOSER" "$G_CTRL_SITE")"
G2G3_N="$(_g_unredirected "$G_CTRL_ATTR_REVERSED" "$G_CTRL_SITE")"

assert "G2g1: a redirect belonging to ANOTHER command on the same line does not capture the site (got $G2G1_N unredirected)" \
    test "$G2G1_N" -eq 1
assert "G2g2: ... nor does one belonging to another command on the block CLOSER line (got $G2G2_N unredirected)" \
    test "$G2G2_N" -eq 1
assert "G2g3: ... and a REVERSED 2>&1 >file still leaks, because fd 2 is aimed at the inherited stdout first (got $G2G3_N unredirected)" \
    test "$G2G3_N" -eq 1

echo ""
echo "--- G3/G1: every static-only roster member, over its own source ---"

for _g_i in "${!G_MEMBERS[@]}"; do
    _g_m="${G_MEMBERS[$_g_i]}"
    # Empty G_SCAN entry means "scan the member itself" (eight of nine).
    _g_f="${G_SCAN[$_g_i]:-$_g_m}"
    # Shown in both descriptions only when it differs, so a RED names the file
    # a reader must actually open. Basenames, so this stays safe to print.
    _g_via=""
    [ "$_g_f" = "$_g_m" ] || _g_via=" -> $_g_f"
    _g_j="$TMPG/${_g_m}.logical"
    _d_join_logical "$SCRIPT_DIR/$_g_f" > "$_g_j"
    _g_re="(${G_EXEC_VERB_RE}|${G_EXEC_FIRST_RE})${G_SITE[$_g_i]}"
    _g_nsites="$(_g_sites "$_g_j" "$_g_re")"
    _g_nbare="$(_g_unredirected "$_g_j" "$_g_re")"

    # G3 BEFORE G1, the D4a analogue: G1 asserts a ZERO, and a stale or
    # typo'd site target would make that zero green forever. This is the arm
    # that catches a member whose invocation shape MOVED -- the failure mode a
    # declared (rather than derived) table is exposed to, and the reason a
    # declared table is acceptable at all.
    assert "G3 [$_g_m$_g_via]: its declared deadline-capable site is still there (non-vacuity; got $_g_nsites sites)" \
        test "$_g_nsites" -ge 1

    # Counts and the member BASENAME only -- never a matched source line, and
    # `test` as the checker for the same reason D1 and D4b use it: assert dumps
    # a FAILING checker's captured output, so printing the offending line would
    # BE the leak this section exists to prevent. Both counts are precomputed
    # into plain variables above, never a $(...) inside the description, so
    # neither assert can become an instance of what E1 forbids.
    assert "G1 [$_g_m$_g_via]: every one of its deadline-capable sites diverts stderr off the inherited fd 2 (got $_g_nsites sites, $_g_nbare unredirected)" \
        test "$_g_nbare" -eq 0
done

echo ""
echo "=== H: run_all.sh neutralises the SLOT family in re-emitted member output ==="

# THE SYSTEMIC LAYER, and the complement to Sections D/F/G. Those close the
# leak AT SOURCE, per site, per audited member: a deadline-capable site diverts
# its own stderr so its sentinel never enters run_all's `<n>.out` capture at
# all. That is the first line of defence and it stays the first line of
# defence -- but it is whack-a-mole by construction, and it only covers members
# the F/G roster machinery has actually derived. A NEW or UNAUDITED member that
# quotes a live sentinel in assertion prose, or leaks one from a nested
# subprocess, walks straight past it. Section H pins the backstop: run_all.sh
# NEUTRALIZES the slot family in the member output it re-emits, so no member's
# captured bytes can reach dark-factory's classifier wearing a live anchor.
#
# This is the exact two-layer framing the @@REIFY_CLOCK_ family already uses
# (run_all.sh's _RA_CLOCK_SANITIZE block, about the per-source stderr-isolation
# patches of tasks 4802/4887/4931 that could not close the assertion-text half).
#
# WHAT IS DELIBERATELY *NOT* NEUTRALIZED, and must not be: run_all's OWN pool
# wait sentinel. It rides the worker subshell's INHERITED parent fd 2 -- the
# `> .out 2>&1` redirect is scoped to the member `bash` command only -- so it
# never enters the re-emission path this section drives. Section C pins that
# behaviourally (C1/C2/C5), and it is load-bearing: run_all's pool wait is the
# one finite-WAIT path absent from DF's three-basename allowlist, so the
# sentinel is its ONLY classification route.
#
# Shape mirrors tests/infra/test_run_all_clock_marker_sanitize.sh (the same pin
# for the clock family): temp fixture dir + PRIVATE classification manifest +
# the pool env knobs, with pool-path/fixture-ran preconditions asserted FIRST so
# nothing here can pass vacuously via the legacy serial fallback (which this
# sanitizer deliberately does not cover).
#
# RECURSION NOTE: run_all.sh is driven against a TEMP fixture dir only, never
# the real tests/infra/ (this file is itself auto-discovered by the outer
# run_all).
#
# SELF-POLLUTION: $H_OUT deliberately HOLDS live tokens before the fix and is
# the assertion target after it. It stays in its temp dir -- never cat/echoed --
# and every assertion below reports a COUNT or a member BASENAME only, per the
# discipline at the top of this file and the E1 lint.

# The neutralized prefix run_all must produce. Not a live token, so it is safe
# as a literal; assembled beside $SP so the two stay visibly paired.
QSP='@@REIFY_QUOTED_SLOT_'

TMPH="$(mktemp -d)"; _TMPDIRS+=("$TMPH")

# Fixture member emitting BOTH pollution shapes -- bare column-0 and
# quoted-in-prose -- on BOTH stdout and stderr (run_all merges them into one
# `.out`). Tokens come from $SP via an UNQUOTED heredoc, so no live token is
# ever contiguous in THIS file's source; `\$\$` stays literal and expands at
# fixture runtime.
#
# It also emits all FOUR GROUNDED SHAPES of dark-factory's OTHER slot anchor --
# the per-wrapper `<basename>.sh: failed to acquire ... within Ns` deadline
# lines. That anchor is a SEPARATE half of the same classification (H2): those
# lines carry no `@@` token at all, so no prefix rewrite can reach them, and
# the 5623 mislabel was driven by exactly this half with ZERO sentinels
# present. The four shapes are transcribed from their real emitters:
#   1. bare column-0, with the real HG-2 multi-line-continuation trailing `))`
#      (scripts/lib_test_semaphore.sh)
#   2. leading-whitespace         (scripts/lib_lane_x_flock.sh)
#   3. `ERROR: `-prefixed -- the sole grounded source of that prefix
#      (scripts/cargo-test-occt-gated.sh)
#   4. the `within unlimiteds` wart REIFY_TEST_SEMAPHORE_WAIT=unlimited
#      produces, which DF's `\S+s\b` deliberately covers
# Split across stdout and stderr, as the two real streams are.
#
# Finally it emits both live forms QUOTED INSIDE assert-shaped `  FAIL: ` lines
# -- the shape run_all's _ra_collect_fail_detail actually harvests (its branch-1
# `^[[:space:]]*FAIL:` anchor). That is what gives H3 a second, independent
# application site to assert on; it mirrors test_run_all_clock_marker_sanitize.sh's
# `test_marker_flaky.sh`, which exists for exactly the same reason. They are
# harmless on the PASSING fixture: the collector runs only for a failed member.
_write_slot_fixture() {  # <path> <exit_code>
    cat > "$1" <<SLOTFIXEOF
#!/usr/bin/env bash
echo "${SP}TIMEOUT@@ reason=fixture slots=1 waited=0 disposition=fatal lock=/tmp/x"
echo "  PASS: fixture: stdout quotes ${SP}TIMEOUT@@ reason=fixture in assertion prose"
echo "lib_test_semaphore.sh: failed to acquire test slot within 0s (LOCK=/tmp/l, N=1))"
echo "  lib_lane_x_flock.sh: failed to acquire Lane-X lock within 1s (LOCK=/tmp/m)"
echo "${SP}TIMEOUT@@ reason=fixture slots=1 waited=0 disposition=fatal lock=/tmp/x.\$\$" >&2
echo "  PASS: fixture: stderr quotes ${SP}TIMEOUT@@ reason=fixture in assertion prose" >&2
echo "ERROR: cargo-test-occt-gated.sh: failed to acquire OCCT slot within 1800s (LOCK=/o, N=1)" >&2
echo "lib_test_semaphore.sh: failed to acquire test slot within unlimiteds (LOCK=/x, N=8)" >&2
echo "  FAIL: fixture: stderr contains ${SP}TIMEOUT@@ reason=fixture (assertion prose)"
echo "  FAIL: fixture: stderr contains lib_test_semaphore.sh: failed to acquire test slot within 0s (LOCK=/tmp/l, N=1)"
SLOTFIXEOF
    echo "exit $2" >> "$1"
    chmod +x "$1"
}

cat > "$TMPH/classification.manifest" <<'EOF'
test_slot_pass.sh pool
test_slot_flaky.sh pool
EOF
_write_slot_fixture "$TMPH/test_slot_pass.sh" 0
# The FAILING twin drives the OTHER two re-emit shapes: the Phase-2.5 serial
# retry's dual-attempt emission, and _ra_collect_fail_detail -> the Summary
# FAILED-DETAIL region, which is the text verify.sh / DF's merge-gate block
# reason quotes verbatim. H3 below is scoped to that region.
_write_slot_fixture "$TMPH/test_slot_flaky.sh" 1

H_OUT="$TMPH/ra_out.txt"
RUN_ALL_CLASSIFICATION_MANIFEST="$TMPH/classification.manifest" \
    REIFY_RUN_ALL_POOL_LOCK="$TMPH/pool.lock" \
    REIFY_RUN_ALL_POOL_CONCURRENCY=2 \
    REIFY_RUN_ALL_POOL_PSI_DISABLE=1 \
    timeout 300 bash "$RUN_ALL" "$TMPH" > "$H_OUT" 2>&1 || true

# H0 FIRST: without both of these, H1 could pass vacuously via the legacy
# serial fallback (no pool re-emission at all, hence nothing to rewrite).
assert "H0a: the pool path was actually taken (INFO: run_all.sh pool: N= present)" \
    _has_text "$H_OUT" "INFO: run_all.sh pool: N="
assert "H0b: the fixture member actually ran (the normal re-emit site)" \
    _has_text "$H_OUT" "--- Running: test_slot_pass.sh ---"

# $D_ANCHOR is dark-factory's live sentinel anchor, transcribed once in Section
# D (`^[[:blank:]]*` + the token) and reused here rather than retyped, so the
# two arms of this file can never drift apart. `|| true` because grep -c exits
# 1 on a zero count, which `set -e` would otherwise turn into a suite abort.
H1_LIVE_N="$(grep -acE -- "$D_ANCHOR" "$H_OUT" || true)"

assert "H1a: the member's sentinels were rewritten to the neutralized QUOTED form" \
    _has_text "$H_OUT" "${QSP}TIMEOUT@@"
assert "H1b: ZERO re-emitted lines still match DF's live sentinel anchor (got $H1_LIVE_N)" \
    test "$H1_LIVE_N" -eq 0

echo ""
echo "--- H2: the BASENAME deadline half, DF's other slot anchor ---"

# $H_DF_ANCHOR is dark-factory's live basename-deadline anchor, transcribed
# ONCE near the top of this file (beside $SP) because A6d needs it too --
# duplicating a cross-repo regex is precisely the drift this suite exists to
# catch.

H2_LIVE_N="$(grep -acE -- "$H_DF_ANCHOR" "$H_OUT" || true)"

assert "H2a: ZERO re-emitted lines still match DF's live basename-deadline anchor (got $H2_LIVE_N)" \
    test "$H2_LIVE_N" -eq 0

# H2b: the rewrite must NEUTRALIZE, not destroy. A human reading the verify log
# still needs the emitting script, the verb, and the operator-controlled lock
# path -- so each of the three basenames keeps all three. Literal checks only,
# so a rewrite that silently dropped a field turns this RED.
assert "H2b1: the lib_test_semaphore deadline line stays human-readable (basename kept)" \
    _has_text "$H_OUT" "lib_test_semaphore.sh"
assert "H2b2: ... its LOCK= field survives the rewrite" \
    _has_text "$H_OUT" "LOCK=/tmp/l"
assert "H2b3: the lib_lane_x_flock deadline line stays human-readable (basename kept)" \
    _has_text "$H_OUT" "lib_lane_x_flock.sh"
assert "H2b4: ... its LOCK= field survives the rewrite" \
    _has_text "$H_OUT" "LOCK=/tmp/m"
assert "H2b5: the cargo-test-occt-gated deadline line stays human-readable (basename kept)" \
    _has_text "$H_OUT" "cargo-test-occt-gated.sh"
assert "H2b6: ... its LOCK= field survives the rewrite" \
    _has_text "$H_OUT" "LOCK=/o"
assert "H2b7: the shared verb survives on all three" \
    _has_text "$H_OUT" "failed to acquire"

# H2c: THE INDENT-IS-NOT-ENOUGH CONTROL, and the reason the chosen transform
# inserts a token rather than shifting the line. DF's anchor tolerates leading
# horizontal whitespace (`^[ \t]*`), so the obvious cheap "just indent it"
# simplification neutralizes NOTHING -- this assertion is what makes that a
# machine-checked fact instead of a comment, and turns RED the day someone
# swaps the rewrite for an indent. Also doubles as H2a's positive control: an
# absence-assert with a typo'd anchor is green forever.
H2C_IN="$TMPH/h2c-indented.out"
printf '  lib_test_semaphore.sh: failed to acquire test slot within 0s (LOCK=/tmp/l, N=1)\n' > "$H2C_IN"
H2C_N="$(grep -acE -- "$H_DF_ANCHOR" "$H2C_IN" || true)"

assert "H2c: a merely INDENTED deadline line still matches DF's anchor, so indentation is not a defence (got $H2C_N)" \
    test "$H2C_N" -eq 1

echo ""
echo "--- H3: the SECOND application site, _ra_collect_fail_detail's Summary region ---"

# Sanitizing only the per-member re-emission is not enough. run_all harvests a
# failed pool member's `  FAIL: ` lines and REPRINTS them in the Summary's
# FAILED-DETAIL region -- and THAT region is the one verify.sh and dark-factory's
# merge-gate block reason quote verbatim, so a live anchor there is if anything
# more consequential than one in the bulk output. The clock rule is applied at
# BOTH sites for exactly this reason; H3 pins that the two slot rules are too.
#
# SCOPED EXTRACTION is mandatory, not tidiness: the same FAIL lines also appear
# in the per-member re-emission, where H1/H2 already prove they are sanitized.
# A whole-file count could therefore go green on the strength of the FIRST site
# alone and never see the second. Slicing to the FAILED-DETAIL region is what
# makes this assertion about the collector.
H3_REGION="$TMPH/h3-failed-detail.out"
sed -n '/^--- FAILED-DETAIL: /,/^--- END FAILED-DETAIL: /p' "$H_OUT" > "$H3_REGION"

# H3a/H3b FIRST: both are non-vacuity preconditions. Without the retry branch
# there is no dual-attempt emission, and without a FAILED-DETAIL block the
# extracted region is EMPTY -- in which case H3c/H3d would count zero and pass
# while proving nothing at all.
assert "H3a: the failing fixture took the retry/dual-attempt emit branch" \
    _has_text "$H_OUT" "--- attempt 2 (serial retry) ---"
assert "H3b: the Summary FAILED-DETAIL block was emitted for that member" \
    _has_text "$H_OUT" "--- FAILED-DETAIL: test_slot_flaky.sh ---"

H3_SENTINEL_N="$(grep -acE -- "$D_ANCHOR" "$H3_REGION" || true)"
H3_BASENAME_N="$(grep -acE -- "$H_DF_ANCHOR" "$H3_REGION" || true)"

# H3c/H3d are the ANCHORED arm, and they are honest about what they are: on
# TODAY's collector grammar both are structurally-guaranteed zeros, because
# _ra_collect_fail_detail harvests only lines that themselves begin (modulo
# leading blanks) with `FAIL:` or `<TOKEN> FAIL`, which no `^[ \t]*`-anchored
# slot line can. MEASURED, not assumed: with the collector unsanitized, this
# region carries both live forms MID-LINE and neither anchor matches. They are
# kept as a standing guard on that structural claim -- if the collector's own
# anchors are ever loosened (a `-A`/context flag, a whole-block harvest, a
# third branch), these are what turn RED. The arm that is actually load-bearing
# TODAY is H3e/H3f below.
assert "H3c: ZERO lines in the FAILED-DETAIL region match DF's live sentinel anchor (got $H3_SENTINEL_N)" \
    test "$H3_SENTINEL_N" -eq 0
assert "H3d: ZERO lines in the FAILED-DETAIL region match DF's live basename-deadline anchor (got $H3_BASENAME_N)" \
    test "$H3_BASENAME_N" -eq 0

# H3e/H3f: SUBSTRING absence, which is the strictly stronger property and the
# one that is RED until the collector applies the two new rules. This is not a
# gratuitous tightening -- it is the assertion shape the sibling pin for the
# clock family uses for its CORE cases
# (tests/infra/test_run_all_clock_marker_sanitize.sh, `_out_lacks`), for the
# reason run_all.sh's own _RA_CLOCK_SANITIZE block states: the rewrite exists to
# break BOTH substring and line-anchored matching, because DF's matcher was
# HISTORICALLY substring-based and was only line-anchored later (Layer 2). A
# quoted-in-prose live token in the one region DF's merge-gate block reason
# quotes verbatim is exactly the residue that regression would re-expose.
# H3g is their non-vacuity: without it an empty or unrelated region would
# satisfy both absences while proving nothing.
assert "H3e: no live SLOT sentinel token survives ANYWHERE in the FAILED-DETAIL region, mid-line included" \
    _lacks_text "$H3_REGION" "${SP}TIMEOUT@@"
assert "H3f: no live basename-deadline literal survives there either" \
    _lacks_text "$H3_REGION" "lib_test_semaphore.sh: failed to acquire "
assert "H3g: ... and the region really did carry those FAIL lines, in neutralized form (non-vacuity)" \
    _has_text "$H3_REGION" "${QSP}TIMEOUT@@"

echo ""
echo "--- H4: the fd-2 breadcrumb that keeps a neutralized line debuggable ---"

# The sentinel rewrite announces itself -- `@@REIFY_QUOTED_SLOT_TIMEOUT@@` is
# visibly not the live token. The BASENAME rewrite deliberately does not: a
# `[quoted]` line reads identically whether the member was QUOTING the message
# or had actually hit a real rc=75 deadline and continued. run_all's own fd-2
# sentinel covers the runner's own starvation and DF's failing-leg scoping
# covers the abort-loudly case; the case in between had no breadcrumb at all,
# so $_ra_note_slot_rewrite leaves one. H4 pins BOTH halves of what that line
# has to be: present and member-named (or it is useless to the human it exists
# for), and inert to DF's classifier (or the debuggability aid becomes a
# fourth pollution source of its own).
H4_LINES="$TMPH/h4-breadcrumbs.out"
grep -aF -- 'INFO: run_all.sh neutralized ' "$H_OUT" > "$H4_LINES" || true
H4_N="$(grep -c . "$H4_LINES" || true)"
H4_SENTINEL_N="$(grep -acE -- "$D_ANCHOR" "$H4_LINES" || true)"
H4_BASENAME_N="$(grep -acE -- "$H_DF_ANCHOR" "$H4_LINES" || true)"

assert "H4a: run_all announced the basename rewrite on its own fd 2, naming the member (got $H4_N breadcrumb line(s))" \
    _has_text "$H4_LINES" "from test_slot_pass.sh"
assert "H4b1: the breadcrumb matches NEITHER of DF's slot anchors -- not the sentinel one (got $H4_SENTINEL_N)" \
    test "$H4_SENTINEL_N" -eq 0
assert "H4b2: ... nor the basename-deadline one (got $H4_BASENAME_N)" \
    test "$H4_BASENAME_N" -eq 0
assert "H4c: it carries no live SLOT token of its own -- it is a diagnostic, not a re-emission" \
    _lacks_text "$H4_LINES" "${SP}TIMEOUT@@"

test_summary
