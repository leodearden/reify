"""The cache half of the canonical-build-path primitive.

scripts/canon_run.py decides where a command runs; this module decides what
cache it runs with (dark-factory plans/canonical-build-path-shared-cache-prd.md
C4-C6): the per-policy cargo and command environment and the CARGO_HOME pin,
the host.toml store and fallback settings, which real sccache serves them, and
the store link: the probe, the loopback relay sccache talks to the store
through, and the watchdog that cuts that relay when the store stops answering;
the private per-invocation server session over a unix socket; supervise(),
which runs one command around a session and measures it; and the build_cache
record, written by the event channel's one encoder.
It knows nothing of trees, build-directory modes or mount namespaces, and
imports nothing from canon_run.
"""
from __future__ import annotations

import contextlib
import fcntl
import http.client
import json
import os
import random
import re
import resource
import selectors
import shutil
import signal
import socket
import stat
import struct
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse
from base64 import b64encode
from collections.abc import Callable, Iterator, Mapping
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


class CacheSessionError(Exception):
    """A one-line reason no private sccache server could be started."""


@contextlib.contextmanager
def refusing_os_errors(action: str) -> Iterator[None]:
    """An OSError from the body as the one-line CacheSessionError `cannot <action>: <why>`."""
    try:
        yield
    except OSError as exc:
        raise CacheSessionError(f"cannot {action}: {exc.strerror or exc}") from exc


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
    problem = None if store_url is None else store_url_problem(store_url)
    if problem:
        raise CacheConfigError(f"host config {path}: store_url {store_url!r} {problem}")
    if "fallback_dir" in values:
        values["fallback_dir"] = os.path.expanduser(values["fallback_dir"])
    return CacheSettings(**values)


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


def private_dir_problem(info: os.stat_result, uid: int) -> str:
    """Why lstat info is not uid's own directory closed to group and others, or "" when it is."""
    if stat.S_ISLNK(info.st_mode):
        return "is a symbolic link"
    if not stat.S_ISDIR(info.st_mode):
        return "is not a directory"
    if info.st_uid != uid:
        return f"is owned by uid {info.st_uid}, not {uid}"
    if info.st_mode & 0o022:
        return f"has mode {oct(stat.S_IMODE(info.st_mode))}: writable by group or others"
    return ""


def choose_fallback_dir(settings: CacheSettings, tmp_root: str = "/tmp") -> str:
    """The configured fallback directory, else a per-user one under tmp_root (a landlocked agent).

    Anyone can pre-create a path under a shared tmp_root, so the per-user one is used only
    when it is this user's private directory.
    """
    if is_writable_dir(settings.fallback_dir):
        return settings.fallback_dir
    per_user = os.path.join(tmp_root, f"reify-canon-fallback-{os.getuid()}")
    with refusing_os_errors(f"create the fallback directory {per_user}"):
        os.makedirs(per_user, mode=0o700, exist_ok=True)
        problem = private_dir_problem(os.lstat(per_user), os.getuid())
    if problem:
        raise CacheSessionError(f"fallback directory {per_user} {problem}; remove it, or set fallback_dir in host.toml")
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


def is_executable_file(path: str) -> bool:
    return os.path.isfile(path) and os.access(path, os.X_OK)


def resolve_real_sccache(path_value: str) -> str | None:
    """The first executable `sccache` on path_value that is not a reify-canon guard."""
    for entry in path_value.split(os.pathsep):
        candidate = os.path.join(entry, "sccache")
        if entry and is_executable_file(candidate) and not is_guard(candidate):
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
    except (OSError, UnicodeError, http.client.HTTPException):
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


SETPRIV = "/usr/bin/setpriv"
READ_ONLY_LINE = "storage write check failed"
POLL_INTERVAL = 0.05


@dataclass(frozen=True)
class SessionOutcome:
    backend: Backend
    reason: Reason
    hits: int
    misses: int
    write_errors: int
    detached_server: int
    store_attempts: int


