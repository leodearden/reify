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

# Snapshot the caller's LD_LIBRARY_PATH before §5 rewrites it, so §5 can tell
# whether it inherited one and only then explain that it prepended the tbb pin.
_INHERITED_LD_LIBRARY_PATH="${LD_LIBRARY_PATH:-}"

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

# -- 1b. Preflight: refuse to run into a known-bad launch ----------------------
# Deliberately before every expensive step (npm install, the frontend build, and
# a cargo --release build that can take minutes) — this is a failure the user
# would otherwise only see after that wait.
#
# Set REIFY_GUI_SKIP_PREFLIGHT=1 to bypass (break-glass).
if [ "${REIFY_GUI_SKIP_PREFLIGHT:-}" != "1" ]; then
    # Display. reify-gui is a GTK/WebKit app; with no X11 or Wayland display it
    # cannot open a window at all, and the failure otherwise surfaces only
    # AFTER the release build as an opaque GTK abort. We deliberately do NOT
    # probe EGL — see the matching note in scripts/run-gui-dev.sh §1b.
    if [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
        echo "Error: no display: DISPLAY and WAYLAND_DISPLAY are both unset — reify-gui needs an X11/Wayland display; export DISPLAY=:0 (or set REIFY_GUI_SKIP_PREFLIGHT=1 to bypass)" >&2
        exit 1
    fi
fi

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

# Pin oneTBB ahead of everything else — INCLUDING whatever LD_LIBRARY_PATH we
# were handed. Deliberately OUTSIDE the snap conditional above, which never
# fires on a PPA host. #5192 already puts $TBB_PIN_DIR first in each binary's
# DT_RUNPATH, but the loader searches LD_LIBRARY_PATH BEFORE DT_RUNPATH, so an
# inherited /usr/lib path would otherwise bind system libtbb 12.11 over the
# deps 12.18 and die on a missing symbol. The caller's entries are preserved,
# never scrubbed or reordered. (#7254)
TBB_PIN_DIR="/opt/reify-deps/tbb-pin"
if [ -d "$TBB_PIN_DIR" ]; then
    export LD_LIBRARY_PATH="$TBB_PIN_DIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    if [ -n "$_INHERITED_LD_LIBRARY_PATH" ]; then
        echo "==> Note: inherited LD_LIBRARY_PATH preserved, but $TBB_PIN_DIR prepended ahead of it (the loader searches LD_LIBRARY_PATH before DT_RUNPATH, so an inherited /usr/lib path would otherwise bind system libtbb 12.11 over the deps 12.18 — see #5192/#7254)"
    fi
fi

# -- 6. Launch ----------------------------------------------------------------
echo "==> Launching target/release/reify-gui $FILE"
exec target/release/reify-gui "$FILE"
