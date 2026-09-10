#!/usr/bin/env bash
# tests/infra/test_lib_task_citation.sh
# Hermetic tests for scripts/lib_task_citation.sh (task #7244).
#
# The library under test is PURE — every input arrives as an explicit
# parameter, it invokes no git, reads no caller global, and parses no argv —
# so this suite sources it directly and calls the three functions. There is no
# repo fixture and no temp state to clean up.
#
# Blocks (added incrementally across task #7244's TDD steps):
#   step-1 — the three functions' behaviour (escape, predicate, id harvest)
#   step-3 — SPOT-delegation guard: the grammar lives in the lib and nowhere
#            else, and its consumers source it rather than re-inlining it
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
LIB="$REPO_ROOT/scripts/lib_task_citation.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/lib_task_citation.sh hermetic tests (task 7244) ==="

# The suite CANNOT degrade past a missing library: every assertion below calls
# into it. Report the absence as a normal FAIL line (rather than dying inside
# `source` under `set -e`, which produces no PASS/FAIL output at all and no
# Results line for run_all.sh to parse) and stop.
if [ ! -f "$LIB" ]; then
    assert "scripts/lib_task_citation.sh exists" test -f "$LIB"
    test_summary
    exit 1
fi
# shellcheck source=scripts/lib_task_citation.sh
source "$LIB"

# ── negation wrapper ──────────────────────────────────────────────────────────
# `assert` reports PASS on exit 0, so a "must NOT cite" case needs its verdict
# inverted. `! "$@"` is exempt from `set -e` inside the function body.
not() { ! "$@"; }

# stdout_is <expected> <cmd...> — the function under test writes its result to
# stdout, so the assertion has to compare captured output, not an exit code.
stdout_is() {
    local expected="$1"; shift
    [ "$("$@")" = "$expected" ]
}

# ─────────────────────────────────────────────────────────────────────────────
# Block A (step-1a) — task_citation_regex_escape
#
# Asserted BOTH literally (the escape's own output) and behaviourally (the
# escaped result interpolated into the predicate's ERE). The literal check
# alone would not prove the escape is fit for its only purpose; the
# behavioural check alone would not localise a failure to the escape.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: task_citation_regex_escape ---"

assert "A1: '.' and '/' are escaped (t.sk/ -> t\\.sk\\/)" \
    stdout_is 't\.sk\/' task_citation_regex_escape 't.sk/'
assert "A2: '+' is escaped (a+b/ -> a\\+b\\/)" \
    stdout_is 'a\+b\/' task_citation_regex_escape 'a+b/'
assert "A3: [a-zA-Z0-9_] bytes pass through untouched" \
    stdout_is 'task_9Z' task_citation_regex_escape 'task_9Z'
assert "A4: every other byte class is escaped (\$^*?[](){}|\\\\ etc.)" \
    stdout_is '\$\^\*\?\[\]\(\)\{\}\|\\\.\+\-\ ' \
    task_citation_regex_escape '$^*?[](){}|\.+- '
assert "A5: the empty string escapes to the empty string" \
    stdout_is '' task_citation_regex_escape ''

# Behavioural: with the prefix escaped, ERE metacharacters match LITERALLY.
A_DOT_RE="$(task_citation_regex_escape 't.sk/')"
assert "A6: escaped 't.sk/' matches the literal prefix 't.sk/'" \
    task_citation_message_cites 'Merge t.sk/7 into main' 7 "$A_DOT_RE"
assert "A7: escaped 't.sk/' does NOT match 'task/' ('.' is not a wildcard)" \
    not task_citation_message_cites 'Merge task/7 into main' 7 "$A_DOT_RE"

A_PLUS_RE="$(task_citation_regex_escape 'a+b/')"
assert "A8: escaped 'a+b/' matches the literal prefix 'a+b/'" \
    task_citation_message_cites 'Merge a+b/7 into main' 7 "$A_PLUS_RE"
assert "A9: escaped 'a+b/' does NOT match 'ab/' ('+' is not a repeat)" \
    not task_citation_message_cites 'Merge ab/7 into main' 7 "$A_PLUS_RE"

# ─────────────────────────────────────────────────────────────────────────────
# Block B (step-1b) — merge-subject citation form, with digit boundaries
#
# The boundary must hold in BOTH directions: a shorter id must not match a
# longer one in the message (568 vs 5686) and a longer id must not match a
# shorter one (56860 vs 5686).
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: task_citation_message_cites — merge-subject form ---"

