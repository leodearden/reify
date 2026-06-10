#!/usr/bin/env bash
# Self-launching one-command acceptance runner for the task-4202 Find-uses smoke.
#
# Usage:
#   bash gui/test/visual/run_find_uses_smoke.sh
#   # or via npm:
#   npm --prefix gui run test:smoke:find-uses
#
# The runner:
#   1. Resolves REIFY_DEBUG_PORT (env if valid 1..65535, else allocates a free port).
#   2. Sets DISPLAY="${DISPLAY:-:0}" so the Tauri webview can instantiate.
#   3. Backgrounds scripts/run-gui-dev.sh with the fixture + REIFY_DEBUG=1 /
#      REIFY_DEBUG_PORT=$PORT (run-gui-dev.sh reaps both vite and reify-gui via
#      its own SIGTERM/EXIT trap — we do not reimplement process-tree reaping).
#   4. Runs `node gui/test/visual/smoke_find_uses.mjs` (the 4202 driver; it has
#      its own 60s waitForServer health poll).
#   5. On driver exit, SIGTERMs the run-gui-dev.sh process so its trap reaps
#      both children, then exits with the driver's exit code.
#
# An EXIT/INT/TERM trap ensures the GUI is reaped even on early failure.
#
# Lifecycle design mirrors gui/test/visual/run.ts spawnGui/waitForDebugServer/reapGui.

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
    # source the helper so allocate_free_port() is available.
    # shellcheck source=../../../scripts/lib_portable.sh
    source "$REPO_ROOT/scripts/lib_portable.sh"
    allocate_free_port
}

PORT=$(resolve_port)
export REIFY_DEBUG_PORT="$PORT"
echo "run_find_uses_smoke: using debug port $PORT"

# ---------------------------------------------------------------------------
# 2. Ensure an X display is available (Tauri webview needs one).
#    :0 is the host display dispatched agents share; task 4202 launched on it.
# ---------------------------------------------------------------------------
export DISPLAY="${DISPLAY:-:0}"

# ---------------------------------------------------------------------------
# 3. Trap for cleanup — SIGTERM the run-gui-dev.sh process group on exit.
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
# 4. Background run-gui-dev.sh with the fixture.
#    REIFY_DEBUG=1 enables the MCP debug listener on port $PORT.
#    run-gui-dev.sh owns reaping of both vite and reify-gui via its own trap.
# ---------------------------------------------------------------------------
FIXTURE="$REPO_ROOT/gui/test/fixtures/find_uses_smoke.ri"
echo "run_find_uses_smoke: launching GUI with fixture: $FIXTURE"
REIFY_DEBUG=1 REIFY_DEBUG_PORT="$PORT" \
    bash "$REPO_ROOT/scripts/run-gui-dev.sh" "$FIXTURE" &
GUI_LAUNCHER_PID=$!
echo "run_find_uses_smoke: run-gui-dev.sh PID=$GUI_LAUNCHER_PID"

# ---------------------------------------------------------------------------
# 5. Run the 4202 drive-only smoke driver.
#    The driver has its own 60s waitForServer health poll.
# ---------------------------------------------------------------------------
echo "run_find_uses_smoke: running smoke driver…"
DRIVER_RC=0
REIFY_DEBUG_PORT="$PORT" node "$REPO_ROOT/gui/test/visual/smoke_find_uses.mjs" || DRIVER_RC=$?

# ---------------------------------------------------------------------------
# 6. Reap the GUI launcher (its trap reaps both vite and reify-gui).
# ---------------------------------------------------------------------------
if [ -n "$GUI_LAUNCHER_PID" ]; then
    echo "run_find_uses_smoke: sending SIGTERM to run-gui-dev.sh (PID=$GUI_LAUNCHER_PID)"
    kill "$GUI_LAUNCHER_PID" 2>/dev/null || true
    wait "$GUI_LAUNCHER_PID" 2>/dev/null || true
    GUI_LAUNCHER_PID=""
fi

exit "$DRIVER_RC"
