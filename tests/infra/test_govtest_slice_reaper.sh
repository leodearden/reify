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
# is running on the box.  That seam — and the lifecycle-only seam Block F uses
# — are both armed by REIFY_GOVTEST_TEST_MODE=1, which this file is the only
# thing in the repo to set; C7/C8 and F5 pin that the arming key is genuinely
# required, so a stray var on a production run cannot engage either one.
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
    got="$(REIFY_GOVTEST_TEST_MODE=1 REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS="$alive" \
        govtest_stale_units "$self" "$listing")" || rc=$?
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
    out="$(REIFY_GOVTEST_TEST_MODE=1 REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS="$alive" \
        govtest_stale_units "$self" "$listing")" || rc=$?
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

# --- C7/C8: the fake-liveness seam must be ARMING-GATED -------------------
#
# Every assertion above passes REIFY_GOVTEST_TEST_MODE=1 alongside the pid
# list. These two prove the marker is load-bearing rather than decorative.
#
# WHY IT MATTERS. Unlike the REIFY_CPU_GOV_TEST_* fixture seams — which
# redirect a READ and can at worst make a row measure the wrong file — this
# one replaces the oracle deciding what gets STOPPED, on the production path:
# govtest_reap_stale runs at the top of every real test_cpu_load_governance.sh
# run. A stray non-empty pid list with no arming key would make every govtest
# pid outside that list read DEAD, INVERTING the one-directional fail-safe and
# letting the sweep stop a live concurrent lane's parent slice mid-measurement.
#
# Both cases are host-independent despite using the REAL `kill -0` oracle:
#   * $$ is this very script, so it is alive by construction; and
#   * pid_max is capped at 2^22 = 4194304 on 64-bit Linux, so the
#     $_ALIVE_NONE sentinel can never name a live process.
# Neither call sets REIFY_GOVTEST_TEST_MODE, and both pass a pid list chosen
# so the fake oracle — if it were still consulted — would give the OPPOSITE
# answer to `kill -0`. That is what makes them fail RED against a library
# that honours the list unconditionally.
_expect_stale_unarmed() {
    local self="$1" alive="$2" listing="$3" want="$4" got rc=0
    got="$(REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS="$alive" \
        govtest_stale_units "$self" "$listing")" || rc=$?
    if [ "$rc" -ne 0 ]; then
        echo "govtest_stale_units returned $rc, want 0"
        return 1
    fi
    if [ "$got" != "$want" ]; then
        printf 'unarmed govtest_stale_units self=%s alive="%s" =>\n%s\n--- want ---\n%s\n' \
            "$self" "$alive" "$got" "$want"
        return 1
    fi
    return 0
}

# C7 — the dangerous direction. Without the arming key the fake list is
# ignored and `kill -0 $$` reports this script alive, so its units survive.
# A library that consulted the list would call $$ dead and emit its parent.
assert "C7: pid list WITHOUT the arming key is ignored — a live pid is not reaped" \
    _expect_stale_unarmed 999 "$_ALIVE_NONE" "$(_listing_triple $$)" ""

# C8 — and the oracle is genuinely `kill -0` rather than a blanket "assume
# alive": unarmed, a pid above pid_max still reaps. Without this, C7 would
# also pass against a library that had simply stopped reaping altogether.
assert "C8: unarmed, the real kill -0 oracle still reaps a pid that cannot exist" \
    _expect_stale_unarmed 999 "$$" "$(_listing_triple "$_ALIVE_NONE")" \
        "reify-govtest${_ALIVE_NONE}.slice"

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
# file.
#
# TWO PATH-HYGIENE CONSTRAINTS, both forced by the systemctl-absent scenario
# (D4), which sets PATH to a directory holding NOTHING:
#   * the shebang is /bin/bash (absolute), NOT /usr/bin/env bash — `env` would
#     have to resolve `bash` through that empty PATH and the stub would die
#     before recording anything;
#   * the body uses bash BUILTINS ONLY. An earlier draft echoed the fixture
#     with `cat`, which silently produced nothing under the restricted PATH
#     and made a working reaper look like it stopped no units. `systemctl`
#     lives in the same directory as coreutils, so the empty-PATH scenario
#     cannot be relaxed to "coreutils but no systemctl".
# The library under test is already builtin-only apart from systemctl itself,
# so it needs no equivalent accommodation.
cat > "$_STUB_ROOT/bin-ok/systemctl" <<'STUBEOF'
#!/bin/bash
printf '%s\n' "$*" >> "$GOVTEST_STUB_LOG"
for _a in "$@"; do
    if [ "$_a" = "list-units" ]; then
        if [ -s "${GOVTEST_STUB_LISTING_FILE:-/nonexistent}" ]; then
            while IFS= read -r _l; do printf '%s\n' "$_l"; done \
                < "$GOVTEST_STUB_LISTING_FILE"
        fi
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
    REIFY_GOVTEST_TEST_MODE=1 \
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

