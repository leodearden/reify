#!/usr/bin/python3 -I
"""Decide a build directory's mode and run a command in it.

The canonical-build-path primitive (dark-factory
plans/canonical-build-path-shared-cache-prd.md C1-C7): ``status`` reports
the mode of the build directory under the current tree, and ``exec -- <cmd>``
and ``cargo <args>`` run a command either untouched at the tree's real path or
at CANON, the one path every stamped build directory is built at. The outermost
invocation at CANON supervises its command (a mount namespace is entered by
re-running this script inside it); a real-path one on an enabled host is
supervised only to record it. What cache a command at CANON gets, and the
supervision itself (C4-C6), are the sibling module canon_cache.py's business.
"""
from __future__ import annotations

import contextlib
import fcntl
import functools
import importlib.util
import json
import os
import pwd
import re
import select
import signal
import stat
import struct
import subprocess
import sys
import tempfile
import tomllib
from collections.abc import Callable, Mapping
from dataclasses import dataclass
from enum import Enum, StrEnum
from pathlib import Path
from typing import NoReturn

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import canon_cache  # noqa: E402  (imported after sys.path manipulation)

CANON_DEFAULT = "/opt/reify-canon/ws"
STAMP_NAME = ".reify-build-root"
BWRAP = "/usr/bin/bwrap"
ENV = "/usr/bin/env"
MOUNTINFO = "/proc/self/mountinfo"
EXIT_REFUSED = 126
EXIT_NOT_FOUND = 127
EXIT_USAGE = 2
USAGE = "usage: canon_run.py status | exec -- <command> [args...] | cargo [args...]"
REBUILD_FIX = "delete the build directory to rebuild it at the real path"
TRAMPOLINE_MARKER = b"reify-canon-trampoline"
RESTORE_POLL_MS = 5
STARTUP_IGNORED = (signal.SIGPIPE, signal.SIGXFSZ)  # CPython sets both to SIG_IGN at startup
# Linux's PIDFD_GET_INFO (include/uapi/linux/pidfd.h, struct pidfd_info VER0, 6.15+).
PIDFD_GET_INFO = 0xC040FF0B  # _IOWR(0xFF, 11, 64 bytes)
PIDFD_INFO_EXIT = 1 << 3
PIDFD_INFO_SIZE = 64
PIDFD_EXIT_CODE_OFFSET = 60  # s32 wait status
VALUE_TAKING_GLOBALS = frozenset({"--color", "--config", "-Z", "-C"})
PASS_THROUGH = frozenset(
    {
        "metadata",
        "locate-project",
        "pkgid",
        "tree",
        "search",
        "login",
        "logout",
        "new",
        "init",
        "add",
        "remove",
        "update",
        "fetch",
        "vendor",
        "generate-lockfile",
        "help",
        "version",
        "install",
    }
)


class Mode(StrEnum):
    CANON = "canon"
    REAL_PATH = "real_path"
    REFUSED = "refused"


class Route(Enum):
    BOUND = "bound"
    NAMESPACE = "namespace"
    NONE = "none"


class Refusal(Exception):
    """A one-line reason this invocation must not run."""


@dataclass(frozen=True)
class HostConfig:
    path: Path
    enabled: bool
    policy: canon_cache.Policy
    cache: canon_cache.CacheSettings


@dataclass(frozen=True)
class BuildDirState:
    stamp: str | None
    built_in: bool


@dataclass(frozen=True)
class Reachability:
    route: Route
    reason: str


@dataclass(frozen=True)
class Decision:
    """C1's verdict; cache_reason is set on the two real-path rows a build_cache record reports."""

    mode: Mode
    reason: str
    route: Route = Route.NONE
    cache_reason: canon_cache.Reason = canon_cache.Reason.NONE


@dataclass(frozen=True)
class Tree:
    root: Path
    real_root: Path
    dev: int
    ino: int

    def contains(self, path: str | Path) -> bool:
        return is_inside(path, self.root) or is_inside(path, self.real_root)


@dataclass(frozen=True)
class Supervisor:
    """The outermost invocation's mode and, at CANON, the cache every command under it uses."""

    mode: Mode
    cache: canon_cache.CacheEnvironment | None

    def to_record(self) -> dict[str, object]:
        return {"mode": self.mode.value, "cache": None if self.cache is None else self.cache.to_record()}

    @classmethod
    def from_record(cls, record: object) -> Supervisor | None:
        """None unless a canon supervisor carries a cache and a real_path one carries none."""
        if not isinstance(record, dict) or "cache" not in record:
            return None
        if record.get("mode") == Mode.REAL_PATH:
            return cls(Mode.REAL_PATH, None) if record["cache"] is None else None
        if record.get("mode") != Mode.CANON:
            return None
        cache = canon_cache.CacheEnvironment.from_record(record["cache"])
        return None if cache is None else cls(Mode.CANON, cache)


@dataclass(frozen=True)
class ActiveTree:
    """REIFY_CANON_ACTIVE: the tree an outer invocation runs, read back by nested ones.

    supervisor is None from the process that enters a mount namespace, which supervises nothing.
    """

    dev: int
    ino: int
    real_root: str
    canon: str
    supervisor: Supervisor | None = None


