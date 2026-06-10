#!/usr/bin/env bash
# Self-launching one-command acceptance runner for the task-4404 diagnostics e2e smoke.
#
# Usage:
#   bash gui/test/visual/run_diagnostics_e2e_smoke.sh
#   # or via npm:
#   npm --prefix gui run test:smoke:diagnostics
#
# The runner:
#   1. Resolves REIFY_DEBUG_PORT (env if valid 1..65535, else allocates a free port).
#   2. Sets DISPLAY="${DISPLAY:-:0}" so the Tauri webview can instantiate.
#   3. (Unless REIFY_SMOKE_SKIP_PREBUILD=1) runs synchronous pre-build steps —
#      sidecar npm install, sidecar build, gui npm install, cargo build reify-gui —
#      so the cold-build cost is paid OUTSIDE the readiness window.
#   4. Backgrounds REIFY_SMOKE_LAUNCHER (default: scripts/run-gui-dev.sh) with the
#      fixture + REIFY_DEBUG=1 / REIFY_DEBUG_PORT=$PORT.
#   5. Polls the debug health endpoint up to REIFY_SMOKE_WAIT_MS (default 180000ms)
#      with a kill -0 liveness check on each iteration — aborts early if the launcher
#      dies rather than waiting the full budget.
#   6. Runs `node gui/test/visual/smoke_diagnostics_e2e.mjs` (the 4404 driver).
#      Note: the driver contains its own waitForServer(60_000) readiness loop —
#      that loop passes instantly because step 5 already confirmed the server is
#      ready.  Both readiness gates are intentional and serve distinct purposes:
#      step 5 polls with a kill -0 liveness check (bash-side, driver-agnostic);
#      the driver's waitForServer is part of its own unchanged contract (design
#      decision 2, task 4456).  Do NOT remove one believing the other is dead code.
#   7. SIGTERMs the launcher so its trap reaps both vite and reify-gui, then exits
#      with the driver's exit code.
#
# An EXIT/INT/TERM trap ensures the GUI is reaped even on early failure.
#
# Lifecycle design mirrors gui/test/visual/run.ts spawnGui/waitForDebugServer/reapGui.
# Adapted from run_find_uses_smoke.sh (task-4202); only the fixture and driver differ.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
cd "$REPO_ROOT"

# ---------------------------------------------------------------------------
# 1. Resolve REIFY_DEBUG_PORT
#    Mirrors endpoint.ts resolveDebugPort / allocateFreePort semantics and
#    the setup-worktree-debug-port.sh contract: strict ^[0-9]+$ pattern,
#    value 1..65535, no whitespace.
# ---------------------------------------------------------------------------
resolve_port() {
    local raw="${REIFY_DEBUG_PORT:-}"
    if [[ "$raw" =~ ^[0-9]+$ ]] && [ "$raw" -ge 1 ] && [ "$raw" -le 65535 ]; then
        echo "$raw"
        return
    fi
    # Fall back to allocating a free ephemeral port (lib_portable.sh pattern).
    # shellcheck source=../../../scripts/lib_portable.sh
    source "$REPO_ROOT/scripts/lib_portable.sh"
    allocate_free_port
}

PORT=$(resolve_port)
export REIFY_DEBUG_PORT="$PORT"
echo "run_diagnostics_e2e_smoke: using debug port $PORT"

# ---------------------------------------------------------------------------
# 2. Ensure an X display is available (Tauri webview needs one).
#    :0 is the host display dispatched agents share.
# ---------------------------------------------------------------------------
export DISPLAY="${DISPLAY:-:0}"

# ---------------------------------------------------------------------------
# 2a. Ensure /opt/reify-deps/lib is on LD_LIBRARY_PATH when present.
#     libopenvdb.so.13+ links against TBB 12.18; on hosts where only
#     TBB 12.11 is installed at the system path the binary fails with
#     "undefined symbol" at startup.  This mirrors the .cargo/run-with-occt.sh
#     logic (which fires for `cargo run` but NOT for direct binary invocations).
# ---------------------------------------------------------------------------
REIFY_DEPS_LIB="/opt/reify-deps/lib"
if [ -d "$REIFY_DEPS_LIB" ] && ls "$REIFY_DEPS_LIB"/libTKernel.so* >/dev/null 2>&1; then
    export LD_LIBRARY_PATH="$REIFY_DEPS_LIB${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi

# ---------------------------------------------------------------------------
# 2b. Disable WebKit GBM/DMABuf renderer on headless or DRI-unavailable hosts.
#     On systems where the Nvidia driver exposes DRI fds but the mesa EGL
#     GBM backend cannot create a screen (pci id 10de:…, driver null), WebKit
#     crashes with "Could not create GBM EGL display: EGL_NOT_INITIALIZED".
#     WEBKIT_DISABLE_DMABUF_RENDERER=1 forces fallback to the GLX/xlib path,
#     which works correctly against DISPLAY=:0.  This matches the approach used
#     by the Tauri upstream CI and other headless Tauri test harnesses.
# ---------------------------------------------------------------------------
export WEBKIT_DISABLE_DMABUF_RENDERER="${WEBKIT_DISABLE_DMABUF_RENDERER:-1}"

# ---------------------------------------------------------------------------
# 3. Trap for cleanup — SIGTERM the launcher on exit.
# ---------------------------------------------------------------------------
GUI_LAUNCHER_PID=""
cleanup() {
    if [ -n "$GUI_LAUNCHER_PID" ]; then
        kill "$GUI_LAUNCHER_PID" 2>/dev/null || true
        wait "$GUI_LAUNCHER_PID" 2>/dev/null || true
        GUI_LAUNCHER_PID=""
    fi
}
trap cleanup EXIT
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM

