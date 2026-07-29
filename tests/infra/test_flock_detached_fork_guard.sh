#!/usr/bin/env bash
# tests/infra/test_flock_detached_fork_guard.sh
#
# Repo-wide regression guard (task #5817):
#   Flags a tracked shell script that BOTH (a) acquires a flock on a numbered
#   FD **it opens itself**, AND (b) forks a DETACHED background child (`... &`)
#   downstream of that acquire, WITHOUT (c) an explicit `flock -u <fd>` release.
#
#   Zero offenders exist today.  The guard exists so the class cannot bite a
#   THIRD time (2026-04-20 sccache/FD-9 wedge; 2026-07-28 seed-warm-lane.sh,
#   fixed by #5705).
#
# The guard is a LOAD-INDEPENDENT static scan: the verdict is a function of
# script TEXT only — no filesystem stat, no network, no model call — so it is
# host-independent and deterministic (the scripts/lock-charter-guard.sh C-P3
# discipline).  That is what lets it sit in the hermetic `pool` bucket.
#
# ── The four line classes ────────────────────────────────────────────────────
#   OPEN(N)    — a depth-0, non-comment, non-continuation line that OPENS FD N
#                (`exec <ws> N` followed by `>`, `>>` or `<` whose next char is
#                not `&`).  A close (`exec N>&-`) and a dup (`exec N>&M`) are
#                deliberately NOT opens.
#   ACQUIRE(N) — a depth-0, non-comment, non-continuation line invoking `flock`
#                in command position with N as a bare, tail-anchored FD operand,
#                occurring after OPEN(N).
#   UNLOCK(N)  — a non-comment line running `flock` in command position with
#                `-u` and a bare, tail-anchored operand N.  FILE-LEVEL and not
#                order-sensitive: the sanctioned remedy is a `flock -u N` inside
#                a function registered as an EXIT trap, whose textual position
#                relative to the fork carries no meaning.
#   DETACH     — a non-comment line, at subshell depth 0 after its own paren
#                delta, whose FINAL EFFECTIVE TOKEN is a bare `&`.
#
#   FLAG iff there is an N with OPEN(N) < ACQUIRE(N) < DETACH and NO UNLOCK(N).
#
# ── The two false-positive exemptions are STRUCTURAL, not allow-listed ───────
#   FOREGROUND children are SAFE.  Only a line whose final effective token is a
#   bare `&` is a DETACH, so `"$@" 9<&-` (scripts/lib_test_semaphore.sh) can
#   never enter the candidate set.
#   INHERITED-FD paths are SAFE.  A candidate requires a LOCAL open, so a file
#   that merely inherits FD 9 from its caller is never considered — the guard is
#   structurally incapable of advising a `flock -u 9` that would release the
#   CALLER's lock.  (scripts/seed-warm-lane.sh documents that hazard at its
#   contract block and installs its trap INSIDE the acquire branch for the same
#   "make it impossible rather than conditionally avoided" reason.)
#
# ── SELF-MATCH SAFETY ────────────────────────────────────────────────────────
#   This guard scans every tracked shell file, including itself.  Fixture bodies
#   carrying the offending shape are therefore ASSEMBLED FROM SHELL VARIABLES at
#   runtime and written only into a `mktemp -d` dir — never emitted as literal
#   source lines (the test_no_new_wallclock_upper_bounds.sh /
#   test_reify_audit_ptodo.sh convention).  A dedicated assertion pins that this
#   file scans clean.
#
# ── HONEST LIMITATION (deliberate, not an oversight) ─────────────────────────
#   A file-local syntactic scan cannot follow FD provenance across `source`.
#   tests/infra/run_all.sh holds the Lane-X FD 9 opened inside the sourced
#   scripts/lib_lane_x_flock.sh and forks pool workers with `) &`; it contains no
#   local open, so it is out of criterion (a)'s stated shape.  It is
#   independently safe — each worker runs `bash ... 9<&-` and closes with
#   `exec 9>&-`.  Chasing provenance across `source` would need interprocedural
#   analysis and would forfeit the host-independent, no-stat property above.
#
# WHY an explicit unlock and not a close, plus the measured held-after-exit
# rates: the LANE-LOCK RELEASE CONTRACT block at the flock acquire in
# scripts/seed-warm-lane.sh is the single source of truth (G7 — not restated
# here).
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== flock-held-across-detached-fork regression guard ==="

