"""
canon_cache_fixture.py — shared builders for the scripts/canon_cache.py suites
(test_canon_cache_units.py, test_canon_cache_host.py, and the BoundRoute
cases of test_canon_run_units.py): a fake sccache whose server follows a
scripted plan, and runners that supervise a command or end like one; for the
host suite, a real `rclone serve webdav` store, a workspace with registry and C
units, its host.toml, and journal and process readers. The fake WebDAV store
and the relay's runner are the store link's, in canon_store_fixture.

Not a test module itself (no `test_` prefix): run_all.sh never discovers it,
and the suites import it via the sys.path idiom. Builds on canon_run_fixture
and canon_store_fixture.
"""

import functools
import itertools
import json
import os
import secrets
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import canon_run_fixture as fx  # noqa: E402  (imported after sys.path manipulation)
import canon_store_fixture as sfx  # noqa: E402

sys.path.insert(0, str(fx.REPO_ROOT / "scripts"))
from verify_events_lib import read_events  # noqa: E402


# A fake sccache. With SCCACHE_START_SERVER=1 it is a server: it logs itself to
# calls.jsonl beside this file, takes its behaviour from FAKE_SCCACHE_BEHAVIOUR
# or else the next entry of the comma-separated `plan` file beside it (default
# ok), and serves `stats` and `stop` lines on SCCACHE_SERVER_UDS:
#   exit           prints sccache's storage read failure and exits 2 before binding
#   readonly       prints the write-check failure, then binds
#   ok             binds
#   late_readonly  binds, and prints the write-check failure when asked for stats
#   die            binds, then exits leaving its socket file once `die-now` exists
# Otherwise it is a client: --show-stats prints the server's stats in sccache
# 0.14's JSON shape, --stop-server stops the server and waits for it to go, and
# finding no listener is logged as `autostart`.
FAKE_SCCACHE = r"""#!/usr/bin/python3 -I
import fcntl
import json
import os
import socket
import sys

HERE = os.path.dirname(os.path.abspath(sys.argv[0]))
READ_ONLY = "sccache: warning: storage write check failed: Locked\n"
LOGGED = ("SCCACHE_", "CARGO_", "REIFY_CANON_")


def inherited_fds():
    fds = []
    for name in os.listdir("/proc/self/fd"):
        try:
            os.fstat(int(name))
        except OSError:
            continue
        fds.append(int(name))
    return sorted(fds)


def log(**entry):
    entry["pid"] = os.getpid()
    with open(os.path.join(HERE, "calls.jsonl"), "a") as calls:
        calls.write(json.dumps(entry) + "\n")


def next_behaviour():
    plan_path, counter_path = os.path.join(HERE, "plan"), os.path.join(HERE, "counter")
    plan = open(plan_path).read().strip().split(",") if os.path.exists(plan_path) else []
    index = int(open(counter_path).read()) if os.path.exists(counter_path) else 0
    with open(counter_path, "w") as counter:
        counter.write(str(index + 1))
    return plan[index] if index < len(plan) else "ok"


def lock_held():
    path = os.environ.get("FAKE_SCCACHE_LOCK")
    if not path:
        return None
    with open(path, "a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return True
        fcntl.flock(lock, fcntl.LOCK_UN)
    return False


def endpoint_open(url):
    # A reset racing the handshake (a relay to a refusing store) still means something listens.
    if not url:
        return None
    host, port = url.removeprefix("http://").split("/")[0].split(":")
    try:
        socket.create_connection((host, int(port)), timeout=5).close()
    except ConnectionResetError:
        return True
    except OSError:
        return False
    return True


def stats_json():
    given = json.loads(os.environ.get("FAKE_SCCACHE_STATS") or "{}")
    return json.dumps({"stats": {
        "cache_hits": {"counts": given.get("hits", {}), "adv_counts": {}},
        "cache_misses": {"counts": given.get("misses", {}), "adv_counts": {}},
        "cache_write_errors": given.get("write_errors", 0),
    }})


def serve():
    fds = inherited_fds()
    environ = {name: value for name, value in os.environ.items() if name.startswith(LOGGED)}
    behaviour = os.environ.get("FAKE_SCCACHE_BEHAVIOUR") or next_behaviour()
    log(role="server", behaviour=behaviour, env=environ, fds=fds, lock_held=lock_held())
    if behaviour == "exit":
        sys.stderr.write("sccache: error: Server startup failed: cache storage failed to read:"
                         " Unexpected (permanent) at read => end of file\n")
        sys.exit(2)
    if behaviour == "readonly":
        sys.stderr.write(READ_ONLY)
        sys.stderr.flush()
    path = os.environ["SCCACHE_SERVER_UDS"]
    listener = socket.socket(socket.AF_UNIX)
    listener.bind(path)
    listener.listen(8)
    if behaviour == "die":
        listener.settimeout(0.02)
    while True:
        try:
            connection, _ = listener.accept()
        except TimeoutError:
            if os.path.exists(os.path.join(HERE, "die-now")):
                os._exit(0)
            continue
        with connection:
            connection.settimeout(None)
            command = connection.makefile().readline().strip()
            if command == "stats":
                if behaviour == "late_readonly":
                    sys.stderr.write(READ_ONLY)
                    sys.stderr.flush()
                log(role="server", command="stats")
                connection.sendall(stats_json().encode())
            elif command == "stop":
                log(role="server", command="stop",
                    relay_open=endpoint_open(os.environ.get("SCCACHE_WEBDAV_ENDPOINT")))
                connection.sendall(b"stopping\n")
                listener.close()
                sys.exit(0)


def client(arguments):
    command = "stats" if "--show-stats" in arguments else "stop" if "--stop-server" in arguments else ""
    connection = socket.socket(socket.AF_UNIX)
    try:
        connection.connect(os.environ.get("SCCACHE_SERVER_UDS", ""))
    except OSError:
        log(role="client", argv=arguments, autostart=True)
        sys.exit(2)
    log(role="client", argv=arguments)
    with connection:
        connection.sendall(command.encode() + b"\n")
        answer = connection.makefile().read()
    if command == "stats":
        print(answer)


if os.environ.get("SCCACHE_START_SERVER") == "1":
    serve()
else:
    client(sys.argv[1:])
"""


