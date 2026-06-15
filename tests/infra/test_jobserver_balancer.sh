#!/usr/bin/env bash
# Tests for scripts/jobserver-balancer.py — the dual-FIFO custodian daemon
# introduced in task 4515 (jobserver balancer α).
#
# Behavioural tests run HERMETICALLY: mktemp FIFOs + env overrides + a
# backgrounded daemon process this file starts and kills on EXIT.  The live
# /tmp/reify-jobserver-{merge,task} paths and systemctl are NEVER touched.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

BALANCER="$REPO_ROOT/scripts/jobserver-balancer.py"

# ──────────────────────────────────────────────────────────────────────────────
# Shared fixture state (populated by start_balancer, cleared by cleanup trap)
# ──────────────────────────────────────────────────────────────────────────────
_BALANCER_PID=""
_MERGE_FIFO=""
_TASK_FIFO=""
_FIXTURE_TOKENS=""
_PSI_FIXTURE=""

# ──────────────────────────────────────────────────────────────────────────────
# fionread <fifo>
#   Non-destructive FIONREAD token count for a FIFO path.
#   Prints the number of bytes readable (buffered tokens).
#   Prints -1 if the path does not exist or cannot be opened.
#   Reuses the python3 fcntl/termios.FIONREAD idiom from jobserver-canary.sh:22-31.
# ──────────────────────────────────────────────────────────────────────────────
fionread() {
    local path="$1"
    python3 - "$path" <<'PY'
import fcntl, termios, os, struct, sys
try:
    fd = os.open(sys.argv[1], os.O_RDONLY | os.O_NONBLOCK)
except OSError:
    print(-1); raise SystemExit
try:
    print(struct.unpack('i', fcntl.ioctl(fd, termios.FIONREAD, struct.pack('i', 0)))[0])
finally:
    os.close(fd)
PY
}

# ──────────────────────────────────────────────────────────────────────────────
# fionread_pair <fifo1> <fifo2>
#   Read FIONREAD for TWO FIFOs in a SINGLE python3 process.
#   Both reads happen within microseconds of each other, preventing the race
#   where a daemon transfer fires between two separate fionread() shell calls
#   (each of which spawns a ~30ms python3 subprocess).
#   Prints two space-separated integers: "merge_count task_count".
#   Prints "-1 -1" if either path is unavailable.
# ──────────────────────────────────────────────────────────────────────────────
fionread_pair() {
    local merge_path="$1"
    local task_path="$2"
    python3 - "$merge_path" "$task_path" <<'PY'
import fcntl, termios, os, struct, sys

def fr(path):
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
        n = struct.unpack('i', fcntl.ioctl(fd, termios.FIONREAD, struct.pack('i', 0)))[0]
        os.close(fd)
        return n
    except OSError:
        return -1

m = fr(sys.argv[1])
t = fr(sys.argv[2])
print(m, t)
PY
}

# ──────────────────────────────────────────────────────────────────────────────
# write_psi_fixture <path> <avg10>
#   Write a kernel-format /proc/pressure/cpu fixture file so the daemon reads
#   a deterministic pressure value during tests.
#   Format (matches kernel layout):
#     some avg10=<avg10> avg60=0.00 avg300=0.00 total=0
#     full avg10=0.00 avg60=0.00 avg300=0.00 total=0
# ──────────────────────────────────────────────────────────────────────────────
write_psi_fixture() {
    local path="$1"
    local avg10="$2"
    printf 'some avg10=%s avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
        "$avg10" > "$path"
}

# ──────────────────────────────────────────────────────────────────────────────
# read_held_back_file <path>
#   Read the held-back state file (cat → int, default 0 on error/absence).
#   Handles: absent file → 0; garbage content → 0; valid integer → value.
# ──────────────────────────────────────────────────────────────────────────────
read_held_back_file() {
    local path="$1"
    local val
    val="$(cat "$path" 2>/dev/null || true)"
    case "$val" in
        ''|*[!0-9]*) echo 0 ;;
        *) echo "$val" ;;
    esac
}

# ──────────────────────────────────────────────────────────────────────────────
# start_balancer <tokens> <poll_interval_seconds> [psi_fixture_path]
#   Launch the balancer daemon in the background against mktemp FIFOs.
#   Populates _BALANCER_PID, _MERGE_FIFO, _TASK_FIFO, _FIXTURE_TOKENS.
#   NEVER uses the live /tmp/reify-jobserver-* paths or systemctl.
#
#   psi_fixture_path (optional): path to a PSI fixture file to inject via
#     REIFY_JOBSERVER_PSI_PROC_PATH.  When ABSENT, a low-pressure fixture
#     (avg10=0.00) is created automatically and stored in _PSI_FIXTURE so
#     existing Blocks 4/11 stay pure-C4 regardless of real host CPU pressure
#     (prevents flake once step-8 wires PSI-reading into the daemon).
#     When PRESENT, the caller owns the fixture file lifecycle.
# ──────────────────────────────────────────────────────────────────────────────
start_balancer() {
    local tokens="${1:-4}"
    local poll="${2:-0.05}"
    local psi_fixture_arg="${3:-}"

    _MERGE_FIFO="$(mktemp -u /tmp/test-balancer-merge-XXXXXX)"
    _TASK_FIFO="$(mktemp -u /tmp/test-balancer-task-XXXXXX)"
    _FIXTURE_TOKENS="$tokens"

    # PSI fixture: auto-create low-pressure fixture if caller didn't supply one.
    # The env var is ignored by the current daemon (before step-2 implements
    # read_pressure), so Blocks 4/11 pass unchanged; once step-8 wires real PSI
    # reading, the fixture keeps them deterministically at avg10=0.00 → no hold.
    local psi_proc_path
    if [ -n "$psi_fixture_arg" ]; then
        psi_proc_path="$psi_fixture_arg"
        # Caller owns this file; do NOT track in _PSI_FIXTURE for cleanup.
    else
        _PSI_FIXTURE="$(mktemp /tmp/test-balancer-psi-XXXXXX)"
        write_psi_fixture "$_PSI_FIXTURE" "0.00"
        psi_proc_path="$_PSI_FIXTURE"
    fi

    REIFY_JOBSERVER_MERGE_FIFO="$_MERGE_FIFO" \
    REIFY_JOBSERVER_TASK_FIFO="$_TASK_FIFO" \
    REIFY_JOBSERVER_TOKENS="$tokens" \
    REIFY_JOBSERVER_POLL_INTERVAL="$poll" \
    REIFY_JOBSERVER_PSI_PROC_PATH="$psi_proc_path" \
        python3 "$BALANCER" &
    _BALANCER_PID=$!
}

# ──────────────────────────────────────────────────────────────────────────────
# wait_for_seed [timeout_seconds]
#   Poll until FIONREAD(merge) + FIONREAD(task) == _FIXTURE_TOKENS or timeout.
#   Returns 0 on success, 1 on timeout.
# ──────────────────────────────────────────────────────────────────────────────
wait_for_seed() {
    local timeout="${1:-5}"
    local deadline=$(( $(date +%s) + timeout ))
    while true; do
        local m t
        m="$(fionread "$_MERGE_FIFO" 2>/dev/null || echo -1)"
        t="$(fionread "$_TASK_FIFO"  2>/dev/null || echo -1)"
        if [ "$m" -ge 0 ] && [ "$t" -ge 0 ]; then
            local total=$(( m + t ))
            if [ "$total" -eq "$_FIXTURE_TOKENS" ]; then
                return 0
            fi
        fi
        if [ "$(date +%s)" -ge "$deadline" ]; then
            return 1
        fi
        sleep 0.05
    done
}

# ──────────────────────────────────────────────────────────────────────────────
# _cleanup_balancer
#   Kill the background daemon (if running) and remove temp FIFOs.
#   Called by the EXIT trap so every test path is leak-free.
# ──────────────────────────────────────────────────────────────────────────────
_cleanup_balancer() {
    if [ -n "$_BALANCER_PID" ] && kill -0 "$_BALANCER_PID" 2>/dev/null; then
        kill "$_BALANCER_PID" 2>/dev/null || true
        wait "$_BALANCER_PID" 2>/dev/null || true
    fi
    _BALANCER_PID=""
    [ -n "$_MERGE_FIFO"  ] && rm -f "$_MERGE_FIFO"  || true
    [ -n "$_TASK_FIFO"   ] && rm -f "$_TASK_FIFO"   || true
    [ -n "$_PSI_FIXTURE" ] && rm -f "$_PSI_FIXTURE" || true
    _MERGE_FIFO=""
    _TASK_FIFO=""
    _PSI_FIXTURE=""
}

trap _cleanup_balancer EXIT

echo "=== jobserver-balancer.py tests ==="

# ──────────────────────────────────────────────────────────────────────────────
# Block 1: script contract (test-1)
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 1: script contract ---"

assert "scripts/jobserver-balancer.py exists" \
    test -f "$BALANCER"

assert "scripts/jobserver-balancer.py is executable" \
    test -x "$BALANCER"

assert "first line is '#!/usr/bin/env python3'" \
    bash -c "head -1 '$BALANCER' | grep -qxF '#!/usr/bin/env python3'"

# ──────────────────────────────────────────────────────────────────────────────
# Block 2: dual-FIFO seeding + conservation (C1) + custodian (C5) (test-2)
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 2: dual-FIFO seeding + C1 conservation + C5 custodian ---"

_cleanup_balancer   # ensure clean state from any prior run

start_balancer 4 0.05
_B2_SEEDED=0

if wait_for_seed 10; then
    _B2_SEEDED=1
fi

assert "merge FIFO exists as a named pipe" \
    test -p "$_MERGE_FIFO"

assert "task FIFO exists as a named pipe" \
    test -p "$_TASK_FIFO"

assert "daemon seeded both FIFOs within timeout" \
    test "$_B2_SEEDED" -eq 1

# Read token counts from the two pools
_b2_m=$(fionread "$_MERGE_FIFO")
_b2_t=$(fionread "$_TASK_FIFO")

assert "FIONREAD(merge) + FIONREAD(task) == TOKENS immediately after seed" \
    test $(( _b2_m + _b2_t )) -eq "$_FIXTURE_TOKENS"

assert "daemon process is still alive (C5 custodian holds FDs)" \
    kill -0 "$_BALANCER_PID"

# Brief pause (a few poll intervals) then check token conservation again
sleep 0.3

