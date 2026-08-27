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
#   (b) LIVE control — the SHIPPED spec + tombstone sidecar scan CLEAN.
#   (c) duplicate anchor ID — flagged, BOTH sites named.
#   (d) malformed anchor ID (non-hex / too long / missing space) — flagged.
#   (e) an ID that is simultaneously live and tombstoned — flagged.
#   (f) tombstone-file row grammar + LC_ALL=C sortedness (f1-f3 fire, f4 is
#       the well-formed control proving comments/blanks are not sort keys).
#   (g) rule 6, dangling placement — an anchor followed by a blank line, and
#       an anchor as the file's last line.
#   (h) anti-vacuity / hard-fail — missing or empty --spec, missing
#       --tombstones are exit 2, never a graceful skip.
#   (i) unknown flag — exit 2.
#   (j) rule 5 — an anchored paragraph DELETED without a same-diff tombstone.
#   (k) the converse control — the same deletion WITH a tombstone row is
#       clean, so (j)/(k) are a discriminator rather than "reds on any
#       deletion", which would make the tombstone mechanism unusable.
#   (l) a no-op base is clean — rule 5's clean verdict is earned, not
#       hardcoded.
#   (m) base resolution is never a SKIP — an unreadable --base-spec, an
#       unresolvable --base rev, and both flags together are all exit 2.
#   (n) the DEFAULT base is live — a flagless run really consults git.
#   (o) §9.2 is ACTUALLY anchored in the shipped spec — structurally
#       (every heading present is anchored) rather than by a pinned count,
#       so leaf η's later graduation edits to §9.2 cannot red it.
#   (p) the NORMATIVE authoring note exists, is linked from the spec front
#       matter, states every clause of the contract, and contains no
#       valid-format anchor ID (a copy-paste collision hazard).
#
# NO-SILENT-GREEN FLOOR: a $RAN counter is incremented by every scenario and
# checked after test_summary. A future guard condition that skipped every
# scenario would otherwise let this HARD GATE report green having asserted
# nothing.
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

SPEC_REL="docs/reify-language-spec.md"
TOMB_REL="docs/reify-language-spec.tombstones"
SPEC="$REPO_ROOT/$SPEC_REL"
TOMBSTONES="$REPO_ROOT/$TOMB_REL"
LINT="$REPO_ROOT/scripts/spec-anchor-lint.sh"
NOTE="$REPO_ROOT/docs/notes/spec-anchor-contract.md"
TS_CONSUMER="$REPO_ROOT/tree-sitter-reify/tests/spec_purpose_example_grammar.rs"

# Did ANY scenario actually execute?  Consulted after test_summary; a run that
# asserted nothing must not exit 0.  See NO-SILENT-GREEN FLOOR in the header.
RAN=0

# ---------------------------------------------------------------------------
# Single EXIT trap covers every temp path this suite mints. Registering two
# separate traps would silently replace the first with the second and leak
# temps on exit (the convention test_reify_audit_ptodo.sh:457-486 records).
# ---------------------------------------------------------------------------
TMPWORK=""
cleanup_all() {
    # "|| true" per line: [ -n "" ] short-circuits to rc 1, which would
    # otherwise become the trap's exit code and override the suite's status.
    [ -n "$TMPWORK" ] && rm -rf "$TMPWORK" || true
}
trap cleanup_all EXIT

TMPWORK="$(mktemp -d)"

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

# ---------------------------------------------------------------------------
# Lint harness — run scripts/spec-anchor-lint.sh once, capture rc + combined
# output into GLOBALS, then assert over those globals. `assert` runs "$@" in
# THIS shell (redirect only, no command-substitution subshell), so the global
# mutations survive; that is exactly what this idiom needs.
# ---------------------------------------------------------------------------
LINT_RC=0
LINT_OUT=""

_run_lint() {
    LINT_OUT=""
    LINT_RC=0
    LINT_OUT="$(bash "$LINT" "$@" 2>&1)" || LINT_RC=$?
}

_rc_is() {
    [ "$LINT_RC" = "$1" ] && return 0
    echo "expected rc $1, got $LINT_RC; output was:"
    printf '%s\n' "$LINT_OUT"
    return 1
}

_out_has() {
    case "$LINT_OUT" in
        *"$1"*) return 0 ;;
    esac
    echo "expected output to contain '$1'; output was:"
    printf '%s\n' "$LINT_OUT"
    return 1
}

_out_lacks() {
    case "$LINT_OUT" in
        *"$1"*)
            echo "expected output NOT to contain '$1'; output was:"
            printf '%s\n' "$LINT_OUT"
            return 1 ;;
    esac
    return 0
}

_out_empty() {
    [ -z "$LINT_OUT" ] && return 0
    echo "expected EMPTY output; output was:"
    printf '%s\n' "$LINT_OUT"
    return 1
}

_out_line_count_is() {
    local _n=0
    if [ -n "$LINT_OUT" ]; then
        _n="$(printf '%s\n' "$LINT_OUT" | wc -l)"
    fi
    [ "$_n" -eq "$1" ] && return 0
    echo "expected $1 output line(s), got $_n; output was:"
    printf '%s\n' "$LINT_OUT"
    return 1
}

_lt() { [ "$1" -lt "$2" ]; }

_eq() { [ "$1" = "$2" ]; }

