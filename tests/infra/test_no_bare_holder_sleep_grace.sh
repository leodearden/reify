#!/usr/bin/env bash
# tests/infra/test_no_bare_holder_sleep_grace.sh
#
# Regression guard (task #6247, PRD infra-test-wallclock-deflake.md D4):
#   Flags a BARE FIXED SLEEP used as a holder-acquisition grace in
#   tests/infra/*.sh, so the idiom task 6247 removed cannot silently return.
#
# WHAT IS WRONG WITH THE IDIOM. `sleep 0.2  # give holder time to acquire`
# guesses at a duration on both sides at once. It can be OUTRUN -- the holder is
# not holding yet, so the code under test takes the uncontended path, and the
# assertions about contention pass vacuously or fail for the wrong reason. And
# where the holder is itself self-timed it can be OVERRUN -- the grace eats into
# the hold, so the contention window the test needs shrinks below what the
# assertions require. A causal barrier closes the first side and a test-released
# holder closes the second; tests/infra/slot_holder_handshake_lib.sh is where
# both live.
#
# The guard itself is a LOAD-INDEPENDENT static scan -- it is NOT a timing test.
#
# TWO CLAUSES:
#   (A) LEXEME. A `sleep <number>` statement whose own inline comment, or the
#       comment line directly above it, names the thing it is waiting for
#       (holder / grace / "give ... time to" / "let ... acquire"). The comment
#       is the admission: the author knew what the causal event was and slept
#       for a guessed interval instead of waiting for it.
#   (B) STRUCTURAL. A bare `sleep <number>` within three lines of a
#       backgrounded `flock -x` holder spawn, with no loop keyword in between.
#       This catches the same idiom stripped of its comment, which the lexeme
#       clause alone cannot see.
#
# WHAT IS DELIBERATELY LEGAL. A barrier's OWN bounded poll loop sleeps between
# probes -- that is how a barrier is built, and clause B's loop-keyword
# exemption plus clause A's comment requirement keep every such loop unflagged.
# Escape a deliberate survivor with `# holder-sle''ep:allow -- <reason>` (spelled
# without the split in real code); tests/infra/README.md records why each one is
# blessed.
#
# SELF-MATCH SAFETY: every pattern fragment and every fixture string is
# assembled from '' -split parts, so this source file contains no literal
# flaggable construct. Fixtures are written into `mktemp -d` dirs and NEVER into
# tests/infra/, following test_no_new_wallclock_upper_bounds.sh.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob; declared
# `pool` in run-all-classification.manifest -- it is hermetic (its own mktemp
# fixtures, a read-only scan of the tree) and nests no suite.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== Bare holder-grace sleep regression guard (task 6247) ==="

# --- Fixture vocabulary -------------------------------------------------------
# Assembled from '' -split parts so this file carries no literal instance of the
# idiom it flags (self-match safety).
_HS_SLEEP='sle''ep'
_HS_GRACE='giv''e hol''der tim''e to acquire'
_HS_ESC='holder-sle''ep:allow'
_HS_SPAWN='( floc''k -x 9; sle''ep 45 ) 9>>"$_lock" &'
_HS_PROBE='whil''e ! ( floc''k -n -x 9 ) 9>>"$_lock"; do'

# Every fixture lives under ONE root, removed by ONE handler. A per-fixture
# cleanup LIST cannot work here: `_hs_fixture` is always called as `$( ... )`,
# so anything it appended would be appended to the SUBSHELL's copy and
# discarded on substitution, leaving each fixture directory behind.
_HS_ROOT="$(mktemp -d)"
trap 'rm -rf "$_HS_ROOT"' EXIT

# _hs_fixture LINE... -- write a fixture script into a fresh directory under
# _HS_ROOT and echo that directory. Each LINE is emitted verbatim, so a case
# reads as the shell it plants rather than as a printf format.
_hs_fixture() {
    local _d; _d="$(mktemp -d -p "$_HS_ROOT")"
    local _l
    printf '#!/usr/bin/env bash\n' > "$_d/fixture.sh"
    for _l in "$@"; do printf '%s\n' "$_l" >> "$_d/fixture.sh"; done
    echo "$_d"
}

