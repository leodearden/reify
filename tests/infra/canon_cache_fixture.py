"""
canon_cache_fixture.py — shared builders for the scripts/canon_cache.py suites
(test_canon_cache_units.py, test_canon_cache_host.py, and the BoundRoute
cases of test_canon_run_units.py): a fake WebDAV store whose behaviour switches
at run time, and a fake sccache whose server follows a scripted plan; for the
host suite, a real `rclone serve webdav` store, a workspace with registry and C
units, its host.toml, and journal and process readers.

Not a test module itself (no `test_` prefix): run_all.sh never discovers it,
and the suites import it via the sys.path idiom. Builds on canon_run_fixture.
"""

import functools
import http.server
import itertools
import json
import os
import secrets
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import tomllib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import canon_run_fixture as fx  # noqa: E402  (imported after sys.path manipulation)

sys.path.insert(0, str(fx.REPO_ROOT / "scripts"))
from verify_events_lib import read_events  # noqa: E402

SLOW_PREFIX = "/slow"
SLOW_HEAD = b"x" * 1024
SLOW_LENGTH = 1024 * 1024
RELEASE_WAIT = 60.0


class _StoreHandler(http.server.BaseHTTPRequestHandler):
    """Answers as the owning FakeStore's mode says; every response is byte-deterministic."""

    protocol_version = "HTTP/1.1"
    server_version = "FakeStore"
    sys_version = ""

    def date_time_string(self, timestamp=None):
        return "Thu, 01 Jan 2026 00:00:00 GMT"

    def log_message(self, format, *args):
        pass

    def do_GET(self):
        store = self.server.store
        store.record(self)
        if store.mode == "hang":
            store.released.wait(RELEASE_WAIT)
            return
        if store.mode == "auth":
            self.answer(401, b"")
        elif self.path.endswith("/.sccache_check"):
            self.answer(store.check_status, b"")
        elif self.path.startswith(SLOW_PREFIX):
            self.send_response(200)
            self.send_header("Content-Length", str(SLOW_LENGTH))
            self.end_headers()
            self.wfile.write(SLOW_HEAD)
            self.wfile.flush()
            store.released.wait(RELEASE_WAIT)
            self.wfile.write(b"x" * (SLOW_LENGTH - len(SLOW_HEAD)))
        else:
            self.answer(200, f"body of {self.path}\n".encode())

    def do_PUT(self):
        store = self.server.store
        store.record(self)
        length = int(self.headers.get("Content-Length") or 0)
        self.rfile.read(length)
        self.answer(201 if store.mode == "ok" else 401, b"")

    def answer(self, status, body):
        self.send_response(status)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


class _Server(http.server.ThreadingHTTPServer):
    daemon_threads = True
    block_on_close = False

    def handle_error(self, request, client_address):
        """A transfer the relay cut is expected, so a green run prints no traceback for it."""
        if isinstance(sys.exception(), (ConnectionResetError, BrokenPipeError)):
            return
        super().handle_error(request, client_address)


class FakeStore:
    """A loopback HTTP store: 'ok', 'auth' (401), 'hang' (never answers) or 'closed'.

    'ok' answers .sccache_check with check_status (404 by default), other GETs with
    200, PUT with 201; a GET under /slow sends SLOW_HEAD and then waits for release().
    """

    def __init__(self):
        self.mode = "ok"
        self.check_status = 404
        self.requests = []
        self.authorizations = []
        self.released = threading.Event()
        self._lock = threading.Lock()
        self._server = _Server(("127.0.0.1", 0), _StoreHandler)
        self._server.store = self
        self.port = self._server.server_address[1]
        self.url = f"http://127.0.0.1:{self.port}/reify"
        self._thread = threading.Thread(
            target=self._server.serve_forever, kwargs={"poll_interval": 0.05}, daemon=True
        )
        self._thread.start()

    def record(self, handler):
        with self._lock:
            self.requests.append((handler.command, handler.path))
            self.authorizations.append(handler.headers.get("Authorization"))

    def set_mode(self, mode):
        if mode == "closed":
            self._close_listener()
        self.mode = mode

    def release(self):
        self.released.set()

    def _close_listener(self):
        if self._thread is not None:
            self._server.shutdown()
            self._server.server_close()
            self._thread = None

    def stop(self):
        self.release()
        self._close_listener()

    def __enter__(self):
        return self

    def __exit__(self, *exc_info):
        self.stop()


def closed_port():
    """A loopback port nothing listens on (bound, then released)."""
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def can_bind(port):
    """Whether a listener could take the loopback port now (SO_REUSEADDR: TIME_WAIT does not count)."""
    with socket.socket() as probe:
        probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            probe.bind(("127.0.0.1", port))
        except OSError:
            return False
        return True


