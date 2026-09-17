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
#   step-20 — digit-bearing branch prefixes (D9-D12), and Block F: the
#            harvest/arbiter AGREEMENT invariant, asserted as set EQUALITY
#   esc-7244-16 — Block H: the conventional-commit subject arm, the form
#            reify's own task commits use and the one the grammar lacked
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
assert "D8b: a conventional-commit subject id is harvested" \
    stdout_is '5686' task_citation_peer_ids 'impl(5686): GREEN — tier 4' "$PFX"
assert "D8c: a '(<id>' in a BODY line is collected but rejected (subject-only arm)" \
    stdout_is '5686' task_citation_peer_ids "impl(5686): subject

test(4880): a body line listing another commit" "$PFX"

# D9-D12 (step-20) — the branch prefix is CALLER-SUPPLIED and may itself carry
# digits, so the id can never be re-derived from the matched text by a
# character class: it is only ever what REMAINS once the matched sigil is
# stripped. D11 is the discriminating case — a prefix whose digit is trailing
# with no separator defeats a trailing-digit-run normalisation too, so this
# assertion is what forbids that repair as well as the character-class one.
D_T2SLASH_RE="$(task_citation_regex_escape 't2/')"
assert "D9: a digit INSIDE the prefix does not fuse onto the id (t2/ + t2/200)" \
    stdout_is '200' task_citation_peer_ids 'Merge t2/200 into main' "$D_T2SLASH_RE"

D_ONESLASH_RE="$(task_citation_regex_escape '1/')"
assert "D10: a digit LEADING the prefix does not fuse onto the id (1/ + 1/200)" \
    stdout_is '200' task_citation_peer_ids 'Merge 1/200 into main' "$D_ONESLASH_RE"

D_T2BARE_RE="$(task_citation_regex_escape 't2')"
assert "D11: a digit TRAILING a separatorless prefix does not fuse (t2 + t2200)" \
    stdout_is '200' task_citation_peer_ids 'Merge t2200 into main' "$D_T2BARE_RE"

assert "D12: the '#' form is unaffected by a digit-bearing prefix" \
    stdout_is '200' task_citation_peer_ids 'closes #200' "$D_T2SLASH_RE"

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
E_KINDS_FRAGMENT='merge|impl|amend|fix|test|feat|chore|docs|refactor|style|build'

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
assert "E2b: the lib's CODE carries the conventional-commit kind list (guard is non-vacuous)" \
    _carries "$REPO_ROOT/$E_LIB_REL" "$E_KINDS_FRAGMENT"

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
    assert "E6b[$_c]: carries NO second copy of the conventional-commit kind list" \
        _lacks "$_abs" "$E_KINDS_FRAGMENT"
done

# Tree-wide backstop: no OTHER tracked script may grow a copy either. Scoped to
# scripts/ and hooks/ (where a consumer would plausibly live); the lib itself
# is the only permitted carrier.
_no_other_carriers() {
    local f hits=""
    while IFS= read -r f; do
        case "$f" in */"$E_LIB_REL"|*/lib_task_citation.sh) continue ;; esac
        if _carries "$f" "$E_MERGE_FRAGMENT" || _carries "$f" "$E_HASH_FRAGMENT" \
                || _carries "$f" "$E_KINDS_FRAGMENT"; then
            hits="$hits$f"$'\n'
        fi
    done < <(find "$REPO_ROOT/scripts" "$REPO_ROOT/hooks" -type f 2>/dev/null | sort)
    [ -z "$hits" ] || { printf 'unexpected carriers:\n%s' "$hits"; return 1; }
}
assert "E7: no other file under scripts/ or hooks/ carries the grammar in code" \
    _no_other_carriers


