"""C5's store link: the probe, the loopback relay a private sccache server reaches the store through, and the watchdog that cuts it."""
from __future__ import annotations

import contextlib
import http.client
import os
import selectors
import socket
import struct
import threading
import time
import urllib.parse
from base64 import b64encode
from collections.abc import Callable
from dataclasses import dataclass, field


@dataclass(frozen=True)
class Store:
    """Where the WebDAV store is (C13's store_url) and who to say we are."""

    url: str
    user: str | None = None
    password: str | None = field(default=None, repr=False)

    @property
    def key_prefix(self) -> str:
        return urllib.parse.urlsplit(self.url).path.strip("/")


@dataclass(frozen=True)
class LinkKnobs:
    """The link's tuning constants (tests replace them)."""

    probe_timeout: float = 2.0
    slow: float = 2.0
    watchdog_interval: float = 15.0
    watchdog_failures: int = 2


def store_url_problem(store_url: str) -> str:
    """Why store_url cannot name the store, or "" when it can."""
    parts = urllib.parse.urlsplit(store_url)
    if parts.scheme != "http" or not parts.hostname:
        return "is not an http:// URL"
    try:
        parts.hostname.encode("idna")
    except UnicodeError:
        return "names a host that is not a valid DNS name"
    return ""


def basic_auth(store: Store) -> dict[str, str]:
    if store.user is None:
        return {}
    credentials = f"{store.user}:{store.password or ''}".encode()
    return {"Authorization": "Basic " + b64encode(credentials).decode()}


def probe_store(store: Store, knobs: LinkKnobs, clock: Callable[[], float] = time.monotonic) -> bool:
    """C5: GET <store_url>/.sccache_check (sccache's own start-up check) answers 200 or 404 in time."""
    parts = urllib.parse.urlsplit(store.url)
    began = clock()
    connection = http.client.HTTPConnection(parts.hostname, parts.port or 80, timeout=knobs.probe_timeout)
    try:
        connection.request("GET", parts.path.rstrip("/") + "/.sccache_check", headers=basic_auth(store))
        response = connection.getresponse()
        response.read()
    except (OSError, UnicodeError, http.client.HTTPException):
        return False
    finally:
        connection.close()
    return response.status in (200, 404) and clock() - began <= knobs.slow


RELAY_CHUNK = 65536
RELAY_BUFFER_LIMIT = 4 * RELAY_CHUNK
RELAY_ACCEPT_BACKOFF = 0.1


def reset(connection: socket.socket) -> None:
    """Close with SO_LINGER 0, so the peer gets a reset rather than an orderly end."""
    try:
        connection.setsockopt(socket.SOL_SOCKET, socket.SO_LINGER, struct.pack("ii", 1, 0))
    except OSError:
        pass
    connection.close()


class _Stream:
    """One socket of a relayed pair: what is queued for it and whether its reading side ended."""

    def __init__(self, sock: socket.socket, *, connecting: bool = False):
        self.sock = sock
        self.peer: _Stream | None = None
        self.outgoing = bytearray()
        self.connecting = connecting
        self.read_done = False
        self.write_done = False

    def interest(self) -> int:
        if self.connecting:
            return selectors.EVENT_WRITE
        events = 0
        if not self.read_done and len(self.peer.outgoing) < RELAY_BUFFER_LIMIT:
            events |= selectors.EVENT_READ
        if self.outgoing:
            events |= selectors.EVENT_WRITE
        return events


