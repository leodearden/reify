#!/usr/bin/env bash
# Single-command release-mode launcher for reify-gui (task 2228).
#
# Usage: scripts/run-gui.sh <file.ri>
#
# Performs every build step needed to launch reify-gui from a clean checkout:
#   1. Build the sidecar (idempotent; ~20ms tsup bundle).
#   2. Install gui/ npm deps + build the frontend (produces gui/dist).
#   3. Build the reify-gui cargo binary in release mode (with feature `gui`).
#   4. Export LD_LIBRARY_PATH so OCCT's bundled shared libraries are found.
#   5. exec target/release/reify-gui <file.ri>.
#
# For dev-mode (vite dev server, configurable debug MCP port, devtools)
# use scripts/run-gui-dev.sh instead.

set -euo pipefail

# -- 1. Validate args ----------------------------------------------------------
if [ "$#" -lt 1 ]; then
    echo "Usage: scripts/run-gui.sh <file>" >&2
    echo "" >&2
    echo "  <file>  path to a .ri source file" >&2
    echo "" >&2
    echo "Launches reify-gui in release mode after building all dependencies." >&2
    echo "For dev mode (vite, devtools, MCP debug listener), use run-gui-dev.sh." >&2
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

# -- 2. Build the sidecar (fast, idempotent) -----------------------------------
echo "==> Building sidecar..."
bash gui/sidecar/build-sidecar.sh

# -- 3. Install gui frontend deps + build dist ---------------------------------
echo "==> Installing gui dependencies + building frontend..."
(cd gui && npm install --no-audit --no-fund --silent && npm run build)

# -- 4. Build reify-gui in release mode ----------------------------------------
echo "==> Building reify-gui (release)..."
cargo build -p reify-gui --bin reify-gui --features gui --release

# -- 5. Export OCCT LD_LIBRARY_PATH (snap freecad only) ------------------------
# Mirrors .cargo/run-with-occt.sh; required for direct binary invocation since
# the cargo runner only fires for `cargo run`, not for direct target/* exec.
# Only prepend the snap path if it exists — the PPA install (default in
# scripts/setup-dev.sh) puts OCCT in /usr/lib where the loader finds it
# without help.
SNAP_OCCT_LIB="/snap/freecad/current/usr/lib"
if [ -d "$SNAP_OCCT_LIB" ]; then
    export LD_LIBRARY_PATH="$SNAP_OCCT_LIB${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi

# -- 6. Launch ----------------------------------------------------------------
echo "==> Launching target/release/reify-gui $FILE"
exec target/release/reify-gui "$FILE"
