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

# ---------------------------------------------------------------------------
# Block G — govtest_profile_set: the name-grammar PROFILE (task 6386).
#
# WHY THE LIBRARY IS PARAMETERISED AT ALL.  tests/infra/test_cpu_governed_exec_
# hostexcl.sh creates FIVE per-run systemd units under its own `reify-test`
# prefix and leaks them in the same two ways task 5930 closed for the govtest
# prefix.  Copying ~340 lines of reaper to serve it would be a lockstep
# duplicate of an already-reviewed safety mechanism; instead the two literals
# this library hardcoded — the prefix and the child-suffix set — become a
# validated profile and everything else is shared.
#
# THE SETTER IS A SAFETY BOUNDARY, NOT A CONVENIENCE.  govtest_slice_pid's
# anchored grammar is the single chokepoint that makes the production
# reify-governed-*.slice units unreachable from every stop path in this
# library, and the profile is interpolated UNQUOTED into that regex (quoted,
# it would match literally and the grammar would never fire at all).  The
# charset validation asserted in G8/G9 is therefore the thing standing between
# a caller and a widened blast radius: a prefix of `reify-.*` would make
# `reify.slice` a match, and that is the shared implicit ROOT of the LIVE
# orchestrator hierarchy — stopping it cascades into real agent placement.
#
# From G1 on, this FILE runs under the reify-test profile; Blocks H, I and J
# all target that namespace.  Blocks A-F above are deliberately left untouched
# and ran under the DEFAULT profile — their staying green is the regression
# proof that test_cpu_load_governance.sh's behaviour is byte-for-byte
# preserved.  G12 closes that loop from the other side, in a child shell that
# sources the library and never calls the setter at all.
# ---------------------------------------------------------------------------
echo ""
echo "--- Block G: govtest_profile_set profile parameterisation ---"

_G_PROFILE_ERR="$_STUB_ROOT/profile.err"

# _profile_is_reify_test — the profile is ambient state rather than a value
# that can be read back, so "unchanged" is asserted through the two functions
# it actually drives.  Used by the refusal cases below to prove a rejected
# call left the previous profile intact rather than half-applying it.
_profile_is_reify_test() {
    local pid units
    pid="$(govtest_slice_pid 'reify-test1234.slice')"
    if [ "$pid" != "1234" ]; then
        printf 'profile drifted: govtest_slice_pid reify-test1234.slice => %s, want 1234\n' "$pid"
        return 1
    fi
    units="$(govtest_slice_units 1234 | tr '\n' ',')"
    if [ "$units" != "reify-test1234-agents.slice,reify-test1234-merge.slice,reify-test1234-taskweight.slice,reify-test1234-mergeweight.slice,reify-test1234.slice," ]; then
        printf 'profile drifted: govtest_slice_units 1234 => %s\n' "$units"
        return 1
    fi
    return 0
}

# (a) — G1 ARMS the profile for the remainder of this file.  assert() runs its
# checker directly in THIS shell (redirect only, no command-substitution
# subshell), which is what lets an ambient-state mutation like this one
# survive the call.
_set_test_profile() {
    govtest_profile_set reify-test agents merge taskweight mergeweight
}
assert "G1: govtest_profile_set reify-test agents merge taskweight mergeweight => rc 0" \
    _set_test_profile

assert "G2: parent reify-test1234.slice => 1234 under the armed profile" \
    _expect_pid "reify-test1234.slice" "1234"
assert "G3: inherited child reify-test1234-agents.slice => 1234" \
    _expect_pid "reify-test1234-agents.slice" "1234"
assert "G4: new child reify-test1234-taskweight.slice => 1234" \
    _expect_pid "reify-test1234-taskweight.slice" "1234"
assert "G5: new child reify-test1234-mergeweight.slice => 1234" \
    _expect_pid "reify-test1234-mergeweight.slice" "1234"

# (b) THE SAFETY NEGATIVES.  Grouped by the hazard each one represents rather
# than one assert per string, so a failure names the class that broke.
_expect_pid_all_empty() {
    local unit got rc=0
    for unit in "$@"; do
        got="$(govtest_slice_pid "$unit")"
        if [ -n "$got" ]; then
            printf "govtest_slice_pid '%s' => '%s', want EMPTY\n" "$unit" "$got"
            rc=1
        fi
    done
    return "$rc"
}

# G6 is the assertion this whole design turns on.  reify.slice is the shared
# implicit root of BOTH hierarchies — the production reify-governed.slice and
# reify-governed-agents.slice nest under it and carry live orchestrator agent
# placement — so a grammar that matched it would hand every stop path in this
# library the ability to cascade-kill the running fleet.
assert "G6: reify.slice and the production reify-governed-* slices => EMPTY (never selectable)" \
    _expect_pid_all_empty \
        "reify.slice" \
        "reify-governed.slice" \
        "reify-governed-agents.slice" \
        "reify-governed-merge.slice"

# G7 — the LEGACY pidless dash-nesting parents this task exists to reap are
# correctly OUTSIDE the pid grammar.  That is not an oversight to be fixed by
# widening the regex: they carry no pid, so the liveness oracle has nothing to
# consult, which is exactly why they need the separate explicitly-listed path
# Blocks H/I add rather than admission here.  reify-test-task-1234.slice is
# the pre-rename D7 unit — its extra dash segment is what vivified those
# parents in the first place.
assert "G7: legacy pidless parents and the pre-rename D7/D8 names => EMPTY" \
    _expect_pid_all_empty \
        "reify-test.slice" \
        "reify-test-task.slice" \
        "reify-test-merge.slice" \
        "reify-test-task-1234.slice"

assert "G8: shape violations (non-numeric pid, unknown suffix, wrong unit type, empty) => EMPTY" \
    _expect_pid_all_empty \
        "reify-testabc.slice" \
        "reify-test1234-other.slice" \
        "reify-test1234-agents.scope" \
        ""

# G9 — the two prefixes are disjoint namespaces.  A sweep armed for one
# profile must never reach the other's units: test_cpu_load_governance.sh and
# test_cpu_governed_exec_hostexcl.sh can run in the same shared per-user
# systemd session, and each owns its own liveness bookkeeping.
assert "G9: cross-profile isolation — reify-govtest1234.slice => EMPTY under the reify-test profile" \
    _expect_pid_all_empty "reify-govtest1234.slice"

# (c)/(d) THE REFUSALS.  A rejected profile must leave the previous one fully
# intact: half-applying a prefix while keeping the old suffixes would produce a
# grammar nobody reviewed.
_expect_profile_refused() {
    local rc=0 intact_rc=0
    : > "$_G_PROFILE_ERR"
    govtest_profile_set "$@" >/dev/null 2>"$_G_PROFILE_ERR" || rc=$?

    # The "previous profile survived" half of the contract, read FIRST —
    # before the containment re-arm below could mask it.
    _profile_is_reify_test || intact_rc=$?

    # CONTAINMENT, and it is not belt-and-braces: a regression in any of these
    # refusals means the call SUCCEEDED, which silently re-points the profile
    # this whole FILE runs under from G1 onwards. Measured while mutation-
    # testing G18 — deleting the guard under test turned one real failure into
    # NINE, with H1-H4/H11/H12 and J3/J4 all reporting causes that named the
    # wrong thing, because they were parsing unit names under a profile nobody
    # meant to set. Re-arming unconditionally keeps a broken refusal reported
    # as the one failure it is. It cannot hide the drift, which was already
    # read into intact_rc above.
    _set_test_profile || true

    if [ "$rc" -eq 0 ]; then
        printf 'govtest_profile_set %s returned 0, want non-zero\n' "$*"
        return 1
    fi
    if [ ! -s "$_G_PROFILE_ERR" ]; then
        printf 'govtest_profile_set %s refused SILENTLY — want a message on stderr\n' "$*"
        return 1
    fi
    return "$intact_rc"
}

# G10 — regex metacharacters are the dangerous shape, because the prefix is
# interpolated unquoted into the [[ =~ ]] pattern.  `reify-.*` would match
# reify.slice (see G6); `reify-test$` and `[a-z]+` are the same class of
# widening reached by different metacharacters.  `reify_test` and "" are the
# conservative-charset backstop: nothing outside [a-z0-9-] gets in, so no
# future metacharacter needs its own case here.
_g10() {
    _expect_profile_refused 'reify-.*' agents merge || return 1
    _expect_profile_refused 'reify-test$' agents merge || return 1
    _expect_profile_refused '[a-z]+' agents merge || return 1
    _expect_profile_refused 'reify_test' agents merge || return 1
    _expect_profile_refused '' agents merge || return 1
    return 0
}

