#!/usr/bin/env bash
# tests/infra/test_seed_lane_lock_release_soak.sh
# Repeat-N characterization harness for scripts/seed-warm-lane.sh's lane-lock
# release-at-exit property.
# Task: #5705
#
# WHAT IT MEASURES
#
# seed's two detached background jobs are spelled
# `{ _close_inherited_fds; rm -rf ...; } </dev/null >/dev/null 2>&1 &` (task #6219
# amendment), and that inherited-FD close runs in the forked CHILD — so for a
# scheduling window the child still holds a dup of the open file description
# carrying seed's exclusive flock, and the lane lock stays HELD past seed's own
# exit. This harness drives N real seed
# invocations under induced CPU contention and counts how many times the lock
# reads back HELD immediately after seed exits, reporting one structured line:
#
#     SEED_LANE_LOCK_SOAK held=<n> iters=<N> workers=<w>
#
# THE MECHANISM, THE ARGUMENT AND THE MEASURED RATES ARE NOT RESTATED HERE.
# They live in exactly one place — the LANE-LOCK RELEASE CONTRACT block at the
# flock acquire in scripts/seed-warm-lane.sh — and this file is the instrument
# for RE-measuring them, which is the strongest possible reason not to keep a
# copy that a re-measurement would have to remember to update in lockstep.
#
# TWO LAYERS
#
#   LAYER 1 — ALWAYS RUNS. Hermetic, sub-second positive/negative control for
#             the probe primitive itself, sited BEFORE the opt-in gate (a
#             _skip() exits 0, so anything after the gate is DEAD on an ordinary
#             run). Without it the harness could ship a probe that silently
#             always answers "free".
#
#   LAYER 2 — ARMED ONLY when REIFY_RUN_SEED_LANE_LOCK_SOAK=1. Real seed
#             invocations under cpu_load_fixture.sh contention.
#
# ENV KNOBS
#   REIFY_RUN_SEED_LANE_LOCK_SOAK   — set to 1 to arm layer 2 (default: skip).
#   REIFY_SEED_LANE_LOCK_SOAK_ITERS — iteration count for layer 2 (default 50).
#                                     Sizing: this harness's OWN pre-fix RED
#                                     proof was held=4 at iters=200, i.e. ~2% per
#                                     iteration, so P(detect) = 1-(1-0.02)^N is
#                                     ~64% at N=50, ~98% at N=200 and needs
#                                     N~460 for 99.99%. Use >=200 for a one-shot
#                                     RED proof; the default is sized for a
#                                     re-measurement a human is watching.
#                                     (This per-iteration DETECTION rate is a
#                                     property of this instrument and is not the
#                                     same quantity as the three-arm mechanism
#                                     measurement cited above — do not merge the
#                                     two, and do not copy that one here.)
#
# The pass assertion is on the HELD COUNT (`held == 0`), never on elapsed time:
# post-fix, LOCK_UN drops the lock strictly before seed's parent exits and the
# probe runs strictly after, so a non-zero count can only mean the release
# regressed. (An elapsed-time upper bound would also be rejected by
# tests/infra/test_no_new_wallclock_upper_bounds.sh.)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob; classified
# `host-exclusive` in tests/infra/run-all-classification.manifest because the
# armed layer does real CPU burn.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/seed-warm-lane.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"
# shellcheck source=tests/infra/load_tolerance_lib.sh
source "$SCRIPT_DIR/load_tolerance_lib.sh"

echo "=== seed-warm-lane.sh lane-lock release-at-exit soak (task 5705) ==="

# ─────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ─────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
_BGPIDS=()

