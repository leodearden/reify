#!/usr/bin/env bash
# Tests for scripts/jobserver-canary.sh — dual-pool C2 sum invariant,
# idle-leak reseed, and idle-only guard.
#
# Hermetic: mktemp FIFOs + env overrides + PATH-stubbed systemctl.
# The live /tmp/reify-jobserver-* paths and systemd are NEVER touched.
#
# Blocks:
#   A — C2 sum invariant: skewed-but-sum-correct idle is legitimate (no reseed)
#   B — real leak (sum < nproc) while idle → reseed + sum restored   (step-3/4)
#   C — idle-only guard: a mid-build leak is SKIPPED                  (step-5/6)
#   D — either FIFO missing → unconditional reseed                    (step-7/8)
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

CANARY="$REPO_ROOT/scripts/jobserver-canary.sh"
BALANCER="$REPO_ROOT/scripts/jobserver-balancer.py"

# ──────────────────────────────────────────────────────────────────────────────
# Shared fixture state (populated per block, cleared by _cleanup_canary)
# ──────────────────────────────────────────────────────────────────────────────
_HOLDER_PID=""
_BALANCER_PID=""
_MERGE_FIFO=""
_TASK_FIFO=""
_READY_FILE=""
_STUB_DIR=""
_CALLS_FILE=""
_FIXTURE_TOKENS=4   # all tests use TOKENS=4

# ─── Create stub directory and systemctl stub ─────────────────────────────────
_STUB_DIR="$(mktemp -d /tmp/test-canary-stub-XXXXXX)"
_CALLS_FILE="$(mktemp /tmp/test-canary-calls-XXXXXX)"

cat > "$_STUB_DIR/systemctl" << 'STUB_EOF'
#!/usr/bin/env bash
# Record args to SYSTEMCTL_CALLS file (env var → file path).
echo "$@" >> "${SYSTEMCTL_CALLS:-/dev/null}"
# If REIFY_TEST_SPAWN_BALANCER=1 and this is a 'restart' call, respawn the
# real balancer against the test FIFOs (Block B end-to-end sum-restored check).
if [ "${REIFY_TEST_SPAWN_BALANCER:-0}" = "1" ]; then
    case "$*" in
        *restart*)
            if [ -n "${_REIFY_TEST_BALANCER_SCRIPT:-}" ] && \
               [ -n "${_REIFY_TEST_MERGE_FIFO:-}" ]     && \
               [ -n "${_REIFY_TEST_TASK_FIFO:-}" ]; then
                REIFY_JOBSERVER_MERGE_FIFO="$_REIFY_TEST_MERGE_FIFO" \
                REIFY_JOBSERVER_TASK_FIFO="$_REIFY_TEST_TASK_FIFO" \
                REIFY_JOBSERVER_TOKENS="${REIFY_JOBSERVER_TOKENS:-4}" \
                REIFY_JOBSERVER_POLL_INTERVAL=0.05 \
                    python3 "$_REIFY_TEST_BALANCER_SCRIPT" &
                [ -n "${_REIFY_TEST_BALANCER_PID_FILE:-}" ] \
                    && echo $! > "$_REIFY_TEST_BALANCER_PID_FILE"
            fi
            ;;
    esac
fi
STUB_EOF
chmod +x "$_STUB_DIR/systemctl"