@dataclass(frozen=True)
class SessionFiles:
    """The supervisor's files in the build directory, named by its pid."""

    socket: str
    err: str
    uncached_log: str

    NAMES_AROUND_PID = (
        (".reify-sccache.", ".sock"),
        (".reify-sccache.", ".err"),
        (".reify-canon-uncached.", ".log"),
    )

    @classmethod
    def in_build_dir(cls, build_dir: str, pid: int) -> SessionFiles:
        return cls(*(os.path.join(build_dir, f"{prefix}{pid}{suffix}") for prefix, suffix in cls.NAMES_AROUND_PID))

    @classmethod
    def owner_pid(cls, name: str) -> int | None:
        """The pid a basename names when it is exactly one of these files' names, else None."""
        for prefix, suffix in cls.NAMES_AROUND_PID:
            spelled = re.fullmatch(re.escape(prefix) + "([1-9][0-9]*)" + re.escape(suffix), name)
            if spelled:
                return int(spelled.group(1))
        return None


def with_parent_death_signal(argv: list[str], signal_name: str) -> list[str]:
    """argv under setpriv --pdeathsig, so it dies with the thread that spawned it."""
    if os.access(SETPRIV, os.X_OK):
        return [SETPRIV, "--pdeathsig", signal_name, "--", *argv]
    return argv


def answers(socket_path: str) -> bool:
    with socket.socket(socket.AF_UNIX) as probe:
        try:
            probe.connect(socket_path)
        except OSError:
            return False
    return True


def read_text(path: str) -> str:
    try:
        return Path(path).read_text(errors="replace")
    except OSError:
        return ""


def unlink_quietly(path: str) -> None:
    with contextlib.suppress(OSError):
        os.unlink(path)


def pid_alive(pid: int) -> bool:
    """Whether a process with this pid exists, by signal 0; a zombie counts as alive."""
    try:
        os.kill(pid, 0)
    except (ProcessLookupError, OverflowError):
        return False
    except PermissionError:
        return True
    return True


def remove_stale_session_files(build_dir: str, own_pid: int) -> None:
    """Unlink build_dir's session files named by a dead pid, or by own_pid, which has started none there yet."""
    try:
        names = os.listdir(build_dir)
    except OSError:
        return
    for name in names:
        owner = SessionFiles.owner_pid(name)
        if owner is not None and (owner == own_pid or not pid_alive(owner)):
            unlink_quietly(os.path.join(build_dir, name))


def reap(process: subprocess.Popen, knobs: SessionKnobs) -> None:
    """Wait for process, terminating and then killing it when it outlives socket_wait."""
    for escalate in (None, process.terminate, process.kill):
        if escalate is not None:
            escalate()
        try:
            process.wait(knobs.socket_wait)
            return
        except subprocess.TimeoutExpired:
            continue
    process.wait()


def stop_now(process: subprocess.Popen, knobs: SessionKnobs) -> None:
    if process.poll() is None:
        process.terminate()
    reap(process, knobs)


@contextlib.contextmanager
def start_lock(knobs: SessionKnobs, clock: Callable[[], float]) -> Iterator[None]:
    """The host's server-start lock, waited for at most start_lock_wait; then start anyway."""
    try:
        lock = open(knobs.start_lock, "a")
    except OSError:
        yield
        return
    with lock:
        deadline = clock() + knobs.start_lock_wait
        while True:
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                if clock() >= deadline:
                    break
                time.sleep(POLL_INTERVAL)
        yield


def server_env(
    env: Mapping[str, str],
    files: SessionFiles,
    backend: Backend,
    settings: CacheSettings,
    fallback_dir: str,
    relay: Relay | None,
) -> dict[str, str]:
    """C4's server environment: no inherited SCCACHE_*, a foreground server, then the backend."""
    composed = dict(env)
    drop_sccache_variables(composed)
    composed.update(
        SCCACHE_START_SERVER="1",
        SCCACHE_NO_DAEMON="1",
        SCCACHE_IDLE_TIMEOUT="0",
        SCCACHE_SERVER_UDS=files.socket,
    )
    if backend is Backend.SHARED:
        composed["SCCACHE_WEBDAV_ENDPOINT"] = relay.endpoint
        composed["SCCACHE_WEBDAV_KEY_PREFIX"] = urllib.parse.urlsplit(settings.store_url).path.strip("/")
        if settings.store_user is not None:
            composed["SCCACHE_WEBDAV_USERNAME"] = settings.store_user
        if settings.store_password is not None:
            composed["SCCACHE_WEBDAV_PASSWORD"] = settings.store_password
    else:
        composed["SCCACHE_DIR"] = fallback_dir
        composed["SCCACHE_CACHE_SIZE"] = settings.fallback_cap
    return composed