TMPWORK="$(mktemp -d)"
trap 'rm -rf "$TMPWORK"' EXIT

# ---------------------------------------------------------------------------
# _write_fixture <basename> <line>...
#
# Writes a fixture script to "$TMPWORK/<basename>", one argument per line.
#
# SELF-MATCH SAFETY: callers MUST assemble any offending token from shell
# variables (adjacent-single-quote '' splitting) rather than writing it as a
# literal line here — see the header.  This helper exists so every fixture
# lands under the mktemp dir and never in the tracked corpus.
# ---------------------------------------------------------------------------
_write_fixture() {
    local name="$1"
    shift
    local path="$TMPWORK/$name"
    printf '%s\n' "$@" >"$path"
}

# ---------------------------------------------------------------------------
# _flock_fork_offenders <file>...
#
# THE DETECTOR.  One awk pass per file; fixtures and the real corpus share this
# single implementation so a control can never drift away from the live scan.
#
# Prints one line per hit to stdout:
#   OFFENDER <file> fd=<N> open=<lineno> acq=<lineno> detach=<lineno>
# Returns 1 if any hit was emitted across all inputs, 0 if none.
#
# Line classes are as documented in the header.  Comment lines (trimmed content
# starting with `#`) are never any class.
# ---------------------------------------------------------------------------
_flock_fork_offenders() {
    local _f _rc=0 _out
    for _f in "$@"; do
        [ -f "$_f" ] || continue
        _out="$(awk -v FNAME="$_f" '
            # Tail anchor for a bare FD operand: the operand must END the
            # command, so a `9` embedded in prose (followed by a backtick, a
            # word, ...) is never read as an FD.
            BEGIN { ANCH = "([[:space:]]*(;|&&|[|][|]|2>)|[[:space:]]*$)" }

            # COMMAND POSITION: everything before the flock token is empty, a
            # command separator, or a shell keyword that introduces a command.
            # This is what keeps a `flock -u N` inside usage/help prose or a
            # quoted pattern from counting.
            function is_cmd_pos(p,   lw) {
                sub(/[[:space:]]+$/, "", p)
                if (p == "") return 1
                if (p ~ /[;&|({!]$/) return 1
                lw = p
                sub(/^.*[[:space:]]/, "", lw)
                return (lw == "if" || lw == "then" || lw == "elif" ||
                        lw == "else" || lw == "do" || lw == "while" ||
                        lw == "until")
            }

            # A standalone `(` before the flock token means the acquire happens
            # in a SUBSHELL — the forking shell never holds that lock.
            function has_subshell_open(p) {
                return (p ~ /(^|[[:space:]])\(([[:space:]]|$)/)
            }

            # DETACH: the FINAL EFFECTIVE TOKEN of the line — after stripping a
            # trailing `# ...` comment tail — is a bare `&`.  Matched as "a
            # character that is not `&`, `>` or `|`, followed by `&`, followed
            # only by optional trailing whitespace".
            #
            # This is FALSE-POSITIVE TRAP #1 made structural: a FOREGROUND child
            # is SAFE — the parent holds the lock for exactly as long as the
            # child runs and reaps it before exiting — so flagging that shape
            # would push scripts toward releasing a lock they are still
            # legitimately using.  Admits `) &`, `} 9<&- &`, `cmd &` and
            # `( ... ) 9>"$L" &`; rejects `&&`, `>&2`, `2>&1`, and a
            # non-terminal `9<&-` such as the foreground `"$@" 9<&-` in
            # scripts/lib_test_semaphore.sh.
            function is_detach(s) {
                sub(/[[:space:]]+#.*$/, "", s)
                return (s ~ /[^&>|]&[[:space:]]*$/)
            }

            # Subshell-depth delta for one line, counted ONLY over STANDALONE
            # paren tokens:
            #   `(` at start-of-line or after whitespace AND followed by
            #       whitespace or end-of-line            -> +1
            #   `)` at start-of-line or after whitespace AND followed by
            #       whitespace, end-of-line, `&`, `>`, `|` or `;`  -> -1
            #
            # Everything else is deliberately invisible: `$( ... )` (the `(` is
            # preceded by `$`), arithmetic `$(( ... ))`, function headers
            # `f() {` (`(` preceded by a word char, `)` by `(`), and case
            # patterns such as `*)` or `""|*[!0-9]*)` (the `)` is not preceded
            # by whitespace).  A syntactic scan cannot see quoting or heredoc
            # bodies, so this is an approximation -- but a conservative one,
            # and the whole-corpus scan below measures its verdict on the real
            # tree rather than assuming it.
            function paren_delta(s,   d, i, c, prev, nxt, len) {
                d = 0
                len = length(s)
                for (i = 1; i <= len; i++) {
                    c = substr(s, i, 1)
                    if (c != "(" && c != ")") continue
                    prev = (i > 1) ? substr(s, i - 1, 1) : ""
                    nxt = (i < len) ? substr(s, i + 1, 1) : ""
                    if (prev != "" && prev != " " && prev != "\t") continue
                    if (c == "(") {
                        if (nxt == "" || nxt == " " || nxt == "\t") d++
                    } else {
                        if (nxt == "" || nxt == " " || nxt == "\t" ||
                            nxt == "&" || nxt == ">" || nxt == "|" ||
                            nxt == ";") d--
                    }
                }
                return d
            }

            {
                t = $0
                sub(/^[[:space:]]+/, "", t)

                # Is THIS physical line a continuation of the previous one?
                # Captured before prev_cont is overwritten below.  A comment
                # line never continues: bash consumes a `#` comment to the end
                # of the line and a trailing backslash in it carries no
                # meaning.
                is_cont = prev_cont
                if (t == "" || t ~ /^#/) { prev_cont = 0; next }
                prev_cont = ($0 ~ /\\$/)

                # Subshell depth BEFORE and AFTER this line.  OPEN and ACQUIRE
                # qualify on depth-at-START == 0 (a descriptor opened inside a
                # subshell is not the forking shell holding it), while DETACH
                # qualifies on depth-at-END == 0 (so `) &` and
                # `( ... ) 9>"$L" &`, whose net delta returns to 0, still count
                # as forks performed BY the lock-holding shell).
                #
                # Clamped at 0 on the way down: a single miscounted `)` -- from
                # a heredoc body or a quoted string this scan cannot see --
                # must not strand the file at a permanently non-zero depth,
                # which would silently disable the guard for it.  A dead
                # instrument that still reports "clean" is the worst failure
                # mode available here.
                depth_start = depth
                depth += paren_delta(t)
                if (depth < 0) depth = 0
                depth_end = depth

                fpos = index(t, "flock")
                if (fpos > 0) {
                    pre = substr(t, 1, fpos - 1)
                    post = substr(t, fpos)
                    cmdpos = is_cmd_pos(pre)
                } else {
                    pre = ""; post = ""; cmdpos = 0
                }

                # ---- OPEN(N): the script opens FD N ITSELF ----
                # `exec <ws> N` followed by `>`, `>>` or `<` whose next
                # character is NOT `&`.  That excludes the close `exec N>&-`
                # and the dup `exec N>&M`, neither of which creates the open
                # file description whose lock this shell would be carrying.
                #
                # This one rule is what makes FALSE-POSITIVE TRAP #2 —
                # inherited FDs — STRUCTURAL rather than allow-listed: with no
                # OPEN(N) there is no ACQUIRE(N) candidate, so the guard cannot
                # advise a `flock -u N` on a descriptor it never saw this file
                # open, which on an inherited FD would release the CALLER lock.
                #
                # A close must also NOT count as a CLEARING signal: the real
                # historical offender closed FD 9 on its refusal branches
                # UPSTREAM of the fork, so crediting that would have passed it.
                if (!is_cont && depth_start == 0 &&
                    match(t, /exec[[:space:]]+[0-9]+(>>?|<)[^&]/)) {
                    seg = substr(t, RSTART, RLENGTH)
                    match(seg, /[0-9]+/)
                    n = substr(seg, RSTART, RLENGTH)
                    if (!(n in openln)) { openln[n] = FNR; ord[++nord] = n }
                }

                # ---- ACQUIRE(N): flock, in command position, on a bare
                # ---- tail-anchored operand N, outside any subshell open.
                # A continuation line is disqualified here (it is an ARGUMENT to
                # whatever command opened the logical line, not a statement in
                # its own right) but NOT under UNLOCK or DETACH below: a
                # continuation can neither clear a lock this shell holds nor be
                # the fork itself, so excluding it there would only lose
                # coverage.
                if (!is_cont && depth_start == 0 && cmdpos &&
                    !has_subshell_open(pre)) {
                    for (k = 1; k <= nord; k++) {
                        n = ord[k]
                        if ((n in acqln) || FNR <= openln[n]) continue
                        if (post ~ ("[[:space:]]" n ANCH)) acqln[n] = FNR
                    }
                }

                # ---- UNLOCK(N): the sanctioned explicit release ----
                # FILE-LEVEL and not order-sensitive: the remedy is a release
                # registered as an EXIT trap, so its textual position relative
                # to the fork carries no meaning.  Recorded independently of
                # ord[] because the release may precede the open.
                if (cmdpos && match(post, /-u[[:space:]]+[0-9]+/)) {
                    useg = substr(post, RSTART, RLENGTH)
                    urest = substr(post, RSTART + RLENGTH)
                    match(useg, /[0-9]+/)
                    un = substr(useg, RSTART, RLENGTH)
                    if (urest ~ ("^" ANCH)) unl[un] = FNR
                }

                # ---- DETACH: a fork downstream of the acquire ----
                if (depth_end == 0 && is_detach(t)) {
                    for (k = 1; k <= nord; k++) {
                        n = ord[k]
                        if (!(n in acqln) || (n in detln)) continue
                        if (FNR > acqln[n]) detln[n] = FNR
                    }
                }
            }
            END {
                for (k = 1; k <= nord; k++) {
                    n = ord[k]
                    if (!(n in detln) || (n in unl)) continue
                    printf "OFFENDER %s fd=%s open=%s acq=%s detach=%s\n", \
                        FNAME, n, openln[n], acqln[n], detln[n]
                }
            }
        ' "$_f")" || return 2
        if [ -n "$_out" ]; then
            printf '%s\n' "$_out"
            _rc=1
        fi
    done
    return "$_rc"
}

# ---------------------------------------------------------------------------
# Fixture tokens — SELF-MATCH SAFETY (see header).
#
# Every token that could make a line flaggable is assembled here from adjacent
# single-quoted fragments ('' splitting), so this source file contains no
# literal open / acquire / detach construct.  Fixture bodies built from these
# are written only into $TMPWORK by _write_fixture, never emitted as tracked
# source lines.
# ---------------------------------------------------------------------------
_FX_AMP='&'
_FX_OPEN='ex''ec 9>"$LOCK"'
_FX_CLOSE='ex''ec 9>&- || true'
_FX_CLOSE_BARE='ex''ec 9>&-'
_FX_OPEN_APPEND='ex''ec 9>>"$LOCK"'
_FX_ACQ_IF='if ! flo''ck -n 9; then'
_FX_ACQ_OR='flo''ck -xn 9 || exit 1'
_FX_ACQ_INHERIT='flo''ck -n 9 || true'
_FX_ACQ_SHARED='    flo''ck -s 9'
_FX_SQUAT_ONELINE='( flo''ck -x 9 && touch "$READY" && sleep 300 ) 9>"$LOCK" '"$_FX_AMP"
_FX_CONT_HEAD='assert "the test shell itself holds the lane lock on FD 9" \'
_FX_CONT_TAIL='    flo''ck -n 9'
_FX_DETACH_SUBSHELL='( sleep 300 ) '"$_FX_AMP"
_FX_DETACH='{ rm -rf "$TRASH"; } 9<&- '"$_FX_AMP"
_FX_FG_CHILD='"$@" 9<&-'
_FX_UNLOCK='    flo''ck -u 9 2>/dev/null || true'

# ---------------------------------------------------------------------------
# Harness — run the detector once, capture rc + stdout into globals, then
# assert over those globals (assert runs "$@" in THIS shell, so the globals
# survive).
# ---------------------------------------------------------------------------
OFFENDERS_RC=0
OFFENDERS_OUT=""

_run_offenders() {
    OFFENDERS_OUT=""
    OFFENDERS_RC=0
    OFFENDERS_OUT="$(_flock_fork_offenders "$@")" || OFFENDERS_RC=$?
}

_rc_is() { [ "$OFFENDERS_RC" = "$1" ]; }

_out_has() { case "$OFFENDERS_OUT" in *"$1"*) return 0 ;; *) return 1 ;; esac; }

_out_lacks() { case "$OFFENDERS_OUT" in *"$1"*) return 1 ;; *) return 0 ;; esac; }

_out_empty() { [ -z "$OFFENDERS_OUT" ]; }

_lt() { [ "$1" -lt "$2" ]; }

# Scan <file>... and succeed only if NOTHING was flagged.  Offender lines are
# echoed on failure so assert's captured-output dump names the file, the FD and
# the line numbers rather than just reporting a false boolean.
_scans_clean() {
    _run_offenders "$@"
    if [ -n "$OFFENDERS_OUT" ]; then
        printf '%s\n' "$OFFENDERS_OUT"
        return 1
    fi
    [ "$OFFENDERS_RC" -eq 0 ]
}

_out_line_count_is() {
    local _n=0
    if [ -n "$OFFENDERS_OUT" ]; then
        _n="$(printf '%s\n' "$OFFENDERS_OUT" | wc -l)"
    fi
    [ "$_n" -eq "$1" ]
}

# ---------------------------------------------------------------------------
# Cycle 1 — POSITIVE CONTROL.
#
# The real tree has zero offenders, so nothing in it keeps the matcher honest.
# This synthetic offender is the "a detector, not a constant failure" half:
# a locally-opened FD 9, a flock acquire on it, a detached fork downstream, and
# no unlock anywhere.  Its report must be ACTIONABLE (path + fd + line numbers),
# not a bare boolean.
# ---------------------------------------------------------------------------
echo "--- Cycle 1: positive control — a synthetic offender must be FLAGGED ---"

_write_fixture pos_offender.sh \
    '#!/usr/bin/env bash' \
    'set -euo pipefail' \
    'LOCK=/tmp/fixture.lock' \
    'TRASH=/tmp/fixture.trash' \
    "$_FX_OPEN" \
    "$_FX_ACQ_IF" \
    '    exit 75' \
    'fi' \
    'echo "holding the lane lock"' \
    "$_FX_DETACH" \
    'echo "done"'

_run_offenders "$TMPWORK/pos_offender.sh"

assert "positive control: synthetic offender is flagged (rc 1)" _rc_is 1
assert "positive control: report names the fixture path" \
    _out_has "$TMPWORK/pos_offender.sh"
assert "positive control: report names fd=9" _out_has "fd=9"
assert "positive control: report names the OPEN line (open=5)" _out_has "open=5"
assert "positive control: report names the ACQUIRE line (acq=6)" _out_has "acq=6"
assert "positive control: report names the DETACH line (detach=10)" \
    _out_has "detach=10"

# ---------------------------------------------------------------------------
# Cycle 2 — NEGATIVE CONTROL: criterion (c), the sanctioned RELEASED shape.
#
# Same offender, plus the remedy: a release function carrying `flock -u 9`,
# registered as an EXIT trap.  The unlock sits UPSTREAM of the open here on
# purpose — the rule is FILE-LEVEL, because a trap's textual position relative
# to the fork carries no meaning (in scripts/seed-warm-lane.sh the release is
# defined well before the forks it protects, yet runs after them, at exit).
# ---------------------------------------------------------------------------
echo "--- Cycle 2: negative control — the released shape must scan CLEAN ---"

_write_fixture neg_released.sh \
    '#!/usr/bin/env bash' \
    'set -euo pipefail' \
    'LOCK=/tmp/fixture.lock' \
    'TRASH=/tmp/fixture.trash' \
    '_release_lock() {' \
    "$_FX_UNLOCK" \
    "    $_FX_CLOSE" \
    '}' \
    "$_FX_OPEN" \
    "$_FX_ACQ_IF" \
    '    exit 75' \
    'fi' \
    'trap _release_lock EXIT' \
    'echo "holding the lane lock"' \
    "$_FX_DETACH" \
    'echo "done"'

_run_offenders "$TMPWORK/neg_released.sh"

assert "negative control: released shape is CLEAN (rc 0)" _rc_is 0
assert "negative control: released shape prints nothing" _out_empty

# A clean file must not mask a dirty one, and a dirty file must not smear onto
# a clean one — scan both together and demand exactly one named offender.
_run_offenders "$TMPWORK/pos_offender.sh" "$TMPWORK/neg_released.sh"

assert "mixed scan: still flags (rc 1)" _rc_is 1
assert "mixed scan: reports exactly one offender" _out_line_count_is 1
assert "mixed scan: names the offender" _out_has "$TMPWORK/pos_offender.sh"
assert "mixed scan: does NOT name the released file" \
    _out_lacks "$TMPWORK/neg_released.sh"

# ---------------------------------------------------------------------------
# Cycle 3 — REAL-SHAPE NON-VACUITY: mutation control against historical
# instance #2 (scripts/seed-warm-lane.sh, fixed by #5705).
#
# A synthetic fixture alone is weak: it is written to match whatever the
# matcher currently does, so matcher drift and fixture drift move together.
# Deleting the ONE release statement from the real ~1400-line script
# reproduces the pre-#5705 shape at full scale — complete with the prose, the
# refusal-branch closes, the `case` blocks and the continuation lines that
# actually break naive matchers.
#
# The pair is a DISCRIMINATOR, not a constant verdict: the mutant must FLAG and
# the shipped script must scan CLEAN.
# ---------------------------------------------------------------------------
echo "--- Cycle 3: mutation control on the REAL scripts/seed-warm-lane.sh ---"

_SEED_SRC="$REPO_ROOT/scripts/seed-warm-lane.sh"
_SEED_MUT="$TMPWORK/seed_mut.sh"

assert "mutation control: the real seed script exists" test -f "$_SEED_SRC"

# Delete the release STATEMENT (command position), not the prose that mentions
# it.  Deliberately narrow so a reword fails the line-count check below rather
# than silently degenerating into "scan the real file twice".
awk '{
        t = $0
        sub(/^[[:space:]]+/, "", t)
        if (t ~ /^flock[[:space:]]+-u[[:space:]]+9([[:space:]]|$)/) next
        print
     }' "$_SEED_SRC" >"$_SEED_MUT"

