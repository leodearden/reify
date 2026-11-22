#!/usr/bin/python3 -I
"""Decide a build directory's mode and run a command in it.

The canonical-build-path primitive (dark-factory
plans/canonical-build-path-shared-cache-prd.md C1-C4, C7): ``status`` reports
the mode of the build directory under the current tree, and ``exec -- <cmd>``
and ``cargo <args>`` run a command either untouched at the tree's real path or
at CANON, the one path every stamped build directory is built at.
"""
from __future__ import annotations

import functools
import json
import os
import pwd
import re
import stat
import subprocess
import sys
import tomllib
from collections.abc import Callable, Mapping
from dataclasses import dataclass
from enum import Enum, StrEnum
from pathlib import Path

CANON_DEFAULT = "/opt/reify-canon/ws"
STAMP_NAME = ".reify-build-root"
BWRAP = "/usr/bin/bwrap"
MOUNTINFO = "/proc/self/mountinfo"
EXIT_REFUSED = 126
EXIT_NOT_FOUND = 127
EXIT_USAGE = 2
USAGE = "usage: canon_run.py status | exec -- <command> [args...] | cargo [args...]"
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


class Policy(StrEnum):
    SHARED = "shared"
    MIXED = "mixed"
    INCREMENTAL = "incremental"


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
    policy: Policy


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


@dataclass(frozen=True)
class Tree:
    root: Path
    real_root: Path
    dev: int
    ino: int

    def contains(self, path: str | Path) -> bool:
        return is_inside(path, self.root) or is_inside(path, self.real_root)


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
        return HostConfig(path, False, Policy.SHARED)
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as exc:
        raise Refusal(f"cannot read host config {path}: {exc}") from exc
    enabled = data.get("enabled", False)
    if not isinstance(enabled, bool):
        raise Refusal(f"host config {path}: enabled {enabled!r} is not true or false")
    policy = data.get("policy", Policy.SHARED.value)
    if policy not in tuple(Policy):
        choices = ", ".join(Policy)
        raise Refusal(f"host config {path}: policy {policy!r} is not one of {choices}")
    return HostConfig(path, enabled, Policy(policy))


def is_inside(path: str | Path, root: str | Path) -> bool:
    """Lexical containment at a path boundary; no symlink is resolved."""
    path, root = os.path.normpath(path), os.path.normpath(root)
    return path == root or path.startswith(root.rstrip("/") + "/")


def is_real_dir(path: str | Path) -> bool:
    try:
        return stat.S_ISDIR(os.lstat(path).st_mode)
    except OSError:
        return False


def find_tree(cwd: str, environ: Mapping[str, str]) -> Tree | None:
    """The first directory at or above cwd holding Cargo.lock and scripts/canon_run.py."""
    start = Path(cwd)
    for root in (start, *start.parents):
        if os.path.isfile(root / "Cargo.lock") and os.path.isfile(root / "scripts" / "canon_run.py"):
            root_stat = os.stat(root)
            return Tree(root, root, root_stat.st_dev, root_stat.st_ino)
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


def read_build_dir_state(build_dir: str | Path) -> BuildDirState:
    build = Path(build_dir)
    stamp_path = build / STAMP_NAME
    try:
        stamp_text = stamp_path.read_text(encoding="utf-8", errors="surrogateescape")
    except (FileNotFoundError, NotADirectoryError):
        stamp = None
    except OSError as exc:
        raise Refusal(f"cannot read {stamp_path}: {exc}") from exc
    else:
        stamp = stamp_text.removesuffix("\n")
    built_in = any(
        fingerprint.is_dir()
        for pattern in ("*/.fingerprint", "*/*/.fingerprint")
        for fingerprint in build.glob(pattern)
    )
    return BuildDirState(stamp, built_in)


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
    """C1's table; host_enabled and reachability are called only by the rows that need them."""
    if state.stamp is not None:
        if state.stamp != canon:
            return Decision(Mode.REFUSED, f"stamped for {state.stamp!r}, not for CANON {canon}")
        reach = reachability()
        if reach.route is Route.NONE:
            return Decision(
                Mode.REFUSED, f"stamped for CANON {canon}, which is unreachable: {reach.reason}"
            )
        return Decision(Mode.CANON, f"stamped for CANON {canon}; {reach.reason}")
    if state.built_in:
        return Decision(Mode.REAL_PATH, "unstamped and already built at its real path")
    if not host_enabled():
        return Decision(Mode.REAL_PATH, "fresh, and canonical builds are disabled on this host")
    reach = reachability()
    if reach.route is Route.NONE:
        return Decision(Mode.REAL_PATH, f"fresh, and CANON is unreachable: {reach.reason}")
    return Decision(Mode.CANON, f"fresh on an enabled host; {reach.reason}")


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
    report["build_dir_mode"] = decision.mode.value
    report["reason"] = decision.reason
    return report


