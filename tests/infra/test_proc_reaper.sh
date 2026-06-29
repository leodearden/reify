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

# Per-instance sentinels — different per-$$ to avoid cross-instance collisions
# under concurrent verify (same idiom as test_portable_timeout.sh:69-70).
_SENT_KILL=$(($$ * 10 + 3))    # used in reaper_kill_pgroup behavioral test
_SENT_PGROUP=$(($$ * 10 + 5))  # used in reaper_run_in_pgroup / teardown tests
_SENT_FAKE=$(($$ * 10 + 7))    # used in reap-orphans fixture / e2e test

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
        sleep 1
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

export -f _pid_effectively_gone _poll_pid_gone _orphan_ppid_settled

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

# -- Test 1c: behavioral: kill_pgroup kills leader AND child --
# Explicitly guards against vacuous pass when lib doesn't exist ([ -f guard ]).
assert "reaper_kill_pgroup kills the process-group leader and its child" \
    env LIB_REAPER="$LIB_REAPER" _SENT_KILL="$_SENT_KILL" _POLL_ATTEMPTS="$_POLL_ATTEMPTS" \
    bash -c '
        [ -f "$LIB_REAPER" ] || exit 1
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_grep=$(command -v grep)
        _abs_kill=$(command -v kill)
        _abs_awk=$(command -v awk)

        # Pre-clean any stale sentinel sleeps from prior runs.
        "$_abs_ps" -A -o pid,args 2>/dev/null \
            | "$_abs_grep" -E "[[:space:]]sleep ${_SENT_KILL}$" \
            | "$_abs_awk" "{print \$1}" \
            | while read -r _pid; do "$_abs_kill" -9 "$_pid" 2>/dev/null || true; done
        "$_abs_sleep" 0.3

        source "$LIB_REAPER"

        # Launch a process group: leader bash spawns two sleep children.
        # Under set -m, the backgrounded command is the group leader (PGID == $!).
        REIFY_REAPER_GRACE_SECS=0
        set -m 2>/dev/null || true
        bash -c "\"$_abs_sleep\" $_SENT_KILL & \"$_abs_sleep\" $_SENT_KILL; wait" &
        _pgid=$!
        set +m 2>/dev/null || true

        # Brief pause to let children start.
        "$_abs_sleep" 0.3

        reaper_kill_pgroup "$_pgid"

        # Poll until all sentinel sleeps are gone.
        _found=0
        for ((_t=1; _t<=_POLL_ATTEMPTS; _t++)); do
            _found=0
            if "$_abs_ps" -A -o pid,args 2>/dev/null \
                | "$_abs_grep" -qE "[[:space:]]sleep ${_SENT_KILL}$"; then
                _found=1
            fi
            [ "$_found" -eq 0 ] && break
            "$_abs_sleep" 1
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
        _FIXTURE_DIR="$_FIXTURE_DIR" \
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
        "$_FAKE_BIN" "$_SENT_FAKE" </dev/null >/dev/null 2>&1 &
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
        _FIXTURE_DIR="$_FIXTURE_DIR" bash -c '
        [ -f "$LIB_REAPER" ] || exit 1
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_kill=$(command -v kill)
        _abs_grep=$(command -v grep)

        "$_FAKE_BIN" "$_SENT_FAKE" </dev/null >/dev/null 2>&1 &
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
        _FIXTURE_DIR="$_FIXTURE_DIR" bash -c '
        [ -f "$LIB_REAPER" ] || exit 1
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_kill=$(command -v kill)
        _abs_grep=$(command -v grep)

        "$_FAKE_BIN" "$_SENT_FAKE" </dev/null >/dev/null 2>&1 &
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
    env LIB_REAPER="$LIB_REAPER" _SENT_FAKE="$_SENT_FAKE" bash -c '
        [ -f "$LIB_REAPER" ] || exit 1
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_kill=$(command -v kill)
        _abs_grep=$(command -v grep)

        # Launch a plain sleep (not under any deps dir) as background child.
        "$_abs_sleep" "$_SENT_FAKE" &
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
        _FIXTURE_DIR="$_FIXTURE_DIR" bash -c '
        [ -f "$LIB_REAPER" ] || exit 1
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_kill=$(command -v kill)
        _abs_grep=$(command -v grep)

        "$_FAKE_BIN" "$_SENT_FAKE" </dev/null >/dev/null 2>&1 &
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