def raw_get(port, path):
    """One HTTP/1.0 GET over a fresh connection; the full response bytes, or what arrived before a reset.

    A reset can beat the connect itself (a relay resets at once when its store refuses).
    """
    received = b""
    try:
        with socket.create_connection(("127.0.0.1", port)) as connection:
            connection.sendall(f"GET {path} HTTP/1.0\r\nHost: store\r\n\r\n".encode())
            while chunk := connection.recv(65536):
                received += chunk
    except (ConnectionResetError, BrokenPipeError):
        pass
    return received


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


# A relay run out of file descriptors, in its own process so the suite's
# RLIMIT_NOFILE never changes. argv: <scripts dir> <store port> <case>; prints
# one JSON verdict line. Every socket wait has a 30 s hang guard (not a bound).
#   accept  one descriptor left: the relay accepts a client but cannot open its
#           upstream; then, with the limit restored, a fresh GET through it
#   loop    no descriptor left when the relay's loop starts; then cut, a connect
#           to its port, and close
#   backoff no descriptor left while a client waits to be accepted: the process
#           time this process spends over the next second (a spinning relay
#           thread spends about all of it), then, with the limit restored, that
#           client's GET
RELAY_FD_RUNNER = r"""
import json
import os
import resource
import socket
import sys
import time

sys.path.insert(0, sys.argv[1])
import canon_cache

store_port, case = int(sys.argv[2]), sys.argv[3]
HANG_GUARD = 30.0
REQUEST = b"GET /reify/object HTTP/1.0\r\nHost: store\r\n\r\n"
soft, hard = resource.getrlimit(resource.RLIMIT_NOFILE)


def lowest_free_fd():
    descriptor = os.open("/dev/null", os.O_RDONLY)
    os.close(descriptor)
    return descriptor


def limit_open_files(count):
    resource.setrlimit(resource.RLIMIT_NOFILE, (count, hard))


def holds_a_selector():
    for name in os.listdir("/proc/self/fd"):
        try:
            if os.readlink(f"/proc/self/fd/{name}") == "anon_inode:[eventpoll]":
                return True
        except OSError:
            pass
    return False


def wait_for_the_relay_selector():
    deadline = time.monotonic() + HANG_GUARD
    while not holds_a_selector():
        if time.monotonic() > deadline:
            sys.exit("the relay thread never created its selector")
        time.sleep(0.01)


def guarded_socket():
    client = socket.socket()
    client.settimeout(HANG_GUARD)
    return client


def exchange(client, port=None):
    # The HTTP status through the relay; "reset" for a reset or an empty answer, "timeout" at the guard.
    # With no port the client is already connected.
    received = b""
    try:
        if port is not None:
            client.connect(("127.0.0.1", port))
        client.sendall(REQUEST)
        while chunk := client.recv(65536):
            received += chunk
    except (ConnectionResetError, BrokenPipeError):
        return "reset"
    except TimeoutError:
        return "timeout"
    finally:
        client.close()
    return int(received.split()[1]) if received else "reset"


def refused(port):
    with guarded_socket() as probe:
        try:
            probe.connect(("127.0.0.1", port))
        except ConnectionRefusedError:
            return True
    return False


relay = canon_cache.Relay("127.0.0.1", store_port)
if case == "accept":
    relay.start()
    wait_for_the_relay_selector()
    client = guarded_socket()
    limit_open_files(lowest_free_fd() + 1)
    first = exchange(client, relay.port)
    limit_open_files(soft)
    second = exchange(guarded_socket(), relay.port)
    relay.cut()
    relay.close()
    verdict = {"first": first, "second": second, "closed": True}
elif case == "loop":
    limit_open_files(lowest_free_fd())
    relay.start()
    relay.cut()
    limit_open_files(soft)
    was_refused = refused(relay.port)
    relay.close()
    verdict = {"cut": True, "refused": was_refused, "closed": True}
elif case == "backoff":
    relay.start()
    wait_for_the_relay_selector()
    client = guarded_socket()
    limit_open_files(lowest_free_fd())
    client.connect(("127.0.0.1", relay.port))
    before = time.process_time()
    time.sleep(1.0)
    spent = time.process_time() - before
    limit_open_files(soft)
    status = exchange(client)
    relay.cut()
    relay.close()
    verdict = {"cpu_secs": round(spent, 3), "served": status, "closed": True}
else:
    sys.exit(f"unknown case {case!r}")
print(json.dumps(verdict))
"""


def write_relay_fd_runner(directory):
    runner = Path(directory, "relay_fd_runner.py")
    runner.write_text(RELAY_FD_RUNNER)
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
        self.port = closed_port()
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
        while not canon_cache.probe_store(settings, canon_cache.SessionKnobs()):
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