# ---------------------------------------------------------------------------
# _detect_bare_holder_sleep_grace <dir> [exclude_basename]
#
# Scans "$dir"/*.sh non-recursively (minus <exclude_basename>) for a fixed sleep
# standing in for a holder-acquisition barrier. A physical line is a candidate
# only if it IS a sleep statement -- `^[[:space:]]*sleep[[:space:]]+[0-9]` --
# which is what keeps a self-timed holder body such as `( flock -x 9; sleep 45 )`
# out of the candidate set: there the sleep is the HOLD, not a grace, and clause
# B needs that line intact as its anchor.
#
# A candidate is a violation iff, after the escape check:
#   (A) a holder-grace lexeme appears in the candidate's own inline comment or
#       in the comment line directly above it; OR
#   (B) the candidate falls within three lines after a backgrounded `flock -x`
#       holder spawn, with no loop keyword anywhere from the spawn to the
#       candidate.
#
# The escape token is checked FIRST, and on the lookback line as well as the
# candidate, so a survivor can be blessed from either half of the clause-A form.
#
# Per-physical-line `[[ =~ ]]`, not `echo | grep` per line: the live scan covers
# ~190 files, and a per-line pipe is also the esc-4574-42 EINTR class. No
# logical-line joiner is needed here (unlike the wall-clock guard, whose
# construct spans continuations) -- this idiom is always ONE physical line, and
# clause B's window is defined in PHYSICAL lines by construction.
#
# Prints each violation as "file:lineno: <content>" to stderr.
# Returns 1 if any violations found, 0 if none.
# ---------------------------------------------------------------------------
_detect_bare_holder_sleep_grace() {
    local dir="$1"
    local exclude_base="${2:-}"

    # Pattern fragments, '' -split so this source carries no literal instance of
    # what it flags. Alternated upper/lower initials rather than a nocasematch
    # shopt, which would leak into every other match in this file.
    local _esc_re;   _esc_re='holder-sle''ep:allow'
    local _sleep_re; _sleep_re='^[[:space:]]*sle''ep[[:space:]]+[0-9]'
    local _lex_re;   _lex_re='[Gg]iv''e[[:space:]].*[[:space:]]tim''e[[:space:]]to|[Hh]old''er|[Gg]rac''e|[Ll]e''t[[:space:]].*[[:space:]]acquir''e'
    local _spawn_re; _spawn_re='floc''k[[:space:]]+-x.*&[[:space:]]*$'
    local _loop_re;  _loop_re='(^|[^[:alnum:]_])(whil''e|unti''l|don''e)([^[:alnum:]_]|$)'

    # Violations accumulate in a local array, not a temp file: nothing outlives
    # the call, so there is no cleanup path to get wrong. (A `trap ... RETURN`
    # cleanup would fire only after these locals are popped, which is both a
    # `set -u` abort and a leak of whatever it was meant to remove.)
    local -a _viol=()

    local f
    for f in "$dir"/*.sh; do
        [ -f "$f" ] || continue
        local base; base="$(basename "$f")"
        if [ -n "$exclude_base" ] && [ "$base" = "$exclude_base" ]; then
            continue
        fi

        # `since_spawn` counts physical lines since the last holder spawn, or -1
        # when no spawn is in reach; `window_loop` records whether a loop keyword
        # has appeared since that spawn, and is cleared by each new spawn.
        local line prev="" lineno=0 since_spawn=-1 window_loop=0
        local dist inline above
        while IFS= read -r line || [ -n "$line" ]; do
            lineno=$(( lineno + 1 ))
            dist=-1
            if [ "$since_spawn" -ge 0 ]; then dist=$(( since_spawn + 1 )); fi
            # Evaluated before the clause-B test so a loop keyword on the
            # candidate's own line exempts it too.
            if [[ "$line" =~ $_loop_re ]]; then window_loop=1; fi

            if [[ "$line" =~ $_sleep_re ]] \
               && ! [[ "$line" =~ $_esc_re ]] \
               && ! [[ "$prev" =~ $_esc_re ]]; then
                inline=""
                case "$line" in *'#'*) inline="${line#*#}" ;; esac
                above=""
                case "$prev" in [[:space:]]*'#'*|'#'*) above="$prev" ;; esac

                if [[ "$inline" =~ $_lex_re ]] || [[ "$above" =~ $_lex_re ]]; then
                    _viol+=("${f}:${lineno}: ${line}")
                elif [ "$dist" -ge 1 ] && [ "$dist" -le 3 ] && [ "$window_loop" -eq 0 ]; then
                    _viol+=("${f}:${lineno}: ${line}")
                fi
            fi

            if [[ "$line" =~ $_spawn_re ]]; then
                since_spawn=0
                window_loop=0
            elif [ "$since_spawn" -ge 0 ]; then
                since_spawn="$dist"
                if [ "$since_spawn" -gt 3 ]; then since_spawn=-1; fi
            fi
            prev="$line"
        done < "$f"
    done

    if [ "${#_viol[@]}" -gt 0 ]; then
        printf '%s\n' "${_viol[@]}" >&2
        return 1
    fi
    return 0
}

# _hs_scan_rc DIR -- echo the detector's exit code (0 clean, 1 violations).
# Each case asserts an EXACT code, so a detector that failed to load would
# report 127 and fail every case rather than accidentally satisfying one.
_hs_scan_rc() {
    local _rc=0
    _detect_bare_holder_sleep_grace "$1" "${2:-}" 2>/dev/null || _rc=$?
    echo "$_rc"
}

# ===========================================================================
# Section A: clause A -- the comment names what the sleep is waiting for
# ===========================================================================
echo ""
echo "--- Section A: lexeme clause (inline comment / comment directly above) ---"

_a1="$(_hs_fixture "$_HS_SLEEP 0.2   # $_HS_GRACE")"
assert "A1: the retired idiom -- an inline grace comment on a fixed sleep -- is flagged (returns 1)" \
    test "$(_hs_scan_rc "$_a1")" -eq 1

_a2="$(_hs_fixture "# $_HS_GRACE" "$_HS_SLEEP 0.2")"
assert "A2: the same admission on the comment line DIRECTLY ABOVE is flagged (returns 1)" \
    test "$(_hs_scan_rc "$_a2")" -eq 1

_a3="$(_hs_fixture "$_HS_SLEEP 0.2   # $_HS_GRACE -- $_HS_ESC: one-sided, can only false-FAIL")"
assert "A3: a blessed survivor carrying the escape token is NOT flagged (returns 0)" \
    test "$(_hs_scan_rc "$_a3")" -eq 0

# The false-positive control that keeps every barrier implementation legal: a
# poll loop sleeps between probes by construction, and carries no admission.
_a4="$(_hs_fixture "$_HS_PROBE" "    $_HS_SLEEP 0.2" 'done')"
assert "A4: a barrier's own bounded poll loop (no grace comment) is NOT flagged (returns 0)" \
    test "$(_hs_scan_rc "$_a4")" -eq 0

# ===========================================================================
# Section B: clause B -- the idiom stripped of its comment
# ===========================================================================
echo ""
echo "--- Section B: structural clause (bare sleep just after a holder spawn) ---"

_b1="$(_hs_fixture "$_HS_SPAWN" '_HOLDER_PID=$!' "$_HS_SLEEP 0.3")"
assert "B1: an UNCOMMENTED fixed sleep two lines after a backgrounded flock -x spawn is flagged (returns 1)" \
    test "$(_hs_scan_rc "$_b1")" -eq 1

_b2="$(_hs_fixture "$_HS_SPAWN" '_HOLDER_PID=$!' "$_HS_SLEEP 0.3   # $_HS_ESC: the probe below is the real barrier")"
assert "B2: the same line carrying the escape token is NOT flagged (returns 0)" \
    test "$(_hs_scan_rc "$_b2")" -eq 0

_b3="$(_hs_fixture "$_HS_SPAWN" "$_HS_PROBE" "    $_HS_SLEEP 0.2" 'done')"
assert "B3: a holder spawn followed by a while-guarded poll loop is NOT flagged (returns 0)" \
    test "$(_hs_scan_rc "$_b3")" -eq 0

# Pins the stated window. A sleep far from any spawn is ordinary pacing, not a
# holder grace; without this control the clause could widen to the whole file
# and every case above would still pass.
_b4="$(_hs_fixture "$_HS_SPAWN" '_HOLDER_PID=$!' '_a=1' '_b=2' '_c=3' "$_HS_SLEEP 0.3")"
assert "B4: a bare sleep FIVE lines after the spawn is outside the window and NOT flagged (returns 0)" \
    test "$(_hs_scan_rc "$_b4")" -eq 0

# ===========================================================================
# Section C: degenerate inputs
# ===========================================================================
echo ""
echo "--- Section C: degenerate inputs ---"

_c1="$(_hs_fixture '_x=1' 'echo hello')"
assert "C1: a fixture with no sleep at all returns 0" \
    test "$(_hs_scan_rc "$_c1")" -eq 0

_c2="$(mktemp -d -p "$_HS_ROOT")"
assert "C2: an empty directory returns 0" \
    test "$(_hs_scan_rc "$_c2")" -eq 0

# ===========================================================================
# Section D: LIVE scan of the real tests/infra
#
# PRD D4 lands this guard only after the suite is clean, so this is the capstone
# that pins task 6247's removals: steps 12/14/16 converted every in-scope site to
# a causal barrier, and this assertion is what stops the idiom coming back.
# ===========================================================================
echo ""
echo "--- Section D: live scan of real tests/infra ---"

# Exclude this file by basename. Its own source carries no literal flaggable
# construct (see SELF-MATCH SAFETY above), so this is belt-and-braces rather
# than load-bearing -- and it is what lets the fixture vocabulary above stay
# readable.
_guard_base="$(basename "${BASH_SOURCE[0]}")"

assert "live scan: no un-escaped bare holder-grace sleeps in tests/infra (returns 0)" \
    test "$(_hs_scan_rc "$SCRIPT_DIR" "$_guard_base")" -eq 0

test_summary
