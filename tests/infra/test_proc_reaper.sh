#!/usr/bin/env bash
# Unit tests for scripts/lib_proc_reaper.sh:
#   reaper_kill_pgroup  — TERM->grace->KILL escalation for a process group
#   reaper_run_in_pgroup / reaper_teardown — run-in-pgroup + tracked teardown
#   reap-orphans subcommand  — host-wide orphan scan
# And for verify.sh wiring and the end-to-end scripts/reap-orphaned-test-binaries.sh.
#
# Auto-discovered by tests/infra/run_all.sh.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
LIB_REAPER="$REPO_ROOT/scripts/lib_proc_reaper.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

[ -f "$SCRIPT_DIR/load_tolerance_lib.sh" ] || { echo "ERROR: load_tolerance_lib.sh not found at $SCRIPT_DIR/load_tolerance_lib.sh"; exit 1; }
source "$SCRIPT_DIR/load_tolerance_lib.sh"

# Preserve a keepalive fd (task 4931 / esc-3002-145 FIX #2): assert() runs
# "$@" >/dev/null 2>&1 (test_helpers.sh), which would swallow a keepalive
# emitted from inside an assert-wrapped poll loop's own stderr. fd 3 is
# duplicated from the ORIGINAL stderr here, before any assert() call, and is
# inherited by every subsequent child/subshell in this file (export + fd
# inheritance) — assert()'s 1>/2> redirection does not touch fd 3, so
# keepalives emitted there escape to the real stream.
exec 3>&2
export REIFY_KEEPALIVE_FD=3

# Export the keepalive helpers (+ the clock_emit_heartbeat grammar they
# reuse when reachable) so hermetic `env ... bash -c` subshells below can
# call them without re-sourcing this file.
export -f load_tolerant_keepalive load_tolerant_sleep_tick
declare -F _reify_keepalive_write >/dev/null 2>&1 && export -f _reify_keepalive_write
declare -F clock_emit_heartbeat >/dev/null 2>&1 && export -f clock_emit_heartbeat

# Per-instance sentinels — different per-$$ to avoid cross-instance collisions
# under concurrent verify (same idiom as test_portable_timeout.sh:69-70).
_SENT_KILL=$(($$ * 10 + 3))    # used in reaper_kill_pgroup behavioral test
_SENT_PGROUP=$(($$ * 10 + 5))  # used in reaper_run_in_pgroup / teardown tests
_SENT_FAKE=$(($$ * 10 + 7))    # used in reap-orphans fixture / e2e test

# Self-expiring sentinel duration (task 4931 / esc-3002-145 FIX #1).
# Every sentinel process launched below runs for this many seconds instead of
# an unbounded/huge duration, so a fixture that outlives this test's own
# teardown (e.g. a SIGKILL of the whole test tree, whose TERM/INT trap never
# runs) self-expires quickly instead of surviving for hundreds of days (a raw
# per-$$ sentinel value used directly as a sleep duration, e.g.
# `sleep 41441945`, is ~479 days for a real PID).
# Must be > ~240s, the max load-scaled teardown-poll window
# (_POLL_ATTEMPTS = load_tolerant_attempts(30) = 30 x CAP 8) so a genuine
# teardown FAILURE is still detected non-vacuously (the sentinel must still be
# alive at poll exhaustion). Must be << 7200s (REIFY_REAPER_MIN_AGE_SECS
# default) so a SIGKILL orphan self-reaps long before the host-wide reaper's
# sweep-age threshold would ever consider it.
_SENTINEL_SLEEP_SECS="${_SENTINEL_SLEEP_SECS:-600}"

# Load-scaled poll budgets; computed before any stripped-PATH subshells.
_POLL_ATTEMPTS=$(load_tolerant_attempts 30)   # reaper_kill_pgroup poll budget
_POLL_ATTEMPTS_5=$(load_tolerant_attempts 5)  # (legacy; kept for any future callers)
_POLL_ATTEMPTS_ORPHAN=$(load_tolerant_attempts 20)  # post-SIGKILL orphan-reap budget

# ---------------------------------------------------------------------------
# Zombie-aware "effectively gone" helpers.
# Treat ps -o s= state '' (reaped/never-existed) OR 'Z'/'Z+' (zombie) as
# effectively gone.  After `kill -9`, a process is a zombie ('Z') until its
# parent calls wait(); the naive `ps -o pid= | grep -q .` reports it PRESENT,
# causing false-alive timeouts under load.  These helpers close that race.
#
# export -f makes the functions available inside the `env ... bash -c '...'`
# assertion subshells (env preserves BASH_FUNC_* exports; non-interactive bash
# imports them at startup).  PATH is intact at every call site so bare ps/sleep
# resolve normally.
# ---------------------------------------------------------------------------
_pid_effectively_gone() {
    local _s
    _s=$(ps -o s= -p "$1" 2>/dev/null | tr -d ' ' || echo "")
    [ -z "$_s" ] || case "$_s" in Z*) true ;; *) false ;; esac
}

_poll_pid_gone() {
    local _pid="$1" _n="$2" _t
    for ((_t=1; _t<=_n; _t++)); do
        _pid_effectively_gone "$_pid" && return 0
        # Keepalive-bearing tick (task 4931 / esc-3002-145 FIX #2): a bare
        # `sleep 1` here can run this loop up to ~240s under load
        # (_POLL_ATTEMPTS = load_tolerant_attempts(30)) with zero interim
        # output, which would otherwise trip dark-factory's clock-stop
        # heartbeat-idle backstop (180s) into a false 'wedged' kill of a
        # genuinely-alive poll. load_tolerant_sleep_tick emits a throttled
        # @@REIFY_CLOCK_HEARTBEAT@@ to the preserved keepalive fd.
        load_tolerant_sleep_tick
    done
    return 1
}

_orphan_ppid_settled() {
    local _parent="$1" _child="$2" _n="$3"
    # Wait for parent to be effectively gone (reparenting completes at parent
    # exit, i.e. as soon as the parent is a zombie).  || true: even on timeout
    # we still sample so the caller gets whatever PPID is current.
    _poll_pid_gone "$_parent" "$_n" || true
    ps -o ppid= -p "$_child" 2>/dev/null | tr -d ' ' || echo ""
}

# _poll_survivor_alive <pid> <n>
#   Condition-polled proof that <pid> is still alive (task 5148): retries up
#   to <n> times via the zombie-aware _pid_effectively_gone (tolerating a
#   transient-empty ps read instead of misreporting it as dead), ticking
#   load_tolerant_sleep_tick between samples. Returns 0 (alive) on the first
#   not-gone sample, 1 only once the whole budget is exhausted — still
#   non-vacuous: a genuinely-dead pid exhausts the poll and returns 1.
#   Shared by the Part 5 end-to-end survivor check and its Test 5b hermetic
#   regression lock so BOTH exercise this ONE code path: a reversion to a
#   single-shot ps sample anywhere in here flips both assertions, rather than
#   a lock that only guards a duplicated copy of the loop (reviewer
#   follow-up on the original Test 5b, which inlined its own copy).
_poll_survivor_alive() {
    local _pid="$1" _n="$2" _t
    for ((_t=1; _t<=_n; _t++)); do
        if ! _pid_effectively_gone "$_pid"; then
            return 0
        fi
        load_tolerant_sleep_tick
    done
    return 1
}

# ---------------------------------------------------------------------------
# _spawn_sentinel_pgroup <marker_bin> <count>
#   Background <count> copies of "<marker_bin>" "${_SENTINEL_SLEEP_SECS}" and
#   wait for them — the shared launcher for the "N self-expiring sentinels in
#   one process group" shape used by both Test 3b (SIGTERM teardown) and the
#   Test 3d SIGKILL self-reap regression (task 4931 / esc-3002-145 FIX #1).
#   Intended to run inside reaper_run_in_pgroup's `eval "$_cmd" &` context, so
#   its own PID (captured by the caller via $!) becomes the process-group
#   leader and each spawned sentinel is a child sharing that pgroup —
#   identical topology to the inline "sleep <dur> & sleep <dur>; wait" idiom
#   it replaces.
#   When $_SPAWN_SENTINEL_PIDFILE is set (non-empty), appends each spawned
#   sentinel's PID as its own line — lets a SIGKILL harness (which cannot
#   rely on a TERM/INT trap) recover the sentinel PIDs for non-vacuity /
#   self-reap assertions after the harness parent is gone.
#   export -f'd so it is visible inside the `env ... bash -c` / nested
#   `bash -c "..."` harness contexts: exported bash functions propagate
#   transitively through nested bash -c invocations (verified: a function
#   export -f'd in this process is callable inside `bash -c 'bash -c "..."'`).
# ---------------------------------------------------------------------------
_spawn_sentinel_pgroup() {
    local _marker_bin="$1" _count="$2" _i _spid
    for ((_i = 1; _i <= _count; _i++)); do
        "$_marker_bin" "${_SENTINEL_SLEEP_SECS:-600}" &
        _spid=$!
        if [ -n "${_SPAWN_SENTINEL_PIDFILE:-}" ]; then
            echo "$_spid" >> "$_SPAWN_SENTINEL_PIDFILE"
        fi
    done
    wait
}

export -f _pid_effectively_gone _poll_pid_gone _orphan_ppid_settled _poll_survivor_alive _spawn_sentinel_pgroup

echo "=== lib_proc_reaper.sh unit tests ==="

# Temp dir registry — cleaned by trap.
_TMPDIRS=()
_cleanup_dirs() {
    for _d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$_d" 2>/dev/null || true; done
}
trap '_cleanup_dirs' EXIT

# ===========================================================================
# Part 1 — reaper_kill_pgroup
# ===========================================================================

echo ""
echo "--- Part 1: reaper_kill_pgroup ---"

# -- Test 1a: structural: lib exists --
assert "lib_proc_reaper.sh exists" \
    test -f "$LIB_REAPER"

# -- Test 1b: structural: kill_pgroup uses PID-reuse-safe process-group form --
# reaper_kill_pgroup MUST use 'kill ... -- -<pgid>' form (not individual-PID kill).
# Mirrors test_portable_timeout.sh Test 17.
assert "reaper_kill_pgroup uses process-group kill form (kill -- -)" \
    bash -c '[ -f "$1" ] && grep -qE "kill[[:space:]].*--[[:space:]]+-" "$1"' _ "$LIB_REAPER"