# ---------------------------------------------------------------------------
# Fixture builders. Every mutant is a copy of the REAL shipped spec, mutated
# under $TMPWORK — a synthetic-only fixture is written to match whatever the
# checker currently does, so fixture drift and checker drift move together.
# ---------------------------------------------------------------------------

# _fresh_ids <file> <n> — emit <n> well-formed, distinct sc-XXXXXX IDs (one
# per line, ascending in LC_ALL=C order) that do NOT already occur in <file>.
# Deterministic rather than random on purpose: a fixture ID must be
# reproducible, and the absence check is what keeps it collision-free against
# whatever the live spec carries.
_fresh_ids() {
    local file="$1" n="$2" i=0 c=0 id
    while [ "$c" -lt "$n" ] && [ "$i" -lt 4096 ]; do
        id="$(printf 'sc-f0%04x' "$i")"
        i=$((i + 1))
        if grep -qF -- "$id" "$file"; then continue; fi
        printf '%s\n' "$id"
        c=$((c + 1))
    done
    [ "$c" -eq "$n" ]
}

# _first_anchor_id <file> — the ID of the first well-formed anchor line.
_first_anchor_id() {
    awk 'match($0, /^<!-- sc-anchor: sc-[0-9a-f]{6} -->$/) {
             id = $0
             sub(/^<!-- sc-anchor: /, "", id)
             sub(/ -->$/, "", id)
             print id
             exit
         }' "$1"
}

# _spec_copy_with_anchors <dest> <n> — copy the SHIPPED spec to <dest>,
# guaranteeing at least <n> well-formed anchors in it.
#
# ORDER-INDEPENDENCE: once the §9.2 seeding lands, the shipped spec already
# carries more than <n> anchors and the copy is byte-identical — every mutant
# below is then built from the real substrate at full scale. Before it lands,
# the helper seeds well-formed anchors itself, so these scenarios neither
# depend on nor anticipate the seeding step.
_spec_copy_with_anchors() {
    local dest="$1" n="$2" have ids
    have="$(grep -cE '^<!-- sc-anchor: sc-[0-9a-f]{6} -->$' "$SPEC" || true)"
    if [ "${have:-0}" -ge "$n" ]; then
        cp "$SPEC" "$dest"
        return 0
    fi
    ids="$(_fresh_ids "$SPEC" "$n" | tr '\n' ' ')"
    awk -v ids="$ids" -v n="$n" '
        BEGIN { split(ids, ID, " "); k = 0; fence = 0; prev_blank = 1 }
        {
            if ($0 ~ /^[[:space:]]*```/) { fence = !fence; print; prev_blank = 0; next }
            if (!fence && k < n && prev_blank && $0 != "" && $0 !~ /^<!--/ && NR > 5) {
                k++
                printf "<!-- sc-anchor: %s -->\n", ID[k]
            }
            print
            prev_blank = ($0 == "")
        }
    ' "$SPEC" >"$dest"
}

# _insert_before_paragraph <file> <ordinal> <text> — insert <text> as its own
# line immediately before the <ordinal>-th paragraph start outside any fenced
# code block, in place. awk exits 3 (and `set -e` aborts loudly) if the file
# has fewer eligible paragraphs, so an out-of-range ordinal can never silently
# degenerate into "copy the file unchanged".
_insert_before_paragraph() {
    local file="$1" ord="$2" text="$3"
    awk -v ord="$ord" -v text="$text" '
        BEGIN { k = 0; fence = 0; prev_blank = 1; done = 0 }
        {
            if ($0 ~ /^[[:space:]]*```/) { fence = !fence; print; prev_blank = 0; next }
            if (!done && !fence && prev_blank && $0 != "" && $0 !~ /^<!--/ && NR > 5) {
                k++
                if (k == ord) { print text; done = 1 }
            }
            print
            prev_blank = ($0 == "")
        }
        END { if (!done) exit 3 }
    ' "$file" >"$file.ins"
    mv "$file.ins" "$file"
}

# _delete_anchored_paragraph <file> <id> — remove the anchor line carrying
# <id> AND the paragraph it anchors (the contiguous non-blank run that follows
# it), in place. This is the shape rule 5 exists to catch: the ID vanishes
# from the spec entirely, so a consumer's cite silently resolves to nothing.
_delete_anchored_paragraph() {
    local file="$1" id="$2"
    awk -v id="$id" '
        BEGIN { del = 0; hit = 0 }
        {
            if ($0 == "<!-- sc-anchor: " id " -->") { del = 1; hit = 1; next }
            if (del) {
                if ($0 ~ /^[[:space:]]*$/) { del = 0 }
                next
            }
            print
        }
        END { if (!hit) exit 3 }
    ' "$file" >"$file.del"
    mv "$file.del" "$file"
}

# _line_of <file> <literal> — 1-based line number of the FIRST occurrence.
_line_of() { grep -nF -- "$2" "$1" | head -n 1 | cut -d: -f1; }
# _last_line_of <file> <literal> — 1-based line number of the LAST occurrence.
_last_line_of() { grep -nF -- "$2" "$1" | tail -n 1 | cut -d: -f1; }

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
RAN=$((RAN + 1))

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

# ===========================================================================
# (b) LIVE CONTROL — the SHIPPED corpus scans CLEAN.
#
# The anti-vacuity partner to every mutant below: without it, a permanently
# broken lint (wrong regex, unreadable input silently swallowed, an early
# `exit 0`) reads exactly like "no violations found". Deliberately invoked
# with REPO-RELATIVE paths, which also pins that the lint resolves them
# against its repo root rather than the caller's CWD.
# ===========================================================================
echo ""
echo "--- (b) LIVE control: the SHIPPED spec + tombstone sidecar scan CLEAN ---"
RAN=$((RAN + 1))

