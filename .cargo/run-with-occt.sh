#!/bin/bash
# Wrapper script invoked by cargo (configured in .cargo/config.toml).
#
# Sets LD_LIBRARY_PATH for OCCT shared libraries when they live outside
# the default loader path. The FreeCAD snap installs into a per-snap
# directory the dynamic linker doesn't know about, so we prepend it
# only when that directory actually exists. The FreeCAD PPA install
# (what scripts/setup-dev.sh uses) puts libs in /usr/lib, where the
# loader finds them with no help.
SNAP_OCCT_LIB="/snap/freecad/current/usr/lib"
if [ -d "$SNAP_OCCT_LIB" ]; then
    export LD_LIBRARY_PATH="$SNAP_OCCT_LIB${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi
exec "$@"