def fake_sccache_dir(parent, name="fake-sccache"):
    """A directory holding the fake `sccache`; returns its real path."""
    directory = Path(parent, name)
    directory.mkdir()
    sccache = directory / "sccache"
    sccache.write_text(FAKE_SCCACHE)
    sccache.chmod(0o755)
    return os.path.realpath(directory)


def set_plan(directory, plan):
    Path(directory, "plan").write_text(plan)
    Path(directory, "counter").unlink(missing_ok=True)


def read_sccache_calls(directory):
    log = Path(directory, "calls.jsonl")
    if not log.exists():
        return []
    return [json.loads(line) for line in log.read_text().splitlines()]


def servers(directory):
    return [call for call in read_sccache_calls(directory) if call["role"] == "server" and "behaviour" in call]


def process_gone(pid):
    """True once pid is neither running nor an unreaped zombie."""
    try:
        with open(f"/proc/{pid}/stat") as stat:
            return stat.read().rsplit(")", 1)[1].split()[0] == "X"
    except FileNotFoundError:
        return True


# Opens a shared session on the fake sccache (argv: scripts dir, fake dir, build
# dir, start-lock path, started marker) and supervises a command that touches
# the marker and then sleeps, so a test can signal this supervisor mid-command.
# It ends through exit_like, as canon_run.main does.
SUPERVISE_RUNNER = r"""
import dataclasses
import os
import signal
import sys

sys.path.insert(0, sys.argv[1])
import canon_cache

fake, build, lock, started, sleep_secs = sys.argv[2:7]
signal.signal(signal.SIGINT, signal.default_int_handler)
signal.signal(signal.SIGQUIT, signal.SIG_DFL)
knobs = dataclasses.replace(canon_cache.SessionKnobs(), start_lock=lock, tmp_root=build)
env = {"PATH": "/usr/bin:/bin"}
settings = canon_cache.CacheSettings(fallback_dir=os.path.join(build, "fallback"))
session = canon_cache.open_session(
    canon_cache.Policy.SHARED, settings, canon_build_dir=build,
    sccache=os.path.join(fake, "sccache"), env=env, knobs=knobs,
)
sys.exit(canon_cache.exit_like(canon_cache.supervise(
    ["/bin/sh", "-c", 'touch "$0"; exec sleep "$1"', started, sleep_secs],
    session.environment.for_command(env),
    build,
    session=session,
    record_base=canon_cache.RecordBase("canon", canon_cache.Policy.SHARED),
    subcommand="",
    channel=None,
    journal=os.path.join(build, ".reify-canon-journal.jsonl"),
)))
"""


def write_supervise_runner(directory):
    runner = Path(directory, "supervise_runner.py")
    runner.write_text(SUPERVISE_RUNNER)
    return str(runner)


