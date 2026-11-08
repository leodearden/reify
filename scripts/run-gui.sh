#!/usr/bin/env bash
# Single-command release-mode launcher for reify-gui (task 2228).
#
# Usage: scripts/run-gui.sh [file.ri]
#
# With no file, reify-gui opens on the launcher Start view (Open/New/Recent).
#
# Performs every build step needed to launch reify-gui from a checkout that
# scripts/setup-dev.sh has provisioned:
#   1. Validate args, then PREFLIGHT: refuse a launch with no display BEFORE
#      any expensive step. Bypass with REIFY_GUI_SKIP_PREFLIGHT=1.
#   2. Build the sidecar (idempotent; ~20ms tsup bundle). Without gui/sidecar's
#      npm deps, build-sidecar.sh exits 1 and prints the remedy, so this script
#      stops before any npm or cargo step.
#   3. Install gui/ npm deps + build the frontend (produces gui/dist).
#   4. Build the reify-gui cargo binary in release mode with features `gui` +
#      `embedded-frontend` (see gui/src-tauri/Cargo.toml), so the window serves
#      step 3's gui/dist, never devUrl.
#   5. Export LD_LIBRARY_PATH (OCCT's bundled libs + the tbb pin) and the
#      WebKit renderer default.
#   6. exec target/release/reify-gui [file.ri].
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
if [ "$#" -gt 1 ]; then
    echo "Usage: scripts/run-gui.sh [file.ri]" >&2
    echo "" >&2
    echo "  file.ri  path to a .ri source file; omit it to open the launcher Start view" >&2
    echo "" >&2
    echo "Builds and launches reify-gui in release mode (run scripts/setup-dev.sh first)." >&2
    echo "For dev mode (vite, devtools, MCP debug listener), use run-gui-dev.sh." >&2
    exit 1
fi

# FILE is the .ri reify-gui opens, or empty for the Start view.
FILE="${1:-}"

if [ -n "$FILE" ]; then
    case "$FILE" in
        *.ri) ;;
        *)
            echo "Error: file must have .ri extension: $FILE" >&2
            exit 1
            ;;
    esac

    [ -f "$FILE" ] || { echo "Error: file not found: $FILE" >&2; exit 1; }
fi

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
# §3's gui/dist is a compile-time input: `embedded-frontend` embeds it.
echo "==> Building reify-gui (release)..."
cargo build -p reify-gui --bin reify-gui --features gui,embedded-frontend --release

# -- 5. Launch environment -----------------------------------------------------
# The whole launch loader path and the WebKit renderer default, shared with
# scripts/run-gui-dev.sh: scripts/lib_gui_launch.sh.
gui_launch_env_pin

# -- 6. Launch ----------------------------------------------------------------
echo "==> Launching target/release/reify-gui ${FILE:-(no file: Start view)}"
exec target/release/reify-gui ${FILE:+"$FILE"}
