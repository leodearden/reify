#!/usr/bin/env bash
# jobserver-canary.sh — guard against cargo-jobserver token depletion.
#
# C2 sum invariant: when the build is IDLE,
#   FIONREAD(merge) + FIONREAD(task) + held_back
# must equal nproc (the total seeded by reify-jobserver.service). A sum less
# than nproc while idle means tokens have leaked; restarting the service
# re-seeds both pools to nproc.
#
# held_back is the pressure-reservoir count published by the balancer daemon
# to ${REIFY_JOBSERVER_HELD_BACK_FILE:-/tmp/reify-jobserver-held-back}. A
# non-zero reservoir means the balancer is intentionally withholding task-pool
# tokens under CPU pressure — not a leak. Absent or garbage file → 0 (safe
# default; a dead daemon has no FIFOs, so the FIFO-missing branch reseeds
# before this check runs, bounding stale-file risk to the alive-daemon case).
#
# The canary checks only the SUM — per-pool splits are irrelevant. A skewed-
# but-sum-correct idle state (e.g. 32/0 after a merge burst) is LEGITIMATE:
# the balancer's give-back/ratchet corrects it on its own.
# Only sum+held_back < nproc while idle is a real token leak (PRD §4 C2, §8 T-a).
#
# Run periodically by reify-jobserver-canary.timer.
set -uo pipefail

MERGE_FIFO=${REIFY_JOBSERVER_MERGE_FIFO:-/tmp/reify-jobserver-merge}
TASK_FIFO=${REIFY_JOBSERVER_TASK_FIFO:-/tmp/reify-jobserver-task}
SEEDED=${REIFY_JOBSERVER_TOKENS:-$(python3 -c 'import os;print(len(os.sched_getaffinity(0)))')}
HELD_BACK_FILE=${REIFY_JOBSERVER_HELD_BACK_FILE:-/tmp/reify-jobserver-held-back}
SVC=reify-jobserver.service

# read_held_back — read the balancer's pressure-reservoir count.
# Returns 0 on absent file, empty file, or non-integer content (fail-safe).
read_held_back() {
    local _v
    _v="$(cat "$HELD_BACK_FILE" 2>/dev/null)" || true
    case "${_v:-}" in
        ''|*[!0-9]*) echo 0 ;;   # absent, empty, or garbage → 0
        *) echo "$_v" ;;
    esac
}

tokens_sum() {  # FIONREAD sum for both FIFOs in one python3 process; -1 if either absent
  python3 - "$MERGE_FIFO" "$TASK_FIFO" <<'PY'
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

build_active() {  # count live cargo/compiler/linker procs (exclude stopped/zombie)
  if [ -n "${REIFY_JOBSERVER_BUILD_ACTIVE_CMD:-}" ]; then
    bash -c "$REIFY_JOBSERVER_BUILD_ACTIVE_CMD"
  else
    ps -eo stat,comm | awk '
      $2 ~ /^(cargo|cargo-nextest|rustc|cc1|cc1plus|rust-lld|lld|lto)$/ && $1 !~ /[TZ]/ { n++ }
      END { print n + 0 }'
  fi
}

reseed() { echo "jobserver-canary: $1 — re-seeding $SVC"; systemctl --user restart "$SVC"; }

# Either FIFO gone → custodian daemon is dead; restart unconditionally.
# A dead daemon means no verify can be using the FIFO, so restart is safe even
# under apparent build activity.
if [ ! -p "$MERGE_FIFO" ] || [ ! -p "$TASK_FIFO" ]; then
    reseed "merge/task FIFO missing"; exit 0
fi

# Require the build to be idle across the whole sampling window before acting,
# so we never re-seed while a verify is mid-flight.
for i in 1 2 3; do
  if [ "$(build_active)" -gt 0 ]; then
    echo "jobserver-canary: build active — skipping (sum=$(tokens_sum)/$SEEDED)"
    exit 0
  fi
  [ "$i" -lt 3 ] && sleep "${REIFY_JOBSERVER_CANARY_SETTLE_SLEEP:-5}"
done

s=$(tokens_sum)
[ -z "$s" ] && s=-1  # guard: malformed/empty python output → treat as vanished
hb=$(read_held_back)
if [ "$s" -lt 0 ]; then
    reseed "FIFO vanished mid-check"
elif [ $(( s + hb )) -lt "$SEEDED" ]; then
    reseed "idle but only $s+$hb/$SEEDED tokens (leaked)"
else
    echo "jobserver-canary: ok (idle, $s+$hb/$SEEDED tokens)"
fi
