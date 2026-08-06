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
#   E — seed fail-closed abort ⇒ the caller DISCARDS the uncertified
#       <lane>/target it aborted onto (task 5635; PRD
#       docs/prds/warm-lane-pool-cow-seeding.md §9.5 inv.13 caller obligation)
#   F — live-process-reference gate: a lane whose target/ is referenced by a
#       live process (cwd / open fd / mmap) is PRESERVED even when its flock is
#       free (task 5823; the flock is a reseed mutex, not a liveness oracle —
#       esc-5334-6)
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

# Arm the shared-trash litter guard (task 5612). Sited immediately after
# `trap cleanup EXIT` because the helper registers its per-run root into
# _TMPDIRS and so must follow this file's own `_TMPDIRS=()`.
# Rationale, ordering contract, stem rules and honest scope: see the
# CANONICAL WIRING CONTRACT comment in tests/infra/test_helpers.sh.
init_isolated_lane_root test-thin-warm-lane

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

# ── discard-FAILURE fixture (shared shape with tests/infra/test_warm_lane_gc.sh) ──
# _discard_fail_seed_stub_body — printed to stdout; redirect into a --seed-script
# stub file. Models a POST-CLONE fail-closed seed abort whose caller-side discard
# of <lane>/target then FAILS:
#   1. recreates "$2/target" carrying a HAZARD marker — the uncertified CoW clone
#      the seed aborted ONTO (seed-warm-lane.sh deliberately does not rm it; PRD
#      docs/prds/warm-lane-pool-cow-seeding.md §9.5 inv.13);
#   2. `chmod a-w "$2"` so the caller's `rm -rf "$2/target"` cannot unlink the
#      `target` entry from the lane dir and exits non-zero (EACCES);
#   3. exits 1 — the non-zero exit IS the caller's discard predicate (for the real
#      primitive, which runs under `set -euo pipefail` and writes stdout exactly
#      once at its terminal echo, non-zero exit and empty stdout always arrive
#      together).
# The chmod deliberately lands only AFTER the free-first `rm -rf <lane>/target`
# this script performs BEFORE the re-seed (T2 ordering), so that first rm still
# succeeds and only the post-abort discard fails — which a PATH-level `rm` stub
# (tests/infra/test_warm_lane_gc.sh's K4 technique) could not achieve.
#
# MEASURED technique (2026-07-31, this suite's /tmp): with the lane dir
# non-writable, `rm -rf <lane>/target` still removes target/'s CONTENTS but cannot
# unlink the `target` entry itself — it exits 1 and prints "Permission denied".
# Assert on the PRESENCE of <lane>/target, therefore, not on the HAZARD marker
# inside it.
#
# Every arm built on this stub MUST:
#   - be gated on `[ "$(id -u)" -ne 0 ]` — uid 0 bypasses DAC write checks, so the
#     fixture silently inverts under root (same skip idiom as
#     tests/infra/test_warm_lane_audit.sh's mode-000 record arm); and
#   - call `_restore_discard_fail_lane <lane_dir>` immediately after the run and
#     BEFORE any assertion can abort the suite — `trap cleanup EXIT`'s `rm -rf`
#     over _TMPDIRS cannot remove a non-writable lane dir either.
_discard_fail_seed_stub_body() {
    cat << 'STUB_EOF'
#!/usr/bin/env bash
echo "$*" >> "$SEED_LOG"
LANE_DIR="$2"
# POST-CLONE fail-closed abort: the uncertified CoW clone is left in place, and
# the lane dir is made non-writable so the caller's discard of it fails.
mkdir -p "$LANE_DIR/target"
touch "$LANE_DIR/target/HAZARD"
chmod a-w "$LANE_DIR"
exit 1
STUB_EOF
}

