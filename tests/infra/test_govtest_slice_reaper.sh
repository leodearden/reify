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

# ---------------------------------------------------------------------------
# Block C — govtest_stale_units: the pure filter that decides WHICH leaked
# runs get reaped.
#
# Input is raw `systemctl --user list-units --all --plain --no-legend` text
# with the unit name in field 1. The fixture rows below reproduce the exact
# shape captured from the live host on 2026-08-20 for the real leaked run
# 270780, e.g.
#
#   reify-govtest270780.slice        loaded active active Slice /reify/govtest270780
#
# Liveness is driven ENTIRELY through REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS so
# no assertion depends on what happens to be running on the host. The var is
# set to a non-empty value on EVERY call here — including calls where nothing
# is meant to be alive — because an empty/unset value falls back to a real
# `kill -0`, and a low fixture pid like 111 may well be a live process on the
# test host, which would silently invert the expected result.
# ---------------------------------------------------------------------------
echo ""
echo "--- Block C: govtest_stale_units liveness + dedup filter ---"

# A pid far above any plausible /proc/sys/kernel/pid_max, used as the
# "nothing is alive" sentinel: non-empty (so the seam stays engaged) but
# matching no fixture pid.
_ALIVE_NONE="999999999"

# _listing_triple <pid> — the three rows one leaked run leaves behind.
_listing_triple() {
    local pid="$1"
    printf 'reify-govtest%s-agents.slice loaded active active Slice /reify/govtest%s/agents\n' "$pid" "$pid"
    printf 'reify-govtest%s-merge.slice  loaded active active Slice /reify/govtest%s/merge\n' "$pid" "$pid"
    printf 'reify-govtest%s.slice        loaded active active Slice /reify/govtest%s\n' "$pid" "$pid"
}

# _expect_stale <self_pid> <alive_list> <listing> <want>
#   The call runs inside a command substitution, so the env-prefixed knob is
#   confined to that subshell and cannot leak into later assertions.
#
#   The exit status is checked as well as the output, and NOT merely as
#   thoroughness: several assertions below expect EMPTY output, and an absent
#   or crashing implementation also produces empty output. Requiring rc=0
#   is what stops those from passing vacuously.
_expect_stale() {
    local self="$1" alive="$2" listing="$3" want="$4" got rc=0
    got="$(REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS="$alive" govtest_stale_units "$self" "$listing")" || rc=$?
    if [ "$rc" -ne 0 ]; then
        echo "govtest_stale_units returned $rc, want 0"
        return 1
    fi
    if [ "$got" != "$want" ]; then
        printf 'govtest_stale_units self=%s alive="%s" =>\n%s\n--- want ---\n%s\n' \
            "$self" "$alive" "$got" "$want"
        return 1
    fi
    return 0
}

# C1 — the core contract: three leaked units collapse to ONE action, and that
# action targets the PARENT. Measured on this host: stopping the parent slice
# alone made both children vanish from `list-units --all`, so emitting the
# children too would be two redundant stops plus an ordering hazard.
assert "C1: dead run's parent+2 children => exactly one line, the PARENT" \
    _expect_stale 999 "$_ALIVE_NONE" "$(_listing_triple 111)" "reify-govtest111.slice"

# C2 — the safety direction that matters most. run_all.sh schedules many
# lanes concurrently against ONE shared per-user systemd session, so reaping
# a live run's parent would kill a concurrent governance measurement midway
# and produce a confusing false RED in an unrelated lane.
assert "C2: live pid (in FAKE_ALIVE_PIDS) => NOTHING, even with all three units present" \
    _expect_stale 999 "222" "$(_listing_triple 222)" ""

# C3 — belt-and-braces self-exclusion: the caller's own units are never
# candidates, and this must hold on the liveness oracle's WORD alone. 555 is
# deliberately absent from the alive list, so it reads dead through the seam;
# only the explicit self check can save it.
assert "C3: caller's own self_pid => NOTHING even when it reads dead via the seam" \
    _expect_stale 555 "444" "$(_listing_triple 555)" ""