# ──────────────────────────────────────────────────────────────────────────────
# hold_seed_fifos <merge_fifo> <task_fifo> <merge_n> <task_n> <ready_file>
#   Background a python3 holder that:
#     1. os.mkfifo both paths (idempotent if already exist)
#     2. Opens both O_RDWR|O_NONBLOCK
#     3. Writes merge_n/task_n byte-tokens
#     4. Touches ready_file to signal seeding complete
#     5. Sleeps 300 s holding the FDs (FIONREAD is non-destructive)
#   Sets global _HOLDER_PID.
# ──────────────────────────────────────────────────────────────────────────────
hold_seed_fifos() {
    local merge_fifo="$1"
    local task_fifo="$2"
    local merge_n="$3"
    local task_n="$4"
    local ready_file="$5"

    python3 - "$merge_fifo" "$task_fifo" "$merge_n" "$task_n" "$ready_file" <<'PY' &
import os, sys, time

merge_path, task_path = sys.argv[1], sys.argv[2]
merge_n, task_n = int(sys.argv[3]), int(sys.argv[4])
ready_file = sys.argv[5]

for path in (merge_path, task_path):
    try:
        os.mkfifo(path)
    except FileExistsError:
        pass

merge_fd = os.open(merge_path, os.O_RDWR | os.O_NONBLOCK)
task_fd  = os.open(task_path,  os.O_RDWR | os.O_NONBLOCK)

if merge_n > 0:
    os.write(merge_fd, b'\x00' * merge_n)
if task_n > 0:
    os.write(task_fd,  b'\x00' * task_n)

with open(ready_file, 'w') as f:
    f.write('ready')

# Hold FDs open so the pipes stay alive; FIONREAD counts are non-destructive.
time.sleep(300)
PY
    _HOLDER_PID=$!
}

# ──────────────────────────────────────────────────────────────────────────────
# wait_for_ready <ready_file> [timeout_seconds]
#   Poll until ready_file is non-empty (signals holder has seeded both FIFOs).
# ──────────────────────────────────────────────────────────────────────────────
wait_for_ready() {
    local ready_file="$1"
    local timeout="${2:-5}"
    local deadline=$(( $(date +%s) + timeout ))
    while [ ! -s "$ready_file" ]; do
        if [ "$(date +%s)" -ge "$deadline" ]; then
            echo "wait_for_ready: timed out after ${timeout}s" >&2
            return 1
        fi
        sleep 0.05
    done
    return 0
}

# ──────────────────────────────────────────────────────────────────────────────
# wait_for_seed [timeout_seconds]
#   Poll until fionread_sum(_MERGE_FIFO, _TASK_FIFO) == _FIXTURE_TOKENS.
#   Used after a real balancer respawn to confirm it re-seeded both pools.
# ──────────────────────────────────────────────────────────────────────────────
wait_for_seed() {
    local timeout="${1:-10}"
    local deadline=$(( $(date +%s) + timeout ))
    while true; do
        local s
        s="$(fionread_sum "$_MERGE_FIFO" "$_TASK_FIFO" 2>/dev/null || echo -1)"
        [ "$s" -eq "$_FIXTURE_TOKENS" ] && return 0
        if [ "$(date +%s)" -ge "$deadline" ]; then
            echo "wait_for_seed: timed out — sum=$s, want=$_FIXTURE_TOKENS" >&2
            return 1
        fi
        sleep 0.05
    done
}

# ──────────────────────────────────────────────────────────────────────────────
# fionread_sum <merge_fifo> <task_fifo>
#   FIONREAD for both FIFOs in one python3 process (avoids inter-sample races).
#   Reuses the open+ioctl idiom from jobserver-canary.sh:22-31.
#   Prints the integer sum, or -1 if either path is absent/unopenable.
# ──────────────────────────────────────────────────────────────────────────────
fionread_sum() {
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
print(-1 if m < 0 or t < 0 else m + t)
PY
}

# ──────────────────────────────────────────────────────────────────────────────
# run_canary [build_active_cmd]
#   Invoke the canary with all env seams wired for test isolation.
#   Uses fixture globals: _MERGE_FIFO, _TASK_FIFO, _FIXTURE_TOKENS, _CALLS_FILE.
# ──────────────────────────────────────────────────────────────────────────────
run_canary() {
    local build_active_cmd="${1:-echo 0}"
    REIFY_JOBSERVER_MERGE_FIFO="$_MERGE_FIFO" \
    REIFY_JOBSERVER_TASK_FIFO="$_TASK_FIFO" \
    REIFY_JOBSERVER_TOKENS="$_FIXTURE_TOKENS" \
    REIFY_JOBSERVER_BUILD_ACTIVE_CMD="$build_active_cmd" \
    REIFY_JOBSERVER_CANARY_SETTLE_SLEEP=0 \
    SYSTEMCTL_CALLS="$_CALLS_FILE" \
    PATH="$_STUB_DIR:$PATH" \
        bash "$CANARY"
}

