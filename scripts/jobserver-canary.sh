#!/usr/bin/env bash
# jobserver-canary.sh — guard against cargo-jobserver token depletion.
#
# C2 sum invariant: when the build is IDLE, FIONREAD(merge) + FIONREAD(task)
# must equal nproc (the total seeded by reify-jobserver.service). A sum less
# than nproc while idle means tokens have leaked; restarting the service
# re-seeds both pools to nproc.
#
# The canary checks only the SUM — per-pool splits are irrelevant. A skewed-
# but-sum-correct idle state (e.g. 32/0 after a merge burst) is LEGITIMATE:
# the balancer's give-back/ratchet leaves it and it corrects on its own.
# Only sum < nproc while idle is a real token leak (PRD §4 C2, §8 T-a).
#
# Run periodically by reify-jobserver-canary.timer.
set -uo pipefail

MERGE_FIFO=${REIFY_JOBSERVER_MERGE_FIFO:-/tmp/reify-jobserver-merge}
TASK_FIFO=${REIFY_JOBSERVER_TASK_FIFO:-/tmp/reify-jobserver-task}
SEEDED=${REIFY_JOBSERVER_TOKENS:-$(python3 -c 'import os;print(len(os.sched_getaffinity(0)))')}
SVC=reify-jobserver.service

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
if [ "$s" -lt 0 ]; then
    reseed "FIFO vanished mid-check"
elif [ "$s" -lt "$SEEDED" ]; then
    reseed "idle but only $s/$SEEDED tokens (leaked)"
else
    echo "jobserver-canary: ok (idle, $s/$SEEDED tokens)"
fi