# C4 — everything at once, including the two names that must never be
# selectable: the production slice carrying real agent placement, and
# dark-factory's own unit namespace.
_C4_LISTING="$(_listing_triple 111)
$(_listing_triple 222)
$(_listing_triple 333)
reify-governed-agents.slice     loaded active active Slice /reify/governed/agents
df-verify-abc-def.scope         loaded active running /usr/bin/env

"
assert "C4: mixed listing (dead + live + self + production + df-verify + blank) => only the dead parent" \
    _expect_stale 333 "222" "$_C4_LISTING" "reify-govtest111.slice"

# C5 — several leaked runs reap independently, one parent line each, in
# first-seen order, with the two children of each deduped away.
_C5_LISTING="$(_listing_triple 111)
$(_listing_triple 777)"
_C5_WANT="reify-govtest111.slice
reify-govtest777.slice"
assert "C5: two distinct dead pids => one deduped parent line each, first-seen order" \
    _expect_stale 999 "$_ALIVE_NONE" "$_C5_LISTING" "$_C5_WANT"

# C6 — an empty listing is the STEADY STATE (no leaks), so it must be silent
# and, critically, exit 0: the sweep runs under `set -euo pipefail` in
# test_cpu_load_governance.sh, where a non-zero return would abort the whole
# governance suite before a single row ran.
_expect_stale_rc0() {
    local self="$1" alive="$2" listing="$3" out rc=0
    out="$(REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS="$alive" govtest_stale_units "$self" "$listing")" || rc=$?
    if [ "$rc" -ne 0 ]; then
        echo "govtest_stale_units returned $rc, want 0"
        return 1
    fi
    if [ -n "$out" ]; then
        printf 'expected no output, got:\n%s\n' "$out"
        return 1
    fi
    return 0
}

assert "C6a: empty listing => no output, exit 0" \
    _expect_stale_rc0 999 "$_ALIVE_NONE" ""
assert "C6b: whitespace-and-blank-lines-only listing => no output, exit 0" \
    _expect_stale_rc0 999 "$_ALIVE_NONE" "$(printf '\n   \n\t\n\n')"

# ---------------------------------------------------------------------------
# Block D — govtest_reap_stale: the ACTUATOR.
#
# Driven against a STUBBED `systemctl` placed first on PATH, so the
# assertions observe exactly which units the sweep would stop without ever
# contacting the real systemd user session. Each scenario runs in a CHILD
# shell (invoked via "$BASH", an absolute path, so the PATH restriction
# cannot break the interpreter lookup itself) which keeps the PATH override
# from leaking into the rest of this file.
# ---------------------------------------------------------------------------
echo ""
echo "--- Block D: govtest_reap_stale actuator (stubbed systemctl) ---"

_STUB_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/govtest-reaper.XXXXXX")"
# Own EXIT trap — this file installs no other, so no chaining is needed.
trap 'rm -rf "$_STUB_ROOT"' EXIT

mkdir -p "$_STUB_ROOT/bin-ok" "$_STUB_ROOT/bin-fail" "$_STUB_ROOT/bin-none"

_REAP_LOG="$_STUB_ROOT/stub.log"
_REAP_LISTING="$_STUB_ROOT/listing.txt"
_REAP_STDERR="$_STUB_ROOT/driver.err"
_REAP_DRIVER="$_STUB_ROOT/driver.sh"
_REAP_ALIVE="$_ALIVE_NONE"
_REAP_ADD_SELF=0
_REAP_RC=0

# The OK stub: record the full argv, and answer `list-units` from the fixture
# file. Shebang is /bin/bash (absolute), NOT /usr/bin/env bash, because the
# systemctl-absent scenario restricts PATH to an empty dir — `env bash` would
# then fail to resolve the interpreter and the stub would misreport.
cat > "$_STUB_ROOT/bin-ok/systemctl" <<'STUBEOF'
#!/bin/bash
printf '%s\n' "$*" >> "$GOVTEST_STUB_LOG"
for _a in "$@"; do
    if [ "$_a" = "list-units" ]; then
        [ -s "${GOVTEST_STUB_LISTING_FILE:-/nonexistent}" ] && cat "$GOVTEST_STUB_LISTING_FILE"
        exit 0
    fi
