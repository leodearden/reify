"""
canon_run_fixture.py — shared builders for the scripts/canon_run.py suites
(test_canon_run_units.py, test_canon_run_host.py): scratch trees, the
host.toml writer, minimal environments, the CLI runner and its BOUND-route
variant, a recording fake cargo, and for the host suite a buildable crate,
real-cargo environments and the namespace/landlock probes.

Not a test module itself (no `test_` prefix): run_all.sh never discovers it,
and the suites import it via the sys.path idiom.
"""

import functools
import importlib.util
import json
import os
import pwd
import shutil
import signal
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
CANON_RUN = REPO_ROOT / "scripts" / "canon_run.py"
TRAMPOLINE = REPO_ROOT / "scripts" / "canon-bin" / "cargo"
GUARD = REPO_ROOT / "scripts" / "canon-bin" / "guard" / "sccache"
CANON_RUN_SIBLINGS = ("canon_cache.py", "lib_event_log.sh")
PYTHON = "/usr/bin/python3"
BWRAP = "/usr/bin/bwrap"
LANDLOCK_EXEC = REPO_ROOT / "gui" / "src-tauri" / "sandbox" / "landlock_exec.py"
FIXTURE_DATA = "fixture data\n"


@functools.cache
def load_canon_run():
    """Loaded once, so every caller sees the same classes (canon_run.Refusal and the rest)."""
    spec = importlib.util.spec_from_file_location("canon_run", CANON_RUN)
    module = importlib.util.module_from_spec(spec)
    # Registered before exec_module so @dataclass resolves cls.__module__.
    sys.modules["canon_run"] = module
    spec.loader.exec_module(module)
    return module


def load_canon_cache():
    """The canon_cache module canon_run imported, so enum identity holds across both."""
    load_canon_run()
    return sys.modules["canon_cache"]


def make_tree(parent, name):
    """A tree canon_run.py recognises: Cargo.lock beside scripts/canon_run.py and its siblings."""
    root = Path(parent, name)
    canon_bin = root / "scripts" / "canon-bin"
    canon_bin.mkdir(parents=True)
    (root / "Cargo.lock").write_text("version = 4\n")
    shutil.copy2(CANON_RUN, root / "scripts" / "canon_run.py")
    for sibling in CANON_RUN_SIBLINGS:
        shutil.copy2(REPO_ROOT / "scripts" / sibling, root / "scripts" / sibling)
    if TRAMPOLINE.exists():
        shutil.copy2(TRAMPOLINE, canon_bin / "cargo")
    if GUARD.exists():
        (canon_bin / "guard").mkdir()
        shutil.copy2(GUARD, canon_bin / "guard" / "sccache")
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


# Runs <scripts dir>/canon_run.py's main with one extra mountinfo line naming
# CANON as a mount point. With REIFY_CANON_ROOT set to the tree's own path, C3's
# bound check then holds (same directory, listed as a mount), so the BOUND route
# runs where no mount namespace can be entered (argv: scripts dir, canon_run args).
BOUND_RUNNER = r"""
import sys

sys.path.insert(0, sys.argv[1])
import canon_run

canon = canon_run.canon_root(canon_run.caller_environ())
escaped = canon.replace("\\", "\\134").replace(" ", "\\040")
bound = f"36 35 0:51 / {escaped} rw,relatime shared:1 - ext4 /dev/vda1 rw\n"
host_mountinfo = canon_run.read_mountinfo
canon_run.read_mountinfo = lambda: host_mountinfo() + bound
sys.exit(canon_run.main(sys.argv[2:]))
"""


def bound_argv(tree, *args, runner_dir):
    """The BOUND runner's argv for canon_run args, writing the runner into runner_dir if absent."""
    runner = Path(runner_dir, "bound_runner.py")
    if not runner.exists():
        runner.write_text(BOUND_RUNNER)
    return [PYTHON, "-I", str(runner), str(Path(tree, "scripts")), *args]


def run_bound(tree, cwd, *args, env, runner_dir):
    """run_canon on the BOUND route: env's REIFY_CANON_ROOT must be the tree's own path."""
    return run(bound_argv(tree, *args, runner_dir=runner_dir), cwd=cwd, env=env)


