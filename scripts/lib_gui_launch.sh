#!/usr/bin/env bash
# Shared launch-environment helpers for the reify-gui launchers (#7254).
# Designed to be sourced, not executed directly.
#
# Usage:  source "$(dirname "${BASH_SOURCE[0]}")/lib_gui_launch.sh"
#   or:   source "$REPO_ROOT/scripts/lib_gui_launch.sh"
#
# Sourced by scripts/run-gui.sh (release) and scripts/run-gui-dev.sh
# (dev). The two launchers differ in build profile and in whether they run a
# vite dev server, but they must agree EXACTLY on the environment they hand
# the binary — and on the preflight that decides a launch is hopeless before
# paying for a multi-minute build. Before this lib the inherited-
# LD_LIBRARY_PATH snapshot, the display preflight and the oneTBB pin +
# WebKit renderer default were duplicated near-verbatim across both files,
# down to byte-identical user-facing strings. That is the shape that lets two
# copies drift silently: gui/test/visual/lib_e2e_smoke.sh's own header records
# exactly that outcome for a six-way copy of the GUI-launch lifecycle (one
# copy quietly lost both the deps LD_LIBRARY_PATH block and the WebKit
# disable).
#
# Sets NO shell options: sourcing must not change the caller's `set -e` /
# `set -u` / pipefail state. Every function below is safe under
# `set -euo pipefail`, and none of them call `exit` — they return non-zero and
# leave the exit decision to the launcher.
#
# Also sourced by gui/test/visual/lib_e2e_smoke.sh (#7273), which calls
# gui_launch_env_pin in its §2b.

