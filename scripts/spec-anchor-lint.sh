#!/usr/bin/env bash
# spec-anchor-lint.sh — well-formedness gate for the spec-conformance
# sc-anchor substrate (task #6758, leaf α of
# docs/prds/v0_6/spec-conformance-suite.md; D3, §8.1).
#
# WHY
#   The conformance suite cites clauses of docs/reify-language-spec.md. Keying
#   a cite to a SECTION NUMBER is fragile: a renumber silently re-points every
#   cite, and nothing detects it (this leaf performed exactly such a renumber,
#   §15's mis-numbered `13.1` → `15.1`). The substrate that replaces section
#   numbers is an opaque, randomly-minted `sc-XXXXXX` anchor written as a
#   standalone HTML comment immediately preceding the anchored paragraph or
#   heading, plus a tombstone sidecar recording retired IDs.
#
#   That substrate is only worth anything if it is mechanically well-formed:
#   a duplicated ID makes a cite ambiguous, a typo'd ID makes a cite resolve
#   to nothing while looking fine in source, and a paragraph deleted without a
#   tombstone turns every consumer's cite into a silent dangling reference.
#   This gate is what keeps those three from happening.
#
# INPUT
#   --spec <path>        the specification to scan
#                        (default: docs/reify-language-spec.md)
#   --tombstones <path>  the retired-ID sidecar
#                        (default: docs/reify-language-spec.tombstones)
#   --repo-root <dir>    root that relative --spec/--tombstones resolve
#                        against (default: this script's parent directory)
#   -h | --help          usage
#
#   Relative paths resolve against --repo-root, NOT the caller's CWD, so the
#   gate behaves identically from any directory.
#
# RULES (all six HARD-FAIL; there is no --warn, no --strict promotion, and no
# skip path anywhere in this script)
#   1. FORMAT       — any line containing `sc-anchor` outside a fenced code
#                     block must match `^<!-- sc-anchor: sc-[0-9a-f]{6} -->$`
#                     EXACTLY. Catching the NEAR MISS is the point: an anchor
#                     the lint silently ignores is worse than no anchor, since
#                     a consumer greps for it and finds nothing.
#   2. UNIQUENESS   — no ID occurs twice in the spec. Every occurrence is
#                     reported, so both sites are named.
#   3. DISJOINTNESS — no live ID is also listed in the tombstone file. An ID
#                     is either live or retired, never both.
#   4. TOMBSTONES   — every data row is `sc-XXXXXX <YYYY-MM-DD> <reason>` with
#                     a calendar-plausible date; IDs are unique within the
#                     file; data rows are in LC_ALL=C ascending ID order.
#                     `#` comments and blank lines are ignored anywhere and
#                     are NOT sort keys.
#   6. PLACEMENT    — every anchor line is immediately followed by a line that
#                     is non-blank and is not itself an anchor line; an anchor
#                     as the file's last line is a violation. Without this,
#                     §8.1's "immediately preceding the anchored paragraph" is
#                     decorative and rule 5 has no referent — you cannot
#                     detect the deletion of a paragraph never bound to an ID.
#   (Rule 5, the deletion ⇒ same-diff tombstone rule, is added by the next
#   step of this leaf and documents its own base semantics here.)
#
#   Fenced code blocks are skipped when scanning for anchors, so a fenced
#   EXAMPLE anchor in some future spec section is never mistaken for a live
#   one. Prose that DISCUSSES the mechanism therefore belongs in the authoring
#   note (docs/notes/spec-anchor-contract.md), not in the spec body — or
#   inside a fence.
#
# OUTPUT
#   One `file:line: <message>` per violation on STDERR, followed by a summary
#   line naming the total and a remediation block. Nothing on stdout.
#
# EXIT
#   0  clean — no violations
#   1  at least one violation
#   2  usage error, missing-or-empty input, or an internal awk failure
#
#   1 and 2 are strictly distinct and that separation is load-bearing: an
#   internal failure or an unscannable corpus must never read as "clean". A
#   gate that scanned nothing looks identical, from the caller's side, to a
#   gate that scanned everything and found it clean.
#
# Usage: scripts/spec-anchor-lint.sh [--repo-root <dir>] [--spec <path>]
#                                    [--tombstones <path>]