class Relay:
    """C5's loopback byte relay from 127.0.0.1:<ephemeral> to the store.

    One thread owns every socket. cut() resets every open connection and from then
    on resets each new one at once, keeping the port so nothing else can take it;
    close() releases it. The store is resolved once, at construction, which raises
    OSError or UnicodeError when it cannot be. If the loop fails, its listener
    closes (new connections are refused) and cut() and close() still return.
    When accept() fails for want of a resource (EMFILE, ENFILE), the listener
    leaves the selector for RELAY_ACCEPT_BACKOFF, so the queued connection does
    not wake the loop again at once; it is accepted once one is free.
    """

    def __init__(self, store_host: str, store_port: int):
        family, _, _, _, address = socket.getaddrinfo(store_host, store_port, type=socket.SOCK_STREAM)[0]
        self._family, self._target = family, address
        self._listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._listener.bind(("127.0.0.1", 0))
        self.port = self._listener.getsockname()[1]
        self._wake_reader, self._wake_writer = socket.socketpair()
        self._lock = threading.Lock()
        self._requests: list[tuple[str, threading.Event]] = []
        self._is_cut = False
        self._ended = False
        self._thread: threading.Thread | None = None

    @property
    def endpoint(self) -> str:
        return f"http://127.0.0.1:{self.port}"

    def start(self) -> None:
        self._listener.listen(64)
        self._listener.setblocking(False)
        self._thread = threading.Thread(target=self._loop, name="reify-canon-relay", daemon=True)
        self._thread.start()

    def cut(self) -> None:
        self._request("cut")

    def close(self) -> None:
        if self._thread is not None:
            self._request("close")
            self._thread.join()
            self._thread = None
        for owned in (self._listener, self._wake_reader, self._wake_writer):
            owned.close()

    def __enter__(self) -> Relay:
        self.start()
        return self

    def __exit__(self, *exc_info: object) -> None:
        self.close()

    def _request(self, command: str) -> None:
        if self._thread is None:
            return
        done = threading.Event()
        with self._lock:
            if self._ended:
                return
            self._requests.append((command, done))
        with contextlib.suppress(OSError):
            self._wake_writer.send(b"\0")
        done.wait()

    def _loop(self) -> None:
        streams: dict[socket.socket, _Stream] = {}
        failed = False
        try:
            with selectors.DefaultSelector() as selector:
                self._select_until_closed(selector, streams)
        except Exception:
            failed = True
        finally:
            for stream in list(streams.values()):
                self._drop(streams, stream)
            self._end(failed)

    def _select_until_closed(
        self, selector: selectors.BaseSelector, streams: dict[socket.socket, _Stream]
    ) -> None:
        selector.register(self._listener, selectors.EVENT_READ)
        selector.register(self._wake_reader, selectors.EVENT_READ)
        registered: dict[socket.socket, int] = {}
        resume_accepting_at: float | None = None
        running = True
        while running:
            timeout = None if resume_accepting_at is None else max(0.0, resume_accepting_at - time.monotonic())
            for key, events in selector.select(timeout):
                if key.fileobj is self._listener:
                    if not self._accept(streams):
                        selector.unregister(self._listener)
                        resume_accepting_at = time.monotonic() + RELAY_ACCEPT_BACKOFF
                elif key.fileobj is self._wake_reader:
                    running = self._serve_requests(streams)
                elif key.fileobj in streams:
                    self._pump(streams, streams[key.fileobj], events)
            if resume_accepting_at is not None and time.monotonic() >= resume_accepting_at:
                selector.register(self._listener, selectors.EVENT_READ)
                resume_accepting_at = None
            self._reregister(selector, streams, registered)

    def _end(self, failed: bool) -> None:
        """Refuse new connections first when the loop failed, then release every waiting request."""
        if failed:
            self._listener.close()
        with self._lock:
            self._ended = True
            pending, self._requests = self._requests, []
        for _, done in pending:
            done.set()

    def _serve_requests(self, streams: dict[socket.socket, _Stream]) -> bool:
        self._wake_reader.recv(4096)
        with self._lock:
            requests, self._requests = self._requests, []
        running = True
        for command, done in requests:
            self._is_cut = self._is_cut or command == "cut"
            running = running and command != "close"
            for stream in list(streams.values()):
                self._drop(streams, stream)
            done.set()
        return running

    def _accept(self, streams: dict[socket.socket, _Stream]) -> bool:
        """False when accept() itself failed, leaving the connection queued."""
        try:
            client, _ = self._listener.accept()
        except BlockingIOError:
            return True
        except OSError:
            return False
        if self._is_cut:
            reset(client)
            return True
        upstream = None
        try:
            client.setblocking(False)
            upstream = socket.socket(self._family, socket.SOCK_STREAM)
            upstream.setblocking(False)
            upstream.connect_ex(self._target)
        except OSError:
            if upstream is not None:
                upstream.close()
            reset(client)
            return True
        near, far = _Stream(client), _Stream(upstream, connecting=True)
        near.peer, far.peer = far, near
        streams[client], streams[upstream] = near, far
        return True

    def _pump(self, streams: dict[socket.socket, _Stream], stream: _Stream, events: int) -> None:
        try:
            if stream.connecting:
                error = stream.sock.getsockopt(socket.SOL_SOCKET, socket.SO_ERROR)
                if error:
                    raise ConnectionRefusedError(error, os.strerror(error))
                stream.connecting = False
                return
            if events & selectors.EVENT_READ:
                data = stream.sock.recv(RELAY_CHUNK)
                if data:
                    stream.peer.outgoing += data
                else:
                    stream.read_done = True
            if events & selectors.EVENT_WRITE and stream.outgoing:
                sent = stream.sock.send(stream.outgoing)
                del stream.outgoing[:sent]
            for end in (stream, stream.peer):
                if end.peer.read_done and not end.outgoing and not end.write_done:
                    end.sock.shutdown(socket.SHUT_WR)
                    end.write_done = True
        except (BlockingIOError, InterruptedError):
            return
        except OSError:
            self._drop(streams, stream)
            return
        if stream.write_done and stream.peer.write_done:
            for end in (stream, stream.peer):
                streams.pop(end.sock, None)
                end.sock.close()

    @staticmethod
    def _drop(streams: dict[socket.socket, _Stream], stream: _Stream) -> None:
        for end in (stream, stream.peer):
            if streams.pop(end.sock, None) is not None:
                reset(end.sock)

    @staticmethod
    def _reregister(
        selector: selectors.BaseSelector,
        streams: dict[socket.socket, _Stream],
        registered: dict[socket.socket, int],
    ) -> None:
        for sock in [sock for sock in registered if sock not in streams]:
            del registered[sock]
            selector.unregister(sock)
        for sock, stream in streams.items():
            wanted = stream.interest()
            current = registered.get(sock, 0)
            if wanted == current:
                continue
            if not wanted:
                selector.unregister(sock)
                del registered[sock]
            elif current:
                selector.modify(sock, wanted)
                registered[sock] = wanted
            else:
                selector.register(sock, wanted)
                registered[sock] = wanted