_b2_m2=$(fionread "$_MERGE_FIFO")
_b2_t2=$(fionread "$_TASK_FIFO")

assert "sum still == TOKENS after a few poll intervals (buffered tokens persist)" \
    test $(( _b2_m2 + _b2_t2 )) -eq "$_FIXTURE_TOKENS"

_cleanup_balancer

# ──────────────────────────────────────────────────────────────────────────────
# Block 3: baseline split merge-favored + non-starving (test-3)
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 3: merge-favored baseline split (merge > task, task >= 1) ---"

_cleanup_balancer   # ensure clean state

# Use TOKENS=4 so a strict merge>task inequality is well-defined (3 vs 1)
start_balancer 4 0.05
wait_for_seed 10 || true

_b3_m=$(fionread "$_MERGE_FIFO")
_b3_t=$(fionread "$_TASK_FIFO")

assert "merge pool > task pool (merge-favored baseline)" \
    test "$_b3_m" -gt "$_b3_t"

assert "task pool >= 1 (non-starving — prevents donate-idle thrash)" \
    test "$_b3_t" -ge 1

assert "merge + task == TOKENS (C1 still conserved)" \
    test $(( _b3_m + _b3_t )) -eq "$_FIXTURE_TOKENS"

_cleanup_balancer

# ──────────────────────────────────────────────────────────────────────────────
# Block 4: donate-idle migration via the transfer primitive (test-4)
#   Direction 1 (just-task): task is fully drained by a consumer → merge should
#     migrate its idle tokens to the task pool until task is no longer 0-free.
#     Wait for FIONREAD(task) > 0 within a bounded timeout.
#   Direction 2 (just-merge): merge is fully drained → task migrates to merge.
#     Wait for FIONREAD(merge) > 0 within a bounded timeout.
#
#   Each direction uses fresh daemon + mktemp FIFOs to keep them independent.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 4: donate-idle migration — direction 1 (consumer drains task) ---"

_cleanup_balancer

# Use TOKENS=4, fast poll (0.02 s) for a short migration timeout
start_balancer 4 0.02
wait_for_seed 10 || true

# Drain the task FIFO completely: read all bytes from it in a tight non-blocking
# loop, holding the tokens (never writing back) to simulate rustc holding tokens.
# We use python3 with O_RDONLY|O_NONBLOCK so the read is non-blocking.
_b4_consumer_held_file=$(mktemp)
python3 - "$_TASK_FIFO" "$_b4_consumer_held_file" <<'PY' &
import os, time, sys
path  = sys.argv[1]
count_file = sys.argv[2]
held = 0
# Try to drain the FIFO; keep reading until EAGAIN for up to 3 seconds
fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
deadline = time.monotonic() + 3.0
while time.monotonic() < deadline:
    try:
        data = os.read(fd, 64)
        held += len(data)
    except BlockingIOError:
        if held > 0:
            break
        time.sleep(0.01)
os.close(fd)
with open(count_file, 'w') as f:
    f.write(str(held))
# Hold tokens indefinitely (never write back) to simulate a consumer
time.sleep(30)
PY
_b4_consumer_pid=$!

# Wait for the consumer to finish draining the task pool.
# We do NOT poll FIONREAD(task)==0 from outside: the daemon's donate-idle tick
# refills the drained pool back to 1 within one POLL_INTERVAL (0.02s), so the
# transient 0-free window is far shorter than the bash detector's ~80ms
# effective sample period and would be missed on most runs (flaky).  Instead we
# gate on the consumer's held-count file becoming non-empty: the consumer writes
# its held count AFTER draining to EAGAIN and BEFORE its 30s hold, so a non-empty
# file (held > 0) proves it actually grabbed tokens — the demand signal.
_b4_task_drained=0
_b4_t0=$(date +%s)
while true; do
    if [ -s "$_b4_consumer_held_file" ]; then
        _b4_held_now=$(cat "$_b4_consumer_held_file" 2>/dev/null || echo 0)
        if [ "$_b4_held_now" -gt 0 ]; then
            _b4_task_drained=1; break
        fi
    fi
    [ $(( $(date +%s) - _b4_t0 )) -ge 5 ] && break
    sleep 0.05
done

assert "consumer drained task pool (held > 0, demand signal established)" \
    test "$_b4_task_drained" -eq 1

# Now wait for the balancer to migrate tokens from merge → task
# (donate-idle: merge has free tokens + task has 0 free = live demand)
_b4_migrated_to_task=0
_b4_t0=$(date +%s)
while true; do
    _b4_task_now=$(fionread "$_TASK_FIFO" 2>/dev/null || echo -1)
    if [ "$_b4_task_now" -gt 0 ]; then
        _b4_migrated_to_task=1; break
    fi
    [ $(( $(date +%s) - _b4_t0 )) -ge 10 ] && break
    sleep 0.05
done

assert "balancer migrated tokens from merge → task pool (donate-idle dir-1)" \
    test "$_b4_migrated_to_task" -eq 1

# Conservation: consumer_held + fionread(merge) + fionread(task) == TOKENS
# Use fionread_pair to read BOTH FIFOs in one python3 process, minimising the
# race where a daemon transfer fires between two sequential fionread() calls.
# Additionally retry up to 3 times with a 10 ms gap: C1 is only guaranteed at
# tick boundaries, not mid-transfer (token is in-hand between donor read and
# recipient write), so a single sample in the narrow in-flight window can read
# TOKENS-1.  In steady state (both pools > 0) no transfers fire, so retrying
# quickly eliminates the transient without weakening the invariant.
_b4_conserved=0
for _b4_retry in 1 2 3; do
    _b4_pair=$(fionread_pair "$_MERGE_FIFO" "$_TASK_FIFO")
    _b4_merge_now=$(echo "$_b4_pair" | awk '{print $1}')
    _b4_task_now=$(echo  "$_b4_pair" | awk '{print $2}')
    _b4_held=$(cat "$_b4_consumer_held_file" 2>/dev/null || echo 0)
    if [ $(( _b4_held + _b4_merge_now + _b4_task_now )) -eq "$_FIXTURE_TOKENS" ]; then
        _b4_conserved=1; break
    fi
    sleep 0.01
done

assert "C1: consumer_held + fionread(merge) + fionread(task) == TOKENS (dir-1)" \
    test "$_b4_conserved" -eq 1

# Clean up direction-1
kill "$_b4_consumer_pid" 2>/dev/null || true
wait "$_b4_consumer_pid" 2>/dev/null || true
rm -f "$_b4_consumer_held_file"
_cleanup_balancer

# ── Direction 2: consumer drains MERGE → task should donate to merge ──────────
echo ""
echo "--- Block 4: donate-idle migration — direction 2 (consumer drains merge) ---"

# Use TOKENS=8 (merge_baseline=6, task_baseline=2) so the consumer can drain all
# 6 merge tokens while leaving 2 free task tokens.  After one donation the daemon
# reaches stable state (merge=1, task=1): both pools > 0, no further transfers,
# and fionread(merge) > 0 is a stable post-condition rather than an oscillating
# 0/1 value.  With TOKENS=4 the consumer leaves only 1 free token after drain and
# the daemon bounces it merge↔task every tick (acknowledged α oscillation; C4
# hysteresis is β's scope), making the "migrated to merge" assertion racy.
start_balancer 8 0.02
wait_for_seed 10 || true

_b4_consumer_held_file2=$(mktemp)
python3 - "$_MERGE_FIFO" "$_b4_consumer_held_file2" <<'PY' &
import os, time, sys
path  = sys.argv[1]
count_file = sys.argv[2]
held = 0
fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
deadline = time.monotonic() + 3.0
while time.monotonic() < deadline:
    try:
        data = os.read(fd, 64)
        held += len(data)
    except BlockingIOError:
        if held > 0:
            break
        time.sleep(0.01)
os.close(fd)
with open(count_file, 'w') as f:
    f.write(str(held))
time.sleep(30)
PY
_b4_consumer_pid2=$!

# Wait for consumer to finish draining the merge pool — gate on the held-count
# file (same race fix as dir-1: the transient FIONREAD(merge)==0 state is
# refilled by donate-idle within one poll tick and is not reliably observable).
_b4_merge_drained=0
_b4_t0=$(date +%s)
while true; do
    if [ -s "$_b4_consumer_held_file2" ]; then
        _b4_held_now2=$(cat "$_b4_consumer_held_file2" 2>/dev/null || echo 0)
        if [ "$_b4_held_now2" -gt 0 ]; then
            _b4_merge_drained=1; break
        fi
    fi
    [ $(( $(date +%s) - _b4_t0 )) -ge 5 ] && break
    sleep 0.05
done

assert "consumer drained merge pool (held > 0, demand signal established)" \
    test "$_b4_merge_drained" -eq 1

# Wait for balancer to migrate tokens from task → merge
_b4_migrated_to_merge=0
_b4_t0=$(date +%s)
while true; do
    _b4_merge_now=$(fionread "$_MERGE_FIFO" 2>/dev/null || echo -1)
    if [ "$_b4_merge_now" -gt 0 ]; then
        _b4_migrated_to_merge=1; break
    fi
    [ $(( $(date +%s) - _b4_t0 )) -ge 10 ] && break
    sleep 0.05
done

assert "balancer migrated tokens from task → merge pool (donate-idle dir-2)" \
    test "$_b4_migrated_to_merge" -eq 1

# Conservation — atomic pair read + retry (same race guard as dir-1).
# With TOKENS=8 the steady state is merge=1, task=1, consumer_held=6 and no
# active transfers, so the retry is effectively a no-op; it is kept for
# consistency with dir-1 and as a cheap safety net.
_b4_conserved2=0
for _b4d2_retry in 1 2 3; do
    _b4_pair2=$(fionread_pair "$_MERGE_FIFO" "$_TASK_FIFO")
    _b4_merge_now2=$(echo "$_b4_pair2" | awk '{print $1}')
    _b4_task_now2=$(echo  "$_b4_pair2" | awk '{print $2}')
    _b4_held2=$(cat "$_b4_consumer_held_file2" 2>/dev/null || echo 0)
    if [ $(( _b4_held2 + _b4_merge_now2 + _b4_task_now2 )) -eq "$_FIXTURE_TOKENS" ]; then
        _b4_conserved2=1; break
    fi
    sleep 0.01
done

assert "C1: consumer_held + fionread(merge) + fionread(task) == TOKENS (dir-2)" \
    test "$_b4_conserved2" -eq 1

