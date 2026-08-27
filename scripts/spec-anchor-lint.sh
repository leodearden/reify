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
#   --base <rev>         revision whose spec text is rule 5's "before" side
#                        (default: HEAD). Mutually exclusive with --base-spec.
#   --base-spec <file>   read the "before" side from this file instead of git
#                        (for hermetic testing). Mutually exclusive with --base.
#   --repo-root <dir>    root that relative --spec/--tombstones/--base-spec
#                        resolve against (default: this script's parent dir)
#   -h | --help          usage
#
#   Relative paths resolve against --repo-root, NOT the caller's CWD, so the
#   gate behaves identically from any directory.
#
# RULES (all seven HARD-FAIL; there is no --warn, no --strict promotion, and no
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
#   5. DELETION     — every ID that was live at the BASE and is absent from
#                     the current spec must have a row in the tombstone file.
#                     Deleting an anchored paragraph without retiring its id in
#                     the SAME diff turns every consumer's cite into a silent
#                     dangling reference, which is strictly worse than citing a
#                     section number (a stale section number is visibly stale).
#   7. NO SWALLOWED — every anchor-SHAPED line in the file is either LIVE or
#                     REPORTED. Anchor-shaped lines are tallied over the whole
#                     file with fence state IGNORED and reconciled against the
#                     live set, so a line the fence walk skipped is named
#                     rather than silently dropped. See NO SWALLOWED ANCHORS
#                     below for the two holes this closes.
#
#   Fenced code blocks are skipped when scanning for anchors, so a fenced
#   EXAMPLE anchor in some future spec section is never mistaken for a live
#   one. Prose that DISCUSSES the mechanism therefore belongs in the authoring
#   note (docs/notes/spec-anchor-contract.md), not in the spec body — or
#   inside a fence, written with the non-hex metavariable id `sc-XXXXXX` so it
#   is not anchor-SHAPED at all (rule 7 reports a hex-id anchor in a fence).
#
# NO SWALLOWED ANCHORS (rule 7) — why a fence-skipping scan needs a tally
#   The fence walk toggles on any ``` line, which leaves two ways for a real
#   anchor to become invisible while the gate still reports "clean":
#     - DESYNC. A nested construct — a 4-backtick block that itself contains a
#       ``` line, routine when documenting markdown — flips the toggle an odd
#       number of times and leaves fence state stuck ON for an ARBITRARY
#       trailing region. Rules 1/2/6 go inert there and nothing says so.
#     - IN-FENCE ANCHOR. A well-formed anchor written inside a fence is not a
#       live anchor, so a consumer greps its id and finds nothing — the same
#       silent-dangling-cite failure rule 1 exists to prevent, one level up.
#   Neither is detectable from the LIVE set alone: "scanned everything, found
#   nothing" and "stopped scanning at line 900" produce byte-identical output.
#   The whole-file tally is what distinguishes them, and its reconciliation
#   (live + swallowed == total) is checked as an INTERNAL invariant, so a
#   future divergence between the two arms' regexes is exit 2 rather than a
#   quiet under-count.
#
# BASE SEMANTICS — stated honestly rather than implied
#   The default base is HEAD, which gives a PER-COMMIT posture: it catches
#   deletions in the WORKING TREE relative to the last commit. Branch-wide
#   coverage follows only because every commit passes the gate in turn — it is
#   NOT a property of any single invocation. A merge-time caller that wants
#   branch scope must pass `--base <merge-base>` explicitly. Wiring that
#   merge-gate invocation is leaf θ (#6766), not this script.
#
#   Four configurations are genuine base-RESOLUTION FAILURES and are exit 2,
#   never a downgrade to "deletion check skipped":
#     - `--base-spec` naming an unreadable file;
#     - `--base` naming a rev that does not resolve to a commit;
#     - --repo-root not being a git work tree (when a git base is needed);
#     - an in-tree `--spec` that does not exist at the base rev — this is the
#       spec-was-renamed case, and hard-failing it is what stops a rename from
#       silently orphaning every id.
#
#   ONE configuration resolves to an EMPTY base rather than failing: a `--spec`
#   OUTSIDE --repo-root (a candidate copy under /tmp, say) has no git history
#   at all, so no id was live at the base and rule 5 is satisfied. That is an
#   earned comparison result over an artifact with no prior version, not a
#   skip — and it cannot mask a real deletion in the gate, because the gate
#   always runs on the in-tree spec, which takes the hard-failing branch above.
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
#                                    [--base <rev> | --base-spec <file>]

