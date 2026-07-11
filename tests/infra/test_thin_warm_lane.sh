#!/usr/bin/env bash
# tests/infra/test_thin_warm_lane.sh
# Hermetic tests for scripts/thin-warm-lane.sh (task 5174, PRD
# docs/prds/warm-lane-pool-sizing-lifecycle.md §9.3).
#
# Real rm/flock scoped to mktemp lane fixtures (hermetic-by-default, no PATH
# stubbing of coreutils needed — only --seed-script is stubbed, via the
# script's own hermetic test seam).
#
# run_helper captures STDOUT and STDERR SEPARATELY:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks:
#   A — CLI/usage contract + lane_dir existence guard (step-1/step-2)
#   B — precondition-refusal + T3 flock guard (step-3/step-4)
#   C — FREE-FIRST reclaim + T1 source-intact (step-5/step-6)
#   D — --reseed opt-in + free-BEFORE-stage ordering (step-7/step-8)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/thin-warm-lane.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/thin-warm-lane.sh hermetic tests (task 5174) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state + cleanup
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
_BGPIDS=()
cleanup() {
    for pid in "${_BGPIDS[@]+${_BGPIDS[@]}}"; do
        kill "$pid" 2>/dev/null || true
    done
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

# _wait_for_reader_lock <ready-marker> <deadline-seconds>
# Causal ordering (technique R, docs/prds/infra-test-wallclock-deflake.md,
# task #4847): polls for the READY marker file in 0.05s ticks instead of a
# fixed sleep, so a background flock holder's acquisition is causally
# guaranteed complete before the caller's next statement runs -- a fixed
# sleep races the holder under CPU/IO load (the script-under-test's flock -n
# could win first, spuriously turning a T3-refusal assertion green->red).
# Mirrors tests/infra/test_warm_lane_gc.sh's identically-named helper.
_wait_for_reader_lock() {
    local ready_marker="$1"
    local deadline_s="$2"
    local max_ticks=$(( deadline_s * 20 ))
    local tick=0
    while [ "$tick" -lt "$max_ticks" ]; do
        [ -f "$ready_marker" ] && return 0
        sleep 0.05
        tick=$(( tick + 1 ))
    done
    return 1
}

ERR_FILE="$(mktemp /tmp/test-thin-warm-lane-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── run_helper ─────────────────────────────────────────────────────────────────
# Invokes thin-warm-lane.sh, capturing OUT (stdout), ERR_OUT (stderr), RC.
run_helper() {
    local rc=0
    > "$ERR_FILE"
    OUT="$(bash "$SCRIPT" "$@" 2>"$ERR_FILE")" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# ──────────────────────────────────────────────────────────────────────────────
# Block A — CLI/usage contract + lane_dir existence guard
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: CLI/usage contract + lane_dir existence guard ---"

assert "A1: script exists" test -f "$SCRIPT"
assert "A2: script is executable" test -x "$SCRIPT"
assert "A3: script has the #!/usr/bin/env bash shebang" \
    bash -c 'head -1 "$1" | grep -qx "#!/usr/bin/env bash"' _ "$SCRIPT"

# A4/A5: --help exits 0 and prints a usage line to stderr
run_helper --help
assert "A4: --help exits 0" test "$RC" -eq 0
assert "A5: --help prints a 'Usage' line to stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi usage' _ "$ERR_OUT"

# A6: an unknown flag exits 2
run_helper /tmp --totally-bogus-flag-xyz
assert "A6: unknown flag exits 2" test "$RC" -eq 2

# A7: a missing positional lane_dir exits 2
run_helper
assert "A7: missing positional lane_dir exits 2" test "$RC" -eq 2

# A8: a nonexistent lane_dir exits 1 with a diagnostic on stderr, touches nothing
A8_LANE="$(mktemp -u /tmp/test-thin-warm-lane-a8-XXXXXX)"
run_helper "$A8_LANE"
assert "A8: nonexistent lane_dir exits 1" test "$RC" -eq 1
assert "A8: nonexistent lane_dir prints a diagnostic on stderr" \
    bash -c '[ -n "$1" ]' _ "$ERR_OUT"
assert "A8: nonexistent lane_dir still does not exist afterward (touches nothing)" \
    bash -c '[ ! -e "$1" ]' _ "$A8_LANE"

# A9: a lane_dir that exists but is a regular file (not a directory) exits 1
A9_LANE="$(mktemp /tmp/test-thin-warm-lane-a9-XXXXXX)"
_TMPDIRS+=("$A9_LANE")
run_helper "$A9_LANE"
assert "A9: lane_dir that is a regular file exits 1" test "$RC" -eq 1
assert "A9: regular-file lane_dir is untouched" test -f "$A9_LANE"

# ──────────────────────────────────────────────────────────────────────────────
# Block B — precondition-refusal + T3 flock guard
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: precondition-refusal + T3 flock guard ---"

# ── B1: lane_dir OUTSIDE REIFY_WARM_LANE_MOUNT is refused ─────────────────────
B1_MOUNT="$(mktemp -d /tmp/test-thin-warm-lane-b1-mount-XXXXXX)"
_TMPDIRS+=("$B1_MOUNT")
B1_OUTSIDE_LANE="$(mktemp -d /tmp/test-thin-warm-lane-b1-outside-XXXXXX)"
_TMPDIRS+=("$B1_OUTSIDE_LANE")
mkdir -p "$B1_OUTSIDE_LANE/target"
touch "$B1_OUTSIDE_LANE/target/MARKER"

REIFY_WARM_LANE_MOUNT="$B1_MOUNT" run_helper "$B1_OUTSIDE_LANE"
assert "B1: lane_dir outside REIFY_WARM_LANE_MOUNT exits 1" test "$RC" -eq 1
assert "B1: target/ untouched (outside-mount refusal)" \
    test -f "$B1_OUTSIDE_LANE/target/MARKER"

# ── B2: lane_dir resolving to the mount's base dir is refused (self-clobber) ──
B2_MOUNT="$(mktemp -d /tmp/test-thin-warm-lane-b2-mount-XXXXXX)"
_TMPDIRS+=("$B2_MOUNT")
mkdir -p "$B2_MOUNT/base/target"
touch "$B2_MOUNT/base/target/MARKER"

REIFY_WARM_LANE_MOUNT="$B2_MOUNT" run_helper "$B2_MOUNT/base"
assert "B2: lane_dir resolving to the mount's base dir exits 1" test "$RC" -eq 1
assert "B2: target/ untouched (self-clobber refusal)" \
    test -f "$B2_MOUNT/base/target/MARKER"

# ── B3: an ASSIGNED lane (external flock held) is refused with EX_TEMPFAIL ────
B3_LANE="$(mktemp -d /tmp/test-thin-warm-lane-b3-lane-XXXXXX)"
_TMPDIRS+=("$B3_LANE")
mkdir -p "$B3_LANE/target"
touch "$B3_LANE/target/MARKER"

B3_LOCK="${B3_LANE}.lock"
_TMPDIRS+=("$B3_LOCK")
B3_READY="${B3_LOCK}.ready-marker"
_TMPDIRS+=("$B3_READY")
touch "$B3_LOCK"
# Causal handshake (see _wait_for_reader_lock above) instead of a fixed sleep:
# the subshell touches B3_READY AFTER acquiring flock -x, so the assertions
# below only run once the lock is provably held.
( flock -x 9 && touch "$B3_READY" && sleep 300 ) 9>"$B3_LOCK" &
B3_LOCK_PID=$!
_BGPIDS+=("$B3_LOCK_PID")
_wait_for_reader_lock "$B3_READY" 30

run_helper "$B3_LANE"
assert "B3: ASSIGNED lane (flock held) exits 75 (EX_TEMPFAIL)" test "$RC" -eq 75
assert "B3: target/ byte-intact under lock contention (T3)" \
    test -f "$B3_LANE/target/MARKER"
assert "B3: stderr mentions the lock/live-consumer refusal" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "lock|consumer|assigned"' _ "$ERR_OUT"

kill "$B3_LOCK_PID" 2>/dev/null || true
wait "$B3_LOCK_PID" 2>/dev/null || true

test_summary