kill "$_b4_consumer_pid2" 2>/dev/null || true
wait "$_b4_consumer_pid2" 2>/dev/null || true
rm -f "$_b4_consumer_held_file2"
_cleanup_balancer

# ──────────────────────────────────────────────────────────────────────────────
# Block 5: setup-dev.sh reify-jobserver.service unit rewrite (test-5)
#   Grep-the-source validation — no systemctl, no live service restart.
#   Mirrors the test_setup_dev_no_ldconfig.sh pattern.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 5: setup-dev.sh jobserver unit rewrite (grep-the-source) ---"

SETUP_DEV="$REPO_ROOT/scripts/setup-dev.sh"

# 5a: ExecStart runs the daemon via ${repo_dir}/scripts/jobserver-balancer.py
assert "ExecStart references scripts/jobserver-balancer.py (new daemon)" \
    bash -c "grep -Ev '^[[:space:]]*#' '$SETUP_DEV' | grep -qF 'scripts/jobserver-balancer.py'"

# 5b: old single-pool seeder is GONE — no 'sleep infinity' in an uncommented line
#     inside the jobserver unit context
assert "old 'sleep infinity' seeder line is absent (uncommented)" \
    bash -c "! grep -Ev '^[[:space:]]*#' '$SETUP_DEV' | grep -qF 'sleep infinity'"

# 5c: old printf seeder is GONE
assert "old 'printf %%032s' seeder is absent (uncommented)" \
    bash -c "! grep -Ev '^[[:space:]]*#' '$SETUP_DEV' | grep -qF '%%032s'"

# 5d: ExecStopPost removes the MERGE FIFO path
assert "ExecStopPost references reify-jobserver-merge in setup-dev.sh" \
    bash -c "grep -Ev '^[[:space:]]*#' '$SETUP_DEV' | grep -qF 'reify-jobserver-merge'"

# 5e: ExecStopPost removes the TASK FIFO path
assert "ExecStopPost references reify-jobserver-task in setup-dev.sh" \
    bash -c "grep -Ev '^[[:space:]]*#' '$SETUP_DEV' | grep -qF 'reify-jobserver-task'"

# 5f: PartOf=orchestrator-reify.service is retained
assert "PartOf=orchestrator-reify.service is retained" \
    bash -c "grep -Ev '^[[:space:]]*#' '$SETUP_DEV' | grep -qF 'PartOf=orchestrator-reify.service'"

# 5g: Restart=on-failure is retained
assert "Restart=on-failure is retained" \
    bash -c "grep -Ev '^[[:space:]]*#' '$SETUP_DEV' | grep -qF 'Restart=on-failure'"

# 5h: chmod +x includes jobserver-balancer.py
assert "chmod +x line includes jobserver-balancer.py" \
    bash -c "grep -Ev '^[[:space:]]*#' '$SETUP_DEV' | grep -qF 'jobserver-balancer.py'"

# ──────────────────────────────────────────────────────────────────────────────
# Block 6: canary timer re-enabled in setup-dev.sh (test-6, η/4521 lockstep)
#   Grep-the-source, hermetic — no systemctl.  Mirrors the test-5 pattern.
#
#   γ/4517 rewrote the canary for the dual-FIFO pools (jobserver-canary.sh
#   now tracks both /tmp/reify-jobserver-{merge,task}); η/4521 proved the
#   end-to-end acceptance criteria (a)–(d) before landing.  The timer is now
#   safe to re-enable: it is added to the `enable --now` line and the
#   previous `disable --now reify-jobserver-canary.timer` line is removed.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 6: canary timer re-enabled in setup-dev.sh (grep-the-source) ---"

# 6a: An uncommented 'enable --now' line lists reify-jobserver-canary.timer
assert "reify-jobserver-canary.timer in 'enable --now' (γ dual-pool canary live)" \
    bash -c "grep -Ev '^[[:space:]]*#' '$SETUP_DEV' | grep -F 'enable --now' | grep -qF 'reify-jobserver-canary.timer'"

# 6b: NO uncommented 'disable --now reify-jobserver-canary.timer' line remains
assert "no uncommented 'disable --now reify-jobserver-canary.timer' line (stale neutralizer removed)" \
    bash -c "! ( grep -Ev '^[[:space:]]*#' '$SETUP_DEV' | grep -qF 'disable --now reify-jobserver-canary.timer' )"

# 6c: GUARD — the dual-pool daemon is still auto-enabled (timer re-enable
#     must not have accidentally removed the daemon from 'enable --now')
assert "reify-jobserver.service still in 'enable --now' (daemon remains enabled)" \
    bash -c "grep -Ev '^[[:space:]]*#' '$SETUP_DEV' | grep -F 'enable --now' | grep -qF 'reify-jobserver.service'"

# ──────────────────────────────────────────────────────────────────────────────
# Block 7: _transfer_burst unit-tests via importlib heredoc (test-7)
#   Loads jobserver-balancer.py (hyphenated → not importable by name) via
#   importlib.util.spec_from_file_location.  exec_module runs only module-level
#   config (safe defaults), not main().
#
#   Hermetic mktemp FIFOs opened O_RDWR, pre-seeded with known token counts.
#   Each sub-test checks:
#     (a) max_count >= available → all tokens moved, donor empty (stops at EAGAIN)
#     (b) max_count < available  → exactly max_count tokens moved
#     (c) C1 conservation: donor_after + recipient_after == donor_before + recipient_before
#
#   RED before impl: _transfer_burst does not exist → AttributeError → exit 1.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 7: _transfer_burst unit-test ---"

_b7_exit=0
{
python3 - "$BALANCER" <<'PY'
import importlib.util, os, sys, struct, fcntl, termios, tempfile

spec = importlib.util.spec_from_file_location("jb", sys.argv[1])
mod  = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)   # runs module-level config only, not main()

def fionread(fd):
    buf = struct.pack('i', 0)
    return struct.unpack('i', fcntl.ioctl(fd, termios.FIONREAD, buf))[0]

def open_pair():
    """Return (donor_fd, recv_fd, path_a, path_b) using hermetic mktemp FIFOs."""
    pa = tempfile.mktemp(prefix="/tmp/test-burst-a-")
    pb = tempfile.mktemp(prefix="/tmp/test-burst-b-")
    os.mkfifo(pa); os.mkfifo(pb)
    fda = os.open(pa, os.O_RDWR | os.O_NONBLOCK)
    fdb = os.open(pb, os.O_RDWR | os.O_NONBLOCK)
    return fda, fdb, pa, pb

def close_pair(fda, fdb, pa, pb):
    os.close(fda); os.close(fdb)
    os.unlink(pa); os.unlink(pb)

errors = []

# ── (a) max_count >= available: all tokens moved, donor empty ──────────────
fda, fdb, pa, pb = open_pair()
AVAIL_A = 5
os.write(fda, b'+' * AVAIL_A)
before_d = fionread(fda); before_r = fionread(fdb)
moved_a = mod._transfer_burst(fda, fdb, AVAIL_A + 4)  # max_count > available
after_d = fionread(fda);  after_r  = fionread(fdb)
if moved_a != AVAIL_A:
    errors.append(f"(a) moved={moved_a}, want {AVAIL_A}")
if after_d != 0:
    errors.append(f"(a) donor not empty: {after_d}")
if after_r != AVAIL_A:
    errors.append(f"(a) recipient has {after_r}, want {AVAIL_A}")
if before_d + before_r != after_d + after_r:
    errors.append(f"(a) C1: {before_d}+{before_r} != {after_d}+{after_r}")
close_pair(fda, fdb, pa, pb)

# ── (b) max_count < available: exactly max_count tokens moved ─────────────
fda, fdb, pa, pb = open_pair()
AVAIL_B = 6; MAX_B = 3
os.write(fda, b'+' * AVAIL_B)
before_d = fionread(fda); before_r = fionread(fdb)
moved_b = mod._transfer_burst(fda, fdb, MAX_B)
after_d = fionread(fda);  after_r  = fionread(fdb)
if moved_b != MAX_B:
    errors.append(f"(b) moved={moved_b}, want {MAX_B}")
if after_d != AVAIL_B - MAX_B:
    errors.append(f"(b) donor has {after_d}, want {AVAIL_B - MAX_B}")
if after_r != MAX_B:
    errors.append(f"(b) recipient has {after_r}, want {MAX_B}")
if before_d + before_r != after_d + after_r:
    errors.append(f"(b) C1: {before_d}+{before_r} != {after_d}+{after_r}")
close_pair(fda, fdb, pa, pb)

# ── (c) C1 across two successive bursts ──────────────────────────────────
fda, fdb, pa, pb = open_pair()
AVAIL_C = 4
os.write(fda, b'+' * AVAIL_C)
moved_c1 = mod._transfer_burst(fda, fdb, 2)
moved_c2 = mod._transfer_burst(fda, fdb, 2)
final_d  = fionread(fda); final_r = fionread(fdb)
if final_d + final_r != AVAIL_C:
    errors.append(f"(c) C1 across two bursts: {final_d}+{final_r} != {AVAIL_C}")
if moved_c1 + moved_c2 != AVAIL_C:
    errors.append(f"(c) total moved {moved_c1+moved_c2}, want {AVAIL_C}")
close_pair(fda, fdb, pa, pb)

if errors:
    sys.stderr.write("FAIL _transfer_burst:\n" + "\n".join("  " + e for e in errors) + "\n")
    sys.exit(1)
print("OK: _transfer_burst")
PY
} || _b7_exit=$?
assert "_transfer_burst: (a) full burst moves all tokens, donor empty" \
    test "$_b7_exit" -eq 0
assert "_transfer_burst: (b) capped burst moves exactly max_count" \
    test "$_b7_exit" -eq 0
assert "_transfer_burst: (c) C1 conservation holds across all cases" \
    test "$_b7_exit" -eq 0

# ──────────────────────────────────────────────────────────────────────────────
# Block 8: decide() merge-demanded branch + monotonicity invariant (test-8)
#   Loads jobserver-balancer.py via importlib heredoc.
#
#   (a) For each free_task k in 1..tokens-1, with free_merge==0 (merge demanded),
#       decide() must return ("t2m", k) — all task spare moves to merge.
#   (b) MONOTONICITY sweep: for ALL (free_merge=0, free_task=0..tokens),
#       both idle_ticks < threshold AND idle_ticks >= threshold, decide() must
#       NEVER return action "m2t" (merge never donates back while 0-free).
#
#   RED: decide() does not exist yet.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 8: decide() merge-demanded branch + monotonicity ---"