@dataclass(frozen=True)
class Invocation:
    argv: tuple[str, ...]
    env: Mapping[str, str]
    cwd: str


def caller_environ() -> dict[str, str]:
    """The environment this process was exec'd with; os.environ gains PEP 538's LC_CTYPE."""
    with open("/proc/self/environ", "rb") as environ_file:
        raw = environ_file.read()
    environ = {}
    for entry in raw.split(b"\0"):
        name, separator, value = entry.partition(b"=")
        if separator and name:
            environ[os.fsdecode(name)] = os.fsdecode(value)
    return environ


def canon_root(environ: Mapping[str, str]) -> str:
    return environ.get("REIFY_CANON_ROOT") or CANON_DEFAULT


def host_config_path(environ: Mapping[str, str]) -> Path:
    override = environ.get("REIFY_CANON_HOST_CONFIG")
    if override:
        return Path(override)
    home = environ.get("HOME") or pwd.getpwuid(os.getuid()).pw_dir
    return Path(home, ".config", "reify-canon", "host.toml")


def load_host_config(path: Path) -> HostConfig:
    try:
        with open(path, "rb") as config_file:
            data = tomllib.load(config_file)
    except FileNotFoundError:
        return HostConfig(path, False, canon_cache.Policy.SHARED, canon_cache.CacheSettings())
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as exc:
        raise Refusal(f"cannot read host config {path}: {exc}") from exc
    enabled = data.get("enabled", False)
    if not isinstance(enabled, bool):
        raise Refusal(f"host config {path}: enabled {enabled!r} is not true or false")
    policy = data.get("policy", canon_cache.Policy.SHARED.value)
    if policy not in tuple(canon_cache.Policy):
        choices = ", ".join(canon_cache.Policy)
        raise Refusal(f"host config {path}: policy {policy!r} is not one of {choices}")
    try:
        cache = canon_cache.parse_cache_settings(data, path)
    except canon_cache.CacheConfigError as exc:
        raise Refusal(str(exc)) from exc
    return HostConfig(path, enabled, canon_cache.Policy(policy), cache)


def cache_refusal(host: HostConfig, search_path: str) -> str:
    """Empty when host's policy can run with the composed search_path, else why not."""
    if host.policy is canon_cache.Policy.INCREMENTAL:
        return ""
    if canon_cache.resolve_real_sccache(search_path) is not None:
        return ""
    return (
        f"host policy '{host.policy}' needs a real sccache, and none is on the CANON PATH;"
        f' install sccache, or set policy = "incremental" or enabled = false in {host.path}'
    )


def is_inside(path: str | Path, root: str | Path) -> bool:
    """Lexical containment at a path boundary; no symlink is resolved."""
    path, root = os.path.normpath(path), os.path.normpath(root)
    return path == root or path.startswith(root.rstrip("/") + "/")


def is_real_dir(path: str | Path) -> bool:
    try:
        return stat.S_ISDIR(os.lstat(path).st_mode)
    except OSError:
        return False


def format_active(tree: Tree, canon: str, supervisor: Supervisor | None = None) -> str:
    record: dict[str, object] = {
        "dev": tree.dev, "ino": tree.ino, "real_root": str(tree.real_root), "canon": canon
    }
    if supervisor is not None:
        record["supervisor"] = supervisor.to_record()
    return json.dumps(record, sort_keys=True)


def parse_active(value: str | None) -> ActiveTree | None:
    try:
        record = json.loads(value or "")
    except ValueError:
        return None
    if not isinstance(record, dict):
        return None
    dev, ino, real_root, canon = (record.get(key) for key in ("dev", "ino", "real_root", "canon"))
    if type(dev) is not int or type(ino) is not int:
        return None
    if not isinstance(real_root, str) or not isinstance(canon, str):
        return None
    return ActiveTree(dev, ino, real_root, canon, Supervisor.from_record(record.get("supervisor")))


def is_nested(active: ActiveTree | None, tree: Tree, decision: Decision) -> bool:
    """Run under a supervisor of this same tree and mode, which already records and caches for it."""
    if active is None or active.supervisor is None:
        return False
    if (active.dev, active.ino) != (tree.dev, tree.ino) or active.supervisor.mode is not decision.mode:
        return False
    return decision.mode is not Mode.CANON or decision.route is Route.BOUND


def find_tree(cwd: str, environ: Mapping[str, str]) -> Tree | None:
    """The first directory at or above cwd holding Cargo.lock and scripts/canon_run.py.

    Its real root is REIFY_CANON_ACTIVE's when that record names this directory.
    """
    start = Path(cwd)
    for root in (start, *start.parents):
        if os.path.isfile(root / "Cargo.lock") and os.path.isfile(root / "scripts" / "canon_run.py"):
            root_stat = os.stat(root)
            active = parse_active(environ.get("REIFY_CANON_ACTIVE"))
            names_root = active is not None and (active.dev, active.ino) == (
                root_stat.st_dev,
                root_stat.st_ino,
            )
            real_root = Path(active.real_root) if names_root else root
            return Tree(root, real_root, root_stat.st_dev, root_stat.st_ino)
    return None