_SEED_LINES_ORIG="$(wc -l <"$_SEED_SRC")"
_SEED_LINES_MUT="$(wc -l <"$_SEED_MUT")"

assert "mutation control: the mutation actually removed a line" \
    _lt "$_SEED_LINES_MUT" "$_SEED_LINES_ORIG"

_run_offenders "$_SEED_MUT"

assert "mutation control: pre-#5705 shape of the REAL script is FLAGGED (rc 1)" \
    _rc_is 1
assert "mutation control: the flagged FD is 9" _out_has "fd=9"

_run_offenders "$_SEED_SRC"

assert "mutation control: the SHIPPED real script is CLEAN (rc 0)" _rc_is 0
assert "mutation control: the SHIPPED real script prints nothing" _out_empty

# ---------------------------------------------------------------------------
# Cycle 4 — FALSE-POSITIVE TRAP #1: FOREGROUND children are SAFE.
#
# A lock-holder that runs its child in the FOREGROUND is doing nothing wrong:
# it holds the lock for exactly the child's duration and reaps it before
# exiting.  Flagging that shape would be actively harmful — it would push
# scripts toward releasing a lock they are still legitimately using.
#
# The exemption is STRUCTURAL: only a line whose FINAL EFFECTIVE TOKEN is a
# bare `&` is a DETACH, so `"$@" 9<&-` can never enter the candidate set.
# ---------------------------------------------------------------------------
echo "--- Cycle 4: foreground children and ampersand-lookalikes are CLEAN ---"