# ---------------------------------------------------------------------------
# 4. Synchronous pre-build (unless REIFY_SMOKE_SKIP_PREBUILD=1).
#    These steps mirror run-gui-dev.sh's own build sequence; they are
#    idempotent, so the backgrounded launcher re-runs them as fast no-ops.
#    Paying the cold-build cost HERE moves it OUTSIDE the readiness window.
# ---------------------------------------------------------------------------
if [ "${REIFY_SMOKE_SKIP_PREBUILD:-0}" != "1" ]; then
    echo "run_diagnostics_e2e_smoke: pre-building sidecar + gui + reify-gui binary…"
    (cd "$REPO_ROOT/gui/sidecar" && npm install --no-audit --no-fund --silent)
    bash "$REPO_ROOT/gui/sidecar/build-sidecar.sh"
    (cd "$REPO_ROOT/gui" && npm install --no-audit --no-fund --silent)
    cargo build -p reify-gui --bin reify-gui --features gui
    echo "run_diagnostics_e2e_smoke: pre-build complete"
fi

# ---------------------------------------------------------------------------
# 5. Background the launcher (overridable for testing via REIFY_SMOKE_LAUNCHER).
#    REIFY_DEBUG=1 enables the MCP debug listener on port $PORT.
#    The launcher (run-gui-dev.sh) owns reaping of vite+reify-gui via its trap.
# ---------------------------------------------------------------------------
LAUNCHER="${REIFY_SMOKE_LAUNCHER:-$REPO_ROOT/scripts/run-gui-dev.sh}"
FIXTURE="$REPO_ROOT/gui/test/fixtures/diagnostics_main.ri"
echo "run_diagnostics_e2e_smoke: launching GUI with fixture: $FIXTURE"
REIFY_DEBUG=1 REIFY_DEBUG_PORT="$PORT" \
    bash "$LAUNCHER" "$FIXTURE" &
GUI_LAUNCHER_PID=$!
echo "run_diagnostics_e2e_smoke: launcher PID=$GUI_LAUNCHER_PID"

# ---------------------------------------------------------------------------
# 6. Bash-side readiness + liveness gate.
#    Poll the debug health endpoint up to REIFY_SMOKE_WAIT_MS (default 180000).
#    On each iteration also check kill -0 $GUI_LAUNCHER_PID — abort early if
#    the launcher has died rather than waiting the full budget.
# ---------------------------------------------------------------------------
WAIT_MS="${REIFY_SMOKE_WAIT_MS:-180000}"
# Convert to whole seconds (ceil).
WAIT_SEC=$(( (WAIT_MS + 999) / 1000 ))
HEALTH_URL="http://127.0.0.1:${PORT}/mcp"
HEALTH_BODY='{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"health","arguments":{}}}'

echo "run_diagnostics_e2e_smoke: waiting up to ${WAIT_SEC}s for debug server on port ${PORT}…"
_start=$SECONDS
_server_ready=0
while [ $(( SECONDS - _start )) -lt "$WAIT_SEC" ]; do
    # Liveness check — abort early if the launcher died.
    if ! kill -0 "$GUI_LAUNCHER_PID" 2>/dev/null; then
        _launcher_rc=0
        wait "$GUI_LAUNCHER_PID" 2>/dev/null || _launcher_rc=$?
        GUI_LAUNCHER_PID=""
        echo "run_diagnostics_e2e_smoke: launcher exited early (rc=${_launcher_rc})" >&2
        # Treat launcher-exited-before-ready as failure regardless of exit code.
        # A launcher exit-0 without a bound debug server means the driver never ran
        # and readiness was never confirmed — that is a false PASS if we propagate 0.
        exit "$(( _launcher_rc == 0 ? 1 : _launcher_rc ))"
    fi
    # Health poll.
    if curl -fsS -X POST "$HEALTH_URL" \
            -H 'Content-Type: application/json' \
            -d "$HEALTH_BODY" \
            --max-time 2 \
            >/dev/null 2>&1; then
        _server_ready=1
        break
    fi
    sleep 0.5
done

if [ "$_server_ready" -ne 1 ]; then
    echo "run_diagnostics_e2e_smoke: debug server did not become ready within ${WAIT_SEC}s" >&2
    exit 1
fi
echo "run_diagnostics_e2e_smoke: debug server ready ($(( SECONDS - _start ))s elapsed)"

# ---------------------------------------------------------------------------
# 7. Run the 4404 diagnostics e2e smoke driver.
#    The driver's own waitForServer passes instantly (server already ready).
# ---------------------------------------------------------------------------
echo "run_diagnostics_e2e_smoke: running smoke driver…"
DRIVER_RC=0
REIFY_DEBUG_PORT="$PORT" node "$REPO_ROOT/gui/test/visual/smoke_diagnostics_e2e.mjs" || DRIVER_RC=$?

# ---------------------------------------------------------------------------
# 8. Reap the GUI launcher (its trap reaps both vite and reify-gui).
# ---------------------------------------------------------------------------
if [ -n "$GUI_LAUNCHER_PID" ]; then
    echo "run_diagnostics_e2e_smoke: sending SIGTERM to launcher (PID=$GUI_LAUNCHER_PID)"
    kill "$GUI_LAUNCHER_PID" 2>/dev/null || true
    wait "$GUI_LAUNCHER_PID" 2>/dev/null || true
    GUI_LAUNCHER_PID=""
fi

exit "$DRIVER_RC"