assert "(b) the lint script exists" test -f "$LINT"
assert "(b) the tombstone sidecar exists (an absent sidecar is exit 2, never an empty pass)" \
    test -f "$TOMBSTONES"

_run_lint --spec "$SPEC_REL" --tombstones "$TOMB_REL"
assert "(b) the SHIPPED corpus is CLEAN (rc 0)" _rc_is 0
assert "(b) the SHIPPED corpus produces NO output" _out_empty

# ===========================================================================
# (c) DUPLICATE ID — flagged, and BOTH sites named.
# ===========================================================================
echo ""
echo "--- (c) duplicate anchor ID: flagged, both sites named ---"
RAN=$((RAN + 1))

DUP="$TMPWORK/dup.md"
_spec_copy_with_anchors "$DUP" 4
DUP_ID="$(_first_anchor_id "$DUP")"
DUP_LINE_TXT="<!-- sc-anchor: $DUP_ID -->"
_DUP_BEFORE="$(wc -l <"$DUP")"
_insert_before_paragraph "$DUP" 60 "$DUP_LINE_TXT"
_DUP_AFTER="$(wc -l <"$DUP")"
_DUP_OCC="$(grep -cF -- "$DUP_LINE_TXT" "$DUP" || true)"

# Meta-assertions: without these, a restructure that stopped mutating would
# silently degenerate into "scan the pristine file twice" and still look green.
assert "(c) meta: the fixture carries a well-formed anchor to duplicate" \
    bash -c '[ -n "$1" ]' -- "$DUP_ID"
assert "(c) meta: the mutation actually added a line" _lt "$_DUP_BEFORE" "$_DUP_AFTER"
assert "(c) meta: the duplicated ID now occurs exactly twice" _eq "$_DUP_OCC" 2

_DUP_L1="$(_line_of "$DUP" "$DUP_LINE_TXT")"
_DUP_L2="$(_last_line_of "$DUP" "$DUP_LINE_TXT")"

_run_lint --spec "$DUP" --tombstones "$TOMBSTONES"
assert "(c) a duplicated anchor ID is FLAGGED (rc 1)" _rc_is 1
assert "(c) output names the offending ID $DUP_ID" _out_has "$DUP_ID"
assert "(c) output names the FIRST site ($DUP:$_DUP_L1)" _out_has "$DUP:$_DUP_L1"
assert "(c) output names the SECOND site ($DUP:$_DUP_L2)" _out_has "$DUP:$_DUP_L2"

# ===========================================================================
# (d) MALFORMED ID — three near-miss mutants.
#
# Catching the NEAR MISS (rather than only scanning for well-formed lines) is
# the point: a typo'd anchor that the lint silently ignores is strictly worse
# than no anchor at all, because a consumer greps for it and finds nothing.
# ===========================================================================
echo ""
echo "--- (d) malformed anchor IDs: non-hex / too long / missing space ---"
RAN=$((RAN + 1))

_D_IDX=0
for BAD in '<!-- sc-anchor: sc-XYZ123 -->' '<!-- sc-anchor: sc-1234567 -->' '<!-- sc-anchor:sc-abc123 -->'; do
    _D_IDX=$((_D_IDX + 1))
    MUT="$TMPWORK/malformed_$_D_IDX.md"
    _spec_copy_with_anchors "$MUT" 4
    _M_BEFORE="$(wc -l <"$MUT")"
    _insert_before_paragraph "$MUT" 40 "$BAD"
    _M_AFTER="$(wc -l <"$MUT")"
    assert "(d$_D_IDX) meta: the mutation actually inserted '$BAD'" \
        _lt "$_M_BEFORE" "$_M_AFTER"
    _M_LINE="$(_line_of "$MUT" "$BAD")"
    _run_lint --spec "$MUT" --tombstones "$TOMBSTONES"
    assert "(d$_D_IDX) malformed anchor '$BAD' is FLAGGED (rc 1)" _rc_is 1
    assert "(d$_D_IDX) output names the site ($MUT:$_M_LINE)" _out_has "$MUT:$_M_LINE"
done

# ===========================================================================
# (e) LIVE/TOMBSTONE DISJOINTNESS — an ID cannot be both live and retired.
# ===========================================================================
echo ""
echo "--- (e) an ID that is simultaneously live and tombstoned ---"
RAN=$((RAN + 1))

E_SPEC="$TMPWORK/disjoint.md"
_spec_copy_with_anchors "$E_SPEC" 4
E_ID="$(_first_anchor_id "$E_SPEC")"
E_TOMB="$TMPWORK/disjoint.tombstones"
{
    printf '# tombstone fixture\n'
    printf '%s 2026-08-27 retired while still live -- the violation under test\n' "$E_ID"
} >"$E_TOMB"

assert "(e) meta: the tombstoned ID is genuinely still live in the spec copy" \
    bash -c 'grep -qF -- "<!-- sc-anchor: $1 -->" "$2"' -- "$E_ID" "$E_SPEC"

