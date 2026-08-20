#!/usr/bin/env bash
# tests/infra/test_govtest_slice_reaper.sh — guards the govtest slice reaper
# library (task 5930).
#
# WHAT IT GUARDS
# tests/infra/test_cpu_load_governance.sh creates three per-run systemd user
# units — reify-govtest$$.slice and its -agents / -merge children — and leaked
# them in two distinct ways:
#   (1) CLEAN EXIT.  _row4_confine_apply_quota vivifies the PARENT slice from
#       four call sites, but only two of them recorded the flag that the EXIT
#       trap consulted before stopping it, so on a host with cgroup governance
#       but no `taskset` the parent survived a fully green run.
#   (2) SIGKILL.  A verify timeout / harness reap / OOM kill skips the EXIT
#       trap entirely (measured: TERM, INT and HUP all DO run it — only KILL
#       does not), leaving all three units behind with nothing to clean them.
# tests/infra/govtest_slice_reaper_lib.sh closes (1) with an unconditional
# teardown and (2) with a startup sweep of dead predecessors.  This file is
# that library's test.
#
# HERMETIC — which is what justifies the `pool` bucket in
# run-all-classification.manifest.  It is pure bash string handling plus a
# STUBBED `systemctl` placed first on PATH; it never touches real cgroups,
# never contacts the real systemd user session, and never stops a real unit.
# Process liveness is driven through the REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS
# seam rather than real host PIDs, so the result does not depend on what else
# is running on the box.
#
# Auto-discovered by tests/infra/run_all.sh (glob test_*.sh).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

REAPER_LIB="$SCRIPT_DIR/govtest_slice_reaper_lib.sh"
[ -f "$REAPER_LIB" ] || {
    echo "ERROR: govtest_slice_reaper_lib.sh not found at $REAPER_LIB" >&2
    exit 1
}
# shellcheck source=tests/infra/govtest_slice_reaper_lib.sh
source "$REAPER_LIB"

echo "=== govtest slice reaper tests (task 5930) ==="

# ---------------------------------------------------------------------------
# Block A — govtest_slice_pid: the name grammar.
#
# This is the single chokepoint that decides whether a unit name is eligible
# to be stopped at all, so its NEGATIVES matter more than its positives: the
# production slices (reify-governed-{agents,merge}.slice) live in the same
# per-user systemd session and must never be selectable by any code path here.
# ---------------------------------------------------------------------------
echo ""
echo "--- Block A: govtest_slice_pid name grammar ---"

# _expect_pid <unit> <want>   — want "" means "not a govtest unit".
_expect_pid() {
    local unit="$1" want="$2" got
    got="$(govtest_slice_pid "$unit")"
    if [ "$got" != "$want" ]; then
        echo "govtest_slice_pid '$unit' => '$got', want '$want'"
        return 1
    fi
    return 0
}

assert "A1: parent reify-govtest1285669.slice => 1285669" \
    _expect_pid "reify-govtest1285669.slice" "1285669"
assert "A2: child reify-govtest1285669-agents.slice => 1285669" \
    _expect_pid "reify-govtest1285669-agents.slice" "1285669"
assert "A3: child reify-govtest1285669-merge.slice => 1285669" \
    _expect_pid "reify-govtest1285669-merge.slice" "1285669"

# The production slice is the most important negative: it is what a
# too-loose prefix match would sweep away mid-run on a live host.
assert "A4: production reify-governed-agents.slice => EMPTY (never selectable)" \
    _expect_pid "reify-governed-agents.slice" ""
assert "A5: production reify-governed-merge.slice => EMPTY (never selectable)" \
    _expect_pid "reify-governed-merge.slice" ""
assert "A6: reify-govtest.slice (no digits) => EMPTY" \
    _expect_pid "reify-govtest.slice" ""
assert "A7: reify-govtestabc.slice (non-numeric pid) => EMPTY" \
    _expect_pid "reify-govtestabc.slice" ""
assert "A8: reify-govtest123-other.slice (unknown child suffix) => EMPTY" \
    _expect_pid "reify-govtest123-other.slice" ""
assert "A9: reify-govtest123-agents.scope (wrong unit suffix) => EMPTY" \
    _expect_pid "reify-govtest123-agents.scope" ""
assert "A10: df-verify-x-y.scope (dark-factory's own units) => EMPTY" \
    _expect_pid "df-verify-x-y.scope" ""
assert "A11: empty string => EMPTY" \
    _expect_pid "" ""

# ---------------------------------------------------------------------------
# Block B — govtest_slice_units: this run's three unit names, in TEARDOWN
# order (children first, parent last).  The order is the contract, not an
# accident: it mirrors the children-then-parent rationale already documented
# in test_cpu_load_governance.sh's _cleanup_all.
# ---------------------------------------------------------------------------
echo ""
echo "--- Block B: govtest_slice_units emission + ordering ---"

_expect_units() {
    local pid="$1" want="$2" got
    got="$(govtest_slice_units "$pid")"
    if [ "$got" != "$want" ]; then
        printf 'govtest_slice_units %s =>\n%s\n--- want ---\n%s\n' "$pid" "$got" "$want"
        return 1
    fi
    return 0
}

_B_WANT="reify-govtest4242-agents.slice
reify-govtest4242-merge.slice
reify-govtest4242.slice"

assert "B1: govtest_slice_units 4242 emits agents, merge, parent in teardown order" \
    _expect_units 4242 "$_B_WANT"

# Round-trip: every name the emitter produces must be re-recognised by the
# grammar as belonging to the SAME pid.  This is what keeps the two halves of
# the library from drifting apart — an emitter change that the grammar does
# not accept would make teardown emit names the sweep can never clean up.
_units_roundtrip() {
    local pid="$1" unit got rc=0
    while IFS= read -r unit; do
        [ -n "$unit" ] || continue
        got="$(govtest_slice_pid "$unit")"
        if [ "$got" != "$pid" ]; then
            echo "roundtrip: '$unit' => '$got', want '$pid'"
            rc=1
        fi
    done <<EOF
$(govtest_slice_units "$pid")
EOF
    return "$rc"
}

assert "B2: every emitted unit name round-trips back to pid 4242 via govtest_slice_pid" \
    _units_roundtrip 4242

test_summary