def build_dir_for(
    tree: Tree, cwd: str, environ: Mapping[str, str], target_dir_arg: str | None = None
) -> Path:
    chosen = (
        target_dir_arg
        or environ.get("CARGO_TARGET_DIR")
        or environ.get("CARGO_BUILD_TARGET_DIR")
    )
    if not chosen:
        return tree.root / "target"
    return Path(os.path.normpath(os.path.join(cwd, chosen)))


def read_stamp(build_dir: str | Path) -> str | None:
    stamp_path = Path(build_dir, STAMP_NAME)
    try:
        stamp_text = stamp_path.read_text(encoding="utf-8", errors="surrogateescape")
    except (FileNotFoundError, NotADirectoryError):
        return None
    except OSError as exc:
        raise Refusal(f"cannot read {stamp_path}: {exc}") from exc
    return stamp_text.removesuffix("\n")


def read_build_dir_state(build_dir: str | Path) -> BuildDirState:
    build = Path(build_dir)
    built_in = any(
        fingerprint.is_dir()
        for pattern in ("*/.fingerprint", "*/*/.fingerprint")
        for fingerprint in build.glob(pattern)
    )
    return BuildDirState(read_stamp(build), built_in)


_MOUNTINFO_ESCAPE = re.compile(r"\\([0-7]{3})")


def mount_points(mountinfo_text: str) -> frozenset[str]:
    points = set()
    for line in mountinfo_text.splitlines():
        fields = line.split(" ")
        if len(fields) > 4:
            points.add(_MOUNTINFO_ESCAPE.sub(lambda m: chr(int(m.group(1), 8)), fields[4]))
    return frozenset(points)


def canon_bound_to(canon: str, tree_root: str | Path, mountinfo_text: str) -> bool:
    """C3: CANON is a mount point in this namespace and is the tree's directory."""
    if not is_real_dir(canon) or os.path.normpath(canon) not in mount_points(mountinfo_text):
        return False
    try:
        canon_stat, tree_stat = os.stat(canon), os.stat(tree_root)
    except OSError:
        return False
    return (canon_stat.st_dev, canon_stat.st_ino) == (tree_stat.st_dev, tree_stat.st_ino)


def namespace_refusal(real_root: str | Path, canon: str) -> str:
    """Empty when bwrap can bind real_root at canon in a new mount namespace, else why not."""
    probe_argv = [BWRAP, "--dev-bind", "/", "/", "--bind", str(real_root), canon, "/bin/true"]
    try:
        probe = subprocess.run(
            probe_argv,
            env={"PATH": "/usr/bin:/bin"},
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            check=False,
        )
    except OSError as exc:
        return f"{BWRAP} cannot run: {exc}"
    if probe.returncode == 0:
        return ""
    first_line = next(iter(probe.stderr.strip().splitlines()), "no stderr")
    return f"{BWRAP} cannot enter a mount namespace (rc={probe.returncode}): {first_line}"


def canon_reachability(
    canon: str, tree: Tree, mountinfo_text: str, probe: Callable[[], str]
) -> Reachability:
    if not is_real_dir(canon):
        return Reachability(Route.NONE, f"CANON {canon} is not provisioned")
    if canon_bound_to(canon, tree.root, mountinfo_text):
        return Reachability(Route.BOUND, f"CANON {canon} is already this tree")
    refusal = probe()
    if refusal:
        return Reachability(Route.NONE, refusal)
    return Reachability(Route.NAMESPACE, f"a mount namespace can bind the tree at CANON {canon}")


def read_mountinfo() -> str:
    try:
        return Path(MOUNTINFO).read_text(encoding="utf-8", errors="surrogateescape")
    except OSError:
        return ""


def reachability_of(canon: str, tree: Tree) -> Callable[[], Reachability]:
    def compute() -> Reachability:
        return canon_reachability(
            canon, tree, read_mountinfo(), lambda: namespace_refusal(tree.real_root, canon)
        )

    return compute


def decide_mode(
    state: BuildDirState,
    canon: str,
    host_enabled: Callable[[], bool],
    reachability: Callable[[], Reachability],
) -> Decision:
    """C1's table; host_enabled and reachability are called only by the rows that need them.

    A refusal's reason ends with the fix for that row.
    """
    if state.stamp is not None:
        if state.stamp != canon:
            return Decision(
                Mode.REFUSED, f"stamped for {state.stamp!r}, not for CANON {canon}; {REBUILD_FIX}"
            )
        reach = reachability()
        if reach.route is Route.NONE:
            return Decision(
                Mode.REFUSED,
                f"stamped for CANON {canon}, which is unreachable: {reach.reason};"
                f" run where a mount namespace can be entered, or {REBUILD_FIX}",
            )
        return Decision(Mode.CANON, f"stamped for CANON {canon}; {reach.reason}", reach.route)
    if state.built_in:
        return Decision(
            Mode.REAL_PATH,
            "unstamped and already built at its real path",
            cache_reason=canon_cache.Reason.LEGACY_BUILD_DIR,
        )
    if not host_enabled():
        return Decision(Mode.REAL_PATH, "fresh, and canonical builds are disabled on this host")
    reach = reachability()
    if reach.route is Route.NONE:
        return Decision(
            Mode.REAL_PATH,
            f"fresh, and CANON is unreachable: {reach.reason}",
            cache_reason=canon_cache.Reason.CANON_UNAVAILABLE,
        )
    return Decision(Mode.CANON, f"fresh on an enabled host; {reach.reason}", reach.route)