# Source guard — prevent double-sourcing (mirrors scripts/lib_portable.sh).
if [ "${_REIFY_LIB_GUI_LAUNCH_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_LIB_GUI_LAUNCH_SH_SOURCED=1

# The workspace-bin loader-path class lives in the lower lib next to this one.
# A failed source returns non-zero so a `set -e` launcher aborts rather than
# launching without the loader-path policy.
case "${BASH_SOURCE[0]}" in
    */*) _gui_launch_lib_dir="${BASH_SOURCE[0]%/*}" ;;
    *) _gui_launch_lib_dir=. ;;
esac
# shellcheck source=scripts/lib_native_ld_path.sh
source "$_gui_launch_lib_dir/lib_native_ld_path.sh" || {
    echo "lib_gui_launch.sh: cannot source $_gui_launch_lib_dir/lib_native_ld_path.sh" >&2
    return 1
}
unset _gui_launch_lib_dir

# The tbb-only RUNPATH pin dir materialised by scripts/build-manifold-deps.sh
# (a libtbb.so.12 symlink into the deps lib). #5192's "mechanism A''".
GUI_LAUNCH_TBB_PIN_DIR="/opt/reify-deps/tbb-pin"

# Snapshot the caller's LD_LIBRARY_PATH at SOURCE time — i.e. before any
# launcher stanza rewrites it — so gui_launch_env_pin() can tell whether it
# actually inherited one, and only then explain that it prepended the pin.
#
# Sourcing this lib EARLY is therefore part of the contract: a launcher that
# sourced it after its own LD_LIBRARY_PATH edits would report "nothing
# inherited" for a value it had set itself, and the notice would go silent
# exactly when it matters.
_GUI_LAUNCH_INHERITED_LD_LIBRARY_PATH="${LD_LIBRARY_PATH:-}"

# gui_launch_preflight_display
#
# Refuse a launch that cannot possibly open a window. reify-gui is a
# GTK/WebKit app; with neither an X11 nor a Wayland display it fails only
# AFTER the cargo build, as an opaque GTK abort — so the check belongs before
# every expensive step.
#
# We deliberately do NOT probe EGL here. `eglinfo` (mesa-utils) is not in
# scripts/setup-dev.sh's package set, so it cannot be hard-required, and
# gui_launch_env_pin()'s WEBKIT_DISABLE_DMABUF_RENDERER=1 default already
# neutralises the NVIDIA+Mesa GBM failure an EGL probe would flag — the probe
# would be both undependable and a false alarm.
#
# Prints ONE error line to stderr and returns 1; never exits, so the caller
# owns the exit code. Honouring REIFY_GUI_SKIP_PREFLIGHT is likewise the
# caller's job — each launcher wraps its whole preflight block in that guard.
gui_launch_preflight_display() {
    if [ -n "${DISPLAY:-}" ] || [ -n "${WAYLAND_DISPLAY:-}" ]; then
        return 0
    fi
    echo "Error: no display: DISPLAY and WAYLAND_DISPLAY are both unset — reify-gui needs an X11/Wayland display; export DISPLAY=:0 (or set REIFY_GUI_SKIP_PREFLIGHT=1 to bypass)" >&2
    return 1
}

# _gui_launch_env_already_pinned
#
# True when LD_LIBRARY_PATH already leads with exactly what gui_launch_env_pin
# puts there: the tbb pin dir (when it exists), then the workspace-bin dirs.
# Every entry is compared as a whole colon-delimited component, so a sibling
# such as /opt/reify-deps/tbb-pin-old does not count as the pin.
_gui_launch_env_already_pinned() {
    local rest="${LD_LIBRARY_PATH:-}"
    if [ -d "$GUI_LAUNCH_TBB_PIN_DIR" ]; then
        case "$rest:" in
            "$GUI_LAUNCH_TBB_PIN_DIR:"*) rest="${rest#"$GUI_LAUNCH_TBB_PIN_DIR"}" ;;
            *) return 1 ;;
        esac
        rest="${rest#:}"
    fi
    native_ld_path_leads workspace-bin "$rest"
}

# gui_launch_env_pin
#
# Set the two environment defaults every direct reify-gui invocation needs.
#
# ORDERING CONTRACT: this is the only LD_LIBRARY_PATH mutation a launcher
# makes: the workspace-bin dirs (scripts/lib_native_ld_path.sh), then the tbb
# pin LAST so it leads. Anything that prepended after it would land ahead of
# the pin and defeat it.
#
# IDEMPOTENT: when LD_LIBRARY_PATH already leads with the pin dir and then the
# workspace-bin dirs, neither prepend runs and no notice is printed. That is the
# normal state of a launcher run as a CHILD of one that already pinned
# (lib_e2e_smoke.sh pins, then runs scripts/run-gui-dev.sh, which sources this
# lib afresh); a second pass there would only duplicate the entries and repeat
# the notice. A caller that leads with the pin ALONE still gets both prepends.
gui_launch_env_pin() {
    # Disable WebKit's GBM/DMABuf renderer by default. On systems where the
    # NVIDIA driver exposes DRI fds but the Mesa EGL GBM backend cannot create
    # a screen, WebKitGTK aborts with
    # "Could not create GBM EGL display: EGL_NOT_INITIALIZED". =1 forces the
    # GLX/xlib fallback path.
    # The `:-1` form is deliberate: export WEBKIT_DISABLE_DMABUF_RENDERER=0 to
    # restore the DMABUF path on a host where it works.
    export WEBKIT_DISABLE_DMABUF_RENDERER="${WEBKIT_DISABLE_DMABUF_RENDERER:-1}"

    if _gui_launch_env_already_pinned; then
        return 0
    fi

    native_ld_path_prepend workspace-bin || return

    # Pin oneTBB ahead of everything else — INCLUDING whatever LD_LIBRARY_PATH
    # we were handed.
    #
    # #5192 already puts the pin dir first in each binary's DT_RUNPATH, but the
    # loader searches LD_LIBRARY_PATH BEFORE DT_RUNPATH. So a caller exporting
    # LD_LIBRARY_PATH=/usr/lib/x86_64-linux-gnu:... silently defeats that pin:
    # the binary binds the system libtbb 12.11 instead of the deps 12.18 and
    # dies on a missing symbol. Leading with the pin dir restores #5192's
    # guarantee while PRESERVING the caller's entries — we never scrub or
    # reorder them. (#7254)
    if [ -d "$GUI_LAUNCH_TBB_PIN_DIR" ]; then
        export LD_LIBRARY_PATH="$GUI_LAUNCH_TBB_PIN_DIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
        if [ -n "$_GUI_LAUNCH_INHERITED_LD_LIBRARY_PATH" ]; then
            echo "==> Note: inherited LD_LIBRARY_PATH preserved, but $GUI_LAUNCH_TBB_PIN_DIR prepended ahead of it (the loader searches LD_LIBRARY_PATH before DT_RUNPATH, so an inherited /usr/lib path would otherwise bind system libtbb 12.11 over the deps 12.18 — see #5192/#7254)"
        fi
    fi
}
