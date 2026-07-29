#!/usr/bin/env bash
# Shared-lifecycle contract for gui/test/visual/lib_e2e_smoke.sh (tasks 4456, 5596).
#
# Originally the infrastructure guard for gui/test/visual/run_find_uses_smoke.sh
# alone, pinning the readiness-race fix (reviewer finding: flaky_test_readiness_race)
# *behaviorally*: it ran the real runner with a launcher stub that dies
# immediately and asserted the liveness guard aborts early (non-zero, fast)
# rather than blocking until the full readiness deadline.
#
# Task 5596 extracted that lifecycle — port resolution, env hygiene, the reap
# trap, the optional pre-build, the backgrounded launcher, the readiness+liveness
# poll loop and the SIGTERM teardown — into gui/test/visual/lib_e2e_smoke.sh,
# shared by all SIX e2e smoke runners.  This file grew with it: it now runs the
# full experiment against EVERY runner, so it is the contract for the shared
# implementation rather than a single-runner guard.
#
# NOTE: deliberately NO source-text/grep assertions. Greppping the runner for
# literal fragments (`kill -0`, `REIFY_SMOKE_WAIT_MS`, the `&` launcher line)
# matches the runner's own header COMMENTS as well as its executable code, so a
# regression that deletes the liveness logic but leaves the descriptive comment
# would keep such contracts green — passing on the very failure they claim to
# pin. The behavioral contract below is the only one that proves behavior.
#
# That NOTE is also why coverage of the six runners is proved by RUNNING each of
# them through the real scenarios rather than by grepping each for a
# `source .*lib_e2e_smoke.sh` line: a source-text check would match the header
# comment of a runner whose delegation had been reverted.
#
# ── THREE-ARM EXPERIMENT (task 5596) ─────────────────────────────────────────
# The launcher-death predicate used to be
#     grep -qiE "launcher|exited|early|died|liveness|kill"
# which was MEASURED vacuous, not merely suspected: run against the real runner
# on a HAPPY path (alive stub launcher, PATH-stubbed curl->0 and node->0,
# REIFY_SMOKE_SKIP_PREBUILD=1) the runner exits 0 and still prints its startup
# banner `run_find_uses_smoke: launcher PID=<pid>` — which that alternation
# matches on its bare `launcher` branch.  The assertion was therefore satisfiable
# by startup-banner output alone and passed regardless of whether the launcher
# ever died.
#
# The predicate is now the canonical machine marker emitted only on a death path:
#     E2E_SMOKE_LAUNCHER_DEATH phase=<readiness|post-driver> rc=<n> pid=<n>
#
# A narrower regex would only ASSERT discrimination; the arms below DEMONSTRATE it.
#   ARM A (positive, death at readiness) — launcher exits 1 immediately:
#       rc != 0, aborts far inside the budget, marker PRESENT.
#   ARM B (negative control, launcher lives) — health + driver stubbed green:
#       rc == 0, startup banner PRESENT (so the arm really ran the normal path),
#       marker ABSENT.
#   ARM C (post-driver post-mortem) — the launcher survives readiness, then dies
#       DURING the driver run while the driver still exits 0:
#       rc != 0, `phase=post-driver` marker PRESENT, banner PRESENT.
#
# Arm B is what makes Arm A's assertion non-vacuous: the same output token that
# Arm A demands is proven absent on a run that produces the banner.  Arm C pins
# the correctness gap a readiness-only liveness check leaves open: liveness was
# only ever checked BEFORE the driver ran, so a launcher that crashed mid-run
# behind a green driver produced a false PASS.
#
# Auto-discovered by tests/infra/run_all.sh (matches test_*.sh pattern).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

# The canonical death marker.  Assembled from a variable so every arm below
# tests the SAME token and the positive/negative pair can never drift apart.
DEATH_MARKER="E2E_SMOKE_LAUNCHER_DEATH"

# All six runners that delegate to the shared lifecycle.  Every one is put
# through every arm — that behavioral sweep IS the proof of delegation.
RUNNERS=(
    run_find_uses_smoke.sh
    run_appearance_e2e_smoke.sh
    run_diagnostics_e2e_smoke.sh
    run_multi_pane_e2e_smoke.sh
    run_surface_finish_viewport_e2e_smoke.sh
    run_mesh_count_parity_e2e_smoke.sh
)