class StartFailure(StrEnum):
    NONE = ""
    EXITED = "exited"
    READ_ONLY = "read_only"


def start_server(
    sccache: str, env: Mapping[str, str], files: SessionFiles, knobs: SessionKnobs, clock: Callable[[], float]
) -> tuple[subprocess.Popen, StartFailure]:
    """Spawn a foreground server under the start lock and check it: ready, and not read-only."""
    with start_lock(knobs, clock):
        unlink_quietly(files.socket)
        with refusing_os_errors(f"open {files.err}"):
            err = open(files.err, "wb")
        with err, refusing_os_errors(f"start {sccache}"):
            process = subprocess.Popen(
                with_parent_death_signal([sccache], "TERM"),
                env=env,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=err,
                close_fds=True,
            )
        deadline = clock() + knobs.socket_wait
        while not answers(files.socket):
            if process.poll() is not None or clock() >= deadline:
                return process, StartFailure.EXITED
            time.sleep(POLL_INTERVAL)
        if READ_ONLY_LINE in read_text(files.err):
            return process, StartFailure.READ_ONLY
        return process, StartFailure.NONE


def summed_counters(stats_text: str) -> tuple[int, int, int]:
    """(hits, misses, write_errors) from `--show-stats --stats-format json`, languages summed."""
    try:
        stats = json.loads(stats_text)["stats"]
        return (
            sum(stats["cache_hits"]["counts"].values()),
            sum(stats["cache_misses"]["counts"].values()),
            int(stats["cache_write_errors"]),
        )
    except (ValueError, KeyError, TypeError, AttributeError):
        return 0, 0, 0


class CacheSession:
    """A private sccache server for one outermost invocation, and how to tear it down (C4, C5)."""

    def __init__(
        self,
        *,
        environment: CacheEnvironment,
        backend: Backend,
        reason: Reason,
        store_attempts: int,
        process: subprocess.Popen,
        relay: Relay | None,
        watchdog: Watchdog | None,
        files: SessionFiles,
        client_env: Mapping[str, str],
        knobs: SessionKnobs,
    ):
        self.environment = environment
        self.backend = backend
        self.reason = reason
        self.store_attempts = store_attempts
        self.watchdog = watchdog
        self._process, self._relay, self._files = process, relay, files
        self._client_env, self._knobs = client_env, knobs
        self._outcome: SessionOutcome | None = None

    def __enter__(self) -> CacheSession:
        return self

    def __exit__(self, *exc_info: object) -> None:
        self.close()

    def close(self) -> SessionOutcome:
        """Watchdog, stats, stop, relay, files: in that order, once."""
        if self._outcome is not None:
            return self._outcome
        counters, detached, read_only = (0, 0, 0), 0, False
        try:
            if self.watchdog is not None:
                self.watchdog.stop()
            detached = int(self._process.poll() is not None and answers(self._files.socket))
            if answers(self._files.socket):
                counters = summed_counters(self._client("--show-stats", "--stats-format", "json"))
                self._client("--stop-server")
            reap(self._process, self._knobs)
            read_only = READ_ONLY_LINE in read_text(self._files.err)
        finally:
            if self._relay is not None:
                self._relay.close()
            unlink_quietly(self._files.socket)
            unlink_quietly(self._files.err)
        self._outcome = SessionOutcome(
            self.backend, self._closing_reason(read_only), *counters, detached, self.store_attempts
        )
        return self._outcome

    def _closing_reason(self, read_only: bool) -> Reason:
        if self.watchdog is not None and self.watchdog.lost:
            return Reason.STORE_LOST
        if read_only and self.backend is Backend.SHARED:
            return Reason.READ_ONLY_SERVER
        return self.reason

    def _client(self, *args: str) -> str:
        """The real sccache as a client, with the command's environment, never the server's."""
        if not answers(self._files.socket):
            return ""
        try:
            completed = subprocess.run(
                [self.environment.server.sccache, *args],
                env=self._client_env,
                stdin=subprocess.DEVNULL,
                capture_output=True,
                text=True,
                timeout=self._knobs.socket_wait,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired):
            return ""
        return completed.stdout