set -euo pipefail

# Anchor IDs and tombstone rows are pure ASCII, and rule 4's sortedness is
# DEFINED in C collation — so pin the locale rather than inheriting whatever
# the caller's LC_COLLATE happens to be (en_US.UTF-8 collates differently and
# would make the same file sorted or unsorted depending on the environment).
export LC_ALL=C

REPO_ROOT=""
SPEC_ARG=""
TOMB_ARG=""
BASE_REV=""
BASE_SPEC_ARG=""
SAW_BASE=0
SAW_BASE_SPEC=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --repo-root)   REPO_ROOT="${2:-}";      shift 2 ;;
        --spec)        SPEC_ARG="${2:-}";       shift 2 ;;
        --tombstones)  TOMB_ARG="${2:-}";       shift 2 ;;
        --base)        BASE_REV="${2:-}";       SAW_BASE=1;      shift 2 ;;
        --base-spec)   BASE_SPEC_ARG="${2:-}";  SAW_BASE_SPEC=1; shift 2 ;;
        -h|--help)
            echo "Usage: $0 [--repo-root <dir>] [--spec <path>] [--tombstones <path>]"
            echo "          [--base <rev> | --base-spec <file>]"
            exit 0 ;;
        *) echo "ERROR: unknown argument: $1" >&2; exit 2 ;;
    esac
done

if [[ $SAW_BASE -eq 1 && $SAW_BASE_SPEC -eq 1 ]]; then
    echo "ERROR: --base and --base-spec are mutually exclusive (given both)" >&2
    exit 2
fi

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

# ── Scratch file for a git-materialised base. Registered once, before any
# code path can create it, so no branch can leak it.
_BASE_SCRATCH=""
_cleanup() { [[ -n "$_BASE_SCRATCH" ]] && rm -f "$_BASE_SCRATCH" || true; }
trap _cleanup EXIT

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
# It also implements rule 7 in the same pass: `total` tallies anchor-SHAPED
# lines with fence state IGNORED, so a line the fence walk skipped is reported
# (and reconciled below) instead of vanishing. See NO SWALLOWED ANCHORS in the
# header for the desync and in-fence holes that closes.
#
# Emits TAB-separated records:
#   A <line> <id>          a well-formed, LIVE anchor
#   V <line> <message>     a format, placement, or swallowed-anchor violation
#   C <total> <live> <sw>  the rule-7 tally, emitted exactly once at END
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

            # RULE 7, arm 1 — the whole-file tally. Deliberately evaluated
            # BEFORE the fence bookkeeping and with fence state ignored: it is
            # the ONLY reading of the file that a desynchronised fence toggle
            # cannot suppress, which is what makes it a check rather than a
            # second opinion from the same broken walk.
            is_anchor = ($0 ~ /^<!-- sc-anchor: sc-[0-9a-f]{6} -->$/)
            if (is_anchor) total++

            if ($0 ~ /^[[:space:]]*```/) { fence = !fence; next }
            if (fence) {
                # RULE 7, arm 2 — an anchor-shaped line the fence walk is
                # about to skip. Named here, where its line number is known.
                # A MALFORMED `sc-anchor` mention inside a fence is left
                # alone: that is exactly the documented-example form.
                if (is_anchor) {
                    swallowed++
                    printf "V\t%d\tanchor-shaped line is inside a fenced code block, so it is NOT a live anchor and no consumer can resolve its id; move it out of the fence, or write the example with the non-hex metavariable id `sc-XXXXXX`. (If this line looks like it should be OUTSIDE a fence, an unbalanced or nested ``` fence earlier in the file has desynchronised fence tracking and everything after it is going unscanned.)\n", FNR
                }
                next
            }
            if (index($0, "sc-anchor") == 0) next

            if (is_anchor) {
                id = $0
                sub(/^<!-- sc-anchor: /, "", id)
                sub(/ -->$/, "", id)
                printf "A\t%d\t%s\n", FNR, id
                live++
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
            printf "C\t%d\t%d %d\n", total, live, swallowed
        }
    ' "$1"
}

