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
# B4/B4b pin what `^` actually anchors to, because it is the one place a
# future reader is likely to "fix" the grammar and thereby break parity.
# `^` is grep's per-LINE anchor, NOT a message-start anchor. Reify inherits
# that verbatim, and it is if anything STRICTER than dark-factory's own
# find_merge_marker, which matches _merge_subject() with `git log
# --grep --fixed-strings` — i.e. anywhere in the message, unanchored. So a
# merge subject on a later line DOES count; what does not count is a line
# where `Merge` is not the first byte.
assert "B4: '^' is a LINE anchor — a merge subject on a later line counts" \
    task_citation_message_cites "chore: cleanup
Merge task/5686 into main" 5686 "$PFX"
assert "B4b: a subject not at line start does NOT count" \
    not task_citation_message_cites "Re-Merge task/5686 into main
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

# ─────────────────────────────────────────────────────────────────────────────
# Block E (step-3) — SPOT-delegation guard
#
# The point of this library is that the grammar exists ONCE. That is a
# property of the tree, not of the library, so it has to be asserted against
# the consumers: each must SOURCE the lib, and none may carry its own copy of
# either ERE. Asserted in both directions — the consumers must not contain the
# fragments AND the lib must, so deleting the grammar outright cannot pass.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block E: SPOT-delegation guard ---"

# The two ERE fragments the lib owns, as grep -F needles. Deliberately spelled
# WITHOUT any variable name: an earlier draft used the lib's own `${prefix_re}`
# and passed vacuously against a consumer that spelled the same variable
# `${BRANCH_PREFIX_RE}`. These two are the invariant, name-independent core of
# each ERE.
E_MERGE_FRAGMENT='^Merge '
E_HASH_FRAGMENT='(^|[^0-9])#'

E_LIB_REL="scripts/lib_task_citation.sh"
# Every consumer named in the lib header. A new consumer belongs on this list.
E_CONSUMERS=(
    "scripts/warm-lane-degenerate-ref-check.sh"
    "scripts/task-branch-contamination-sweep.sh"
)

# _code_of <file> — <file> with whole-line comments removed.
#
# The needles above are matched against CODE only. Prose that describes the
# grammar is not a second copy of it — every consumer's header should be free
# to explain what "cites task N" means, and the lib header quotes both forms
# itself. What must not reappear is an executable copy.
_code_of() { grep -vE '^[[:space:]]*#' "$1"; }

# _carries <file> <needle> — true iff <needle> appears in <file>'s CODE.
_carries() { _code_of "$1" | grep -qF -- "$2"; }

# _lacks <file> <needle> — the inverse, as its own function so `assert` reports
# the intended direction rather than needing a `not` wrapper.
_lacks() { ! _carries "$1" "$2"; }

# _sources_lib <file> — true iff <file> has a `source`/`.` line naming the lib.
_sources_lib() {
    grep -qE '^[[:space:]]*(source|\.)[[:space:]]+.*lib_task_citation\.sh' "$1"
}

# (c) Non-vacuity FIRST: if the lib did not carry the grammar, every _lacks
# assertion below would pass trivially on an empty tree.
assert "E1: the lib's CODE carries the merge-subject ERE (guard is non-vacuous)" \
    _carries "$REPO_ROOT/$E_LIB_REL" "$E_MERGE_FRAGMENT"
assert "E2: the lib's CODE carries the '#<id>' boundary ERE (guard is non-vacuous)" \
    _carries "$REPO_ROOT/$E_LIB_REL" "$E_HASH_FRAGMENT"

for _c in "${E_CONSUMERS[@]}"; do
    _abs="$REPO_ROOT/$_c"
    # A consumer that does not exist yet (task 7244 builds the second one in a
    # later step) is not a violation — but it must not be silently skipped
    # either, or this guard would pass on a typo'd path. Assert existence for
    # the ones the lib header names, and let the SUT-creation step turn the
    # FAIL green.
    assert "E3[$_c]: consumer exists" test -f "$_abs"
    [ -f "$_abs" ] || continue
    assert "E4[$_c]: sources $E_LIB_REL" _sources_lib "$_abs"
    assert "E5[$_c]: carries NO second copy of the merge-subject ERE" \
        _lacks "$_abs" "$E_MERGE_FRAGMENT"
    assert "E6[$_c]: carries NO second copy of the '#<id>' boundary ERE" \
        _lacks "$_abs" "$E_HASH_FRAGMENT"
done

# Tree-wide backstop: no OTHER tracked script may grow a copy either. Scoped to
# scripts/ and hooks/ (where a consumer would plausibly live); the lib itself
# is the only permitted carrier.
_no_other_carriers() {
    local f hits=""
    while IFS= read -r f; do
        case "$f" in */"$E_LIB_REL"|*/lib_task_citation.sh) continue ;; esac
        if _carries "$f" "$E_MERGE_FRAGMENT" || _carries "$f" "$E_HASH_FRAGMENT"; then
            hits="$hits$f"$'\n'
        fi
    done < <(find "$REPO_ROOT/scripts" "$REPO_ROOT/hooks" -type f 2>/dev/null | sort)
    [ -z "$hits" ] || { printf 'unexpected carriers:\n%s' "$hits"; return 1; }
}
assert "E7: no other file under scripts/ or hooks/ carries the grammar in code" \
    _no_other_carriers


test_summary