done
exit 0
STUBEOF

# The FAILING stub: every invocation exits non-zero, standing in for a broken
# or unavailable systemd user session.
cat > "$_STUB_ROOT/bin-fail/systemctl" <<'STUBEOF'
#!/bin/bash
printf '%s\n' "$*" >> "$GOVTEST_STUB_LOG"
exit 1
STUBEOF

chmod +x "$_STUB_ROOT/bin-ok/systemctl" "$_STUB_ROOT/bin-fail/systemctl"

# The driver runs in the child shell under `set -euo pipefail` — the SAME
# discipline test_cpu_load_governance.sh runs under — so a reaper that lets a
# failing systemctl escape as a non-zero status is caught here rather than in
# production as an aborted governance suite.
cat > "$_REAP_DRIVER" <<'DRIVEREOF'
#!/bin/bash
set -euo pipefail
# shellcheck source=tests/infra/govtest_slice_reaper_lib.sh
source "$GOVTEST_DRIVER_LIB"
if [ "${GOVTEST_DRIVER_ADD_SELF:-0}" = "1" ]; then
    # Append THIS child's own three units to the fixture, so the run has a
    # chance to (wrongly) reap itself.
    printf 'reify-govtest%s-agents.slice loaded active active Slice /reify/govtest%s/agents\n' "$$" "$$" >> "$GOVTEST_STUB_LISTING_FILE"
    printf 'reify-govtest%s-merge.slice  loaded active active Slice /reify/govtest%s/merge\n' "$$" "$$" >> "$GOVTEST_STUB_LISTING_FILE"
    printf 'reify-govtest%s.slice        loaded active active Slice /reify/govtest%s\n' "$$" "$$" >> "$GOVTEST_STUB_LISTING_FILE"
fi
echo "SELF_PID=$$" >&2
_rc=0
if [ "$#" -ge 1 ]; then
    govtest_reap_stale "$1" || _rc=$?
else
    govtest_reap_stale || _rc=$?
fi
exit "$_rc"
DRIVEREOF
chmod +x "$_REAP_DRIVER"

# _stub_reap <bindir> <listing> [self_pid]
#   Truncate the log, seed the listing fixture, run the driver. Sets _REAP_RC.
_stub_reap() {
    local bindir="$1" listing="$2"
    shift 2
    : > "$_REAP_LOG"
    printf '%s\n' "$listing" > "$_REAP_LISTING"
    _REAP_RC=0
    GOVTEST_STUB_LOG="$_REAP_LOG" \
    GOVTEST_STUB_LISTING_FILE="$_REAP_LISTING" \
    GOVTEST_DRIVER_LIB="$REAPER_LIB" \
    GOVTEST_DRIVER_ADD_SELF="$_REAP_ADD_SELF" \
    REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS="$_REAP_ALIVE" \
    PATH="$bindir" \
    "$BASH" "$_REAP_DRIVER" "$@" >/dev/null 2>"$_REAP_STDERR" || _REAP_RC=$?
    return 0
}

# Every unit the stub was asked to `stop`, in invocation order.
_reap_stopped_units() {
    sed -n 's/^--user stop //p' "$_REAP_LOG" 2>/dev/null || true
}

# _expect_reap_stops <bindir> <alive> <add_self> <listing> <want> [self_pid]
_expect_reap_stops() {
    local bindir="$1" alive="$2" add_self="$3" listing="$4" want="$5"
    shift 5
    _REAP_ALIVE="$alive"
    _REAP_ADD_SELF="$add_self"
    _stub_reap "$bindir" "$listing" "$@"
    if [ "$_REAP_RC" -ne 0 ]; then
        echo "govtest_reap_stale rc=$_REAP_RC, want 0"
        cat "$_REAP_STDERR" 2>/dev/null || true
        return 1
    fi
    local got
    got="$(_reap_stopped_units)"
    if [ "$got" != "$want" ]; then
        printf 'stopped:\n%s\n--- want ---\n%s\n--- full stub log ---\n' "$got" "$want"
        cat "$_REAP_LOG" 2>/dev/null || true
        return 1
    fi
    return 0
}

