"""
canon_run_fixture.py — shared builders for the scripts/canon_run.py suites
(test_canon_run_units.py, test_canon_run_host.py): scratch trees, the
host.toml writer, minimal environments and the CLI runner.

Not a test module itself (no `test_` prefix): run_all.sh never discovers it,
and the suites import it via the sys.path idiom.
"""

import importlib.util
import os
import shutil
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
CANON_RUN = REPO_ROOT / "scripts" / "canon_run.py"
TRAMPOLINE = REPO_ROOT / "scripts" / "canon-bin" / "cargo"
PYTHON = "/usr/bin/python3"


def load_canon_run():
    spec = importlib.util.spec_from_file_location("canon_run", CANON_RUN)
    module = importlib.util.module_from_spec(spec)
    # Registered before exec_module so @dataclass resolves cls.__module__.
    sys.modules["canon_run"] = module
    spec.loader.exec_module(module)
    return module


def make_tree(parent, name):
    """A tree canon_run.py recognises: Cargo.lock beside scripts/canon_run.py."""
    root = Path(parent, name)
    canon_bin = root / "scripts" / "canon-bin"
    canon_bin.mkdir(parents=True)
    (root / "Cargo.lock").write_text("version = 4\n")
    shutil.copy2(CANON_RUN, root / "scripts" / "canon_run.py")
    if TRAMPOLINE.exists():
        shutil.copy2(TRAMPOLINE, canon_bin / "cargo")
    return os.path.realpath(root)


def write_host_config(path, *, enabled=None, policy=None, extra_lines=()):
    """host.toml in dark-factory reify_canon_host_config.render's shape."""
    lines = []
    if enabled is not None:
        lines.append(f"enabled = {'true' if enabled else 'false'}")
    if policy is not None:
        lines.append(f'policy = "{policy}"')
    lines.extend(extra_lines)
    Path(path).write_text("".join(line + "\n" for line in lines))


def scratch_env(tmp, **overrides):
    """An explicit minimal environment with no LANG/LC_* keys; CANON is never created."""
    home = Path(tmp, "home")
    home.mkdir(exist_ok=True)
    env = {
        "PATH": "/usr/bin:/bin",
        "HOME": str(home),
        "REIFY_CANON_ROOT": str(Path(tmp, "canon-unprovisioned")),
        "REIFY_CANON_HOST_CONFIG": str(Path(tmp, "host.toml")),
    }
    env.update(overrides)
    return env


def run_canon(tree, cwd, *args, env):
    """Run <tree>/scripts/canon_run.py <args> from cwd; no timeout (no wall-clock bound)."""
    script = Path(tree, "scripts", "canon_run.py")
    return subprocess.run(
        [PYTHON, "-I", str(script), *args],
        cwd=cwd,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