cleanup() {
    for pid in "${_BGPIDS[@]+${_BGPIDS[@]}}"; do
        kill "$pid" 2>/dev/null || true
    done
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

# Mint the per-run lane grandparent. MUST come after `_TMPDIRS=()` and
# `trap cleanup EXIT` — see init_isolated_lane_root's own contract. The stem
# also makes any trash entry seed leaves attributable to this suite.
init_isolated_lane_root test-seed-lock-soak

# _skip(reason) — emit SKIP on stderr, report the asserts run so far, exit 0.
# Mirrors tests/infra/test_warm_lane_pool.sh's identically-named helper.
_skip() {
    echo "SKIP: $*" >&2
    test_summary
    exit 0
}

# _wait_for_reader_lock <ready-marker> <deadline-seconds>
# Causal ordering (technique R, docs/prds/infra-test-wallclock-deflake.md):
# polls for the READY marker in 0.05s ticks instead of a fixed sleep, so a
# background flock holder's acquisition is causally guaranteed complete before
# the caller's next statement runs. A fixed sleep races the holder under load —
# the exact bug class this harness exists to measure. Mirrors the
# identically-named helper in test_seed_warm_lane.sh / test_thin_warm_lane.sh /
# test_warm_lane_gc.sh.
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

# _wait_until <deadline-seconds> <predicate-cmd...>
# Generic bounded causal poll (technique R): runs "$@" every 0.05s until it
# exits 0 or the deadline elapses. Same tick cadence and deadline arithmetic as
# _wait_for_reader_lock above; mirrors test_seed_warm_lane.sh's helper of the
# same name. Deliberately NOT an elapsed-time assertion -- it only ever waits
# LONGER on a slow box, and the pass/fail signal is the predicate.
_wait_until() {
    local deadline_s="$1"
    shift
    local max_ticks=$(( deadline_s * 20 ))
    local tick=0
    while [ "$tick" -lt "$max_ticks" ]; do
        "$@" && return 0
        sleep 0.05
        tick=$(( tick + 1 ))
    done
    return 1
}

# ─────────────────────────────────────────────────────────────────────────────
# The probe primitive — BYTE-FOR-BYTE the form test_seed_warm_lane.sh's H4 uses
# (`bash -c 'exec 8>"$1"; flock -n 8' _ "$LOCK"`), so this harness and H4 can
# never disagree about what "the lock is free" means. A separate PROCESS is
# mandatory: flock is not re-entrant across a process tree, but a lock held by
# THIS shell on another descriptor would also read back as free from within
# this shell.
#
# _probe_lock_free  — succeeds when the lock is FREE (acquirable).
# _probe_lock_held  — succeeds when the lock is HELD by someone else.
# ─────────────────────────────────────────────────────────────────────────────
_probe_lock_free() {
    bash -c 'exec 8>"$1"; flock -n 8' _ "$1"
}

_probe_lock_held() {
    ! _probe_lock_free "$1"
}

# ─────────────────────────────────────────────────────────────────────────────
# LAYER 1 — always-run hermetic control for the probe primitive.
#
# Sited BEFORE the opt-in gate below on purpose: _skip() calls test_summary and
# exits 0, so every statement after the gate is DEAD in the default (unarmed)
# environment — which is every ordinary run of this suite. A detector whose
# only self-check lived after the gate would never be exercised.
#
# Two-way, per the R8c/R8d "a detector, not a constant failure" idiom already
# used in test_seed_warm_lane.sh Block R: the probe must report HELD on a
# genuinely held lock AND FREE once it is released.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Layer 1: probe primitive positive/negative control (always runs) ---"

L1_LANE="$(make_isolated_lane L1-lane)"
L1_LOCK="${L1_LANE}.lock"
L1_READY="${L1_LOCK}.ready-marker"
touch "$L1_LOCK"

# Causal handshake: the subshell touches the ready marker only AFTER it has
# acquired flock -x, so the probe below fires against a provably held lock.
( flock -x 9 && touch "$L1_READY" && sleep 300 ) 9>"$L1_LOCK" &
L1_HOLDER_PID=$!
_BGPIDS+=("$L1_HOLDER_PID")

assert "L1-setup: background holder acquired the lock within the deadline (ready marker appeared)" \
    _wait_for_reader_lock "$L1_READY" 30

assert "L1a: probe reports HELD while a separate process holds flock -x" \
    _probe_lock_held "$L1_LOCK"

kill "$L1_HOLDER_PID" 2>/dev/null || true
wait "$L1_HOLDER_PID" 2>/dev/null || true

assert "L1b: probe reports FREE once that holder is gone (a detector, not a constant HELD)" \
    _probe_lock_free "$L1_LOCK"

# ─────────────────────────────────────────────────────────────────────────────
# Opt-in gate — everything below is DEAD unless the soak is armed.
# ─────────────────────────────────────────────────────────────────────────────
if [ "${REIFY_RUN_SEED_LANE_LOCK_SOAK:-}" != "1" ]; then
    _skip "set REIFY_RUN_SEED_LANE_LOCK_SOAK=1 to arm the seed lane-lock release soak"
fi

# ─────────────────────────────────────────────────────────────────────────────
# LAYER 2 — ARMED: N real seed invocations under induced CPU contention.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Layer 2: armed soak against the real scripts/seed-warm-lane.sh ---"

assert "L2-setup: scripts/seed-warm-lane.sh exists" test -f "$SCRIPT"

SOAK_ITERS="${REIFY_SEED_LANE_LOCK_SOAK_ITERS:-50}"
case "$SOAK_ITERS" in
    ''|*[!0-9]*|0)
        echo "ERROR: REIFY_SEED_LANE_LOCK_SOAK_ITERS must be a positive integer (got '$SOAK_ITERS')" >&2
        exit 64
        ;;
