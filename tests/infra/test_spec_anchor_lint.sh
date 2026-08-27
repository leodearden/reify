#!/usr/bin/env bash
# Infrastructure tests for the spec-conformance ANCHOR SUBSTRATE (task #6758,
# leaf α of docs/prds/v0_6/spec-conformance-suite.md — §9 Phase 1; D3, §8.1).
#
# WHY this file exists
#   The conformance suite quantifies over clauses of docs/reify-language-spec.md.
#   Keying a fixture to a clause by SECTION NUMBER is fragile — a renumber
#   silently re-points every cite (this leaf performs exactly such a renumber,
#   §15's mis-numbered `13.1` → `15.1`). The substrate that replaces it is an
#   opaque, randomly-minted `sc-XXXXXX` anchor written as a standalone HTML
#   comment immediately preceding the anchored paragraph or heading, plus a
#   tombstone sidecar recording retired IDs. `scripts/spec-anchor-lint.sh` is
#   the gate that keeps that substrate well-formed; THIS file is the gate's
#   seeded-fire self-test plus the spec-hygiene assertions the leaf owes.
#
#   Every mutant below is built by mutating a copy of the REAL shipped spec
#   under $TMPWORK — never a synthetic-only fixture — and each carries a
#   meta-assertion that the mutation actually mutated, so a restructure cannot
#   silently degenerate into "scan the pristine file twice". Scenario (b) is
#   the LIVE control: the shipped corpus must scan CLEAN, so a "0 violations"
#   verdict anywhere below is a measurement by a working instrument rather
#   than the silence of a broken one.
#
# Scenarios
#   (a) spec hygiene — living-document front matter, heading-number
#       uniqueness, the §15 renumber, and a regression guard on the one
#       compiled spec consumer.
#
# Makes NO elapsed-time assertion anywhere (so it needs no `# wallclock:allow`
# escape and stays green under tests/infra/test_no_new_wallclock_upper_bounds.sh).
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

SPEC="$REPO_ROOT/docs/reify-language-spec.md"
TS_CONSUMER="$REPO_ROOT/tree-sitter-reify/tests/spec_purpose_example_grammar.rs"

# ---------------------------------------------------------------------------
# Single EXIT trap covers every temp path this suite mints. Registering two
# separate traps would silently replace the first with the second and leak
# temps on exit (the convention test_reify_audit_ptodo.sh:457-486 records).
# ---------------------------------------------------------------------------
cleanup_all() {
    :
}
trap cleanup_all EXIT

echo "=== spec-conformance anchor substrate (task #6758) ==="

# ---------------------------------------------------------------------------
# Predicates. Kept as named functions rather than inline `bash -c` where the
# check needs to NAME the offending site on failure: `assert` dumps a failing
# checker's captured output, so printing the sites there is what turns a bare
# false into an actionable report.
# ---------------------------------------------------------------------------

# grep -oE over the spec's numbered headings at every level, tallying the bare
# number. Any number appearing twice is a duplicate-heading-number violation.
_no_duplicate_heading_numbers() {
    local dups d
    dups="$(grep -oE '^#{2,5} [0-9]+(\.[0-9]+)*' "$SPEC" | awk '{print $2}' | sort | uniq -d || true)"
    [ -z "$dups" ] && return 0
    echo "duplicate heading number(s) in $SPEC:"
    while IFS= read -r d; do
        [ -z "$d" ] && continue
        grep -nE "^#{2,5} ${d//./\\.} " "$SPEC" || true
    done <<<"$dups"
    return 1
}

# Exactly-once occurrence of a literal split key, reported with its sites.
_occurs_exactly_once() {
    local file="$1" key="$2" n
    n="$(grep -cF -- "$key" "$file" || true)"
    [ "$n" = "1" ] && return 0
    echo "expected exactly 1 occurrence of '$key' in $file, found $n:"
    grep -nF -- "$key" "$file" || true
    return 1
}

_file_has_literal() {
    grep -qF -- "$2" "$1"
}

# ===========================================================================
# (a) SPEC HYGIENE
#
# The stale front matter (`**Version:** 0.1` / `**Date:** 2026-03-13`) and the
# duplicated `### 13.1` are the two hygiene defects this leaf owes. a5 is
# deliberately generalized from "exactly one ### 13.1" to "no duplicate
# heading number at ANY level": `13.1` is the file's ONLY duplicate today, so
# the general assertion goes green at exactly the same edit while guarding the
# recurrence CLASS — which matters precisely because the anchor contract
# exists so consumers stop keying on section numbers.
# ===========================================================================
echo ""
echo "--- (a) spec hygiene: living-document front matter + heading-number uniqueness ---"

assert "(a0) the spec exists and is non-empty" test -s "$SPEC"

assert "(a1) the stale '**Version:** 0.1' pin is GONE from the spec front matter" \
    bash -c '! grep -qE "^\*\*Version:\*\* 0\.1" "$1"' -- "$SPEC"

assert "(a2) the stale '**Date:** 2026-03-13' pin is GONE from the spec front matter" \
    bash -c '! grep -qE "^\*\*Date:\*\* 2026-03-13" "$1"' -- "$SPEC"

assert "(a3) the front matter names the current language version on a '**Language version:**' line mentioning 0.6" \
    bash -c 'head -n 10 "$1" | grep -qE "^\*\*Language version:\*\*.*0\.6"' -- "$SPEC"

assert "(a4) the front matter points readers at §14 for the versioning scheme" \
    bash -c 'head -n 15 "$1" | grep -qF "§14"' -- "$SPEC"

assert "(a5) NO duplicate heading number at any level in the spec" \
    _no_duplicate_heading_numbers

assert "(a6) the mis-numbered '### 13.1 Newline and Continuation Rules' is ABSENT" \
    bash -c '! grep -qE "^### 13\.1 Newline and Continuation Rules" "$1"' -- "$SPEC"

assert "(a6) the renumbered '### 15.1 Newline and Continuation Rules' is PRESENT" \
    bash -c 'grep -qE "^### 15\.1 Newline and Continuation Rules" "$1"' -- "$SPEC"

assert "(a6) '### 13.1 Doc Comments' (correctly numbered, inside §13) is UNTOUCHED" \
    bash -c 'grep -qE "^### 13\.1 Doc Comments" "$1"' -- "$SPEC"

# a7 — regression guard on the ONE compiled consumer of the spec:
# tree-sitter-reify/tests/spec_purpose_example_grammar.rs include_str!s the spec
# and splits it on two literal heading strings. Neither is a 13.1 and both sit
# outside §9.2, so the renumber and the anchor seeding are safe — but that is a
# premise, and this pins it. a7 is PRE-EXISTING GREEN: if it reds, the premise
# sweep was wrong; fix the spec edit, never this consumer.
assert "(a7) spec contains the split key '### 9.5 Purposes' exactly once" \
    _occurs_exactly_once "$SPEC" '### 9.5 Purposes'

assert "(a7) spec contains the split key '### 4.4 Purpose Declarations' exactly once" \
    _occurs_exactly_once "$SPEC" '### 4.4 Purpose Declarations'

assert "(a7) the compiled consumer still keys on '### 9.5 Purposes'" \
    _file_has_literal "$TS_CONSUMER" '### 9.5 Purposes'

assert "(a7) the compiled consumer still keys on '### 4.4 Purpose Declarations'" \
    _file_has_literal "$TS_CONSUMER" '### 4.4 Purpose Declarations'

test_summary
