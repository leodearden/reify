#!/bin/bash
# Wrapper script invoked by cargo (configured in .cargo/config.toml).
#
# Sets LD_LIBRARY_PATH for OCCT shared libraries when they live outside
# the default loader path. The FreeCAD snap installs into a per-snap
# directory the dynamic linker doesn't know about, so we prepend it
# only when that directory actually exists. The FreeCAD PPA install
# (what scripts/setup-dev.sh uses) puts libs in /usr/lib, where the
# loader finds them with no help. /opt/reify-deps is an alternative
# install location used when OCCT 7.9 is installed outside both the
# snap and the FreeCAD PPA (e.g. via conda/mamba or a manual build).
SNAP_OCCT_LIB="/snap/freecad/current/usr/lib"
if [ -d "$SNAP_OCCT_LIB" ]; then
    export LD_LIBRARY_PATH="$SNAP_OCCT_LIB${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi

# /opt/reify-deps/lib ships OCCT 7.9 on some hosts. reify-kernel-gmsh's build.rs
# adds it to the linker search path, so the linker may resolve -lTKernel to the
# 7.9 SONAME there even when /usr/lib has 7.8. No rpath is baked in, so the
# loader needs LD_LIBRARY_PATH to find libTKernel.so.7.9 at runtime. The
# libTKernel.so* presence check avoids polluting LD_LIBRARY_PATH when the
# directory exists but does not contain the relevant libraries.
REIFY_DEPS_LIB="/opt/reify-deps/lib"
if [ -d "$REIFY_DEPS_LIB" ] && ls "$REIFY_DEPS_LIB"/libTKernel.so* >/dev/null 2>&1; then
    export LD_LIBRARY_PATH="$REIFY_DEPS_LIB${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi
exec "$@"