esac

# Load workers: min(nproc, 24). 24 is the count the pre-fix rate was measured
# at on a 32-core host; capping at nproc keeps a smaller box from thrashing.
_NPROC="$(nproc 2>/dev/null || echo 4)"
SOAK_WORKERS="$_NPROC"
[ "$SOAK_WORKERS" -gt 24 ] && SOAK_WORKERS=24
[ "$SOAK_WORKERS" -lt 1 ] && SOAK_WORKERS=1

# Minimal PATH `cp` shim. seed HARD-REQUIRES reflink (`cp -a --reflink=always`
# with no fallback), which /tmp (ext4) cannot satisfy — so the shim drops the
# --reflink=always flag and execs a real `cp -a`. This is the whole reason the
# soak duplicates any fixture scaffolding at all; everything else it needs is
# real.
SOAK_STUB_DIR="$(mktemp -d /tmp/test-seed-lock-soak-stub-XXXXXX)"
_TMPDIRS+=("$SOAK_STUB_DIR")
cat > "$SOAK_STUB_DIR/cp" << 'SOAK_CP_EOF'
#!/usr/bin/env bash
_args=()
for _a in "$@"; do
    case "$_a" in
        --reflink=always) ;;
        *) _args+=("$_a") ;;
    esac
done
exec /bin/cp "${_args[@]}"
SOAK_CP_EOF
chmod +x "$SOAK_STUB_DIR/cp"

# Shared base fixture: an empty .warm-base-meta sidecar so the RUSTFLAGS and
# invocation-fingerprint guards pass.
SOAK_BASE_PARENT="$(mktemp -d /tmp/test-seed-lock-soak-base-XXXXXX)"
_TMPDIRS+=("$SOAK_BASE_PARENT")
SOAK_BASE="$SOAK_BASE_PARENT/target"
mkdir -p "$SOAK_BASE/debug"
echo "base artifact" > "$SOAK_BASE/debug/base_artifact.a"
printf 'RUSTFLAGS=\nINVOCATION=\n' > "$SOAK_BASE_PARENT/.warm-base-meta"

SOAK_OUT_FILE="$(mktemp /tmp/test-seed-lock-soak-out-XXXXXX)"
_TMPDIRS+=("$SOAK_OUT_FILE")
SOAK_ERR_FILE="$(mktemp /tmp/test-seed-lock-soak-err-XXXXXX)"
_TMPDIRS+=("$SOAK_ERR_FILE")

