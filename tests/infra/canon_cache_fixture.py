"""
canon_cache_fixture.py — shared builders for the scripts/canon_cache.py suites
(test_canon_cache_units.py, test_canon_cache_host.py): a fake WebDAV store
whose behaviour switches at run time.

Not a test module itself (no `test_` prefix): run_all.sh never discovers it,
and the suites import it via the sys.path idiom. Builds on canon_run_fixture.
"""

import http.server
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
