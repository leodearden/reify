"""The cache half of the canonical-build-path primitive.

scripts/canon_run.py decides where a command runs; this module decides what
cache it runs with (dark-factory plans/canonical-build-path-shared-cache-prd.md
C4-C6): the per-policy cargo and command environment and the CARGO_HOME pin,
the host.toml store and fallback settings, which real sccache serves them, and
the store link: the probe, the loopback relay sccache talks to the store
through, and the watchdog that cuts that relay when the store stops answering.
It knows nothing of trees, build-directory modes or mount namespaces, and
imports nothing from canon_run.
"""
from __future__ import annotations

import http.client
import os
import selectors
import socket
import struct
import tempfile
import threading
import time
import urllib.parse
from base64 import b64encode
from collections.abc import Callable, Mapping
from dataclasses import asdict, dataclass, field, fields
from enum import StrEnum
from pathlib import Path


class Policy(StrEnum):
    SHARED = "shared"
    MIXED = "mixed"
    INCREMENTAL = "incremental"


class Backend(StrEnum):
    SHARED = "shared"
    LOCAL_FALLBACK = "local_fallback"
    NONE = "none"


class Reason(StrEnum):
    NONE = ""
    STORE_UNREACHABLE = "store_unreachable"
    STORE_LOST = "store_lost"
    STORE_NOT_CONFIGURED = "store_not_configured"
    READ_ONLY_SERVER = "read_only_server"
    LEGACY_BUILD_DIR = "legacy_build_dir"
    CANON_UNAVAILABLE = "canon_unavailable"


PINNED_CARGO_HOME = "/tmp/reify-agent-cargo-home"
GUARD_MARKER = b"reify-canon-guard"
MARKER_WINDOW = 1024
DEFAULT_FALLBACK_DIR = "~/.cache/reify-canon/fallback"
DEFAULT_FALLBACK_CAP = "10G"
CLIENT_IDLE_TIMEOUT = "1800"
CARGO_INCREMENTAL_BY_POLICY = {Policy.SHARED: "0", Policy.MIXED: "1", Policy.INCREMENTAL: "1"}
SETTING_KEYS = ("store_url", "store_user", "store_password", "fallback_dir", "fallback_cap")


class CacheConfigError(ValueError):
    """A one-line reason host.toml's cache settings cannot be used."""


@dataclass(frozen=True)
class CacheSettings:
    """host.toml's store and fallback keys (C13)."""

    store_url: str | None = None
    store_user: str | None = None
    store_password: str | None = field(default=None, repr=False)
    fallback_dir: str = os.path.expanduser(DEFAULT_FALLBACK_DIR)
    fallback_cap: str = DEFAULT_FALLBACK_CAP


def parse_cache_settings(table: Mapping[str, object], path: str | Path) -> CacheSettings:
    values = {}
    for key in SETTING_KEYS:
        value = table.get(key)
        if value is None:
            continue
        if not isinstance(value, str):
            raise CacheConfigError(f"host config {path}: {key} {value!r} is not a string")
        values[key] = value
    store_url = values.get("store_url")
    if store_url is not None:
        parts = urllib.parse.urlsplit(store_url)
        if parts.scheme != "http" or not parts.hostname:
            raise CacheConfigError(f"host config {path}: store_url {store_url!r} is not an http:// URL")
    if "fallback_dir" in values:
        values["fallback_dir"] = os.path.expanduser(values["fallback_dir"])
    return CacheSettings(**values)


def is_writable_dir(directory: str) -> bool:
    """Created if missing, and a file can really be made in it (landlock hides from access(2))."""
    try:
        os.makedirs(directory, exist_ok=True)
        descriptor, probe = tempfile.mkstemp(dir=directory, prefix=".reify-canon-write-probe.")
    except OSError:
        return False
    os.close(descriptor)
    os.unlink(probe)
    return True


