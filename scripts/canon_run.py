#!/usr/bin/python3 -I
"""Decide a build directory's mode and run a command in it.

The canonical-build-path primitive (dark-factory
plans/canonical-build-path-shared-cache-prd.md C1-C7): ``status`` reports
the mode of the build directory under the current tree, and ``exec -- <cmd>``
and ``cargo <args>`` run a command either untouched at the tree's real path or
at CANON, the one path every stamped build directory is built at. What cache a
command at CANON gets (C4-C6) is the sibling module canon_cache.py's business.
"""
from __future__ import annotations

import functools
import importlib.util
import json
import os
import pwd
import re
import stat
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
MOUNTINFO = "/proc/self/mountinfo"
EXIT_REFUSED = 126
EXIT_NOT_FOUND = 127
EXIT_USAGE = 2
USAGE = "usage: canon_run.py status | exec -- <command> [args...] | cargo [args...]"
REBUILD_FIX = "delete the build directory to rebuild it at the real path"
TRAMPOLINE_MARKER = b"reify-canon-trampoline"
MARKER_WINDOW = 1024
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


class CommandNotFound(Exception):
    """A one-line reason the command to run could not be found or started."""


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
    mode: Mode
    reason: str
    route: Route = Route.NONE


@dataclass(frozen=True)
class Tree:
    root: Path
    real_root: Path
    dev: int
    ino: int

    def contains(self, path: str | Path) -> bool:
        return is_inside(path, self.root) or is_inside(path, self.real_root)


@dataclass(frozen=True)
class ActiveTree:
    """REIFY_CANON_ACTIVE: the tree a CANON invocation bound, read back by nested ones."""

    dev: int
    ino: int
    real_root: str
    canon: str


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


def format_active(tree: Tree, canon: str) -> str:
    record = {"dev": tree.dev, "ino": tree.ino, "real_root": str(tree.real_root), "canon": canon}
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
    return ActiveTree(dev, ino, real_root, canon)


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
        return Decision(Mode.REAL_PATH, "unstamped and already built at its real path")
    if not host_enabled():
        return Decision(Mode.REAL_PATH, "fresh, and canonical builds are disabled on this host")
    reach = reachability()
    if reach.route is Route.NONE:
        return Decision(Mode.REAL_PATH, f"fresh, and CANON is unreachable: {reach.reason}")
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
    uses_guard = canon_cache.CacheEnvironment(host.policy, None).uses_guard
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
            environ.get("PATH", ""), tree, canon, environ.get("HOME"), guard=uses_guard
        )
        refusal = cache_refusal(host, search_path)
        if decision.mode is Mode.CANON and refusal:
            decision = Decision(Mode.REFUSED, refusal)
    report["build_dir_mode"] = decision.mode.value
    report["reason"] = decision.reason
    report["store_reachable"] = (
        None
        if host.cache.store_url is None
        else canon_cache.probe_store(host.cache, canon_cache.SessionKnobs())
    )
    return report


def is_trampoline(path: str) -> bool:
    try:
        with open(path, "rb") as candidate:
            return TRAMPOLINE_MARKER in candidate.read(MARKER_WINDOW)
    except OSError:
        return False


def executable_cargos(path_value: str) -> list[str]:
    """Every `<entry>/cargo` on path_value that is an executable regular file, in PATH order."""
    candidates = (os.path.join(entry, "cargo") for entry in path_value.split(os.pathsep) if entry)
    return [cargo for cargo in candidates if os.path.isfile(cargo) and os.access(cargo, os.X_OK)]


def resolve_real_cargo(path_value: str) -> str | None:
    """The caller-order rule: the first unmarked cargo after the first trampoline on PATH.

    When no unmarked cargo follows a trampoline, or nothing is marked, the first
    unmarked cargo anywhere on PATH; else None.
    """
    cargos = [(cargo, is_trampoline(cargo)) for cargo in executable_cargos(path_value)]
    first_trampoline = next((index for index, (_, mark) in enumerate(cargos) if mark), len(cargos))
    after_first = [cargo for cargo, mark in cargos[first_trampoline:] if not mark]
    anywhere = [cargo for cargo, mark in cargos if not mark]
    return next(iter(after_first or anywhere), None)


