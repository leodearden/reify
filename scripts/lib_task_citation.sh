#!/usr/bin/env bash
# scripts/lib_task_citation.sh — the task-citation grammar, SPOT.
#
# "Does this commit message cite task N?" is a NORMATIVE question across the
# reify/dark-factory seam: dark-factory's reconciliation decides whether a
# branch landed by asking it, and reify's classifiers must answer it the same
# way or the two repos disagree about which branches are phantom-done. This
# file is the ONLY copy of reify's answer.
#
# Consumers (both source this file; neither may re-inline the EREs):
#   scripts/warm-lane-degenerate-ref-check.sh    — landed-vs-degenerate ref
#                                                  classification
#   scripts/task-branch-contamination-sweep.sh   — peer-commit census
#
# DO NOT re-inline any ERE in a consumer. A second copy is the drift this file
# exists to prevent, and tests/infra/test_lib_task_citation.sh fails if one
# reappears.
#
# The grammar — a message cites <id> iff ANY of:
#   * a line of it is `Merge <prefix><id> into …` (grep's `^` is a LINE anchor;
#     see Block B of the test suite for why that is kept);
#   * its SUBJECT (first line only) is a conventional-commit head citing <id>:
#     `<kind>(<id>)` / `<kind>(<id>:`, or `<kind>` followed later on the line
#     by `<prefix><id>`, where <kind> is dark-factory's closed list;
#   * it carries a `#<id>` reference anywhere,
# each with boundary safety in both directions: task/1 must not match
# "Merge task/10 into main", impl(5) must not match impl(50), and #45 must not
# match #4588.
#
# Relationship to dark-factory — measured, not assumed. dark-factory's
# normative pattern is orchestrator/git_ops.py DEFAULT_COMMIT_CITATION_PATTERN,
# applied to the SUBJECT only (its task 2675). Arm by arm:
#   * merge-subject arm — agrees with DF's `^Merge task/{tid} into `
#     alternative, and with find_merge_marker's unanchored search.
#   * conventional-commit arm — mirrors DF's first alternative, kind list and
#     `[):]` terminator included, and is subject-only for the same reason DF's
#     is: a squash or merge body that LISTS `impl(<id>): …` lines is not a
#     citation. This is the form reify's own commits actually use, so without
#     it the grammar missed ~98% of task commits.
#   * `#<id>` arm — reify-only; DF has no counterpart. Kept because
#     warm-lane-degenerate-ref-check.sh's `landed` verdict rests on it for tips
#     like "feat(selectors): … (#4857, Option B)".
#   * DF's two UNANCHORED paren alternatives, `(#?<id>)` and `(task <id>)`, are
#     deliberately NOT mirrored. DF backs them with an effect-present check at
#     every call site; neither consumer here has one, and on reify's history
#     they attribute orchestrator subjects such as "chore: save WIP before
#     warm-lane reclaim (task 1933)" to 1933, and enumerations like "(2)" to
#     task 2.
#
# Purity contract — every function here takes all of its input as explicit
# parameters. No git invocation, no caller global (REPO_DIR, BRANCH_PREFIX_RE
# and friends are passed in, never read), no argv parsing, no `set -e` /
# `pipefail` side effect on a sourcing script, no stdout beyond the documented
# result. That is what makes this file safe to `source` from anywhere.
#
# The converse of that contract is the reason for the feeding idiom below: this
# file does not set `pipefail`, but every consumer does, so a pipeline's status
# here is the caller's shell option applied to OUR process list. NEVER make a
# `grep -q` the READER of a pipe. `grep -q` exits at its first match, and a
# writer still writing when it goes away dies of SIGPIPE — which `pipefail`
# then promotes into the pipeline's status, turning a MATCH into a non-match.
# The arbiter therefore feeds grep by REDIRECTION (`<<<`), so grep's own
# match/no-match is the verdict and no second process can overrule it. The
# harvest's pipeline is exempt for a checkable reason, not a size guess: every
# stage there (`grep -oE`, `sed`, `sort`) consumes to EOF, so no reader ever
# departs early and there is no SIGPIPE window to lose a citation through.
# Block G of tests/infra/test_lib_task_citation.sh pins this.
#
# Exports exactly three functions:
#   task_citation_regex_escape  <string>                       -> escaped (stdout)
#   task_citation_message_cites <message> <id> <escaped_prefix> -> exit 0/1
#   task_citation_peer_ids      <message> <escaped_prefix>      -> ids (stdout)