def build_dir_decision(
    tree: Tree, build_dir: Path, canon: str, host_enabled: Callable[[], bool]
) -> Decision:
    if not tree.contains(build_dir):
        return Decision(Mode.REAL_PATH, f"build directory {build_dir} is outside the tree")
    state = read_build_dir_state(build_dir)
    return decide_mode(state, canon, host_enabled, reachability_of(canon, tree))


def status_report(environ: Mapping[str, str], cwd: str) -> dict[str, object]:
    canon = canon_root(environ)
    host = load_host_config(host_config_path(environ))
    report: dict[str, object] = {
        "canon_root": canon,
        "canon_provisioned": is_real_dir(canon),
        "host_config": str(host.path),
        "host_enabled": host.enabled,
        "policy": host.policy.value,
        "tree_root": None,
        "build_dir": None,
    }
    tree = find_tree(cwd, environ)
    if tree is None:
        decision = Decision(Mode.REAL_PATH, f"{cwd} is inside no tree")
    else:
        build_dir = build_dir_for(tree, cwd, environ)
        report["tree_root"] = str(tree.real_root)
        report["build_dir"] = str(build_dir)
        decision = build_dir_decision(tree, build_dir, canon, lambda: host.enabled)
        search_path = canon_search_path(
            environ.get("PATH", ""), tree, canon, environ.get("HOME"), guard=canon_cache.uses_guard(host.policy)
        )
        refusal = cache_refusal(host, search_path)
        if decision.mode is Mode.CANON and refusal:
            decision = Decision(Mode.REFUSED, refusal)
    report["build_dir_mode"] = decision.mode.value
    report["reason"] = decision.reason
    report["store_reachable"] = canon_cache.store_reachable(host.cache)
    return report


def is_trampoline(path: str) -> bool:
    return canon_cache.carries_marker(path, TRAMPOLINE_MARKER)


def resolve_real_cargo(path_value: str) -> str | None:
    """The caller-order rule: the first unmarked cargo after the first trampoline on PATH.

    When no unmarked cargo follows a trampoline, or nothing is marked, the first
    unmarked cargo anywhere on PATH; else None.
    """
    cargos = [(cargo, is_trampoline(cargo)) for cargo in canon_cache.executables_named(path_value, "cargo")]
    first_trampoline = next((index for index, (_, mark) in enumerate(cargos) if mark), len(cargos))
    after_first = [cargo for cargo, mark in cargos[first_trampoline:] if not mark]
    anywhere = [cargo for cargo, mark in cargos if not mark]
    return next(iter(after_first or anywhere), None)


def resolve_canon_cargo(path_value: str) -> str | None:
    """The first unmarked cargo on path_value.

    C4's composed PATH starts with the tree's own trampoline, so every cargo on it
    follows the first trampoline whether or not that entry is visible from here.
    """
    return next(
        (cargo for cargo in canon_cache.executables_named(path_value, "cargo") if not is_trampoline(cargo)), None
    )


def cargo_subcommand_index(args: list[str]) -> int | None:
    index = 1 if args and args[0].startswith("+") else 0
    while index < len(args):
        arg = args[index]
        if arg == "--explain" or arg.startswith("--explain="):
            return None
        if arg in VALUE_TAKING_GLOBALS:
            index += 2
        elif arg.startswith("-"):
            index += 1
        else:
            return index
    return None


def option_value(args: list[str], name: str) -> str | None:
    """`name V` or `name=V` before any `--`."""
    for index, arg in enumerate(args):
        if arg == "--":
            return None
        if arg == name and index + 1 < len(args):
            return args[index + 1]
        if arg.startswith(name + "="):
            return arg[len(name) + 1 :]
    return None


def cargo_passes_through(args: list[str], environ: Mapping[str, str], cwd: str, tree: Tree) -> bool:
    """Decision 10: cargo invocations that build nothing in the tree run untouched."""
    index = cargo_subcommand_index(args)
    if index is None:
        return True
    if args[index] not in PASS_THROUGH:
        return False
    if args[index] != "install":
        return True
    install_args = args[index + 1 :]
    if any(arg == "--path" or arg.startswith("--path=") for arg in install_args):
        return False
    target_dirs = (
        option_value(install_args, "--target-dir"),
        environ.get("CARGO_TARGET_DIR"),
        environ.get("CARGO_BUILD_TARGET_DIR"),
    )
    return not any(
        target_dir and tree.contains(os.path.join(cwd, target_dir)) for target_dir in target_dirs
    )


def rebase(value: str, tree: Tree, canon: str) -> str:
    """An absolute path in the tree, under either spelling, re-rooted at canon; else value."""
    if not os.path.isabs(value):
        return value
    for root in (tree.real_root, tree.root):
        if is_inside(value, root):
            relative = os.path.relpath(os.path.normpath(value), os.path.normpath(root))
            return canon if relative == "." else os.path.join(canon, relative)
    return value