# G11 — the DASH refusal is load-bearing, not tidiness.  systemd dash-nesting
# means a child suffix of `d7-task` would name reify-test1234-d7-task.slice,
# which vivifies a NEW implicit parent reify-test1234-d7.slice that nothing
# names, tears down, or sweeps — recreating the exact leak class this task
# exists to close.  The dot and empty cases keep the emitted name inside the
# grammar the pid regex will later have to re-recognise.
_g11() {
    _expect_profile_refused reify-test agents 'd7-task' || return 1
    _expect_profile_refused reify-test 'task.weight' merge || return 1
    _expect_profile_refused reify-test agents '' || return 1
    _expect_profile_refused reify-test 'Agents' merge || return 1
    return 0
}

assert "G10: prefixes with regex metacharacters or an off-charset shape are REFUSED, profile left intact" \
    _g10
assert "G11: child suffixes carrying a DASH, a dot, an uppercase char or nothing are REFUSED, profile left intact" \
    _g11

# (e) — govtest_slice_name: the single-name accessor.  The hostexcl suite needs
# each of its five names individually (they go into five separate
# REIFY_CPU_GOVERN_SLICE_* overrides), so it cannot consume the newline-
# separated teardown list govtest_slice_units emits.
_expect_slice_name() {
    local want="$1"
    shift
    local got
    got="$(govtest_slice_name "$@")"
    if [ "$got" != "$want" ]; then
        printf "govtest_slice_name %s => '%s', want '%s'\n" "$*" "$got" "$want"
        return 1
    fi
    return 0
}

assert "G12: govtest_slice_name 1234 => reify-test1234.slice (bare parent)" \
    _expect_slice_name "reify-test1234.slice" 1234
assert "G13: govtest_slice_name 1234 taskweight => reify-test1234-taskweight.slice" \
    _expect_slice_name "reify-test1234-taskweight.slice" 1234 taskweight
assert "G14: govtest_slice_name 1234 agents => reify-test1234-agents.slice" \
    _expect_slice_name "reify-test1234-agents.slice" 1234 agents

# (f) — emission order is TEARDOWN order: every declared child in DECLARED
# order, then the bare parent LAST.  The parent must stay last for the same
# reason task 5930 documented — never leave a quota'd empty parent behind — and
# the children must stay in declared order so the hostexcl suite's five names
# and this list cannot disagree about which suffix is which.
_G_UNITS_WANT="reify-test1234-agents.slice
reify-test1234-merge.slice
reify-test1234-taskweight.slice
reify-test1234-mergeweight.slice
reify-test1234.slice"

assert "G15: govtest_slice_units 1234 emits four children in declared order, bare parent LAST" \
    _expect_units 1234 "$_G_UNITS_WANT"

# G16 re-runs Block B2's anti-drift discipline under the new profile: an
# emitter that produced a name the grammar does not accept would make teardown
# stop units the startup sweep could never recognise as its own residue —
# precisely the leak this task is closing, reintroduced one level up.
assert "G16: every emitted unit name round-trips back to pid 1234 via govtest_slice_pid" \
    _units_roundtrip 1234

# (g) DEFAULT-PROFILE REGRESSION — a child shell that sources the library and
# never calls the setter.  Blocks A-F above prove the default is preserved for
# a process that sourced the library BEFORE any setter call existed; this
# proves it for a fresh process, which is what test_cpu_load_governance.sh
# actually is.  Driven as a child rather than checked here because this file's
# own profile is now armed and cannot be un-armed without a second setter call
# that would itself be the thing under test.
_G_DEFAULT_DRIVER="$_STUB_ROOT/default-profile.sh"
cat > "$_G_DEFAULT_DRIVER" <<'DRIVEREOF'
#!/bin/bash
set -euo pipefail
# shellcheck source=tests/infra/govtest_slice_reaper_lib.sh
source "$GOVTEST_DRIVER_LIB"
# govtest_profile_set is deliberately NOT called: the profile applied at
# SOURCE time is the whole subject of this drive.
printf 'PID_PARENT=%s\n' "$(govtest_slice_pid 'reify-govtest1285669.slice')"
printf 'PID_AGENTS=%s\n' "$(govtest_slice_pid 'reify-govtest1285669-agents.slice')"
printf 'PID_MERGE=%s\n' "$(govtest_slice_pid 'reify-govtest1285669-merge.slice')"
printf 'PID_OTHER=%s\n' "$(govtest_slice_pid 'reify-test1234.slice')"
printf 'PID_PROD=%s\n' "$(govtest_slice_pid 'reify-governed-agents.slice')"
printf 'UNITS=%s\n' "$(govtest_slice_units 4242 | tr '\n' ',')"
DRIVEREOF
chmod +x "$_G_DEFAULT_DRIVER"

_G_DEFAULT_WANT="PID_PARENT=1285669
PID_AGENTS=1285669
PID_MERGE=1285669
PID_OTHER=
PID_PROD=
UNITS=reify-govtest4242-agents.slice,reify-govtest4242-merge.slice,reify-govtest4242.slice,"

_expect_default_profile_in_child() {
    local got rc=0
    got="$(GOVTEST_DRIVER_LIB="$REAPER_LIB" "$BASH" "$_G_DEFAULT_DRIVER" 2>&1)" || rc=$?
    if [ "$rc" -ne 0 ]; then
        printf 'default-profile driver rc=%s, want 0:\n%s\n' "$rc" "$got"
        return 1
    fi
    if [ "$got" != "$_G_DEFAULT_WANT" ]; then
        printf 'default-profile driver =>\n%s\n--- want ---\n%s\n' "$got" "$_G_DEFAULT_WANT"
        return 1
    fi
    return 0
}
assert "G17: a child shell that sources the library and never sets a profile keeps the reify-govtest default" \
    _expect_default_profile_in_child

# (h) THE PREFIX/PID BOUNDARY. A prefix ending in a digit passes the charset
# test in G10 — `reify-test1` is lowercase alphanumerics in dash-separated
# segments — but it makes two profiles' grammars OVERLAP instead of disjoint:
# reify-test1234.slice reads as pid 1234 under `reify-test` and as pid 234
# under `reify-test1`, because nothing marks where the prefix stops and the pid
# starts. That is not a cosmetic ambiguity. The pid is what the liveness oracle
# consults, so one suite's startup sweep could read the OTHER suite's LIVE
# parent slice as a dead run — its pid, misparsed, belongs to nothing — and
# stop it mid-measurement. G9 asserts the disjoint-namespace property from the
# unit side; this asserts the validation that actually guarantees it.
#
# `reify-govtest9` and the bare `x9` cover the same hazard at the two other
# shapes a prefix can take (multi-segment, and a minimal single segment), so
# the rule reads as "no digit at the end" rather than "not that one string".
_g18() {
    _expect_profile_refused 'reify-test1' agents merge || return 1
    _expect_profile_refused 'reify-govtest9' agents merge || return 1
    _expect_profile_refused 'x9' agents || return 1
    return 0
}
assert "G18: a prefix ending in a DIGIT is REFUSED (the prefix/pid boundary must be unambiguous), profile left intact" \
    _g18

