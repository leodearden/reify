#!/usr/bin/env bash
# Single-command dev-mode launcher for reify-gui (task 2228).
#
# Usage: scripts/run-gui-dev.sh <file.ri>
#
# Performs every build step needed to launch reify-gui in dev mode:
#   1. Install gui/sidecar/ npm deps (tsup needs typescript at runtime).
#   2. Build the sidecar (idempotent; ~20ms tsup bundle).
#   3. Install gui/ npm deps (vite needs them).
#   4. Start the vite dev server in the background and wait for :${REIFY_VITE_PORT:-1420}.
#   5. Build the reify-gui cargo binary in DEBUG profile (with feature `gui`).
#   6. Export REIFY_DEBUG=1 + OCCT LD_LIBRARY_PATH.
#   7. Run target/debug/reify-gui <file.ri> as a backgrounded child and
#      `wait`, so SIGTERM/SIGINT to this script reach the trap which reaps
#      both vite and reify-gui.
#
# IMPORTANT: this script does NOT `exec` the GUI binary. `exec` would replace
# the shell process with the binary, killing the trap that reaps vite.
# We background reify-gui and `wait` so signals delivered to the script
# trigger cleanup of BOTH children — otherwise an external `kill` of the
# script orphans reify-gui (it survives showing "connection refused" once
# vite is reaped).

set -euo pipefail

# Snapshot the caller's LD_LIBRARY_PATH before §7 rewrites it, so §7 can tell
# whether it inherited one and only then explain that it prepended the tbb pin.
_INHERITED_LD_LIBRARY_PATH="${LD_LIBRARY_PATH:-}"

# -- 1. Validate args ---------------------------------------------------------
if [ "$#" -lt 1 ]; then
    echo "Usage: scripts/run-gui-dev.sh <file>" >&2
    echo "" >&2
    echo "  <file>  path to a .ri source file" >&2
    echo "" >&2
    echo "Launches reify-gui in dev mode (vite dev server on :1420 by default, devtools," >&2
    echo "MCP debug listener on :\${REIFY_DEBUG_PORT:-3939} via REIFY_DEBUG=1)." >&2
    echo "For release mode, use scripts/run-gui.sh." >&2
    exit 1
fi

FILE="$1"

case "$FILE" in
    *.ri) ;;
    *)
        echo "Error: file must have .ri extension: $FILE" >&2
        exit 1
        ;;
esac

[ -f "$FILE" ] || { echo "Error: file not found: $FILE" >&2; exit 1; }

# Resolve repo root from this script's path so the script works from any cwd.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

# Vite dev server port: default 1420, overridable via REIFY_VITE_PORT.
# Used by tests/infra/test_run_gui_scripts.sh Test 25 to avoid collisions
# with another worktree's vite already bound to :1420. See task 2308.
REIFY_VITE_PORT="${REIFY_VITE_PORT:-1420}"

# Debug server port: default 3939, overridable via REIFY_DEBUG_PORT.
# Set per worktree to avoid collisions when running concurrent GUI smokes.
# Exported so reify-gui binds the chosen port and the sidecar inherits it.
REIFY_DEBUG_PORT="${REIFY_DEBUG_PORT:-3939}"
export REIFY_DEBUG_PORT

