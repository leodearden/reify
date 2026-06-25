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

    # Build ERE pattern fragments (for bash [[ =~ ]]) split with ''
    # (adjacent single-quoted strings) so this source file never contains a
    # contiguous time-signal keyword that could self-match the live scan.
    # '' splitting only works in single-quoted tokens; all '' splits below
    # are in single-quoted contexts.
    #
    # Using [[ =~ ]] instead of echo|grep avoids spawning two subprocesses
    # per logical line, making the live scan fast enough for 99 test files.
    local _op_re; _op_re='-l''[et][[:space:]][0-9]'
    local _ass_re; _ass_re='asse''rt'
    local _esc_re; _esc_re='wallcl''ock:allow'
    local _wc_re
    _wc_re='elap''sed|with''in[[:space:]]+[0-9]+[ms]s?|second''s|wall|durat''ion'
    # Variable-name suffixes: single-quoted so '' splits work correctly.
    local _wc_var_sfx
    _wc_var_sfx='ELAP''SED|_M?S([^A-Za-z0-9_]|$)|_NS([^A-Za-z0-9_]|$)|SECOND''S([^A-Za-z0-9_]|$)'
    _wc_re="${_wc_re}|${_wc_var_sfx}"

    local _viof; _viof="$(mktemp)"
    local _linesf; _linesf="$(mktemp)"
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
            # Note: variables used unquoted on RHS of =~ so bash treats
            # their values as ERE patterns (quoting would make them literal).
            if [[ "$logical" =~ $_esc_re ]]; then continue; fi
            # (1) Assert-wired
            if ! [[ "$logical" =~ $_ass_re ]]; then continue; fi
            # (2) Upper-bound operator: -le <int> or -lt <int>
            if ! [[ "$logical" =~ $_op_re ]]; then continue; fi
            # (3) Wall-clock lexeme
            if ! [[ "$logical" =~ $_wc_re ]]; then continue; fi
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

# Collect all mktemp -d directories for cleanup at EXIT.  Individual
# `trap ... EXIT` calls replace each other; a single handler over an array
# ensures every tmpdir is removed regardless of which section runs last.
_TMPDIRS=()
trap '[ "${#_TMPDIRS[@]}" -gt 0 ] && rm -rf "${_TMPDIRS[@]}"' EXIT

_s1_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s1_tmpdir")

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

# ===========================================================================
# Section 2: Hermetic allowlist assertions — each case must return ZERO
#             (NOT flagged) or confirm a true positive still fires.
# ===========================================================================
echo ""
echo "--- Section 2: hermetic allowlist fixtures ---"

# ---------------------------------------------------------------------------
# 2a: Lower-bound discriminator (-ge / -gt on elapsed var) — NOT flagged.
#     Only -le/-lt are upper bounds; lower bounds cannot be flaky-high.
# ---------------------------------------------------------------------------
_s2a_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2a_tmpdir")

printf '#!/usr/bin/env bash\n' > "$_s2a_tmpdir/fixture.sh"
printf '%s "completed above minimum" test "$_elapsed" -ge 1\n' \
    "$_ASS_WORD" >> "$_s2a_tmpdir/fixture.sh"
printf '%s "completed above zero" test "$_elapsed" -gt 0\n' \
    "$_ASS_WORD" >> "$_s2a_tmpdir/fixture.sh"

_s2a_rc=0
_detect_wallclock_upper_bound "$_s2a_tmpdir" 2>/dev/null || _s2a_rc=$?
assert "2a: lower-bound (-ge/-gt on elapsed) NOT flagged (returns 0)" \
    test "$_s2a_rc" -eq 0

# ---------------------------------------------------------------------------
# 2b: Inline escape (wallclock:allow token) — NOT flagged.
#     Blessed generous guards carry this token to opt out of the detector.
# ---------------------------------------------------------------------------
_s2b_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2b_tmpdir")

# Assemble the escape token from variables (self-match safety).
_ESC_TOKEN='wallcl''ock:allow'

printf '#!/usr/bin/env bash\n' > "$_s2b_tmpdir/fixture.sh"
printf '%s "%s val" test "$el" %s 3 # %s\n' \
    "$_ASS_WORD" "$_WC_LEX_PART" "$_UB_OP" "$_ESC_TOKEN" >> "$_s2b_tmpdir/fixture.sh"

_s2b_rc=0
_detect_wallclock_upper_bound "$_s2b_tmpdir" 2>/dev/null || _s2b_rc=$?
assert "2b: wallclock:allow escape on assert line NOT flagged (returns 0)" \
    test "$_s2b_rc" -eq 0

# ---------------------------------------------------------------------------
# 2c: Non-wall-clock upper bounds — NOT flagged.
#     Disk-MiB and port-range upper bounds have no time lexeme.
# ---------------------------------------------------------------------------
_s2c_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2c_tmpdir")

printf '#!/usr/bin/env bash\n' > "$_s2c_tmpdir/fixture.sh"
# disk-MiB: no time lexeme in description or variable name
printf '%s "CoW delta under 50 MiB limit" test "$delta_mib" -le 50\n' \
    "$_ASS_WORD" >> "$_s2c_tmpdir/fixture.sh"
# port-range: no time lexeme
printf '%s "port in valid range 1-65535" test "$port" -le 65535\n' \
    "$_ASS_WORD" >> "$_s2c_tmpdir/fixture.sh"

_s2c_rc=0
_detect_wallclock_upper_bound "$_s2c_tmpdir" 2>/dev/null || _s2c_rc=$?
assert "2c: non-wall-clock upper bounds (MiB/port) NOT flagged (returns 0)" \
    test "$_s2c_rc" -eq 0