# _restore_discard_fail_lane <lane_dir> — undo the stub's `chmod a-w` so the
# suite's EXIT-trap cleanup can remove the fixture. Idempotent; safe to call on a
# lane the stub never touched.
_restore_discard_fail_lane() {
    chmod u+w "$1" 2>/dev/null || true
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

# A10: --reseed without --base is a usage error (exit 2), fired during arg
# parsing before any existence/lock/rm work.
run_helper /tmp --reseed
assert "A10: --reseed without --base exits 2" test "$RC" -eq 2

# A11: --base given with no following value exits 2
run_helper /tmp --base
assert "A11: --base with no value exits 2" test "$RC" -eq 2

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
# Inverse of Block F's F4 (task 5823). BOTH refusals exit 75, so the stderr
# reason is the only thing that tells them apart in dark-factory's logs — and it
# has to be pinned in both directions or a single shared message would keep each
# one-sided assert green. F4 pins that a process-reference refusal never names
# the flock reason; this pins that a flock-held lane never names the
# process-reference reason. It also pins the ORDERING: the flock gate fires
# FIRST, so a flock-held lane is refused before the ~1.9s /proc walk is ever run.
assert "B3: stderr does NOT name the live-process-reference refusal (the two exit-75 reasons stay distinguishable; task 5823)" \
    bash -c '! printf "%s\n" "$1" | grep -qi "live process reference"' _ "$ERR_OUT"

kill "$B3_LOCK_PID" 2>/dev/null || true
wait "$B3_LOCK_PID" 2>/dev/null || true

# ── B4: lane_dir literally IS the mount root is refused (self-clobber) ────────
# The under-mount guard's trailing-slash case-match ("$_rp_mount"/* against
# "$_rp_lane_dir/") treats the mount root itself as "under" the mount (glob
# * also matches the empty tail), so this case slips past that guard and is
# caught only by the separate self-clobber check's explicit ==mount-root test.
B4_MOUNT="$(mktemp -d /tmp/test-thin-warm-lane-b4-mount-XXXXXX)"
_TMPDIRS+=("$B4_MOUNT")
mkdir -p "$B4_MOUNT/target"
touch "$B4_MOUNT/target/MARKER"

REIFY_WARM_LANE_MOUNT="$B4_MOUNT" run_helper "$B4_MOUNT"
assert "B4: lane_dir == mount root exits 1 (self-clobber)" test "$RC" -eq 1
assert "B4: target/ untouched (mount-root self-clobber refusal)" \
    test -f "$B4_MOUNT/target/MARKER"

# ── B5: a lane_dir literally named 'base' is refused even when
# REIFY_WARM_LANE_MOUNT is unset (the basename=='base' guard is unconditional,
# independent of the under-mount guard) ────────────────────────────────────────
B5_PARENT="$(mktemp -d /tmp/test-thin-warm-lane-b5-parent-XXXXXX)"
_TMPDIRS+=("$B5_PARENT")
B5_LANE="$B5_PARENT/base"
mkdir -p "$B5_LANE/target"
touch "$B5_LANE/target/MARKER"

# Save/restore rather than a bare unset: the OPTIONAL df-delta layer in Block C
# gates on an operator-supplied REIFY_WARM_LANE_MOUNT (private reflink mount),
# and this test must not silently disable that layer for the rest of the run.
_B5_HAD_MOUNT=0
if [ -n "${REIFY_WARM_LANE_MOUNT+x}" ]; then
    _B5_HAD_MOUNT=1
    _B5_SAVED_MOUNT="$REIFY_WARM_LANE_MOUNT"
fi
unset REIFY_WARM_LANE_MOUNT
run_helper "$B5_LANE"
assert "B5: lane_dir named 'base' exits 1 (REIFY_WARM_LANE_MOUNT unset)" test "$RC" -eq 1
assert "B5: target/ untouched ('base'-name refusal, mount unset)" \
    test -f "$B5_LANE/target/MARKER"
if [ "$_B5_HAD_MOUNT" = "1" ]; then
    export REIFY_WARM_LANE_MOUNT="$_B5_SAVED_MOUNT"
fi

# _thin_detect_private_mount() — rung-1-only substrate detector (no loopback
# self-provisioning, unlike test_warm_lane_pool.sh's detect_private_substrate,
# to stay `pool`-safe): returns 0 when REIFY_WARM_LANE_MOUNT is set and
# reflink-capable, 1 otherwise. Gates the OPTIONAL df-delta assertion below;
# never gates (or skips) the hermetic core.
_thin_detect_private_mount() {
    [ -n "${REIFY_WARM_LANE_MOUNT:-}" ] || return 1
    [ -d "${REIFY_WARM_LANE_MOUNT}" ] || return 1
    local probe_src probe_dst
    probe_src="$(mktemp "${REIFY_WARM_LANE_MOUNT}/.thin-reflink-probe-XXXXXX" 2>/dev/null)" || return 1
    probe_dst="${probe_src}.dst"
    if cp --reflink=always "$probe_src" "$probe_dst" 2>/dev/null; then
        rm -f "$probe_src" "$probe_dst" 2>/dev/null || true
        return 0
    fi
    rm -f "$probe_src" "$probe_dst" 2>/dev/null || true
    return 1
}

# ──────────────────────────────────────────────────────────────────────────────
# Block C — FREE-FIRST reclaim + T1 source-intact
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: FREE-FIRST reclaim + T1 source-intact ---"

C_LANE="$(mktemp -d /tmp/test-thin-warm-lane-c-lane-XXXXXX)"
_TMPDIRS+=("$C_LANE")

# target/ with real weight (du -sb footprint > 0), so removal is meaningful.
mkdir -p "$C_LANE/target/debug"
dd if=/dev/zero of="$C_LANE/target/debug/blob.bin" bs=1024 count=256 2>/dev/null

C_TARGET_FOOTPRINT_BEFORE="$(du -sb "$C_LANE/target" | cut -f1)"
assert "C1: target/ fixture has nonzero footprint before thinning" \
    test "$C_TARGET_FOOTPRINT_BEFORE" -gt 0

# Source tree + real .git + an uncommitted WIP file (unlanded work) -- all
# must survive thinning byte-intact (T1).
mkdir -p "$C_LANE/src"
echo 'fn main() {}' > "$C_LANE/src/main.rs"
git init -q "$C_LANE"
git -C "$C_LANE" config user.email "test@test.local"
git -C "$C_LANE" config user.name "Test"
git -C "$C_LANE" add src/main.rs
git -C "$C_LANE" commit -q -m "initial"
C_HEAD_BEFORE="$(git -C "$C_LANE" rev-parse HEAD)"
echo "uncommitted work" > "$C_LANE/WIP.txt"

# --seed-script stub: records invocations. Must NOT be called on the default
# (no --reseed) path.
C_SEED_LOG="$(mktemp /tmp/test-thin-warm-lane-c-seedlog-XXXXXX)"
_TMPDIRS+=("$C_SEED_LOG")
C_SEED_STUB="$(mktemp /tmp/test-thin-warm-lane-c-seedstub-XXXXXX)"
_TMPDIRS+=("$C_SEED_STUB")
cat > "$C_SEED_STUB" << 'STUB_EOF'
#!/usr/bin/env bash
echo "$*" >> "$SEED_LOG"
exit 0
STUB_EOF
chmod +x "$C_SEED_STUB"
export SEED_LOG="$C_SEED_LOG"

run_helper "$C_LANE" --seed-script "$C_SEED_STUB"

assert "C2: exit 0" test "$RC" -eq 0
assert "C3: target/ is GONE" bash -c '[ ! -e "$1" ]' _ "$C_LANE/target"
assert "C4: source tree byte-intact (src/main.rs unchanged)" \
    bash -c '[ "$(cat "$1")" = "fn main() {}" ]' _ "$C_LANE/src/main.rs"
assert "C5: .git/ intact (HEAD unchanged)" \
    bash -c '[ "$(git -C "$1" rev-parse HEAD)" = "$2" ]' _ "$C_LANE" "$C_HEAD_BEFORE"
assert "C6: uncommitted WIP file byte-intact" \
    bash -c '[ "$(cat "$1")" = "uncommitted work" ]' _ "$C_LANE/WIP.txt"
assert "C7: STDOUT equals the realpath-resolved lane_dir (single line, no diagnostics mixed in)" \
    test "$OUT" = "$(realpath -m "$C_LANE")"
assert "C8: seed-script NOT invoked (no reseed staged by default)" \
    bash -c '[ ! -s "$1" ]' _ "$C_SEED_LOG"

# ── OPTIONAL substrate-gated df-delta (B4 literal, direction-only per G6/D8) ───
# Runs only when REIFY_WARM_LANE_MOUNT points at a private reflink mount;
# skips gracefully otherwise (never a frozen GB constant, never false-RED off
# substrate). Only this one assertion is gated -- not the hermetic core above.
if _thin_detect_private_mount; then
    C_DF_LANE="$(mktemp -d "${REIFY_WARM_LANE_MOUNT}/test-thin-warm-lane-df-XXXXXX")"
    _TMPDIRS+=("$C_DF_LANE")
    mkdir -p "$C_DF_LANE/target"
    dd if=/dev/zero of="$C_DF_LANE/target/blob.bin" bs=1M count=2 2>/dev/null

    C_DF_BEFORE="$(df --output=avail -m "$C_DF_LANE" 2>/dev/null | tail -1 | tr -d ' ')"
    run_helper "$C_DF_LANE"
    C_DF_RC="$RC"
    C_DF_AFTER="$(df --output=avail -m "$C_DF_LANE" 2>/dev/null | tail -1 | tr -d ' ')"
    echo "Block C df: before=${C_DF_BEFORE}MiB after=${C_DF_AFTER}MiB" >&2

    assert "C9: df-gated control run exits 0" test "$C_DF_RC" -eq 0
    assert "C9: df available did not decrease after thinning (direction-only, private mount)" \
        test "$C_DF_AFTER" -ge "$C_DF_BEFORE"
else
    echo "SKIP: Block C df-delta assertion -- REIFY_WARM_LANE_MOUNT not set to a private reflink mount" >&2
fi

# ──────────────────────────────────────────────────────────────────────────────
# Block D — --reseed opt-in + free-BEFORE-stage ordering (T2)
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block D: --reseed opt-in + free-BEFORE-stage ordering (T2) ---"

D_BASE="$(mktemp -d /tmp/test-thin-warm-lane-d-base-XXXXXX)"
_TMPDIRS+=("$D_BASE")

# --seed-script stub: logs argv AND whether <lane>/target existed AT CALL TIME
# (proves free-before-stage ordering when it's invoked under --reseed).
D_SEED_LOG="$(mktemp /tmp/test-thin-warm-lane-d-seedlog-XXXXXX)"
_TMPDIRS+=("$D_SEED_LOG")
D_SEED_STUB="$(mktemp /tmp/test-thin-warm-lane-d-seedstub-XXXXXX)"
_TMPDIRS+=("$D_SEED_STUB")
cat > "$D_SEED_STUB" << 'STUB_EOF'
#!/usr/bin/env bash
{
    echo "ARGV: $*"
    if [ -e "$2/target" ]; then
        echo "PRESENCE: PRESENT"
    else
        echo "PRESENCE: ABSENT"
    fi
} >> "$SEED_LOG"
exit 0
STUB_EOF
chmod +x "$D_SEED_STUB"
export SEED_LOG="$D_SEED_LOG"

# ── D1: default run (no --reseed) leaves the seed-script log EMPTY ────────────
D_LANE_A="$(mktemp -d /tmp/test-thin-warm-lane-d-a-XXXXXX)"
_TMPDIRS+=("$D_LANE_A")
mkdir -p "$D_LANE_A/target"
touch "$D_LANE_A/target/MARKER"

run_helper "$D_LANE_A" --seed-script "$D_SEED_STUB"
assert "D1: default (no --reseed) run exits 0" test "$RC" -eq 0
assert "D1: default run leaves the seed-script log EMPTY (no clone staged)" \
    bash -c '[ ! -s "$1" ]' _ "$D_SEED_LOG"

# ── D2: --reseed invokes the seed-script AFTER the free (T2) ──────────────────
D_LANE_B="$(mktemp -d /tmp/test-thin-warm-lane-d-b-XXXXXX)"
_TMPDIRS+=("$D_LANE_B")
mkdir -p "$D_LANE_B/target"
touch "$D_LANE_B/target/MARKER"

run_helper "$D_LANE_B" --reseed --base "$D_BASE" --seed-script "$D_SEED_STUB"
assert "D2: --reseed run exits 0" test "$RC" -eq 0
assert "D2: seed-script WAS invoked" bash -c '[ -s "$1" ]' _ "$D_SEED_LOG"
assert "D2: seed-script argv includes --fresh-checkout" \
    bash -c 'grep -q -- "--fresh-checkout" "$1"' _ "$D_SEED_LOG"
# thin holds ${LANE_DIR}.lock on FD 9 (T3, line 236) across the seed call, so
# the seed must NOT re-acquire it — else it self-refuses under seed-warm-lane.sh's
# fail-safe lane-lock default (esc-5214/task 5354). thin therefore passes
# --assume-lane-lock-held so the seed skips its own acquire.
assert "D2: seed-script argv includes --assume-lane-lock-held (thin already holds the lane lock on FD 9; task 5354)" \
    bash -c 'grep -q -- "--assume-lane-lock-held" "$1"' _ "$D_SEED_LOG"
assert "D2: seed-script argv includes the lane_dir" \
    bash -c 'grep -qF "$2" "$1"' _ "$D_SEED_LOG" "$D_LANE_B"
assert "D2: seed-script observed target ABSENT at invocation (free-before-stage, T2)" \
    bash -c 'grep -q "PRESENCE: ABSENT" "$1"' _ "$D_SEED_LOG"

# ── D3: --reseed best-effort — a failing seed-script does not flip the exit
# code. The free already succeeded (target/ is gone); that is the operation
# this script guarantees, so a reseed failure is logged, not fatal.
D_LANE_C="$(mktemp -d /tmp/test-thin-warm-lane-d-c-XXXXXX)"
_TMPDIRS+=("$D_LANE_C")
mkdir -p "$D_LANE_C/target"
touch "$D_LANE_C/target/MARKER"

D_FAIL_STUB="$(mktemp /tmp/test-thin-warm-lane-d-failstub-XXXXXX)"
_TMPDIRS+=("$D_FAIL_STUB")
cat > "$D_FAIL_STUB" << 'STUB_EOF'
#!/usr/bin/env bash
exit 1
STUB_EOF
chmod +x "$D_FAIL_STUB"

run_helper "$D_LANE_C" --reseed --base "$D_BASE" --seed-script "$D_FAIL_STUB"
assert "D3: --reseed with a failing seed-script still exits 0 (best-effort)" \
    test "$RC" -eq 0
assert "D3: target/ is still gone despite the reseed failure" \
    bash -c '[ ! -e "$1" ]' _ "$D_LANE_C/target"
# This stub refuses BEFORE staging anything, so the free-first rm (T2) is the
# whole story and Block E's discard has nothing to remove. `rm -rf` exits 0 on a
# missing path, so the discard branch must PROBE for the target/ rather than
# infer from its own exit status — otherwise it reports a discard that never
# happened. The post-clone counterpart is E1.
assert "D3: stderr reports there was NOTHING to discard (no false 'discarded' claim)" \
    bash -c 'printf "%s\n" "$1" | grep -qi "nothing to discard"' _ "$ERR_OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block E — seed fail-closed abort ⇒ discard the uncertified lane target/
# (task 5635; PRD docs/prds/warm-lane-pool-cow-seeding.md §9.5 inv.13,
# "Caller obligation on the fail-closed path")
#
# D3 above covers a PRE-clone seed failure — its stub never recreates target/, so
# the free-first rm is the whole story and "already freed" is accurate. This block
# covers the POST-clone case, which is the one the seed's three fail-closed
# post-conditions actually produce: they all fire AFTER target/ has been replaced
# with the CoW clone, so the seed aborts ONTO a hazardous clone it just refused to
# certify — and deliberately does not rm it, leaving that to the caller.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block E: seed fail-closed abort => discard the uncertified lane target/ ---"

E_BASE="$(mktemp -d /tmp/test-thin-warm-lane-e-base-XXXXXX)"
_TMPDIRS+=("$E_BASE")

E_SEED_LOG="$(mktemp /tmp/test-thin-warm-lane-e-seedlog-XXXXXX)"
_TMPDIRS+=("$E_SEED_LOG")

# ONE stub serves both E1 and E2; only $SEED_RC differs between the two runs.
# That IS the pair's point: the discard is keyed on the seed's EXIT STATUS, so a
# stub leaving a byte-identical <lane>/target behind must have that clone
# DISCARDED at rc=1 and PRESERVED at rc=0. Without E2 an unconditional `rm -rf`
# would keep E1 green. (Neutral marker name: the same staged clone is hazardous
# only when the seed refused to certify it.)
E_SEED_STUB="$(mktemp /tmp/test-thin-warm-lane-e-seedstub-XXXXXX)"
_TMPDIRS+=("$E_SEED_STUB")
cat > "$E_SEED_STUB" << 'STUB_EOF'
#!/usr/bin/env bash
echo "$*" >> "$SEED_LOG"
LANE_DIR="$2"
mkdir -p "$LANE_DIR/target"
touch "$LANE_DIR/target/CLONE_MARKER"
exit "${SEED_RC:-0}"
STUB_EOF
chmod +x "$E_SEED_STUB"
export SEED_LOG="$E_SEED_LOG"

# ── E1: a POST-CLONE fail-closed abort ⇒ the clone is discarded, exit still 0 ──
E_LANE_A="$(mktemp -d /tmp/test-thin-warm-lane-e-a-XXXXXX)"
_TMPDIRS+=("$E_LANE_A")
mkdir -p "$E_LANE_A/target"
touch "$E_LANE_A/target/MARKER"

SEED_RC=1 run_helper "$E_LANE_A" --reseed --base "$E_BASE" --seed-script "$E_SEED_STUB"

assert "E1: the uncertified clone is DISCARDED — <lane>/target is GONE" \
    bash -c '[ ! -e "$1" ]' _ "$E_LANE_A/target"
# The lane is now in exactly the state thin GUARANTEES (target/ freed) — cold but
# safe — so the documented best-effort-reseed contract still holds and the exit
# code stays 0. thin's exit code tracks whether the lane is left SAFE, not whether
# the re-seed succeeded.
assert "E1: exit is still 0 (lane left cold-but-safe; best-effort reseed contract holds)" \
    test "$RC" -eq 0
assert "E1: stdout is still exactly the resolved lane_dir (single line, contract intact)" \
    test "$OUT" = "$(realpath -m "$E_LANE_A")"
# Anchored on a [warn]-tagged line specifically: the free-first `ok "Freed
# <lane>/target"` already names that same path, so an unanchored grep would go
# green without the discard ever being reported.
assert "E1: a stderr [warn] line names the discarded path" \
    bash -c 'printf "%s\n" "$1" | grep -F "[warn]" | grep -qF "$2/target"' _ "$ERR_OUT" "$E_LANE_A"
assert "E1: stderr cites the §9.5 inv.13 caller obligation" \
    bash -c 'printf "%s\n" "$1" | grep -qF "inv.13"' _ "$ERR_OUT"

# ── E2: POSITIVE CONTROL — a CERTIFYING seed's clone is never discarded ────────
# Load-bearing: same stub, same staged target/, rc=0 instead of rc=1.
E_LANE_B="$(mktemp -d /tmp/test-thin-warm-lane-e-b-XXXXXX)"
_TMPDIRS+=("$E_LANE_B")
mkdir -p "$E_LANE_B/target"
touch "$E_LANE_B/target/MARKER"

SEED_RC=0 run_helper "$E_LANE_B" --reseed --base "$E_BASE" --seed-script "$E_SEED_STUB"

assert "E2: exit 0" test "$RC" -eq 0
assert "E2: a certifying seed's <lane>/target SURVIVES (discard keys on exit status, not on presence)" \
    test -d "$E_LANE_B/target"
assert "E2: the staged clone is byte-intact (nothing was discarded)" \
    test -f "$E_LANE_B/target/CLONE_MARKER"

# ── E3: the DISCARD ITSELF fails ⇒ the lane is hazardous and must NOT be
# handed to a caller. This is thin mirroring the seed's own empty-stdout refusal
# one level up: a lane still carrying an uncertified clone is exactly what a
# consumer must never be given, so no lane path is emitted at all.
# Technique + root-skip rationale: _discard_fail_seed_stub_body above.
if [ "$(id -u)" -ne 0 ]; then
    E_LANE_C="$(mktemp -d /tmp/test-thin-warm-lane-e-c-XXXXXX)"
    _TMPDIRS+=("$E_LANE_C")
    mkdir -p "$E_LANE_C/target"
    touch "$E_LANE_C/target/MARKER"

    E_FAIL_STUB="$(mktemp /tmp/test-thin-warm-lane-e-failstub-XXXXXX)"
    _TMPDIRS+=("$E_FAIL_STUB")
    _discard_fail_seed_stub_body > "$E_FAIL_STUB"
    chmod +x "$E_FAIL_STUB"

    run_helper "$E_LANE_C" --reseed --base "$E_BASE" --seed-script "$E_FAIL_STUB"
    # Restore BEFORE any assertion can abort the suite: `trap cleanup EXIT`'s
    # `rm -rf` over _TMPDIRS cannot remove a non-writable lane dir either.
    _restore_discard_fail_lane "$E_LANE_C"

    # Exit 1 SPECIFICALLY, not merely non-zero: 1 is the pinned "the rm we
    # guarantee did not happen" code in this script's header exit-code table,
    # and 75 (EX_TEMPFAIL) is a caller-meaningful requeue signal here — a
    # refactor that leaked 75 out of this branch would change how a dark-factory
    # caller reacts to a hazardous lane while a `-ne 0` assert stayed green.
    assert "E3: a FAILED discard exits 1 (the pinned code; the lane is not left safe)" \
        test "$RC" -eq 1
    assert "E3: STDOUT is EMPTY — a lane carrying an uncertified clone is never emitted" \
        test -z "$OUT"
    assert "E3: <lane>/target is RETAINED (the discard did not happen)" \
        test -d "$E_LANE_C/target"
    assert "E3: a stderr [error] line names the RETAINED <lane>/target so an operator can find it" \
        bash -c 'printf "%s\n" "$1" | grep -F "[error]" | grep -qF "$2/target"' _ "$ERR_OUT" "$E_LANE_C"
    assert "E3: stderr states the lane was NOT returned" \
        bash -c 'printf "%s\n" "$1" | grep -F "[error]" | grep -qiE "not returned|not emitted"' _ "$ERR_OUT"
    assert "E3: stderr carries the rm's own error text (failure detail not swallowed)" \
        bash -c 'printf "%s\n" "$1" | grep -qi "permission denied"' _ "$ERR_OUT"
else
    echo "  SKIP: E3 discard-failure asserts (running as uid 0; DAC write checks are bypassed)"
fi

# ──────────────────────────────────────────────────────────────────────────────
# Block F — live-process-reference gate (task 5823)
#
# ROOT CAUSE (esc-5334-6, 2026-07-26): the lane flock is a reseed MUTEX, not a
# liveness oracle. <lane>.lock is held only across the ACQUIRE reseed (task 5354)
# and across dark-factory's run_scoped_verification (DF 3027) — NEVER across the
# implement phase, where an agent runs cargo build/test for tens of minutes. So
# Block B's `flock -n 9` gate is BLIND for most of an ASSIGNED lane's life, and
# thin would happily `rm -rf <lane>/target` out from under a live build (the
# _lane-5 218-target "No such file or directory" storm of 2026-07-25, on the
# sibling gc path task 5572 closed).
#
# Every lane in this block therefore has a FREE flock — no holder is started
# anywhere in Block F — so the existing T3 gate cannot account for ANY refusal
# observed here. The only thing that can is the /proc live-reference check.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block F: live-process-reference gate (task 5823) ---"

# --seed-script stub: records invocations, so "the refusal precedes any work" is
# observable rather than assumed. Same shape as Block C's stub.
F_SEED_LOG="$(mktemp /tmp/test-thin-warm-lane-f-seedlog-XXXXXX)"
_TMPDIRS+=("$F_SEED_LOG")
F_SEED_STUB="$(mktemp /tmp/test-thin-warm-lane-f-seedstub-XXXXXX)"
_TMPDIRS+=("$F_SEED_STUB")
cat > "$F_SEED_STUB" << 'STUB_EOF'
#!/usr/bin/env bash
echo "$*" >> "$SEED_LOG"
exit 0
STUB_EOF
chmod +x "$F_SEED_STUB"
export SEED_LOG="$F_SEED_LOG"

# ── F-cwd: a live helper whose CWD is <lane>/target ───────────────────────────
# The exact build-cwd shape a cargo/rustc process holds. Mirrors
# tests/infra/test_warm_lane_gc.sh's P-basic fixture (:1520-1557).
F_ROOT="$(mktemp -d /tmp/test-thin-warm-lane-f-XXXXXX)"
_TMPDIRS+=("$F_ROOT")
F_LANE="$F_ROOT/_lane-f"
mkdir -p "$F_LANE/target"
touch "$F_LANE/target/DIVERGENT_MARKER"
# The lane lock exists (pool acquire/release convention, inv.2) and is FREE.
touch "${F_LANE}.lock"

# The helper touches READY only AFTER cd'ing in, so _wait_for_reader_lock proves
# the cwd is established before thin runs (causal ordering, technique R — never a
# fixed sleep). `exec sleep` so the tracked PID is the one holding the cwd.
F_READY="$F_ROOT/helper.ready"
( cd "$F_LANE/target" && touch "$F_READY" && exec sleep 300 ) &
F_HELPER_PID=$!
_BGPIDS+=("$F_HELPER_PID")
_wait_for_reader_lock "$F_READY" 30

run_helper "$F_LANE" --seed-script "$F_SEED_STUB"

# 75 (EX_TEMPFAIL) SPECIFICALLY, not merely non-zero: dark-factory's
# _run_thin_warm_lane logs rc=75 at DEBUG as a benign skip ("release-thin is not
# an escalation/fault", §9.5 inv.11) and EVERY other non-zero rc at WARNING, so a
# code that merely refuses would turn each lingering-reference release into a
# false fault in the operator's log.
assert "F1: a live-cwd-referenced lane exits 75 (EX_TEMPFAIL), with the flock FREE" \
    test "$RC" -eq 75
assert "F2: <lane>/target still exists (nothing was freed)" \
    test -d "$F_LANE/target"
assert "F2: target/DIVERGENT_MARKER byte-intact (the live build's cache is untouched)" \
    test -f "$F_LANE/target/DIVERGENT_MARKER"
assert "F3: stderr names the PROCESS-REFERENCE refusal" \
    bash -c 'printf "%s\n" "$1" | grep -qi "live process reference"' _ "$ERR_OUT"
# The mirror of the B3 assert added above: with every lock in this block FREE,
# naming the flock reason here would be a misattribution — and would mean the
# refusal came from the wrong gate entirely. Mirrors gc's P7.
assert "F4: stderr does NOT name the flock refusal (no misattribution; every lock in Block F is FREE)" \
    bash -c '! printf "%s\n" "$1" | grep -q "flock -n failed"' _ "$ERR_OUT"
assert "F5: seed-script log is EMPTY (the refusal precedes any work)" \
    bash -c '[ ! -s "$1" ]' _ "$F_SEED_LOG"

# ─────────────────────────────────────────────────────────────────────────────
# Block TRASH: shared-trash litter guard (task 5612). Two asserts, deliberately
# kept as two independently-reported signals: TRASH2 can realistically only ever
# report "clean", which is indistinguishable from a checker that stopped working
# — TRASH1 is the hermetic control proving the instrument still fires.
# Full rationale and honest scope: the CANONICAL WIRING CONTRACT comment in
# tests/infra/test_helpers.sh.
# ─────────────────────────────────────────────────────────────────────────────
assert "TRASH1: shared-trash litter detector is live (self-test fires on a synthetic bare-/tmp lane)" \
    assert_shared_trash_litter_detector_live
assert "TRASH2: no lane in this suite littered the machine-shared /tmp/.reseed-trash" \
    assert_no_shared_trash_litter

test_summary
