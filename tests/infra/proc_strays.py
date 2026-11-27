"""
proc_strays.py — the processes a fixture left behind, found by a needle in
their /proc cmdline, and the SIGKILL that clears them at cleanup.

A suite that runs the real tests/infra/run_all.sh on a fixture makes every
fixture process carry a needle in its argv (the fixture root's path, or a
unique token passed as `exec -a`), so whatever is still alive at cleanup can be
found and killed without touching anything else on the host.

Not a test module itself (no `test_` prefix): run_all.sh never discovers it,
and the suites import it via the sys.path idiom.
"""

import contextlib
import os
import signal
from pathlib import Path


def strays(*needles):
    """(pid, cmdline) of every live process but this one whose cmdline holds a needle."""
    own = os.getpid()
    wanted = [n.encode() for n in needles]
    found = []
    for entry in os.listdir("/proc"):
        if not entry.isdigit() or int(entry) == own:
            continue
        try:
            cmdline = Path("/proc", entry, "cmdline").read_bytes()
        except OSError:
            continue
        if any(n in cmdline for n in wanted):
            found.append((int(entry), cmdline.replace(b"\0", b" ").decode(errors="replace").strip()))
    return found


def kill_strays(*needles):
    """SIGKILL every process strays(*needles) finds; one already gone is skipped."""
    for pid, _ in strays(*needles):
        with contextlib.suppress(ProcessLookupError, PermissionError):
            os.kill(pid, signal.SIGKILL)