# Source guard — prevent double-sourcing when two consumers meet in one shell.
if [ "${_REIFY_LIB_TASK_CITATION_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_LIB_TASK_CITATION_SH_SOURCED=1

# dark-factory DEFAULT_COMMIT_CITATION_PATTERN's conventional-commit kinds,
# verbatim. A closed list, like DF's: an open `[a-z]+` would also accept kinds
# DF does not (verify, review, step-N — ~2% of reify's id-headed subjects),
# and the two repos would then disagree on exactly those commits.
_TASK_CITATION_KINDS='(merge|impl|amend|fix|test|feat|chore|docs|refactor|style|build)'

# task_citation_regex_escape <string>
# Escapes ERE metacharacters in <string> (anything outside [a-zA-Z0-9_]) so it
# can be safely interpolated into a `grep -E` pattern as a literal. Guards the
# merge-subject check below against a caller-supplied branch prefix containing
# regex metacharacters (e.g. "." or "+"), which would otherwise be interpreted
# as regex syntax instead of matched literally. Prints the escaped string with
# no trailing newline.
task_citation_regex_escape() {
    printf '%s' "$1" | sed -e 's/[^a-zA-Z0-9_]/\\&/g'
}

# task_citation_message_cites <message> <id> <escaped_prefix>
# True iff <message> cites task <id> under the grammar documented above.
# <escaped_prefix> must already have been through task_citation_regex_escape.
#
# This function is the single ARBITER of the grammar: task_citation_peer_ids
# below harvests candidates permissively and defers every verdict here, so the
# two can never disagree about what "cites" means. That claim holds only
# because of the normalisation invariant documented on the harvest below;
# Block F of tests/infra/test_lib_task_citation.sh enforces it as set
# equality, in both directions, over a corpus of branch prefixes.
task_citation_message_cites() {
    local msg="$1" id="$2" prefix_re="$3"
    if grep -qE "^Merge ${prefix_re}${id} into " <<<"$msg"; then
        return 0
    fi
    if grep -qE "^${_TASK_CITATION_KINDS}(\(${id}[):]|.*[^A-Za-z0-9_]${prefix_re}${id}([^A-Za-z0-9_]|\$))" \
            <<<"${msg%%$'\n'*}"; then
        return 0
    fi
    if grep -qE "(^|[^0-9])#${id}([^0-9]|\$)" <<<"$msg"; then
        return 0
    fi
    return 1
}

# task_citation_peer_ids <message> <escaped_prefix>
# Prints every task id <message> cites, one per line, numerically sorted and
# de-duplicated; prints nothing when it cites none. No id-width restriction —
# the grammar is digit-boundary-delimited, not width-delimited.
#
# Two stages, deliberately: a PERMISSIVE scan collects every digit run that
# follows a '#', a '(' or the branch prefix (a superset of what the grammar
# accepts — the scan is not subject-scoped, the arbiter is),
# then task_citation_message_cites adjudicates each candidate. The scan is not
# a second copy of the grammar — it decides nothing — which is why a candidate
# like the '5686' in "4#5686" is collected and then correctly rejected.
#
# NORMALISATION INVARIANT — a candidate id is what REMAINS once the matched
# sigil is stripped, never what a character class re-derives from the whole
# match. The branch prefix is CALLER-SUPPLIED and may itself contain digits,
# and neither obvious alternative survives that:
#   * deleting every non-digit from the match fuses the prefix's digits onto
#     the id — prefix `t2/` turns "Merge t2/200 into main" into 2200, which
#     the arbiter then correctly rejects, so the real id 200 is never emitted
#     and the caller sees a SILENT FALSE NEGATIVE on the very merge-subject
#     form this grammar exists to recognise;
#   * taking the trailing digit run fails the same way whenever the prefix's
#     digit is trailing with no separator — prefix `t2` over
#     "Merge t2200 into main" yields 2200 again.
# Stripping the matched sigil yields 200 in both. The strip reuses the
# escaping contract already in force: task_citation_regex_escape backslashes
# every non-alphanumeric byte, so an escaped prefix interpolates into a
# /-delimited `sed -E` as safely as into the `grep -E` above (a '/' arrives as
# '\/', never as a bare delimiter). An empty prefix is unaffected — the
# alternation matches empty and strips nothing.
task_citation_peer_ids() {
    local msg="$1" prefix_re="$2" candidates id
    candidates="$(printf '%s\n' "$msg" \
        | grep -oE "(#|${prefix_re}|\()[0-9]+" 2>/dev/null \
        | sed -E "s/^(#|${prefix_re}|\()//" \
        | sort -u)" || candidates=""
    # Word-splitting is safe and intended here: every candidate is a digit run.
    for id in $candidates; do
        if task_citation_message_cites "$msg" "$id" "$prefix_re"; then
            printf '%s\n' "$id"
        fi
    done | sort -nu
}