_run_lint --spec "$E_SPEC" --tombstones "$E_TOMB"
assert "(e) a live-AND-tombstoned ID is FLAGGED (rc 1)" _rc_is 1
assert "(e) output names the offending ID $E_ID" _out_has "$E_ID"
assert "(e) output names the spec path" _out_has "$E_SPEC"
assert "(e) output names the tombstone path" _out_has "$E_TOMB"

# ===========================================================================
# (f) TOMBSTONE FILE — row grammar and LC_ALL=C sortedness.
#
# f4 is the discriminator's clean half: without it, f1-f3 would be satisfied
# by a lint that reds on every tombstone file, which would make the whole
# mechanism unusable.
# ===========================================================================
echo ""
echo "--- (f) tombstone row grammar + sortedness ---"
RAN=$((RAN + 1))

F_SPEC="$TMPWORK/tomb_spec.md"
_spec_copy_with_anchors "$F_SPEC" 4
# Fresh IDs checked absent from THIS spec copy, so (f) exercises the tombstone
# rules alone and can never accidentally trip the (e) disjointness rule.
F_IDS="$(_fresh_ids "$F_SPEC" 2)"
F_A="$(printf '%s\n' "$F_IDS" | sed -n 1p)"
F_B="$(printf '%s\n' "$F_IDS" | sed -n 2p)"

assert "(f) meta: two distinct fixture IDs were minted in ascending order" \
    bash -c '[ -n "$1" ] && [ -n "$2" ] && [ "$1" \< "$2" ]' -- "$F_A" "$F_B"
assert "(f) meta: neither fixture ID collides with a live ID in the spec copy" \
    bash -c '! grep -qF -- "$1" "$3" && ! grep -qF -- "$2" "$3"' -- "$F_A" "$F_B" "$F_SPEC"

# (f1) descending data rows
F1="$TMPWORK/f1.tombstones"
printf '# fixture header\n%s 2026-08-27 retired\n%s 2026-08-27 retired\n' "$F_B" "$F_A" >"$F1"
_run_lint --spec "$F_SPEC" --tombstones "$F1"
assert "(f1) descending tombstone rows are FLAGGED (rc 1)" _rc_is 1
assert "(f1) output names the out-of-order row ($F1:3)" _out_has "$F1:3"

# (f2) malformed date
F2="$TMPWORK/f2.tombstones"
printf '# fixture header\n%s 2026-13-45 retired\n' "$F_A" >"$F2"
_run_lint --spec "$F_SPEC" --tombstones "$F2"
assert "(f2) a calendar-impossible date is FLAGGED (rc 1)" _rc_is 1
assert "(f2) output names the offending row ($F2:2)" _out_has "$F2:2"

# (f3) no reason field
F3="$TMPWORK/f3.tombstones"
printf '# fixture header\n%s 2026-08-27\n' "$F_A" >"$F3"
_run_lint --spec "$F_SPEC" --tombstones "$F3"
assert "(f3) a row with no reason field is FLAGGED (rc 1)" _rc_is 1
assert "(f3) output names the offending row ($F3:2)" _out_has "$F3:2"

# (f4) well-formed control: comments and blanks interleaved, rows ascending
F4="$TMPWORK/f4.tombstones"
{
    printf '# fixture header\n'
    printf '\n'
    printf '%s 2026-01-02 superseded; forwarding anchor in the same section\n' "$F_A"
    printf '# a comment BETWEEN two correctly ordered data rows\n'
    printf '\n'
    printf '%s 2026-03-04 paragraph deleted in a rewrite\n' "$F_B"
    printf '\n'
    printf '# trailing comment\n'
} >"$F4"
_run_lint --spec "$F_SPEC" --tombstones "$F4"
assert "(f4) a well-formed tombstone file with interleaved comments/blanks is CLEAN (rc 0)" _rc_is 0
assert "(f4) the well-formed control produces NO output" _out_empty

# ===========================================================================
# (g) RULE 6 — PLACEMENT. §8.1 says an anchor sits "immediately preceding the
# anchored paragraph (or heading)". Without this rule that clause is
# decorative: an anchor stranded before a blank line or at EOF anchors
# nothing, and the deletion rule then has no well-defined referent.
# ===========================================================================
echo ""
echo "--- (g) rule 6: an anchor must immediately precede what it anchors ---"
RAN=$((RAN + 1))

# (g1) anchor followed by a blank line
G1="$TMPWORK/dangling_blank.md"
_spec_copy_with_anchors "$G1" 4
G1_ID="$(_first_anchor_id "$G1")"
G1_LINE="$(_line_of "$G1" "<!-- sc-anchor: $G1_ID -->")"
_G1_BEFORE="$(wc -l <"$G1")"
awk -v n="$G1_LINE" '{ print; if (NR == n) print "" }' "$G1" >"$G1.ins"
mv "$G1.ins" "$G1"
_G1_AFTER="$(wc -l <"$G1")"
assert "(g1) meta: the mutation actually inserted the blank line" _lt "$_G1_BEFORE" "$_G1_AFTER"
assert "(g1) meta: the line after the anchor is now blank" \
    bash -c '[ -z "$(sed -n "$(($1 + 1))p" "$2")" ]' -- "$G1_LINE" "$G1"
_run_lint --spec "$G1" --tombstones "$TOMBSTONES"
assert "(g1) an anchor followed by a blank line is FLAGGED (rc 1)" _rc_is 1
assert "(g1) output names the dangling anchor's line ($G1:$G1_LINE)" _out_has "$G1:$G1_LINE"