# (i) THE CHILDLESS PROFILE. govtest_slice_pid carries a SECOND branch for a
# profile that declares no children — `^<prefix>([0-9]+)\.slice$`, spelled out
# rather than letting the alternation collapse to `()?`, which is undefined in
# POSIX ERE. Both consumers declare children, so nothing else in the repo
# reaches that branch; it is nonetheless live code inside the single chokepoint
# deciding whether a unit may be STOPPED, and a regression in it (a dropped
# anchor, say) would ship green behind the four-suffix reify-test profile G2-G9
# exercise and the two-suffix default G17 does. So it gets its own coverage
# rather than being left to be discovered by whoever next needs a childless
# profile.
#
# DRIVEN IN A CHILD SHELL, like G17 and for the same reason: this file's
# profile is armed from G1 and Blocks H, I and J all depend on it, so a
# third profile must not be set in this process.
#
# The negatives are chosen to be the ones a BROKEN branch would leak, not a
# generic sample: `reify.slice` (the live production root — the assertion this
# library's whole safety argument turns on), a suffixed child that this profile
# never declared, and the two anchor probes — a prefixed and a suffixed string
# that only `^` and `$` respectively keep out.
_G_CHILDLESS_DRIVER="$_STUB_ROOT/childless-profile.sh"
cat > "$_G_CHILDLESS_DRIVER" <<'DRIVEREOF'
#!/bin/bash
set -euo pipefail
# shellcheck source=tests/infra/govtest_slice_reaper_lib.sh
source "$GOVTEST_DRIVER_LIB"
_rc=0
govtest_profile_set reify-solo || _rc=$?
printf 'SET_RC=%s\n' "$_rc"
printf 'PID_PARENT=%s\n' "$(govtest_slice_pid 'reify-solo55.slice')"
printf 'PID_UNDECLARED_CHILD=%s\n' "$(govtest_slice_pid 'reify-solo55-agents.slice')"
printf 'PID_ROOT=%s\n' "$(govtest_slice_pid 'reify.slice')"
printf 'PID_BARE=%s\n' "$(govtest_slice_pid 'reify-solo.slice')"
printf 'PID_PROD=%s\n' "$(govtest_slice_pid 'reify-governed-agents.slice')"
printf 'PID_LEFT_ANCHOR=%s\n' "$(govtest_slice_pid 'xreify-solo55.slice')"
printf 'PID_RIGHT_ANCHOR=%s\n' "$(govtest_slice_pid 'reify-solo55.slicex')"
printf 'UNITS=%s\n' "$(govtest_slice_units 55 | tr '\n' ',')"
DRIVEREOF
chmod +x "$_G_CHILDLESS_DRIVER"

# UNITS is the round-trip half: with no children declared, a run owns exactly
# one unit and it is the one PID_PARENT just parsed back.
_G_CHILDLESS_WANT="SET_RC=0
PID_PARENT=55
PID_UNDECLARED_CHILD=
PID_ROOT=
PID_BARE=
PID_PROD=
PID_LEFT_ANCHOR=
PID_RIGHT_ANCHOR=
UNITS=reify-solo55.slice,"

_expect_childless_profile_in_child() {
    local got rc=0
    got="$(GOVTEST_DRIVER_LIB="$REAPER_LIB" "$BASH" "$_G_CHILDLESS_DRIVER" 2>&1)" || rc=$?
    if [ "$rc" -ne 0 ]; then
        printf 'childless-profile driver rc=%s, want 0:\n%s\n' "$rc" "$got"
        return 1
    fi
    if [ "$got" != "$_G_CHILDLESS_WANT" ]; then
        printf 'childless-profile driver =>\n%s\n--- want ---\n%s\n' "$got" "$_G_CHILDLESS_WANT"
        return 1
    fi
    return 0
}
assert "G19: a profile declaring NO children parses its bare parent and nothing else" \
    _expect_childless_profile_in_child

# ---------------------------------------------------------------------------
# Block H — govtest_legacy_stale: the PIDLESS dash-nesting parent filter
# (task 6386).
#
# WHAT THESE UNITS ARE.  Before the D7/D8 rename,
# test_cpu_governed_exec_hostexcl.sh named two of its five slices
# `reify-test-task-<pid>.slice` and `reify-test-merge-<pid>.slice`.  systemd
# dash-nesting means each of those implies parents `reify-test.slice` and
# `reify-test-{task,merge}.slice`, vivified automatically and named by nothing
# — so no teardown stopped them and no pid-keyed sweep could ever recognise
# them.  Measured on the host on 2026-08-21, all three were present, `loaded
# active active`, and EMPTY (TasksCurrent=0):
#
#   reify-test-merge.slice  loaded active active Slice /reify/test/merge
#   reify-test-task.slice   loaded active active Slice /reify/test/task
#   reify-test.slice        loaded active active Slice /reify/test
#
# They carry no pid, so govtest_stale_units' liveness oracle has nothing to
# consult and Block G7 correctly keeps them outside the pid grammar.  This is
# the separate, explicitly-listed path they need instead — and the safety
# argument has to be rebuilt from scratch for it, because "the pid is dead" is
# no longer available as the justification for a stop.  What replaces it is
# EMPTINESS, read off a fresh enumeration.
#
# Runs under the reify-test profile armed by Block G1.
# ---------------------------------------------------------------------------
echo ""
echo "--- Block H: govtest_legacy_stale pidless-parent filter ---"

# The three rows measured on the host, in the order systemctl printed them.
_H_HOST_LISTING="reify-test-merge.slice  loaded active active Slice /reify/test/merge
reify-test-task.slice   loaded active active Slice /reify/test/task
reify-test.slice        loaded active active Slice /reify/test"

# The legacy names test_cpu_governed_exec_hostexcl.sh will pass, in teardown
# order: the two dash-children first, their shared root last.
_H_LEGACY_TASK="reify-test-task.slice"
_H_LEGACY_MERGE="reify-test-merge.slice"
_H_LEGACY_ROOT="reify-test.slice"

# _expect_legacy_stale <listing> <want> [legacy...]
#   Exit status is checked as well as output, and not merely as thoroughness:
#   most assertions here expect EMPTY output, and an absent implementation
#   produces empty output too.  Requiring rc=0 is what stops those passing
#   vacuously.
_expect_legacy_stale() {
    local listing="$1" want="$2"
    shift 2
    local got rc=0
    got="$(govtest_legacy_stale "$listing" "$@")" || rc=$?
    if [ "$rc" -ne 0 ]; then
        echo "govtest_legacy_stale returned $rc, want 0"
        return 1
    fi
    if [ "$got" != "$want" ]; then
        printf 'govtest_legacy_stale legacy="%s" =>\n%s\n--- want ---\n%s\n' \
            "$*" "$got" "$want"
        return 1
    fi
    return 0
}

# (a) THE EMPTINESS RULE.  A pidless parent may be stopped only when the fresh
# enumeration shows it has no dash-child left — stopping it otherwise would
# cascade into whatever that child holds.  On the measured host listing that
# means the two leaves go and the root stays, because its two legacy children
# are still listed.
_H_A_WANT="reify-test-task.slice
reify-test-merge.slice"
assert "H1: measured host listing => the two childless leaves, NOT their still-parented root" \
    _expect_legacy_stale "$_H_HOST_LISTING" "$_H_A_WANT" \
        "$_H_LEGACY_TASK" "$_H_LEGACY_MERGE" "$_H_LEGACY_ROOT"

# (b) CONVERGENCE.  H1 + H2 together pin the deliberate TWO-PASS convergence
# to zero: the first post-landing run stops the two leaves, the next run's
# enumeration holds only the root, and that run stops it.  Nothing in the repo
# produces these names any more once the rename lands, so the sweep then stays
# silent forever (H6).  Two passes is acceptable precisely because the
# consuming suite is host-exclusive on the hot path of every run_all.sh — it
# is two verify runs, not two weeks — and it is the same fail-safe DIRECTION
# govtest_stale_units already documents for pid reuse: a false negative costs
# one more sweep, a false positive stops something live.
assert "H2: once the children are gone, the root becomes stoppable (two-pass convergence)" \
    _expect_legacy_stale "$_H_LEGACY_ROOT        loaded active active Slice /reify/test" \
        "$_H_LEGACY_ROOT" \
        "$_H_LEGACY_TASK" "$_H_LEGACY_MERGE" "$_H_LEGACY_ROOT"

# (c) LIVE-DESCENDANT BLOCK.  A concurrent lane still running the PRE-rename
# script would hold reify-test-task-<pid>.slice under reify-test-task.slice.
# Stopping the parent cascades — measured in task 5930 — so that lane's slice
# would be pulled out from under a live governance measurement.  The
# any-dash-child rule blocks it without needing to know it is a pid unit.
_H_C_LISTING="$_H_HOST_LISTING
reify-test-task-999.slice loaded active active Slice /reify/test/task/999"
assert "H3: a live pre-rename descendant suppresses its legacy parent (cascade guard)" \
    _expect_legacy_stale "$_H_C_LISTING" "$_H_LEGACY_MERGE" \
        "$_H_LEGACY_TASK" "$_H_LEGACY_MERGE" "$_H_LEGACY_ROOT"