def resolve_canon_cargo(path_value: str) -> str | None:
    """The first unmarked cargo on path_value.

    C4's composed PATH starts with the tree's own trampoline, so every cargo on it
    follows the first trampoline whether or not that entry is visible from here.
    """
    return next((cargo for cargo in executable_cargos(path_value) if not is_trampoline(cargo)), None)


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
    env["REIFY_CANON_ACTIVE"] = format_active(tree, canon)
    env["REIFY_VERIFY_LEDGER_LANE_DIR"] = str(tree.real_root)
    cache_home = grammar_cache_dir(tree.real_root)
    if cache_home is None:
        env.pop("REIFY_TS_CACHE_HOME", None)
    else:
        env["REIFY_TS_CACHE_HOME"] = cache_home
    env = cache.for_cargo(env) if cargo else cache.for_command(env)
    if cargo:
        real_cargo = resolve_canon_cargo(env["PATH"])
        if real_cargo is None:
            raise CommandNotFound("no cargo on the CANON PATH other than reify-canon trampolines")
        argv = [real_cargo, *argv]
    return Invocation(tuple(argv), env, env["PWD"])


def flatten(message: object) -> str:
    return " ".join(str(message).splitlines())


def execute(file: str, argv: list[str], env: Mapping[str, str], *, search: bool) -> NoReturn:
    exec_call = os.execvpe if search else os.execve
    try:
        exec_call(file, argv, env)
    except OSError as exc:
        raise CommandNotFound(f"cannot run {file}: {exc}") from exc


def exec_untouched(command: list[str], *, cargo: bool, environ: Mapping[str, str]) -> NoReturn:
    """C2's real-path mode: the command with the caller's exact environment and cwd."""
    if not cargo:
        return execute(command[0], command, environ, search=True)
    real_cargo = resolve_real_cargo(environ.get("PATH", ""))
    if real_cargo is None:
        raise CommandNotFound("no cargo on PATH other than reify-canon trampolines")
    return execute(real_cargo, ["cargo", *command], environ, search=False)


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


def enter_canon(
    command: list[str],
    *,
    cargo: bool,
    environ: Mapping[str, str],
    tree: Tree,
    canon: str,
    cwd: str,
    build_dir: Path,
    host: HostConfig,
    route: Route,
) -> NoReturn:
    """Run the command at CANON: directly when CANON is already the tree, else under bwrap."""
    cache = canon_cache.CacheEnvironment(host.policy, None)
    invocation = canon_invocation(command, environ, tree, canon, cwd, cache, cargo=cargo)
    refusal = cache_refusal(host, invocation.env["PATH"])
    if refusal:
        raise Refusal(refusal)
    ensure_stamp(build_dir, canon)
    if route is Route.NAMESPACE:
        file, search = BWRAP, False
        argv = [
            "bwrap", "--dev-bind", "/", "/", "--bind", str(tree.real_root), canon,
            "--chdir", invocation.cwd, "--die-with-parent", "--", *invocation.argv,
        ]
    else:
        try:
            os.chdir(invocation.cwd)
        except OSError as exc:
            raise Refusal(f"cannot enter {invocation.cwd}: {exc}") from exc
        file, search = invocation.argv[0], not cargo
        argv = ["cargo", *invocation.argv[1:]] if cargo else list(invocation.argv)
    execute(file, argv, invocation.env, search=search)


def run(command: list[str], *, cargo: bool, environ: Mapping[str, str]) -> NoReturn:
    """`exec` (command is the full command) or `cargo` (command is cargo's arguments)."""
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
    if decision.mode is Mode.REAL_PATH:
        return exec_untouched(command, cargo=cargo, environ=environ)
    if decision.mode is Mode.REFUSED:
        raise Refusal(f"build directory {build_dir}: {decision.reason}")
    enter_canon(
        command,
        cargo=cargo,
        environ=environ,
        tree=tree,
        canon=canon,
        cwd=cwd,
        build_dir=build_dir,
        host=host(),
        route=decision.route,
    )


def main(argv: list[str]) -> int:
    environ = caller_environ()
    try:
        match argv:
            case ["status"]:
                print(json.dumps(status_report(environ, os.getcwd())))
                return 0
            case ["exec", "--", *command] if command:
                return run(command, cargo=False, environ=environ)
            case ["cargo", *args]:
                return run(args, cargo=True, environ=environ)
    except Refusal as refusal:
        print(f"canon_run: {flatten(refusal)}", file=sys.stderr)
        return EXIT_REFUSED
    except CommandNotFound as not_found:
        print(f"canon_run: {flatten(not_found)}", file=sys.stderr)
        return EXIT_NOT_FOUND
    print(USAGE, file=sys.stderr)
    return EXIT_USAGE


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
