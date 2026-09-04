#!/usr/bin/env bash
# Single-command release-mode launcher for reify-gui (task 2228).
#
# Usage: scripts/run-gui.sh <file.ri>
#
# Performs every build step needed to launch reify-gui from a clean checkout:
#   1. Validate args, then PREFLIGHT: refuse a launch with no display BEFORE
#      any expensive step. Bypass with REIFY_GUI_SKIP_PREFLIGHT=1.
#   2. Build the sidecar (idempotent; ~20ms tsup bundle).
#   3. Install gui/ npm deps + build the frontend (produces gui/dist).
#   4. Build the reify-gui cargo binary in release mode (with feature `gui`).
#   5. Export LD_LIBRARY_PATH (OCCT's bundled libs + the tbb pin) and the
#      WebKit renderer default.
#   6. exec target/release/reify-gui <file.ri>.
#
# Environment contract (full version in scripts/run-gui-dev.sh's header; the
# mechanism for all three is scripts/lib_gui_launch.sh, sourced by BOTH
# launchers so they cannot drift apart on it):
#   LD_LIBRARY_PATH   an inherited value is PRESERVED, but
#                     /opt/reify-deps/tbb-pin is prepended ahead of it — the
#                     loader searches LD_LIBRARY_PATH before DT_RUNPATH, so a
#                     caller path naming /usr/lib/x86_64-linux-gnu would
#                     otherwise bind system libtbb 12.11 over the deps 12.18
#                     and defeat #5192's pin.
#   WEBKIT_DISABLE_DMABUF_RENDERER   defaults to 1; export 0 to restore the
#                     DMABUF path on a host where it works.
#   REIFY_GUI_SKIP_PREFLIGHT         set to 1 to skip the display preflight.
#
# For dev-mode (vite dev server, configurable debug MCP port, devtools)
# use scripts/run-gui-dev.sh instead.

set -euo pipefail

# Resolve this script's directory up-front so the shared launch helpers can be
# sourced BEFORE anything touches LD_LIBRARY_PATH — lib_gui_launch.sh snapshots
# the caller's value at source time, and a late source would report "nothing
# inherited" for a value this script had set itself. REPO_ROOT is derived from
# SCRIPT_DIR below, after argument validation.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/lib_gui_launch.sh
source "$SCRIPT_DIR/lib_gui_launch.sh"

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
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

# -- 1b. Preflight: refuse to run into a known-bad launch ----------------------
# Deliberately before every expensive step (npm install, the frontend build, and
# a cargo --release build that can take minutes) — this is a failure the user
# would otherwise only see after that wait.
#
# Set REIFY_GUI_SKIP_PREFLIGHT=1 to bypass (break-glass).
if [ "${REIFY_GUI_SKIP_PREFLIGHT:-}" != "1" ]; then
    # Display — shared with scripts/run-gui-dev.sh, so the check and its
    # message live in scripts/lib_gui_launch.sh (including the note on why we
    # do NOT probe EGL). It prints its own error and returns 1; the exit is
    # ours.
    gui_launch_preflight_display || exit 1
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

# The oneTBB LD_LIBRARY_PATH pin + the WebKit renderer default, shared with
# scripts/run-gui-dev.sh — see scripts/lib_gui_launch.sh for both rationales.
#
# MUST come after the snap prepend above: gui_launch_env_pin PREPENDS the pin
# dir, so anything that edits LD_LIBRARY_PATH afterwards would land ahead of it
# and defeat #5192's pin. It is deliberately not nested in the snap conditional
# either — the snap dir does not exist on a PPA host.
gui_launch_env_pin

# -- 6. Launch ----------------------------------------------------------------
echo "==> Launching target/release/reify-gui $FILE"
exec target/release/reify-gui "$FILE"