# (g2) anchor as the file's last line
G2="$TMPWORK/dangling_eof.md"
_spec_copy_with_anchors "$G2" 4
G2_ID="$(_fresh_ids "$G2" 1)"
_G2_BEFORE="$(wc -l <"$G2")"
printf '<!-- sc-anchor: %s -->\n' "$G2_ID" >>"$G2"
_G2_AFTER="$(wc -l <"$G2")"
assert "(g2) meta: the mutation actually appended the trailing anchor" \
    _lt "$_G2_BEFORE" "$_G2_AFTER"
_run_lint --spec "$G2" --tombstones "$TOMBSTONES"
assert "(g2) an anchor as the file's LAST line is FLAGGED (rc 1)" _rc_is 1
assert "(g2) output names the trailing anchor's line ($G2:$_G2_AFTER)" _out_has "$G2:$_G2_AFTER"

# ===========================================================================
# (h) ANTI-VACUITY / HARD-FAIL — the lint never gracefully skips.
#
# Exit 2 ("could not scan") is kept strictly distinct from exit 1 ("scanned,
# found violations") precisely so these three configurations cannot be
# mistaken for a clean corpus.
# ===========================================================================
echo ""
echo "--- (h) anti-vacuity: missing/empty inputs are exit 2, never a pass ---"
RAN=$((RAN + 1))

_run_lint --spec "$TMPWORK/does-not-exist.md" --tombstones "$TOMBSTONES"
assert "(h1) a nonexistent --spec is exit 2" _rc_is 2
assert "(h1) a nonexistent --spec is NEVER rc 0" bash -c '[ "$1" != 0 ]' -- "$LINT_RC"

H_EMPTY="$TMPWORK/empty.md"
: >"$H_EMPTY"
assert "(h2) meta: the empty-spec fixture really is zero bytes" \
    bash -c '[ ! -s "$1" ] && [ -f "$1" ]' -- "$H_EMPTY"
_run_lint --spec "$H_EMPTY" --tombstones "$TOMBSTONES"
assert "(h2) an EMPTY --spec is exit 2 (a zero-byte corpus is not a clean corpus)" _rc_is 2
assert "(h2) an EMPTY --spec is NEVER rc 0" bash -c '[ "$1" != 0 ]' -- "$LINT_RC"

_run_lint --spec "$SPEC" --tombstones "$TMPWORK/no-such.tombstones"
assert "(h3) a nonexistent --tombstones is exit 2" _rc_is 2
assert "(h3) a nonexistent --tombstones is NEVER rc 0" bash -c '[ "$1" != 0 ]' -- "$LINT_RC"

# ===========================================================================
# (i) UNKNOWN FLAG — a usage error, not a silently ignored argument.
# ===========================================================================
echo ""
echo "--- (i) unknown flag ---"
RAN=$((RAN + 1))

_run_lint --nope
assert "(i) an unknown flag is a usage error (rc 2)" _rc_is 2
assert "(i) the usage error names the offending flag" _out_has "--nope"

# ===========================================================================
# (j) RULE 5 — an anchored paragraph DELETED without a same-diff tombstone.
#
# This is the mechanism §8.1 hangs the whole anchor contract on: without it,
# "cite by opaque ID" degrades to "cite by an ID that may or may not still
# exist", which is strictly worse than citing a section number (at least a
# stale section number is visibly stale).
#
# Built hermetically from the REAL spec so it exercises full scale, not a toy.
# ===========================================================================
echo ""
echo "--- (j) rule 5: a deletion with NO tombstone row ---"
RAN=$((RAN + 1))

J_BASE="$TMPWORK/j_base.md"
_spec_copy_with_anchors "$J_BASE" 4
J_GONE="$(_first_anchor_id "$J_BASE")"
J_KEPT="$(awk 'match($0, /^<!-- sc-anchor: sc-[0-9a-f]{6} -->$/) {
                   id = $0; sub(/^<!-- sc-anchor: /, "", id); sub(/ -->$/, "", id)
                   n++
                   if (n == 2) { print id; exit }
               }' "$J_BASE")"
J_CUR="$TMPWORK/j_cur.md"
cp "$J_BASE" "$J_CUR"
_delete_anchored_paragraph "$J_CUR" "$J_GONE"

# An unrelated-but-well-formed tombstone file: (j) must fire because THIS id
# is missing from it, not because the file is empty or malformed.
J_OTHER="$(_fresh_ids "$J_BASE" 1)"
J_TOMB="$TMPWORK/j.tombstones"
{
    printf '# tombstone fixture\n'
    printf '%s 2026-05-06 an unrelated retirement\n' "$J_OTHER"
} >"$J_TOMB"

assert "(j) meta: two distinct anchor IDs were available in the base" \
    bash -c '[ -n "$1" ] && [ -n "$2" ] && [ "$1" != "$2" ]' -- "$J_GONE" "$J_KEPT"
assert "(j) meta: the vanished ID IS present in the base copy" \
    bash -c 'grep -qF -- "$1" "$2"' -- "$J_GONE" "$J_BASE"
assert "(j) meta: the vanished ID is NOT present in the current copy" \
    bash -c '! grep -qF -- "$1" "$2"' -- "$J_GONE" "$J_CUR"
assert "(j) meta: the SURVIVING ID is still present in the current copy" \
    bash -c 'grep -qF -- "$1" "$2"' -- "$J_KEPT" "$J_CUR"
