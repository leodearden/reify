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
# Block 6: canary timer neutralized in setup-dev.sh (test-6)
#   Grep-the-source, hermetic — no systemctl.  Mirrors the test-5 pattern.
#
#   After alpha, the daemon only creates /tmp/reify-jobserver-{merge,task}.
#   The legacy reify-jobserver-canary.timer still targets /tmp/reify-jobserver
#   (single FIFO), so each 5-min tick would hit the "FIFO absent → reseed →
#   unconditional restart" path, SIGKILLing in-flight rustc and leaking tokens.
#   Neutralizing it here prevents that restart-loop regression until the gamma
#   task rewrites the canary for the dual-FIFO pools.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 6: canary timer neutralized in setup-dev.sh (grep-the-source) ---"

# 6a: NO uncommented 'enable --now' line still lists reify-jobserver-canary.timer
assert "reify-jobserver-canary.timer NOT in 'enable --now' (broken timer removed)" \
    bash -c "! ( grep -Ev '^[[:space:]]*#' '$SETUP_DEV' | grep -F 'enable --now' | grep -qF 'reify-jobserver-canary.timer' )"

# 6b: An uncommented line actively disables the timer (stops already-running timers
#     on hosts provisioned before alpha; a plain drop from 'enable --now' would not
#     stop an already-running timer)
assert "uncommented 'disable --now reify-jobserver-canary.timer' line present" \
    bash -c "grep -Ev '^[[:space:]]*#' '$SETUP_DEV' | grep -qF 'disable --now reify-jobserver-canary.timer'"

# 6c: GUARD — the dual-pool daemon is still auto-enabled (only the canary timer
#     was removed, not the daemon itself)
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

test_summary