# Induced contention for the whole soak. cpu_load_fixture.sh self-bounds via an
# internal `timeout` per worker and reaps its own workers on EXIT, so an
# over-generous duration costs nothing — we kill it as soon as the loop ends.
# 3s/iteration + 30s slack is comfortably above the measured per-iteration cost
# (~0.39s on a 32-core host under this harness's own 24-worker load); this is a
# resource budget, never an assertion.
SOAK_LOAD_SECS=$(( SOAK_ITERS * 3 + 30 ))
echo "  (starting $SOAK_WORKERS load workers for up to ${SOAK_LOAD_SECS}s)"
bash "$SCRIPT_DIR/cpu_load_fixture.sh" "$SOAK_WORKERS" "$SOAK_LOAD_SECS" \
    --label seed-lock-soak >/dev/null 2>&1 &
SOAK_LOAD_PID=$!
_BGPIDS+=("$SOAK_LOAD_PID")

# ─────────────────────────────────────────────────────────────────────────────
# LOAD-READINESS HANDSHAKE (technique R, docs/prds/infra-test-wallclock-deflake.md)
#
# Induced contention is the ENTIRE mechanism by which this instrument widens
# seed's fork-to-close window, so measuring during the load RAMP is measuring an
# idle box and reporting it as clean. Backgrounding the fixture and falling
# straight into the loop gives no causal guarantee at all — hence a bounded
# poll, exactly as the layer-1 control uses a ready-marker handshake rather than
# a fixed sleep.
#
# THE GAP IS MEASURABLE, which is what settles that this is worth the code: an
# iteration costs ~0.17s when the box is effectively idle but ~0.39s once 24
# spinners are genuinely established (ITERS=50 -> 18s, ITERS=200 -> 78s wall,
# measured). A run that overlaps the ramp is therefore not merely "slightly
# less contended" — it is measurably a different experiment, and reporting its
# held=0 as a clean result would be a false negative.
#
# cpu_load_fixture.sh forks `timeout <d> sh -c 'while true; do :; done' &` per
# worker, so its own children are the `timeout`s and the SPINNERS — the things
# that actually burn CPU — are its grandchildren.
#
# WHY NOT --emit-cgroup: that file is documented to be written BEFORE the
# workers are forked, so it proves the fixture STARTED, which is the one thing
# we already know. WHY NOT /proc/loadavg: it is a ~1-minute exponential average
# and lags the ramp by far longer than the whole soak.
# ─────────────────────────────────────────────────────────────────────────────
_soak_spinner_pids() {
    local _t
    for _t in $(pgrep -P "$SOAK_LOAD_PID" 2>/dev/null || true); do
        pgrep -P "$_t" 2>/dev/null || true
    done
}