def rebase_argument(argument: str, tree: Tree, canon: str) -> str:
    if argument.startswith("-") and "=" in argument:
        name, _, value = argument.partition("=")
        return f"{name}={rebase(value, tree, canon)}"
    return rebase(argument, tree, canon)


def canon_search_path(
    path_value: str, tree: Tree, canon: str, home: str | None, *, guard: bool
) -> str:
    """C4: CANON's guard (with sccache), its trampoline, $HOME/.cargo/bin, then the rest.

    Every spelling of the tree's two directories leaves the rest, so re-composing is idempotent.
    """
    front = []
    if guard and os.path.isfile(tree.root / "scripts" / "canon-bin" / "guard" / "sccache"):
        front.append(os.path.join(canon, "scripts", "canon-bin", "guard"))
    if os.path.isfile(tree.root / "scripts" / "canon-bin" / "cargo"):
        front.append(os.path.join(canon, "scripts", "canon-bin"))
    if home:
        front.append(os.path.join(home, ".cargo", "bin"))
    tree_bins = [
        os.path.join(spelling, "scripts", "canon-bin", *guard_part)
        for spelling in (tree.real_root, tree.root, canon)
        for guard_part in ((), ("guard",))
    ]
    dropped = {os.path.normpath(entry) for entry in (*front, *tree_bins)}
    rest = [
        entry
        for entry in (path_value.split(os.pathsep) if path_value else [])
        if not entry or os.path.normpath(entry) not in dropped
    ]
    return os.pathsep.join([*front, *rest])


@functools.cache
def grammar_cache_dir(real_root: Path) -> str | None:
    """REIFY_TS_CACHE_HOME for real_root, derived by that tree's own prd-capability-check.py."""
    pcc_path = Path(real_root, "scripts", "prd-capability-check.py")
    if not pcc_path.is_file():
        return None
    try:
        spec = importlib.util.spec_from_file_location("prd_capability_check", pcc_path)
        pcc = importlib.util.module_from_spec(spec)
        sys.modules["prd_capability_check"] = pcc
        spec.loader.exec_module(pcc)
        context = pcc.ProbeContext(
            repo_root=str(real_root),
            reify_bin=None,
            reify_bin_refusal="not used: canon_run derives the grammar cache dir only",
            tree_sitter_bin="tree-sitter",
        )
        return pcc.grammar_cache_home(context)
    except Exception as exc:  # an optional export must never block a build
        warning = f"canon_run: warning: REIFY_TS_CACHE_HOME not exported: {flatten(exc)}"
        print(warning, file=sys.stderr)
        return None


def canon_invocation(
    command: list[str],
    environ: Mapping[str, str],
    tree: Tree,
    canon: str,
    cwd: str,
    cache: canon_cache.CacheEnvironment,
    *,
    cargo: bool,
) -> Invocation:
    """What the command gets at CANON (C3, C4); for cargo, command is cargo's arguments."""
    argv = [rebase_argument(argument, tree, canon) for argument in command]
    env = dict(environ)
    for name, value in environ.items():
        if name.startswith("CARGO_"):
            env[name] = rebase(value, tree, canon)
    env["PWD"] = rebase(cwd, tree, canon)
    env["PATH"] = canon_search_path(
        environ.get("PATH", ""), tree, canon, environ.get("HOME"), guard=cache.uses_guard
    )
    env["REIFY_CANON_ACTIVE"] = format_active(tree, canon, Supervisor(Mode.CANON, cache))
    env["REIFY_VERIFY_LEDGER_LANE_DIR"] = str(tree.real_root)
    cache_home = grammar_cache_dir(tree.real_root)
    if cache_home is None:
        env.pop("REIFY_TS_CACHE_HOME", None)
    else:
        env["REIFY_TS_CACHE_HOME"] = cache_home
    env = cache.for_cargo(env) if cargo else cache.for_command(env)
    if cargo:
        argv = [canon_cargo(env["PATH"]), *argv]
    return Invocation(tuple(argv), env, env["PWD"])


def canon_cargo(path_value: str) -> str:
    """The real cargo on the composed CANON PATH; CommandNotFound when there is none."""
    real_cargo = resolve_canon_cargo(path_value)
    if real_cargo is None:
        raise canon_cache.CommandNotFound("no cargo on the CANON PATH other than reify-canon trampolines")
    return real_cargo


def flatten(message: object) -> str:
    return " ".join(str(message).splitlines())


def execute(file: str, argv: list[str], env: Mapping[str, str], *, search: bool) -> NoReturn:
    """Become the command, with the signals CPython ignores at default, as Popen's restore_signals gives."""
    for number in STARTUP_IGNORED:
        signal.signal(number, signal.SIG_DFL)
    exec_call = os.execvpe if search else os.execve
    try:
        exec_call(file, argv, env)
    except OSError as exc:
        raise canon_cache.CommandNotFound(f"cannot run {file}: {exc}") from exc


def untouched_argv(command: list[str], *, cargo: bool, environ: Mapping[str, str]) -> list[str]:
    """The command as given; for cargo, the real cargo (by the caller-order rule) and its arguments."""
    if not cargo:
        return list(command)
    real_cargo = resolve_real_cargo(environ.get("PATH", ""))
    if real_cargo is None:
        raise canon_cache.CommandNotFound("no cargo on PATH other than reify-canon trampolines")
    return [real_cargo, *command]


