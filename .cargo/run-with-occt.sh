#!/bin/bash
# Wrapper script to set LD_LIBRARY_PATH for OCCT shared libraries.
export LD_LIBRARY_PATH="/snap/freecad/current/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
exec "$@"
