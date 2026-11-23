"""The cache half of the canonical-build-path primitive.

scripts/canon_run.py decides where a command runs; this module decides what
cache it runs with (dark-factory plans/canonical-build-path-shared-cache-prd.md
C4-C6): the per-policy cargo and command environment and the CARGO_HOME pin,
the host.toml store and fallback settings, and which real sccache serves them.
It knows nothing of trees, build-directory modes or mount namespaces, and
imports nothing from canon_run.
"""
from __future__ import annotations

import os
import tempfile
import urllib.parse
from collections.abc import Mapping
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
