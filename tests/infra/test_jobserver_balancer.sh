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

# (More assertion blocks are appended by subsequent TDD steps.)

test_summary