# The scripts/lib_test_semaphore.sh shape: hold the slot for the child's whole
# run, with FD 9 closed in the child so no descendant daemon inherits it.
_write_fixture neg_foreground.sh \
    '#!/usr/bin/env bash' \
    'set -euo pipefail' \
    'LOCK=/tmp/fixture.lock' \
    "$_FX_OPEN_APPEND" \
    "$_FX_ACQ_OR" \
    "$_FX_FG_CHILD" \
    "$_FX_CLOSE_BARE"

assert "foreground child: the semaphore shape is CLEAN" \
    _scans_clean "$TMPWORK/neg_foreground.sh"

# Lines that merely CONTAIN an ampersand are not forks.
_write_fixture neg_nondetach_tokens.sh \
    '#!/usr/bin/env bash' \
    'set -euo pipefail' \
    'LOCK=/tmp/fixture.lock' \
    "$_FX_OPEN_APPEND" \
    "$_FX_ACQ_OR" \
    'echo hi 2>&1' \
    'true && echo ok' \
    'printf x >&2' \
    'cat /dev/null 9<&-' \
    '# a comment whose text ends with an ampersand: sleep 1 '"$_FX_AMP" \
    "$_FX_CLOSE_BARE"

assert "ampersand lookalikes: 2>&1 / && / >&2 / 9<&- / comment are CLEAN" \
    _scans_clean "$TMPWORK/neg_nondetach_tokens.sh"

