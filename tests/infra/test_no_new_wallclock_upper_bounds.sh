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