# Uniquely-named copied-sleep marker binary for Test 1c (task 4931 /
# esc-3002-145 FIX #1): the marker's file NAME is the unique grep marker
# (embeds the per-$$ sentinel), decoupled from its runtime DURATION
# (_SENTINEL_SLEEP_SECS) — mirrors the Part 2/5 copied-binary idiom so a
# sentinel that somehow outlives this test's own teardown self-expires in
# _SENTINEL_SLEEP_SECS instead of surviving for however long a raw
# `sleep <huge-per-$$-value>` would run.
_P1_KILL_DIR="$(mktemp -d)"
_TMPDIRS+=("$_P1_KILL_DIR")
_KILL_MARKER_BIN="$_P1_KILL_DIR/reify_reaper_sentinel_kill_${_SENT_KILL}"
cp "$(command -v sleep)" "$_KILL_MARKER_BIN"
chmod +x "$_KILL_MARKER_BIN"

# -- Test 1c: behavioral: kill_pgroup kills leader AND child --
# Explicitly guards against vacuous pass when lib doesn't exist ([ -f guard ]).
assert "reaper_kill_pgroup kills the process-group leader and its child" \
    env LIB_REAPER="$LIB_REAPER" _KILL_MARKER_BIN="$_KILL_MARKER_BIN" \
        _SENTINEL_SLEEP_SECS="$_SENTINEL_SLEEP_SECS" _POLL_ATTEMPTS="$_POLL_ATTEMPTS" \
    bash -c '
        [ -f "$LIB_REAPER" ] || exit 1
        [ -x "$_KILL_MARKER_BIN" ] || exit 1
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_grep=$(command -v grep)
        _abs_kill=$(command -v kill)
        _abs_awk=$(command -v awk)
        _marker_name=$(basename "$_KILL_MARKER_BIN")
        # Anchor on "<name><space><digits><EOL>" — matches only a genuine
        # sentinel CHILD process (whose entire argv is exactly "<path> <secs>"),
        # never the polling grep'\''s own argv (marker name followed by regex
        # metacharacters, not a real space+digit) nor the leader bash'\''s argv
        # (whose line does not end right after the digits).
        _pattern="${_marker_name}[[:space:]][0-9]+\$"

        # Pre-clean any stale sentinel marker processes from prior runs.
        "$_abs_ps" -A -o pid,args 2>/dev/null \
            | "$_abs_grep" -E "$_pattern" \
            | "$_abs_awk" "{print \$1}" \
            | while read -r _pid; do "$_abs_kill" -9 "$_pid" 2>/dev/null || true; done
        "$_abs_sleep" 0.3

        source "$LIB_REAPER"

        # Launch a process group: leader bash spawns two sentinel children.
        # Under set -m, the backgrounded command is the group leader (PGID == $!).
        REIFY_REAPER_GRACE_SECS=0
        set -m 2>/dev/null || true
        bash -c "\"$_KILL_MARKER_BIN\" \"$_SENTINEL_SLEEP_SECS\" & \"$_KILL_MARKER_BIN\" \"$_SENTINEL_SLEEP_SECS\"; wait" &
        _pgid=$!
        set +m 2>/dev/null || true

        # Brief pause to let children start.
        "$_abs_sleep" 0.3

        # NON-VACUITY: confirm at least one sentinel is actually alive BEFORE
        # calling reaper_kill_pgroup. Without this guard, a fixture-spawn
        # failure (marker binary fails to exec, or the children have not yet
        # forked under load) would make the pattern match nothing both before
        # and after the kill, so the post-kill poll _found=0 would masquerade
        # as a PASS without ever having exercised the kill path — the same
        # vacuous-pass class this task set out to eliminate (mirrors Test 3d
        # non-vacuity guard). Bounded fixed wait (not load-scaled): forking a
        # copied-sleep binary is near-instant even under load.
        # NOTE: no apostrophes in this comment block — see rationale above.
        _pre_found=0
        for ((_t=1; _t<=20; _t++)); do
            if "$_abs_ps" -A -o pid,args 2>/dev/null \
                | "$_abs_grep" -qE "$_pattern"; then
                _pre_found=1
                break
            fi
            "$_abs_sleep" 0.2
        done
        if [ "$_pre_found" -ne 1 ]; then
            echo "FAIL: no sentinel process appeared before kill (vacuous fixture)" >&2
            exit 1
        fi

        reaper_kill_pgroup "$_pgid"

        # Poll until all sentinel marker processes are gone.
        # Keepalive-bearing tick (task 4931 / esc-3002-145 FIX #2) — see the
        # _poll_pid_gone comment above for rationale (this loop shares the
        # same load-scaled _POLL_ATTEMPTS budget, up to ~240s under load).
        # NOTE: no apostrophes in this comment block — it lives inside a
        # single-quoted bash -c '...' string, where a literal quote char
        # would prematurely close the string and break the outer parse.
        _found=0
        for ((_t=1; _t<=_POLL_ATTEMPTS; _t++)); do
            _found=0
            if "$_abs_ps" -A -o pid,args 2>/dev/null \
                | "$_abs_grep" -qE "$_pattern"; then
                _found=1
            fi
            [ "$_found" -eq 0 ] && break
            load_tolerant_sleep_tick
        done
        exit "$_found"
    '

# -- Test 1d: reaper_kill_pgroup on a stale/nonexistent PGID returns 0 (ESRCH-safe) --
assert "reaper_kill_pgroup with nonexistent PGID returns 0 (ESRCH-safe)" \
    env LIB_REAPER="$LIB_REAPER" bash -c '
        [ -f "$LIB_REAPER" ] || exit 1
        source "$LIB_REAPER"
        reaper_kill_pgroup 999999999
    '

# ===========================================================================
# Part 2 — reap-orphans subcommand
# ===========================================================================

echo ""
echo "--- Part 2: reap-orphans subcommand ---"

# Build a hermetic fixture: tmp/target/debug/deps/reify_faketest_<SENT>
_FIXTURE_DIR="$(mktemp -d)"
_TMPDIRS+=("$_FIXTURE_DIR")
_DEPS_DIR="$_FIXTURE_DIR/target/debug/deps"
mkdir -p "$_DEPS_DIR"
_FAKE_BIN="$_DEPS_DIR/reify_faketest_${_SENT_FAKE}"
cp "$(command -v sleep)" "$_FAKE_BIN"
chmod +x "$_FAKE_BIN"

# -- Test 2a: POSITIVE — binary under deps glob, matching PPID, killed --
# Uses simplified PPID-matching: set ORPHAN_PPIDS to the fake binary's actual PPID.
# No reparenting needed — the reaper filter works on any configured PPID.
assert "reap-orphans kills a binary under the deps glob whose PPID is in ORPHAN_PPIDS" \
    env LIB_REAPER="$LIB_REAPER" _FAKE_BIN="$_FAKE_BIN" _SENT_FAKE="$_SENT_FAKE" \
        _FIXTURE_DIR="$_FIXTURE_DIR" _SENTINEL_SLEEP_SECS="$_SENTINEL_SLEEP_SECS" \
        _POLL_ATTEMPTS_ORPHAN="$_POLL_ATTEMPTS_ORPHAN" bash -c '
        [ -f "$LIB_REAPER" ] || exit 1
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_grep=$(command -v grep)
        _abs_kill=$(command -v kill)
        _abs_awk=$(command -v awk)

        # Pre-clean stale instances.
        "$_abs_ps" -A -o pid,args 2>/dev/null \
            | "$_abs_grep" -E "reify_faketest_${_SENT_FAKE}" \
            | "$_abs_awk" "{print \$1}" \
            | while read -r _p; do "$_abs_kill" -9 "$_p" 2>/dev/null || true; done
        "$_abs_sleep" 0.3

        # Launch fake binary as a background child.
        "$_FAKE_BIN" "$_SENTINEL_SLEEP_SECS" </dev/null >/dev/null 2>&1 &
        _fake_pid=$!
        "$_abs_sleep" 0.2

        # Get the fake binary'\''s actual PPID (= current bash -c PID).
        # Part 2a deliberately keeps its exact single-PPID match: the launcher
        # (this bash -c) never exits, so the fake binary'\''s PPID is stable and
        # no reparenting can occur.  This preserves the selectivity that the
        # negative tests 2b-2e are specifically asserting.
        _fake_ppid=$("$_abs_ps" -o ppid= -p "$_fake_pid" 2>/dev/null | tr -d " " || echo "")
        [ -n "$_fake_ppid" ] || { echo "FAIL: could not read PPID of fake binary" >&2; exit 1; }

        # Run reap-orphans with ORPHAN_PPIDS = fake binary'\''s actual PPID.
        REIFY_REAPER_DEPS_GLOB="${_FIXTURE_DIR}/target/debug/deps/*" \
        REIFY_REAPER_MIN_AGE_SECS=0 \
        REIFY_REAPER_ORPHAN_PPIDS="$_fake_ppid" \
        REIFY_REAPER_COMMS="" \
        REIFY_REAPER_UID=$(id -u) \
            bash "$LIB_REAPER" reap-orphans >/dev/null 2>&1 || true

        # Poll until the fake binary is gone (zombie-aware; bumped budget base 20).
        _poll_pid_gone "$_fake_pid" "$_POLL_ATTEMPTS_ORPHAN"; exit $?
    '