def choose_fallback_dir(settings: CacheSettings, tmp_root: str = "/tmp") -> str:
    """The configured fallback directory, else a per-user one under tmp_root (a landlocked agent)."""
    if is_writable_dir(settings.fallback_dir):
        return settings.fallback_dir
    per_user = os.path.join(tmp_root, f"reify-canon-fallback-{os.getuid()}")
    os.makedirs(per_user, mode=0o700, exist_ok=True)
    return per_user


@dataclass(frozen=True)
class ServerAddress:
    """Where a session's private server listens and what its clients fall back to."""

    socket: str
    sccache: str
    uncached_log: str
    fallback_dir: str
    fallback_cap: str


@dataclass(frozen=True)
class CacheEnvironment:
    """C4's variable table: what cargo, and the command around it, get under a policy.

    server is None under incremental, which uses no sccache in any form.
    """

    policy: Policy
    server: ServerAddress | None

    @property
    def uses_guard(self) -> bool:
        return self.policy is not Policy.INCREMENTAL

    def for_command(self, env: Mapping[str, str]) -> dict[str, str]:
        """The command's environment: the pin and, with sccache, the client's server variables."""
        composed = dict(env)
        composed["CARGO_HOME"] = PINNED_CARGO_HOME
        if self.uses_guard:
            drop_sccache_variables(composed)
            composed.update(self.client_variables())
        return composed

    def for_cargo(self, env: Mapping[str, str]) -> dict[str, str]:
        """The real cargo's environment: for_command plus the wrapper and incremental setting."""
        composed = self.for_command(env)
        composed.pop("CARGO_BUILD_RUSTC_WRAPPER", None)
        if self.uses_guard:
            composed["RUSTC_WRAPPER"] = "sccache"
        else:
            composed.pop("RUSTC_WRAPPER", None)
            drop_sccache_variables(composed)
        composed["CARGO_INCREMENTAL"] = CARGO_INCREMENTAL_BY_POLICY[self.policy]
        return composed

    def client_variables(self) -> dict[str, str]:
        if self.server is None:
            return {}
        return {
            "SCCACHE_SERVER_UDS": self.server.socket,
            "SCCACHE_DIR": self.server.fallback_dir,
            "SCCACHE_CACHE_SIZE": self.server.fallback_cap,
            "SCCACHE_IDLE_TIMEOUT": CLIENT_IDLE_TIMEOUT,
            "REIFY_CANON_SCCACHE": self.server.sccache,
            "REIFY_CANON_UNCACHED_LOG": self.server.uncached_log,
        }

    def to_record(self) -> dict[str, object]:
        server = None if self.server is None else asdict(self.server)
        return {"policy": self.policy.value, "server": server}

    @classmethod
    def from_record(cls, record: object) -> CacheEnvironment | None:
        if not isinstance(record, dict) or "server" not in record:
            return None
        policy, server = record.get("policy"), record["server"]
        if policy not in tuple(Policy):
            return None
        if server is None:
            return cls(Policy(policy), None)
        names = [address_field.name for address_field in fields(ServerAddress)]
        if not isinstance(server, dict) or not all(isinstance(server.get(name), str) for name in names):
            return None
        return cls(Policy(policy), ServerAddress(**{name: server[name] for name in names}))


def drop_sccache_variables(env: dict[str, str]) -> None:
    for name in [name for name in env if name.startswith("SCCACHE_")]:
        del env[name]


def is_guard(path: str) -> bool:
    try:
        with open(path, "rb") as candidate:
            return GUARD_MARKER in candidate.read(MARKER_WINDOW)
    except OSError:
        return False


def resolve_real_sccache(path_value: str) -> str | None:
    """The first executable `sccache` on path_value that is not a reify-canon guard."""
    for entry in path_value.split(os.pathsep):
        candidate = os.path.join(entry, "sccache")
        if entry and os.path.isfile(candidate) and os.access(candidate, os.X_OK) and not is_guard(candidate):
            return candidate
    return None