# ──────────────────────────────────────────────────────────────────────────────
# _cleanup_canary — kill holder/balancer, remove temp FIFOs and ready-file
# ──────────────────────────────────────────────────────────────────────────────
_cleanup_canary() {
    if [ -n "$_HOLDER_PID" ] && kill -0 "$_HOLDER_PID" 2>/dev/null; then
        kill "$_HOLDER_PID" 2>/dev/null || true
        wait "$_HOLDER_PID" 2>/dev/null || true
    fi
    _HOLDER_PID=""
    if [ -n "$_BALANCER_PID" ] && kill -0 "$_BALANCER_PID" 2>/dev/null; then
        kill "$_BALANCER_PID" 2>/dev/null || true
        wait "$_BALANCER_PID" 2>/dev/null || true
    fi
    _BALANCER_PID=""
    [ -n "$_MERGE_FIFO"  ] && rm -f "$_MERGE_FIFO"  || true
    [ -n "$_TASK_FIFO"   ] && rm -f "$_TASK_FIFO"   || true
    [ -n "$_READY_FILE"  ] && rm -f "$_READY_FILE"  || true
    _MERGE_FIFO=""; _TASK_FIFO=""; _READY_FILE=""
}

_cleanup_all() {
    _cleanup_canary
    [ -n "$_STUB_DIR"   ] && rm -rf "$_STUB_DIR"  || true
    [ -n "$_CALLS_FILE" ] && rm -f  "$_CALLS_FILE" || true
}

trap _cleanup_all EXIT

echo "=== jobserver-canary.sh tests ==="

# ──────────────────────────────────────────────────────────────────────────────
# Block A: C2 sum invariant — skewed-but-sum-correct idle is NOT a leak
#
#   Three sum-correct splits at TOKENS=4 with idle build ('echo 0'):
#     merge=4/task=0, merge=0/task=4, merge=3/task=1.
#   The canary must NOT call systemctl restart for any of these.
#
#   RED: the pre-rewrite canary ignores env seams and reads /tmp/reify-jobserver
#   (which does not exist in the dual-FIFO world); FIFO absent → unconditional
#   reseed → systemctl stub records a restart call → assert fails.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: C2 sum invariant — skewed splits are not leaked ---"

_ba_splits=("4 0" "0 4" "3 1")
for _ba_split in "${_ba_splits[@]}"; do
    _ba_m="${_ba_split%% *}"
    _ba_t="${_ba_split##* }"

    _cleanup_canary
    _MERGE_FIFO="$(mktemp -u /tmp/test-canary-merge-XXXXXX)"
    _TASK_FIFO="$(mktemp -u /tmp/test-canary-task-XXXXXX)"
    _READY_FILE="$(mktemp /tmp/test-canary-ready-XXXXXX)"
    > "$_CALLS_FILE"   # reset call log for this sub-test

    hold_seed_fifos "$_MERGE_FIFO" "$_TASK_FIFO" "$_ba_m" "$_ba_t" "$_READY_FILE"
    wait_for_ready "$_READY_FILE"

    _ba_rc=0
    run_canary 'echo 0' || _ba_rc=$?

    assert "Block A (merge=${_ba_m}/task=${_ba_t}): canary exits 0 (ok path)" \
        test "$_ba_rc" -eq 0

    assert "Block A (merge=${_ba_m}/task=${_ba_t}): no systemctl restart (sum-correct is not a leak)" \
        bash -c "! grep -qF 'restart' '$_CALLS_FILE'"

    _cleanup_canary
done

test_summary