def open_relay(settings: CacheSettings) -> Relay | None:
    """The relay to store_url's host, or None when that host does not resolve."""
    store = urllib.parse.urlsplit(settings.store_url)
    try:
        return Relay(store.hostname, store.port or 80)
    except (OSError, UnicodeError):
        return None


def open_session(
    policy: Policy,
    settings: CacheSettings,
    *,
    canon_build_dir: str,
    sccache: str,
    env: Mapping[str, str],
    knobs: SessionKnobs = SessionKnobs(),
    probe: Callable[[], bool] | None = None,
    rng: random.Random | None = None,
    sleep: Callable[[float], None] = time.sleep,
    clock: Callable[[], float] = time.monotonic,
) -> CacheSession:
    """Start the private server for shared or mixed (C4): the store when it holds, else the fallback."""
    if policy is Policy.INCREMENTAL:
        raise ValueError("the incremental policy runs no sccache server")
    if not is_executable_file(sccache):
        raise CacheSessionError(f"cannot start {sccache}: not an executable file")
    rng = rng or random.Random()
    probe = probe or (lambda: probe_store(settings, knobs))
    with refusing_os_errors(f"create the build directory {canon_build_dir}"):
        os.makedirs(canon_build_dir, exist_ok=True)
    remove_stale_session_files(canon_build_dir, os.getpid())
    files = SessionFiles.in_build_dir(canon_build_dir, os.getpid())
    fallback_dir = choose_fallback_dir(settings, knobs.tmp_root)
    backend, reason = choose_backend(settings, probe)
    attempts, relay, process = 0, None, None
    try:
        relay = open_relay(settings) if backend is Backend.SHARED else None
        if backend is Backend.SHARED and relay is None:
            backend, reason = Backend.LOCAL_FALLBACK, Reason.STORE_UNREACHABLE
        if relay is not None:
            relay.start()
            shared_env = server_env(env, files, backend, settings, fallback_dir, relay)
            failure = StartFailure.NONE
            while attempts < knobs.start_attempts:
                if attempts:
                    sleep(rng.uniform(*knobs.retry_delay))
                attempts += 1
                process, failure = start_server(sccache, shared_env, files, knobs, clock)
                if failure is StartFailure.NONE:
                    break
                stop_now(process, knobs)
            if failure is not StartFailure.NONE:
                relay.close()
                relay = None
                backend = Backend.LOCAL_FALLBACK
                read_only = failure is StartFailure.READ_ONLY
                reason = Reason.READ_ONLY_SERVER if read_only else Reason.STORE_UNREACHABLE
        if backend is Backend.LOCAL_FALLBACK:
            local_env = server_env(env, files, backend, settings, fallback_dir, None)
            process, failure = start_server(sccache, local_env, files, knobs, clock)
            if failure is not StartFailure.NONE:
                stop_now(process, knobs)
                first_line = next(iter(read_text(files.err).splitlines()), "no stderr")
                raise CacheSessionError(f"the local-fallback sccache server did not start: {first_line}")
    except BaseException:
        if relay is not None:
            relay.close()
        if process is not None and process.poll() is None:
            stop_now(process, knobs)
        unlink_quietly(files.socket)
        unlink_quietly(files.err)
        raise
    address = ServerAddress(files.socket, sccache, files.uncached_log, fallback_dir, settings.fallback_cap)
    environment = CacheEnvironment(policy, address)
    return CacheSession(
        environment=environment,
        backend=backend,
        reason=reason,
        store_attempts=attempts,
        process=process,
        relay=relay,
        watchdog=None if relay is None else Watchdog(probe, relay, knobs, clock),
        files=files,
        client_env=environment.for_command(env),
        knobs=knobs,
    )


EVENT_LIB = Path(__file__).with_name("lib_event_log.sh")
JOURNAL_NAME = ".reify-canon-journal.jsonl"


def journal_path(build_dir: str | Path) -> str:
    """The invocation journal: every supervised invocation's build_cache record, in its build directory."""
    return os.path.join(build_dir, JOURNAL_NAME)


