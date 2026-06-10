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
# start_balancer <tokens> <poll_interval_seconds>
#   Launch the balancer daemon in the background against mktemp FIFOs.
#   Populates _BALANCER_PID, _MERGE_FIFO, _TASK_FIFO, _FIXTURE_TOKENS.
#   NEVER uses the live /tmp/reify-jobserver-* paths or systemctl.
# ──────────────────────────────────────────────────────────────────────────────
start_balancer() {
    local tokens="${1:-4}"
    local poll="${2:-0.05}"

    _MERGE_FIFO="$(mktemp -u /tmp/test-balancer-merge-XXXXXX)"
    _TASK_FIFO="$(mktemp -u /tmp/test-balancer-task-XXXXXX)"
    _FIXTURE_TOKENS="$tokens"

    REIFY_JOBSERVER_MERGE_FIFO="$_MERGE_FIFO" \
    REIFY_JOBSERVER_TASK_FIFO="$_TASK_FIFO" \
    REIFY_JOBSERVER_TOKENS="$tokens" \
    REIFY_JOBSERVER_POLL_INTERVAL="$poll" \
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
    [ -n "$_MERGE_FIFO" ] && rm -f "$_MERGE_FIFO" || true
    [ -n "$_TASK_FIFO"  ] && rm -f "$_TASK_FIFO"  || true
    _MERGE_FIFO=""
    _TASK_FIFO=""
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

# Wait for the consumer to drain the task pool (FIONREAD(task) == 0)
_b4_task_drained=0
_b4_t0=$(date +%s)
while true; do
    _b4_t_now=$(fionread "$_TASK_FIFO" 2>/dev/null || echo -1)
    if [ "$_b4_t_now" -eq 0 ]; then
        _b4_task_drained=1; break
    fi
    [ $(( $(date +%s) - _b4_t0 )) -ge 5 ] && break
    sleep 0.05
done

assert "consumer drained task pool to 0 (demand signal established)" \
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
_b4_merge_now=$(fionread "$_MERGE_FIFO")
_b4_task_now=$(fionread "$_TASK_FIFO")
_b4_held=$(cat "$_b4_consumer_held_file" 2>/dev/null || echo 0)

assert "C1: consumer_held + fionread(merge) + fionread(task) == TOKENS (dir-1)" \
    test $(( _b4_held + _b4_merge_now + _b4_task_now )) -eq "$_FIXTURE_TOKENS"

# Clean up direction-1
kill "$_b4_consumer_pid" 2>/dev/null || true
wait "$_b4_consumer_pid" 2>/dev/null || true
rm -f "$_b4_consumer_held_file"
_cleanup_balancer

# ── Direction 2: consumer drains MERGE → task should donate to merge ──────────
echo ""
echo "--- Block 4: donate-idle migration — direction 2 (consumer drains merge) ---"

start_balancer 4 0.02
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

# Wait for consumer to drain merge pool
_b4_merge_drained=0
_b4_t0=$(date +%s)
while true; do
    _b4_m_now=$(fionread "$_MERGE_FIFO" 2>/dev/null || echo -1)
    if [ "$_b4_m_now" -eq 0 ]; then
        _b4_merge_drained=1; break
    fi
    [ $(( $(date +%s) - _b4_t0 )) -ge 5 ] && break
    sleep 0.05
done

assert "consumer drained merge pool to 0 (demand signal established)" \
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

# Conservation
_b4_merge_now2=$(fionread "$_MERGE_FIFO")
_b4_task_now2=$(fionread "$_TASK_FIFO")
_b4_held2=$(cat "$_b4_consumer_held_file2" 2>/dev/null || echo 0)

assert "C1: consumer_held + fionread(merge) + fionread(task) == TOKENS (dir-2)" \
    test $(( _b4_held2 + _b4_merge_now2 + _b4_task_now2 )) -eq "$_FIXTURE_TOKENS"

kill "$_b4_consumer_pid2" 2>/dev/null || true
wait "$_b4_consumer_pid2" 2>/dev/null || true
rm -f "$_b4_consumer_held_file2"
_cleanup_balancer

# (More assertion blocks are appended by subsequent TDD steps.)

test_summary
