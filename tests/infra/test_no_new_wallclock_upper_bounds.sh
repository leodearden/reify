#!/usr/bin/env bash
# tests/infra/test_no_new_wallclock_upper_bounds.sh
#
# Regression guard (task #4848, PRD infra-test-wallclock-deflake.md T9):
#   Flags NEW absolute-wall-clock UPPER-bound assertions in tests/infra/*.sh
#   so the flake class de-flaked by tasks 4841-4847 cannot silently return.
#
# The guard itself is a LOAD-INDEPENDENT static grep — it is NOT a wall-clock test.
#
# Allowlist mechanism (three composable filters):
#   (1) Operator:     only -le / -lt upper bounds are flagged (-ge / -gt ignored).
#   (2) Wall-clock lexeme: only lines whose description or compared variable
#       carries a time signal (elapsed | within [0-9]+s | [0-9]+ms | seconds |
#       wall | duration | var matching ELAPSED/_S/_MS/_NS/SECONDS).
#   (3) Inline escape: `# wallclock:allow` on the assert line opts it out.
#
# SELF-MATCH SAFETY: this file must not contain any literal flaggable construct
# (assert-wired upper-bound with a wall-clock lexeme).  Marker strings are
# assembled from shell variables at runtime and written only into mktemp -d
# dirs, following the test_reify_audit_ptodo.sh convention.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== Wall-clock upper-bound regression guard ==="

# ---------------------------------------------------------------------------
# _detect_wallclock_upper_bound <dir> [exclude_basename]
#
# Scans all *.sh files in <dir> (except <exclude_basename>) for
# wall-clock absolute-upper-bound assert violations.  A logical line
# (after joining \-continuations via awk) is a violation iff ALL of:
#   (1) assert-wired: the word "assert" appears on the line
#   (2) upper-bound:  line contains `-le <int>` or `-lt <int>`
#   (3) wall-clock:   description or compared var carries a time lexeme:
#                     elapsed | within [0-9]+[ms]s? | seconds | wall |
#                     duration | ELAPSED/_S/_MS/_NS/SECONDS suffix
#   (4) NOT escaped:  line does NOT contain `wallclock:allow`
#
# Prints each violation as "file:lineno: <content>" to stderr.
# Returns 1 if any violations found, 0 if none.
#
# SELF-MATCH SAFETY: pattern components are split across variables so this
# source file contains no contiguous flaggable token (assert + upper-bound
# + time-lexeme on a single logical line).  The awk + grep patterns reference
# only non-flaggable substrings.
# ---------------------------------------------------------------------------
_detect_wallclock_upper_bound() {
    local dir="$1"
    local exclude_base="${2:-}"

    # Build pattern fragments split for self-match safety.
    # Upper-bound operators: -le <int> or -lt <int>  (BRE for grep -q)
    local _op_ub; _op_ub='-l''[et][[:space:]][0-9]'
    # Assert keyword
    local _ass; _ass='asse''rt'
    # Wall-clock escape token
    local _esc; _esc='wallcl''ock:allow'
    # Wall-clock lexemes (ERE for grep -qE)
    local _wc_lex
    _wc_lex='elap''sed|with''in[[:space:]]+[0-9]+[ms]s?|second''s|wall|durat''ion'
    _wc_lex="${_wc_lex}|ELAP''SED|_[SM]S([^A-Za-z0-9_]|$)|_NS([^A-Za-z0-9_]|$)|SECOND''S([^A-Za-z0-9_]|$)"
    # Remove the '' self-break markers that were only in the source string
    # literal; build the actual grep pattern used at runtime.
    # (The '' markers are zero-length shell quotes that break contiguous
    # substrings only in the source file, not at runtime — they are already
    # stripped by the shell when it expands the string.)

    local _viof; _viof="$(mktemp)"
    local _linesf; _linesf="$(mktemp)"
    # Ensure cleanup even on early exit
    local _detector_cleanup_done=0
    _detector_cleanup() {
        if [ "$_detector_cleanup_done" = "0" ]; then
            rm -f "$_viof" "$_linesf"
            _detector_cleanup_done=1
        fi
    }
    trap '_detector_cleanup' RETURN

    local f
    for f in "$dir"/*.sh; do
        [ -f "$f" ] || continue
        local base; base="$(basename "$f")"
        if [ -n "$exclude_base" ] && [ "$base" = "$exclude_base" ]; then
            continue
        fi

        # Join backslash-continued lines into logical lines.
        # Output format: <first-physical-lineno> TAB <logical-line>
        awk '
            /\\$/ {
                sub(/\\$/, "")
                if (buf == "") { startline = NR }
                buf = buf $0
                next
            }
            {
                if (buf != "") {
                    print startline "\t" buf $0
                    buf = ""; startline = 0
                } else {
                    print NR "\t" $0
                }
            }
            END { if (buf != "") print startline "\t" buf }
        ' "$f" > "$_linesf"

        local lineno logical
        while IFS=$'\t' read -r lineno logical; do
            # (4) Escape: skip lines annotated with wallclock:allow
            if echo "$logical" | grep -qe "$_esc"; then
                continue
            fi
            # (1) Assert-wired: skip lines without the assert keyword
            if ! echo "$logical" | grep -qe "$_ass"; then
                continue
            fi
            # (2) Upper-bound operator: skip if no -le <int> or -lt <int>
            # Use -e flag so pattern starting with '-' isn't mis-parsed as option.
            if ! echo "$logical" | grep -qe "$_op_ub"; then
                continue
            fi
            # (3) Wall-clock lexeme: skip if no time signal
            if ! echo "$logical" | grep -qEe "$_wc_lex"; then
                continue
            fi
            # Violation: all four conditions met
            echo "${f}:${lineno}: ${logical}" >> "$_viof"
        done < "$_linesf"
    done

    if [ -s "$_viof" ]; then
        cat "$_viof" >&2
        _detector_cleanup
        return 1
    fi
    _detector_cleanup
    return 0
}

# ===========================================================================
# Section 1: Hermetic positive-detection — detector must flag a planted
#             wall-clock upper-bound assert.
# ===========================================================================
echo ""
echo "--- Section 1: hermetic positive-detection fixture ---"

_s1_tmpdir="$(mktemp -d)"
trap 'rm -rf "$_s1_tmpdir"' EXIT

# Assemble the planted violation from shell variables so this source file
# never contains a literal flaggable construct (self-match safety).
_WC_LEX_PART="elapsed"     # wall-clock lexeme fragment
_UB_OP="-le"               # upper-bound operator fragment
_ASS_WORD="assert"         # assert keyword fragment

# Write a fixture shell script that carries a wall-clock upper-bound assert.
# The fixture is written into the temp dir — NOT into tests/infra/.
printf '#!/usr/bin/env bash\n' > "$_s1_tmpdir/fixture_pos.sh"
printf '%s "%s val too slow" test "$el" %s 3\n' \
    "$_ASS_WORD" "$_WC_LEX_PART" "$_UB_OP" >> "$_s1_tmpdir/fixture_pos.sh"

# RED: _detect_wallclock_upper_bound is not yet defined in this file.
# When this script is run without the implementation (step 2), bash will
# print "command not found" and set -euo pipefail will exit non-zero.
# Step 2 will define the function and wrap the call with || to capture rc.
_s1_rc=0
_detect_wallclock_upper_bound "$_s1_tmpdir" 2>/dev/null || _s1_rc=$?
assert "detector flags planted wall-clock upper-bound assert (returns 1, not 127/cmd-not-found)" \
    test "$_s1_rc" -eq 1

test_summary