# -- Test 2b: NEGATIVE n1 — PPID NOT in orphan set → process SPARED --
assert "reap-orphans spares a matching binary whose PPID is NOT in ORPHAN_PPIDS" \
    env LIB_REAPER="$LIB_REAPER" _FAKE_BIN="$_FAKE_BIN" _SENT_FAKE="$_SENT_FAKE" \
        _FIXTURE_DIR="$_FIXTURE_DIR" _SENTINEL_SLEEP_SECS="$_SENTINEL_SLEEP_SECS" bash -c '
        [ -f "$LIB_REAPER" ] || exit 1
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_kill=$(command -v kill)
        _abs_grep=$(command -v grep)

        "$_FAKE_BIN" "$_SENTINEL_SLEEP_SECS" </dev/null >/dev/null 2>&1 &
        _live_pid=$!
        "$_abs_sleep" 0.2

        # Run reap-orphans with ORPHAN_PPIDS that does NOT include the fake'\''s PPID.
        REIFY_REAPER_DEPS_GLOB="${_FIXTURE_DIR}/target/debug/deps/*" \
        REIFY_REAPER_MIN_AGE_SECS=0 \
        REIFY_REAPER_ORPHAN_PPIDS="999999998 999999999" \
        REIFY_REAPER_COMMS="nonexistent_init_comm_zzz" \
        REIFY_REAPER_UID=$(id -u) \
            bash "$LIB_REAPER" reap-orphans >/dev/null 2>&1 || true

        # Assert the live process is still alive.
        _alive=0
        "$_abs_ps" -o pid= -p "$_live_pid" 2>/dev/null | "$_abs_grep" -q . && _alive=1 || true
        "$_abs_kill" -9 "$_live_pid" 2>/dev/null || true
        wait "$_live_pid" 2>/dev/null || true
        exit $((1 - _alive))
    '

# -- Test 2c: NEGATIVE n2 — younger than MIN_AGE → SPARED --
assert "reap-orphans spares a binary under the deps glob younger than MIN_AGE_SECS" \
    env LIB_REAPER="$LIB_REAPER" _FAKE_BIN="$_FAKE_BIN" _SENT_FAKE="$_SENT_FAKE" \
        _FIXTURE_DIR="$_FIXTURE_DIR" _SENTINEL_SLEEP_SECS="$_SENTINEL_SLEEP_SECS" bash -c '
        [ -f "$LIB_REAPER" ] || exit 1
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_kill=$(command -v kill)
        _abs_grep=$(command -v grep)

        "$_FAKE_BIN" "$_SENTINEL_SLEEP_SECS" </dev/null >/dev/null 2>&1 &
        _pid=$!
        "$_abs_sleep" 0.2

        _ppid=$("$_abs_ps" -o ppid= -p "$_pid" 2>/dev/null | tr -d " " || echo "")

        REIFY_REAPER_DEPS_GLOB="${_FIXTURE_DIR}/target/debug/deps/*" \
        REIFY_REAPER_MIN_AGE_SECS=9999 \
        REIFY_REAPER_ORPHAN_PPIDS="$_ppid" \
        REIFY_REAPER_COMMS="" \
        REIFY_REAPER_UID=$(id -u) \
            bash "$LIB_REAPER" reap-orphans >/dev/null 2>&1 || true

        _alive=0
        "$_abs_ps" -o pid= -p "$_pid" 2>/dev/null | "$_abs_grep" -q . && _alive=1 || true
        "$_abs_kill" -9 "$_pid" 2>/dev/null || true
        wait "$_pid" 2>/dev/null || true
        exit $((1 - _alive))
    '

# -- Test 2d: NEGATIVE n3 — not under deps glob → SPARED --
assert "reap-orphans spares a binary NOT under the configured deps glob" \
    env LIB_REAPER="$LIB_REAPER" _SENTINEL_SLEEP_SECS="$_SENTINEL_SLEEP_SECS" bash -c '
        [ -f "$LIB_REAPER" ] || exit 1
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_kill=$(command -v kill)
        _abs_grep=$(command -v grep)

        # Launch a plain sleep (not under any deps dir) as background child.
        # Self-expiring duration (task 4931 / esc-3002-145 FIX #1): this
        # fixture is never grep-matched by sentinel value (PID is captured
        # directly via $!), so no unique-marker binary is needed here — only
        # the duration needs to be bounded so a SIGKILL of this whole test
        # process (before the "$_abs_kill" -9 "$_pid" cleanup below runs)
        # self-expires instead of leaking a bare (non-deps-glob-matchable)
        # sleep for an unbounded duration.
        "$_abs_sleep" "$_SENTINEL_SLEEP_SECS" &
        _pid=$!
        "$_abs_sleep" 0.2

        _ppid=$("$_abs_ps" -o ppid= -p "$_pid" 2>/dev/null | tr -d " " || echo "")

        REIFY_REAPER_DEPS_GLOB="/nonexistent/target/debug/deps/*" \
        REIFY_REAPER_MIN_AGE_SECS=0 \
        REIFY_REAPER_ORPHAN_PPIDS="$_ppid" \
        REIFY_REAPER_COMMS="" \
        REIFY_REAPER_UID=$(id -u) \
            bash "$LIB_REAPER" reap-orphans >/dev/null 2>&1 || true

        _alive=0
        "$_abs_ps" -o pid= -p "$_pid" 2>/dev/null | "$_abs_grep" -q . && _alive=1 || true
        "$_abs_kill" -9 "$_pid" 2>/dev/null || true
        wait "$_pid" 2>/dev/null || true
        exit $((1 - _alive))
    '

# -- Test 2e: NEGATIVE n4 — --dry-run reports candidate but does NOT kill --
assert "reap-orphans --dry-run reports candidate but does not kill it" \
    env LIB_REAPER="$LIB_REAPER" _FAKE_BIN="$_FAKE_BIN" _SENT_FAKE="$_SENT_FAKE" \
        _FIXTURE_DIR="$_FIXTURE_DIR" _SENTINEL_SLEEP_SECS="$_SENTINEL_SLEEP_SECS" bash -c '
        [ -f "$LIB_REAPER" ] || exit 1
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_kill=$(command -v kill)
        _abs_grep=$(command -v grep)

        "$_FAKE_BIN" "$_SENTINEL_SLEEP_SECS" </dev/null >/dev/null 2>&1 &
        _pid=$!
        "$_abs_sleep" 0.2

        _ppid=$("$_abs_ps" -o ppid= -p "$_pid" 2>/dev/null | tr -d " " || echo "")

        REIFY_REAPER_DEPS_GLOB="${_FIXTURE_DIR}/target/debug/deps/*" \
        REIFY_REAPER_MIN_AGE_SECS=0 \
        REIFY_REAPER_ORPHAN_PPIDS="$_ppid" \
        REIFY_REAPER_COMMS="" \
        REIFY_REAPER_UID=$(id -u) \
            bash "$LIB_REAPER" reap-orphans --dry-run >/dev/null 2>&1 || true

        # The candidate must still be alive.
        _alive=0
        "$_abs_ps" -o pid= -p "$_pid" 2>/dev/null | "$_abs_grep" -q . && _alive=1 || true
        "$_abs_kill" -9 "$_pid" 2>/dev/null || true
        wait "$_pid" 2>/dev/null || true
        exit $((1 - _alive))
    '

# ===========================================================================
# Part 3 — reaper_run_in_pgroup + reaper_teardown
# ===========================================================================

echo ""
echo "--- Part 3: reaper_run_in_pgroup / reaper_teardown ---"

# -- Test 3a: normal completion — exits with correct code, teardown is a no-op --
assert "reaper_run_in_pgroup 'exit 42' returns exit code 42" \
    env LIB_REAPER="$LIB_REAPER" bash -c '
        [ -f "$LIB_REAPER" ] || exit 1
        source "$LIB_REAPER"
        reaper_run_in_pgroup "exit 42" && exit 1 || [ $? -eq 42 ]
    '

assert "reaper_teardown after completed pass is a no-op (exits 0)" \
    env LIB_REAPER="$LIB_REAPER" bash -c '
        [ -f "$LIB_REAPER" ] || exit 1
        source "$LIB_REAPER"
        reaper_run_in_pgroup "exit 0"
        reaper_teardown
        reaper_teardown
    '

# Uniquely-named copied-sleep marker binary for Test 3b (task 4931 /
# esc-3002-145 FIX #1) — see Test 1c comment above for rationale.
_P3_PGROUP_DIR="$(mktemp -d)"
_TMPDIRS+=("$_P3_PGROUP_DIR")
_PGROUP_MARKER_BIN="$_P3_PGROUP_DIR/reify_reaper_sentinel_pgroup_${_SENT_PGROUP}"
cp "$(command -v sleep)" "$_PGROUP_MARKER_BIN"
chmod +x "$_PGROUP_MARKER_BIN"