def exec_untouched(command: list[str], *, cargo: bool, environ: Mapping[str, str]) -> NoReturn:
    """C2's real-path mode: the command with the caller's exact environment and cwd."""
    argv = untouched_argv(command, cargo=cargo, environ=environ)
    if not cargo:
        return execute(argv[0], argv, environ, search=True)
    return execute(argv[0], ["cargo", *argv[1:]], environ, search=False)


def ensure_stamp(build_dir: Path, canon: str) -> None:
    """C1: mark build_dir as built at canon; a stamp already there must name canon.

    A build directory already stamped for canon is left untouched.
    """
    stamp_path = build_dir / STAMP_NAME
    if read_stamp(build_dir) == canon:
        return
    try:
        os.makedirs(build_dir, exist_ok=True)
        descriptor, pending = tempfile.mkstemp(dir=build_dir, prefix=f"{STAMP_NAME}.")
    except OSError as exc:
        raise Refusal(f"cannot stamp {stamp_path}: {exc}") from exc
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as pending_file:
            pending_file.write(canon + "\n")
        os.chmod(pending, 0o644)
        os.link(pending, stamp_path)
    except FileExistsError:
        existing = read_stamp(build_dir)
        if existing != canon:
            raise Refusal(f"{stamp_path} holds {existing!r}, not CANON {canon}") from None
    except OSError as exc:
        raise Refusal(f"cannot stamp {stamp_path}: {exc}") from exc
    finally:
        os.unlink(pending)


@dataclass(frozen=True)
class Call:
    """One `exec` or `cargo` invocation inside a tree; command is cargo's arguments for cargo."""

    command: tuple[str, ...]
    cargo: bool
    environ: Mapping[str, str]
    tree: Tree
    canon: str
    cwd: str
    build_dir: Path

    @property
    def arguments(self) -> list[str]:
        """This invocation's own command line after the script name."""
        return ["cargo", *self.command] if self.cargo else ["exec", "--", *self.command]

    @property
    def subcommand(self) -> str:
        """C6's subcommand: cargo's, or empty for exec."""
        index = cargo_subcommand_index(list(self.command)) if self.cargo else None
        return "" if index is None else self.command[index]

    @property
    def channel(self) -> str | None:
        return self.environ.get("REIFY_VERIFY_EVENT_LOG") or None

    def at_canon(self, cache: canon_cache.CacheEnvironment) -> Invocation:
        return canon_invocation(
            list(self.command), self.environ, self.tree, self.canon, self.cwd, cache, cargo=self.cargo
        )


def reentry_bwrap(tree: Tree, canon: str, cwd: str, info_fd: int) -> list[str]:
    """bwrap binding the tree's real root at canon, writing the re-run's pid to info_fd."""
    return [
        BWRAP, "--dev-bind", "/", "/", "--bind", str(tree.real_root), canon,
        "--chdir", rebase(cwd, tree, canon), "--die-with-parent", "--info-fd", str(info_fd),
    ]


def reentry_command(canon: str, arguments: list[str]) -> list[str]:
    """This same script, re-run at canon with this invocation's arguments."""
    return [sys.executable, "-I", os.path.join(canon, "scripts", "canon_run.py"), *arguments]


def reentry_env(environ: Mapping[str, str], tree: Tree, canon: str) -> dict[str, str]:
    """The caller's environment plus the tree record, so the re-run finds the real root."""
    return dict(environ, REIFY_CANON_ACTIVE=format_active(tree, canon))


def namespace_argv(bwrap: list[str], command: list[str], restored: frozenset[int]) -> list[str]:
    """bwrap running command through env, which gives the restored signals back their defaults."""
    if not restored:
        return [*bwrap, "--", *command]
    names = ",".join(signal.Signals(number).name.removeprefix("SIG") for number in sorted(restored))
    return [*bwrap, "--", ENV, f"--default-signal={names}", "--", *command]


def read_child_pid(info_read: int) -> int | None:
    """bwrap's --info-fd record: the re-run's pid (no pid namespace, so a host pid)."""
    with os.fdopen(info_read, "rb") as info:
        text = info.read()
    try:
        pid = json.loads(text).get("child-pid")
    except (ValueError, AttributeError):
        return None
    return pid if type(pid) is int else None


def ignored_mask(pid: int) -> int:
    """The SigIgn mask in /proc/<pid>/status; 0 when it cannot be read."""
    try:
        with open(f"/proc/{pid}/status") as status:
            for line in status:
                name, _, value = line.partition(":")
                if name == "SigIgn":
                    return int(value, 16)
    except (OSError, ValueError):
        pass
    return 0


def await_restored(pidfd: int, pid: int, numbers: frozenset[int]) -> None:
    """Until pid ignores none of numbers, or pidfd reports it exited.

    The status is read before the pidfd is polled, so a stranger reusing pid can only end
    the wait early, and a signal then sent through the exited pidfd reaches no one.
    """
    wanted = sum(1 << (number - 1) for number in numbers)
    exited = select.poll()
    exited.register(pidfd, select.POLLIN)
    while ignored_mask(pid) & wanted:
        if exited.poll(RESTORE_POLL_MS):
            return


