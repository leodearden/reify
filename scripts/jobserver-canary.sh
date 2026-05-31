#!/usr/bin/env bash
# jobserver-canary.sh — guard against cargo-jobserver token depletion.
#
# The shared FIFO jobserver (/tmp/reify-jobserver, reify-jobserver.service) is
# seeded with 32 byte-tokens once at service start. A token is a byte a rustc
# reads from the FIFO and must write back when the compile finishes; a rustc
# SIGKILLed mid-compile (verify timeout, storm cleanup, orchestrator restart)
# destroys its token PERMANENTLY. Over time the pool drifts toward 0 and every
# verify silently runs at -j1 with most cores idle.
#
# This canary re-seeds the pool, but ONLY when the build is idle: a quiescent
# jobserver should hold all 32 tokens, so "idle AND tokens < 32" unambiguously
# means leaked tokens — and restarting the service then can never disrupt an
# in-flight verify (which would hold its own O_RDWR view of the old FIFO
# anyway). Run periodically by reify-jobserver-canary.timer.
set -uo pipefail

FIFO=/tmp/reify-jobserver
SVC=reify-jobserver.service
SEEDED=32

tokens() {  # available tokens via FIONREAD (non-destructive); -1 if FIFO absent
  python3 - "$FIFO" <<'PY'
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

build_active() {  # count live cargo/compiler/linker procs (exclude stopped/zombie)
  ps -eo stat,comm | awk '
    $2 ~ /^(cargo|cargo-nextest|rustc|cc1|cc1plus|rust-lld|lld|lto)$/ && $1 !~ /[TZ]/ { n++ }
    END { print n + 0 }'
}

reseed() { echo "jobserver-canary: $1 — re-seeding $SVC"; systemctl --user restart "$SVC"; }

# FIFO gone entirely → jobserver is dead; restart unconditionally.
if [ ! -p "$FIFO" ]; then reseed "FIFO $FIFO missing"; exit 0; fi

# Require the build to be idle across the whole sampling window before acting,
# so we never re-seed while a verify is mid-flight.
for i in 1 2 3; do
  if [ "$(build_active)" -gt 0 ]; then
    echo "jobserver-canary: build active — skipping (tokens=$(tokens)/$SEEDED)"
    exit 0
  fi
  [ "$i" -lt 3 ] && sleep 5
done

t=$(tokens); [ -z "$t" ] && t=$SEEDED
if [ "$t" -lt 0 ]; then
  reseed "FIFO vanished mid-check"
elif [ "$t" -lt "$SEEDED" ]; then
  reseed "idle but only $t/$SEEDED tokens (leaked)"
else
  echo "jobserver-canary: ok (idle, $t/$SEEDED tokens)"
fi