# -- Test 3b: teardown-on-signal tears down the whole process group --
# A harness subshell sources the lib, runs a long-running pass in background
# so we can SIGTERM the harness mid-pass.
# Assert: after SIGTERM, both sentinel processes inside the pass are reaped.
# NOTE: the harness bash -c uses "..." quoting; trap body uses escaped inner quotes.
assert "reaper_teardown on SIGTERM kills the entire in-flight process group" \
    env LIB_REAPER="$LIB_REAPER" _PGROUP_MARKER_BIN="$_PGROUP_MARKER_BIN" \
        _SENTINEL_SLEEP_SECS="$_SENTINEL_SLEEP_SECS" _POLL_ATTEMPTS="$_POLL_ATTEMPTS" \
    bash -c '
        [ -f "$LIB_REAPER" ] || exit 1
        [ -x "$_PGROUP_MARKER_BIN" ] || exit 1
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_grep=$(command -v grep)
        _abs_kill=$(command -v kill)
        _abs_awk=$(command -v awk)
        _abs_bash=$(command -v bash)
        _marker_name=$(basename "$_PGROUP_MARKER_BIN")
        # Anchor on "<name><space><digits><EOL>" — see Test 1c'\''s comment for
        # why this specifically avoids self-matching the polling grep'\''s own
        # argv and the leader bash'\''s argv.
        _pattern="${_marker_name}[[:space:]][0-9]+\$"

        # Pre-clean stale sentinel marker processes.
        "$_abs_ps" -A -o pid,args 2>/dev/null \
            | "$_abs_grep" -E "$_pattern" \
            | "$_abs_awk" "{print \$1}" \
            | while read -r _p; do "$_abs_kill" -9 "$_p" 2>/dev/null || true; done
        "$_abs_sleep" 0.3

        # Harness subshell: installs teardown trap (using double-quotes for trap body
        # since we are inside a double-quoted bash -c string), starts a long-running pass
        # via the shared _spawn_sentinel_pgroup launcher (exported via export -f at top-level;
        # exported bash functions propagate through nested bash -c invocations).
        # reaper_run_in_pgroup is called WITHOUT a trailing ampersand: backgrounding
        # the call itself would fork a subshell to run it, and _REIFY_REAP_PGIDS is
        # a plain array whose append inside that subshell never propagates back to
        # this harness, so the TERM/INT trap (which runs reaper_teardown HERE)
        # would see an empty array and tear down nothing. reaper_run_in_pgroup
        # already does its own internal eval-plus-wait for the pass, so calling it
        # directly (no outer background) blocks this harness in place with the
        # pgid tracked in the SAME shell that owns the trap.
        "$_abs_bash" -c "
            source \"$LIB_REAPER\"
            export _SENTINEL_SLEEP_SECS=\"$_SENTINEL_SLEEP_SECS\"
            REIFY_REAPER_GRACE_SECS=0
            trap \"reaper_teardown; exit 143\" TERM INT
            reaper_run_in_pgroup \"_spawn_sentinel_pgroup '\''$_PGROUP_MARKER_BIN'\'' 2\"
        " &
        _harness_pid=$!

        # Let the pass start.
        "$_abs_sleep" 0.8

        # NON-VACUITY: confirm at least one sentinel is actually alive BEFORE
        # SIGTERM-ing the harness. Without this guard, a fixture-spawn
        # failure (e.g. the two _spawn_sentinel_pgroup children have not yet
        # forked within this window under load) would make the pattern match
        # nothing both before and after teardown, so the post-teardown poll
        # _found=0 would masquerade as a PASS without ever having exercised
        # the teardown-on-signal path — the same vacuous-pass class this task
        # set out to eliminate (mirrors Test 3d non-vacuity guard). Bounded
        # fixed wait (not load-scaled): forking sentinels is near-instant
        # even under load.
        # NOTE: no apostrophes in this comment block — see rationale above.
        _pre_found=0
        for ((_t=1; _t<=20; _t++)); do
            if "$_abs_ps" -A -o pid,args 2>/dev/null \
                | "$_abs_grep" -qE "$_pattern"; then
                _pre_found=1
                break
            fi
            "$_abs_sleep" 0.2
        done
        if [ "$_pre_found" -ne 1 ]; then
            echo "FAIL: no sentinel process appeared before SIGTERM (vacuous fixture)" >&2
            exit 1
        fi

        # SIGTERM the harness.
        "$_abs_kill" -TERM "$_harness_pid" 2>/dev/null || true
        wait "$_harness_pid" 2>/dev/null || true

        # Poll until all sentinel marker processes are gone.
        # Keepalive-bearing tick (task 4931 / esc-3002-145 FIX #2) — see the
        # _poll_pid_gone comment above for rationale (this loop shares the
        # same load-scaled _POLL_ATTEMPTS budget, up to ~240s under load).
        # NOTE: no apostrophes in this comment block — it lives inside a
        # single-quoted bash -c '...' string, where a literal quote char
        # would prematurely close the string and break the outer parse.
        _found=0
        for ((_t=1; _t<=_POLL_ATTEMPTS; _t++)); do
            _found=0
            if "$_abs_ps" -A -o pid,args 2>/dev/null \
                | "$_abs_grep" -qE "$_pattern"; then
                _found=1
            fi
            [ "$_found" -eq 0 ] && break
            load_tolerant_sleep_tick
        done
        exit "$_found"
    '

# -- Test 3c: reaper_teardown is idempotent (callable twice, no error) --
assert "reaper_teardown is idempotent (two calls, no error)" \
    env LIB_REAPER="$LIB_REAPER" bash -c '
        [ -f "$LIB_REAPER" ] || exit 1
        source "$LIB_REAPER"
        reaper_teardown
        reaper_teardown
    '

# -- Test 3d: SIGKILL-orphan leak-proofing (task 4931 / esc-3002-145 FIX #1) --
# A REAL heartbeat-idle-watchdog kill is SIGKILL, not SIGTERM: uncatchable, so
# the harness's own "reaper_teardown; exit 143" TERM/INT trap NEVER runs (this
# is exactly what Test 3b's SIGTERM path cannot exercise). Before self-expiring
# sentinels (step-4), a Test-3b-shaped fixture used the huge per-$$ sentinel
# value as BOTH the grep marker and the sleep duration — a SIGKILL orphan
# would sleep for that many seconds (hundreds of days for a real PID),
# unreclaimable by the host-wide reaper (/usr/bin/sleep is not under any
# target/*/deps glob it matches on). This regression proves a SIGKILL-orphaned
# sentinel self-expires within a short bounded window instead.
# RED today: `_spawn_sentinel_pgroup` is undefined (added in step-4) — the
# harness's inner reaper_run_in_pgroup call fails immediately, so no sentinel
# PIDs ever land in the pidfile and the non-vacuity assertion fails.
_SENT_KILL3D=$(($$ * 10 + 9))
_KILL3D_DIR="$(mktemp -d)"
_TMPDIRS+=("$_KILL3D_DIR")
_KILL3D_MARKER_BIN="$_KILL3D_DIR/reify_reaper_sentinel_sigkill_regr_${_SENT_KILL3D}"
cp "$(command -v sleep)" "$_KILL3D_MARKER_BIN"
chmod +x "$_KILL3D_MARKER_BIN"

assert "SIGKILL of the harness (uncatchable — trap never runs) still leaves sentinels that self-reap within the bounded window" \
    env LIB_REAPER="$LIB_REAPER" _KILL3D_MARKER_BIN="$_KILL3D_MARKER_BIN" \
        _SENTINEL_SLEEP_SECS=5 _POLL_ATTEMPTS="$_POLL_ATTEMPTS" \
    bash -c '
        [ -f "$LIB_REAPER" ] || exit 1
        _abs_bash=$(command -v bash)
        _abs_sleep=$(command -v sleep)
        _abs_kill=$(command -v kill)

        _pidfile=$(mktemp)
        trap "rm -f \"$_pidfile\"" EXIT

        # Harness: sources the lib, installs the SAME teardown trap the
        # production verify.sh executor installs, then runs two sentinels
        # through reaper_run_in_pgroup (mirrors Test 3b) — but THIS harness
        # is SIGKILLed below, so the trap never fires.
        "$_abs_bash" -c "
            source \"$LIB_REAPER\"
            export _SENTINEL_SLEEP_SECS=\"$_SENTINEL_SLEEP_SECS\"
            export _SPAWN_SENTINEL_PIDFILE=\"$_pidfile\"
            trap \"reaper_teardown; exit 143\" TERM INT
            reaper_run_in_pgroup \"_spawn_sentinel_pgroup '\''$_KILL3D_MARKER_BIN'\'' 2\" &
            wait
        " &
        _harness_pid=$!

        # Wait for both sentinel PIDs to land in the pidfile (up to 4s).
        for ((_t=1; _t<=20; _t++)); do
            [ -s "$_pidfile" ] && [ "$(wc -l < "$_pidfile" | tr -d " ")" -ge 2 ] && break
            "$_abs_sleep" 0.2
        done

        # SIGKILL (uncatchable) the harness — simulates the heartbeat-idle
        # watchdog kill; "reaper_teardown; exit 143" does NOT run.
        "$_abs_kill" -9 "$_harness_pid" 2>/dev/null || true
        wait "$_harness_pid" 2>/dev/null || true

        # NON-VACUITY: at least one sentinel must be ALIVE immediately after
        # the SIGKILL (guards against a vacuous pass if the launcher/fixtures
        # never actually started — e.g. _spawn_sentinel_pgroup undefined).
        [ -s "$_pidfile" ] || { echo "FAIL: no sentinel PIDs recorded (launcher never ran)" >&2; exit 1; }
        _alive=0
        while read -r _spid; do
            [ -n "$_spid" ] || continue
            _pid_effectively_gone "$_spid" || _alive=1
        done < "$_pidfile"
        if [ "$_alive" -ne 1 ]; then
            echo "FAIL: no sentinel was alive immediately after SIGKILL (vacuous)" >&2
            exit 1
        fi

        # SELF-REAP: poll a bounded (load-scaled) window and assert ALL
        # orphaned sentinels are gone — self-expired via the short
        # _SENTINEL_SLEEP_SECS, without the trap and without the host-wide
        # reaper (which cannot match a bare copied-sleep binary anyway).
        _remaining=0
        while read -r _spid; do
            [ -n "$_spid" ] || continue
            _poll_pid_gone "$_spid" "$_POLL_ATTEMPTS" || _remaining=$((_remaining + 1))
        done < "$_pidfile"
        exit "$_remaining"
    '

# -- Test 3e: FIX #2 wiring integration (task 4931 / esc-3002-145) --
# A representative wired load-scaled poll (_poll_pid_gone) must emit a
# HEARTBEAT keepalive marker that ESCAPES assert()-style output suppression
# (test_helpers.sh assert() runs `"$@" >/dev/null 2>&1`) when routed to a
# PRESERVED fd. Reproduces that suppression with an explicit inner subshell
# redirect, mirroring exactly how every assert()-wrapped poll loop in this
# file is invoked. The HEARTBEAT token rides only in the suppressed grep
# argument below, never in an echoed description (esc-4789-63 /
# assert_marker convention).
# RED today: _poll_pid_gone still ticks via a bare `sleep 1` (no
# load_tolerant_sleep_tick), and no keepalive is ever emitted regardless of
# fd routing.
assert "a representative wired load-scaled poll (_poll_pid_gone) emits a HEARTBEAT marker to the preserved keepalive fd even under assert()-style output suppression" \
    env REIFY_CLOCK_HEARTBEAT_SECS=1 bash -c '
        _abs_sleep=$(command -v sleep)
        "$_abs_sleep" 10 &
        _live_pid=$!
        _cap=$(mktemp)
        exec 4>"$_cap"
        (
            export REIFY_KEEPALIVE_FD=4
            _poll_pid_gone "$_live_pid" 3
        ) >/dev/null 2>&1
        exec 4>&-
        kill -9 "$_live_pid" 2>/dev/null || true
        wait "$_live_pid" 2>/dev/null || true
        _n=$(grep -cE "reason=load_scaled_poll" "$_cap")
        rm -f "$_cap"
        [ "$_n" -ge 1 ]
    '

# ===========================================================================
# Part 4 — verify.sh wiring (structural, hermetic)
# ===========================================================================

echo ""
echo "--- Part 4: verify.sh wiring (structural) ---"

_VERIFY_SH="$REPO_ROOT/scripts/verify.sh"