echo "=== test_find_uses_smoke_runner: shared e2e smoke lifecycle contract ==="

# ---------------------------------------------------------------------------
# Shared arm scaffold.
#
# Every arm gets its OWN tmpdir/bin so stubs can never leak between arms, and
# its own debug port so a lingering listener from one arm cannot make another
# arm's health poll succeed by accident.
# ---------------------------------------------------------------------------
_TMPDIRS=()
# The trailing `return 0` is load-bearing, not decoration. With an EMPTY
# _TMPDIRS the loop below runs once over the `:-` placeholder empty string,
# `[ -n "" ]` is false, and the short-circuited `&&` leaves the function's
# status at 1 — which, as an EXIT-trap body under `set -e`, becomes the SUITE's
# exit status. That yields the worst possible failure mode: "Results: N passed,
# 0 failed" printed while run_all.sh records the file as FAILED.
cleanup() {
    local d
    for d in "${_TMPDIRS[@]:-}"; do
        [ -n "$d" ] && rm -rf "$d"
    done
    return 0
}
trap cleanup EXIT INT TERM

# _alloc_port — hands out a distinct port per (runner, arm) pair from a fixed
# base, monotonically, so distinctness is structural rather than something a
# future edit has to remember to maintain. Sets _PORT.
_NEXT_PORT=59900
_alloc_port() {
    _PORT="$_NEXT_PORT"
    _NEXT_PORT=$(( _NEXT_PORT + 1 ))
}

# _new_bindir — mktemps a fresh <tmpdir>/bin, registers the tmpdir for cleanup,
# and sets _BINDIR to the bin path.
#
# It sets a GLOBAL rather than echoing, deliberately: a `x=$(_new_bindir)` call
# site would run this body in a command-substitution SUBSHELL, so the
# `_TMPDIRS+=(...)` registration would be discarded the moment that subshell
# exited — silently leaking every arm's tmpdir and leaving the array empty
# (measured: array_len=0). Setting a global keeps registration in the main shell.
_new_bindir() {
    local d
    d=$(mktemp -d)
    _TMPDIRS+=("$d")
    mkdir -p "$d/bin"
    _BINDIR="$d/bin"
}

# _write_stub <path> <body...> — writes an executable bash stub.
_write_stub() {
    local path="$1"
    shift
    {
        echo '#!/usr/bin/env bash'
        printf '%s\n' "$@"
    } > "$path"
    chmod +x "$path"
}

# _smoke_arm_run <bindir> <runner> <port> <wait_ms> [EXTRA=VAL ...]
#
# Runs the real runner with:
#   REIFY_SMOKE_SKIP_PREBUILD=1        (skip cargo/npm build steps)
#   REIFY_SMOKE_LAUNCHER=<bindir>/stub_launcher.sh
#   REIFY_SMOKE_WAIT_MS=<wait_ms>      (readiness budget)
#   REIFY_DEBUG_PORT=<port>            (valid port, so resolve_port doesn't allocate)
#   DISPLAY=:99                        (dummy display, must not open a window)
#   PATH=<bindir>:$PATH                (PATH-stub injection for curl/node)
#
# Sets _ARM_RC, _ARM_OUT (combined stdout+stderr) and _ARM_ELAPSED (seconds).
_smoke_arm_run() {
    local bindir="$1" runner="$2" port="$3" wait_ms="$4"
    shift 4
    local _arm_start=$SECONDS
    _ARM_RC=0
    _ARM_OUT=$(
        env \
            REIFY_SMOKE_SKIP_PREBUILD=1 \
            REIFY_SMOKE_LAUNCHER="$bindir/stub_launcher.sh" \
            REIFY_SMOKE_WAIT_MS="$wait_ms" \
            REIFY_DEBUG_PORT="$port" \
            DISPLAY=:99 \
            PATH="$bindir:$PATH" \
            "$@" \
            bash "$runner" 2>&1
    ) || _ARM_RC=$?
    _ARM_ELAPSED=$(( SECONDS - _arm_start ))
}