# (d) PID UNITS DO NOT BLOCK.  Measured on the host: reify-test<pid>.slice
# parents to reify.slice, NOT to reify-test.slice — `reify-test1234` is ONE
# dash segment, not `reify-test` + `1234` — so no cascade from stopping
# reify-test.slice can reach a concurrent lane's own hierarchy.  Without this
# assertion the obvious "starts with the prefix" implementation would look
# correct while making the legacy sweep permanently blocked by any concurrent
# run, i.e. never converging.
_H_D_LISTING="$_H_LEGACY_ROOT        loaded active active Slice /reify/test
reify-test1234.slice        loaded active active Slice /reify/test1234
reify-test1234-agents.slice loaded active active Slice /reify/test1234/agents"
assert "H4: a concurrent run's reify-test<pid>.slice units do NOT block the legacy root" \
    _expect_legacy_stale "$_H_D_LISTING" "$_H_LEGACY_ROOT" "$_H_LEGACY_ROOT"

# (e) THE REFUSALS — the safety core.  Each of these is passed EXPLICITLY as a
# legacy arg AND is present in the listing, so only the prefix re-check can
# keep it out.  reify.slice is the one that matters: it is the shared implicit
# root of BOTH hierarchies, the production reify-governed.slice /
# reify-governed-agents.slice nest under it carrying live orchestrator agent
# placement, and stopping it would cascade into the running fleet.  It is also
# exactly the name a careless "strip the last dash segment and stop the
# parent" design would arrive at from reify-test.slice, which is why it is
# asserted rather than assumed.
_H_E_LISTING="reify.slice                 loaded active active Slice /reify
reify-governed.slice        loaded active active Slice /reify/governed
reify-governed-agents.slice loaded active active Slice /reify/governed/agents
reify-govtest999.slice      loaded active active Slice /reify/govtest999
../reify.slice              loaded active active Slice /reify"
assert "H5: reify.slice, the production slices, the other profile and a traversal string are NEVER emitted" \
    _expect_legacy_stale "$_H_E_LISTING" "" \
        "reify.slice" "reify-governed.slice" "reify-governed-agents.slice" \
        "reify-govtest999.slice" "../reify.slice"

# (f) A name INSIDE the pid grammar is refused here.  Those belong to
# govtest_reap_stale, which consults the liveness oracle before stopping
# anything; routing one through this function would stop it on emptiness
# alone and bypass that check entirely.
assert "H6: a pid-grammar name passed as a legacy arg is refused (that is reap_stale's job)" \
    _expect_legacy_stale "reify-test1234.slice loaded active active Slice /reify/test1234" "" \
        "reify-test1234.slice"

# (g) The quiet cases.  H7 is the STEADY STATE this task converges to and the
# state every run must be in afterwards, so anything emitted here would mean
# the sweep reaps on every single run forever.
assert "H7: a legacy name absent from the listing is not emitted (converged steady state)" \
    _expect_legacy_stale "reify-test1234.slice loaded active active Slice /reify/test1234" "" \
        "$_H_LEGACY_TASK" "$_H_LEGACY_MERGE" "$_H_LEGACY_ROOT"
assert "H8: empty listing => no output, exit 0" \
    _expect_legacy_stale "" "" "$_H_LEGACY_TASK" "$_H_LEGACY_ROOT"
assert "H9: whitespace-and-blank-lines-only listing => no output, exit 0" \
    _expect_legacy_stale "$(printf '\n   \n\t\n\n')" "" "$_H_LEGACY_TASK" "$_H_LEGACY_ROOT"
assert "H10: no legacy args at all => no output, exit 0" \
    _expect_legacy_stale "$_H_HOST_LISTING" ""

# (h) Emission follows ARGUMENT order, not listing order — the caller controls
# teardown order and the two disagree on the measured host listing, where
# systemctl printed merge before task.  Dedup keeps a doubly-passed name from
# producing two stops.
_H_H_WANT="reify-test-merge.slice
reify-test-task.slice"
assert "H11: emission is in ARGUMENT order and deduplicated when a name is passed twice" \
    _expect_legacy_stale "$_H_HOST_LISTING" "$_H_H_WANT" \
        "$_H_LEGACY_MERGE" "$_H_LEGACY_TASK" "$_H_LEGACY_MERGE"

# (i) PIDLESS ONLY, STRUCTURALLY. H6 covers a name inside the PID GRAMMAR
# (reify-test1234.slice); this covers the other pid-bearing shape — the
# PRE-rename reify-test-task-1234.slice, which the grammar cannot recognise
# (Block G7) and which the namespace filter alone WOULD admit, since `task`
# and `1234` are both [a-z0-9]+. It is presented here in the worst possible
# light for the filter: passed explicitly as an arg, present in the listing,
# and with no dash-child of its own, so emptiness is the only thing standing
# behind a stop — and emptiness is no evidence at all about whether pid 1234 is
# still running. Stopping it would pull a live pre-rename lane's slice out from
# under its measurement while bypassing the liveness oracle govtest_reap_stale
# applies to exactly this kind of name. Its pidless parents remain emittable in
# the same call, so the guard is proven to be per-argument rather than a
# whole-call abort.
_H_I_LISTING="reify-test-task-1234.slice  loaded active active Slice /reify/test/task/1234
reify-test-merge.slice      loaded active active Slice /reify/test/merge"
assert "H12: a pid-bearing PRE-rename name is never emitted, even present and childless" \
    _expect_legacy_stale "$_H_I_LISTING" "$_H_LEGACY_MERGE" \
        "reify-test-task-1234.slice" "$_H_LEGACY_MERGE"

# ---------------------------------------------------------------------------
# Block I — govtest_reap_legacy: the ACTUATOR for Block H's filter.
#
# Reuses Block D's stubbed-systemctl harness verbatim — the same bin-ok /
# bin-fail / bin-none directories under $_STUB_ROOT, the same absolute-"$BASH"
# child-shell driver, the same _reap_stopped_units log reader — so nothing here
# contacts the real systemd user session or stops a real unit.
#
# The driver arms the reify-test profile itself rather than inheriting it,
# because it is a separate PROCESS: this is what the consuming suite will do
# too, so the drive exercises the real ordering (source, set profile, sweep).
# It runs under `set -euo pipefail`, the discipline that suite runs under, so a
# non-zero status escaping the sweep is caught here rather than in production
# as a host-exclusive gate aborted before a single row ran.
# ---------------------------------------------------------------------------
echo ""
echo "--- Block I: govtest_reap_legacy actuator (stubbed systemctl) ---"

_LEGACY_DRIVER="$_STUB_ROOT/legacy-driver.sh"
_LEGACY_OUT="$_STUB_ROOT/legacy.out"
_LEGACY_RC=0

cat > "$_LEGACY_DRIVER" <<'DRIVEREOF'
#!/bin/bash
set -euo pipefail
# shellcheck source=tests/infra/govtest_slice_reaper_lib.sh
source "$GOVTEST_DRIVER_LIB"
govtest_profile_set reify-test agents merge taskweight mergeweight
_rc=0
govtest_reap_legacy "$@" || _rc=$?
exit "$_rc"
DRIVEREOF
chmod +x "$_LEGACY_DRIVER"

# _stub_reap_legacy <bindir> <listing> [legacy...]
#   Truncate log + stdout capture, seed the listing fixture, run the driver.
#   Sets _LEGACY_RC.
_stub_reap_legacy() {
    local bindir="$1" listing="$2"
    shift 2
    : > "$_REAP_LOG"
    : > "$_LEGACY_OUT"
    printf '%s\n' "$listing" > "$_REAP_LISTING"
    _LEGACY_RC=0
    GOVTEST_STUB_LOG="$_REAP_LOG" \
    GOVTEST_STUB_LISTING_FILE="$_REAP_LISTING" \
    GOVTEST_DRIVER_LIB="$REAPER_LIB" \
    PATH="$bindir" \
    "$BASH" "$_LEGACY_DRIVER" "$@" >"$_LEGACY_OUT" 2>"$_REAP_STDERR" || _LEGACY_RC=$?
    return 0
}