# Real-tree controls — the files the task names as must-not-flag.
assert "real tree: scripts/lib_test_semaphore.sh is CLEAN" \
    _scans_clean "$REPO_ROOT/scripts/lib_test_semaphore.sh"
assert "real tree: scripts/cargo-test-occt-gated.sh is CLEAN" \
    _scans_clean "$REPO_ROOT/scripts/cargo-test-occt-gated.sh"
assert "real tree: scripts/lib_lane_x_flock.sh is CLEAN" \
    _scans_clean "$REPO_ROOT/scripts/lib_lane_x_flock.sh"

# ---------------------------------------------------------------------------
# Cycle 5 — FALSE-POSITIVE TRAP #2: INHERITED-FD paths are SAFE.
#
# This trap is the sharpest one, because the "fix" the guard would imply is
# WORSE than the bug it commemorates: an unguarded `flock -u 9` on a path where
# FD 9 was inherited from the CALLER releases the CALLER's lock, silently
# dropping the one-consumer exclusivity the caller is relying on.
#
# The exemption is therefore STRUCTURAL, not allow-listed: criterion (a)
# requires the file to OPEN the descriptor itself, so a file that merely
# inherits FD 9 never enters the candidate set at all and the guard is
# incapable of advising a release it has no right to advise.  (Whence
# scripts/seed-warm-lane.sh installs its release trap INSIDE the branch that
# opened the FD, for the same make-it-impossible reason; its contract block
# spells the hazard out.)
#
# A CLOSE is not an open, either: `exec 9>&-` merely drops this process's
# descriptor.  Treating it as an open would flag the inheritors — and would
# also have PASSED the real historical offender, whose refusal branches close
# FD 9 upstream of the fork.
# ---------------------------------------------------------------------------
echo "--- Cycle 5: inherited-FD paths must never be flagged ---"