def is_invocation_file(name: str) -> bool:
    """Whether a basename names a file one supervised invocation, or its private server, writes in a build dir."""
    return name == JOURNAL_NAME or SessionFiles.owner_pid(name) is not None


@dataclass(frozen=True)
class BuildCacheRecord:
    """C6's record of one outermost invocation; mode is canon_run's mode value."""

    mode: str
    policy: Policy
    backend: Backend
    reason: Reason
    hits: int
    misses: int
    write_errors: int
    uncached_machine_flags: int
    detached_server: int
    store_attempts: int
    wall_secs: float
    cpu_secs: float


def write_record(record: BuildCacheRecord, subcommand: str, channel: str | None, journal: str) -> None:
    """One build_cache line to the channel (when given) and the journal, by lib_event_log.sh.

    Never fatal, like the lib itself: a failed write is dropped.
    """
    arguments = [
        str(record.mode),
        record.policy.value,
        record.backend.value,
        record.reason.value,
        *(str(count) for count in (
            record.hits,
            record.misses,
            record.write_errors,
            record.uncached_machine_flags,
            record.detached_server,
            record.store_attempts,
        )),
        f"{record.wall_secs:.3f}",
        f"{record.cpu_secs:.3f}",
        subcommand,
    ]
    env = {"PATH": "/usr/bin:/bin", "REIFY_VERIFY_EVENT_LOG": channel or "", "EVENT_LOG_COPY_TO": journal}
    with contextlib.suppress(OSError, subprocess.SubprocessError):
        subprocess.run(
            ["/bin/bash", "-c", 'source "$0" && event_log_emit_build_cache "$@"', str(EVENT_LIB), *arguments],
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )


class CommandNotFound(Exception):
    """A one-line reason a command could not be found or started."""


@dataclass(frozen=True)
class RecordBase:
    """What the caller knows before the command runs; a session supplies the rest."""

    mode: str
    policy: Policy
    backend: Backend = Backend.NONE
    reason: Reason = Reason.NONE


SUPERVISED_SIGNALS = (signal.SIGTERM, signal.SIGHUP, signal.SIGINT, signal.SIGQUIT)


def forwarded_signals() -> frozenset[int]:
    """The supervised signals this process does not ignore now: the ones the caller hears of."""
    return frozenset(number for number in SUPERVISED_SIGNALS if signal.getsignal(number) != signal.SIG_IGN)


def resolve_command(command: str, env: Mapping[str, str], cwd: str) -> str:
    if "/" not in command:
        found = shutil.which(command, path=env.get("PATH", ""))
    else:
        found = os.path.join(cwd, command)
    if found is None or not is_executable_file(found):
        raise CommandNotFound(f"cannot run {command}: not found or not executable")
    return found


class SignalForwarder:
    """PRD C3's one signal rule, for the supervisor and the namespace parent alike (esc-10418-3).

    Build it while the caller's dispositions are current. Inside `with`, a supervised signal
    the caller ignores stays ignored, and any other one sent to this process goes to the
    target deliver_to names, held until it does; the caller's dispositions return at exit.
    So a signal to this pid ends the command as it would in real-path mode. A signal to the
    whole group reaches the command twice, which one at default dispositions dies of once.
    """

    def __init__(self) -> None:
        self._caller = {number: signal.getsignal(number) for number in SUPERVISED_SIGNALS}
        self.forwarded = forwarded_signals()
        self._send: Callable[[int], None] | None = None
        self._held: list[int] = []

    def __enter__(self) -> SignalForwarder:
        for number in SUPERVISED_SIGNALS:
            signal.signal(number, self._forward if number in self.forwarded else signal.SIG_IGN)
        return self

    def __exit__(self, *exc_info: object) -> None:
        for number, handler in self._caller.items():
            signal.signal(number, handler)

    def deliver_to(self, send: Callable[[int], None]) -> None:
        """Send every signal from now on, and every one held so far, through send."""
        self._send = send
        held, self._held = self._held, []
        for signum in held:
            send(signum)

    def _forward(self, signum: int, _frame: object) -> None:
        if self._send is None:
            self._held.append(signum)
        else:
            self._send(signum)


