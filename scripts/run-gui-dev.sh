#!/usr/bin/env bash
# Single-command dev-mode launcher for reify-gui (task 2228).
#
# Usage: scripts/run-gui-dev.sh <file.ri>
#
# Performs every build step needed to launch reify-gui in dev mode:
#   1. Build the sidecar (idempotent; ~20ms tsup bundle).
#   2. Install gui/ npm deps (vite needs them).
#   3. Start the vite dev server in the background and wait for :${REIFY_VITE_PORT:-1420}.
#   4. Build the reify-gui cargo binary in DEBUG profile (with feature `gui`).
#   5. Export REIFY_DEBUG=1 + OCCT LD_LIBRARY_PATH.
#   6. Run target/debug/reify-gui <file.ri> AS A CHILD (NOT exec) so the
#      EXIT trap fires and we can reap vite cleanly.
#
# IMPORTANT: this script does NOT `exec` the GUI binary. `exec` would replace
# the shell process with the binary, killing the EXIT trap that reaps the
# vite background process. We run it as a child, capture its exit code,
# explicitly kill vite, and propagate the binary's exit code.

set -euo pipefail

# -- 1. Validate args ---------------------------------------------------------
if [ "$#" -lt 1 ]; then
    echo "Usage: scripts/run-gui-dev.sh <file>" >&2
    echo "" >&2
    echo "  <file>  path to a .ri source file" >&2
    echo "" >&2
    echo "Launches reify-gui in dev mode (vite dev server on :1420 by default, devtools," >&2
    echo "MCP debug listener on :3939 via REIFY_DEBUG=1)." >&2
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

# -- 2. Build the sidecar -----------------------------------------------------
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

# Install EXIT trap to reap vite on every termination path. This MUST stay
# active for the whole script — we deliberately do NOT exec the GUI binary
# so this trap fires when reify-gui exits (or when the user Ctrl-C's during
# the polling loop, etc.).
#
# We reap descendants first via `pkill -P "$VITE_PID"`: npm typically forks
# vite as a child, and signaling only npm can leave vite holding the port.
# `pkill -P` is best-effort (may not be on every system); the `|| true` keeps
# the trap robust under set -e.
trap 'pkill -P "$VITE_PID" 2>/dev/null || true; kill "$VITE_PID" 2>/dev/null || true; wait "$VITE_PID" 2>/dev/null || true' EXIT

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
# REIFY_DEBUG=1 enables the MCP debug listener on 127.0.0.1:3939 (see
# gui/src-tauri/src/main.rs). LD_LIBRARY_PATH is required for direct binary
# invocation since the cargo runner only fires for `cargo run`.
export REIFY_DEBUG=1
export LD_LIBRARY_PATH="/snap/freecad/current/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

# -- 8. Run reify-gui as a CHILD (not exec) ----------------------------------
# Critical: do NOT use `exec` here — `exec` replaces the shell process with
# the binary, which kills the EXIT trap that should reap vite.
echo "==> Launching target/debug/reify-gui $FILE (REIFY_DEBUG=1)"
RC=0
target/debug/reify-gui "$FILE" || RC=$?

# Trap will reap vite on exit; propagate the binary's exit code.
exit "$RC"
