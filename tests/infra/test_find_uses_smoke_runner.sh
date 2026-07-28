#!/usr/bin/env bash
# Infrastructure guard for gui/test/visual/run_find_uses_smoke.sh (task 4456).
#
# Pins the readiness-race fix (reviewer finding: flaky_test_readiness_race)
# *behaviorally*: it runs the real runner with a launcher stub that dies
# immediately and asserts the liveness guard aborts early (non-zero, fast)
# rather than blocking until the full readiness deadline.
#
# NOTE: deliberately NO source-text/grep assertions. Greppping the runner for
# literal fragments (`kill -0`, `REIFY_SMOKE_WAIT_MS`, the `&` launcher line)
# matches the runner's own header COMMENTS as well as its executable code, so a
# regression that deletes the liveness logic but leaves the descriptive comment
# would keep such contracts green — passing on the very failure they claim to
# pin. The behavioral contract below is the only one that proves behavior.
#
# ── TWO-ARM EXPERIMENT (task 5596) ───────────────────────────────────────────
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
# A narrower regex would only ASSERT discrimination; the pair of arms below
# DEMONSTRATES it:
#   ARM A (positive, death at readiness) — launcher exits 1 immediately:
#       rc != 0, aborts far inside the budget, marker PRESENT.
#   ARM B (negative control, launcher lives) — health + driver stubbed green:
#       rc == 0, startup banner PRESENT (so the arm really ran the normal path),
#       marker ABSENT.
# Arm B is what makes Arm A's assertion non-vacuous: the same output token that
# Arm A demands is proven absent on a run that produces the banner.
#
# Auto-discovered by tests/infra/run_all.sh (matches test_*.sh pattern).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

RUNNER="$REPO_ROOT/gui/test/visual/run_find_uses_smoke.sh"

# The canonical death marker.  Assembled from a variable so every arm below
# tests the SAME token and the positive/negative pair can never drift apart.
DEATH_MARKER="E2E_SMOKE_LAUNCHER_DEATH"

echo "=== test_find_uses_smoke_runner: readiness-race-fix contract ==="

assert "runner exists" \
    test -f "$RUNNER"

# ---------------------------------------------------------------------------
# Shared arm scaffold.
#
# Every arm gets its OWN tmpdir/bin so stubs can never leak between arms, and
# its own debug port so a lingering listener from one arm cannot make another
# arm's health poll succeed by accident.
# ---------------------------------------------------------------------------
_TMPDIRS=()
cleanup() {
    local d
    for d in "${_TMPDIRS[@]:-}"; do
        [ -n "$d" ] && rm -rf "$d"
    done
}
trap cleanup EXIT INT TERM

# _new_bindir — mktemps a fresh <tmpdir>/bin, registers the tmpdir for cleanup,
# and echoes the bin path.
_new_bindir() {
    local d
    d=$(mktemp -d)
    _TMPDIRS+=("$d")
    mkdir -p "$d/bin"
    echo "$d/bin"
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
echo ""
echo "--- ARM A: launcher-death causes early non-zero exit (not a timeout hang) ---"

_a_bin=$(_new_bindir)
_write_stub "$_a_bin/stub_launcher.sh" 'exit 1'
# Stub node: should not be reached; exits 1 if called.
_write_stub "$_a_bin/node" \
    'echo "STUB_ERROR: node driver should not be reached when launcher dies" >&2' \
    'exit 1'

_smoke_arm_run "$_a_bin" "$RUNNER" 59999 600000
_t4_rc="$_ARM_RC"
_t4_out="$_ARM_OUT"
_t4_elapsed="$_ARM_ELAPSED"

assert "runner exits non-zero when launcher dies immediately" \
    bash -c '[ "$1" -ne 0 ]' _ "$_t4_rc"

assert "runner aborts within 15s (liveness guard, not 600s deadline)" \
    bash -c '[ "$1" -lt 15 ]' _ "$_t4_elapsed" # wallclock:allow — liveness: discriminated by rc!=0 + the launcher-death marker, not by elapsed magnitude

# Match the death path's OWN machine marker, not a broad alternation. The runner
# unconditionally announces `launcher PID=<pid>` before the readiness loop is
# even entered, so a pattern with a bare `launcher` branch matches on EVERY run —
# including one where the liveness guard was deleted outright (measured; see the
# header). Only a death path prints this token; ARM B proves it stays absent on
# the happy path.
assert "runner emits the readiness-phase launcher-death marker (not just its startup banner)" \
    bash -c 'printf "%s\n" "$2" | grep -qF "$1 phase=readiness rc="' _ "$DEATH_MARKER" "$_t4_out"

# ===========================================================================
# ARM B — negative control: launcher LIVES, health + driver stubbed green.
#
# `exec sleep N` (not a bare `sleep N &`) so the runner's `$!` IS the sleep
# itself and its SIGTERM teardown reaps it directly rather than orphaning a
# detached child that would keep the output pipe open.
# ===========================================================================
echo ""
echo "--- ARM B: negative control — live launcher, green driver, NO death marker ---"

_b_bin=$(_new_bindir)
_write_stub "$_b_bin/stub_launcher.sh" 'exec sleep 20'
# Health poll succeeds immediately.
_write_stub "$_b_bin/curl" 'exit 0'
# Driver succeeds.
_write_stub "$_b_bin/node" 'exit 0'

_smoke_arm_run "$_b_bin" "$RUNNER" 59998 30000
_b_rc="$_ARM_RC"
_b_out="$_ARM_OUT"

assert "control: runner exits 0 on the happy path (live launcher, green driver)" \
    bash -c '[ "$1" -eq 0 ]' _ "$_b_rc"

assert "control: happy path DOES print the startup banner (arm really ran the normal path)" \
    bash -c 'printf "%s\n" "$1" | grep -qF "launcher PID="' _ "$_b_out"

assert "control: happy path emits NO launcher-death marker (so ARM A's predicate is not vacuous)" \
    bash -c '! printf "%s\n" "$2" | grep -qF "$1"' _ "$DEATH_MARKER" "$_b_out"

test_summary
