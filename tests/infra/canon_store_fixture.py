"""
canon_store_fixture.py — shared builders for the suites of scripts/canon_store.py,
the store link (test_canon_store_units.py, and the cases of
test_canon_cache_units.py and canon_cache_fixture.py that reach a store): a fake
WebDAV store whose behaviour switches at run time, loopback port helpers, and a
runner that starves a relay of file descriptors in its own process.

Not a test module itself (no `test_` prefix): run_all.sh never discovers it,
and the suites import it via the sys.path idiom. Imports no sibling fixture.
"""

import http.server
import socket
import sys
import threading
from pathlib import Path

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
import canon_store

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


relay = canon_store.Relay("127.0.0.1", store_port)
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