PFX="$(task_citation_regex_escape 'task/')"
B_SUBJ='Merge task/5686 into main'

assert "B1: 'Merge task/5686 into main' cites 5686" \
    task_citation_message_cites "$B_SUBJ" 5686 "$PFX"
assert "B2: it does NOT cite 568 (message id is longer)" \
    not task_citation_message_cites "$B_SUBJ" 568 "$PFX"
assert "B3: it does NOT cite 56860 (queried id is longer)" \
    not task_citation_message_cites "$B_SUBJ" 56860 "$PFX"
assert "B4: the subject form is anchored — a body line does not count" \
    not task_citation_message_cites "chore: cleanup
Merge task/5686 into main" 5686 "$PFX"
assert "B5: the trailing ' into ' is required" \
    not task_citation_message_cites 'Merge task/5686 into' 5686 "$PFX"
assert "B6: a multi-line message still matches on its subject line" \
    task_citation_message_cites "$B_SUBJ

Some body text." 5686 "$PFX"

# ─────────────────────────────────────────────────────────────────────────────
# Block C (step-1c) — '#<id>' citation form, with digit boundaries
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: task_citation_message_cites — '#<id>' form ---"

assert "C1: a body line containing '#5686' cites 5686" \
    task_citation_message_cites "fix(x): something

Follows up on #5686 as agreed." 5686 "$PFX"
assert "C2: '#56861' does NOT cite 5686 (trailing digit)" \
    not task_citation_message_cites 'fix(x): follows up on #56861' 5686 "$PFX"
assert "C3: '#568' does NOT cite 5686" \
    not task_citation_message_cites 'fix(x): follows up on #568' 5686 "$PFX"
assert "C4: 'x5686' does NOT cite 5686 (no '#')" \
    not task_citation_message_cites 'fix(x): follows up on x5686' 5686 "$PFX"
assert "C5: '4#5686' does NOT cite 5686 (leading digit before '#')" \
    not task_citation_message_cites 'fix(x): follows up on 4#5686' 5686 "$PFX"
assert "C6: '#5686' at end-of-line cites 5686" \
    task_citation_message_cites 'fix(x): follows up on #5686' 5686 "$PFX"
assert "C7: '#5686' at start-of-line cites 5686" \
    task_citation_message_cites '#5686 is the follow-up' 5686 "$PFX"
assert "C8: 'x#5686' cites 5686 (only a preceding DIGIT breaks the boundary)" \
    task_citation_message_cites 'see esc#5686 for detail' 5686 "$PFX"
assert "C9: a message citing nothing cites no id" \
    not task_citation_message_cites 'chore: tidy up the readme' 5686 "$PFX"

# ─────────────────────────────────────────────────────────────────────────────
# Block D (step-1d) — task_citation_peer_ids
#
# Prints every id the message cites, one per line, sorted and de-duplicated.
# The harvest must agree with the predicate exactly: an id the predicate
# rejects must not appear, and every id it accepts must.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block D: task_citation_peer_ids ---"

D_MSG="Merge task/5686 into main

Carries #4880 and #5685, and #4880 again."

assert "D1: both citation forms are harvested, sorted and de-duplicated" \
    stdout_is '4880
5685
5686' task_citation_peer_ids "$D_MSG" "$PFX"
assert "D2: a message citing no task prints nothing" \
    stdout_is '' task_citation_peer_ids 'chore: tidy up the readme' "$PFX"
assert "D3: output is numerically sorted, not lexicographically" \
    stdout_is '99
100' task_citation_peer_ids 'closes #100 and #99' "$PFX"
assert "D4: a boundary-violating '#' reference is not harvested" \
    stdout_is '56861' task_citation_peer_ids 'follows up on #56861' "$PFX"
assert "D5: a digit-prefixed '#' reference is not harvested" \
    stdout_is '' task_citation_peer_ids 'follows up on 4#5686' "$PFX"
assert "D6: a bare '<prefix><id>' outside a merge subject is not harvested" \
    stdout_is '' task_citation_peer_ids 'rebased onto task/5686 yesterday' "$PFX"
assert "D7: the harvest honours an escaped metacharacter prefix" \
    stdout_is '7' task_citation_peer_ids 'Merge t.sk/7 into main' "$A_DOT_RE"
assert "D8: an id repeated in both forms appears exactly once" \
    stdout_is '5686' task_citation_peer_ids "Merge task/5686 into main

Re-lands #5686." "$PFX"

test_summary