# No local open anywhere: FD 9 arrives from the caller.
_write_fixture neg_inherited.sh \
    '#!/usr/bin/env bash' \
    'set -euo pipefail' \
    'TRASH=/tmp/fixture.trash' \
    '# FD 9 is inherited from the caller; this script never opens it.' \
    "$_FX_ACQ_INHERIT" \
    "$_FX_DETACH" \
    'echo "done"'

assert "inherited FD: a file that never opens FD 9 is CLEAN" \
    _scans_clean "$TMPWORK/neg_inherited.sh"

# The only `exec 9` line is a CLOSE — which is not an open.
_write_fixture neg_close_only.sh \
    '#!/usr/bin/env bash' \
    'set -euo pipefail' \
    'TRASH=/tmp/fixture.trash' \
    "$_FX_CLOSE_BARE" \
    "$_FX_ACQ_INHERIT" \
    "$_FX_DETACH" \
    'echo "done"'

assert "inherited FD: a lone CLOSE of FD 9 is not an open, so CLEAN" \
    _scans_clean "$TMPWORK/neg_close_only.sh"

# Real-tree controls — the inherited-FD consumers the task names, plus
# tests/infra/run_all.sh, whose only `exec 9` occurrence is a close inside a
# pool-worker subshell while it forks workers with `) &`.
assert "real tree: scripts/warm-lane-gc.sh is CLEAN" \
    _scans_clean "$REPO_ROOT/scripts/warm-lane-gc.sh"