# -- 1b. Preflight: refuse to run into a known-bad launch --------------------
# These checks are deliberately the FIRST thing after arg/port resolution and
# before every expensive step (npm install, sidecar build, vite spawn, and a
# cargo build that can take minutes). Each failure below is one a user would
# otherwise only see after that wait.
#
# Set REIFY_GUI_SKIP_PREFLIGHT=1 to bypass the whole block (break-glass).
if [ "${REIFY_GUI_SKIP_PREFLIGHT:-}" != "1" ]; then
    # Display. reify-gui is a GTK/WebKit app; with no X11 or Wayland display it
    # cannot open a window at all, and the failure otherwise surfaces only
    # AFTER the cargo build as an opaque GTK abort.
    #
    # We deliberately do NOT probe EGL here: `eglinfo` (mesa-utils) is not in
    # scripts/setup-dev.sh's package set, so it cannot be hard-required, and
    # §7's WEBKIT_DISABLE_DMABUF_RENDERER=1 default already neutralises the
    # NVIDIA+Mesa GBM failure an EGL probe would flag — the probe would be both
    # undependable and a false alarm.
    if [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
        echo "Error: no display: DISPLAY and WAYLAND_DISPLAY are both unset — reify-gui needs an X11/Wayland display; export DISPLAY=:0 (or set REIFY_GUI_SKIP_PREFLIGHT=1 to bypass)" >&2
        exit 1
    fi

    # Vite port occupancy. §5's readiness loop calls curl BEFORE checking
    # `kill -0 "$VITE_PID"`, so a FOREIGN listener on this port makes curl
    # succeed on iteration 1 — while the vite we spawn has already exited on
    # vite.config.ts's `strictPort: true`. The script would then pair a fresh
    # reify-gui with a stale, unrelated vite serving another worktree's build.
    # A pre-spawn occupancy check is the only ordering that closes that.
    #
    # `|| true` keeps the probe set -e-safe: a FAILING curl is the normal,
    # healthy path (nothing is listening yet).
    _preflight_port_rc=0
    curl -fsS --max-time 2 "http://127.0.0.1:$REIFY_VITE_PORT/" >/dev/null 2>&1 \
        || _preflight_port_rc=$?
    if [ "$_preflight_port_rc" -eq 0 ]; then
        echo "Error: vite port $REIFY_VITE_PORT is already in use — something is already serving http://127.0.0.1:$REIFY_VITE_PORT/" >&2
        # Best-effort listener pid. `ss` is not guaranteed present, and its
        # absence must never fail the script — the message above already
        # stands on its own.
        _preflight_listener=""
        if command -v ss >/dev/null 2>&1; then
            _preflight_listener="$(ss -ltnpH "sport = :$REIFY_VITE_PORT" 2>/dev/null \
                | grep -oE 'pid=[0-9]+' | head -1 | cut -d= -f2 || true)"
        fi
        if [ -n "$_preflight_listener" ]; then
            echo "  Listener pid $_preflight_listener (\`ls -l /proc/$_preflight_listener/cwd\` shows which worktree it serves); free it with \`kill $_preflight_listener\`, or pick another port with REIFY_VITE_PORT=<port>." >&2
        else
            echo "  Free the port, or pick another one with REIFY_VITE_PORT=<port>." >&2
        fi
        exit 1
    fi
fi

# -- 2. Install sidecar npm deps ---------------------------------------------
# build-sidecar.sh runs `npx tsup`, which requires `typescript` to be present
# in gui/sidecar/node_modules. On a fresh checkout (or fresh worktree) this
# directory doesn't exist yet, so install before building. Idempotent.
echo "==> Installing sidecar dependencies..."
(cd gui/sidecar && npm install --no-audit --no-fund --silent)

# -- 3. Build the sidecar -----------------------------------------------------
echo "==> Building sidecar..."
bash gui/sidecar/build-sidecar.sh

# -- 3. Install gui frontend deps (vite needs them) ---------------------------
echo "==> Installing gui dependencies..."
(cd gui && npm install --no-audit --no-fund --silent)

# -- 4. Start vite dev server in background -----------------------------------
# IMPORTANT: We use `pushd`/`popd` instead of `(cd gui && npm run dev ...) &`.
# The subshell-based form sets `$!` to the SUBSHELL's pid, not npm's, so the
# EXIT trap's `kill "$VITE_PID"` may signal the subshell while leaving the
# real npm/vite process alive — vite then keeps :1420 bound and the next dev
# run fails to start. With pushd/popd, `$!` points directly at npm.
#
# We additionally spawn npm in its OWN PROCESS GROUP, via the `set -m` idiom
# borrowed from lib_proc_reaper.sh's reaper_run_in_pgroup. Under monitor mode
# bash puts a background job in a fresh group whose PGID equals `$!`, so
# VITE_PID doubles as the PGID and cleanup() can signal the whole tree. Without
# it, `npm run dev` forks an intermediate `sh -c` which forks the node/vite
# process that actually holds the port, and that GRANDCHILD outlives every
# signal cleanup() can address (see the note there).
echo "==> Starting vite dev server on :$REIFY_VITE_PORT..."
# Save the caller's monitor-mode state so we restore exactly what we found.
_had_monitor=0
case $- in *m*) _had_monitor=1 ;; esac
set -m 2>/dev/null || true
VITE_PGROUP=0
case $- in *m*) VITE_PGROUP=1 ;; esac
if [ "$VITE_PGROUP" -eq 0 ]; then
    # Monitor mode unavailable: npm shares OUR process group, so a group kill
    # would signal this script too. cleanup() falls back to `pkill -P` in that
    # case — degraded (the grandchild can survive), so say so out loud.
    echo "Warning: 'set -m' unavailable; vite teardown degraded to direct children only — a vite grandchild may survive and keep :$REIFY_VITE_PORT bound" >&2