def open_restored_child(info_read: int, restored: frozenset[int]) -> int | None:
    """A pidfd for the child bwrap reports, once that child no longer ignores restored; None if there is none.

    Without a pidfd no signal could reach the child, so a pidfd the kernel will not open is a refusal.
    """
    pid = read_child_pid(info_read)
    if pid is None:
        return None
    try:
        pidfd = os.pidfd_open(pid)
    except ProcessLookupError:
        return None
    except OSError as exc:
        raise Refusal(f"cannot open a pidfd for the namespace's command (pid {pid}): {exc}") from exc
    await_restored(pidfd, pid, restored)
    return pidfd


def send_through(pidfd: int, signum: int) -> None:
    with contextlib.suppress(ProcessLookupError):
        signal.pidfd_send_signal(pidfd, signum)


def exited_returncode(pidfd: int) -> int | None:
    """The returncode, in subprocess's terms, of the reaped process pidfd names.

    None while it runs, or on a kernel that keeps no exit record (before Linux 6.15).
    """
    info = bytearray(PIDFD_INFO_SIZE)
    struct.pack_into("Q", info, 0, PIDFD_INFO_EXIT)
    try:
        fcntl.ioctl(pidfd, PIDFD_GET_INFO, info)
    except OSError:
        return None
    if not struct.unpack_from("Q", info, 0)[0] & PIDFD_INFO_EXIT:
        return None
    try:
        return os.waitstatus_to_exitcode(struct.unpack_from("i", info, PIDFD_EXIT_CODE_OFFSET)[0])
    except ValueError:
        return None


def ignore(numbers: frozenset[int]) -> None:
    for number in numbers:
        signal.signal(number, signal.SIG_IGN)


def run_in_namespace(
    bwrap_for: Callable[[int], list[str]],
    command: list[str],
    env: Mapping[str, str],
    forwarder: canon_cache.SignalForwarder,
) -> int:
    """Spawn bwrap around command ignoring the forwarded signals, then forward under the supervisor's rule.

    bwrap handles no signal, and --die-with-parent SIGKILLs the re-run when bwrap dies, which
    would lose the re-run's teardown and record. So bwrap starts with the forwarded signals
    ignored, env inside the namespace gives them back their defaults before command starts,
    and this process stands in for the caller, forwarding what is sent to it to command, whose
    pid bwrap writes to the info fd. bwrap writes that pid before its child sets up, so signals
    stay held until the child has restored them, and go through a pidfd, never a bare pid.
    bwrap reports its child's death by signal N as exit 128+N, so this returns bwrap's
    returncode, except -N where that pidfd's exit record agrees the child died of N.
    """
    info_read, info_write = os.pipe()
    os.set_inheritable(info_write, True)
    with contextlib.ExitStack() as after_forwarding, forwarder:
        try:
            argv = namespace_argv(bwrap_for(info_write), command, forwarder.forwarded)
            bwrap = subprocess.Popen(
                canon_cache.with_parent_death_signal(argv, "KILL"),
                env=env,
                close_fds=False,
                preexec_fn=functools.partial(ignore, forwarder.forwarded),
            )
        except OSError as exc:
            os.close(info_read)
            raise canon_cache.CommandNotFound(f"cannot run {BWRAP}: {exc}") from exc
        finally:
            os.close(info_write)
        pidfd = open_restored_child(info_read, forwarder.forwarded)
        if pidfd is not None:
            after_forwarding.callback(os.close, pidfd)
            forwarder.deliver_to(functools.partial(send_through, pidfd))
        status = bwrap.wait()
        if pidfd is not None and status > 128 and exited_returncode(pidfd) == 128 - status:
            return 128 - status
        return status


def validated_canon_path(call: Call, host: HostConfig) -> str:
    """The composed CANON PATH, once the command and, with sccache, a real sccache resolve on it."""
    search_path = canon_search_path(
        call.environ.get("PATH", ""), call.tree, call.canon, call.environ.get("HOME"),
        guard=canon_cache.uses_guard(host.policy),
    )
    if call.cargo:
        canon_cargo(search_path)
    refusal = cache_refusal(host, search_path)
    if refusal:
        raise Refusal(refusal)
    if not call.cargo:
        canon_cache.resolve_command(call.command[0], call.environ, call.cwd)
    return search_path


def enter_namespace(call: Call, host: HostConfig) -> int:
    """The NAMESPACE route: validate, stamp, then re-run this invocation where CANON is bound."""
    validated_canon_path(call, host)
    ensure_stamp(call.build_dir, call.canon)
    return run_in_namespace(
        lambda info_fd: reentry_bwrap(call.tree, call.canon, call.cwd, info_fd),
        reentry_command(call.canon, call.arguments),
        reentry_env(call.environ, call.tree, call.canon),
        canon_cache.SignalForwarder(),
    )


def exec_nested(call: Call, cache: canon_cache.CacheEnvironment) -> NoReturn:
    """Under a CANON supervisor of this tree: stamp, then its cache, no session and no record of our own."""
    ensure_stamp(call.build_dir, call.canon)
    invocation = call.at_canon(cache)
    try:
        os.chdir(invocation.cwd)
    except OSError as exc:
        raise Refusal(f"cannot enter {invocation.cwd}: {exc}") from exc
    if call.cargo:
        execute(invocation.argv[0], ["cargo", *invocation.argv[1:]], invocation.env, search=False)
    execute(invocation.argv[0], list(invocation.argv), invocation.env, search=True)