set -euo pipefail

# Anchor IDs and tombstone rows are pure ASCII, and rule 4's sortedness is
# DEFINED in C collation — so pin the locale rather than inheriting whatever
# the caller's LC_COLLATE happens to be (en_US.UTF-8 collates differently and
# would make the same file sorted or unsorted depending on the environment).
export LC_ALL=C

REPO_ROOT=""
SPEC_ARG=""
TOMB_ARG=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --repo-root)   REPO_ROOT="${2:-}";  shift 2 ;;
        --spec)        SPEC_ARG="${2:-}";   shift 2 ;;
        --tombstones)  TOMB_ARG="${2:-}";   shift 2 ;;
        -h|--help)
            echo "Usage: $0 [--repo-root <dir>] [--spec <path>] [--tombstones <path>]"
            exit 0 ;;
        *) echo "ERROR: unknown argument: $1" >&2; exit 2 ;;
    esac
done

if [[ -z "$REPO_ROOT" ]]; then
    REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fi
if [[ ! -d "$REPO_ROOT" ]]; then
    echo "ERROR: --repo-root is not a directory: $REPO_ROOT" >&2
    exit 2
fi

# Resolve a possibly-relative path against REPO_ROOT (never against CWD).
_resolve() {
    case "$1" in
        /*) printf '%s' "$1" ;;
        *)  printf '%s/%s' "$REPO_ROOT" "$1" ;;
    esac
}

SPEC_PATH="$(_resolve "${SPEC_ARG:-docs/reify-language-spec.md}")"
TOMB_PATH="$(_resolve "${TOMB_ARG:-docs/reify-language-spec.tombstones}")"

# ── ANTI-VACUITY. A missing or zero-byte spec, or a missing sidecar, is a
# SCAN FAILURE (exit 2), never an empty pass. This is the exact configuration
# on which a "gracefully skip" gate reports green while checking nothing.
if [[ ! -f "$SPEC_PATH" ]]; then
    echo "ERROR: spec not found: $SPEC_PATH" >&2
    exit 2
fi
if [[ ! -s "$SPEC_PATH" ]]; then
    echo "ERROR: spec is empty: $SPEC_PATH (a zero-byte corpus is not a clean corpus)" >&2
    exit 2
fi
if [[ ! -f "$TOMB_PATH" ]]; then
    echo "ERROR: tombstone sidecar not found: $TOMB_PATH" >&2
    echo "       The sidecar must EXIST even while empty — an absent sidecar is a" >&2
    echo "       scan failure, not an empty set of retired IDs." >&2
    exit 2
fi

# ── Violation accumulator. Every entry is a ready-to-print
# `file:line: message` string; they are emitted together at the end so the
# report is one block rather than interleaved with progress noise.
_violations=()
_add_violation() { _violations+=("$1:$2: $3"); }

# ═══════════════════════════════════════════════════════════════════════════
# SPEC SCAN — ONE awk pass, deliberately not a `grep -oP | sort -u` pipeline.
# That two-process shape was the diagnosed truncation point in task-4586 (see
# scripts/check_event_inventory.sh:135-139). The single pass simultaneously
# tracks ```-fence state, validates anchor-line format (rule 1), records
# (id, line) pairs for rules 2/3, and resolves the one-line lookahead that
# rule 6 needs.
#
# Emits TAB-separated records:
#   A <line> <id>       a well-formed anchor
#   V <line> <message>  a format or placement violation
# ═══════════════════════════════════════════════════════════════════════════
_scan_spec() {
    awk '
        BEGIN { fence = 0; pending = 0; pending_line = 0 }
        {
            # Resolve any pending rule-6 lookahead against THIS line first,
            # before the fence bookkeeping consumes it: a fence opener is a
            # perfectly good thing for an anchor to precede.
            if (pending) {
                if ($0 ~ /^[[:space:]]*$/) {
                    printf "V\t%d\tanchor is followed by a blank line and anchors nothing; an anchor must IMMEDIATELY precede the paragraph or heading it anchors\n", pending_line
                } else if (index($0, "sc-anchor") > 0) {
                    printf "V\t%d\tanchor is immediately followed by another anchor line; each anchor must directly precede the paragraph or heading it anchors\n", pending_line
                }
                pending = 0
            }

            if ($0 ~ /^[[:space:]]*```/) { fence = !fence; next }
            if (fence) next
            if (index($0, "sc-anchor") == 0) next

            if ($0 ~ /^<!-- sc-anchor: sc-[0-9a-f]{6} -->$/) {
                id = $0
                sub(/^<!-- sc-anchor: /, "", id)
                sub(/ -->$/, "", id)
                printf "A\t%d\t%s\n", FNR, id
                pending = 1
                pending_line = FNR
            } else {
                printf "V\t%d\tmalformed anchor line (expected exactly `<!-- sc-anchor: sc-XXXXXX -->` with a 6-hex-digit id): %s\n", FNR, $0
            }
        }
        END {
            if (pending) {
                printf "V\t%d\tanchor is the LAST line of the file and anchors nothing\n", pending_line
            }
        }
    ' "$1"
}

if ! SPEC_RECORDS="$(_scan_spec "$SPEC_PATH")"; then
    echo "ERROR: awk failed while scanning $SPEC_PATH" >&2
    exit 2
fi

declare -A ANCHOR_LINES=()   # id -> space-separated line numbers
ANCHOR_ORDER=()              # ids in first-occurrence order

while IFS=$'\t' read -r _kind _line _payload; do
    [[ -z "$_kind" ]] && continue
    case "$_kind" in
        A)
            if [[ -z "${ANCHOR_LINES[$_payload]:-}" ]]; then
                ANCHOR_ORDER+=("$_payload")
                ANCHOR_LINES["$_payload"]="$_line"
            else
                ANCHOR_LINES["$_payload"]="${ANCHOR_LINES[$_payload]} $_line"
            fi
            ;;
        V) _add_violation "$SPEC_PATH" "$_line" "$_payload" ;;
        *) echo "ERROR: internal — unrecognised spec-scan record kind '$_kind'" >&2; exit 2 ;;
    esac
done <<<"$SPEC_RECORDS"

# ── RULE 2: uniqueness. Emit one `file:line:` per occurrence so BOTH (all)
# sites are named — "id X is duplicated" without the sites makes the reader
# grep for it themselves.
for _id in "${ANCHOR_ORDER[@]:-}"; do
    [[ -z "$_id" ]] && continue
    read -r -a _lines <<<"${ANCHOR_LINES[$_id]}"
    if [[ ${#_lines[@]} -gt 1 ]]; then
        for _l in "${_lines[@]}"; do
            _add_violation "$SPEC_PATH" "$_l" \
                "duplicate anchor id \`$_id\` (occurs ${#_lines[@]} times in this file, at lines ${ANCHOR_LINES[$_id]}); an id must identify exactly one clause"
        done
    fi
done

# ═══════════════════════════════════════════════════════════════════════════
# TOMBSTONE SCAN — rule 4. Same single-pass shape.
#   T <line> <id>       a well-formed data row
#   V <line> <message>  a grammar or date violation
# ═══════════════════════════════════════════════════════════════════════════
_scan_tombstones() {
    awk '
        /^[[:space:]]*#/ { next }
        /^[[:space:]]*$/ { next }
        {
            if ($0 !~ /^sc-[0-9a-f]{6} [0-9]{4}-[0-9]{2}-[0-9]{2} .+$/) {
                printf "V\t%d\tmalformed tombstone row (expected `sc-XXXXXX <YYYY-MM-DD> <reason>`, all three fields present): %s\n", FNR, $0
                next
            }
            id   = substr($0, 1, 9)
            date = substr($0, 11, 10)
            mm = substr(date, 6, 2) + 0
            dd = substr(date, 9, 2) + 0
            if (mm < 1 || mm > 12 || dd < 1 || dd > 31) {
                printf "V\t%d\tcalendar-implausible retirement date `%s` in tombstone row for `%s`\n", FNR, date, id
                next
            }
            printf "T\t%d\t%s\n", FNR, id
        }
    ' "$1"
}

if ! TOMB_RECORDS="$(_scan_tombstones "$TOMB_PATH")"; then
    echo "ERROR: awk failed while scanning $TOMB_PATH" >&2
    exit 2
fi

declare -A TOMB_LINE=()      # id -> line of its first data row
TOMB_PREV_ID=""
TOMB_PREV_LINE=0

while IFS=$'\t' read -r _kind _line _payload; do
    [[ -z "$_kind" ]] && continue
    case "$_kind" in
        T)
            if [[ -n "${TOMB_LINE[$_payload]:-}" ]]; then
                _add_violation "$TOMB_PATH" "$_line" \
                    "duplicate tombstone row for \`$_payload\` (already retired at line ${TOMB_LINE[$_payload]}); an id is retired exactly once"
            else
                TOMB_LINE["$_payload"]="$_line"
            fi
            # Sortedness is compared between CONSECUTIVE DATA ROWS in file
            # order — comments and blanks were dropped by the scan above, so
            # they are provably not sort keys.
            if [[ -n "$TOMB_PREV_ID" && ! "$_payload" > "$TOMB_PREV_ID" ]]; then
                _add_violation "$TOMB_PATH" "$_line" \
                    "tombstone rows are out of order: \`$_payload\` must sort after \`$TOMB_PREV_ID\` (line $TOMB_PREV_LINE); data rows are LC_ALL=C ascending by id"
            fi
            TOMB_PREV_ID="$_payload"
            TOMB_PREV_LINE="$_line"
            ;;
        V) _add_violation "$TOMB_PATH" "$_line" "$_payload" ;;
        *) echo "ERROR: internal — unrecognised tombstone-scan record kind '$_kind'" >&2; exit 2 ;;
    esac
done <<<"$TOMB_RECORDS"

# ── RULE 3: disjointness. Anchored at the LIVE site and naming the tombstone
# site, so the report names the id and both paths.
for _id in "${ANCHOR_ORDER[@]:-}"; do
    [[ -z "$_id" ]] && continue
    if [[ -n "${TOMB_LINE[$_id]:-}" ]]; then
        read -r -a _lines <<<"${ANCHOR_LINES[$_id]}"
        _add_violation "$SPEC_PATH" "${_lines[0]}" \
            "\`$_id\` is LIVE here but is also tombstoned at $TOMB_PATH:${TOMB_LINE[$_id]}; an id is either live or retired, never both"
    fi
done

# ═══════════════════════════════════════════════════════════════════════════
# REPORT
# ═══════════════════════════════════════════════════════════════════════════
if [[ ${#_violations[@]} -gt 0 ]]; then
    printf '%s\n' "${_violations[@]}" >&2
    {
        echo ""
        echo "ERROR: ${#_violations[@]} sc-anchor violation(s) found."
        echo "  spec:       $SPEC_PATH"
        echo "  tombstones: $TOMB_PATH"
        echo ""
        echo "An anchor is a standalone line \`<!-- sc-anchor: sc-XXXXXX -->\`"
        echo "IMMEDIATELY preceding the paragraph or heading it anchors, whose id is"
        echo "6 random hex digits (\`openssl rand -hex 3\`), unique across the spec, and"
        echo "never reused. Retiring one means moving it into the tombstone sidecar in"
        echo "the SAME diff as the deletion."
        echo ""
        echo "Full contract: docs/notes/spec-anchor-contract.md"
    } >&2
    exit 1
fi

exit 0