def is_trampoline(path: str) -> bool:
    try:
        with open(path, "rb") as candidate:
            return TRAMPOLINE_MARKER in candidate.read(MARKER_WINDOW)
    except OSError:
        return False


def resolve_real_cargo(path_value: str) -> str | None:
    """The first unmarked cargo after the first trampoline on PATH; the first cargo if none is marked."""
    cargos = [
        cargo
        for cargo in (os.path.join(entry, "cargo") for entry in path_value.split(os.pathsep) if entry)
        if os.path.isfile(cargo) and os.access(cargo, os.X_OK)
    ]
    marked = [is_trampoline(cargo) for cargo in cargos]
    start = marked.index(True) + 1 if True in marked else 0
    return next((cargo for cargo, mark in zip(cargos[start:], marked[start:]) if not mark), None)


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


def cargo_passes_through(
    args: list[str], environ: Mapping[str, str], cwd: str, tree_root: str | Path
) -> bool:
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
        target_dir and is_inside(os.path.join(cwd, target_dir), tree_root)
        for target_dir in target_dirs
    )


def flatten(message: object) -> str:
    return " ".join(str(message).splitlines())


def execute(file: str, argv: list[str], env: Mapping[str, str], *, search: bool) -> int:
    try:
        if search:
            os.execvpe(file, argv, env)
        os.execve(file, argv, env)
    except OSError as exc:
        print(f"canon_run: cannot run {file}: {flatten(exc)}", file=sys.stderr)
    return EXIT_NOT_FOUND


def exec_untouched(command: list[str], *, cargo: bool, environ: Mapping[str, str]) -> int:
    """C2's real-path mode: the command with the caller's exact environment and cwd."""
    if not cargo:
        return execute(command[0], command, environ, search=True)
    real_cargo = resolve_real_cargo(environ.get("PATH", ""))
    if real_cargo is None:
        print("canon_run: no cargo on PATH other than reify-canon trampolines", file=sys.stderr)
        return EXIT_NOT_FOUND
    return execute(real_cargo, ["cargo", *command], environ, search=False)


def enter_canon(build_dir: Path) -> int:
    raise Refusal(f"build directory {build_dir} builds at CANON, which this canon_run.py cannot enter yet")


def run(command: list[str], *, cargo: bool, environ: Mapping[str, str]) -> int:
    """`exec` (command is the full command) or `cargo` (command is cargo's arguments)."""
    try:
        cwd = os.getcwd()
    except OSError:
        return exec_untouched(command, cargo=cargo, environ=environ)
    tree = find_tree(cwd, environ)
    if tree is None or (cargo and cargo_passes_through(command, environ, cwd, tree.root)):
        return exec_untouched(command, cargo=cargo, environ=environ)
    target_dir = option_value(command, "--target-dir") if cargo else None
    build_dir = build_dir_for(tree, cwd, environ, target_dir)
    canon = canon_root(environ)
    host = functools.cache(lambda: load_host_config(host_config_path(environ)))
    decision = build_dir_decision(tree, build_dir, canon, lambda: host().enabled)
    if decision.mode is Mode.REAL_PATH:
        return exec_untouched(command, cargo=cargo, environ=environ)
    if decision.mode is Mode.REFUSED:
        raise Refusal(
            f"build directory {build_dir}: {decision.reason}; run where a mount namespace"
            f" can be entered, or delete {build_dir} to rebuild it at the real path"
        )
    return enter_canon(build_dir)


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
    print(USAGE, file=sys.stderr)
    return EXIT_USAGE


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