@dataclass(frozen=True)
class SessionKnobs:
    """Every tuning constant of a session in one place (tests replace them)."""

    probe_timeout: float = 2.0
    slow: float = 2.0
    watchdog_interval: float = 15.0
    watchdog_failures: int = 2
    start_attempts: int = 3
    retry_delay: tuple[float, float] = (0.5, 2.0)
    socket_wait: float = 60.0
    start_lock: str = "/tmp/reify-canon-sccache-start.lock"
    start_lock_wait: float = 30.0
    tmp_root: str = "/tmp"


def basic_auth(settings: CacheSettings) -> dict[str, str]:
    if settings.store_user is None:
        return {}
    credentials = f"{settings.store_user}:{settings.store_password or ''}".encode()
    return {"Authorization": "Basic " + b64encode(credentials).decode()}


def probe_store(
    settings: CacheSettings, knobs: SessionKnobs, clock: Callable[[], float] = time.monotonic
) -> bool:
    """C5: GET <store_url>/.sccache_check (sccache's own start-up check) answers 200 or 404 in time."""
    if settings.store_url is None:
        return False
    parts = urllib.parse.urlsplit(settings.store_url)
    began = clock()
    connection = http.client.HTTPConnection(parts.hostname, parts.port or 80, timeout=knobs.probe_timeout)
    try:
        connection.request("GET", parts.path.rstrip("/") + "/.sccache_check", headers=basic_auth(settings))
        response = connection.getresponse()
        response.read()
    except (OSError, http.client.HTTPException):
        return False
    finally:
        connection.close()
    return response.status in (200, 404) and clock() - began <= knobs.slow


def choose_backend(settings: CacheSettings, probe: Callable[[], bool]) -> tuple[Backend, Reason]:
    """Boundary row 6: the backend a session starts with; probe runs only with a store_url."""
    if settings.store_url is None:
        return Backend.LOCAL_FALLBACK, Reason.STORE_NOT_CONFIGURED
    if not probe():
        return Backend.LOCAL_FALLBACK, Reason.STORE_UNREACHABLE
    return Backend.SHARED, Reason.NONE


RELAY_CHUNK = 65536
RELAY_BUFFER_LIMIT = 4 * RELAY_CHUNK


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
    close() releases it.
    """

    def __init__(self, store_host: str, store_port: int):
        self._target = (store_host, store_port)
        self._listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._listener.bind(("127.0.0.1", 0))
        self.port = self._listener.getsockname()[1]
        self._wake_reader, self._wake_writer = socket.socketpair()
        self._lock = threading.Lock()
        self._requests: list[tuple[str, threading.Event]] = []
        self._is_cut = False
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
            self._requests.append((command, done))
        self._wake_writer.send(b"\0")
        done.wait()

    def _loop(self) -> None:
        selector = selectors.DefaultSelector()
        selector.register(self._listener, selectors.EVENT_READ)
        selector.register(self._wake_reader, selectors.EVENT_READ)
        streams: dict[socket.socket, _Stream] = {}
        registered: dict[socket.socket, int] = {}
        running = True
        while running:
            for key, events in selector.select():
                if key.fileobj is self._listener:
                    self._accept(streams)
                elif key.fileobj is self._wake_reader:
                    running = self._serve_requests(streams)
                elif key.fileobj in streams:
                    self._pump(streams, streams[key.fileobj], events)
            self._reregister(selector, streams, registered)
        selector.close()

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

    def _accept(self, streams: dict[socket.socket, _Stream]) -> None:
        try:
            client, _ = self._listener.accept()
        except (BlockingIOError, InterruptedError):
            return
        if self._is_cut:
            reset(client)
            return
        client.setblocking(False)
        upstream = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        upstream.setblocking(False)
        upstream.connect_ex(self._target)
        near, far = _Stream(client), _Stream(upstream, connecting=True)
        near.peer, far.peer = far, near
        streams[client], streams[upstream] = near, far

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
        knobs: SessionKnobs,
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