def open_canon_session(call: Call, host: HostConfig, search_path: str) -> canon_cache.CacheSession | None:
    """The private server for shared or mixed; None under incremental."""
    if host.policy is canon_cache.Policy.INCREMENTAL:
        return None
    try:
        return canon_cache.open_session(
            host.policy,
            host.cache,
            canon_build_dir=os.path.join(call.canon, "target"),
            sccache=canon_cache.resolve_real_sccache(search_path),
            env=call.environ,
        )
    except canon_cache.CacheSessionError as exc:
        raise Refusal(str(exc)) from exc


def supervise_at_canon(call: Call, host: HostConfig) -> int:
    """The BOUND route: the command at CANON under the host policy, measured and recorded."""
    search_path = validated_canon_path(call, host)
    ensure_stamp(call.build_dir, call.canon)
    session = open_canon_session(call, host, search_path)
    try:
        cache = canon_cache.CacheEnvironment(host.policy, None) if session is None else session.environment
        invocation = call.at_canon(cache)
    except BaseException:
        if session is not None:
            session.close()
        raise
    return canon_cache.supervise(
        list(invocation.argv),
        invocation.env,
        invocation.cwd,
        session=session,
        record_base=canon_cache.RecordBase(canon_cache.RecordMode.CANON, host.policy),
        subcommand=call.subcommand,
        channel=call.channel,
        journal=canon_cache.journal_path(rebase(str(call.build_dir), call.tree, call.canon)),
    )


def supervise_real_path(call: Call, host: HostConfig, reason: canon_cache.Reason) -> int:
    """C6's real_path record: the untouched command, marked so nested invocations stay untouched."""
    argv = untouched_argv(list(call.command), cargo=call.cargo, environ=call.environ)
    supervisor = Supervisor(Mode.REAL_PATH, None)
    env = dict(call.environ, REIFY_CANON_ACTIVE=format_active(call.tree, call.canon, supervisor))
    with contextlib.suppress(OSError):
        os.makedirs(call.build_dir, exist_ok=True)
    return canon_cache.supervise(
        argv,
        env,
        call.cwd,
        session=None,
        record_base=canon_cache.RecordBase(
            canon_cache.RecordMode.REAL_PATH, host.policy, canon_cache.Backend.NONE, reason
        ),
        subcommand=call.subcommand,
        channel=call.channel,
        journal=canon_cache.journal_path(call.build_dir),
    )


def run(command: list[str], *, cargo: bool, environ: Mapping[str, str]) -> int:
    """`exec` (command is the full command) or `cargo` (command is cargo's arguments).

    Returns the command's returncode as subprocess gives it; the exec paths never return.
    """
    active = parse_active(environ.get("REIFY_CANON_ACTIVE"))
    try:
        cwd = os.getcwd()
    except OSError:
        return exec_untouched(command, cargo=cargo, environ=environ)
    tree = find_tree(cwd, environ)
    if tree is None or (cargo and cargo_passes_through(command, environ, cwd, tree)):
        return exec_untouched(command, cargo=cargo, environ=environ)
    target_dir = option_value(command, "--target-dir") if cargo else None
    build_dir = build_dir_for(tree, cwd, environ, target_dir)
    canon = canon_root(environ)
    host = functools.cache(lambda: load_host_config(host_config_path(environ)))
    decision = build_dir_decision(tree, build_dir, canon, lambda: host().enabled)
    call = Call(tuple(command), cargo, environ, tree, canon, cwd, build_dir)
    if is_nested(active, tree, decision):
        if decision.mode is Mode.REAL_PATH:
            return exec_untouched(command, cargo=cargo, environ=environ)
        return exec_nested(call, active.supervisor.cache)
    if decision.mode is Mode.REFUSED:
        raise Refusal(f"build directory {build_dir}: {decision.reason}")
    if decision.mode is Mode.REAL_PATH:
        if decision.cache_reason and host().enabled:
            return supervise_real_path(call, host(), decision.cache_reason)
        return exec_untouched(command, cargo=cargo, environ=environ)
    if decision.route is Route.NAMESPACE:
        return enter_namespace(call, host())
    return supervise_at_canon(call, host())


def main(argv: list[str]) -> int:
    environ = caller_environ()
    try:
        match argv:
            case ["status"]:
                print(json.dumps(status_report(environ, os.getcwd())))
                return 0
            case ["exec", "--", *command] if command:
                return canon_cache.exit_like(run(command, cargo=False, environ=environ))
            case ["cargo", *args]:
                return canon_cache.exit_like(run(args, cargo=True, environ=environ))
    except Refusal as refusal:
        print(f"canon_run: {flatten(refusal)}", file=sys.stderr)
        return EXIT_REFUSED
    except canon_cache.CommandNotFound as not_found:
        print(f"canon_run: {flatten(not_found)}", file=sys.stderr)
        return EXIT_NOT_FOUND
    print(USAGE, file=sys.stderr)
    return EXIT_USAGE


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