assert "verify.sh sources lib_proc_reaper.sh" \
    grep -qE '^[[:space:]]*source "\$SCRIPT_DIR/lib_proc_reaper\.sh"' "$_VERIFY_SH"

assert "verify.sh executor routes non-node-lane commands through reaper_run_in_pgroup" \
    bash -c 'grep -qF "reaper_run_in_pgroup" "$1"' _ "$_VERIFY_SH"

assert "verify.sh installs TERM trap that calls _verify_cleanup (which invokes reaper_teardown)" \
    bash -c 'grep -qE "trap.*_verify_cleanup.*TERM" "$1"' _ "$_VERIFY_SH"

assert "_verify_cleanup calls reaper_teardown" \
    bash -c 'grep -A 30 "_verify_cleanup()" "$1" | grep -q "reaper_teardown"' _ "$_VERIFY_SH"

# -- NO-PLAN-CHURN guard --
# reaper_run_in_pgroup / reaper_teardown must NOT appear in any plan line
# (they live in the executor and traps, below the --print-plan early-exit).
assert "NO-PLAN-CHURN: 'reaper' absent from verify.sh --print-plan output (test scope)" \
    env REPO_ROOT="$REPO_ROOT" bash -c '
        cd "$REPO_ROOT"
        REIFY_TEST_SEMAPHORE_DISABLE=1 \
        bash scripts/verify.sh test --scope all --print-plan 2>/dev/null \
            | grep -v "^#" \
            | grep -qF "reaper" && exit 1 || exit 0
    '

assert "NO-PLAN-CHURN: 'lib_proc_reaper' absent from verify.sh --print-plan (all scope)" \
    env REPO_ROOT="$REPO_ROOT" bash -c '
        cd "$REPO_ROOT"
        REIFY_TEST_SEMAPHORE_DISABLE=1 \
        bash scripts/verify.sh all --scope all --include-infra --print-plan 2>/dev/null \
            | grep -v "^#" \
            | grep -qF "lib_proc_reaper" && exit 1 || exit 0
    '

# ===========================================================================
# Part 5 — end-to-end SIGKILL verification via reap-orphaned-test-binaries.sh
# ===========================================================================

echo ""
echo "--- Part 5: end-to-end SIGKILL verify (reap-orphaned-test-binaries.sh) ---"

_WRAPPER="$REPO_ROOT/scripts/reap-orphaned-test-binaries.sh"

assert "scripts/reap-orphaned-test-binaries.sh exists and is executable" \
    test -x "$_WRAPPER"

# Build a tmp target/debug/deps/ fake binary for the e2e test.
_E2E_DIR="$(mktemp -d)"
_TMPDIRS+=("$_E2E_DIR")
_E2E_DEPS="$_E2E_DIR/target/debug/deps"
mkdir -p "$_E2E_DEPS"
_E2E_FAKE="$_E2E_DEPS/reify_faketest_e2e_${_SENT_FAKE}"
cp "$(command -v sleep)" "$_E2E_FAKE"
chmod +x "$_E2E_FAKE"

# E2E flow:
# 1. A "parent" subprocess backgrounds the fake binary and gets SIGKILL'd.
# 2. The fake binary survives (SIGKILL cannot propagate to children via a trap).
# 3. The wrapper script reaps it based on PPID matching.
#
# We verify two sub-claims:
#   (A) The fake binary is still alive AFTER the parent is SIGKILL'd (non-vacuous).
#   (B) The wrapper reaps it.

assert "SIGKILL to parent does NOT reap the backgrounded test binary (survivor exists)" \
    env _E2E_FAKE="$_E2E_FAKE" _SENT_FAKE="$_SENT_FAKE" \
        _SENTINEL_SLEEP_SECS="$_SENTINEL_SLEEP_SECS" \
        _POLL_ATTEMPTS_ORPHAN="$_POLL_ATTEMPTS_ORPHAN" bash -c '
        _abs_sleep=$(command -v sleep)
        _abs_kill=$(command -v kill)
        _abs_bash=$(command -v bash)
        _pid_file=$(mktemp)
        trap "rm -f \"$_pid_file\"" EXIT

        # A "parent" shell backgrounds the fake binary and writes its PID to a file.
        # Self-expiring duration (task 4931 / esc-3002-145 FIX #1): the binary
        # NAME (already embedding $_SENT_FAKE) stays the unique marker; only
        # the runtime duration is bounded to _SENTINEL_SLEEP_SECS.
        "$_abs_bash" -c "\"$_E2E_FAKE\" \"$_SENTINEL_SLEEP_SECS\" </dev/null >/dev/null 2>&1 & echo \$! > \"$_pid_file\"; wait" &
        _parent_pid=$!
        # Wait for the fake binary to start (poll for PID file).
        for ((_t=1; _t<=20; _t++)); do
            [ -s "$_pid_file" ] && break
            "$_abs_sleep" 0.3
        done
        _fake_pid=$(cat "$_pid_file" 2>/dev/null || echo "")
        [ -n "$_fake_pid" ] || { echo "FAIL: did not get fake binary PID" >&2; exit 1; }

        # SIGKILL the parent, then confirm it is effectively gone (reparenting
        # completes at parent death) before sampling the survivor.
        "$_abs_kill" -9 "$_parent_pid" 2>/dev/null || true
        _poll_pid_gone "$_parent_pid" "$_POLL_ATTEMPTS_ORPHAN" || true

        # STRUCTURAL/condition-polled survivor-alive proof (task 5148):
        # replaces a fixed-wait + single ps sample, which raced a
        # transient-empty ps read under pool load (data/verify-logs/4911).
        # Delegates to the shared _poll_survivor_alive helper (exported at
        # top-of-file) instead of an inline loop, so the Test 5b hermetic
        # regression lock below — which drives this SAME helper — actually
        # guards this code path: a reversion to a single-shot sample
        # anywhere inside _poll_survivor_alive flips both this assertion and
        # the lock, not just a duplicated copy of the loop. Non-vacuous: a
        # genuinely-dead survivor exhausts the poll and returns 1 (RED).
        _poll_survivor_alive "$_fake_pid" "$_POLL_ATTEMPTS_ORPHAN"
        _alive_rc=$?
        "$_abs_kill" -9 "$_fake_pid" 2>/dev/null || true
        exit "$_alive_rc"
    '

# -- Test 5b (regression lock, task 5148): the condition-polled survivor
# check above tolerates a transient-empty ps read instead of misreporting a
# genuinely-alive survivor as DEAD. Under real pool load this was a race
# (observed in data/verify-logs/4911 etc: a fixed wait + ONE ps sample could
# land on a transient-empty read); here it is forced deterministically via a
# stateful `ps` stub (extends the Part 8 SLOW_PS_STUB idiom below) whose FIRST
# invocation emits nothing and whose invocations #2+ delegate to the real ps.
# The shared _poll_survivor_alive helper (see below — the SAME helper driven
# by the production Test 5 check above) retries past that first empty read
# and correctly converges on ALIVE. Locks the fix: a reversion to a
# single-shot sample anywhere in that shared helper would make BOTH this and
# Test 5 go RED again (as Test 5 did before task 5148).
_P5_STUB_DIR="$(mktemp -d)"
_TMPDIRS+=("$_P5_STUB_DIR")
_P5_STUB_COUNTER="$_P5_STUB_DIR/ps_invocation_count"
_abs_real_ps_p5="$(command -v ps)"

cat > "$_P5_STUB_DIR/ps" <<STUB_PS_EOF
#!/usr/bin/env bash
_n=\$(cat "$_P5_STUB_COUNTER" 2>/dev/null || echo 0)
_n=\$((_n + 1))
echo "\$_n" > "$_P5_STUB_COUNTER"
if [ "\$_n" -eq 1 ]; then
    exit 0
fi
exec "$_abs_real_ps_p5" "\$@"
STUB_PS_EOF
chmod +x "$_P5_STUB_DIR/ps"

assert "hermetic regression lock: condition-polled survivor check tolerates a transient-empty ps read (retries past invocation #1, task 5148)" \
    env _E2E_FAKE="$_E2E_FAKE" _SENT_FAKE="$_SENT_FAKE" \
        _SENTINEL_SLEEP_SECS="$_SENTINEL_SLEEP_SECS" \
        _POLL_ATTEMPTS_ORPHAN="$_POLL_ATTEMPTS_ORPHAN" \
        PATH="$_P5_STUB_DIR:$PATH" bash -c '
        _abs_sleep=$(command -v sleep)
        _abs_kill=$(command -v kill)
        _abs_bash=$(command -v bash)
        _pid_file=$(mktemp)
        trap "rm -f \"$_pid_file\"" EXIT

        "$_abs_bash" -c "\"$_E2E_FAKE\" \"$_SENTINEL_SLEEP_SECS\" </dev/null >/dev/null 2>&1 & echo \$! > \"$_pid_file\"; wait" &
        _parent_pid=$!
        for ((_t=1; _t<=20; _t++)); do
            [ -s "$_pid_file" ] && break
            "$_abs_sleep" 0.3
        done
        _fake_pid=$(cat "$_pid_file" 2>/dev/null || echo "")
        [ -n "$_fake_pid" ] || { echo "FAIL: did not get fake binary PID" >&2; exit 1; }

        # SIGKILL the parent (survivor is now orphaned but still alive).
        "$_abs_kill" -9 "$_parent_pid" 2>/dev/null || true

        # Drives the SAME shared _poll_survivor_alive helper (exported at
        # top-of-file, task 5148) as the production Test 5 check above,
        # rather than a duplicated for-loop, so this lock actually guards
        # that shared code path: a regression to a single-shot sample
        # anywhere inside _poll_survivor_alive flips BOTH Test 5 and this
        # lock. _pid_effectively_gone (called by the helper) resolves ps via
        # the PATH prepend above, so the helper FIRST call always hits the
        # stubbed transient-empty invocation #1 and is retried instead of
        # misreported as dead.
        _poll_survivor_alive "$_fake_pid" "$_POLL_ATTEMPTS_ORPHAN"
        _alive_rc=$?
        "$_abs_kill" -9 "$_fake_pid" 2>/dev/null || true
        exit "$_alive_rc"
    '

