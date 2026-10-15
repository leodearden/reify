#!/usr/bin/env bash
# The one place that decides which native-dep dirs a process puts on
# LD_LIBRARY_PATH, by consumer class. Designed to be sourced, not executed.
#
#   cargo-target   any binary cargo/nextest runs: the snap OCCT lib dir if
#                  present, then the deps dir if it holds libTKernel.so*.
#   workspace-bin  reify / reify-gui: the snap dir only, never the deps conda
#                  prefix.
#
# Pure bash and table-free, because .cargo/run-with-occt.sh sources it for
# every test process. Its two default dirs are RUNTIME loader facts, distinct
# from the build-time discovery candidates in reify_build_utils. Sets NO shell
# options. Knobs: REIFY_NATIVE_LD_SNAP_DIR, REIFY_NATIVE_LD_DEPS_DIR (read at
# call time).
#
# Why: docs/notes/native-dep-loader-path.md

# Source guard — prevent double-sourcing (mirrors scripts/lib_portable.sh).
if [ "${_REIFY_LIB_NATIVE_LD_PATH_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_LIB_NATIVE_LD_PATH_SH_SOURCED=1

# _native_ld_path_collect <out-array-name> <class>
#
# Fills the named array with the class's dirs in LOADER SEARCH ORDER (first
# entry searched first). Returns 2 on an unknown class.
_native_ld_path_collect() {
    local -n _nlp_out="$1"
    local class="$2"
    _nlp_out=()
    case "$class" in
        cargo-target | workspace-bin) ;;
        *)
            echo "native_ld_path: unknown consumer class '$class' (expected cargo-target or workspace-bin)" >&2
            return 2
            ;;
    esac
    local snap="${REIFY_NATIVE_LD_SNAP_DIR:-/snap/freecad/current/usr/lib}"
    local deps="${REIFY_NATIVE_LD_DEPS_DIR:-/opt/reify-deps/lib}"
    if [ "$class" = cargo-target ]; then
        local -a tk=("$deps"/libTKernel.so*)
        if [ -e "${tk[0]-}" ]; then
            _nlp_out+=("$deps")
        fi
    fi
    if [ -d "$snap" ]; then
        _nlp_out+=("$snap")
    fi
}

# native_ld_path_dirs <class>
#
# Prints the class's dirs one per line, in loader search order.
native_ld_path_dirs() {
    local -a dirs
    _native_ld_path_collect dirs "$1" || return
    if [ "${#dirs[@]}" -gt 0 ]; then
        printf '%s\n' "${dirs[@]}"
    fi
}

# native_ld_path_prepend <class>
#
# Exports LD_LIBRARY_PATH with the class's dirs ahead of its current value.
# When the class yields no dir, LD_LIBRARY_PATH is left untouched (unset stays
# unset).
native_ld_path_prepend() {
    local -a dirs
    _native_ld_path_collect dirs "$1" || return
    if [ "${#dirs[@]}" -eq 0 ]; then
        return 0
    fi
    local IFS=:
    export LD_LIBRARY_PATH="${dirs[*]}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
}