# ---------------------------------------------------------------------------
# 2d: Bare timeout — NOT flagged (no assert keyword).
#     Anti-hang timeouts that are NOT wired to assert calls are not violations.
# ---------------------------------------------------------------------------
_s2d_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2d_tmpdir")

printf '#!/usr/bin/env bash\n' > "$_s2d_tmpdir/fixture.sh"
printf 'timeout 15 some_long_running_cmd_that_might_hang\n' \
    >> "$_s2d_tmpdir/fixture.sh"

_s2d_rc=0
_detect_wallclock_upper_bound "$_s2d_tmpdir" 2>/dev/null || _s2d_rc=$?
assert "2d: bare timeout (no assert) NOT flagged (returns 0)" \
    test "$_s2d_rc" -eq 0

# ---------------------------------------------------------------------------
# 2e: True positive still fires — lowercase elapsed, un-escaped (rc==1).
# ---------------------------------------------------------------------------
_s2e_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2e_tmpdir")

printf '#!/usr/bin/env bash\n' > "$_s2e_tmpdir/fixture.sh"
printf '%s "%s too slow (no escape)" test "$el" %s 5\n' \
    "$_ASS_WORD" "$_WC_LEX_PART" "$_UB_OP" >> "$_s2e_tmpdir/fixture.sh"

_s2e_rc=0
_detect_wallclock_upper_bound "$_s2e_tmpdir" 2>/dev/null || _s2e_rc=$?
assert "2e: true positive (lowercase elapsed, un-escaped) still fires (returns 1)" \
    test "$_s2e_rc" -eq 1

# ---------------------------------------------------------------------------
# 2f: True positive — UPPERCASE ELAPSED variable name with no time lexeme in
#     description — detector must flag this via the ELAPSED var-name suffix.
#     RED in step 3: the _wc_lex pattern for ELAPSED is incorrectly built via
#     double-quoted ''-splits (shell interprets '' literally in dquotes) so
#     ELAPSED in a var name goes undetected.  Step 4 fixes the pattern.
# ---------------------------------------------------------------------------
_s2f_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2f_tmpdir")

# Assemble UPPERCASE ELAPSED variable name from parts (self-match safety).
_ELAP_WORD='ELAP''SED'   # assembles to ELAPSED

printf '#!/usr/bin/env bash\n' > "$_s2f_tmpdir/fixture.sh"
printf '%s "check finished" test "$%s_14" %s 10\n' \
    "$_ASS_WORD" "$_ELAP_WORD" "$_UB_OP" >> "$_s2f_tmpdir/fixture.sh"

_s2f_rc=0
_detect_wallclock_upper_bound "$_s2f_tmpdir" 2>/dev/null || _s2f_rc=$?
assert "2f: UPPERCASE ELAPSED var name is flagged as wall-clock upper bound (returns 1)" \
    test "$_s2f_rc" -eq 1

# ---------------------------------------------------------------------------
# 2g: True positive — bare _S suffix variable (e.g. wait_S) — detector must
#     flag this via the _M?S var-name branch introduced by the reviewer fix.
#     Prior regex (_[SM]S) matched _MS and _SS but not bare _S, leaving a
#     real hole: `assert "done" test "$wait_S" -le 30` would slip through.
# ---------------------------------------------------------------------------
_s2g_tmpdir="$(mktemp -d)"; _TMPDIRS+=("$_s2g_tmpdir")

# Assemble the bare _S suffix from parts (self-match safety).
_S_SFX='_S'

printf '#!/usr/bin/env bash\n' > "$_s2g_tmpdir/fixture.sh"
printf '%s "check done" test "$wait%s" %s 30\n' \
    "$_ASS_WORD" "$_S_SFX" "$_UB_OP" >> "$_s2g_tmpdir/fixture.sh"

_s2g_rc=0
_detect_wallclock_upper_bound "$_s2g_tmpdir" 2>/dev/null || _s2g_rc=$?
assert "2g: bare _S suffix variable (wait_S -le 30) flagged as wall-clock upper bound (returns 1)" \
    test "$_s2g_rc" -eq 1

# ===========================================================================
# Section 3: LIVE guard — scan the real tests/infra for un-escaped
#             wall-clock upper-bound asserts.
#
# RED (step 5): the three legitimate survivors are not yet annotated, so the
# detector correctly flags them:
#   - test_find_uses_smoke_runner.sh (~line 83)    _t4_elapsed -lt 15
#   - test_occt_flock_gate.sh (~line 192)          _ELAPSED14 -le 10
#   - test_occt_flock_gate.sh (~line 617)          _ELAPSED22 -le 10
# GREEN (step 6): once each survivor carries a `# wallclock:allow` annotation,
# the detector skips them and the live scan returns 0.
# ===========================================================================
echo ""
echo "--- Section 3: live scan of real tests/infra ---"

# Exclude the guard file itself by basename so the live scan can include it
# without matching its own fixture-assembly code (belt+suspenders; the
# self-match safety invariant already ensures the source contains no literal
# flaggable construct, but excluding the basename is the cleanest guarantee).
_guard_base="$(basename "${BASH_SOURCE[0]}")"

_s3_rc=0
_detect_wallclock_upper_bound "$SCRIPT_DIR" "$_guard_base" 2>&1 || _s3_rc=$?

assert "live scan: no un-escaped wall-clock upper-bound asserts in tests/infra (returns 0)" \
    test "$_s3_rc" -eq 0

test_summary