def open_relay(store: Store) -> Relay | None:
    """The relay to the store's host, or None when that host does not resolve."""
    parts = urllib.parse.urlsplit(store.url)
    try:
        return Relay(parts.hostname, parts.port or 80)
    except (OSError, UnicodeError):
        return None


def _wait_on(stopped: threading.Event, seconds: float) -> bool:
    return stopped.wait(seconds)


class Watchdog:
    """C5 / boundary row 7: probe the store every interval; cut the relay after enough failures.

    A probe fails when it says unreachable or takes longer than knobs.slow on clock.
    wait(stopped, seconds) returns True once the watchdog should end.
    """

    def __init__(
        self,
        probe: Callable[[], bool],
        relay: Relay,
        knobs: LinkKnobs,
        clock: Callable[[], float] = time.monotonic,
        wait: Callable[[threading.Event, float], bool] = _wait_on,
    ):
        self._probe, self._relay, self._knobs = probe, relay, knobs
        self._clock, self._wait = clock, wait
        self._stopped = threading.Event()
        self._thread = threading.Thread(target=self._run, name="reify-canon-watchdog", daemon=True)
        self.lost = False

    def start(self) -> None:
        self._thread.start()

    def stop(self) -> None:
        self._stopped.set()
        self.join()

    def join(self) -> None:
        if self._thread.ident is not None:
            self._thread.join()

    def _run(self) -> None:
        failures = 0
        while not self._wait(self._stopped, self._knobs.watchdog_interval):
            began = self._clock()
            healthy = self._probe() and self._clock() - began <= self._knobs.slow
            failures = 0 if healthy else failures + 1
            if failures >= self._knobs.watchdog_failures:
                self._relay.cut()
                self.lost = True
                return