# ===========================================================================
# ARM A — positive: launcher dies at readiness.
#
# A 6-minute budget with an immediately-exiting launcher: the runner must abort
# on the liveness check FAR sooner, and say so with the death marker.
# ===========================================================================
_arm_a() {
    local label="$1" runner="$2" bin
    _alloc_port
    _new_bindir; bin="$_BINDIR"

    _write_stub "$bin/stub_launcher.sh" 'exit 1'
    # Stub node: should not be reached; exits 1 if called.
    _write_stub "$bin/node" \
        'echo "STUB_ERROR: node driver should not be reached when launcher dies" >&2' \
        'exit 1'

    _smoke_arm_run "$bin" "$runner" "$_PORT" 600000
    _t4_rc="$_ARM_RC"
    _t4_out="$_ARM_OUT"
    _t4_elapsed="$_ARM_ELAPSED"

    assert "$label ARM A: exits non-zero when launcher dies immediately" \
        bash -c '[ "$1" -ne 0 ]' _ "$_t4_rc"

    assert "$label ARM A: aborts within 15s (liveness guard, not the 600s deadline)" \
        bash -c '[ "$1" -lt 15 ]' _ "$_t4_elapsed" # wallclock:allow — liveness: discriminated by rc!=0 + the launcher-death marker, not by elapsed magnitude

    # Match the death path's OWN machine marker, not a broad alternation. The
    # runner unconditionally announces `launcher PID=<pid>` before the readiness
    # loop is even entered, so a pattern with a bare `launcher` branch matches on
    # EVERY run — including one where the liveness guard was deleted outright
    # (measured; see the header). Only a death path prints this token; ARM B
    # proves it stays absent on the happy path.
    assert "$label ARM A: emits the readiness-phase launcher-death marker (not just its startup banner)" \
        bash -c 'printf "%s\n" "$2" | grep -qF "$1 phase=readiness rc="' _ "$DEATH_MARKER" "$_t4_out"
}

# ===========================================================================
# ARM B — negative control: launcher LIVES, health + driver stubbed green.
#
# `exec sleep N` (not a bare `sleep N &`) so the runner's `$!` IS the sleep
# itself and its SIGTERM teardown reaps it directly rather than orphaning a
# detached child that would keep the output pipe open.
# ===========================================================================
_arm_b() {
    local label="$1" runner="$2" bin
    _alloc_port
    _new_bindir; bin="$_BINDIR"

    _write_stub "$bin/stub_launcher.sh" 'exec sleep 20'
    # Health poll succeeds immediately.
    _write_stub "$bin/curl" 'exit 0'
    # Driver succeeds.
    _write_stub "$bin/node" 'exit 0'

    _smoke_arm_run "$bin" "$runner" "$_PORT" 30000
    local rc="$_ARM_RC" out="$_ARM_OUT"

    assert "$label ARM B (control): exits 0 on the happy path (live launcher, green driver)" \
        bash -c '[ "$1" -eq 0 ]' _ "$rc"

    assert "$label ARM B (control): happy path DOES print the startup banner (really ran the normal path)" \
        bash -c 'printf "%s\n" "$1" | grep -qF "launcher PID="' _ "$out"

    assert "$label ARM B (control): happy path emits NO launcher-death marker (ARM A's predicate is not vacuous)" \
        bash -c '! printf "%s\n" "$2" | grep -qF "$1"' _ "$DEATH_MARKER" "$out"
}