assert "(j) meta: the tombstone fixture does NOT mention the vanished ID" \
    bash -c '! grep -qF -- "$1" "$2"' -- "$J_GONE" "$J_TOMB"

_run_lint --spec "$J_CUR" --tombstones "$J_TOMB" --base-spec "$J_BASE"
assert "(j) a deletion with no tombstone row is FLAGGED (rc 1)" _rc_is 1
assert "(j) output NAMES the vanished ID ($J_GONE)" _out_has "$J_GONE"
assert "(j) output points at the tombstone file as the place to add the row" \
    _out_has "$J_TOMB"
assert "(j) output does NOT name the surviving ID ($J_KEPT)" _out_lacks "$J_KEPT"

# ===========================================================================
# (k) THE CONVERSE CONTROL — the same deletion, WITH the tombstone row.
#
# Without (k), (j) would be satisfied by a lint that reds on ANY deletion,
# which would make the tombstone mechanism unusable and the whole rule a
# constant verdict rather than a discriminator.
# ===========================================================================
echo ""
echo "--- (k) converse control: the same deletion WITH a tombstone row ---"
RAN=$((RAN + 1))

K_TOMB="$TMPWORK/k.tombstones"
{
    printf '# tombstone fixture\n'
    printf '%s 2026-08-27 paragraph deleted in this very diff; no forwarding anchor\n' "$J_GONE"
} >"$K_TOMB"

assert "(k) meta: the tombstone fixture DOES carry a row for the vanished ID" \
    bash -c 'grep -qF -- "$1" "$2"' -- "$J_GONE" "$K_TOMB"

_run_lint --spec "$J_CUR" --tombstones "$K_TOMB" --base-spec "$J_BASE"
assert "(k) the SAME deletion with a tombstone row is CLEAN (rc 0)" _rc_is 0
assert "(k) the converse control produces NO output" _out_empty

# ===========================================================================
# (l) NO-OP BASE — a byte-identical base deletes nothing.
#
# Proves rule 5's clean verdict is an EARNED comparison result, not a
# hardcoded pass on the "no --base-spec violations to report" path.
# ===========================================================================
echo ""
echo "--- (l) no-op base: a byte-identical base is clean ---"
RAN=$((RAN + 1))

L_CUR="$TMPWORK/l_cur.md"
_spec_copy_with_anchors "$L_CUR" 4
L_BASE="$TMPWORK/l_base.md"
cp "$L_CUR" "$L_BASE"

assert "(l) meta: base and current are byte-identical" cmp -s "$L_CUR" "$L_BASE"
assert "(l) meta: the fixture actually carries anchors to compare" \
    bash -c '[ "$(grep -cE "^<!-- sc-anchor: sc-[0-9a-f]{6} -->$" "$1")" -ge 1 ]' -- "$L_CUR"

_run_lint --spec "$L_CUR" --tombstones "$TOMBSTONES" --base-spec "$L_BASE"
assert "(l) a no-op base yields a CLEAN verdict (rc 0)" _rc_is 0
assert "(l) a no-op base produces NO output" _out_empty

# ===========================================================================
# (m) BASE RESOLUTION IS NOT A SKIP.
#
# The failure mode INV-SF-5 exists to prevent is a gate that silently
# downgrades "I could not determine the base" to "deletion check skipped" —
# green on exactly the configuration where the check matters.
# ===========================================================================
echo ""
echo "--- (m) an unresolvable base is exit 2, never a silent skip ---"
RAN=$((RAN + 1))

_run_lint --spec "$SPEC_REL" --tombstones "$TOMB_REL" --base-spec "$TMPWORK/no-such-base.md"
assert "(m1) an unreadable --base-spec is exit 2" _rc_is 2
assert "(m1) an unreadable --base-spec is NEVER rc 0" bash -c '[ "$1" != 0 ]' -- "$LINT_RC"

_run_lint --spec "$SPEC_REL" --tombstones "$TOMB_REL" --base definitely-not-a-ref
assert "(m2) an unresolvable --base rev is exit 2" _rc_is 2
assert "(m2) an unresolvable --base rev is NEVER rc 0" bash -c '[ "$1" != 0 ]' -- "$LINT_RC"

_run_lint --spec "$SPEC_REL" --tombstones "$TOMB_REL" --base HEAD --base-spec "$SPEC"
assert "(m3) --base and --base-spec together is a usage error (rc 2)" _rc_is 2
assert "(m3) --base and --base-spec together is NEVER rc 0" bash -c '[ "$1" != 0 ]' -- "$LINT_RC"

# ===========================================================================
# (n) THE DEFAULT BASE IS LIVE — a flagless run really consults git.
# ===========================================================================
echo ""
echo "--- (n) default base: a flagless run consults git ---"
RAN=$((RAN + 1))

_run_lint
assert "(n) the flagless invocation on the shipped tree is CLEAN (rc 0)" _rc_is 0
assert "(n) the flagless invocation produces NO output" _out_empty
N_RC="$LINT_RC"
N_OUT="$LINT_OUT"

_run_lint --base HEAD
assert "(n) an explicit --base HEAD yields the same rc as the flagless run" _eq "$LINT_RC" "$N_RC"
assert "(n) an explicit --base HEAD yields byte-identical output to the flagless run" \
    bash -c '[ "$1" = "$2" ]' -- "$LINT_OUT" "$N_OUT"

