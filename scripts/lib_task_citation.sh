#!/usr/bin/env bash
# scripts/lib_task_citation.sh — the task-citation grammar, SPOT.
#
# "Does this commit message cite task N?" is a NORMATIVE question across the
# reify/dark-factory seam: dark-factory's reconciliation decides whether a
# branch landed by asking it, and reify's classifiers must answer it the same
# way or the two repos disagree about which branches are phantom-done. The
# grammar below mirrors dark-factory orchestrator/git_ops.py's citation regex
# byte-for-byte. This file is the ONLY copy of it in reify.
#
# Consumers (both source this file; neither may re-inline the EREs):
#   scripts/warm-lane-degenerate-ref-check.sh    — landed-vs-degenerate ref
#                                                  classification
#   scripts/task-branch-contamination-sweep.sh   — peer-commit census
#
# DO NOT re-inline either ERE in a consumer. A second copy is the drift this
# file exists to prevent, and tests/infra/test_lib_task_citation.sh fails if
# one reappears.
#
# The grammar — a message cites <id> iff EITHER:
#   * its SUBJECT (first line) is `Merge <prefix><id> into …`, or
#   * it carries a `#<id>` reference,
# both with digit-boundary safety in both directions: task/1 must not match
# "Merge task/10 into main", and #45 must not match #4588.
#
# Purity contract — every function here takes all of its input as explicit
# parameters. No git invocation, no caller global (REPO_DIR, BRANCH_PREFIX_RE
# and friends are passed in, never read), no argv parsing, no `set -e` /
# `pipefail` side effect on a sourcing script, no stdout beyond the documented
# result. That is what makes this file safe to `source` from anywhere.
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
# two can never disagree about what "cites" means.
task_citation_message_cites() {
    local msg="$1" id="$2" prefix_re="$3"
    if printf '%s\n' "$msg" | grep -qE "^Merge ${prefix_re}${id} into "; then
        return 0
    fi
    if printf '%s\n' "$msg" | grep -qE "(^|[^0-9])#${id}([^0-9]|\$)"; then
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
# follows a '#' or the branch prefix (a superset of what the grammar accepts),
# then task_citation_message_cites adjudicates each candidate. The scan is not
# a second copy of the grammar — it decides nothing — which is why a candidate
# like the '5686' in "4#5686" is collected and then correctly rejected.
task_citation_peer_ids() {
    local msg="$1" prefix_re="$2" candidates id
    candidates="$(printf '%s\n' "$msg" \
        | grep -oE "(#|${prefix_re})[0-9]+" 2>/dev/null \
        | tr -cd '0-9\n' \
        | sort -u)" || candidates=""
    # Word-splitting is safe and intended here: every candidate is a digit run.
    for id in $candidates; do
        if task_citation_message_cites "$msg" "$id" "$prefix_re"; then
            printf '%s\n' "$id"
        fi
    done | sort -nu
}