fi
pushd gui >/dev/null
npm run dev -- --port "$REIFY_VITE_PORT" &
VITE_PID=$!
popd >/dev/null
if [ "$_had_monitor" -eq 0 ]; then set +m 2>/dev/null || true; fi

# Install cleanup trap to reap BOTH vite and reify-gui on every termination
# path. This MUST stay active for the whole script — we deliberately do NOT
# exec the GUI binary so this trap fires when reify-gui exits, when the user
# Ctrl-C's, or when an external supervisor `kill`s the script.
#
# Why we reap reify-gui too: bash does NOT propagate SIGTERM to foreground
# children by default. Without this, `kill <script-pid>` reaps vite via the
# trap but orphans reify-gui — the window survives, displays "Connection
# refused" once vite dies, and squats on resources.
#
# We reap vite by PROCESS GROUP, not by pid. `npm run dev` forks an
# intermediate `sh -c` which forks the node/vite process that actually holds the
# port, so `pkill -P "$VITE_PID"` — which reaches npm's DIRECT children only —
# leaves that grandchild alive; it then squats on the port and the next dev run
# fails to start. A process group persists while ANY member lives and its PGID
# survives re-parenting, so the group kill still reaches the grandchild after
# npm itself has exited. TERM first, then a short grace, then KILL.
#
# The grace is ~2s rather than lib_proc_reaper.sh's 10s default: this is an
# interactive launcher, and a Ctrl-C that takes ten seconds to return the
# prompt reads as a hang.
#
# `pkill -P "$VITE_PID"` + `kill "$VITE_PID"` are RETAINED as the fallback for
# the case where `set -m` was unavailable and VITE_PID is therefore not a PGID
# of our own making — there, a group kill would target our own group, so we must
# not attempt one.
GUI_PID=""
cleanup() {
    if [ -n "$GUI_PID" ]; then
        kill "$GUI_PID" 2>/dev/null || true
        wait "$GUI_PID" 2>/dev/null || true
    fi
    if [ "${VITE_PGROUP:-0}" -eq 1 ]; then
        kill -TERM -- -"$VITE_PID" 2>/dev/null || true
        # Bounded grace: poll so a fast exit returns promptly instead of always
        # paying the full 2s.
        for _ in $(seq 1 20); do
            kill -0 -- -"$VITE_PID" 2>/dev/null || break
            sleep 0.1
        done
        kill -KILL -- -"$VITE_PID" 2>/dev/null || true
    else
        pkill -P "$VITE_PID" 2>/dev/null || true
        kill "$VITE_PID" 2>/dev/null || true
    fi
    wait "$VITE_PID" 2>/dev/null || true
}
trap cleanup EXIT
# Forward SIGINT/SIGTERM to children explicitly. Bash's default behavior is
# to wait for foreground children before honoring the signal; backgrounding
# reify-gui + `wait` (below) plus these handlers ensures clean shutdown.
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM

# -- 5. Wait for vite readiness ----------------------------------------------
echo "==> Waiting for vite at http://127.0.0.1:$REIFY_VITE_PORT/ ..."
VITE_READY=0
for _ in $(seq 1 60); do
    if curl -fsS "http://127.0.0.1:$REIFY_VITE_PORT/" >/dev/null 2>&1; then
        VITE_READY=1
        break
    fi
    if ! kill -0 "$VITE_PID" 2>/dev/null; then
        vite_rc=0
        wait "$VITE_PID" 2>/dev/null || vite_rc=$?
        echo "Error: vite process exited (rc=$vite_rc) before becoming ready; check vite output above" >&2
        exit 1
    fi
    sleep 0.5
done

if [ "$VITE_READY" -ne 1 ]; then
    echo "Error: vite dev server did not become ready on 127.0.0.1:$REIFY_VITE_PORT within 30s" >&2
    exit 1