if ! SPEC_RECORDS="$(_scan_spec "$SPEC_PATH")"; then
    echo "ERROR: awk failed while scanning $SPEC_PATH" >&2
    exit 2
fi

# ── BASE RESOLUTION (rule 5's "before" side). See BASE SEMANTICS in the header
# for why exactly one configuration resolves to an empty base and the rest are
# hard failures. There is no path here that downgrades an unknown base to
# "deletion check skipped".
BASE_TEXT_PATH=""
BASE_LABEL=""

if [[ $SAW_BASE_SPEC -eq 1 ]]; then
    BASE_TEXT_PATH="$(_resolve "$BASE_SPEC_ARG")"
    if [[ ! -f "$BASE_TEXT_PATH" || ! -r "$BASE_TEXT_PATH" ]]; then
        echo "ERROR: --base-spec is not a readable file: $BASE_TEXT_PATH" >&2
        echo "       Refusing to continue: an unresolvable base must never be" >&2
        echo "       downgraded to \"deletion check skipped\"." >&2
        exit 2
    fi
    BASE_LABEL="$BASE_TEXT_PATH"
else
    BASE_REV="${BASE_REV:-HEAD}"
    if ! git -C "$REPO_ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        echo "ERROR: not a git work tree: $REPO_ROOT" >&2
        echo "       Rule 5 needs a base revision; pass --base-spec <file> to" >&2
        echo "       supply the \"before\" side directly." >&2
        exit 2
    fi
    if ! git -C "$REPO_ROOT" rev-parse --verify --quiet "$BASE_REV^{commit}" >/dev/null; then
        echo "ERROR: --base does not resolve to a commit: $BASE_REV" >&2
        exit 2
    fi
    _spec_rel=""
    case "$SPEC_PATH" in
        "$REPO_ROOT"/*) _spec_rel="${SPEC_PATH#"$REPO_ROOT"/}" ;;
    esac
    if [[ -n "$_spec_rel" ]]; then
        _BASE_SCRATCH="$(mktemp "${TMPDIR:-/tmp}/spec-anchor-base.XXXXXX")"
        if ! git -C "$REPO_ROOT" show "$BASE_REV:$_spec_rel" >"$_BASE_SCRATCH" 2>/dev/null; then
            echo "ERROR: $_spec_rel does not exist at base rev $BASE_REV" >&2
            echo "       If the spec was renamed, pass --base <rev> / --base-spec <file>" >&2
            echo "       naming its previous location — a rename must not silently" >&2
            echo "       orphan every id that was live before it." >&2
            exit 2
        fi
        BASE_TEXT_PATH="$_BASE_SCRATCH"
        BASE_LABEL="$BASE_REV:$_spec_rel"
    else
        # --spec is outside the work tree: it has no git history, so no id was
        # live at the base. An EARNED empty comparison, not a skip (header).
        BASE_TEXT_PATH=""
        BASE_LABEL="(empty base: $SPEC_PATH is outside $REPO_ROOT)"
    fi
fi

# Parse the base with the SAME extractor as the current spec — a single
# derivation, so a format change can never desynchronise the two sides. Only
# `A` records matter here: the base's own violations are the base commit's
# problem, and re-reporting them would blame this diff for them.
BASE_RECORDS=""
if [[ -n "$BASE_TEXT_PATH" ]]; then
    if ! BASE_RECORDS="$(_scan_spec "$BASE_TEXT_PATH")"; then
        echo "ERROR: awk failed while scanning the base spec ($BASE_LABEL)" >&2
        exit 2
    fi
fi

declare -A ANCHOR_LINES=()   # id -> space-separated line numbers
ANCHOR_ORDER=()              # ids in first-occurrence order

_A_SEEN=0                    # A records this loop actually consumed
_SCAN_TOTAL=""               # rule-7 tally: anchor-shaped lines, fence ignored
_SCAN_LIVE=""                # rule-7 tally: anchors the fence walk accepted
_SCAN_SWALLOWED=""           # rule-7 tally: anchor-shaped lines inside a fence

while IFS=$'\t' read -r _kind _line _payload; do
    [[ -z "$_kind" ]] && continue
    case "$_kind" in
        A)
            _A_SEEN=$((_A_SEEN + 1))
            if [[ -z "${ANCHOR_LINES[$_payload]:-}" ]]; then
                ANCHOR_ORDER+=("$_payload")
                ANCHOR_LINES["$_payload"]="$_line"
            else
                ANCHOR_LINES["$_payload"]="${ANCHOR_LINES[$_payload]} $_line"
            fi
            ;;
        V) _add_violation "$SPEC_PATH" "$_line" "$_payload" ;;
        C)
            _SCAN_TOTAL="$_line"
            _SCAN_LIVE="${_payload%% *}"
            _SCAN_SWALLOWED="${_payload##* }"
            ;;
        *) echo "ERROR: internal — unrecognised spec-scan record kind '$_kind'" >&2; exit 2 ;;
    esac
done <<<"$SPEC_RECORDS"

# ── RULE 7's RECONCILIATION. Every anchor-shaped line in the file must be
# accounted for as either LIVE or SWALLOWED, and the live count the scanner
# reported must equal the number of A records this loop actually consumed. A
# mismatch means the two arms disagree — a divergent regex, or a truncated
# record stream (the task-4586 failure mode this script's single-pass shape is
# written against). That is an INTERNAL failure (exit 2), never "clean": a
# gate that lost records mid-stream looks exactly like a clean one from here.
if [[ -z "$_SCAN_TOTAL" ]]; then
    echo "ERROR: internal — the spec scan of $SPEC_PATH emitted no tally record" >&2
    exit 2
fi
if [[ "$_A_SEEN" -ne "$_SCAN_LIVE" ]]; then
    echo "ERROR: internal — spec scan reported $_SCAN_LIVE live anchor(s) but $_A_SEEN record(s) arrived ($SPEC_PATH)" >&2
    exit 2
fi
if [[ $((_SCAN_LIVE + _SCAN_SWALLOWED)) -ne "$_SCAN_TOTAL" ]]; then
    echo "ERROR: internal — anchor tally does not reconcile for $SPEC_PATH:" >&2
    echo "       $_SCAN_TOTAL anchor-shaped line(s) whole-file, but $_SCAN_LIVE live + $_SCAN_SWALLOWED swallowed" >&2
    echo "       Some anchor-shaped line was neither accepted nor reported; refusing to" >&2
    echo "       report a verdict over a corpus that was not fully accounted for." >&2
    exit 2
fi

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

# ── RULE 5: deletion ⇒ same-diff tombstone. `base_ids \ current_ids` must be
# a subset of `tombstone_ids`. Reported against the BASE (path or rev:path),
# because that is where the vanished id can still be seen.
declare -A BASE_ID_LINE=()
BASE_ORDER=()

while IFS=$'\t' read -r _kind _line _payload; do
    [[ "$_kind" == "A" ]] || continue
    if [[ -z "${BASE_ID_LINE[$_payload]:-}" ]]; then
        BASE_ID_LINE["$_payload"]="$_line"
        BASE_ORDER+=("$_payload")
    fi
done <<<"$BASE_RECORDS"

for _id in "${BASE_ORDER[@]:-}"; do
    [[ -z "$_id" ]] && continue
    [[ -n "${ANCHOR_LINES[$_id]:-}" ]] && continue   # still live: nothing deleted
    [[ -n "${TOMB_LINE[$_id]:-}" ]] && continue      # properly retired
    _add_violation "$BASE_LABEL" "${BASE_ID_LINE[$_id]}" \
        "\`$_id\` was live at the base but is absent from $SPEC_PATH and has no row in $TOMB_PATH; deleting an anchored paragraph requires retiring its id in the SAME diff"
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
        echo "  base:       $BASE_LABEL"
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