# Ends this process through canon_cache.exit_like(<returncode>) (argv: scripts
# dir, returncode, comma-separated signal names to ignore first, may be empty).
# Its RLIMIT_CORE soft limit is raised to the hard limit, so a core dump would
# show unless exit_like suppresses it.
EXIT_LIKE_RUNNER = r"""
import resource
import signal
import sys

sys.path.insert(0, sys.argv[1])
import canon_cache

returncode, ignored = int(sys.argv[2]), sys.argv[3]
for name in filter(None, ignored.split(",")):
    signal.signal(signal.Signals[name], signal.SIG_IGN)
resource.setrlimit(resource.RLIMIT_CORE, (resource.getrlimit(resource.RLIMIT_CORE)[1],) * 2)
sys.exit(canon_cache.exit_like(returncode))
"""


def write_exit_like_runner(directory):
    runner = Path(directory, "exit_like_runner.py")
    runner.write_text(EXIT_LIKE_RUNNER)
    return str(runner)


# --- host suite: a real store, a workspace with registry and C units --------

STORE_USER = "reify-canon-test"
STORE_PREFIX = "reify"
HEX = "0123456789abcdef"
READY_WAIT = 60.0


class RcloneStore:
    """A private `rclone serve webdav` on loopback with basic auth, sccache's leaf directories pre-created."""

    def __init__(self, parent, rclone):
        self.directory = Path(parent, "store")
        for a, b, c in itertools.product(HEX, HEX, HEX):
            Path(self.directory, STORE_PREFIX, a, b, c).mkdir(parents=True, exist_ok=True)
        self.password = secrets.token_hex(12)
        self.port = sfx.closed_port()
        self.url = f"http://127.0.0.1:{self.port}/{STORE_PREFIX}"
        self.log = Path(parent, "rclone.log")
        with open(self.log, "wb") as log:
            self.process = subprocess.Popen(
                [rclone, "serve", "webdav", str(self.directory), "--addr", f"127.0.0.1:{self.port}",
                 "--user", STORE_USER, "--pass", self.password, "--dir-cache-time", "1s"],
                env={"PATH": "/usr/bin:/bin", "HOME": str(parent), "RCLONE_CONFIG": "/dev/null"},
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=log,
            )
        self.wait_ready()

    def settings_lines(self):
        return (
            f'store_url = "{self.url}"',
            f'store_user = "{STORE_USER}"',
            f'store_password = "{self.password}"',
        )

    def wait_ready(self):
        """Until the store answers sccache's start-up GET; RuntimeError if rclone exits or never answers."""
        canon_cache = fx.load_canon_cache()
        settings = canon_cache.CacheSettings(store_url=self.url, store_user=STORE_USER, store_password=self.password)
        deadline = time.monotonic() + READY_WAIT
        while not canon_cache.store_reachable(settings):
            if self.process.poll() is not None or time.monotonic() > deadline:
                self.stop()
                raise RuntimeError(f"rclone serve webdav did not come up: {self.log.read_text()[-2000:]}")
            time.sleep(0.05)

    def objects(self):
        """Cache entries the store holds: every file under the prefix but sccache's start-up check."""
        root = Path(self.directory, STORE_PREFIX)
        return sorted(
            path for path in root.rglob("*") if path.is_file() and path.name != ".sccache_check"
        )

    def stop(self):
        if self.process.poll() is None:
            self.process.terminate()
        self.process.wait()


CACHE_TREE_FILES = {
    "Cargo.toml": '[workspace]\nmembers = ["fixture_reader", "c_unit"]\nresolver = "2"\n',
    "fixture_reader/Cargo.toml": (
        '[package]\nname = "fixture_reader"\nversion = "0.1.0"\nedition = "2021"\n\n'
        '[dependencies]\nshlex = "1"\nc_unit = { path = "../c_unit" }\n'
    ),
    "fixture_reader/src/lib.rs": (
        "pub fn words(line: &str) -> Vec<String> {\n"
        "    shlex::split(line).unwrap_or_default()\n"
        "}\n"
    ),
    "fixture_reader/fixtures/data.txt": fx.FIXTURE_DATA,
    "c_unit/Cargo.toml": (
        '[package]\nname = "c_unit"\nversion = "0.1.0"\nedition = "2021"\nbuild = "build.rs"\n\n'
        '[build-dependencies]\ncc = "1"\n'
    ),
    "c_unit/build.rs": (
        "fn main() {\n"
        '    if let Some(fifo) = std::env::var_os("C_UNIT_BLOCK_FIFO") {\n'
        "        let _ = std::fs::read(fifo);\n"
        "    }\n"
        '    println!("cargo:rerun-if-changed=src/unit.c");\n'
        '    cc::Build::new().file("src/unit.c").compile("unit");\n'
        "}\n"
    ),
    "c_unit/src/unit.c": "int c_unit_value(void) { return 42; }\n",
    "c_unit/src/lib.rs": (
        'extern "C" {\n    fn c_unit_value() -> i32;\n}\n\n'
        "pub fn value() -> i32 {\n    unsafe { c_unit_value() }\n}\n"
    ),
}