# -- Test 3b: teardown-on-signal tears down the whole process group --
# A harness subshell sources the lib, runs a long-running pass in background
# so we can SIGTERM the harness mid-pass.
# Assert: after SIGTERM, both sleep processes inside the pass are reaped.
# NOTE: the harness bash -c uses "..." quoting; trap body uses escaped inner quotes.
assert "reaper_teardown on SIGTERM kills the entire in-flight process group" \
    env LIB_REAPER="$LIB_REAPER" _SENT_PGROUP="$_SENT_PGROUP" _POLL_ATTEMPTS="$_POLL_ATTEMPTS" \
    bash -c '
        [ -f "$LIB_REAPER" ] || exit 1
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_grep=$(command -v grep)
        _abs_kill=$(command -v kill)
        _abs_awk=$(command -v awk)
        _abs_bash=$(command -v bash)

        # Pre-clean stale sentinel sleeps.
        "$_abs_ps" -A -o pid,args 2>/dev/null \
            | "$_abs_grep" -E "[[:space:]]sleep ${_SENT_PGROUP}$" \
            | "$_abs_awk" "{print \$1}" \
            | while read -r _p; do "$_abs_kill" -9 "$_p" 2>/dev/null || true; done
        "$_abs_sleep" 0.3

        # Harness subshell: installs teardown trap (using double-quotes for trap body
        # since we are inside a double-quoted bash -c string), starts a long-running pass.
        "$_abs_bash" -c "
            source \"$LIB_REAPER\"
            REIFY_REAPER_GRACE_SECS=0
            trap \"reaper_teardown; exit 143\" TERM INT
            reaper_run_in_pgroup \"$_abs_sleep $_SENT_PGROUP & $_abs_sleep $_SENT_PGROUP; wait\" &
            wait
        " &
        _harness_pid=$!

        # Let the pass start.
        "$_abs_sleep" 0.8

        # SIGTERM the harness.
        "$_abs_kill" -TERM "$_harness_pid" 2>/dev/null || true
        wait "$_harness_pid" 2>/dev/null || true

        # Poll until all sentinel sleeps are gone.
        _found=0
        for ((_t=1; _t<=_POLL_ATTEMPTS; _t++)); do
            _found=0
            if "$_abs_ps" -A -o pid,args 2>/dev/null \
                | "$_abs_grep" -qE "[[:space:]]sleep ${_SENT_PGROUP}$"; then
                _found=1
            fi
            [ "$_found" -eq 0 ] && break
            "$_abs_sleep" 1
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
    env _E2E_FAKE="$_E2E_FAKE" _SENT_FAKE="$_SENT_FAKE" bash -c '
        _abs_sleep=$(command -v sleep)
        _abs_ps=$(command -v ps)
        _abs_kill=$(command -v kill)
        _abs_grep=$(command -v grep)
        _abs_bash=$(command -v bash)
        _pid_file=$(mktemp)
        trap "rm -f \"$_pid_file\"" EXIT

        # A "parent" shell backgrounds the fake binary and writes its PID to a file.
        "$_abs_bash" -c "\"$_E2E_FAKE\" \"$_SENT_FAKE\" </dev/null >/dev/null 2>&1 & echo \$! > \"$_pid_file\"; wait" &
        _parent_pid=$!
        # Wait for the fake binary to start (poll for PID file).
        for ((_t=1; _t<=20; _t++)); do
            [ -s "$_pid_file" ] && break
            "$_abs_sleep" 0.3
        done
        _fake_pid=$(cat "$_pid_file" 2>/dev/null || echo "")
        [ -n "$_fake_pid" ] || { echo "FAIL: did not get fake binary PID" >&2; exit 1; }

        # SIGKILL the parent.
        "$_abs_kill" -9 "$_parent_pid" 2>/dev/null || true
        "$_abs_sleep" 0.5

        # Assert the fake binary is still alive after parent SIGKILL.
        _alive=0
        "$_abs_ps" -o pid= -p "$_fake_pid" 2>/dev/null | "$_abs_grep" -q . && _alive=1 || true
        "$_abs_kill" -9 "$_fake_pid" 2>/dev/null || true
        exit $((1 - _alive))
    '

assert "reap-orphaned-test-binaries.sh reaps an orphaned test binary after parent SIGKILL" \
    env _WRAPPER="$_WRAPPER" _E2E_FAKE="$_E2E_FAKE" _E2E_DIR="$_E2E_DIR" \
        _SENT_FAKE="$_SENT_FAKE" \
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
        "$_abs_bash" -c "\"$_E2E_FAKE\" \"$_SENT_FAKE\" </dev/null >/dev/null 2>&1 & echo \$! > \"$_pid_file\"; wait" &
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

# ===========================================================================
# Part 6 — zombie-aware orphan-reap poll helper
# ===========================================================================