# ITEM 2a fixture (task 5260): a FOREIGN-run e2e marker used to prove the Part-5
# pre-clean below does NOT collateral-kill a concurrent run_all instance's
# suffixed marker. The foreign sentinel is chosen so THIS run's own marker suffix
# (_SENT_FAKE = $$*10+7) is NOT a substring of the foreign suffix ($$*10+1) —
# otherwise the suffixed grep (step-4 GREEN) would still substring-match and kill
# the foreign marker, making GREEN unreachable (see plan design decision).
_SENT_FAKE_FOREIGN=$(($$ * 10 + 1))
_FOREIGN_DIR="$(mktemp -d)"
_TMPDIRS+=("$_FOREIGN_DIR")
_FOREIGN_BIN="$_FOREIGN_DIR/reify_faketest_e2e_${_SENT_FAKE_FOREIGN}"
cp "$(command -v sleep)" "$_FOREIGN_BIN"
chmod +x "$_FOREIGN_BIN"
"$_FOREIGN_BIN" "$_SENTINEL_SLEEP_SECS" </dev/null >/dev/null 2>&1 &
_FOREIGN_PID=$!
assert "foreign-run e2e marker launched alive (precondition, task 5260)" \
    _poll_survivor_alive "$_FOREIGN_PID" 3

assert "reap-orphaned-test-binaries.sh reaps an orphaned test binary after parent SIGKILL" \
    env _WRAPPER="$_WRAPPER" _E2E_FAKE="$_E2E_FAKE" _E2E_DIR="$_E2E_DIR" \
        _SENT_FAKE="$_SENT_FAKE" _SENTINEL_SLEEP_SECS="$_SENTINEL_SLEEP_SECS" \
        _POLL_ATTEMPTS_ORPHAN="$_POLL_ATTEMPTS_ORPHAN" bash -c '
        [ -x "$_WRAPPER" ] || exit 1
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_kill=$(command -v kill)
        _abs_grep=$(command -v grep)
        _abs_bash=$(command -v bash)
        _pid_file=$(mktemp)
        trap "rm -f \"$_pid_file\"" EXIT

        # Pre-clean stale instances.
        "$_abs_ps" -A -o pid,exe 2>/dev/null | "$_abs_grep" "reify_faketest_e2e" \
            | awk "{print \$1}" \
            | while read -r _p; do "$_abs_kill" -9 "$_p" 2>/dev/null || true; done
        "$_abs_sleep" 0.3

        # Simulate the SIGKILL scenario: parent backgrounds fake binary, gets killed.
        # Self-expiring duration — see the sibling E2E assertion above.
        "$_abs_bash" -c "\"$_E2E_FAKE\" \"$_SENTINEL_SLEEP_SECS\" </dev/null >/dev/null 2>&1 & echo \$! > \"$_pid_file\"; wait" &
        _parent_pid=$!
        for ((_t=1; _t<=20; _t++)); do
            [ -s "$_pid_file" ] && break
            "$_abs_sleep" 0.3
        done
        _fake_pid=$(cat "$_pid_file" 2>/dev/null || echo "")
        [ -n "$_fake_pid" ] || { echo "FAIL: did not get fake binary PID" >&2; exit 1; }

        # SIGKILL the parent; fake binary is now orphaned (reparented to init/systemd).
        # _orphan_ppid_settled waits for the parent to be confirmed gone (reparenting
        # completes at parent exit) then samples the orphan'\''s settled PPID — avoids
        # the old sleep 0.3 race where the PPID sample could precede reparenting.
        "$_abs_kill" -9 "$_parent_pid" 2>/dev/null || true
        _fake_ppid=$(_orphan_ppid_settled "$_parent_pid" "$_fake_pid" "$_POLL_ATTEMPTS_ORPHAN")
        [ -n "$_fake_ppid" ] || { echo "FAIL: fake binary already gone before reaper ran" >&2; exit 1; }

        # Broaden the orphan set to the settled PPID + 1 + comms so the match
        # survives reparenting to init/systemd.  The deps-glob + UID + age filters
        # still isolate ONLY this binary, so selectivity is preserved.
        REIFY_REAPER_DEPS_GLOB="${_E2E_DIR}/target/debug/deps/*" \
        REIFY_REAPER_MIN_AGE_SECS=0 \
        REIFY_REAPER_ORPHAN_PPIDS="$_fake_ppid 1" \
        REIFY_REAPER_COMMS="systemd init" \
        REIFY_REAPER_UID=$(id -u) \
            bash "$_WRAPPER" >/dev/null 2>&1 || true

        # Poll until the fake binary is gone (zombie-aware; bumped budget base 20).
        _poll_pid_gone "$_fake_pid" "$_POLL_ATTEMPTS_ORPHAN"; exit $?
    '

# ITEM 2a assertion (task 5260): the Part-5 pre-clean (:945) must scope its
# host-wide `kill -9` to THIS run's suffixed marker (reify_faketest_e2e_${_SENT_FAKE}),
# NOT the unsuffixed host-wide `reify_faketest_e2e` — otherwise a CONCURRENT
# run_all instance's live fixture is collateral-killed. The foreign marker
# launched before the Part-5 assertion above must have SURVIVED that assertion's
# pre-clean. RED with the unsuffixed grep (foreign marker killed → poll exhausts);
# GREEN once :945 is suffixed. Then reap the foreign marker.
assert "e2e pre-clean spares a FOREIGN-suffixed marker — no cross-run collateral (task 5260)" \
    _poll_survivor_alive "$_FOREIGN_PID" 3
kill -9 "$_FOREIGN_PID" 2>/dev/null || true

# ===========================================================================
# Part 6 — zombie-aware orphan-reap poll helper
# ===========================================================================

echo ""
echo "--- Part 6: zombie-aware orphan-reap poll helper ---"

# Deterministic persistent-zombie fixture.
# Design: a parent bash (a) backgrounds a self-expiring sleep, (b) writes
# child PID to pidfile (bash builtin printf — no fork), (c) execs into a
# second self-expiring sleep.  After exec, the parent image is that sleep,
# which has no SIGCHLD handler and never calls wait().  The outer test then
# SIGKILL's the child; the exec'd parent does not reap it → the child stays
# in state 'Z' for the assertion window.
# This avoids bash's internal sigchld_handler (which would reap the child via
# waitpid(WNOHANG) even while bash is blocked on a builtin 'read < FIFO').
# Durations are magnitude-reduced to _SENTINEL_SLEEP_SECS (task 4931 /
# esc-3002-145 FIX #1) instead of the original fixed 1000/100000 — a
# structure-preserving change only (no timeout wrapper), so the exec'd-parent
# no-SIGCHLD-handler zombie/reparent semantics this fixture depends on are
# unchanged; _SENTINEL_SLEEP_SECS still comfortably exceeds the assertion
# window (up to _POLL_ATTEMPTS_ORPHAN load-scaled seconds, below).
_Z_TMPDIR="$(mktemp -d)"
_TMPDIRS+=("$_Z_TMPDIR")
_Z_PIDFILE="$_Z_TMPDIR/zombie_child.pid"
_abs_bash_p6="$(command -v bash)"
_abs_sleep_p6="$(command -v sleep)"
_abs_kill_p6="$(command -v kill)"

"$_abs_bash_p6" -c '
    '"$_abs_sleep_p6"' '"$_SENTINEL_SLEEP_SECS"' &
    _c=$!
    printf "%s\n" "$_c" > "'"$_Z_PIDFILE"'"
    exec '"$_abs_sleep_p6"' '"$_SENTINEL_SLEEP_SECS"'
' &
_ZPARENT=$!

# Poll for the pidfile (up to 20 × 0.2s = 4s).
_ZCHILD=""
for ((_t=1; _t<=20; _t++)); do
    if [ -s "$_Z_PIDFILE" ]; then
        _ZCHILD="$(cat "$_Z_PIDFILE" 2>/dev/null || true)"
        [ -n "$_ZCHILD" ] && break
    fi
    "$_abs_sleep_p6" 0.2
done

# Brief pause to ensure bash has exec'd into sleep (making it the parent).
"$_abs_sleep_p6" 0.2

# Kill the child → the exec'd parent has no SIGCHLD handler, so the child
# stays in state 'Z' (zombie) for the entire assertion window.
"$_abs_kill_p6" -9 "${_ZCHILD:-}" 2>/dev/null || true

# Launch a genuinely live process for non-vacuity of live-detection (assert 4).
"$_abs_sleep_p6" "$_SENTINEL_SLEEP_SECS" &
_ZLIVE=$!

# Assert 1 — NON-VACUITY: confirm the fixture child is in 'Z' state.
# A naive 'ps -o pid= -p <pid> | grep -q .' would see it PRESENT, proving the bug.
# Poll up to _POLL_ATTEMPTS_ORPHAN × 1s to observe the 'Z' transition.
assert "Part 6 fixture: zombie child shows 'Z' state in ps -o s= (non-vacuous: naive pid check would show present)" \
    env _ZCHILD="${_ZCHILD:-0}" _POLL_ATTEMPTS_ORPHAN="${_POLL_ATTEMPTS_ORPHAN:-20}" bash -c '
        _abs_ps=$(command -v ps)
        _state=""
        for ((_t=1; _t<=_POLL_ATTEMPTS_ORPHAN; _t++)); do
            _state=$("$_abs_ps" -o s= -p "$_ZCHILD" 2>/dev/null | tr -d " " || echo "")
            case "$_state" in Z*) break ;; esac
            # Keepalive-bearing tick (task 4931 / esc-3002-145 FIX #2) — this
            # orphan-reap poll shares the load-scaled _POLL_ATTEMPTS_ORPHAN
            # budget (up to ~160s under load) with Part 2/5/7s _poll_pid_gone
            # polls above.
            load_tolerant_sleep_tick
        done
        case "$_state" in Z*) exit 0 ;; *) exit 1 ;; esac
    '

# Assert 2 — _poll_pid_gone treats a zombie pid as effectively gone.
# First confirm _ZCHILD is still in 'Z' state so the assertion specifically
# verifies zombie handling rather than the trivial empty-state (fully-reaped) path.
assert "Part 6: _poll_pid_gone on a zombie pid returns 0 (zombie => effectively gone)" \
    env _ZCHILD="${_ZCHILD:-0}" bash -c '
        _abs_ps=$(command -v ps)
        "$_abs_ps" -o s= -p "$_ZCHILD" 2>/dev/null | grep -q "^Z" || exit 1
        _poll_pid_gone "$_ZCHILD" 3
    '