def fixture_test(marker):
    """fixture_reader's test: reads its fixture by its baked manifest dir and prints that dir and marker."""
    return (
        "#[test]\n"
        "fn reads_its_fixture() {\n"
        '    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/data.txt");\n'
        '    assert_eq!(std::fs::read_to_string(path).unwrap(), "fixture data\\n");\n'
        '    assert_eq!(fixture_reader::words("a b"), vec!["a", "b"]);\n'
        "    assert_eq!(c_unit::value(), 42);\n"
        '    println!("MANIFEST_DIR={}", env!("CARGO_MANIFEST_DIR"));\n'
        f'    println!("MARKER={marker}");\n'
        "}\n"
    )


def write_workspace(root, marker):
    for relative, text in {**CACHE_TREE_FILES, "fixture_reader/tests/fixture.rs": fixture_test(marker)}.items():
        path = Path(root, relative)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)


def real_cargo_env():
    """Real rustup cargo under the pinned CARGO_HOME, run directly rather than through the primitive."""
    home = fx.real_home()
    toolchain = tomllib.loads((fx.REPO_ROOT / "rust-toolchain.toml").read_text())["toolchain"]
    env = {
        "HOME": home,
        "RUSTUP_TOOLCHAIN": toolchain["channel"],
        "CARGO_HOME": fx.load_canon_cache().PINNED_CARGO_HOME,
        "PATH": f"{home}/.cargo/bin:/usr/bin:/bin",
    }
    if os.environ.get("RUSTUP_HOME"):
        env["RUSTUP_HOME"] = os.environ["RUSTUP_HOME"]
    return env


@functools.cache
def cache_lockfile():
    """(Cargo.lock text, "") from `cargo generate-lockfile --offline`, or ("", why it failed)."""
    with tempfile.TemporaryDirectory(prefix="canon-cache-lock-") as scratch:
        write_workspace(scratch, "lock")
        try:
            result = fx.run(["cargo", "generate-lockfile", "--offline"], cwd=scratch, env=real_cargo_env())
        except OSError as exc:
            return "", f"cargo cannot run: {exc}"
        if result.returncode != 0:
            last = next(iter(reversed(result.stderr.strip().splitlines())), "no stderr")
            return "", f"offline lock generation failed under the pinned CARGO_HOME (rc={result.returncode}): {last}"
        return Path(scratch, "Cargo.lock").read_text(), ""


def make_cache_tree(parent, name, marker):
    """A canon_run tree holding the workspace; fixture_reader's test prints marker."""
    root = fx.make_tree(parent, name)
    write_workspace(root, marker)
    lock, reason = cache_lockfile()
    if reason:
        raise RuntimeError(reason)
    Path(root, "Cargo.lock").write_text(lock)
    return root


def host_env(tmp, tree):
    """fx.cargo_env with the pinned CARGO_HOME; one CANON per test, shared by its trees."""
    return fx.cargo_env(tmp, tree, CARGO_HOME=fx.load_canon_cache().PINNED_CARGO_HOME)


def write_cache_host_config(path, *, policy, store, fallback_dir):
    """An enabled host.toml naming the store (None: no store_url) and a fallback under the test's tmp."""
    lines = (*(store.settings_lines() if store else ()), f'fallback_dir = "{fallback_dir}"')
    fx.write_host_config(path, enabled=True, policy=policy, extra_lines=lines)


def read_journal(build_dir):
    events = read_events(fx.load_canon_cache().journal_path(build_dir))
    if events.skipped_lines:
        raise AssertionError(f"{events.skipped_lines} unreadable journal lines in {build_dir}")
    return list(events.records)


def processes_naming(needle):
    """pids of this user's processes whose environment holds needle (bytes)."""
    found = []
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        try:
            with open(f"/proc/{entry}/environ", "rb") as environ:
                if needle in environ.read():
                    found.append(int(entry))
        except OSError:
            continue
    return found


def wait_until_none_name(needle, wait=READY_WAIT):
    """A bounded liveness wait: the pids still naming needle when none do, or when the wait ends."""
    deadline = time.monotonic() + wait
    while (remaining := processes_naming(needle)) and time.monotonic() < deadline:
        time.sleep(0.1)
    return remaining


def rclone_binary():
    return shutil.which("rclone")