_b8_exit=0
{
python3 - "$BALANCER" <<'PY'
import importlib.util, os, sys

spec = importlib.util.spec_from_file_location("jb", sys.argv[1])
mod  = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

TOKENS = 8
baseline_task  = max(1, TOKENS // 4)   # 2
baseline_merge = TOKENS - baseline_task  # 6
epsilon        = 1
threshold      = 5

errors = []

# ── (a) merge-demanded branch: free_merge==0, free_task=k ─────────────────
for k in range(1, TOKENS):
    action, count = mod.decide(
        free_merge=0, free_task=k,
        tokens=TOKENS, baseline_merge=baseline_merge,
        baseline_task=baseline_task, epsilon=epsilon,
        idle_ticks=0, idle_threshold=threshold,
    )
    if action != "t2m" or count != k:
        errors.append(
            f"(a) free_task={k}: got ({action!r},{count}), want ('t2m',{k})"
        )

# ── (b) MONOTONICITY: free_merge==0 → decide never returns "m2t" ──────────
for free_task in range(0, TOKENS + 1):
    for idle_ticks in [0, threshold - 1, threshold, threshold + 2]:
        action, count = mod.decide(
            free_merge=0, free_task=free_task,
            tokens=TOKENS, baseline_merge=baseline_merge,
            baseline_task=baseline_task, epsilon=epsilon,
            idle_ticks=idle_ticks, idle_threshold=threshold,
        )
        if action == "m2t":
            errors.append(
                f"(b) monotonicity broken: free_merge=0, free_task={free_task}, "
                f"idle_ticks={idle_ticks} → ({action!r},{count})"
            )

if errors:
    sys.stderr.write("FAIL decide() merge-demanded/monotonicity:\n"
                     + "\n".join("  " + e for e in errors) + "\n")
    sys.exit(1)
print("OK: decide() merge-demanded + monotonicity")
PY
} || _b8_exit=$?
assert "decide(): merge-demanded branch returns (t2m, free_task) for all k in 1..tokens-1" \
    test "$_b8_exit" -eq 0
assert "decide(): monotonicity — free_merge==0 never returns action 'm2t'" \
    test "$_b8_exit" -eq 0

# ──────────────────────────────────────────────────────────────────────────────
# Block 9: decide() task-demanded give-back-with-ε branch (test-9)
#   Uses tokens strictly > free_merge so sum < tokens (non-idle state).
#
#   (a) free_task==0, free_merge > epsilon → ("m2t", free_merge - epsilon)
#   (b) free_task==0, free_merge == epsilon → ("none", 0)  [at buffer, no give-back]
#   (c) free_task==0, free_merge == 0 → ("none", 0)        [contention, no give-back]
#   (d) module exposes int EPSILON >= 1
#
#   RED: give-back branch not active; EPSILON constant does not exist.
#   (decide() currently returns ("none",0) for give-back states — step-4 stub.)
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 9: decide() give-back-with-ε branch + EPSILON constant ---"

_b9_exit=0
{
python3 - "$BALANCER" <<'PY'
import importlib.util, os, sys

spec = importlib.util.spec_from_file_location("jb", sys.argv[1])
mod  = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

# Check EPSILON constant
errors = []

# ── (d) module exposes int EPSILON >= 1 ───────────────────────────────────
if not hasattr(mod, 'EPSILON'):
    errors.append("(d) EPSILON constant not found in module")
    sys.stderr.write("FAIL decide() give-back:\n" + "\n".join("  " + e for e in errors) + "\n")
    sys.exit(1)
EPS = mod.EPSILON
if not isinstance(EPS, int):
    errors.append(f"(d) EPSILON is not int: {type(EPS)}")
if EPS < 1:
    errors.append(f"(d) EPSILON < 1: {EPS}")

TOKENS = 8
baseline_task  = max(1, TOKENS // 4)   # 2
baseline_merge = TOKENS - baseline_task  # 6
threshold      = 5
# Use TOKENS=8, consumer holds some tokens so sum < TOKENS (non-idle).
# free_task=0 means task pool is fully drained/held.
# free_merge values chosen so free_task+free_merge < TOKENS (tokens held).
HELD = 2  # simulated tokens held by a task consumer

# ── (a) give-back: free_merge > epsilon ──────────────────────────────────
for m in range(EPS + 1, TOKENS - HELD):
    action, count = mod.decide(
        free_merge=m, free_task=0,
        tokens=TOKENS, baseline_merge=baseline_merge,
        baseline_task=baseline_task, epsilon=EPS,
        idle_ticks=0, idle_threshold=threshold,
    )
    if action != "m2t" or count != m - EPS:
        errors.append(
            f"(a) free_merge={m}: got ({action!r},{count}), want ('m2t',{m - EPS})"
        )

# ── (b) at epsilon: no give-back ─────────────────────────────────────────
action, count = mod.decide(
    free_merge=EPS, free_task=0,
    tokens=TOKENS, baseline_merge=baseline_merge,
    baseline_task=baseline_task, epsilon=EPS,
    idle_ticks=0, idle_threshold=threshold,
)
if action != "none" or count != 0:
    errors.append(f"(b) free_merge=epsilon: got ({action!r},{count}), want ('none',0)")

# ── (c) at zero: no give-back (contention) ────────────────────────────────
action, count = mod.decide(
    free_merge=0, free_task=0,
    tokens=TOKENS, baseline_merge=baseline_merge,
    baseline_task=baseline_task, epsilon=EPS,
    idle_ticks=0, idle_threshold=threshold,
)
if action != "none" or count != 0:
    errors.append(f"(c) free_merge=0,free_task=0: got ({action!r},{count}), want ('none',0)")

if errors:
    sys.stderr.write("FAIL decide() give-back:\n" + "\n".join("  " + e for e in errors) + "\n")
    sys.exit(1)
print("OK: decide() give-back-with-epsilon")
PY
} || _b9_exit=$?
assert "decide(): give-back (a) free_merge>ε → (m2t, free_merge-ε)" \
    test "$_b9_exit" -eq 0
assert "decide(): give-back (b) free_merge==ε → (none,0) — buffer preserved" \
    test "$_b9_exit" -eq 0
assert "decide(): give-back (c) free_merge==0,free_task==0 → (none,0) — contention" \
    test "$_b9_exit" -eq 0
assert "module exposes int EPSILON >= 1" \
    test "$_b9_exit" -eq 0

# ──────────────────────────────────────────────────────────────────────────────
# Block 10: decide() IDLE + reset + contention branches + IDLE_RESET_TICKS (test-10)
#   tokens=8, baseline_merge=6, baseline_task=2, epsilon=1
#
#   (a) idle + window NOT elapsed: decide(free_merge=6,free_task=2,idle_ticks<threshold) == ("none",0)
#   (b) idle + elapsed + merge-heavy skew: decide(free_merge=8,free_task=0,...>=threshold) == ("m2t",2)
#   (c) idle + elapsed + task-heavy skew:  decide(free_merge=2,free_task=6,...>=threshold) == ("t2m",4)
#   (d) idle + elapsed + already at baseline: decide(free_merge=6,free_task=2,...>=threshold) == ("none",0)
#   (e) contention: decide(free_merge=0,free_task=0,...) == ("none",0)
#   (f) module exposes int IDLE_RESET_TICKS >= 1
#
#   RED: IDLE_RESET_TICKS does not exist; idle/baseline-reset not implemented.
#   (decide() currently falls through to branch 4 for sum==tokens cases that
#    don't match merge-demanded / give-back, so (a)+(d) may pass by accident,
#    but (b)+(c) fail since merge-heavy/task-heavy idle states give wrong action.)
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 10: decide() IDLE-reset + contention branches + IDLE_RESET_TICKS ---"

_b10_exit=0
{
python3 - "$BALANCER" <<'PY'
import importlib.util, os, sys

spec = importlib.util.spec_from_file_location("jb", sys.argv[1])
mod  = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

errors = []

# ── (f) module exposes int IDLE_RESET_TICKS >= 1 ─────────────────────────
if not hasattr(mod, 'IDLE_RESET_TICKS'):
    errors.append("(f) IDLE_RESET_TICKS constant not found in module")
    sys.stderr.write("FAIL decide() idle:\n" + "\n".join("  " + e for e in errors) + "\n")
    sys.exit(1)
THRESH = mod.IDLE_RESET_TICKS
if not isinstance(THRESH, int):
    errors.append(f"(f) IDLE_RESET_TICKS is not int: {type(THRESH)}")
if THRESH < 1:
    errors.append(f"(f) IDLE_RESET_TICKS < 1: {THRESH}")

TOKENS = 8
baseline_task  = max(1, TOKENS // 4)   # 2
baseline_merge = TOKENS - baseline_task  # 6
EPS    = 1

def d(fm, ft, it):
    return mod.decide(
        free_merge=fm, free_task=ft,
        tokens=TOKENS, baseline_merge=baseline_merge,
        baseline_task=baseline_task, epsilon=EPS,
        idle_ticks=it, idle_threshold=THRESH,
    )

# ── (a) idle + window NOT elapsed → ("none", 0) ──────────────────────────
action, count = d(baseline_merge, baseline_task, THRESH - 1)
if action != "none" or count != 0:
    errors.append(f"(a) idle+not-elapsed: got ({action!r},{count}), want ('none',0)")

# ── (b) idle + elapsed + merge-heavy skew → ("m2t", 2) ──────────────────
# free_merge=8 > baseline_merge=6 (merge-heavy); free_task=0 (task absent)
# sum = 8+0=8 == TOKENS → IDLE branch
action, count = d(8, 0, THRESH)
if action != "m2t" or count != 8 - baseline_merge:
    errors.append(f"(b) merge-heavy idle reset: got ({action!r},{count}), want ('m2t',{8 - baseline_merge})")

# ── (c) idle + elapsed + task-heavy skew → ("t2m", 4) ───────────────────
# free_merge=2 < baseline_merge=6; free_task=6 > baseline_task=2
# sum = 2+6=8 == TOKENS → IDLE branch
action, count = d(2, 6, THRESH)
if action != "t2m" or count != 6 - baseline_task:
    errors.append(f"(c) task-heavy idle reset: got ({action!r},{count}), want ('t2m',{6 - baseline_task})")

# ── (d) idle + elapsed + already at baseline → ("none", 0) ───────────────
action, count = d(baseline_merge, baseline_task, THRESH)
if action != "none" or count != 0:
    errors.append(f"(d) idle+at-baseline: got ({action!r},{count}), want ('none',0)")

# ── (e) contention (both-0, sum < TOKENS) → ("none", 0) ─────────────────
action, count = d(0, 0, 0)
if action != "none" or count != 0:
    errors.append(f"(e) contention: got ({action!r},{count}), want ('none',0)")

if errors:
    sys.stderr.write("FAIL decide() idle/contention:\n"
                     + "\n".join("  " + e for e in errors) + "\n")
    sys.exit(1)
print("OK: decide() idle/contention branches")
PY
} || _b10_exit=$?
assert "decide(): (a) idle+not-elapsed → (none,0)" \
    test "$_b10_exit" -eq 0
assert "decide(): (b) idle+elapsed+merge-heavy → (m2t, merge-baseline_merge)" \
    test "$_b10_exit" -eq 0
assert "decide(): (c) idle+elapsed+task-heavy → (t2m, task-baseline_task)" \
    test "$_b10_exit" -eq 0
assert "decide(): (d) idle+elapsed+at-baseline → (none,0)" \
    test "$_b10_exit" -eq 0
assert "decide(): (e) contention both-0 → (none,0)" \
    test "$_b10_exit" -eq 0
assert "module exposes int IDLE_RESET_TICKS >= 1" \
    test "$_b10_exit" -eq 0

# ──────────────────────────────────────────────────────────────────────────────
# Block 11: behavioral tests — β-distinguishing live-daemon scenarios (test-11)
#   Reuses start_balancer/wait_for_seed/fionread/fionread_pair/_cleanup_balancer.
#
#   Scenario A — just-task ε-buffer:
#     Consumer drains+holds task pool (0-free demand).  Under C4 give-back,
#     merge settles at EPSILON (not 0).  Under α, merge transfers ONE token
#     then stops at merge=5 (single-shot donate-idle), leaving merge >> EPSILON.
#     → RED under α (merge ≠ EPSILON), GREEN under C4.
#
#   Scenario B — idle baseline-reset:
#     Pools skewed to merge=2, task=6 (task-heavy, sum==TOKENS → idle).
#     Under C4 (small IDLE_RESET_TICKS override) the idle branch resets back
#     to merge-favored baseline (merge=6).  Under α both pools > 0 so neither
#     branch fires — skew persists forever.
#     → RED under α (merge stays 2), GREEN under C4.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 11: behavioral — just-task ε-buffer + idle baseline-reset ---"

# Read EPSILON from the module (default 1; respects env override if set).
_b11_EPSILON=$(python3 - "$BALANCER" <<'PY'
import importlib.util, sys
spec = importlib.util.spec_from_file_location("jb", sys.argv[1])
mod  = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
print(mod.EPSILON)
PY
)

# ── Scenario A: just-task ε-buffer ───────────────────────────────────────
echo ""
echo "  Scenario A: just-task ε-buffer settle"

_cleanup_balancer
start_balancer 8 0.05
wait_for_seed 10 || true

# Consumer drains+holds the task pool (simulates rustc holding all task tokens).
# The consumer reads to EAGAIN, writes held count to a file, then sleeps 30s.
_b11a_held_file=$(mktemp)
python3 - "$_TASK_FIFO" "$_b11a_held_file" <<'PY' &
import os, time, sys
path, count_file = sys.argv[1], sys.argv[2]
held = 0
fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
deadline = time.monotonic() + 5.0
while time.monotonic() < deadline:
    try:
        data = os.read(fd, 64)
        held += len(data)
    except BlockingIOError:
        if held > 0:
            break
        time.sleep(0.01)
os.close(fd)
with open(count_file, 'w') as f:
    f.write(str(held))
time.sleep(30)  # hold tokens indefinitely
PY
_b11a_consumer_pid=$!

# Wait for consumer to establish demand (held > 0, task pool drained).
_b11a_drained=0
_b11a_t0=$(date +%s)
while true; do
    if [ -s "$_b11a_held_file" ]; then
        _b11a_held_now=$(cat "$_b11a_held_file" 2>/dev/null || echo 0)
        if [ "$_b11a_held_now" -gt 0 ]; then
            _b11a_drained=1; break
        fi
    fi
    [ $(( $(date +%s) - _b11a_t0 )) -ge 5 ] && break
    sleep 0.05
done

assert "Scenario A: consumer drained task pool (demand established)" \
    test "$_b11a_drained" -eq 1

# Poll until FIONREAD(merge) == EPSILON (C4 give-back settles the pool).
# Under C4: free_task=0, free_merge>EPSILON → ("m2t", free_merge-EPSILON) burst.
# Under α:  one-token donate-idle fires once (merge=5→task=1), then stops.
_b11a_settled=0
_b11a_t0=$(date +%s)
while true; do
    _b11a_m=$(fionread "$_MERGE_FIFO" 2>/dev/null || echo -1)
    if [ "$_b11a_m" -eq "$_b11_EPSILON" ]; then
        # Confirm stability: no further transfers should fire (branch 3 requires
        # free_merge > EPSILON, which is false once free_merge == EPSILON).
        sleep 0.15
        _b11a_m2=$(fionread "$_MERGE_FIFO" 2>/dev/null || echo -1)
        if [ "$_b11a_m2" -eq "$_b11_EPSILON" ]; then
            _b11a_settled=1; break
        fi
    fi
    [ $(( $(date +%s) - _b11a_t0 )) -ge 10 ] && break
    sleep 0.1
done

assert "Scenario A: FIONREAD(merge) == EPSILON (ε-buffer retained by C4 give-back)" \
    test "$_b11a_settled" -eq 1

# Assert C1 and task reaching TOKENS - EPSILON (three-way conservation).
_b11a_c1_ok=0
_b11a_task_ok=0
for _b11a_retry in 1 2 3; do
    _b11a_pair=$(fionread_pair "$_MERGE_FIFO" "$_TASK_FIFO")
    _b11a_m_f=$(echo "$_b11a_pair" | awk '{print $1}')
    _b11a_t_f=$(echo "$_b11a_pair" | awk '{print $2}')
    _b11a_held_f=$(cat "$_b11a_held_file" 2>/dev/null || echo 0)
    _b11a_sum=$(( _b11a_held_f + _b11a_m_f + _b11a_t_f ))
    _b11a_task_sum=$(( _b11a_held_f + _b11a_t_f ))
    if [ "$_b11a_sum" -eq 8 ]; then
        _b11a_c1_ok=1
    fi
    if [ "$_b11a_task_sum" -eq $(( 8 - _b11_EPSILON )) ]; then
        _b11a_task_ok=1
    fi
    [ "$_b11a_c1_ok" -eq 1 ] && [ "$_b11a_task_ok" -eq 1 ] && break
    sleep 0.01
done

assert "Scenario A: C1 — consumer_held + FIONREAD(merge) + FIONREAD(task) == TOKENS" \
    test "$_b11a_c1_ok" -eq 1
assert "Scenario A: consumer_held + FIONREAD(task) == TOKENS - EPSILON" \
    test "$_b11a_task_ok" -eq 1

kill "$_b11a_consumer_pid" 2>/dev/null || true
wait "$_b11a_consumer_pid" 2>/dev/null || true
rm -f "$_b11a_held_file"
_cleanup_balancer

# ── Scenario B: idle baseline-reset ──────────────────────────────────────
echo ""
echo "  Scenario B: idle baseline-reset"

_cleanup_balancer

# Export small IDLE_RESET_TICKS so the reset fires quickly (3 * 0.05s = 0.15s).
export REIFY_JOBSERVER_IDLE_RESET_TICKS=3
start_balancer 8 0.05
# Unset immediately — the background python3 process already has the value.
unset REIFY_JOBSERVER_IDLE_RESET_TICKS
wait_for_seed 10 || true

# Verify initial seeded baseline: merge=6, task=2 (TOKENS=8, task=max(1,8//4)=2).
_b11b_pair0=$(fionread_pair "$_MERGE_FIFO" "$_TASK_FIFO")
_b11b_m0=$(echo "$_b11b_pair0" | awk '{print $1}')
_b11b_t0_v=$(echo "$_b11b_pair0" | awk '{print $2}')

assert "Scenario B: initial seed is merge-favored (merge > task)" \
    test "$_b11b_m0" -gt "$_b11b_t0_v"

# Skew: read 4 tokens from merge FIFO, write 4 tokens to task FIFO.
# Both pools sum to TOKENS=8 throughout (sum preserved → idle state maintained).
# Result: merge=2, task=6 (task-heavy skew).
_b11b_skew_exit=0
{
python3 - "$_MERGE_FIFO" "$_TASK_FIFO" <<'PY'
import os, sys, time
merge_path, task_path = sys.argv[1], sys.argv[2]
SKEW = 4
mfd = os.open(merge_path, os.O_RDWR | os.O_NONBLOCK)
tfd = os.open(task_path,  os.O_RDWR | os.O_NONBLOCK)
moved = 0
deadline = time.monotonic() + 3.0
while moved < SKEW and time.monotonic() < deadline:
    try:
        n = min(SKEW - moved, 8)
        data = os.read(mfd, n)
        if data:
            # Write immediately to preserve sum invariant
            written = 0
            while written < len(data):
                written += os.write(tfd, data[written:])
            moved += len(data)
    except BlockingIOError:
        time.sleep(0.005)
os.close(mfd); os.close(tfd)
if moved != SKEW:
    import sys as _sys
    _sys.stderr.write(f"skew helper: only moved {moved}/{SKEW} tokens\n")
    _sys.exit(1)
PY
} || _b11b_skew_exit=$?

assert "Scenario B: skew helper moved 4 tokens merge→task" \
    test "$_b11b_skew_exit" -eq 0

# Poll until pools reset to merge-favored baseline (merge==6, merge > task).
# Under C4: idle_ticks reaches IDLE_RESET_TICKS=3 → ("t2m", 4) burst resets.
# Under α:  both pools > 0 → neither donate-idle branch fires → skew persists.
_b11b_reset=0
_b11b_t0=$(date +%s)
while true; do
    _b11b_pair=$(fionread_pair "$_MERGE_FIFO" "$_TASK_FIFO")
    _b11b_m=$(echo "$_b11b_pair" | awk '{print $1}')
    _b11b_t=$(echo "$_b11b_pair" | awk '{print $2}')
    if [ "$_b11b_m" -eq 6 ] && [ "$_b11b_m" -gt "$_b11b_t" ]; then
        _b11b_reset=1; break
    fi
    [ $(( $(date +%s) - _b11b_t0 )) -ge 15 ] && break
    sleep 0.1
done

assert "Scenario B: idle-reset drives pools back to merge-favored baseline (merge==6)" \
    test "$_b11b_reset" -eq 1

_cleanup_balancer

# ──────────────────────────────────────────────────────────────────────────────
# Block 12: env-var validation — EPSILON and IDLE_RESET_TICKS error paths (test-12)
#   Spawns python3 with invalid env values; asserts exit code 1.
#   Module-level guards run before main(), so the process exits immediately
#   without creating or touching any FIFOs (hermetic by construction).
#   Mirrors the TOKENS/POLL_INTERVAL validation discipline (α pattern).
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 12: env-var validation — bad EPSILON + bad IDLE_RESET_TICKS ---"

# REIFY_JOBSERVER_EPSILON=0 → below minimum (< 1), must exit 1
_b12_eps0=0
REIFY_JOBSERVER_EPSILON=0 python3 "$BALANCER" 2>/dev/null || _b12_eps0=$?
assert "REIFY_JOBSERVER_EPSILON=0 exits 1 (below minimum)" \
    test "$_b12_eps0" -eq 1

# REIFY_JOBSERVER_EPSILON=abc → non-integer, must exit 1
_b12_epsabc=0
REIFY_JOBSERVER_EPSILON=abc python3 "$BALANCER" 2>/dev/null || _b12_epsabc=$?
assert "REIFY_JOBSERVER_EPSILON=abc exits 1 (not an integer)" \
    test "$_b12_epsabc" -eq 1

# REIFY_JOBSERVER_IDLE_RESET_TICKS=0 → below minimum (< 1), must exit 1
# (EPSILON defaults to 1, so validation reaches the IDLE_RESET_TICKS guard)
_b12_irt0=0
REIFY_JOBSERVER_IDLE_RESET_TICKS=0 python3 "$BALANCER" 2>/dev/null || _b12_irt0=$?
assert "REIFY_JOBSERVER_IDLE_RESET_TICKS=0 exits 1 (below minimum)" \
    test "$_b12_irt0" -eq 1

# REIFY_JOBSERVER_IDLE_RESET_TICKS=abc → non-integer, must exit 1
_b12_irtabc=0
REIFY_JOBSERVER_IDLE_RESET_TICKS=abc python3 "$BALANCER" 2>/dev/null || _b12_irtabc=$?
assert "REIFY_JOBSERVER_IDLE_RESET_TICKS=abc exits 1 (not an integer)" \
    test "$_b12_irtabc" -eq 1

# ──────────────────────────────────────────────────────────────────────────────
# Block 13: read_pressure() unit test via importlib heredoc (test-13)
#   Mirrors Blocks 7-10 importlib style: load the module, call the pure function.
#
#   (a) PSI fixture with avg10=73.21 → returns float ≈73.21 (within 1e-6).
#   (b) Non-existent path → returns None (fail-open).
#   (c) File present but no 'some' line → returns None (fail-open).
#   (d) Module exposes PSI_PROC_PATH str defaulting to /proc/pressure/cpu.
#
#   RED: read_pressure() and PSI_PROC_PATH do not exist yet → AttributeError.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 13: read_pressure() unit test ---"

_b13_exit=0
{
python3 - "$BALANCER" <<'PY'
import importlib.util, os, sys, tempfile

spec = importlib.util.spec_from_file_location("jb", sys.argv[1])
mod  = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

errors = []

# ── (a) well-formed PSI fixture → float ≈73.21 ────────────────────────────
psi_file = tempfile.mktemp(prefix="/tmp/test-psi-fixture-")
try:
    with open(psi_file, 'w') as f:
        f.write("some avg10=73.21 avg60=12.34 avg300=5.00 total=123456\n")
        f.write("full avg10=0.00 avg60=0.00 avg300=0.00 total=0\n")
    result = mod.read_pressure(psi_file)
    if result is None:
        errors.append("(a) returned None for valid PSI file, want ~73.21")
    elif not isinstance(result, float):
        errors.append(f"(a) returned {type(result).__name__}, want float")
    elif abs(result - 73.21) > 1e-6:
        errors.append(f"(a) returned {result}, want 73.21 (diff={abs(result-73.21)})")
finally:
    try: os.unlink(psi_file)
    except FileNotFoundError: pass

# ── (b) non-existent path → None (fail-open) ─────────────────────────────
result_b = mod.read_pressure("/tmp/does-not-exist-test-psi-99999")
if result_b is not None:
    errors.append(f"(b) non-existent path returned {result_b!r}, want None")

# ── (c) garbage file (no 'some' line) → None (fail-open) ─────────────────
psi_garbage = tempfile.mktemp(prefix="/tmp/test-psi-garbage-")
try:
    with open(psi_garbage, 'w') as f:
        f.write("this is not a psi file\nno some line here\n")
    result_c = mod.read_pressure(psi_garbage)
    if result_c is not None:
        errors.append(f"(c) garbage file returned {result_c!r}, want None")
finally:
    try: os.unlink(psi_garbage)
    except FileNotFoundError: pass

# ── (d) module exposes PSI_PROC_PATH (str), default /proc/pressure/cpu ───
if not hasattr(mod, 'PSI_PROC_PATH'):
    errors.append("(d) PSI_PROC_PATH constant not found in module")
else:
    ppi = mod.PSI_PROC_PATH
    if not isinstance(ppi, str):
        errors.append(f"(d) PSI_PROC_PATH is {type(ppi).__name__}, want str")
    elif ppi != "/proc/pressure/cpu":
        errors.append(f"(d) PSI_PROC_PATH default is {ppi!r}, want '/proc/pressure/cpu'")

if errors:
    sys.stderr.write("FAIL read_pressure():\n" + "\n".join("  " + e for e in errors) + "\n")
    sys.exit(1)
print("OK: read_pressure()")
PY
} || _b13_exit=$?

assert "read_pressure(): (a) valid PSI file → float ~73.21" \
    test "$_b13_exit" -eq 0
assert "read_pressure(): (b) non-existent path → None (fail-open)" \
    test "$_b13_exit" -eq 0
assert "read_pressure(): (c) garbage file (no 'some' line) → None (fail-open)" \
    test "$_b13_exit" -eq 0
assert "module exposes PSI_PROC_PATH str defaulting to /proc/pressure/cpu" \
    test "$_b13_exit" -eq 0

# ──────────────────────────────────────────────────────────────────────────────
# Block 14: pressure_decide() unit test + constants + env-validation (test-14)
#   Pure-function tests via importlib heredoc (Blocks 7-10 / Block 13 style).
#   hold=50, release=40, max_held_back=8 throughout.
#
#   (a) HIGH avg10=99 (>=hold): ("hold", min(free_task, max-held_back)) for
#       valid (free_task>0, headroom>0); headroom==0 or free_task==0 → ("none",0).
#   (b) LOW avg10=10 (<release): ("release", held_back); held_back==0 → ("none",0).
#   (c) BAND 40<=avg10<50: ("none",0) for any held_back.
#   (d) avg10=None (fail-open): acts as low pressure (release held_back); NEVER hold.
#   (e) MERGE-SAFE: inspect.signature(pressure_decide) params do NOT contain 'free_merge'.
#   (f) Constants: PRESSURE_HOLD_THRESHOLD/RELEASE_THRESHOLD floats, release<hold;
#       MAX_HELD_BACK int>=0.
#   (g) Env-validation (Block 12 discipline): bad env → exit 1.
#       Hermeticity guard: each spawn uses timeout + temp FIFO paths (not live).
#   RED: function + constants absent → AttributeError/exit-1 mismatch.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 14: pressure_decide() unit test + constants + env-validation ---"

_b14_exit=0
{
python3 - "$BALANCER" <<'PY'
import importlib.util, os, sys, inspect

spec = importlib.util.spec_from_file_location("jb", sys.argv[1])
mod  = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

errors = []

HOLD = 50.0; RELEASE = 40.0; MAX = 8

def pd(avg10, free_task, held_back, max_hb=MAX):
    return mod.pressure_decide(avg10, HOLD, RELEASE, free_task, held_back, max_hb)

# ── (f) constants exist with correct types ────────────────────────────────
for cname in ('PRESSURE_HOLD_THRESHOLD', 'PRESSURE_RELEASE_THRESHOLD'):
    if not hasattr(mod, cname):
        errors.append(f"(f) {cname} missing")
    elif not isinstance(getattr(mod, cname), float):
        errors.append(f"(f) {cname} is {type(getattr(mod,cname)).__name__}, want float")
if not hasattr(mod, 'MAX_HELD_BACK'):
    errors.append("(f) MAX_HELD_BACK missing")
elif not isinstance(mod.MAX_HELD_BACK, int):
    errors.append(f"(f) MAX_HELD_BACK is {type(mod.MAX_HELD_BACK).__name__}, want int")
elif mod.MAX_HELD_BACK < 0:
    errors.append(f"(f) MAX_HELD_BACK={mod.MAX_HELD_BACK} < 0")
if (hasattr(mod, 'PRESSURE_HOLD_THRESHOLD') and
        hasattr(mod, 'PRESSURE_RELEASE_THRESHOLD') and
        isinstance(mod.PRESSURE_HOLD_THRESHOLD, float) and
        isinstance(mod.PRESSURE_RELEASE_THRESHOLD, float)):
    if mod.PRESSURE_RELEASE_THRESHOLD >= mod.PRESSURE_HOLD_THRESHOLD:
        errors.append(f"(f) release({mod.PRESSURE_RELEASE_THRESHOLD}) >= hold({mod.PRESSURE_HOLD_THRESHOLD})")

# ── (e) MERGE-SAFE: pressure_decide has no 'free_merge' parameter ─────────
try:
    sig = inspect.signature(mod.pressure_decide)
    if 'free_merge' in sig.parameters:
        errors.append("(e) pressure_decide has 'free_merge' param — MERGE-SAFE violated")
except Exception as _e:
    errors.append(f"(e) inspect.signature failed: {_e}")

# ── (a) HIGH pressure (avg10>=hold) — hold behavior ──────────────────────
high_cases = [
    # (free_task, held_back, expected_action, expected_count)
    (5, 3, "hold", min(5, MAX - 3)),   # headroom=5, grab min(5,5)=5
    (3, 5, "hold", min(3, MAX - 5)),   # headroom=3, grab min(3,3)=3
    (2, 7, "hold", min(2, MAX - 7)),   # headroom=1, grab min(2,1)=1
    (0, 3, "none", 0),                  # free_task=0 → no grab possible
    (5, 8, "none", 0),                  # headroom=0 (max reached) → none
    (0, 8, "none", 0),                  # both 0 and max → none
]
for (ft, hb, exp_a, exp_c) in high_cases:
    action, count = pd(99.0, ft, hb)
    if (action, count) != (exp_a, exp_c):
        errors.append(
            f"(a) HIGH free_task={ft},held={hb}: got ({action!r},{count}), "
            f"want ({exp_a!r},{exp_c})"
        )

# ── (b) LOW pressure (avg10<release) — release behavior ──────────────────
low_cases = [
    (5, 3, "release", 3),
    (5, 5, "release", 5),
    (5, 0, "none",    0),   # nothing to release
    (0, 3, "release", 3),   # free_task irrelevant for release
]
for (ft, hb, exp_a, exp_c) in low_cases:
    action, count = pd(10.0, ft, hb)
    if (action, count) != (exp_a, exp_c):
        errors.append(
            f"(b) LOW free_task={ft},held={hb}: got ({action!r},{count}), "
            f"want ({exp_a!r},{exp_c})"
        )

# ── (c) BAND (release<=avg10<hold) — no action ───────────────────────────
for avg10 in (40.0, 44.5, 49.0):
    for hb in (0, 3, 8):
        action, count = pd(avg10, 5, hb)
        if (action, count) != ("none", 0):
            errors.append(
                f"(c) BAND avg10={avg10},held={hb}: got ({action!r},{count}), "
                f"want ('none',0)"
            )

# ── (d) avg10=None (fail-open) — release behavior, never hold ────────────
none_cases = [
    (5, 3, "release", 3),
    (0, 5, "release", 5),
    (5, 0, "none",    0),
]
for (ft, hb, exp_a, exp_c) in none_cases:
    action, count = pd(None, ft, hb)
    if (action, count) != (exp_a, exp_c):
        errors.append(
            f"(d) None free_task={ft},held={hb}: got ({action!r},{count}), "
            f"want ({exp_a!r},{exp_c})"
        )
    if action == "hold":
        errors.append(f"(d) None returned 'hold' — NEVER hold on fail-open")

if errors:
    sys.stderr.write("FAIL pressure_decide():\n" + "\n".join("  " + e for e in errors) + "\n")
    sys.exit(1)
print("OK: pressure_decide()")
PY
} || _b14_exit=$?

assert "pressure_decide(): (a) HIGH pressure hold behavior" \
    test "$_b14_exit" -eq 0
assert "pressure_decide(): (b) LOW pressure release behavior" \
    test "$_b14_exit" -eq 0
assert "pressure_decide(): (c) BAND → none" \
    test "$_b14_exit" -eq 0
assert "pressure_decide(): (d) None fail-open (never hold)" \
    test "$_b14_exit" -eq 0
assert "pressure_decide(): (e) MERGE-SAFE (no free_merge param)" \
    test "$_b14_exit" -eq 0
assert "pressure_decide(): (f) constants (HOLD/RELEASE threshold floats, MAX_HELD_BACK int>=0)" \
    test "$_b14_exit" -eq 0

# ── (g) env-validation (Block 12 discipline) ─────────────────────────────
# Hermeticity guard: temp FIFO paths + timeout so the script can't fall through
# to main() and block on creating/opening live FIFOs in the RED state.
_b14_tmp_merge="$(mktemp -u /tmp/test-b14-merge-XXXXXX)"
_b14_tmp_task="$(mktemp -u /tmp/test-b14-task-XXXXXX)"

# bad float for HOLD_THRESHOLD
_b14_hold_abc=0
REIFY_JOBSERVER_MERGE_FIFO="$_b14_tmp_merge" \
REIFY_JOBSERVER_TASK_FIFO="$_b14_tmp_task" \
REIFY_JOBSERVER_PRESSURE_HOLD_THRESHOLD=abc \
    timeout 5 python3 "$BALANCER" 2>/dev/null || _b14_hold_abc=$?
assert "REIFY_JOBSERVER_PRESSURE_HOLD_THRESHOLD=abc exits 1 (not a float)" \
    test "$_b14_hold_abc" -eq 1

# release >= hold (50 == 50 → release not < hold)
_b14_rel_ge=0
REIFY_JOBSERVER_MERGE_FIFO="$_b14_tmp_merge" \
REIFY_JOBSERVER_TASK_FIFO="$_b14_tmp_task" \
REIFY_JOBSERVER_PRESSURE_HOLD_THRESHOLD=50.0 \
REIFY_JOBSERVER_PRESSURE_RELEASE_THRESHOLD=50.0 \
    timeout 5 python3 "$BALANCER" 2>/dev/null || _b14_rel_ge=$?
assert "REIFY_JOBSERVER_PRESSURE_RELEASE_THRESHOLD=50 (>=hold=50) exits 1" \
    test "$_b14_rel_ge" -eq 1

# negative MAX_HELD_BACK
_b14_max_neg=0
REIFY_JOBSERVER_MERGE_FIFO="$_b14_tmp_merge" \
REIFY_JOBSERVER_TASK_FIFO="$_b14_tmp_task" \
REIFY_JOBSERVER_MAX_HELD_BACK=-1 \
    timeout 5 python3 "$BALANCER" 2>/dev/null || _b14_max_neg=$?
assert "REIFY_JOBSERVER_MAX_HELD_BACK=-1 exits 1 (negative)" \
    test "$_b14_max_neg" -eq 1

# Cleanup temp paths (in case RED daemon created them before timeout)
rm -f "$_b14_tmp_merge" "$_b14_tmp_task"

# ──────────────────────────────────────────────────────────────────────────────
# Block 15: suppress_giveback() unit test via importlib heredoc (test-15)
#   Pure-function test: suppress_giveback(avg10, release_threshold, held_back) -> bool.
#
#   Suppression logic (prevents merge→task give-back from refilling a drained
#   task pool while the reservoir has tokens, which would be immediately clawed
#   back → back-door merge drain):
#
#   (a) avg10 >= release_threshold, held_back=0 → True (pressure active)
#   (b) avg10 < release_threshold,  held_back=0 → False (pressure gone, no reservoir)
#   (c) avg10 < release_threshold,  held_back>0 → True (reservoir non-empty keeps suppression)
#   (d) avg10=None, held_back=0 → False (fail-open: no suppression without reservoir)
#   (e) avg10=None, held_back>0 → True (reservoir non-empty overrides fail-open)
#
#   RED: suppress_giveback does not exist → AttributeError → exit 1.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 15: suppress_giveback() unit test ---"

_b15_exit=0
{
python3 - "$BALANCER" <<'PY'
import importlib.util, sys

spec = importlib.util.spec_from_file_location("jb", sys.argv[1])
mod  = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

errors = []
RELEASE = 40.0

def sg(avg10, held_back):
    return mod.suppress_giveback(avg10, RELEASE, held_back)

# ── (a) avg10 >= release, held_back=0 → True (pressure active) ────────────
for avg10 in (40.0, 45.0, 99.0, 50.0):
    result = sg(avg10, 0)
    if result is not True:
        errors.append(f"(a) avg10={avg10},held=0: got {result!r}, want True")

# ── (b) avg10 < release, held_back=0 → False (no pressure, no reservoir) ──
for avg10 in (0.0, 10.0, 39.9):
    result = sg(avg10, 0)
    if result is not False:
        errors.append(f"(b) avg10={avg10},held=0: got {result!r}, want False")

# ── (c) avg10 < release, held_back>0 → True (reservoir guards merge refill) ─
for avg10 in (0.0, 10.0, 39.9):
    for hb in (1, 3, 8):
        result = sg(avg10, hb)
        if result is not True:
            errors.append(f"(c) avg10={avg10},held={hb}: got {result!r}, want True")

# ── (d) avg10=None, held_back=0 → False (fail-open, no suppression) ───────
result = sg(None, 0)
if result is not False:
    errors.append(f"(d) avg10=None,held=0: got {result!r}, want False")

# ── (e) avg10=None, held_back>0 → True (reservoir non-empty suppresses) ───
for hb in (1, 3, 8):
    result = sg(None, hb)
    if result is not True:
        errors.append(f"(e) avg10=None,held={hb}: got {result!r}, want True")

if errors:
    sys.stderr.write("FAIL suppress_giveback():\n" + "\n".join("  " + e for e in errors) + "\n")
    sys.exit(1)
print("OK: suppress_giveback()")
PY
} || _b15_exit=$?

assert "suppress_giveback(): (a) avg10>=release, held=0 → True (pressure active)" \
    test "$_b15_exit" -eq 0
assert "suppress_giveback(): (b) avg10<release, held=0 → False (pressure gone)" \
    test "$_b15_exit" -eq 0
assert "suppress_giveback(): (c) avg10<release, held>0 → True (reservoir guards)" \
    test "$_b15_exit" -eq 0
assert "suppress_giveback(): (d) avg10=None, held=0 → False (fail-open)" \
    test "$_b15_exit" -eq 0
assert "suppress_giveback(): (e) avg10=None, held>0 → True (reservoir suppresses)" \
    test "$_b15_exit" -eq 0

# ──────────────────────────────────────────────────────────────────────────────
# Block 16: live-daemon pressure lifecycle (test-16)
#   Uses pre-1 helpers: write_psi_fixture, read_held_back_file, start_balancer
#   with PSI fixture arg.  TOKENS=8 (task_baseline=2, merge_baseline=6,
#   MAX_HELD_BACK=max(1,8//4)=2).
#
#   Scenario A: hold → release lifecycle
#     HIGH-pressure fixture (avg10=99):
#       (1) held-back file > 0 — pressure stage grabbed task tokens
#       (2) FIONREAD(merge)==merge_baseline — MERGE NOT STRANGLED
#       (3) C1: FIONREAD(merge)+FIONREAD(task)+held_back==TOKENS
#     Switch to LOW-pressure fixture (avg10=0.00):
#       (4) held-back→0 and task refills to task_baseline
#
#   Scenario B: give-back suppression
#     HIGH pressure + drained task pool:
#       (5) FIONREAD(merge) stays above EPSILON (C4 m2t blocked)
#
#   RED: no pressure stage in main() → state file never written → (1) times out;
#        m2t fires freely under HIGH pressure → merge drops to EPSILON → (5) fails.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 16: pressure lifecycle (hold→release + give-back suppression) ---"

_b16_TOKENS=8
_b16_psi_fixture="$(mktemp /tmp/test-b16-psi-XXXXXX)"
_b16_held_back_file="$(mktemp /tmp/test-b16-held-back-XXXXXX)"
_b16_TASK_BL=$(( _b16_TOKENS / 4 ))           # max(1,8//4)=2
_b16_MERGE_BL=$(( _b16_TOKENS - _b16_TASK_BL ))  # 6

_b16_EPSILON=$(python3 - "$BALANCER" <<'PY'
import importlib.util, sys
spec = importlib.util.spec_from_file_location("jb", sys.argv[1])
mod  = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
print(mod.EPSILON)
PY
)

# ── Scenario A: hold → release lifecycle ─────────────────────────────────
echo ""
echo "  Scenario A: hold → release"

write_psi_fixture "$_b16_psi_fixture" "99.00"
_cleanup_balancer

export REIFY_JOBSERVER_HELD_BACK_FILE="$_b16_held_back_file"
start_balancer "$_b16_TOKENS" 0.02 "$_b16_psi_fixture"
unset REIFY_JOBSERVER_HELD_BACK_FILE
wait_for_seed 10 || true

# (1) Poll until held-back file > 0 (pressure stage actively holding tokens)
_b16a_nonzero=0
_b16a_t0=$(date +%s)
while true; do
    _b16a_hb=$(read_held_back_file "$_b16_held_back_file")
    [ "$_b16a_hb" -gt 0 ] && { _b16a_nonzero=1; break; }
    [ $(( $(date +%s) - _b16a_t0 )) -ge 5 ] && break
    sleep 0.05
done

assert "Scenario A: (1) held-back > 0 under HIGH pressure (pressure stage active)" \
    test "$_b16a_nonzero" -eq 1

# (2) FIONREAD(merge)==merge_baseline — MERGE NOT STRANGLED
_b16a_m=$(fionread "$_MERGE_FIFO" 2>/dev/null || echo -1)
assert "Scenario A: (2) FIONREAD(merge)==merge_baseline (MERGE NOT STRANGLED)" \
    test "$_b16a_m" -eq "$_b16_MERGE_BL"

# (3) C1 extended: FIONREAD(merge)+FIONREAD(task)+held_back==TOKENS
_b16a_c1=0
for _b16a_r in 1 2 3; do
    _b16a_pair=$(fionread_pair "$_MERGE_FIFO" "$_TASK_FIFO")
    _b16a_mf=$(echo "$_b16a_pair" | awk '{print $1}')
    _b16a_tf=$(echo "$_b16a_pair" | awk '{print $2}')
    _b16a_hb=$(read_held_back_file "$_b16_held_back_file")
    [ $(( _b16a_mf + _b16a_tf + _b16a_hb )) -eq "$_b16_TOKENS" ] && { _b16a_c1=1; break; }
    sleep 0.05
done
assert "Scenario A: (3) C1 — FIONREAD(merge)+FIONREAD(task)+held_back==TOKENS" \
    test "$_b16a_c1" -eq 1

# Switch to LOW pressure; (4) poll until held-back→0 and task refills
write_psi_fixture "$_b16_psi_fixture" "0.00"

_b16a_rel=0
_b16a_t0=$(date +%s)
while true; do
    _b16a_hb=$(read_held_back_file "$_b16_held_back_file")
    _b16a_tf=$(fionread "$_TASK_FIFO" 2>/dev/null || echo -1)
    [ "$_b16a_hb" -eq 0 ] && [ "$_b16a_tf" -ge "$_b16_TASK_BL" ] && { _b16a_rel=1; break; }
    [ $(( $(date +%s) - _b16a_t0 )) -ge 10 ] && break
    sleep 0.05
done
assert "Scenario A: (4) held-back→0 and task refills to baseline after LOW pressure" \
    test "$_b16a_rel" -eq 1

_cleanup_balancer

# ── Scenario B: give-back suppression under HIGH pressure ─────────────────
echo ""
echo "  Scenario B: give-back suppression"

write_psi_fixture "$_b16_psi_fixture" "99.00"
_cleanup_balancer
> "$_b16_held_back_file"  # reset held-back file

export REIFY_JOBSERVER_HELD_BACK_FILE="$_b16_held_back_file"
start_balancer "$_b16_TOKENS" 0.05 "$_b16_psi_fixture"
unset REIFY_JOBSERVER_HELD_BACK_FILE
wait_for_seed 10 || true

# Consumer drains+holds task pool.  Under HIGH pressure (GREEN), pressure stage
# may grab task tokens first (consumer gets 0).  Either way free_task→0.
_b16b_held_file=$(mktemp /tmp/test-b16b-held-XXXXXX)
python3 - "$_TASK_FIFO" "$_b16b_held_file" <<'PY' &
import os, time, sys
path, count_file = sys.argv[1], sys.argv[2]
held = 0
fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
deadline = time.monotonic() + 1.5
while time.monotonic() < deadline:
    try:
        data = os.read(fd, 64); held += len(data)
    except BlockingIOError:
        if held > 0: break
        time.sleep(0.01)
os.close(fd)
with open(count_file, 'w') as f: f.write(str(held))
time.sleep(30)
PY
_b16b_consumer_pid=$!

# (5) Wait for consumer drain attempt to finish (file written = drain done)
_b16b_t0=$(date +%s)
while [ ! -s "$_b16b_held_file" ]; do
    [ $(( $(date +%s) - _b16b_t0 )) -ge 5 ] && break
    sleep 0.05
done
assert "Scenario B: (5a) consumer drain attempt finished (file written)" \
    test -s "$_b16b_held_file"

# (5b) Poll 1s: assert FIONREAD(merge) NEVER drops to EPSILON.
#   RED: m2t fires (no suppression) → merge→EPSILON → FAIL.
#   GREEN: suppress_giveback active → m2t blocked → merge stays at merge_baseline.
_b16b_supp=1
_b16b_t0=$(date +%s)
while true; do
    _b16b_m=$(fionread "$_MERGE_FIFO" 2>/dev/null || echo -1)
    [ "$_b16b_m" -le "$_b16_EPSILON" ] && { _b16b_supp=0; break; }
    [ $(( $(date +%s) - _b16b_t0 )) -ge 1 ] && break
    sleep 0.05
done
assert "Scenario B: (5b) FIONREAD(merge) stays above EPSILON (give-back suppressed)" \
    test "$_b16b_supp" -eq 1

kill "$_b16b_consumer_pid" 2>/dev/null || true
wait "$_b16b_consumer_pid" 2>/dev/null || true
rm -f "$_b16b_held_file"
_cleanup_balancer
rm -f "$_b16_psi_fixture" "$_b16_held_back_file"

# ──────────────────────────────────────────────────────────────────────────────
# Block 17: setup-dev.sh reify-jobserver.service held-back state file cleanup
#   Grep-the-source assertions (Block 5/6 pattern — hermetic, no systemctl).
#
#   (a) An uncommented ExecStartPre line in the reify-jobserver unit rm's
#       /tmp/reify-jobserver-held-back (cleans stale reservoir on restart).
#   (b) An uncommented ExecStopPost line rm's /tmp/reify-jobserver-held-back
#       (clean shutdown; stale count must not mask a real leak on next start).
#   (c) The reify-jobserver.service Description (or an adjacent comment) mentions
#       "pressure-reactive" or "load-aware" admission.
#
#   GUARD: the addition must not have clobbered the existing lines:
#     (d) ExecStart=jobserver-balancer.py still present
#     (e) ExecStopPost still references reify-jobserver-merge (orig FIFO)
#     (f) ExecStopPost still references reify-jobserver-task (orig FIFO)
#
#   RED: setup-dev.sh not yet updated → (a)/(b)/(c) fail.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 17: setup-dev.sh: ExecStartPre/StopPost rm held-back + Description ---"

SETUP_DEV="$REPO_ROOT/scripts/setup-dev.sh"

# 17a: an uncommented ExecStartPre line rm's reify-jobserver-held-back
assert "17a: ExecStartPre rm's reify-jobserver-held-back (stale cleanup on restart)" \
    bash -c "grep -Ev '^[[:space:]]*#' '$SETUP_DEV' | grep -F 'ExecStartPre' | grep -qF 'reify-jobserver-held-back'"

# 17b: an uncommented ExecStopPost line rm's reify-jobserver-held-back
assert "17b: ExecStopPost rm's reify-jobserver-held-back (clean shutdown)" \
    bash -c "grep -Ev '^[[:space:]]*#' '$SETUP_DEV' | grep -F 'ExecStopPost' | grep -qF 'reify-jobserver-held-back'"

# 17c: Description (or comment near the unit) mentions pressure-reactive or load-aware
assert "17c: unit Description mentions pressure-reactive or load-aware admission" \
    bash -c "grep -Ev '^[[:space:]]*#' '$SETUP_DEV' | grep -iE 'pressure.reactive|load.aware'"

# 17d: GUARD — ExecStart=jobserver-balancer.py not clobbered
assert "17d: GUARD — ExecStart still references scripts/jobserver-balancer.py" \
    bash -c "grep -Ev '^[[:space:]]*#' '$SETUP_DEV' | grep -qF 'scripts/jobserver-balancer.py'"

# 17e: GUARD — ExecStopPost reify-jobserver-merge line still present
assert "17e: GUARD — ExecStopPost still references reify-jobserver-merge (orig FIFO)" \
    bash -c "grep -Ev '^[[:space:]]*#' '$SETUP_DEV' | grep -F 'ExecStopPost' | grep -qF 'reify-jobserver-merge'"

# 17f: GUARD — ExecStopPost reify-jobserver-task line still present
assert "17f: GUARD — ExecStopPost still references reify-jobserver-task (orig FIFO)" \
    bash -c "grep -Ev '^[[:space:]]*#' '$SETUP_DEV' | grep -F 'ExecStopPost' | grep -qF 'reify-jobserver-task'"

test_summary
