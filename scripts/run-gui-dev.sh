#!/usr/bin/env bash
# Single-command dev-mode launcher for reify-gui (task 2228).
#
# Usage: scripts/run-gui-dev.sh <file.ri>
#
# Performs every build step needed to launch reify-gui in dev mode:
#   1. Build the sidecar (idempotent; ~20ms tsup bundle).
#   2. Install gui/ npm deps (vite needs them).
#   3. Start the vite dev server in the background and wait for :1420.
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
    echo "Launches reify-gui in dev mode (vite dev server on :1420, devtools," >&2
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

# Resolve repo root from this script's path so the script works from any cwd.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

# -- 2. Build the sidecar -----------------------------------------------------
echo "==> Building sidecar..."
bash gui/sidecar/build-sidecar.sh

# -- 3. Install gui frontend deps (vite needs them) ---------------------------
echo "==> Installing gui dependencies..."
(cd gui && npm install --no-audit --no-fund --silent)

# -- 4. Start vite dev server in background -----------------------------------
echo "==> Starting vite dev server on :1420..."
(cd gui && npm run dev -- --port 1420) &
VITE_PID=$!

# Install EXIT trap to reap vite on every termination path. This MUST stay
# active for the whole script — we deliberately do NOT exec the GUI binary
# so this trap fires when reify-gui exits (or when the user Ctrl-C's during
# the polling loop, etc.).
trap 'kill "$VITE_PID" 2>/dev/null || true; wait "$VITE_PID" 2>/dev/null || true' EXIT

# -- 5. Wait for vite readiness ----------------------------------------------
echo "==> Waiting for vite at http://127.0.0.1:1420/ ..."
VITE_READY=0
for _ in $(seq 1 60); do
    if curl -fsS http://127.0.0.1:1420/ >/dev/null 2>&1; then
        VITE_READY=1
        break
    fi
    sleep 0.5
done

if [ "$VITE_READY" -ne 1 ]; then
    echo "Error: vite dev server did not become ready on 127.0.0.1:1420 within 30s" >&2
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