# The NAMESPACE route's outer process, canon_run.run_in_namespace, around a fake
# bwrap instead of the real one, so its signal forwarding runs with no mount
# namespace. argv: <scripts dir> <directory> <options JSON>. The runner starts
# from default dispositions, then ignores each signal options' `ignore` names.
# With options' signal_during_start, the argv callback sends this process
# SIGTERM, so the signal arrives while run_in_namespace is starting bwrap. The
# command writes its SigIgn line to `sigign`, touches `started`, then sleeps
# options' command_secs.
#
# The fake bwrap behaves as bwrap 0.9.0 does: it keeps the dispositions it was
# started with, starts what follows its `--` (run_in_namespace's env layer
# around the command) after a setup window spent under those dispositions, and
# writes the child's pid to the info fd BEFORE that window ends. In
# <directory>, it first lists its open descriptors in `bwrap_fds.json`, touches
# `spawned`, waits for `go`, and exits with the child's status or 128+N.
NAMESPACE_PARENT_RUNNER = r"""
import json
import os
import signal
import sys

sys.path.insert(0, sys.argv[1])
import canon_cache
import canon_run

directory, options = sys.argv[2], json.loads(sys.argv[3])
fake_bwrap = os.path.join(directory, "fake_bwrap.py")
command = [
    "/bin/sh", "-c", 'grep SigIgn /proc/self/status > "$0/sigign"; touch "$0/started"; exec sleep "$1"',
    directory, options["command_secs"],
]


def bwrap_for(info_fd):
    if options["signal_during_start"]:
        os.kill(os.getpid(), signal.SIGTERM)
    return [sys.executable, "-I", fake_bwrap, str(info_fd), directory]


for number in canon_cache.SUPERVISED_SIGNALS:
    signal.signal(number, signal.default_int_handler if number == signal.SIGINT else signal.SIG_DFL)
for name in options["ignore"]:
    signal.signal(signal.Signals[name], signal.SIG_IGN)
sys.exit(canon_run.run_in_namespace(bwrap_for, command, {"PATH": "/usr/bin:/bin"}, canon_cache.SignalForwarder()))
"""
FAKE_BWRAP = r"""
import json
import os
import subprocess
import sys
import time

info_fd, directory = int(sys.argv[1]), sys.argv[2]
command = sys.argv[sys.argv.index("--") + 1:]
descriptors = sorted(int(fd) for fd in os.listdir("/proc/self/fd"))
with open(os.path.join(directory, "bwrap_fds.json"), "w") as listing:
    json.dump(descriptors, listing)
open(os.path.join(directory, "spawned"), "w").close()
while not os.path.exists(os.path.join(directory, "go")):
    time.sleep(0.02)
child = subprocess.Popen(["/bin/sh", "-c", 'sleep 0.2; exec "$@"', "fake-bwrap-setup", *command])
with os.fdopen(info_fd, "w") as info:
    json.dump({"child-pid": child.pid, "mnt-namespace": 0}, info, indent=4)
status = child.wait()
sys.exit(128 - status if status < 0 else status)
"""


def start_namespace_parent(
    directory, *, go=True, pass_fds=(), signal_during_start=False, command_secs="60", ignore=()
):
    """The runner above as its own session's leader; `go` lets the fake bwrap start the command at once."""
    Path(directory, "fake_bwrap.py").write_text(FAKE_BWRAP)
    runner = Path(directory, "namespace_parent_runner.py")
    runner.write_text(NAMESPACE_PARENT_RUNNER)
    if go:
        Path(directory, "go").touch()
    options = {
        "signal_during_start": signal_during_start,
        "command_secs": command_secs,
        "ignore": [signal.Signals(number).name for number in ignore],
    }
    return subprocess.Popen(
        [PYTHON, "-I", str(runner), str(REPO_ROOT / "scripts"), str(directory), json.dumps(options)],
        start_new_session=True,
        pass_fds=pass_fds,
    )


def catches(pid, signum):
    """Whether process pid has a handler installed for signum (its SigCgt mask)."""
    with open(f"/proc/{pid}/status") as status:
        for line in status:
            if line.startswith("SigCgt:"):
                return bool(int(line.split()[1], 16) >> (signum - 1) & 1)
    return False


def ignored_signals(path):
    """The signal numbers set in the `SigIgn:` hex mask a command wrote to path."""
    mask = int(Path(path).read_text().split(":", 1)[1], 16)
    return {number for number in range(1, mask.bit_length() + 1) if mask >> (number - 1) & 1}


def start_canon(tree, cwd, *args, env, pass_fds=()):
    """<tree>/scripts/canon_run.py <args> as its own session's leader, every supervised signal at default.

    The env prefix makes a probe independent of a run_all pool or `&` that starts the suite
    with INT/QUIT ignored. stderr is piped for the failure message; no timeout.
    """
    script = Path(tree, "scripts", "canon_run.py")
    return subprocess.Popen(
        ["/usr/bin/env", "--default-signal=HUP,INT,QUIT,TERM", PYTHON, "-I", str(script), *args],
        cwd=cwd,
        env=env,
        pass_fds=pass_fds,
        start_new_session=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
    )


def descendants(pid):
    """pid and every process descended from it, found through the ppid field of /proc/*/stat."""
    children = {}
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        try:
            with open(f"/proc/{entry}/stat") as stat:
                ppid = int(stat.read().rsplit(")", 1)[1].split()[1])
        except (OSError, IndexError, ValueError):
            continue
        children.setdefault(ppid, []).append(int(entry))
    found, frontier = [], [pid]
    while frontier:
        current = frontier.pop()
        found.append(current)
        frontier.extend(children.get(current, ()))
    return found


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