# ===========================================================================
# ARM C — post-driver post-mortem: the launcher survives readiness and then
# dies DURING the driver run, while the driver still exits 0.
#
# This is the false-PASS shape a readiness-only liveness check cannot see:
# liveness was confirmed before the driver started and never re-checked after,
# so a green driver masked a dead launcher entirely.
#
# The scenario is deterministic, not timing-dependent. MEASURED: bash reaps a
# backgrounded child through its SIGCHLD handler promptly, so once the driver
# SIGKILLs the launcher, `kill -0` starts failing at the FIRST poll iteration —
# both from the driver stub (a sibling process) and from the runner shell
# itself. There is no window in which the dead launcher lingers as a
# kill -0-visible zombie, so the post-mortem has no race to lose.
# ===========================================================================
_arm_c() {
    local label="$1" runner="$2" bin
    _alloc_port
    _new_bindir; bin="$_BINDIR"
    local pidfile="$bin/launcher.pid"

    # Launcher: publish the PID the runner holds, then stay alive well past
    # readiness. `$$` survives `exec`, so the sleep IS this pid — which is also
    # the runner's `$!`, so both sides agree on one identity with no pid
    # translation.
    _write_stub "$bin/stub_launcher.sh" \
        'echo "$$" > "$_PIDFILE"' \
        'exec sleep 30'
    # Health poll succeeds immediately, so readiness passes with a LIVE launcher
    # — this arm reaches the driver, unlike ARM A.
    _write_stub "$bin/curl" 'exit 0'
    # Driver: kill the launcher, confirm it is really gone, then report SUCCESS.
    # Exiting 0 is the whole point — the runner must fail on the launcher's
    # death, not on the driver's exit code.
    #
    # The first loop is load-bearing, and its absence was MEASURED to make this
    # arm flaky at ~10% (6 spurious passes in 60 arm-runs, across every runner).
    # Because the health poll is STUBBED green, readiness can be satisfied on
    # the very first iteration — before the backgrounded launcher has run its
    # own first line. A bare `pid=$(cat "$_PIDFILE")` then read a missing file,
    # leaving pid empty; `kill -9 ""` and `kill -0 ""` both fail, so the stub
    # took the `exit 0` success path on iteration 1 WITHOUT EVER KILLING the
    # launcher. The runner then correctly saw a live launcher and exited 0, and
    # both ARM C contract assertions failed for a reason that had nothing to do
    # with the code under test.
    #
    # Waiting for a non-empty pidfile closes that race, and routing every
    # give-up path through STUB_ERROR closes the second half of the hole: an
    # empty pid no longer shares an exit code with a genuine kill, so the
    # premise guard below can actually see it.
    _write_stub "$bin/node" \
        'pid=""' \
        'for _ in $(seq 1 200); do' \
        '    if [ -s "$_PIDFILE" ]; then pid=$(cat "$_PIDFILE"); [ -n "$pid" ] && break; fi' \
        '    sleep 0.05' \
        'done' \
        'if [ -z "$pid" ]; then' \
        '    echo "STUB_ERROR: launcher never published a pid to $_PIDFILE" >&2' \
        '    exit 1' \
        'fi' \
        'kill -9 "$pid" 2>/dev/null || true' \
        'for _ in $(seq 1 200); do kill -0 "$pid" 2>/dev/null || exit 0; sleep 0.05; done' \
        'echo "STUB_ERROR: launcher $pid still alive after 10s" >&2' \
        'exit 1'

    _smoke_arm_run "$bin" "$runner" "$_PORT" 30000 _PIDFILE="$pidfile"
    local rc="$_ARM_RC" out="$_ARM_OUT"

    # Guard the arm's own premise: if the driver stub had bailed out via its
    # error path it would exit 1, and the rc!=0 assertion below would pass for
    # entirely the wrong reason. Absence of STUB_ERROR proves the driver really
    # returned 0.
    assert "$label ARM C: driver stub really reported SUCCESS (rc!=0 below cannot come from the driver)" \
        bash -c '! printf "%s\n" "$1" | grep -qF "STUB_ERROR"' _ "$out"

    assert "$label ARM C: happy-path banner IS present (arm reached the driver, unlike ARM A)" \
        bash -c 'printf "%s\n" "$1" | grep -qF "launcher PID="' _ "$out"

    assert "$label ARM C: exits non-zero — a green driver does not mask a dead launcher" \
        bash -c '[ "$1" -ne 0 ]' _ "$rc"

    assert "$label ARM C: emits the post-driver-phase launcher-death marker" \
        bash -c 'printf "%s\n" "$2" | grep -qF "$1 phase=post-driver rc="' _ "$DEATH_MARKER" "$out"
}

# ===========================================================================
# Sweep every runner through every arm.
#
# Assertion descriptions are prefixed with the runner basename so a failure
# names the offending runner directly.
# ===========================================================================
for _runner_base in "${RUNNERS[@]}"; do
    _runner_path="$REPO_ROOT/gui/test/visual/$_runner_base"

    echo ""
    echo "--- $_runner_base ---"

    assert "$_runner_base: runner exists" \
        test -f "$_runner_path"

    if [ ! -f "$_runner_path" ]; then
        # Nothing to drive; the assert above already recorded the failure.
        continue
    fi

    _arm_a "$_runner_base" "$_runner_path"
    _arm_b "$_runner_base" "$_runner_path"
    _arm_c "$_runner_base" "$_runner_path"
done

test_summary
