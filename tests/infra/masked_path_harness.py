"""
masked_path_harness.py — a PATH that is this host minus named executables, for
unittests that must prove a script's behaviour when a tool is ABSENT.

Not a test module itself (no `test_` prefix): run_all.sh never discovers it,
and the members import it via the sys.path idiom.

Masking is by omission, never by a failing stub: a stub that exits non-zero
drives a script's retry or failure path, not its absent-tool path.
"""

import os
from pathlib import Path


def masked_path_dir(shim: Path, *masked: str) -> Path:
    """Fill `shim` with symlinks to every executable file on PATH except the
    `masked` names, the first occurrence of a name winning as PATH lookup
    would, so PATH=<shim> resolves everything this host does except those
    names. Returns `shim`."""
    shim.mkdir(parents=True, exist_ok=True)
    for entry in os.environ.get("PATH", "").split(os.pathsep):
        if not entry or not os.path.isdir(entry):
            continue
        for name in sorted(os.listdir(entry)):
            source = os.path.join(entry, name)
            link = shim / name
            if name in masked or os.path.lexists(link):
                continue
            if os.path.isfile(source) and os.access(source, os.X_OK):
                link.symlink_to(source)
    return shim