_D_DEAD_ONLY="$(_listing_triple 111)"

assert "D1: dead predecessor => exactly one stop, of the PARENT slice" \
    _expect_reap_stops "$_STUB_ROOT/bin-ok" "$_ALIVE_NONE" 0 "$_D_DEAD_ONLY" \
        "reify-govtest111.slice" 999

# D2 exercises the no-argument form, so self_pid defaults to the child's own
# $$ — and the child injects its OWN three units into the listing first. A
# reaper that forgot the default (or the self check) would stop its own
# parent slice out from under the very run doing the sweep.
_D2_LISTING="$(_listing_triple 111)
$(_listing_triple 222)
reify-governed-agents.slice     loaded active active Slice /reify/governed/agents"
assert "D2: live pid, own pid (via default \$\$) and production slice all skipped; only the dead parent stopped" \
    _expect_reap_stops "$_STUB_ROOT/bin-ok" "222" 1 "$_D2_LISTING" \
        "reify-govtest111.slice"

# D3 — the enumeration must be GLOB-SCOPED. The per-user systemd session is
# shared host-wide, so a sweep that listed every unit and filtered afterwards
# would have a blast radius bounded only by the name grammar. The glob is the
# outer of the two belt-and-braces bounds (the grammar re-filter is the
# inner), matching dark-factory's verify.py:3492/3503 pairing.
_expect_glob_scoped() {
    _REAP_ALIVE="$_ALIVE_NONE"
    _REAP_ADD_SELF=0
    _stub_reap "$_STUB_ROOT/bin-ok" "$_D_DEAD_ONLY" 999
    if ! grep -qF -- 'list-units --all --plain --no-legend reify-govtest*.slice' "$_REAP_LOG"; then
        echo "no glob-scoped list-units invocation in stub log:"
        cat "$_REAP_LOG" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "D3: enumeration is glob-scoped to reify-govtest*.slice, never the whole session" \
    _expect_glob_scoped

# D4 — systemctl absent entirely. The governance suite must still run on a
# host with no systemd user session at all.
_expect_reap_noop_without_systemctl() {
    _REAP_ALIVE="$_ALIVE_NONE"
    _REAP_ADD_SELF=0
    _stub_reap "$_STUB_ROOT/bin-none" "$_D_DEAD_ONLY" 999
    if [ "$_REAP_RC" -ne 0 ]; then
        echo "rc=$_REAP_RC, want 0"
        cat "$_REAP_STDERR" 2>/dev/null || true
        return 1
    fi
    if [ -s "$_REAP_LOG" ]; then
        printf 'expected an empty stub log, got:\n'
        cat "$_REAP_LOG"
        return 1
    fi
    return 0
}
assert "D4: systemctl absent from PATH => exit 0, nothing attempted" \
    _expect_reap_noop_without_systemctl

# D5 — a systemd fault must never change the governance suite's verdict. The
# sweep runs at the very top of that script under `set -euo pipefail`, so an
# escaping non-zero status would abort it before a single row ran and be
# reported as a governance regression.
_expect_reap_survives_failing_systemctl() {
    _REAP_ALIVE="$_ALIVE_NONE"
    _REAP_ADD_SELF=0
    _stub_reap "$_STUB_ROOT/bin-fail" "$_D_DEAD_ONLY" 999
    if [ "$_REAP_RC" -ne 0 ]; then
        echo "rc=$_REAP_RC, want 0 (a failing systemctl must be swallowed)"
        cat "$_REAP_STDERR" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "D5: systemctl failing every call => still exit 0 (fail-soft)" \
    _expect_reap_survives_failing_systemctl

test_summary