# D6 — the reap LOOP itself, driven with more than one line to emit.
#
# Every case above happens to produce exactly ONE stop, so none of them can
# see a loop that terminates after its first iteration. That is not a
# hypothetical defect shape here: the loop's stdin IS the heredoc carrying the
# remaining units, so a callee that consumed stdin would swallow the rest and
# reap only the first of several leaked runs — and the crash-residue this
# sweep exists for accumulates one run per SIGKILL, so multi-run listings are
# the NORMAL case, not the exotic one. (C5 covers multi-run for the pure
# filter, which has no actuator between the lines; E1 does prove a
# three-iteration loop, but for teardown, not this path.) The library detaches
# stdin on the stop for exactly this reason; D6 is what would notice if that
# regressed.
_D6_LISTING="$(_listing_triple 111)
$(_listing_triple 777)"
_D6_WANT="reify-govtest111.slice
reify-govtest777.slice"
assert "D6: TWO dead predecessors => two stops, parents only, first-seen order" \
    _expect_reap_stops "$_STUB_ROOT/bin-ok" "$_ALIVE_NONE" 0 "$_D6_LISTING" \
        "$_D6_WANT" 999

# D7 — the STEADY STATE, and the only case where the sweep must do nothing
# while systemctl is present and healthy. It is the state every run should be
# in once this task lands, so a sweep that stopped something here would be
# reaping live lanes on every single governance run.
assert "D7: listing of only live + own units => enumerated, but nothing stopped" \
    _expect_reap_stops "$_STUB_ROOT/bin-ok" "222" 1 "$(_listing_triple 222)" ""

# ---------------------------------------------------------------------------
# Block E — govtest_slice_teardown: the function that closes the CLEAN-EXIT
# leak.
#
# The leak it replaces: test_cpu_load_governance.sh's _cleanup_all consulted
# three `_ROW4_*_CREATED` flags before stopping anything, but the parent
# slice is vivified from FOUR call sites and only TWO of them set the flag.
# The two that do sit behind branches additionally requiring `taskset` and a
# readable Cpus_allowed_list; the two that don't are gated on cgroup support
# alone. On a host with governance but no `taskset`, the parent was therefore
# created and never stopped — on a fully green exit.
#
# So the assertions below pin UNCONDITIONALITY, which is exactly the property
# the flag mechanism lacked: teardown must issue all three stops with no
# precondition and after no "create" call of any kind. That state — nothing
# recorded as created — is precisely where the old code stopped nothing.
# ---------------------------------------------------------------------------
echo ""
echo "--- Block E: govtest_slice_teardown unconditional teardown ---"

cat > "$_STUB_ROOT/teardown.sh" <<'DRIVEREOF'
#!/bin/bash
set -euo pipefail
# shellcheck source=tests/infra/govtest_slice_reaper_lib.sh
source "$GOVTEST_DRIVER_LIB"
_rc=0
govtest_slice_teardown "$1" || _rc=$?
exit "$_rc"
DRIVEREOF
chmod +x "$_STUB_ROOT/teardown.sh"

# _stub_teardown <bindir> <pid>  — truncate log, run teardown, set _REAP_RC.
_stub_teardown() {
    local bindir="$1" pid="$2"
    : > "$_REAP_LOG"
    _REAP_RC=0
    GOVTEST_STUB_LOG="$_REAP_LOG" \
    GOVTEST_DRIVER_LIB="$REAPER_LIB" \
    PATH="$bindir" \
    "$BASH" "$_STUB_ROOT/teardown.sh" "$pid" >/dev/null 2>"$_REAP_STDERR" || _REAP_RC=$?
    return 0
}