assert "real tree: scripts/thin-warm-lane.sh is CLEAN" \
    _scans_clean "$REPO_ROOT/scripts/thin-warm-lane.sh"
assert "real tree: tests/infra/run_all.sh is CLEAN" \
    _scans_clean "$REPO_ROOT/tests/infra/run_all.sh"

# ---------------------------------------------------------------------------
# Cycle 6 — SUBSHELL-SCOPED opens: a lock-holder SQUATTER is not the holder.
#
# When the open AND the acquire both happen INSIDE the backgrounded compound,
# the forking shell never holds the lock: there is nothing for it to release
# and nothing for it to leak.  This is the standard test-fixture idiom for
# "occupy a lock so the code under test observes contention", used across the
# warm-lane suites — flagging it would put a permanent false positive on files
# that are doing nothing wrong.
#
# The guard therefore qualifies OPEN and ACQUIRE on subshell depth 0 at the
# line's START, so an open nested inside a subshell is not the holder opening
# it, while DETACH is qualified on depth 0 at the line's END so `) &` still
# counts as a fork performed BY the lock-holding shell.
# ---------------------------------------------------------------------------
echo "--- Cycle 6: subshell-scoped squatters are CLEAN ---"

_write_fixture neg_squatter_block.sh \
    '#!/usr/bin/env bash' \
    'set -euo pipefail' \
    'LOCK=/tmp/fixture.lock' \
    '(' \
    "    $_FX_OPEN" \
    "$_FX_ACQ_SHARED" \
    '    sleep 300' \
    ') '"$_FX_AMP" \
    'echo "squatter running"'