# _expect_legacy_stops <bindir> <listing> <want> [legacy...]
_expect_legacy_stops() {
    local bindir="$1" listing="$2" want="$3"
    shift 3
    _stub_reap_legacy "$bindir" "$listing" "$@"
    if [ "$_LEGACY_RC" -ne 0 ]; then
        echo "govtest_reap_legacy rc=$_LEGACY_RC, want 0"
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

# The measured host state (2026-08-21) and the three names the consuming suite
# will pass, in teardown order.
_I_HOST_LISTING="reify-test-merge.slice  loaded active active Slice /reify/test/merge
reify-test-task.slice   loaded active active Slice /reify/test/task
reify-test.slice        loaded active active Slice /reify/test"
_I_WANT="reify-test-task.slice
reify-test-merge.slice"

# (a) — the first post-landing run: the two childless leaves are stopped, in
# argument order, and their still-parented root is NOT.
assert "I1: measured host listing => exactly the two leaves stopped, root left for the next pass" \
    _expect_legacy_stops "$_STUB_ROOT/bin-ok" "$_I_HOST_LISTING" "$_I_WANT" \
        "reify-test-task.slice" "reify-test-merge.slice" "reify-test.slice"

# (b) — GLOB-SCOPED ENUMERATION. The per-user systemd session is shared
# host-wide, so a sweep that listed every unit and filtered afterwards would
# have a blast radius bounded only by the name grammar. The glob is the OUTER
# of the two belt-and-braces bounds; govtest_legacy_stale's prefix re-check is
# the inner. Same verify.py:3492/3503 pairing task 5930 mirrors.
_expect_legacy_glob_scoped() {
    _stub_reap_legacy "$_STUB_ROOT/bin-ok" "$_I_HOST_LISTING" \
        "reify-test-task.slice" "reify-test-merge.slice" "reify-test.slice"
    if ! grep -qF -- 'list-units --all --plain --no-legend reify-test*.slice' "$_REAP_LOG"; then
        echo "no glob-scoped list-units invocation in stub log:"
        cat "$_REAP_LOG" 2>/dev/null || true
        return 1
    fi
    # ...and no OTHER list-units invocation of any shape.
    if grep -- 'list-units' "$_REAP_LOG" \
        | grep -qvF -- 'list-units --all --plain --no-legend reify-test*.slice'; then
        echo "stub log contains an UNSCOPED (or differently scoped) list-units invocation:"
        cat "$_REAP_LOG" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "I2: enumeration is glob-scoped to reify-test*.slice, and nothing else is ever listed" \
    _expect_legacy_glob_scoped

# (c) — THE LOOP ITSELF. The loop's stdin IS the heredoc carrying the remaining
# units, so a stdin-consuming callee would swallow them and truncate the sweep
# to its first unit; the library detaches stdin on the stop for exactly that
# reason, and this is what would notice if that regressed. Three stoppable
# leaves rather than I1's two, so no first-plus-last accident can pass it.
_I_LOOP_LISTING="reify-test-task.slice   loaded active active Slice /reify/test/task
reify-test-merge.slice  loaded active active Slice /reify/test/merge
reify-test-extra.slice  loaded active active Slice /reify/test/extra"
_I_LOOP_WANT="reify-test-task.slice
reify-test-merge.slice
reify-test-extra.slice"
assert "I3: THREE emitted units => three stops (the loop is not truncated by a stdin-consuming stop)" \
    _expect_legacy_stops "$_STUB_ROOT/bin-ok" "$_I_LOOP_LISTING" "$_I_LOOP_WANT" \
        "reify-test-task.slice" "reify-test-merge.slice" "reify-test-extra.slice"

# (d) — each reap is announced on stdout, the same log-what-you-reaped
# discipline govtest_reap_stale follows, so a sweep is visible in the
# host-exclusive suite's transcript instead of happening silently.
_expect_legacy_announced() {
    _stub_reap_legacy "$_STUB_ROOT/bin-ok" "$_I_HOST_LISTING" \
        "reify-test-task.slice" "reify-test-merge.slice" "reify-test.slice"
    local unit
    for unit in reify-test-task.slice reify-test-merge.slice; do
        if ! grep -qF -- "$unit" "$_LEGACY_OUT"; then
            printf 'stopped %s but never announced it on stdout:\n' "$unit"
            cat "$_LEGACY_OUT" 2>/dev/null || true
            return 1
        fi
    done
    # The root was not stopped, so it must not be announced either.
    if grep -qE 'reaped legacy slice: reify-test\.slice$' "$_LEGACY_OUT"; then
        printf 'announced reify-test.slice, which was never stopped:\n'
        cat "$_LEGACY_OUT" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "I4: every unit reaped is announced on stdout, and nothing else is" \
    _expect_legacy_announced

# (e) — THE STEADY STATE. Once this task has converged, no legacy name exists
# and the listing holds only live per-run units. Anything stopped here would
# mean the sweep reaps on EVERY run, forever.
_I_STEADY_LISTING="reify-test1234.slice         loaded active active Slice /reify/test1234
reify-test1234-agents.slice  loaded active active Slice /reify/test1234/agents
reify-test1234-taskweight.slice loaded active active Slice /reify/test1234/taskweight"
_expect_legacy_steady_state() {
    _expect_legacy_stops "$_STUB_ROOT/bin-ok" "$_I_STEADY_LISTING" "" \
        "reify-test-task.slice" "reify-test-merge.slice" "reify-test.slice" || return 1
    # Enumerated, though — the sweep still runs, it just finds nothing.
    if ! grep -qF -- 'list-units' "$_REAP_LOG"; then
        echo "the sweep did not even enumerate:"
        cat "$_REAP_LOG" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "I5: converged steady state — enumerated, but nothing stopped" \
    _expect_legacy_steady_state

# (f) — FAIL-SOFT, both directions. This runs at the TOP of a suite under
# `set -euo pipefail`, so an escaping non-zero status would abort a
# host-exclusive gate before a single row ran and read as a governance
# regression that isn't one.
_expect_legacy_noop_without_systemctl() {
    _stub_reap_legacy "$_STUB_ROOT/bin-none" "$_I_HOST_LISTING" \
        "reify-test-task.slice" "reify-test-merge.slice" "reify-test.slice"
    if [ "$_LEGACY_RC" -ne 0 ]; then
        echo "rc=$_LEGACY_RC, want 0"
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
assert "I6: systemctl absent from PATH => exit 0, nothing attempted" \
    _expect_legacy_noop_without_systemctl

_expect_legacy_survives_failing_systemctl() {
    _stub_reap_legacy "$_STUB_ROOT/bin-fail" "$_I_HOST_LISTING" \
        "reify-test-task.slice" "reify-test-merge.slice" "reify-test.slice"
    if [ "$_LEGACY_RC" -ne 0 ]; then
        echo "rc=$_LEGACY_RC, want 0 (a failing systemctl must be swallowed)"
        cat "$_REAP_STDERR" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "I7: systemctl failing every call => still exit 0 (fail-soft)" \
    _expect_legacy_survives_failing_systemctl

# (g) — the assertion that stands between a bug in this actuator and killing
# live orchestrator agent placement. reify.slice is the shared implicit root
# of BOTH hierarchies; reify-governed-agents.slice carries real agent
# placement. Both are present in the listing AND passed explicitly as legacy
# args here, so only govtest_legacy_stale's prefix re-check keeps them out.
_I_PROD_LISTING="reify.slice                 loaded active active Slice /reify
reify-governed.slice        loaded active active Slice /reify/governed
reify-governed-agents.slice loaded active active Slice /reify/governed/agents
$_I_HOST_LISTING"
assert "I8: reify.slice and reify-governed-agents.slice are never stopped, even when passed as legacy args" \
    _expect_legacy_stops "$_STUB_ROOT/bin-ok" "$_I_PROD_LISTING" "$_I_WANT" \
        "reify.slice" "reify-governed.slice" "reify-governed-agents.slice" \
        "reify-test-task.slice" "reify-test-merge.slice" "reify-test.slice"

# (h) — THE RACE BETWEEN THE ENUMERATION AND THE STOP. Block H's filter decides
# "no dash-child left to cascade into" from a SNAPSHOT. A concurrent lane still
# running the PRE-rename script can create reify-test-task-<pid>.slice in the
# window between that enumeration and the stop, and stopping
# reify-test-task.slice then cascades into that lane's LIVE measurement —
# exactly the hazard the childlessness rule exists to prevent, arrived at by
# timing instead of by a bad filter. Not hypothetical: when the rename landed,
# dozens of lanes still had the old file checked out. The library re-validates
# each unit against a FRESH enumeration immediately before its own stop, which
# is what this drives.
#
# THE STUB IS DELIBERATELY RACY. It answers the FIRST list-units from the
# fixture alone and every one after that from the fixture PLUS the new
# dash-child, so the appearance lands precisely in the window under test.
# Builtin-only and /bin/bash-shebanged for the same PATH-hygiene reasons
# bin-ok documents.
#
# THE SIBLING IS THE OTHER HALF. A re-check that simply aborted the whole sweep
# on any change would also pass a task-only assertion, so reify-test-merge
# .slice — childless in both enumerations — must still be stopped. The
# re-check is per-unit, not per-sweep.
_I_RACY_BIN="$_STUB_ROOT/bin-racy"
_I_RACE_FLAG="$_STUB_ROOT/race.flag"
_I_RACE_ROW="reify-test-task-999.slice loaded active active Slice /reify/test/task/999"
mkdir -p "$_I_RACY_BIN"
cat > "$_I_RACY_BIN/systemctl" <<'STUBEOF'
#!/bin/bash
printf '%s\n' "$*" >> "$GOVTEST_STUB_LOG"
for _a in "$@"; do
    if [ "$_a" = "list-units" ]; then
        if [ -s "${GOVTEST_STUB_LISTING_FILE:-/nonexistent}" ]; then
            while IFS= read -r _l; do printf '%s\n' "$_l"; done \
                < "$GOVTEST_STUB_LISTING_FILE"
        fi
        if [ -e "${GOVTEST_STUB_RACE_FLAG:-/nonexistent}" ]; then
            printf '%s\n' "${GOVTEST_STUB_RACE_ROW:-}"
        else
            printf '' > "${GOVTEST_STUB_RACE_FLAG:-/dev/null}"
        fi
        exit 0
    fi
done
exit 0
STUBEOF
chmod +x "$_I_RACY_BIN/systemctl"

# Two childless leaves at enumeration time; only one of them grows a child.
_I_RACE_LISTING="reify-test-task.slice   loaded active active Slice /reify/test/task
reify-test-merge.slice  loaded active active Slice /reify/test/merge"

_expect_legacy_race_skip() {
    local rc=0 got
    : > "$_REAP_LOG"
    : > "$_LEGACY_OUT"
    rm -f "$_I_RACE_FLAG"
    printf '%s\n' "$_I_RACE_LISTING" > "$_REAP_LISTING"
    GOVTEST_STUB_LOG="$_REAP_LOG" \
    GOVTEST_STUB_LISTING_FILE="$_REAP_LISTING" \
    GOVTEST_STUB_RACE_FLAG="$_I_RACE_FLAG" \
    GOVTEST_STUB_RACE_ROW="$_I_RACE_ROW" \
    GOVTEST_DRIVER_LIB="$REAPER_LIB" \
    PATH="$_I_RACY_BIN" \
    "$BASH" "$_LEGACY_DRIVER" "reify-test-task.slice" "reify-test-merge.slice" \
        >"$_LEGACY_OUT" 2>"$_REAP_STDERR" || rc=$?
    if [ "$rc" -ne 0 ]; then
        echo "govtest_reap_legacy rc=$rc, want 0"
        cat "$_REAP_STDERR" 2>/dev/null || true
        return 1
    fi
    got="$(_reap_stopped_units)"
    if [ "$got" != "reify-test-merge.slice" ]; then
        printf 'stopped:\n%s\n--- want exactly ---\nreify-test-merge.slice\n--- full stub log ---\n' "$got"
        cat "$_REAP_LOG" 2>/dev/null || true
        return 1
    fi
    # The skip is announced, not silent — same log-what-you-did discipline the
    # reap follows, and the only way a transcript can show WHY a legacy name
    # survived a sweep.
    if ! grep -qE 'skipped legacy slice[^:]*: reify-test-task\.slice$' "$_LEGACY_OUT"; then
        printf 'reify-test-task.slice was skipped but never announced on stdout:\n'
        cat "$_LEGACY_OUT" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "I9: a dash-child appearing AFTER the enumeration suppresses that unit's stop, not its sibling's" \
    _expect_legacy_race_skip

# ---------------------------------------------------------------------------
# Block J — WIRING. Prove the library is actually used by
# tests/infra/test_cpu_governed_exec_hostexcl.sh, and that its D7/D8 slices
# were renamed into the $$-scoped hierarchy (task 6386).
#
# Follows Block F's precedent exactly: DRIVE that script as a child process.
# No source-grepping — that is the documentation-meta-test shape this repo's
# TDD rules prohibit, and it could distinguish neither a call placed before the
# EXIT trap from one placed after it, nor a live call from one stranded in a
# dead branch.
#
# COST, and why the seam is what makes this affordable from the pool. The real
# D block places FIVE systemd-run --user scopes on a governance-capable host,
# and the stub `systemctl` cannot intercept those — the wrapper shells out to
# `systemd-run`, a different binary. So without
# REIFY_CPU_GOVEXEC_TEST_LIFECYCLE_ONLY the drive would create real units and
# pay real placement time. REIFY_CPU_GOVERN_DISABLE=1 rides along as a second,
# independent backstop: should the seam ever regress, host_supports_governance
# returns false and the child SKIPs D rather than placing anything, so this
# block degrades to a fast no-op instead of littering the host.
#
# PATH here PREPENDS the stub rather than replacing PATH (as Block D does),
# because the script legitimately needs mktemp and friends before it reaches
# the sweep. Prepending is enough: the stub shadows the real systemctl, so no
# real unit is ever touched.
#
# NOTE ON THE FIXTURE. The stub answers EVERY `list-units` from the same file
# regardless of the glob it was handed, which is deliberate here: it means
# reify.slice and reify-governed-agents.slice reach the filters even though the
# real enumeration glob would have excluded them, so J6 exercises the INNER of
# the two belt-and-braces bounds rather than resting on the outer one.
# ---------------------------------------------------------------------------
echo ""
echo "--- Block J: library wired into test_cpu_governed_exec_hostexcl.sh ---"

_J_STALE_PID=111
_J_LOG="$_STUB_ROOT/wire-j.log"
_J_OUT="$_STUB_ROOT/wire-j.out"
_J_LISTING_FILE="$_STUB_ROOT/wire-j-listing.txt"
_J_RC=0
_J_CHILD_PID=""

# A dead predecessor's residue, the three measured legacy pidless parents
# (host, 2026-08-21), and the two names that must never be touched.
_J_LISTING="reify-test${_J_STALE_PID}-agents.slice loaded active active Slice /reify/test${_J_STALE_PID}/agents
reify-test${_J_STALE_PID}-merge.slice  loaded active active Slice /reify/test${_J_STALE_PID}/merge
reify-test${_J_STALE_PID}.slice        loaded active active Slice /reify/test${_J_STALE_PID}
reify-test-merge.slice                 loaded active active Slice /reify/test/merge
reify-test-task.slice                  loaded active active Slice /reify/test/task
reify-test.slice                       loaded active active Slice /reify/test
reify.slice                            loaded active active Slice /reify
reify-governed-agents.slice            loaded active active Slice /reify/governed/agents"

# Drive the real script ONCE; every assertion below reads the captured log.
printf '%s\n' "$_J_LISTING" > "$_J_LISTING_FILE"
: > "$_J_LOG"
timeout 60 env \
    PATH="$_STUB_ROOT/bin-ok:$PATH" \
    GOVTEST_STUB_LOG="$_J_LOG" \
    GOVTEST_STUB_LISTING_FILE="$_J_LISTING_FILE" \
    REIFY_GOVTEST_TEST_MODE=1 \
    REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS="$_ALIVE_NONE" \
    REIFY_CPU_GOVEXEC_TEST_LIFECYCLE_ONLY=1 \
    REIFY_CPU_GOVERN_DISABLE=1 \
    bash "$SCRIPT_DIR/test_cpu_governed_exec_hostexcl.sh" \
    > "$_J_OUT" 2>&1 || _J_RC=$?

# Every unit name the stub was asked to stop, one per line. Flattened to WORDS
# because a stop may legitimately carry several units in one invocation, and
# because the point of J4 is to count what was torn down without first
# assuming it parses.
_j_stopped_words() {
    sed -n 's/^--user stop //p' "$_J_LOG" 2>/dev/null | tr ' ' '\n' | sed '/^$/d'
}

_j_word_present() {
    _j_stopped_words | grep -qxF -- "$1"
}

_j_exit0() {
    if [ "$_J_RC" -ne 0 ]; then
        echo "child test_cpu_governed_exec_hostexcl.sh rc=$_J_RC (124 = timeout), want 0"
        tail -n 30 "$_J_OUT" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "J1: driven test_cpu_governed_exec_hostexcl.sh exits 0 under the armed lifecycle-only seam" \
    _j_exit0

# J2 — the startup sweep is wired, and collapses the dead predecessor's whole
# residue to ONE stop of the PARENT. Stopping a parent cascades (measured in
# task 5930), so a sweep that also stopped the children would be issuing
# redundant stops plus an ordering hazard.
_j_stale_reaped() {
    local got
    got="$(_j_stopped_words | grep -E "^reify-test${_J_STALE_PID}" || true)"
    if [ "$got" != "reify-test${_J_STALE_PID}.slice" ]; then
        printf 'stops touching pid %s were:\n%s\n--- want exactly ---\nreify-test%s.slice\n--- full stub log ---\n' \
            "$_J_STALE_PID" "$got" "$_J_STALE_PID"
        cat "$_J_LOG" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "J2: the startup sweep is wired — the canned dead predecessor's PARENT slice was stopped, once" \
    _j_stale_reaped

# J3 — teardown is unconditional and wired. The child created NOTHING (it exits
# at the seam), so these stops can only come from an unconditional teardown.
# The child's pid is RECOVERED from the log rather than assumed, since $$
# inside the child differs from this script's own pid — same discipline as F3.
_J_LEGACY_NAMES=" reify-test-task.slice reify-test-merge.slice reify-test.slice reify-test${_J_STALE_PID}.slice "

# Everything the child tore down for ITS OWN run: the stop log minus the names
# we already know belong to the sweeps. Derived WITHOUT consulting the grammar,
# which is what lets J4 be a real assertion about the rename rather than a
# tautology over whatever happens to parse.
_j_child_own_units() {
    local unit
    while IFS= read -r unit; do
        case "$_J_LEGACY_NAMES" in
            *" $unit "*) continue ;;
        esac
        printf '%s\n' "$unit"
    done < <(_j_stopped_words)
}

_j_own_parent_stopped() {
    local unit pid pids="" n=0
    while IFS= read -r unit; do
        pid="$(govtest_slice_pid "$unit")"
        [ -n "$pid" ] || continue
        case " $pids " in
            *" $pid "*) ;;
            *) pids="$pids $pid"; n=$((n + 1)) ;;
        esac
    done < <(_j_child_own_units)

    if [ "$n" -ne 1 ]; then
        printf 'expected exactly ONE own-run pid in the stop log, found %s (%s). Own-run stops were:\n' "$n" "$pids"
        _j_child_own_units
        return 1
    fi
    _J_CHILD_PID="${pids# }"
    if ! _j_word_present "reify-test${_J_CHILD_PID}.slice"; then
        printf 'child pid %s appears in the log but its PARENT slice was never stopped:\n' "$_J_CHILD_PID"
        cat "$_J_LOG" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "J3: teardown is unconditional — child stopped its OWN parent slice having created nothing" \
    _j_own_parent_stopped

# J4 — THE RENAME, end-to-end. RED today for a STRUCTURAL reason, not a
# cosmetic one: the pre-rename script tears down reify-test-task-<pid>.slice
# and reify-test-merge-<pid>.slice, which do NOT parse under this grammar
# precisely because they carry the extra dash segment that vivifies the
# implicit parents Block H exists to reap. Requiring all five own-run stops to
# parse to the child's pid is therefore the same assertion as "every unit this
# run creates has at most one dash segment after the pid".
#
# PAIRWISE DISTINCTNESS guards a suffix collision. cgroup_set_slice_weight sets
# the SLICE's own CPUWeight (scripts/lib_cgroup.sh), so D5's
# REIFY_CPU_GOVERN_W_TASK=250 leaves the D1 task slice at 250; if D7 reused
# that same slice its SLICE_WEIGHT==100 assertion would go red. The five must
# stay five distinct units, merely re-parented.
_j_rename_five_distinct() {
    local unit pid seen=" " n=0
    if [ -z "$_J_CHILD_PID" ]; then
        echo "J3 did not recover a child pid — cannot evaluate the rename"
        return 1
    fi
    while IFS= read -r unit; do
        pid="$(govtest_slice_pid "$unit")"
        if [ "$pid" != "$_J_CHILD_PID" ]; then
            printf "own-run stop '%s' does not parse to the child pid %s under the reify-test grammar (got '%s')\n" \
                "$unit" "$_J_CHILD_PID" "$pid"
            return 1
        fi
        case "$seen" in
            *" $unit "*)
                printf "own-run stop '%s' issued more than once — the five slices are not distinct\n" "$unit"
                return 1
                ;;
        esac
        seen="$seen$unit "
        n=$((n + 1))
    done < <(_j_child_own_units)

    if [ "$n" -ne 5 ]; then
        printf 'expected exactly 5 own-run stops, got %s:\n' "$n"
        _j_child_own_units
        return 1
    fi
    return 0
}
assert "J4: THE RENAME — all five own-run slices parse to the child pid and are pairwise distinct" \
    _j_rename_five_distinct

# J5 — the legacy sweep is wired, with its emptiness rule intact: the two
# childless leaves go, their still-parented root does not.
_j_legacy_reaped() {
    local unit
    for unit in reify-test-task.slice reify-test-merge.slice; do
        if ! _j_word_present "$unit"; then
            printf 'the legacy sweep never stopped %s:\n' "$unit"
            cat "$_J_LOG" 2>/dev/null || true
            return 1
        fi
    done
    if _j_word_present "reify-test.slice"; then
        printf 'stopped reify-test.slice while its legacy children were still listed (cascade hazard):\n'
        cat "$_J_LOG" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "J5: the legacy sweep is wired — the two leaves stopped, their still-parented root left alone" \
    _j_legacy_reaped

# J6 — the assertion that stands between a bug anywhere in this wiring and
# killing live orchestrator agent placement. Both names are in the listing the
# child's sweeps actually saw (see the fixture note above), so only the
# grammar and prefix re-checks keep them out.
_j_production_untouched() {
    local unit
    for unit in reify.slice reify-governed-agents.slice; do
        if _j_word_present "$unit"; then
            printf 'STOPPED %s — this would cascade into live orchestrator agent placement:\n' "$unit"
            cat "$_J_LOG" 2>/dev/null || true
            return 1
        fi
    done
    return 0
}
assert "J6: reify.slice and reify-governed-agents.slice were never stopped by any code path in the drive" \
    _j_production_untouched

# J7 — pins the seam itself, and with it this block's pool cost. A full run
# ends by printing test_helpers.sh's "Results: N passed, M failed"; the
# lifecycle-only path exits before the D block, so that line and every D label
# must be ABSENT. Asserted on output rather than elapsed time, which would be
# flaky under concurrent pool load.
_j_exited_at_seam() {
    local pat
    for pat in '^Results:' '^--- D: governed cgroup placement' 'D1a:' 'D3a:' 'D8:' 'SKIP D:'; do
        if grep -qE -- "$pat" "$_J_OUT"; then
            printf 'child ran past the seam — matched %s:\n' "$pat"
            tail -n 20 "$_J_OUT" 2>/dev/null || true
            return 1
        fi
    done
    return 0
}
assert "J7: the lifecycle-only seam short-circuits before the D block (keeps this a pool-cheap drive)" \
    _j_exited_at_seam

# --- J8: the lifecycle-only seam must be ARMING-GATED ---------------------
#
# J1-J7 pass REIFY_GOVTEST_TEST_MODE=1. J8 is the counterweight: this seam is
# the one knob in that file able to exit 0 having run ZERO placement rows, and
# run_all.sh judges a member by EXIT CODE alone — it parses no "Results:" line
# — so a single stray export would silently turn a host-exclusive gate into a
# no-op that still reports success. A comment saying "never set this" is not a
# guard; this asserts the code is.
#
# Two things are required, and the second is what makes it non-vacuous: the
# refusal must be LOUD (a silent ignore would be its own trap for whoever set
# the var deliberately) and the suite must actually CONTINUE. "Continued" is
# proven POSITIVELY, by the first post-seam output line, never by absence.
#
# Unlike F5 this drive is allowed to run to COMPLETION rather than being killed
# once the evidence appears: REIFY_CPU_GOVERN_DISABLE=1 makes
# host_supports_governance false, so past the seam the child SKIPs D, places
# nothing, and finishes immediately. That removes F5's kill race entirely
# while keeping the same positive evidence.
_J_DISARM_OUT="$_STUB_ROOT/wire-j-disarm.out"
_J_DISARM_LOG="$_STUB_ROOT/wire-j-disarm.log"
_J_DISARM_RC=0

_j_seam_refused_without_arming() {
    : > "$_J_DISARM_OUT"
    : > "$_J_DISARM_LOG"
    _J_DISARM_RC=0
    # NOTE: no REIFY_GOVTEST_TEST_MODE here — that omission IS the test.
    timeout 120 env \
        PATH="$_STUB_ROOT/bin-ok:$PATH" \
        GOVTEST_STUB_LOG="$_J_DISARM_LOG" \
        GOVTEST_STUB_LISTING_FILE="$_J_LISTING_FILE" \
        REIFY_CPU_GOVEXEC_TEST_LIFECYCLE_ONLY=1 \
        REIFY_CPU_GOVERN_DISABLE=1 \
        bash "$SCRIPT_DIR/test_cpu_governed_exec_hostexcl.sh" \
        > "$_J_DISARM_OUT" 2>&1 || _J_DISARM_RC=$?

    if [ "$_J_DISARM_RC" -ne 0 ]; then
        echo "unarmed drive rc=$_J_DISARM_RC (124 = timeout), want 0"
        tail -n 20 "$_J_DISARM_OUT" 2>/dev/null || true
        return 1
    fi
    if ! grep -q 'LIFECYCLE_ONLY=1 IGNORED' "$_J_DISARM_OUT" 2>/dev/null; then
        echo "unarmed LIFECYCLE_ONLY was not refused loudly:"
        head -n 20 "$_J_DISARM_OUT" 2>/dev/null || true
        return 1
    fi
    if ! grep -q -- '--- D: governed cgroup placement' "$_J_DISARM_OUT" 2>/dev/null; then
        echo "unarmed LIFECYCLE_ONLY short-circuited anyway — the suite never reached the D block:"
        head -n 20 "$_J_DISARM_OUT" 2>/dev/null || true
        return 1
    fi
    return 0
}
assert "J8: LIFECYCLE_ONLY without REIFY_GOVTEST_TEST_MODE is refused loudly and the suite runs on" \
    _j_seam_refused_without_arming

# ---------------------------------------------------------------------------
# Block K — MAP-WIRING. Pin scripts/verify-pipeline-infra-tests.txt's routing
# for tests/infra/govtest_slice_reaper_lib.sh (task 6427).
#
# That map routes a changed verify-pipeline artifact to the infra-test
# glob(s) that guard it (consumed by verify.sh's select_infra_tests()); this
# library had no row until task 6427, so a task-scope (--scope branch) verify
# touching only the lib selected zero infra tests. K1/K2 pin the routing so a
# future edit to the map or the classification manifest cannot silently drop
# or misroute it again.
#
# K1 (POSITIVE) — the map routes the lib to THIS file. Mirrors
# select_infra_tests()'s own parse exactly (same active-row filter, same
# two-field `read`, same glob expansion under $REPO_ROOT) — same idiom as
# tests/infra/test_verify_pipeline_guard.sh:599-627 and
# tests/infra/test_target_per_lane_independence.sh:74,293.
#
# K2 (NEGATIVE) — the map never routes the lib to a target classified
# host-exclusive in run-all-classification.manifest (bucket looked up via
# run-all-classification-lib.sh's classification_bucket accessor, the single
# parse implementation for that file, rather than re-parsing it here).
#
# Scoped to THIS artifact's own rows only — NOT a repo-wide "no map row
# targets host-exclusive" invariant. That broader claim is false today
# independent of this task: map lines 39/40/45/50 already route
# provision-warm-lane-fs.sh / seed-warm-lane.sh / refresh-warm-base.sh /
# warm-lane-preflight.sh to test_warm_lane_pool.sh, host-exclusive at
# manifest:71, so asserting the broad form would be a doomed RED no
# implementer could turn green. That precedent is cheap by construction
# (two-layer arm-with-a-knob: the always-run half is hermetic, the real
# end-to-end half is substrate-gated and skips gracefully absent
# REIFY_WARM_LANE_MOUNT / REIFY_RUN_WARM_LANE_GATE) — opposite polarity from
# test_cpu_load_governance.sh, which burns real CPU BY DEFAULT and is cheap
# ONLY under REIFY_GOVTEST_TEST_MODE, a key nothing in production sets. K2
# pins that this lib's map rows never reach that expensive-by-default test —
# a deliberate omission, not missing coverage: the lib's wiring into that
# consumer is already proven hermetically and sub-second by Block F above
# (~line 714), which K1's own row already selects.
# ---------------------------------------------------------------------------
echo ""
echo "--- Block K: verify-pipeline-infra-tests.txt routing ---"

VP_INFRA_MAP="$REPO_ROOT/scripts/verify-pipeline-infra-tests.txt"
_SELF_TEST_PATH="$SCRIPT_DIR/test_govtest_slice_reaper.sh"

# shellcheck source=tests/infra/run-all-classification-lib.sh
source "$SCRIPT_DIR/run-all-classification-lib.sh"

# _map_targets_for <artifact-path> — mirror select_infra_tests()'s parse
# exactly (same active-row filter, same two-field `read`), then print every
# matching row's glob, expanded under $REPO_ROOT, one EXISTING REGULAR FILE
# per line. `-f` (not `-e`) deliberately mirrors verify.sh's own selective-
# infra emitter loop (`[ -f "$_vt" ] || continue`, scripts/verify.sh:2910):
# a glob expansion that resolves to a directory or other non-regular path
# must be excluded here exactly as it would be at runtime, or this helper
# could report a routing that select_infra_tests() would not actually drive.
_map_targets_for() {
    local _want="$1" _artifact _glob _line _expanded
    [ -f "$VP_INFRA_MAP" ] || return 0
    while IFS= read -r _line; do
        read -r _artifact _glob <<< "$_line"
        [ -n "$_artifact" ] || continue
        [ -n "$_glob" ]     || continue
        [ "$_artifact" = "$_want" ] || continue
        for _expanded in "$REPO_ROOT"/$_glob; do
            [ -f "$_expanded" ] && printf '%s\n' "$_expanded"
        done
    done < <(grep -v '^\s*#' "$VP_INFRA_MAP" | grep -v '^\s*$')
    return 0
}

# _map_selects_this_test <artifact-path> — success if _map_targets_for's
# expansion for <artifact-path> contains THIS test file.
_map_selects_this_test() {
    local _want="$1" _t
    while IFS= read -r _t; do
        [ "$_t" = "$_SELF_TEST_PATH" ] && return 0
    done < <(_map_targets_for "$_want")
    return 1
}

# _map_never_targets_bucket <artifact-path> <bucket> — success if NONE of
# <artifact-path>'s mapped targets are classified <bucket> in
# run-all-classification.manifest.
#
# Self-anchored: classification_bucket() returns rc 0 with EMPTY output both
# when the manifest is absent/renamed (`[ -f "$_manifest" ] || return 0` in
# run-all-classification-lib.sh) and when <bucket> is a typo'd token that
# matches no manifest row — either way `_members` would be empty and the
# scan loop below could never match, making the assertion pass vacuously
# instead of catching the regression it exists to catch. Fail loudly instead
# of silently in both cases: an empty bucket lookup means this check's own
# input has disappeared, not that the artifact is clean.
_map_never_targets_bucket() {
    local _want="$1" _bucket="$2" _t _base _members
    _members="$(classification_bucket "$_bucket")"
    if [ -z "$_members" ]; then
        echo "bucket '$_bucket' resolved to no members — manifest missing/renamed or bucket token typo'd; cannot assert a negative against an empty set"
        return 1
    fi
    while IFS= read -r _t; do
        [ -n "$_t" ] || continue
        _base="$(basename "$_t")"
        if printf '%s\n' "$_members" | grep -qx -- "$_base"; then
            echo "artifact $_want routes to $_base, classified $_bucket"
            return 1
        fi
    done < <(_map_targets_for "$_want")
    return 0
}

assert "K1: verify-pipeline-infra-tests.txt maps tests/infra/govtest_slice_reaper_lib.sh -> this test" \
    _map_selects_this_test tests/infra/govtest_slice_reaper_lib.sh

assert "K2: verify-pipeline-infra-tests.txt never routes tests/infra/govtest_slice_reaper_lib.sh to a host-exclusive test" \
    _map_never_targets_bucket tests/infra/govtest_slice_reaper_lib.sh host-exclusive

test_summary