# ─────────────────────────────────────────────────────────────────────────────
# Block F (step-20) — harvest/arbiter AGREEMENT, as set EQUALITY
#
# Block D's header claims this invariant in prose ("an id the predicate rejects
# must not appear, and every id it accepts must") while asserting only its
# first half, which is how a silent FALSE NEGATIVE — a cited id the harvest
# drops — reached review through a fully green suite. This block makes the
# claim executable.
#
# EQUALITY, not containment, is the assertion: containment one way permits the
# false negative, the other way permits a false positive, and only the
# conjunction pins the arbiter as the sole authority on what "cites" means.
#
# The oracle is derived INDEPENDENTLY of the harvest: enumerate every id the
# arbiter COULD accept and adjudicate each one through
# task_citation_message_cites. That candidate set is every SUFFIX of every
# maximal digit run in the message, which is provably complete: every citation
# form requires a non-digit (' ' for the merge form, `[):]` or a non-word byte
# for the conventional-commit form, `[^0-9]|$` for the '#' form) immediately
# after the id, so an accepted id always ends at a run boundary. Nothing
# acceptable can escape it.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block F: harvest/arbiter agreement ---"

# _candidate_ids <message> — every SUFFIX of every maximal digit run,
# sorted-unique. Suffixes rather than whole runs, deliberately: with a
# digit-bearing prefix the accepted id is a PROPER suffix of a longer run
# (prefix `t2` over "Merge t2200 into main" accepts 200 out of the run 2200),
# so a whole-run oracle would be blind to exactly the shape under test.
_candidate_ids() {
    local msg="$1" run i n
    while IFS= read -r run; do
        n=${#run}
        for ((i = 0; i < n; i++)); do
            printf '%s\n' "${run:i}"
        done
    done < <(printf '%s\n' "$msg" | grep -oE '[0-9]+' || true) | sort -u
}

# _arbiter_accepts <message> <escaped_prefix> — the ids the ARBITER accepts,
# in the harvest's own ordering contract so the two are directly comparable.
_arbiter_accepts() {
    local msg="$1" prefix_re="$2" cand
    while IFS= read -r cand; do
        [ -n "$cand" ] || continue
        if task_citation_message_cites "$msg" "$cand" "$prefix_re"; then
            printf '%s\n' "$cand"
        fi
    done < <(_candidate_ids "$msg") | sort -nu
}

# _harvest_agrees <message> <escaped_prefix> — the invariant. On failure it
# prints both sets, which `assert` dumps, so a mismatch localises itself.
_harvest_agrees() {
    local msg="$1" prefix_re="$2" harvested accepted
    harvested="$(task_citation_peer_ids "$msg" "$prefix_re")"
    accepted="$(_arbiter_accepts "$msg" "$prefix_re")"
    [ "$harvested" = "$accepted" ] && return 0
    printf 'harvest=[%s] arbiter=[%s]\n' \
        "${harvested//$'\n'/,}" "${accepted//$'\n'/,}"
    return 1
}

# The prefix corpus spans every way a prefix can interact with the grammar:
# the default, a digit INSIDE / LEADING / TRAILING the prefix, ERE
# metacharacters, and the empty prefix (which makes the alternation match
# everywhere and so strips nothing).
F_PREFIXES=('task/' 't2/' '1/' 't2' 't.sk/' 'a+b/' '')

# @PFX@ is substituted per prefix, so each row is the SAME message shape under
# every prefix — the differential that makes a prefix-dependent verdict visible.
F_TEMPLATES=(
    'Merge @PFX@200 into main'
    'Merge @PFX@200 into main

Carries #4880 and #99, and #4880 again.'
    'chore: tidy up the readme'
    'follows up on #56861'
    'rebased onto @PFX@5686 yesterday'
    'fix(x): 4#5686 and see esc#5686'
    'Merge @PFX@100 into main

Re-lands #100.'
    'Merge task/5686 into main'
    'Merge @PFX@ into main'
    'impl(200): GREEN — the conventional-commit arm'
    'fix(x): rebase onto @PFX@200 tip, see #99'
    'chore: save WIP before warm-lane reclaim (task 1933)'
    'docs(200): subject

test(4880): a body line citing nothing'
    'test(4414/step-5): RED — id followed by a slash'
)

for _pfx in "${F_PREFIXES[@]}"; do
    _pfx_re="$(task_citation_regex_escape "$_pfx")"
    _row=0
    for _tpl in "${F_TEMPLATES[@]}"; do
        _row=$((_row + 1))
        _msg="${_tpl//@PFX@/$_pfx}"
        assert "F[prefix='$_pfx' msg$_row]: harvest == arbiter accept set" \
            _harvest_agrees "$_msg" "$_pfx_re"
    done
done


# ─────────────────────────────────────────────────────────────────────────────
# Block G — the verdict is grep's, never the PIPELINE's
#
# task_citation_message_cites used to ask `printf '%s\n' "$msg" | grep -qE …`.
# `grep -q` exits at its FIRST match, so on a message longer than the pipe
# buffer the writer is still writing when the reader goes away: printf takes
# SIGPIPE (141), and `set -o pipefail` — which EVERY consumer sets, this suite
# and warm-lane-degenerate-ref-check.sh and task-branch-contamination-sweep.sh
# alike — promotes the writer's death into the pipeline's status. The `if` then
# reads a MATCH as a non-match and the id reports as NOT cited: a fail-open on
# exactly the citation the sweep exists to count.
#
# Measured on bash 5.2.21 before the fix: deterministic above ~64KB (200/200
# false negatives) and PROBABILISTIC in the 32-64KB band (1/30 at 32KB, 4/30 at
# 56KB, 17/30 at 60KB) — the same input answered both ways from run to run,
# which is what makes this a flake source rather than merely a size limit. The
# body below is sized well past the buffer so these assertions are
# deterministic: a test for a race must not itself be one.
#
# WHY NOT ASSERT BLOCK F's AGREEMENT INVARIANT HERE — it cannot see this. The
# harvest adjudicates every candidate through the SAME arbiter, so when the
# arbiter goes blind both sides go blind together and the sets stay equal while
# both are empty. Agreement is a relative property and this is an absolute
# failure; only a concrete expected set catches it. That is the same vacuity
# trap P1-P3 and step-22's baseline-first ordering exist to defeat, met here in
# a new place, which is why G1-G3 assert absolute results and G4/G5 are the
# controls proving a merely always-true predicate would not satisfy them.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block G: an oversized message still reports its citations ---"

# ~256KB of digit-free, '#'-free filler, doubled in-shell so the corpus costs no
# forks and no temp files. Digit-free matters: a stray digit run would enter the
# harvest's candidate set and make the expected sets below a moving target.
G_FILLER='filler filler filler filler filler filler filler filler filler'
for _ in $(seq 1 12); do G_FILLER="$G_FILLER"$'\n'"$G_FILLER"; done

G_MERGE="Merge task/200 into main

$G_FILLER"
G_HASH="chore: unrelated subject

Follows up on #4880.
$G_FILLER"
# Both forms plus a long tail: the exact shape whose verdict went both ways.
G_BOTH="Merge task/200 into main

Carries #4880 and #99.
$G_FILLER"

assert "G1: the merge-subject form survives a body past the pipe buffer" \
    task_citation_message_cites "$G_MERGE" 200 "$PFX"
assert "G2: the '#' form survives a body past the pipe buffer" \
    task_citation_message_cites "$G_HASH" 4880 "$PFX"
assert "G3: the harvest returns every id of an oversized message" \
    stdout_is '99
200
4880' task_citation_peer_ids "$G_BOTH" "$PFX"

# G4/G5 — the controls. G1-G3 must not be satisfiable by a predicate that has
# merely become always-true, so the SAME oversized messages must still reject
# an absent id and still honour the digit boundary at size.
assert "G4: an oversized message does NOT cite an id it never names" \
    not task_citation_message_cites "$G_MERGE" 4242 "$PFX"
assert "G5: the digit boundary still holds at size ('task/200' is not 'task/20')" \
    not task_citation_message_cites "$G_MERGE" 20 "$PFX"


# ─────────────────────────────────────────────────────────────────────────────
# Block H (esc-7244-16) — the conventional-commit subject arm
#
# reify's task commits are `impl(5686): …`, not `#5686`: over the last 6000
# non-merge commits on main, 4671 carry an id-headed subject and 4569 of those
# use one of dark-factory's kinds. A grammar without this arm missed the
# esc-6205-4 contamination outright (14 `kind(5686)` commits, 0 citations
# recognised), and warm-lane-degenerate-ref-check.sh classified 85 genuinely
# landed refs as degenerate because their tips read `kind(<own id>): …`.
#
# The arm mirrors DF's DEFAULT_COMMIT_CITATION_PATTERN first alternative and is
# SUBJECT-ONLY, as DF applies it. H6-H9 are the controls that pin what it must
# NOT accept, so the positive cases cannot be satisfied by an arm that has
# merely become permissive.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block H: conventional-commit subject arm ---"

assert "H1: 'impl(5686): …' cites 5686" \
    task_citation_message_cites 'impl(5686): GREEN — arg-side D4 tier 4' 5686 "$PFX"
assert "H2: DF's '(<id>:' terminator is accepted too" \
    task_citation_message_cites 'test(5686: RED — unclosed paren' 5686 "$PFX"
assert "H3: '<kind>' then '<prefix><id>' later on the subject cites <id>" \
    task_citation_message_cites 'fix(x): rebase onto task/5686 tip' 5686 "$PFX"
assert "H4: every DF kind is recognised (merge … build)" \
    bash -c 'source "$1"; for k in merge impl amend fix test feat chore docs refactor style build; do
        task_citation_message_cites "$k(77): s" 77 "$2" || { echo "kind $k rejected"; exit 1; }; done' _ "$LIB" "$PFX"
assert "H5: the arm honours an escaped metacharacter prefix" \
    task_citation_message_cites 'fix: land t.sk/7 now' 7 "$A_DOT_RE"

assert "H6: boundary — 'impl(56860)' does NOT cite 5686, 'impl(5686)' does NOT cite 568" \
    bash -c 'source "$1"; ! task_citation_message_cites "impl(56860): s" 5686 "$2" \
        && ! task_citation_message_cites "impl(5686): s" 568 "$2"' _ "$LIB" "$PFX"
assert "H6b: boundary — 'task/56860' after a kind does NOT cite 5686" \
    not task_citation_message_cites 'fix: land task/56860' 5686 "$PFX"
assert "H7: SUBJECT-ONLY — a body line 'impl(5686): …' does NOT cite 5686" \
    not task_citation_message_cites "docs: squash summary

impl(5686): a listed commit, not a citation" 5686 "$PFX"
assert "H8: a kind outside DF's closed list does NOT cite ('verify(5686)')" \
    not task_citation_message_cites 'verify(5686): cross-crate sweep' 5686 "$PFX"
assert "H8b: kinds are case-sensitive, as DF's are ('Impl(5686)')" \
    not task_citation_message_cites 'Impl(5686): s' 5686 "$PFX"
assert "H9: DF's unanchored paren arms are NOT mirrored ('(task 1933)', '(2)')" \
    bash -c 'source "$1"; ! task_citation_message_cites "chore: save WIP before warm-lane reclaim (task 1933)" 1933 "$2" \
        && ! task_citation_message_cites "feat: gate entry point (2)" 2 "$2"' _ "$LIB" "$PFX"
assert "H9b: '<prefix><id>' without a kind head does NOT cite (D6's shape)" \
    not task_citation_message_cites 'rebased onto task/5686 yesterday' 5686 "$PFX"


test_summary