fi

# -- 6. Build reify-gui in DEBUG profile -------------------------------------
# Debug profile is required so Tauri's runtime selects `devUrl` (vite) instead
# of `frontendDist` — see tauri.conf.json. cfg!(debug_assertions) drives this.
echo "==> Building reify-gui (debug)..."
cargo build -p reify-gui --bin reify-gui --features gui

# -- 7. Set debug-mode env vars ----------------------------------------------
# REIFY_DEBUG=1 enables the MCP debug listener on 127.0.0.1:$REIFY_DEBUG_PORT
# (see gui/src-tauri/src/main.rs). LD_LIBRARY_PATH is required for direct
# binary invocation since the cargo runner only fires for `cargo run`.
export REIFY_DEBUG=1
# Only prepend the snap path if it exists — the PPA install (default in
# scripts/setup-dev.sh) puts OCCT in /usr/lib where the loader finds it
# without help.
SNAP_OCCT_LIB="/snap/freecad/current/usr/lib"
if [ -d "$SNAP_OCCT_LIB" ]; then
    export LD_LIBRARY_PATH="$SNAP_OCCT_LIB${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi

# Pin oneTBB ahead of everything else — INCLUDING whatever LD_LIBRARY_PATH we
# were handed. This block is deliberately OUTSIDE the snap conditional above:
# the snap dir does not exist on a PPA host, so anything nested in it would
# never run there.
#
# #5192 already puts $TBB_PIN_DIR first in each binary's DT_RUNPATH, but the
# loader searches LD_LIBRARY_PATH BEFORE DT_RUNPATH. So a caller exporting
# LD_LIBRARY_PATH=/usr/lib/x86_64-linux-gnu:... silently defeats that pin: the
# binary binds the system libtbb 12.11 instead of the deps 12.18 and dies on a
# missing symbol. Leading with the pin dir restores #5192's guarantee while
# PRESERVING the caller's entries — we never scrub or reorder them. (#7254)
# Disable WebKit's GBM/DMABuf renderer by default. On systems where the NVIDIA
# driver exposes DRI fds but the Mesa EGL GBM backend cannot create a screen,
# WebKitGTK aborts with "Could not create GBM EGL display: EGL_NOT_INITIALIZED".
# =1 forces the GLX/xlib fallback path. Mirrors gui/test/visual/lib_e2e_smoke.sh
# §2b. The `:-1` form is deliberate: export WEBKIT_DISABLE_DMABUF_RENDERER=0 to
# restore the DMABUF path on a host where it works.
export WEBKIT_DISABLE_DMABUF_RENDERER="${WEBKIT_DISABLE_DMABUF_RENDERER:-1}"

TBB_PIN_DIR="/opt/reify-deps/tbb-pin"
if [ -d "$TBB_PIN_DIR" ]; then
    export LD_LIBRARY_PATH="$TBB_PIN_DIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    if [ -n "$_INHERITED_LD_LIBRARY_PATH" ]; then
        echo "==> Note: inherited LD_LIBRARY_PATH preserved, but $TBB_PIN_DIR prepended ahead of it (the loader searches LD_LIBRARY_PATH before DT_RUNPATH, so an inherited /usr/lib path would otherwise bind system libtbb 12.11 over the deps 12.18 — see #5192/#7254)"
    fi
fi

# -- 8. Run reify-gui as a backgrounded CHILD (not exec) ---------------------
# Critical: do NOT use `exec` here — `exec` replaces the shell process with
# the binary, which kills the trap that should reap vite.
#
# We background reify-gui (rather than running it foreground) so that:
#   - The script's own SIGTERM/SIGINT handlers fire promptly (foreground
#     children block bash's signal delivery until they exit).
#   - The cleanup trap can kill GUI_PID explicitly when the script is
#     killed externally, instead of orphaning the GUI window.
echo "==> Launching target/debug/reify-gui $FILE (REIFY_DEBUG=1, port=$REIFY_DEBUG_PORT)"
target/debug/reify-gui "$FILE" &
GUI_PID=$!
RC=0
wait "$GUI_PID" || RC=$?
GUI_PID=""  # already reaped; suppress double-kill in cleanup

# Trap will reap vite on exit; propagate the binary's exit code.
exit "$RC"
