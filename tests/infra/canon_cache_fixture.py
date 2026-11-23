"""
canon_cache_fixture.py — shared builders for the scripts/canon_cache.py suites
(test_canon_cache_units.py, test_canon_cache_host.py): a fake WebDAV store
whose behaviour switches at run time, and a fake sccache whose server follows a
scripted plan.

Not a test module itself (no `test_` prefix): run_all.sh never discovers it,
and the suites import it via the sys.path idiom. Builds on canon_run_fixture.
"""

import http.server
import json
import os
import socket
import sys
import threading
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import canon_run_fixture as fx  # noqa: E402,F401  (imported after sys.path manipulation)

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
    """One HTTP/1.0 GET over a fresh connection; the full response bytes, or what arrived before a reset."""
    received = b""
    with socket.create_connection(("127.0.0.1", port)) as connection:
        try:
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
    if not url:
        return None
    host, port = url.removeprefix("http://").split("/")[0].split(":")
    try:
        socket.create_connection((host, int(port)), timeout=5).close()
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