# ===========================================================================
# (o) §9.2 IS ACTUALLY ANCHORED — the leaf's other user-observable signal.
#
# Asserted STRUCTURALLY, quantifying at run time over whatever headings exist,
# never by a pinned anchor count or a hardcoded "nine subsections". Leaf η
# (#6765) will rewrite §9.2's clause text and may add or split subsections; a
# pinned count would red on that legitimate edit and would then be maintained
# by deletion rather than by thought. Each quantifier carries its own
# NON-VACUITY floor, so "every heading is anchored" can never pass by finding
# zero headings.
# ===========================================================================
echo ""
echo "--- (o) §9.2 is anchored in the SHIPPED spec ---"
RAN=$((RAN + 1))

# o1 — the capability manifest's own α check.
_spec_has_a_wellformed_anchor() {
    local n
    n="$(grep -cE '^<!-- sc-anchor: sc-[0-9a-f]{6} -->$' "$SPEC" || true)"
    [ "${n:-0}" -ge 1 ] && return 0
    echo "no well-formed anchor line found in $SPEC"
    return 1
}

# o2 — the §9.2 section heading itself carries an anchor.
_heading_is_anchored() {
    local pat="$1" out
    out="$(awk -v pat="$pat" '
        $0 ~ pat {
            n++
            if (prev !~ /^<!-- sc-anchor: sc-[0-9a-f]{6} -->$/) {
                printf "%d: heading is NOT immediately preceded by a well-formed anchor: %s\n", FNR, $0
                printf "%d: (the preceding line was: %s)\n", FNR - 1, prev
                bad++
            }
        }
        { prev = $0 }
        END {
            if (n == 0) { printf "NO heading matched %s — the check would be vacuous\n", pat; exit 1 }
            if (bad > 0) exit 1
        }
    ' "$SPEC")" && return 0
    printf '%s\n' "$out"
    return 1
}

# o3 — EVERY `#### 9.2.N` heading present, whatever the current set is.
_all_9_2_subheadings_anchored() { _heading_is_anchored '^#### 9\.2\.[0-9]+ '; }

