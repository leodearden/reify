"""
refresh_warm_base_harness.py — the hermetic staging shared by every unittest
that runs a COPY of scripts/refresh-warm-base.sh (task 10152):
test_refresh_warm_base_built_tree.py, test_prune_unconsulted_target.py
RefreshDelegation and test_evict_dirty_target_units.py RefreshStep.

Not a test module itself (no `test_` prefix): run_all.sh never discovers it,
and the members import it via the sys.path idiom.

SIBLINGS: stage_refresh() copies the refresh into a fresh scripts/ dir beside
every sibling script it calls (REFRESH_SIBLINGS), each a no-op stub unless the
caller asks for the real one or for none. A new sibling call in the refresh is
added to REFRESH_SIBLINGS once, and every member stages it.
PATH: install_cp_stub() writes a `cp` that drops --reflink=always, so the
refresh's reflink copy works on any filesystem; run_refresh() puts it first.
LANES: init_lane() and commit() make a git repository on main under an env
with no GIT_* variables and a fixed test identity.
"""

import os
import shutil
import subprocess
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[2] / "scripts"
REFRESH = SCRIPTS / "refresh-warm-base.sh"
REFRESH_SIBLINGS = ("prune-unconsulted-target.sh", "evict-dirty-target-units.py")
NO_OP_SIBLING = "#!/bin/sh\nexit 0\n"


def env_without(*prefixes):
    return {k: v for k, v in os.environ.items() if not k.startswith(prefixes)}


def write_executable(path, body):
    path.write_text(body)
    path.chmod(0o755)


def stage_refresh(scripts_dir, real=(), absent=()):
    """Create `scripts_dir` holding a copy of the refresh and a no-op stub of
    each sibling, except the siblings named in `real` (copied from scripts/)
    and in `absent` (left out). Returns the copy's path."""
    unknown = sorted((set(real) | set(absent)) - set(REFRESH_SIBLINGS))
    if unknown or set(real) & set(absent):
        raise ValueError(f"real={real!r} absent={absent!r} must name distinct "
                         f"members of {REFRESH_SIBLINGS}")
    scripts_dir.mkdir()
    refresh = scripts_dir / REFRESH.name
    shutil.copy2(REFRESH, refresh)
    for sibling in REFRESH_SIBLINGS:
        if sibling in real:
            shutil.copy2(SCRIPTS / sibling, scripts_dir / sibling)
        elif sibling not in absent:
            write_executable(scripts_dir / sibling, NO_OP_SIBLING)
    return refresh


def install_cp_stub(stub_dir):
    write_executable(stub_dir / "cp", f"""#!/usr/bin/env bash
args=()
for a in "$@"; do
    [ "$a" = "--reflink=always" ] && continue
    args+=("$a")
done
exec {shutil.which("cp")} "${{args[@]}}"
""")


def run_refresh(refresh, stub_dir, advancing, base, landed):
    env = env_without("GIT_")
    env["PATH"] = f"{stub_dir}{os.pathsep}{env.get('PATH', '')}"
    return subprocess.run(["bash", str(refresh), str(advancing), str(base),
                           "--landed-commit", landed],
                          capture_output=True, text=True, env=env, check=False)


def git(lane, *args):
    return subprocess.run(["git", "-C", str(lane), *args], env=env_without("GIT_"),
                          check=True, capture_output=True, text=True).stdout.strip()


def init_lane(lane):
    lane.mkdir(parents=True, exist_ok=True)
    git(lane, "init", "-q", "-b", "main")


def commit(lane, *args):
    """`git commit` in `lane` as the test identity, unsigned and hook-free;
    returns the new HEAD."""
    git(lane, "-c", "user.email=warm-lane-test@localhost", "-c", "user.name=Warm Lane Test",
        "-c", "commit.gpgsign=false", "commit", "-q", "--no-verify", *args)
    return git(lane, "rev-parse", "HEAD")