echo ""
echo "--- Part 6: zombie-aware orphan-reap poll helper ---"

# Deterministic persistent-zombie fixture.
# Design: a parent bash (a) backgrounds sleep 1000, (b) writes child PID to
# pidfile (bash builtin printf — no fork), (c) execs into sleep 100000.
# After exec, the parent image is 'sleep 100000' which has no SIGCHLD handler
# and never calls wait().  The outer test then SIGKILL's the child; sleep 100000
# does not reap it → the child stays in state 'Z' for the assertion window.
# This avoids bash's internal sigchld_handler (which would reap the child via
# waitpid(WNOHANG) even while bash is blocked on a builtin 'read < FIFO').
_Z_TMPDIR="$(mktemp -d)"
_TMPDIRS+=("$_Z_TMPDIR")
_Z_PIDFILE="$_Z_TMPDIR/zombie_child.pid"
_abs_bash_p6="$(command -v bash)"
_abs_sleep_p6="$(command -v sleep)"
_abs_kill_p6="$(command -v kill)"

"$_abs_bash_p6" -c '
    '"$_abs_sleep_p6"' 1000 &
    _c=$!
    printf "%s\n" "$_c" > "'"$_Z_PIDFILE"'"
    exec '"$_abs_sleep_p6"' 100000
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

# Kill the child → parent (sleep 100000) has no SIGCHLD handler, so the child
# stays in state 'Z' (zombie) for the entire assertion window.
"$_abs_kill_p6" -9 "${_ZCHILD:-}" 2>/dev/null || true

# Launch a genuinely live process for non-vacuity of live-detection (assert 4).
"$_abs_sleep_p6" 500 &
_ZLIVE=$!

# Assert 1 — NON-VACUITY: confirm the fixture child is in 'Z' state.
# A naive 'ps -o pid= -p <pid> | grep -q .' would see it PRESENT, proving the bug.
# Poll up to _POLL_ATTEMPTS_ORPHAN × 1s to observe the 'Z' transition.
assert "Part 6 fixture: zombie child shows 'Z' state in ps -o s= (non-vacuous: naive pid check would show present)" \
    env _ZCHILD="${_ZCHILD:-0}" _POLL_ATTEMPTS_ORPHAN="${_POLL_ATTEMPTS_ORPHAN:-20}" bash -c '
        _abs_ps=$(command -v ps)
        _abs_sleep=$(command -v sleep)
        _state=""
        for ((_t=1; _t<=_POLL_ATTEMPTS_ORPHAN; _t++)); do
            _state=$("$_abs_ps" -o s= -p "$_ZCHILD" 2>/dev/null | tr -d " " || echo "")
            case "$_state" in Z*) break ;; esac
            "$_abs_sleep" 1
        done
        case "$_state" in Z*) exit 0 ;; *) exit 1 ;; esac
    '

# Assert 2 — _poll_pid_gone treats a zombie pid as effectively gone.
assert "Part 6: _poll_pid_gone on a zombie pid returns 0 (zombie => effectively gone)" \
    env _ZCHILD="${_ZCHILD:-0}" bash -c '
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

# Fixture: parent bash backgrounds sleep 1000, writes child PID to pidfile,
# then 'wait's.  The outer test SIGKILL's the parent; the child (sleep 1000)
# is orphaned (reparented to init/systemd) while the parent becomes a zombie.
# No deps-glob or reaper invocation needed — we only test the PPID-settle helper.
_P7_TMPDIR="$(mktemp -d)"
_TMPDIRS+=("$_P7_TMPDIR")
_P7_PIDFILE="$_P7_TMPDIR/child.pid"
_abs_bash_p7="$(command -v bash)"
_abs_sleep_p7="$(command -v sleep)"
_abs_kill_p7="$(command -v kill)"

"$_abs_bash_p7" -c '
    '"$_abs_sleep_p7"' 1000 &
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

# SIGKILL the parent → child (sleep 1000) is orphaned (reparented to init/systemd).
# wait: reaps the zombie immediately and silences bash's "Killed" job notification.
"$_abs_kill_p7" -9 "${_P7_PARENT:-}" 2>/dev/null || true
wait "${_P7_PARENT:-}" 2>/dev/null || true

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
assert "Part 7: parent is confirmed zombie/gone immediately after _orphan_ppid_settled returns" \
    env _P7_PARENT="${_P7_PARENT:-}" bash -c '
        _poll_pid_gone "$_P7_PARENT" 1
    '

# Tear down the orphaned child.
"$_abs_kill_p7" -9 "${_P7_CHILD:-}" 2>/dev/null || true
wait "$_P7_PARENT" 2>/dev/null || true

test_summary