def exit_status(returncode: int) -> int:
    """A child's status as a shell reports it: 128+N when signal N killed it."""
    return 128 - returncode if returncode < 0 else returncode


def without_core_dumps() -> None:
    """RLIMIT_CORE at 1, not 0: a piped core_pattern dumps whatever the limit, except at exactly 1."""
    hard = resource.getrlimit(resource.RLIMIT_CORE)[1]
    resource.setrlimit(resource.RLIMIT_CORE, (0 if hard == 0 else 1, hard))


def exit_like(returncode: int) -> int:
    """End this process as a command with subprocess returncode ended: by its signal, else its exit_status.

    Only a supervised signal this process does not ignore is re-raised, so a caller that
    ignores it sees 128+N, as from real-path mode. The re-raise dumps no core of its own.
    """
    signum = -returncode
    if signum in forwarded_signals():
        for stream in filter(None, (sys.stdout, sys.stderr)):
            with contextlib.suppress(OSError, ValueError):
                stream.flush()
        without_core_dumps()
        signal.signal(signum, signal.SIG_DFL)
        signal.pthread_sigmask(signal.SIG_UNBLOCK, {signum})
        os.kill(os.getpid(), signum)
    return exit_status(returncode)


def run_command(
    executable: str,
    argv: list[str],
    env: Mapping[str, str],
    cwd: str,
    session: CacheSession | None,
    forwarder: SignalForwarder,
) -> tuple[int, float]:
    """Spawn from this thread and wait, forwarding signals to it; its returncode and the wall time."""
    started = time.monotonic()
    try:
        process = subprocess.Popen(
            with_parent_death_signal([executable, *argv[1:]], "KILL"), env=env, cwd=cwd, close_fds=False
        )
    except OSError as exc:
        raise CommandNotFound(f"cannot run {argv[0]}: {exc}") from exc
    forwarder.deliver_to(process.send_signal)
    if session is not None and session.watchdog is not None:
        session.watchdog.start()
    returncode = process.wait()
    return returncode, time.monotonic() - started


def drain_line_count(path: str) -> int:
    try:
        with open(path, "rb") as log:
            count = sum(1 for _ in log)
    except OSError:
        return 0
    unlink_quietly(path)
    return count


def children_cpu_secs() -> float:
    usage = resource.getrusage(resource.RUSAGE_CHILDREN)
    return usage.ru_utime + usage.ru_stime


def supervise(
    argv: list[str],
    env: Mapping[str, str],
    cwd: str,
    *,
    session: CacheSession | None,
    record_base: RecordBase,
    subcommand: str,
    channel: str | None,
    journal: str,
) -> int:
    """Run argv to completion around session, then close it and write its build_cache record.

    Returns the command's returncode as subprocess gives it, -N when signal N killed it;
    exit_like ends a process the same way. It never ends the calling process. A command that
    cannot be found raises CommandNotFound after the session is closed, with no record.
    Signals are forwarded until the record is written, so a second copy of one that
    already ended the command cannot cut the teardown short.
    """
    cpu_before = children_cpu_secs()
    with SignalForwarder() as forwarder:
        try:
            returncode, wall = run_command(resolve_command(argv[0], env, cwd), argv, env, cwd, session, forwarder)
        finally:
            outcome = None if session is None else session.close()
        cpu = children_cpu_secs() - cpu_before
        uncached = 0 if session is None else drain_line_count(session.environment.server.uncached_log)
        write_record(build_record(record_base, outcome, uncached, wall, cpu), subcommand, channel, journal)
    return returncode


def build_record(
    base: RecordBase, outcome: SessionOutcome | None, uncached: int, wall: float, cpu: float
) -> BuildCacheRecord:
    if outcome is None:
        return BuildCacheRecord(
            base.mode, base.policy, base.backend, base.reason, 0, 0, 0, uncached, 0, 0, wall, cpu
        )
    return BuildCacheRecord(
        base.mode,
        base.policy,
        outcome.backend,
        outcome.reason,
        outcome.hits,
        outcome.misses,
        outcome.write_errors,
        uncached,
        outcome.detached_server,
        outcome.store_attempts,
        wall,
        cpu,
    )