_E_WANT="reify-govtest4242-agents.slice
reify-govtest4242-merge.slice
reify-govtest4242.slice"

# E1/E2 — all three stops, in children-then-parent order, with nothing
# created first. The ORDER preserves the rationale _cleanup_all already
# carried: stop the confined-quota parent LAST, after its children, so no
# quota'd empty parent unit is left behind.
_expect_teardown_stops() {
    local pid="$1" want="$2" got
    _stub_teardown "$_STUB_ROOT/bin-ok" "$pid"
    if [ "$_REAP_RC" -ne 0 ]; then
        echo "govtest_slice_teardown rc=$_REAP_RC, want 0"
        cat "$_REAP_STDERR" 2>/dev/null || true
        return 1
    fi
    got="$(_reap_stopped_units)"
    if [ "$got" != "$want" ]; then
        printf 'stopped:\n%s\n--- want ---\n%s\n--- full stub log ---\n' "$got" "$want"
        cat "$_REAP_LOG" 2>/dev/null || true
        return 1
    fi
    return 0
}

assert "E1/E2: teardown 4242 stops all three units, children-then-parent, with nothing created first" \
    _expect_teardown_stops 4242 "$_E_WANT"

# E3 — teardown derives ONLY from its argument. It must never enumerate, and
# never touch another run's units: it fires from an EXIT trap while other
# lanes' governance runs may be mid-measurement in the same systemd session.
_expect_teardown_scoped_to_pid() {
    _stub_teardown "$_STUB_ROOT/bin-ok" 4242
    local lines
    # `wc -l`, deliberately NOT `grep -c . || echo 0`: grep prints "0" AND
    # exits 1 on an empty file, so the `||` fallback appends a SECOND "0",
    # the two-line value breaks `[ -ne ]` with rc=2, and `if` reads that as
    # false — which made this assertion pass vacuously against a missing
    # implementation.
    lines="$(wc -l < "$_REAP_LOG" 2>/dev/null || echo 0)"
    lines="${lines//[[:space:]]/}"
    if [ "${lines:-0}" -ne 3 ]; then
        printf 'expected exactly 3 stub invocations (3 stops, no enumeration), got %s:\n' "$lines"
        cat "$_REAP_LOG" 2>/dev/null || true
        return 1
    fi
    if grep -qv '^--user stop reify-govtest4242' "$_REAP_LOG"; then
        printf 'stub log contains an invocation outside pid 4242:\n'
        cat "$_REAP_LOG" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "E3: teardown touches only pid 4242's units and never enumerates" \
    _expect_teardown_scoped_to_pid

# E4/E5 — teardown runs INSIDE the EXIT trap, so a non-zero return would
# overwrite the governance suite's real exit status and report a passing run
# as failed. Neither a missing systemctl nor a systemd fault may do that.
_expect_teardown_rc0() {
    local bindir="$1"
    _stub_teardown "$bindir" 4242
    if [ "$_REAP_RC" -ne 0 ]; then
        echo "rc=$_REAP_RC, want 0"
        cat "$_REAP_STDERR" 2>/dev/null || true
        return 1
    fi
    return 0
}

assert "E4: systemctl absent from PATH => teardown exits 0" \
    _expect_teardown_rc0 "$_STUB_ROOT/bin-none"
assert "E5: systemctl failing every call => teardown still exits 0 (EXIT-trap safe)" \
    _expect_teardown_rc0 "$_STUB_ROOT/bin-fail"

# ---------------------------------------------------------------------------
# Block F — WIRING. Prove the library is actually used by
# tests/infra/test_cpu_load_governance.sh rather than sitting orphaned beside
# it.
#
# This DRIVES that script as a child process; it deliberately does not grep
# its source. A source grep is the documentation-meta-test shape this repo's
# TDD rules prohibit, and it would not distinguish a call placed before the
# EXIT trap from one placed after it, nor a live call from one stranded in a
# dead branch.
#
# COST. Driving the script for real is expensive — a full
# REIFY_CPU_GOVERN_DISABLE=1 run was timed at ~24s wall / ~21s CPU (rc=0, 107
# passed, 21 SKIP), far too heavy for a member of run_all.sh's concurrent
# pool. The REIFY_CPU_GOV_TEST_LIFECYCLE_ONLY=1 seam exits the child right
# after its startup sweep, with the EXIT trap ALREADY installed so teardown
# still fires. That keeps this sub-second and makes the assertions strictly
# MORE non-vacuous: in that state the child has created nothing, which is
# exactly the condition under which the old flag-guarded _cleanup_all stopped
# nothing at all.
#
# PATH here PREPENDS the stub rather than replacing PATH (as Block D does),
# because the governance script legitimately needs mktemp/python3/etc. before
# it reaches the sweep. Prepending is enough: the stub shadows the real
# systemctl, so no real unit is ever touched.
# ---------------------------------------------------------------------------
echo ""
echo "--- Block F: library wired into test_cpu_load_governance.sh ---"

_WIRE_LOG="$_STUB_ROOT/wire.log"
_WIRE_OUT="$_STUB_ROOT/wire.out"
_WIRE_RC=0
_WIRE_STALE_PID=111

# Drive the real script ONCE; every assertion below reads the captured log.
: > "$_REAP_LOG"
_listing_triple "$_WIRE_STALE_PID" > "$_REAP_LISTING"
# REIFY_CPU_GOVERN_DISABLE=1 is inert on the lifecycle-only path (the child
# exits before governance is used). It is passed as a COST backstop: should
# the seam ever regress, this degrades to the ~24s measured full run instead
# of being SIGKILLed at the timeout with nothing learned.
timeout 60 env \
    PATH="$_STUB_ROOT/bin-ok:$PATH" \
    GOVTEST_STUB_LOG="$_REAP_LOG" \
    GOVTEST_STUB_LISTING_FILE="$_REAP_LISTING" \
    REIFY_GOVTEST_TEST_MODE=1 \
    REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS="$_ALIVE_NONE" \
    REIFY_CPU_GOV_TEST_LIFECYCLE_ONLY=1 \
    REIFY_CPU_GOVERN_DISABLE=1 \
    bash "$SCRIPT_DIR/test_cpu_load_governance.sh" \
    > "$_WIRE_OUT" 2>&1 || _WIRE_RC=$?
cp "$_REAP_LOG" "$_WIRE_LOG"

_wire_exit0() {
    if [ "$_WIRE_RC" -ne 0 ]; then
        echo "child test_cpu_load_governance.sh rc=$_WIRE_RC (124 = timeout), want 0"
        tail -n 30 "$_WIRE_OUT" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "F1: driven test_cpu_load_governance.sh exits 0 under the lifecycle-only seam" \
    _wire_exit0

_wire_stale_reaped() {
    if ! grep -qxF -- "--user stop reify-govtest${_WIRE_STALE_PID}.slice" "$_WIRE_LOG"; then
        echo "startup sweep never stopped the canned stale predecessor's parent slice:"
        cat "$_WIRE_LOG" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "F2: the startup sweep is wired — canned dead predecessor's parent slice was stopped" \
    _wire_stale_reaped

# F3 — the clean-exit fix. The child created NOTHING, so this stop can only
# come from an unconditional teardown; the flag-guarded predecessor stopped
# nothing in this state. The child's pid is RECOVERED from the log rather
# than assumed, since $$ inside the child differs from this script's own pid.
_wire_own_parent_stopped() {
    local unit pid pids="" n=0
    while IFS= read -r unit; do
        pid="$(govtest_slice_pid "$unit")"
        [ -n "$pid" ] || continue
        [ "$pid" = "$_WIRE_STALE_PID" ] && continue
        case " $pids " in
            *" $pid "*) ;;
            *) pids="$pids $pid"; n=$((n + 1)) ;;
        esac
    done < <(sed -n 's/^--user stop //p' "$_WIRE_LOG")

    if [ "$n" -ne 1 ]; then
        printf 'expected exactly ONE non-stale pid in the stop log, found %s (%s):\n' "$n" "$pids"
        cat "$_WIRE_LOG" 2>/dev/null || true
        return 1
    fi
    local child_pid="${pids# }"
    if ! grep -qxF -- "--user stop reify-govtest${child_pid}.slice" "$_WIRE_LOG"; then
        echo "child pid $child_pid appears in the log but its PARENT slice was never stopped:"
        cat "$_WIRE_LOG" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "F3: teardown is unconditional — child stopped its OWN parent slice having created nothing" \
    _wire_own_parent_stopped

# F4 — pins the seam itself, and with it this test's pool cost. A full run
# ends by printing test_helpers.sh's "Results: N passed, M failed"; the
# lifecycle-only path exits before any cycle, so that line must be ABSENT.
# Asserted on output rather than elapsed time, which would be flaky under
# concurrent pool load.
_wire_exited_at_seam() {
    if grep -q '^Results:' "$_WIRE_OUT"; then
        echo "child ran the FULL suite — the lifecycle-only seam did not take effect:"
        tail -n 15 "$_WIRE_OUT" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "F4: the lifecycle-only seam short-circuits before any cycle (keeps this a pool-cheap drive)" \
    _wire_exited_at_seam

# --- F5: the lifecycle-only seam must be ARMING-GATED ---------------------
#
# F1-F4 above pass REIFY_GOVTEST_TEST_MODE=1. F5 is the counterweight: the
# seam is the one knob in that script able to exit 0 having run ZERO
# governance rows, and run_all.sh judges a member by EXIT CODE alone (it
# parses no "Results:" line), so a single stray export — a verify_env entry,
# an operator shell, a future harness forwarding REIFY_* wholesale — would
# turn a host-exclusive gate into a silent no-op that still reports success.
# A comment saying "never set this" is not a guard; this asserts the code is.
#
# Two things are required, and the second is what makes it non-vacuous: the
# refusal must be LOUD (a silent ignore would be its own trap for whoever set
# the var deliberately) and the suite must actually CONTINUE. "Continued" is
# proven positively, by the first post-seam output line, not by absence.
#
# COST. Past the seam the child is the full ~24s suite — exactly what must not
# be paid for — so it is stopped the instant it has produced both pieces of
# evidence, which lands in well under a second. The 300 x 0.1s ceiling is a
# deadlock backstop, not the expected cost. SIGTERM is enough: this script's
# EXIT trap demonstrably runs on TERM (only SIGKILL skips it), and the stub
# systemctl is still first on PATH, so the child's teardown touches no real
# unit on the way out.
_DISARM_OUT="$_STUB_ROOT/disarm.out"
_DISARM_LOG="$_STUB_ROOT/disarm-stub.log"
_DISARM_LISTING="$_STUB_ROOT/disarm-listing.txt"

_wire_seam_refused_without_arming() {
    local pid i=0
    : > "$_DISARM_OUT"
    : > "$_DISARM_LOG"
    : > "$_DISARM_LISTING"

    # NOTE: no REIFY_GOVTEST_TEST_MODE here — that omission IS the test.
    PATH="$_STUB_ROOT/bin-ok:$PATH" \
    GOVTEST_STUB_LOG="$_DISARM_LOG" \
    GOVTEST_STUB_LISTING_FILE="$_DISARM_LISTING" \
    REIFY_CPU_GOV_TEST_LIFECYCLE_ONLY=1 \
    REIFY_CPU_GOVERN_DISABLE=1 \
    bash "$SCRIPT_DIR/test_cpu_load_governance.sh" > "$_DISARM_OUT" 2>&1 &
    pid=$!

    while [ "$i" -lt 300 ]; do
        if grep -q 'LIFECYCLE_ONLY=1 IGNORED' "$_DISARM_OUT" 2>/dev/null \
            && grep -q 'Cycle SELF' "$_DISARM_OUT" 2>/dev/null; then
            break
        fi
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.1
        i=$((i + 1))
    done

    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true

    if ! grep -q 'LIFECYCLE_ONLY=1 IGNORED' "$_DISARM_OUT" 2>/dev/null; then
        echo "unarmed LIFECYCLE_ONLY was not refused loudly:"
        head -n 20 "$_DISARM_OUT" 2>/dev/null || true
        return 1
    fi
    if ! grep -q 'Cycle SELF' "$_DISARM_OUT" 2>/dev/null; then
        echo "unarmed LIFECYCLE_ONLY short-circuited anyway — the suite never reached Cycle SELF:"
        head -n 20 "$_DISARM_OUT" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "F5: LIFECYCLE_ONLY without REIFY_GOVTEST_TEST_MODE is refused loudly and the suite runs on" \
    _wire_seam_refused_without_arming

test_summary