RUST_TREE_FILES = {
    "Cargo.toml": '[workspace]\nmembers = ["fixture_reader"]\nresolver = "2"\n',
    "Cargo.lock": (
        "# This file is automatically @generated by Cargo.\n"
        "# It is not intended for manual editing.\n"
        "version = 4\n\n"
        '[[package]]\nname = "fixture_reader"\nversion = "0.1.0"\n'
    ),
    "fixture_reader/Cargo.toml": (
        '[package]\nname = "fixture_reader"\nversion = "0.1.0"\nedition = "2021"\n'
    ),
    "fixture_reader/src/lib.rs": "//! Holds the integration test only.\n",
    "fixture_reader/fixtures/data.txt": FIXTURE_DATA,
    "fixture_reader/tests/fixture.rs": (
        "#[test]\n"
        "fn reads_its_fixture() {\n"
        '    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/data.txt");\n'
        '    assert_eq!(std::fs::read_to_string(path).unwrap(), "fixture data\\n");\n'
        '    println!("MANIFEST_DIR={}", env!("CARGO_MANIFEST_DIR"));\n'
        "}\n"
    ),
}
CARGO_TEST = ("test", "--offline", "--", "--nocapture")


def make_rust_tree(parent, name):
    """make_tree plus a one-crate workspace whose test prints its baked CARGO_MANIFEST_DIR."""
    root = make_tree(parent, name)
    for relative, text in RUST_TREE_FILES.items():
        path = Path(root, relative)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
    return root


def real_home():
    return os.environ.get("HOME") or pwd.getpwuid(os.getuid()).pw_dir


def cargo_env(tmp, tree, **overrides):
    """Real rustup cargo with a scratch CARGO_HOME; CANON is an empty provisioned dir."""
    home = real_home()
    toolchain = tomllib.loads((REPO_ROOT / "rust-toolchain.toml").read_text())["toolchain"]
    canon = Path(tmp, "canon")
    canon.mkdir(exist_ok=True)
    env = {
        "HOME": home,
        "RUSTUP_TOOLCHAIN": toolchain["channel"],
        "CARGO_HOME": str(Path(tmp, "cargo-home")),
        "PATH": f"{tree}/scripts/canon-bin:{home}/.cargo/bin:/usr/bin:/bin",
        "REIFY_CANON_ROOT": str(canon),
        "REIFY_CANON_HOST_CONFIG": str(Path(tmp, "host.toml")),
    }
    if os.environ.get("RUSTUP_HOME"):
        env["RUSTUP_HOME"] = os.environ["RUSTUP_HOME"]
    env.update(overrides)
    return env


def recording_wrapper(tmp):
    """A RUSTC_WRAPPER that logs each call to <tmp>/wrapper.log; returns (wrapper, log)."""
    log = Path(tmp, "wrapper.log")
    wrapper = Path(tmp, "recording-rustc-wrapper")
    wrapper.write_text(f'#!/bin/sh\necho "$@" >> "{log}"\nexec "$@"\n')
    wrapper.chmod(0o755)
    return str(wrapper), log


def landlock(writable, *cmd):
    return [PYTHON, str(LANDLOCK_EXEC), "--writable", str(writable), "--", *cmd]


def bwrap_bind(source, destination, *cmd):
    return [BWRAP, "--dev-bind", "/", "/", "--bind", str(source), str(destination),
            "--die-with-parent", "--", *cmd]


def _refusal(argv):
    probe = run(argv, cwd="/", env={"PATH": "/usr/bin:/bin"})
    return "" if probe.returncode == 0 else f"(rc={probe.returncode}): {probe.stderr.strip()}"


@functools.cache
def namespace_unavailable_reason():
    """canon_run.py's own probe, binding one scratch directory at another."""
    with tempfile.TemporaryDirectory(prefix="canon-run-ns-probe-") as scratch:
        a, b = Path(scratch, "a"), Path(scratch, "b")
        a.mkdir()
        b.mkdir()
        return load_canon_run().namespace_refusal(a, str(b))


@functools.cache
def landlock_unavailable_reason():
    with tempfile.TemporaryDirectory(prefix="canon-run-landlock-probe-") as scratch:
        refusal = _refusal(landlock(scratch, "/bin/true"))
        if refusal:
            return f"landlock_exec.py cannot restrict a process here {refusal}"
        a, b = Path(scratch, "a"), Path(scratch, "b")
        a.mkdir()
        b.mkdir()
        if not _refusal(landlock(scratch, *bwrap_bind(a, b, "/bin/true"))):
            return "bwrap still enters a mount namespace under landlock here"
    return ""