# Assert 3 — _poll_pid_gone treats a never-existed pid as gone (empty ps state).
assert "Part 6: _poll_pid_gone on a never-existed pid returns 0 (empty state => gone)" \
    bash -c '
        _poll_pid_gone 999999990 1
    '

# Assert 4 — NON-VACUITY of live-detection: a genuinely running process is NOT gone.
assert "Part 6: _poll_pid_gone on a live process returns 1 (live process is not gone)" \
    env _ZLIVE="${_ZLIVE}" bash -c '
        _poll_pid_gone "$_ZLIVE" 2 && exit 1 || exit 0
    '

# Tear down fixture processes.
kill -9 "$_ZPARENT" 2>/dev/null || true
wait "$_ZPARENT" 2>/dev/null || true
kill -9 "$_ZLIVE" 2>/dev/null || true
wait "$_ZLIVE" 2>/dev/null || true

# ===========================================================================
# Part 7 — reparent-robust orphan-PPID capture
# ===========================================================================

echo ""
echo "--- Part 7: reparent-robust orphan-PPID capture ---"

# Fixture: parent bash backgrounds a self-expiring sleep, writes child PID to
# pidfile, then 'wait's.  The outer test SIGKILL's the parent; the child is
# orphaned (reparented to init/systemd) while the parent becomes a zombie.
# No deps-glob or reaper invocation needed — we only test the PPID-settle helper.
# Duration magnitude-reduced to _SENTINEL_SLEEP_SECS (task 4931 /
# esc-3002-145 FIX #1) instead of the original fixed 1000 — structure
# unchanged, still comfortably exceeds the assertion window below.
_P7_TMPDIR="$(mktemp -d)"
_TMPDIRS+=("$_P7_TMPDIR")
_P7_PIDFILE="$_P7_TMPDIR/child.pid"
_abs_bash_p7="$(command -v bash)"
_abs_sleep_p7="$(command -v sleep)"
_abs_kill_p7="$(command -v kill)"

"$_abs_bash_p7" -c '
    '"$_abs_sleep_p7"' '"$_SENTINEL_SLEEP_SECS"' &
    echo $! > "'"$_P7_PIDFILE"'"
    wait
' &
_P7_PARENT=$!

# Poll for the child PID (up to 20 × 0.2s = 4s).
_P7_CHILD=""
for ((_t=1; _t<=20; _t++)); do
    if [ -s "$_P7_PIDFILE" ]; then
        _P7_CHILD="$(cat "$_P7_PIDFILE" 2>/dev/null || true)"
        [ -n "$_P7_CHILD" ] && break
    fi
    "$_abs_sleep_p7" 0.2
done

# SIGKILL the parent → the child sleep is orphaned (reparented to init/systemd).
# Do NOT wait here — leave the parent as a zombie so _orphan_ppid_settled's
# settle-wait polling logic is exercised before reparenting is confirmed.
"$_abs_kill_p7" -9 "${_P7_PARENT:-}" 2>/dev/null || true

# Assert 1 — _orphan_ppid_settled waits for the parent to be confirmed gone
# (reparenting is complete once the parent is a zombie/reaped) then echoes
# the orphan'\''s settled PPID.
assert "Part 7: _orphan_ppid_settled returns a non-empty settled PPID after parent SIGKILL" \
    env _P7_PARENT="${_P7_PARENT:-}" _P7_CHILD="${_P7_CHILD:-}" \
        _POLL_ATTEMPTS_ORPHAN="$_POLL_ATTEMPTS_ORPHAN" bash -c '
        _settled=$(_orphan_ppid_settled "$_P7_PARENT" "$_P7_CHILD" "$_POLL_ATTEMPTS_ORPHAN")
        [ -n "$_settled" ]
    '

# Assert 2 — after _orphan_ppid_settled returns, the parent must already be
# gone (reparenting completes at parent exit, i.e. as soon as parent is zombie).
# Invoke the helper in this same subshell so the post-condition is checked
# immediately after the helper returns — non-vacuous even if the outer shell
# has already reaped the parent zombie by this point.
assert "Part 7: parent is confirmed zombie/gone immediately after _orphan_ppid_settled returns" \
    env _P7_PARENT="${_P7_PARENT:-}" _P7_CHILD="${_P7_CHILD:-}" \
        _POLL_ATTEMPTS_ORPHAN="$_POLL_ATTEMPTS_ORPHAN" bash -c '
        _orphan_ppid_settled "$_P7_PARENT" "$_P7_CHILD" "$_POLL_ATTEMPTS_ORPHAN" > /dev/null
        _poll_pid_gone "$_P7_PARENT" 1
    '

# Tear down the orphaned child; reap the parent zombie (if not yet reaped
# by the outer shell'\''s async SIGCHLD handler).
"$_abs_kill_p7" -9 "${_P7_CHILD:-}" 2>/dev/null || true
wait "$_P7_PARENT" 2>/dev/null || true

# ===========================================================================
# Part 8 — bounded host-wide ps scan (timeout wrapper, esc-4889-94)
# ===========================================================================
#
# Guards the _reaper_bounded_ps fix: a stalled host-wide ps under extreme load
# (avg 60+) previously blocked the reaper >180s -> orchestrator heartbeat-idle
# backstop SIGKILLed the verify run.  The fix wraps both ps callsites in a
# bounded helper that returns promptly AND emits a non-silent WARNING on stall.
#
# Stub a slow `ps` (writes nothing, sleeps 30s) first on PATH; set
# REIFY_REAPER_PS_TIMEOUT=2 to make the bound fire at ~2s.
# RED today: no timeout wrapper -> scan waits the full 30s stub; no warning.

echo ""
echo "--- Part 8: bounded host-wide ps scan (timeout wrapper, esc-4889-94) ---"

# Structural guards (mirrors Part 1b / Part 4 style).
assert "lib_proc_reaper.sh defines _reaper_bounded_ps helper" \
    bash -c 'grep -qF "_reaper_bounded_ps()" "$1"' _ "$LIB_REAPER"
assert "lib_proc_reaper.sh references REIFY_REAPER_PS_TIMEOUT knob" \
    bash -c 'grep -qF "REIFY_REAPER_PS_TIMEOUT" "$1"' _ "$LIB_REAPER"

# Behavioral tests 8a/8b require GNU 'timeout' on PATH.  Without it,
# _reaper_bounded_ps falls back to bare ps, the slow-stub would stall the full
# 30 s, and both assertions would fail for the wrong reason — testing the
# fallback path (which is intentionally unguarded) rather than the bound.
# Guard mirrors the helper's own `command -v timeout` guard so the test only
# asserts the bounded path when that path is actually available.
if ! command -v timeout >/dev/null 2>&1; then
    echo "Part 8 behavioral tests SKIP: 'timeout' not on PATH; _reaper_bounded_ps falls back to bare ps in this environment — bounded-ps path untestable without timeout binary"
else

# Build hermetic stub-bin dir: a slow `ps` that writes nothing and sleeps 30s.
# Only `ps` is stubbed; timeout/readlink/kill/tr remain the real binaries
# (PATH-prepend: stub dir first so only `ps` resolves to the stub).
_P8_BINDIR="$(mktemp -d)"
_TMPDIRS+=("$_P8_BINDIR")
cat > "$_P8_BINDIR/ps" <<'SLOW_PS_STUB'
#!/usr/bin/env bash
# Slow ps stub: writes nothing to stdout; sleeps 30s to simulate ps stall.
sleep 30
SLOW_PS_STUB
chmod +x "$_P8_BINDIR/ps"

# Test 8a: MECHANISM proof -- the ps-timeout wrapper fires (marker present in
# stderr) and the sweep returns promptly under a generous anti-hang ceiling,
# even though the stub ps sleeps 30s without the timeout wrapper.
# NON-VACUOUS: unlike an elapsed bound, the marker appears ONLY when
# _reaper_bounded_ps actually times out. If lib's bounding were removed
# (bare-ps fallback), the sweep would still finish under the generous 60s
# ceiling (30s stub < 60s) -- an elapsed bound would pass either way -- but
# the marker would be ABSENT, so this assertion still goes RED.
# REIFY_REAPER_PS_TIMEOUT=2 -> internal timeout fires at ~2s; the outer 60s
# timeout is a pure anti-hang backstop, not a discriminator.
assert "reap-orphans fires the ps-timeout mechanism under a generous anti-hang ceiling (REIFY_REAPER_PS_TIMEOUT=2 bounds scan)" \
    env LIB_REAPER="$LIB_REAPER" P8_BINDIR="$_P8_BINDIR" bash -c '
        _err=$(mktemp)
        trap "rm -f \"$_err\"" EXIT
        PATH="$P8_BINDIR:$PATH" \
        REIFY_REAPER_PS_TIMEOUT=2 \
        REIFY_REAPER_UID=$(id -u) \
            timeout 60 bash "$LIB_REAPER" reap-orphans >/dev/null 2>"$_err"
        _rc=$?
        [ "$_rc" -ne 124 ] && grep -qF "host-wide ps scan exceeded" "$_err"
    '

# Test 8b: non-vacuous proof -- stderr must carry the timeout-warning substring.
# Proves the timeout branch fired (not a vacuous fast return for another reason).
assert "reap-orphans emits 'host-wide ps scan exceeded' warning to stderr when ps times out" \
    env LIB_REAPER="$LIB_REAPER" P8_BINDIR="$_P8_BINDIR" bash -c '
        _err=$(mktemp)
        trap "rm -f \"$_err\"" EXIT
        PATH="$P8_BINDIR:$PATH" \
        REIFY_REAPER_PS_TIMEOUT=2 \
        REIFY_REAPER_UID=$(id -u) \
            bash "$LIB_REAPER" reap-orphans >/dev/null 2>"$_err" || true
        grep -qF "host-wide ps scan exceeded" "$_err"
    '

fi  # end: command -v timeout guard