assert "squatter (block form): open+acquire inside the fork is CLEAN" \
    _scans_clean "$TMPWORK/neg_squatter_block.sh"

_write_fixture neg_squatter_oneline.sh \
    '#!/usr/bin/env bash' \
    'set -euo pipefail' \
    'LOCK=/tmp/fixture.lock' \
    'READY=/tmp/fixture.ready' \
    "$_FX_SQUAT_ONELINE" \
    'echo "squatter running"'

assert "squatter (one-line form): subshell opens and closes on one line, CLEAN" \
    _scans_clean "$TMPWORK/neg_squatter_oneline.sh"

# Real-tree controls — the live squatters in the warm-lane suites.
assert "real tree: tests/infra/test_warm_base_coherence.sh is CLEAN" \
    _scans_clean "$REPO_ROOT/tests/infra/test_warm_base_coherence.sh"
assert "real tree: tests/infra/test_warm_lane_gc.sh is CLEAN" \
    _scans_clean "$REPO_ROOT/tests/infra/test_warm_lane_gc.sh"
assert "real tree: tests/infra/test_warm_lane_audit.sh is CLEAN" \
    _scans_clean "$REPO_ROOT/tests/infra/test_warm_lane_audit.sh"

# ---------------------------------------------------------------------------
# Cycle 7 — BACKSLASH CONTINUATIONS are arguments, not statements.
#
# A physical line that continues the previous one is part of somebody else's
# command.  `assert "..." \` / `flock -n 9` never executes a flock the shell
# then carries across a fork — the FD-9 operand is an ARGUMENT to `assert`.
# Reading it as an acquire mistakes the subject of a claim for the claim.
# ---------------------------------------------------------------------------
echo "--- Cycle 7: backslash-continuation lines are not acquire sites ---"

_write_fixture neg_continuation.sh \
    '#!/usr/bin/env bash' \
    'set -euo pipefail' \
    'LOCK=/tmp/fixture.lock' \
    "$_FX_OPEN" \
    "$_FX_CONT_HEAD" \
    "$_FX_CONT_TAIL" \
    "$_FX_CLOSE_BARE" \
    "$_FX_DETACH_SUBSHELL" \
    'echo "done"'

assert "continuation: an FD-9 operand on a continued line is not an acquire" \
    _scans_clean "$TMPWORK/neg_continuation.sh"

# Real-tree control — and the sharpest one available, because this is the very
# suite that pins the #5705 fix.  Its H7b setup opens FD 9 at top level (:2870)
# and then asserts over it across a continuation (:2871-2872), has detached
# forks downstream at :2929, :2954, :3009, :3068 and :4046, and carries no
# code-level release.
assert "real tree: tests/infra/test_seed_warm_lane.sh is CLEAN" \
    _scans_clean "$REPO_ROOT/tests/infra/test_seed_warm_lane.sh"

test_summary
