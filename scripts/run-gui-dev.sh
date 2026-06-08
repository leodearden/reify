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
echo "==> Starting vite dev server on :$REIFY_VITE_PORT..."
pushd gui >/dev/null
npm run dev -- --port "$REIFY_VITE_PORT" &
VITE_PID=$!
popd >/dev/null

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
# We reap descendants first via `pkill -P "$VITE_PID"`: npm typically forks
# vite as a child, and signaling only npm can leave vite holding the port.
# `pkill -P` is best-effort (may not be on every system); the `|| true` keeps
# the trap robust under set -e.
GUI_PID=""
cleanup() {
    if [ -n "$GUI_PID" ]; then
        kill "$GUI_PID" 2>/dev/null || true
        wait "$GUI_PID" 2>/dev/null || true
    fi
    pkill -P "$VITE_PID" 2>/dev/null || true
    kill "$VITE_PID" 2>/dev/null || true
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
