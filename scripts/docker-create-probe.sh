#!/usr/bin/env bash
# scripts/docker-create-probe.sh [IMAGE] — prove docker can create a container
# within a bound. `docker info` alone proves nothing: it answered throughout
# the 2026-11-17/18 stalls while `docker create` did not return (task 10181).
#
# IMAGE defaults to ubuntu:24.04. Bound: REIFY_DOCKER_CREATE_PROBE_SECS
# (default 120), applied to each docker step: a step still running then gets
# SIGTERM, and SIGKILL 10s later if it ignores that.
#
# Exit: 0 usable; 1 a docker step exited non-zero (its output is relayed);
# 2 usage error; 3 a docker step did not return within the bound.
# Diagnostics go to stderr; stdout stays silent.
set -uo pipefail

say() { echo "docker-create-probe: $*" >&2; }
relay() { printf '%s\n' "$1" | tail -n 40 >&2; }

secs="${REIFY_DOCKER_CREATE_PROBE_SECS:-120}"
if ! [[ "$secs" =~ ^[1-9][0-9]*$ ]]; then
    say "REIFY_DOCKER_CREATE_PROBE_SECS must be a positive integer, got '$secs'"
    exit 2
fi
if [ "$#" -gt 1 ]; then
    say "usage: scripts/docker-create-probe.sh [IMAGE]"
    exit 2
fi
image="${1:-ubuntu:24.04}"

kill_grace_secs=10
bounded() { timeout -k "$kill_grace_secs" "$secs" "$@"; }
# timeout exits 124 when the step died of SIGTERM at the bound, and 137 when it
# outlived SIGTERM and was killed kill_grace_secs later.
stalled() { [ "$1" -eq 124 ] || [ "$1" -eq 137 ]; }

out=$(bounded docker info 2>&1)
rc=$?
if stalled "$rc"; then
    say "\`docker info\` did not return within ${secs}s"
    exit 3
elif [ "$rc" -ne 0 ]; then
    relay "$out"
    say "\`docker info\` failed (exit $rc)"
    exit 1
fi

name="reify-docker-create-probe-$$-$RANDOM"
out=$(bounded docker create --name "$name" --label reify.docker-create-probe=1 "$image" true 2>&1)
rc=$?
if stalled "$rc"; then
    say "\`docker create\` of $image did not return within ${secs}s (the bound also covers pulling $image when it is not present locally)"
    say "a create the daemon finishes later leaves container $name in state Created; remove it with: docker rm $name"
    say "host check: journalctl -k | grep 'waiting for writeback completion'"
    exit 3
elif [ "$rc" -ne 0 ]; then
    relay "$out"
    say "\`docker create\` of $image failed (exit $rc)"
    exit 1
fi

if ! bounded docker rm -f "$name" >/dev/null 2>&1; then
    say "warning: could not remove probe container $name; remove it with: docker rm -f $name"
fi
exit 0