# o4 — scoped density floor: strictly more anchors than headings inside the
# §9.2 range, which is what proves the seeding is PARAGRAPH-level rather than
# headings-only. Both numbers are computed over the live file; no absolutes.
_9_2_anchor_density() {
    local out
    out="$(awk '
        BEGIN { in92 = 0; fence = 0; anchors = 0; heads = 0 }
        {
            if ($0 ~ /^### 9\.2 /) {
                in92 = 1; heads++
                if (prev ~ /^<!-- sc-anchor: sc-[0-9a-f]{6} -->$/) anchors++
                prev = $0; next
            }
            if (in92 && $0 ~ /^### /) in92 = 0
            if (in92) {
                if ($0 ~ /^[[:space:]]*```/) fence = !fence
                else if (!fence) {
                    if ($0 ~ /^<!-- sc-anchor: sc-[0-9a-f]{6} -->$/) anchors++
                    else if ($0 ~ /^#+ /) heads++
                }
            }
            prev = $0
        }
        END {
            printf "§9.2 range: anchors=%d headings=%d\n", anchors, heads
            if (heads == 0) { print "NO §9.2 section found — the check would be vacuous"; exit 1 }
            if (anchors <= heads) exit 1
        }
    ' "$SPEC")" && return 0
    printf '%s\n' "$out"
    return 1
}

# o5 — no anchor lands inside a fenced code block within §9.2.
_no_anchor_inside_9_2_fence() {
    local out
    out="$(awk '
        BEGIN { in92 = 0; fence = 0; seen_fence = 0 }
        {
            if ($0 ~ /^### 9\.2 /) { in92 = 1; next }
            if (in92 && $0 ~ /^### /) in92 = 0
            if (!in92) next
            if ($0 ~ /^[[:space:]]*```/) { fence = !fence; if (fence) seen_fence++; next }
            if (fence && index($0, "sc-anchor") > 0) {
                printf "%d: anchor inside a fenced code block: %s\n", FNR, $0
                bad++
            }
        }
        END {
            if (seen_fence == 0) { print "NO fenced block found inside §9.2 — the check would be vacuous"; exit 1 }
            if (bad > 0) exit 1
        }
    ' "$SPEC")" && return 0
    printf '%s\n' "$out"
    return 1
}

# o6 — cheap smell check that IDs carry NO positional information. Not a
# statistical test: two structural tells, both of which a sequential or
# section-derived minting scheme would trip immediately.
_ids_are_opaque() {
    local ids positional
    ids="$(grep -oE '^<!-- sc-anchor: sc-[0-9a-f]{6} -->$' "$SPEC" \
            | sed -E 's/^<!-- sc-anchor: sc-//; s/ -->$//' || true)"
    if [ -z "$ids" ]; then
        echo "no anchor IDs found — the check would be vacuous"
        return 1
    fi
    positional="$(printf '%s\n' "$ids" | grep '92' || true)"
    if [ -n "$positional" ]; then
        echo "anchor ID(s) contain the section digits '92' — IDs must not correlate with section numbers:"
        printf '%s\n' "$positional"
        return 1
    fi
    return 0
}

_ids_are_not_sorted() {
    local ids n
    ids="$(grep -oE '^<!-- sc-anchor: sc-[0-9a-f]{6} -->$' "$SPEC" \
            | sed -E 's/^<!-- sc-anchor: sc-//; s/ -->$//' || true)"
    n="$(printf '%s\n' "$ids" | grep -c . || true)"
    if [ "${n:-0}" -lt 2 ]; then
        echo "fewer than 2 anchor IDs — the sortedness tell would be vacuous"
        return 1
    fi
    if [ "$ids" = "$(printf '%s\n' "$ids" | LC_ALL=C sort)" ]; then
        echo "anchor IDs appear in ascending order down the file, which betrays sequential minting:"
        printf '%s\n' "$ids"
        return 1
    fi
    return 0
}

assert "(o1) the shipped spec carries at least one well-formed anchor" \
    _spec_has_a_wellformed_anchor

assert "(o2) the '### 9.2' section heading is immediately preceded by an anchor" \
    _heading_is_anchored '^### 9\.2 '

assert "(o3) EVERY '#### 9.2.N' subheading present is immediately preceded by an anchor" \
    _all_9_2_subheadings_anchored

assert "(o4) the §9.2 range holds strictly MORE anchors than headings (seeding is paragraph-level)" \
    _9_2_anchor_density

assert "(o5) no anchor lands inside a fenced code block within §9.2" \
    _no_anchor_inside_9_2_fence

assert "(o6) anchor IDs carry no section digits" _ids_are_opaque

assert "(o6) anchor IDs are NOT in ascending order down the file" _ids_are_not_sorted

# ===========================================================================
# (p) THE NORMATIVE AUTHORING NOTE.
#
# The note is normative for every later spec-conformance wave, so it is
# GUARDED, not merely written: a note that silently loses a clause stops being
# the thing later waves can rely on, and nothing else would notice.
# ===========================================================================
echo ""
echo "--- (p) the normative authoring note ---"
RAN=$((RAN + 1))

# One grep per clause, each a SHORT distinctive literal rather than a prose
# pin: a partial note must not pass, but a reworded sentence must not red.
_note_states() {
    grep -qF -- "$1" "$NOTE" && return 0
    echo "the authoring note does not state: $1"
    return 1
}

assert "(p1) docs/notes/spec-anchor-contract.md exists and is non-empty" test -s "$NOTE"

assert "(p2) the spec front matter LINKS the authoring note" \
    bash -c 'head -n 15 "$1" | grep -qF "docs/notes/spec-anchor-contract.md"' -- "$SPEC"

assert "(p3) the note states the ID grammar" _note_states 'sc-[0-9a-f]{6}'
assert "(p3) the note states the placement form" _note_states '<!-- sc-anchor: sc-XXXXXX -->'
assert "(p3) the note states that IDs are randomly generated" _note_states 'randomly generated'
assert "(p3) the note states that IDs are opaque" _note_states 'opaque'
assert "(p3) the note gives the minting command" _note_states 'openssl rand -hex 3'
assert "(p3) the note states that IDs are NEVER positional" _note_states 'never positional'
assert "(p3) the note states that a tombstoned ID is retired forever" _note_states 'retired forever'
assert "(p3) the note states that a tombstoned ID is never reused" _note_states 'never reused'
assert "(p3) the note names the tombstone sidecar path" _note_states 'docs/reify-language-spec.tombstones'
assert "(p3) the note states the SAME-diff deletion rule" _note_states 'SAME diff'
assert "(p3) the note states a heading anchor's scope" _note_states "whole run of intro prose"
assert "(p3) the note states the consumer rule: resolve by grepping the literal ID" \
    _note_states 'grepping the literal ID'
assert "(p3) the note forbids consumers parsing section numbers for identity" \
    _note_states 'section numbers'

# p4 — COLLISION SAFETY. A realistic-looking example ID in normative
# documentation is a live hazard the moment someone copies it: it either
# duplicates a live ID or burns one the tombstone file has no record of.
# `sc-XXXXXX` is not hex, so it CANNOT match the ID grammar — the hazard is
# made structurally impossible rather than managed after the fact. This also
# keeps the lint's scan surface honest: it reads only the spec and the
# sidecar, so an ID living in the note would be invisible to rule 2.
_note_has_no_valid_id() {
    local hits
    # Self-guarding: grep over a MISSING file yields no hits, so without this
    # the check would pass vacuously whenever the note does not exist —
    # exactly the state it is supposed to be meaningful in.
    if [ ! -s "$NOTE" ]; then
        echo "the authoring note is missing or empty at $NOTE — this check would be vacuous"
        return 1
    fi
    hits="$(grep -oE 'sc-[0-9a-f]{6}' "$NOTE" || true)"
    [ -z "$hits" ] && return 0
    echo "the authoring note contains valid-format anchor ID(s) — a copy-paste collision hazard:"
    printf '%s\n' "$hits"
    grep -nE 'sc-[0-9a-f]{6}' "$NOTE" || true
    return 1
}
assert "(p4) the note contains NO valid-format anchor ID (only the metavariable)" \
    _note_has_no_valid_id

assert "(p5) the note cites the owning PRD" \
    _note_states 'docs/prds/v0_6/spec-conformance-suite.md'

# ---------------------------------------------------------------------------
# Summary.
#
# test_summary exits 1 when FAIL > 0, so control only reaches the $RAN floor
# on the otherwise-all-green path — which is exactly where a zero-assertion
# run would have been laundered into a passing hard gate.
# ---------------------------------------------------------------------------
test_summary

if [ "$RAN" -eq 0 ]; then
    echo "test_spec_anchor_lint.sh: NO scenario executed — refusing to report green for a hard gate that asserted nothing" >&2
    exit 1
fi
