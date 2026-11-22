"""
canon_run_fixture.py — shared builders for the scripts/canon_run.py suites
(test_canon_run_units.py, test_canon_run_host.py): scratch trees, the
host.toml writer, minimal environments, the CLI runner and a recording fake
cargo.

Not a test module itself (no `test_` prefix): run_all.sh never discovers it,
and the suites import it via the sys.path idiom.
"""

import importlib.util
import json
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
    return run([PYTHON, "-I", str(script), *args], cwd=cwd, env=env)


def run(argv, *, cwd, env):
    return subprocess.run(argv, cwd=cwd, env=env, capture_output=True, text=True, check=False)


# Records each call beside itself. A shebang script never sees the argv[0] it
# was exec'd with (the kernel substitutes the script path), so a call's argv[0]
# is the path that ran. Exits with FAKE_CARGO_EXIT, default 0.
FAKE_CARGO = r'''#!/usr/bin/python3 -I
import json
import os
import sys

environ = {}
with open("/proc/self/environ", "rb") as environ_file:
    for entry in environ_file.read().split(b"\0"):
        name, separator, value = entry.partition(b"=")
        if separator:
            environ[os.fsdecode(name)] = os.fsdecode(value)
call = {"argv": sys.argv, "cwd": os.getcwd(), "env": environ}
with open(os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])), "calls.jsonl"), "a") as log:
    log.write(json.dumps(call) + "\n")
sys.exit(int(environ.get("FAKE_CARGO_EXIT", "0")))
'''


def fake_cargo_dir(parent, name="fake-bin"):
    """A directory holding an unmarked recording `cargo`; returns its real path."""
    directory = Path(parent, name)
    directory.mkdir()
    cargo = directory / "cargo"
    cargo.write_text(FAKE_CARGO)
    cargo.chmod(0o755)
    return os.path.realpath(directory)


def read_calls(directory):
    log = Path(directory, "calls.jsonl")
    if not log.exists():
        return []
    return [json.loads(line) for line in log.read_text().splitlines()]