# ===========================================================================
# Part 9 — esc-5020 run_all false-kill rule-out
# ===========================================================================
#
# Reconstructs the live tests/infra/run_all.sh pool topology: a pool subshell
# / test_*.sh launcher (bash, no setsid/systemd-run/nohup/disown anywhere in
# run_all.sh, so ancestry stays intact while live) backgrounds a compiled
# deps-glob binary child while the run_all invocation is still running.
# Asserts the host-wide reaper cannot false-kill that child: the orphan-PPID
# gate (PPID/parent-comm ancestry check) structurally excludes it while its
# ancestry is intact (PPID = the live launcher, comm=bash — neither in
# REIFY_REAPER_ORPHAN_PPIDS={1} nor REIFY_REAPER_COMMS={systemd,init}), fully
# independent of the age gate (isolated here via REIFY_REAPER_MIN_AGE_SECS=0).
# See esc-5020 and docs/notes/orphaned-test-binary-reaper.md.

echo ""
echo "--- Part 9: esc-5020 run_all false-kill rule-out ---"

# Hermetic fixture (reuses the Part 2 deps-glob-binary idiom):
# tmp/target/debug/deps/reify_runall_live_<per-$$>
_SENT_RUNALL=$(($$ * 10 + 1))
_P9_DIR="$(mktemp -d)"
_TMPDIRS+=("$_P9_DIR")
_P9_DEPS="$_P9_DIR/target/debug/deps"
mkdir -p "$_P9_DEPS"
_RUNALL_BIN="$_P9_DEPS/reify_runall_live_${_SENT_RUNALL}"
cp "$(command -v sleep)" "$_RUNALL_BIN"
chmod +x "$_RUNALL_BIN"

# -- Test 9a: RED DRIVER — dry-run reports a "spared (non-orphan/live
# ancestry)" diagnostic naming the child pid. --
# The launcher (this bash -c, standing in for a run_all pool subshell /
# test_*.sh) backgrounds the deps-glob child and stays alive for the whole
# assertion window (it never exits until the script's own final `exit`), so
# the child's PPID is the LIVE launcher (!=1, comm=bash) throughout. Run under
# the reaper's PRODUCTION-DEFAULT orphan gates (ORPHAN_PPIDS=1, COMMS="systemd
# init") — the ancestry-intact child must NOT match, and (RED today) no
# diagnostic exists yet to report why it was spared.
assert "reap-orphans --dry-run reports a spared/non-orphan diagnostic for a live run_all-topology child (esc-5020)" \
    env LIB_REAPER="$LIB_REAPER" _RUNALL_BIN="$_RUNALL_BIN" _SENT_RUNALL="$_SENT_RUNALL" \
        _P9_DIR="$_P9_DIR" _SENTINEL_SLEEP_SECS="$_SENTINEL_SLEEP_SECS" bash -c '
        [ -f "$LIB_REAPER" ] || exit 1
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_kill=$(command -v kill)
        _abs_grep=$(command -v grep)
        _abs_awk=$(command -v awk)

        # Pre-clean stale instances of this fixture binary.
        "$_abs_ps" -A -o pid,args 2>/dev/null \
            | "$_abs_grep" -E "reify_runall_live_${_SENT_RUNALL}" \
            | "$_abs_awk" "{print \$1}" \
            | while read -r _p; do "$_abs_kill" -9 "$_p" 2>/dev/null || true; done
        "$_abs_sleep" 0.3

        "$_RUNALL_BIN" "$_SENTINEL_SLEEP_SECS" </dev/null >/dev/null 2>&1 &
        _child_pid=$!
        "$_abs_sleep" 0.2

        _err=$(mktemp)

        REIFY_REAPER_DEPS_GLOB="${_P9_DIR}/target/debug/deps/*" \
        REIFY_REAPER_MIN_AGE_SECS=0 \
        REIFY_REAPER_ORPHAN_PPIDS=1 \
        REIFY_REAPER_COMMS="systemd init" \
        REIFY_REAPER_UID=$(id -u) \
            bash "$LIB_REAPER" reap-orphans --dry-run >/dev/null 2>"$_err" || true

        _rc=0
        grep -qE "spared \(non-orphan/live ancestry\):.*pid=$_child_pid exe=" "$_err" || _rc=1

        "$_abs_kill" -9 "$_child_pid" 2>/dev/null || true
        wait "$_child_pid" 2>/dev/null || true
        rm -f "$_err"
        exit "$_rc"
    '

# -- Test 9b: REGRESSION LOCK — a NON-dry-run sweep under the SAME
# production-default orphan gates SPARES the live run_all-topology child
# (mirrors Test 2b's negative-spare assertion pattern). --
assert "reap-orphans (non-dry-run) spares a live run_all-topology child under production-default orphan gates (esc-5020)" \
    env LIB_REAPER="$LIB_REAPER" _RUNALL_BIN="$_RUNALL_BIN" _SENT_RUNALL="$_SENT_RUNALL" \
        _P9_DIR="$_P9_DIR" _SENTINEL_SLEEP_SECS="$_SENTINEL_SLEEP_SECS" bash -c '
        [ -f "$LIB_REAPER" ] || exit 1
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_kill=$(command -v kill)
        _abs_grep=$(command -v grep)

        "$_RUNALL_BIN" "$_SENTINEL_SLEEP_SECS" </dev/null >/dev/null 2>&1 &
        _child_pid=$!
        "$_abs_sleep" 0.2

        REIFY_REAPER_DEPS_GLOB="${_P9_DIR}/target/debug/deps/*" \
        REIFY_REAPER_MIN_AGE_SECS=0 \
        REIFY_REAPER_ORPHAN_PPIDS=1 \
        REIFY_REAPER_COMMS="systemd init" \
        REIFY_REAPER_UID=$(id -u) \
            bash "$LIB_REAPER" reap-orphans >/dev/null 2>&1 || true

        _alive=0
        "$_abs_ps" -o pid= -p "$_child_pid" 2>/dev/null | "$_abs_grep" -q . && _alive=1 || true
        "$_abs_kill" -9 "$_child_pid" 2>/dev/null || true
        wait "$_child_pid" 2>/dev/null || true
        exit $((1 - _alive))
    '

# -- Test 9c: NON-VACUITY — the SAME fixture IS reaped once
# REIFY_REAPER_ORPHAN_PPIDS is set to the child's ACTUAL PPID, proving 9b's
# spare was not vacuous (the child genuinely matches the deps-glob + age
# filters; only the orphan-PPID/ancestry gate spared it) — mirrors Test 2a.
# The launcher (this bash -c) never exits before reading the PPID, so the
# child's PPID is stable and no reparenting can occur (same reasoning as
# Test 2a). --
assert "reap-orphans kills the SAME run_all-topology child once ORPHAN_PPIDS matches its real ancestry (non-vacuity for 9b, esc-5020)" \
    env LIB_REAPER="$LIB_REAPER" _RUNALL_BIN="$_RUNALL_BIN" _SENT_RUNALL="$_SENT_RUNALL" \
        _P9_DIR="$_P9_DIR" _SENTINEL_SLEEP_SECS="$_SENTINEL_SLEEP_SECS" \
        _POLL_ATTEMPTS_ORPHAN="$_POLL_ATTEMPTS_ORPHAN" bash -c '
        [ -f "$LIB_REAPER" ] || exit 1
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)

        "$_RUNALL_BIN" "$_SENTINEL_SLEEP_SECS" </dev/null >/dev/null 2>&1 &
        _child_pid=$!
        "$_abs_sleep" 0.2

        _child_ppid=$("$_abs_ps" -o ppid= -p "$_child_pid" 2>/dev/null | tr -d " " || echo "")
        [ -n "$_child_ppid" ] || { echo "FAIL: could not read PPID of run_all-topology child" >&2; exit 1; }

        REIFY_REAPER_DEPS_GLOB="${_P9_DIR}/target/debug/deps/*" \
        REIFY_REAPER_MIN_AGE_SECS=0 \
        REIFY_REAPER_ORPHAN_PPIDS="$_child_ppid" \
        REIFY_REAPER_COMMS="" \
        REIFY_REAPER_UID=$(id -u) \
            bash "$LIB_REAPER" reap-orphans >/dev/null 2>&1 || true

        _poll_pid_gone "$_child_pid" "$_POLL_ATTEMPTS_ORPHAN"; exit $?
    '

# -- Test 9d: SILENCE — a NON-dry-run sweep under the SAME production-default
# orphan gates does NOT emit the "spared (non-orphan/live ancestry)"
# diagnostic for the live run_all-topology child. This locks the load-bearing
# claim (lib_proc_reaper.sh header + docs/notes/orphaned-test-binary-reaper.md)
# that the spare diagnostic is dry-run-only: Test 9b's non-dry-run sweep
# discards stderr wholesale, so it alone would not catch the diagnostic
# accidentally being hoisted out of the `[ "$_dry_run" -eq 1 ]` guard and
# spamming one line per live deps-glob binary on a busy host. --
assert "reap-orphans (non-dry-run) does not emit the spared/non-orphan diagnostic (esc-5020)" \
    env LIB_REAPER="$LIB_REAPER" _RUNALL_BIN="$_RUNALL_BIN" _SENT_RUNALL="$_SENT_RUNALL" \
        _P9_DIR="$_P9_DIR" _SENTINEL_SLEEP_SECS="$_SENTINEL_SLEEP_SECS" bash -c '
        [ -f "$LIB_REAPER" ] || exit 1
        _abs_sleep=$(command -v sleep)
        _abs_kill=$(command -v kill)
        _abs_grep=$(command -v grep)

        "$_RUNALL_BIN" "$_SENTINEL_SLEEP_SECS" </dev/null >/dev/null 2>&1 &
        _child_pid=$!
        "$_abs_sleep" 0.2

        _err=$(mktemp)

        REIFY_REAPER_DEPS_GLOB="${_P9_DIR}/target/debug/deps/*" \
        REIFY_REAPER_MIN_AGE_SECS=0 \
        REIFY_REAPER_ORPHAN_PPIDS=1 \
        REIFY_REAPER_COMMS="systemd init" \
        REIFY_REAPER_UID=$(id -u) \
            bash "$LIB_REAPER" reap-orphans >/dev/null 2>"$_err" || true

        _rc=0
        "$_abs_grep" -qF "spared (non-orphan/live ancestry)" "$_err" && _rc=1

        "$_abs_kill" -9 "$_child_pid" 2>/dev/null || true
        wait "$_child_pid" 2>/dev/null || true
        rm -f "$_err"
        exit "$_rc"
    '

test_summary