# Aggregate utime+stime (clock ticks) across the spinners. /proc/<pid>/stat
# fields 14/15 — but `comm` is parenthesised and may itself contain spaces and
# parens, so everything through the LAST ')' is stripped first, after which
# utime/stime are fields 12/13.
_soak_spinner_cpu_ticks() {
    local _p _stat _total=0
    for _p in $(_soak_spinner_pids); do
        _stat="$(cat "/proc/$_p/stat" 2>/dev/null || true)"
        [ -n "$_stat" ] || continue
        # shellcheck disable=SC2086
        set -- ${_stat##*') '}
        _total=$(( _total + ${12:-0} + ${13:-0} ))
    done
    printf '%s\n' "$_total"
}

_soak_all_workers_up() {
    [ "$(_soak_spinner_pids | grep -c . || true)" -ge "$SOAK_WORKERS" ]
}

# "Burning" = the spinners have collectively advanced by at least one clock
# tick per worker since _SOAK_TICKS0. A forked-but-never-scheduled worker
# contributes nothing, so this cannot pass on a ramp that has stalled.
_SOAK_TICKS0=0
_soak_workers_burning() {
    [ "$(( $(_soak_spinner_cpu_ticks) - _SOAK_TICKS0 ))" -ge "$SOAK_WORKERS" ]
}

assert "L2-setup: pgrep is available (the load handshake below reads the fixture's process tree)" \
    command -v pgrep
assert "L2-setup: all $SOAK_WORKERS busy-spin workers are up before the first measured iteration" \
    _wait_until 60 _soak_all_workers_up
_SOAK_TICKS0="$(_soak_spinner_cpu_ticks)"
assert "L2-setup: ... and they are demonstrably BURNING CPU (utime+stime advancing), not merely forked" \
    _wait_until 60 _soak_workers_burning

SOAK_HELD=0
SOAK_RC_BAD=0
_i=0
while [ "$_i" -lt "$SOAK_ITERS" ]; do
    _i=$(( _i + 1 ))

    _lane="$(make_isolated_lane soak-lane)"
    # A NON-EMPTY <lane>/target is what makes seed rename into .reseed-trash
    # and therefore fork the detached
    # `{ _close_inherited_fds; rm -rf ...; } </dev/null >/dev/null 2>&1 &` whose fork
    # window is under test. REIFY_WARM_LANE_RESEED_TRASH_SYNC is left UNSET so
    # that rm really is backgrounded (the SYNC=1 path completes in-process and
    # has no fork window at all).
    mkdir -p "$_lane/target"
    echo "stale artifact" > "$_lane/target/stale.a"
    _lock="${_lane}.lock"

    # STDOUT is captured via a real FILE, never `$(...)`. Since task #6219 the
    # detached rm inherits no descriptor of its caller's at all (the PIPE-SAFE
    # note in seed's own header states that property), so a `$(...)` here would
    # no longer block on it. The file capture is kept anyway, on the stronger
    # ground that it makes this harness INDEPENDENT of seed's pipe-safety
    # property rather than dependent on its absence: what the soak measures is
    # then identical whether that property holds or regresses, so a future
    # change in either direction cannot silently move the instrument. Same
    # capture discipline as test_seed_warm_lane.sh's run_helper_real.
    : > "$SOAK_OUT_FILE"
    : > "$SOAK_ERR_FILE"
    _rc=0
    RUSTFLAGS="" PATH="$SOAK_STUB_DIR:$PATH" \
        bash "$SCRIPT" "$SOAK_BASE" "$_lane" --fresh-checkout --lane-lock \
        >"$SOAK_OUT_FILE" 2>"$SOAK_ERR_FILE" || _rc=$?
    [ "$_rc" -eq 0 ] || SOAK_RC_BAD=$(( SOAK_RC_BAD + 1 ))

    # Probe IMMEDIATELY after seed's own exit — the instant acquire_lane would.
    if _probe_lock_held "$_lock"; then
        SOAK_HELD=$(( SOAK_HELD + 1 ))
    fi

    # Measured: ~0.39s/iteration under this harness's own load, so ITERS=50 is
    # ~18s and ITERS=200 ~78s — NOT the "minutes" an earlier revision of this
    # comment claimed as the keepalive's justification. It is kept anyway on
    # narrower grounds: it is cheap, and this is the one suite whose runtime
    # scales with an operator-supplied N under deliberate CPU starvation, so a
    # large ITERS on an already-contended box can still outrun dark-factory's
    # stall detector.
    load_tolerant_keepalive seed_lane_lock_soak
done

kill "$SOAK_LOAD_PID" 2>/dev/null || true
wait "$SOAK_LOAD_PID" 2>/dev/null || true

# The structured, greppable result line — the harness's actual output as a
# characterization instrument, emitted whether or not the assert below passes.
echo "SEED_LANE_LOCK_SOAK held=$SOAK_HELD iters=$SOAK_ITERS workers=$SOAK_WORKERS"

assert "L2a: every soak iteration's seed invocation exited 0 (the soak measured real reseeds, not refusals)" \
    test "$SOAK_RC_BAD" -eq 0
assert "L2b: the lane lock was FREE immediately after seed exited on every iteration (held=$SOAK_HELD of $SOAK_ITERS)" \
    test "$SOAK_HELD" -eq 0

test_summary
