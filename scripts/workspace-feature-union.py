#!/usr/bin/env python3
"""
workspace-feature-union.py — keep `cargo <cmd> -p <member>` resolving every
external package to the feature set the base's `--workspace` build compiles.

Under resolver 2 cargo unifies features over the packages a command SELECTS,
so `-p <member>` can resolve an external package (serde, syn, tokio, ...) to a
feature set the warm base's `--workspace` build never compiled, and the lane
rebuilds it cold. The workspace-feature-union crate pins, per side, the
features `--workspace` resolves; every member depends on it, so any `-p`
selection resolves the same variants.

The rule, checked by `cargo tree` resolution alone (no compile):
  - every workspace member that is not a build-dependency of another member
    depends on workspace-feature-union (a normal dependency);
  - a member that IS a build-dependency of another member does not: the
    union's target pins would enter every build-script graph it is part of;
  - for every covered member M, each external (name, version, side, features)
    variant of `cargo tree -p M` also occurs in `cargo tree --workspace`.
    A side is HOST for build-dependencies, proc-macros and everything below
    them, TARGET otherwise; resolver 2 keeps one feature set per side.

Reading a failure:
    UNCOVERED <m>: ...          add `workspace-feature-union.workspace = true`
                                under [dependencies] in the named manifest.
    HOST-MEMBER <m>: ...        <m> is a build-dependency of the named members;
                                remove its workspace-feature-union dependency.
    SPLIT <m> <name> v<ver> (<side>): -p resolves {..}; --workspace resolves {..}
                                `-p <m>` compiles a variant the base never
                                builds. Run
                                `python3 scripts/workspace-feature-union.py generate`,
                                then regenerate crates/reify-eval/engine_hash_closure.txt
                                with the command in its header.

`check` resolves with `--frozen`: it never writes Cargo.lock and never touches
the network. `generate` rewrites the marked region of
crates/workspace-feature-union/Cargo.toml to a fixpoint and resolves with
`--offline`, so the rewrite may update Cargo.lock.

The gate runs from crates/reify-build-utils/tests/workspace_feature_union.rs
(`cargo test -p reify-build-utils --test workspace_feature_union`). Design,
limits and the probe procedure: docs/notes/workspace-feature-union.md.

Exit codes: 0 every member resolves like the workspace; 1 a finding above;
2 an input (cargo metadata, cargo tree, the union manifest) could not be
trusted.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shlex
import subprocess
import sys
from collections.abc import Sequence
from dataclasses import dataclass, replace
from enum import Enum, IntEnum
from pathlib import Path

UNION_CRATE = "workspace-feature-union"
GENERATE_REMEDY = ("remedy: run `python3 scripts/workspace-feature-union.py generate`, "
                   "then regenerate crates/reify-eval/engine_hash_closure.txt with the "
                   "command in its header")


class Verdict(IntEnum):
    OK = 0
    FINDINGS = 1
    INPUT_ERROR = 2


class InputError(Exception):
    """An input the check cannot trust. Always names the input."""


class Side(Enum):
    HOST = "host"
    TARGET = "target"


class LockMode(Enum):
    """How cargo may treat Cargo.lock: `check` must never write it."""
    FROZEN = "--frozen"
    OFFLINE = "--offline"


@dataclass(frozen=True)
class VariantKey:
    name: str
    version: str
    side: Side

    def sort_key(self) -> tuple[str, str, str]:
        return (self.name, self.version, self.side.value)


@dataclass(frozen=True)
class Variant:
    key: VariantKey
    features: frozenset[str]


def variant(name: str, version: str, side: Side, features: frozenset[str]) -> Variant:
    return Variant(VariantKey(name, version, side), features)


def braces(features: frozenset[str]) -> str:
    return "{" + ",".join(sorted(features)) + "}"


# --- cargo ------------------------------------------------------------------

def cargo(root: Path, args: Sequence[str]) -> str:
    command = [os.environ.get("CARGO", "cargo"), *args]
    shown = shlex.join(command)
    try:
        result = subprocess.run(command, cwd=root, capture_output=True, text=True)
    except OSError as error:
        raise InputError(f"`{shown}` could not start: {error}") from None
    if result.returncode != 0:
        tail = "\n".join(result.stderr.strip().splitlines()[-5:])
        raise InputError(f"`{shown}` exited {result.returncode}: {tail}")
    return result.stdout


def host_triple(root: Path) -> str:
    for line in cargo(root, ["-vV"]).splitlines():
        if line.startswith("host: "):
            return line.removeprefix("host: ").strip()
    raise InputError("`cargo -vV` printed no `host:` line")


# --- cargo tree -------------------------------------------------------------

class Section(Enum):
    NORMAL = "normal"
    BUILD = "[build-dependencies]"
    DEV = "[dev-dependencies]"


SECTION_HEADERS = {section.value: section for section in (Section.BUILD, Section.DEV)}
PREFIX_TOKENS = ("|   ", "    ", "|-- ", "`-- ")
TOKEN_WIDTH = 4
DEDUPED_SUFFIX = " (*)"
PACKAGE_HEAD = re.compile(
    r"(?P<name>[A-Za-z0-9_.-]+) v(?P<version>[^ ]+)"
    r"(?P<proc_macro> \(proc-macro\))?(?: \((?P<source>[^()]*)\))?")


@dataclass(frozen=True)
class TreeNode:
    """A package on the current ancestry path, with the section its children
    are listed under."""
    host: bool
    proc_macro: bool
    section: Section = Section.NORMAL

    def child_is_host(self) -> bool:
        return self.host or self.proc_macro or self.section is Section.BUILD


@dataclass(frozen=True)
class TreePackage:
    name: str
    version: str
    proc_macro: bool
    path_source: bool
    features: frozenset[str]


def split_prefix(line: str) -> tuple[int, str]:
    depth = 0
    while line.startswith(PREFIX_TOKENS, depth * TOKEN_WIDTH):
        depth += 1
    return depth, line[depth * TOKEN_WIDTH:]


def parse_package(body: str, where: str) -> TreePackage:
    head, bar, features = body.removesuffix(DEDUPED_SUFFIX).partition("|")
    if not bar:
        raise InputError(f"{where}: not a `{{p}}|{{f}}` package line")
    match = PACKAGE_HEAD.fullmatch(head)
    if match is None:
        raise InputError(f"{where}: unreadable package `{head}`")
    source = match["source"]
    if source is not None and not source.startswith("/"):
        raise InputError(f"{where}: unsupported source `{source}`; the union crate "
                         f"pins registry packages only")
    return TreePackage(name=match["name"], version=match["version"],
                       proc_macro=match["proc_macro"] is not None,
                       path_source=source is not None,
                       features=frozenset(f for f in features.split(",") if f))


def parse_tree(text: str, source: str) -> frozenset[Variant]:
    """Every registry package variant in `cargo tree -f '{p}|{f}' --charset
    ascii` output, each attributed to the side its ancestry puts it on."""
    variants: set[Variant] = set()
    stack: list[TreeNode] = []
    for number, line in enumerate(text.splitlines(), start=1):
        if not line.strip():
            stack = []
            continue
        where = f"{source} line {number}: {line!r}"
        depth, body = split_prefix(line)
        if body.startswith("["):
            section = SECTION_HEADERS.get(body)
            if section is None:
                raise InputError(f"{where}: unknown section header")
            if depth >= len(stack):
                raise InputError(f"{where}: section header with no package at its depth")
            del stack[depth + 1:]
            stack[depth] = replace(stack[depth], section=section)
            continue
        if depth > len(stack):
            raise InputError(f"{where}: deeper than its parent")
        package = parse_package(body, where)
        host = package.proc_macro or (depth > 0 and stack[depth - 1].child_is_host())
        del stack[depth:]
        stack.append(TreeNode(host=host, proc_macro=package.proc_macro))
        if not package.path_source:
            side = Side.HOST if host else Side.TARGET
            variants.add(variant(package.name, package.version, side, package.features))
    return frozenset(variants)


def resolve(root: Path, mode: LockMode, host: str, selection: Sequence[str]) -> frozenset[Variant]:
    args = ["tree", mode.value, *selection, "-e", "normal,build,dev", "-f", "{p}|{f}",
            "--charset", "ascii", "--target", host]
    return parse_tree(cargo(root, args), f"`cargo tree {' '.join(selection)}`")


# --- cargo metadata ---------------------------------------------------------

@dataclass(frozen=True)
class Member:
    name: str
    manifest_path: str
    depends_on_union: bool
    build_dependents: frozenset[str]


@dataclass(frozen=True)
class Workspace:
    members: tuple[Member, ...]
    union_manifest: Path | None

    @property
    def covered(self) -> tuple[Member, ...]:
        """The members that must depend on the union crate."""
        return tuple(m for m in self.members if not m.build_dependents)

    @property
    def build_dependency_members(self) -> tuple[Member, ...]:
        return tuple(m for m in self.members if m.build_dependents)


DEPENDENCY_KINDS = frozenset({None, "build", "dev"})


def workspace_of(metadata: object) -> Workspace:
    source = "cargo metadata"
    try:
        member_ids = frozenset(metadata["workspace_members"])
        packages = [p for p in metadata["packages"] if p["id"] in member_ids]
        names = frozenset(p["name"] for p in packages)
        for package in packages:
            for dependency in package["dependencies"]:
                if dependency["kind"] not in DEPENDENCY_KINDS:
                    raise InputError(f"{source}: {package['name']} -> {dependency['name']}: "
                                     f"unknown dependency kind {dependency['kind']!r}")
        build_dependents = {
            name: frozenset(p["name"] for p in packages
                            for d in p["dependencies"]
                            if d["kind"] == "build" and d["name"] == name)
            for name in names}
        members = tuple(sorted(
            (Member(name=p["name"], manifest_path=p["manifest_path"],
                    depends_on_union=any(d["name"] == UNION_CRATE and d["kind"] is None
                                         for d in p["dependencies"]),
                    build_dependents=build_dependents[p["name"]])
             for p in packages if p["name"] != UNION_CRATE),
            key=lambda m: m.name))
        union = [Path(p["manifest_path"]) for p in packages if p["name"] == UNION_CRATE]
    except (KeyError, TypeError) as error:
        raise InputError(f"{source}: malformed cargo metadata: {error!r}") from None
    return Workspace(members=members, union_manifest=union[0] if union else None)


def read_workspace(root: Path, mode: LockMode) -> Workspace:
    raw = cargo(root, ["metadata", mode.value, "--no-deps", "--format-version", "1"])
    try:
        metadata = json.loads(raw)
    except json.JSONDecodeError as error:
        raise InputError(f"cargo metadata output is not JSON: {error}") from None
    return workspace_of(metadata)


# --- findings ---------------------------------------------------------------

def coverage_findings(ws: Workspace) -> list[str]:
    return [
        *(f"UNCOVERED {m.name}: does not depend on {UNION_CRATE} — add "
          f"`{UNION_CRATE}.workspace = true` under [dependencies] in {m.manifest_path}"
          for m in ws.covered if not m.depends_on_union),
        *(f"HOST-MEMBER {m.name}: is a build-dependency of "
          f"{{{', '.join(sorted(m.build_dependents))}}} — the union's target pins would "
          f"enter their build-script graphs; remove its {UNION_CRATE} dependency from "
          f"{m.manifest_path}"
          for m in ws.build_dependency_members if m.depends_on_union),
    ]


@dataclass(frozen=True)
class Split:
    member: str
    variant: Variant
    workspace_features: tuple[frozenset[str], ...]

    def __str__(self) -> str:
        key = self.variant.key
        if self.workspace_features:
            there = " and ".join(braces(f) for f in self.workspace_features)
        else:
            there = f"no {key.side.value} variant"
        return (f"SPLIT {self.member} {key.name} v{key.version} ({key.side.value}): "
                f"-p resolves {braces(self.variant.features)}; --workspace resolves {there}")


def features_by_key(variants: frozenset[Variant]) -> dict[VariantKey, tuple[frozenset[str], ...]]:
    grouped: dict[VariantKey, list[frozenset[str]]] = {}
    for v in variants:
        grouped.setdefault(v.key, []).append(v.features)
    return {key: tuple(sorted(sets, key=sorted)) for key, sets in grouped.items()}


@dataclass(frozen=True)
class Resolution:
    workspace: frozenset[Variant]
    splits: tuple[Split, ...]


def resolve_splits(root: Path, mode: LockMode, host: str, ws: Workspace) -> Resolution:
    workspace = resolve(root, mode, host, ["--workspace"])
    in_workspace = features_by_key(workspace)
    found = []
    for member in ws.covered:
        selected = resolve(root, mode, host, ["-p", member.name])
        for v in sorted(selected - workspace, key=lambda v: (v.key.sort_key(), sorted(v.features))):
            found.append(Split(member.name, v, in_workspace.get(v.key, ())))
    return Resolution(workspace=workspace, splits=tuple(found))


# --- the union manifest -----------------------------------------------------

BEGIN_MARKER = ("# BEGIN generated by scripts/workspace-feature-union.py generate "
                "— do not edit by hand")
END_MARKER = "# END generated"

Pins = dict[VariantKey, frozenset[str]]


@dataclass(frozen=True)
class UnionManifest:
    """The union crate's Cargo.toml split around its generated region; every
    byte outside the region is written back unchanged."""
    path: Path
    head: str
    tail: str

    def write(self, region: str) -> None:
        self.path.write_text(self.head + region + self.tail, encoding="utf-8")


def read_union_manifest(path: Path) -> UnionManifest:
    try:
        lines = path.read_text(encoding="utf-8").splitlines(keepends=True)
    except (OSError, UnicodeDecodeError) as error:
        raise InputError(f"{path}: unreadable union manifest: {error}") from None
    begins = [i for i, line in enumerate(lines) if line.rstrip("\r\n") == BEGIN_MARKER]
    ends = [i for i, line in enumerate(lines) if line.rstrip("\r\n") == END_MARKER]
    if len(begins) != 1 or len(ends) != 1 or ends[0] < begins[0]:
        raise InputError(f"{path}: needs exactly one `{BEGIN_MARKER}` line followed by "
                         f"exactly one `{END_MARKER}` line")
    return UnionManifest(path=path, head="".join(lines[:begins[0] + 1]),
                         tail="".join(lines[ends[0]:]))


def requirement(version: str) -> str:
    """The widest requirement that still selects only `version`'s semver-
    compatible series, so a pin never constrains Cargo.lock."""
    release = version.partition("+")[0]
    if "-" in release:
        return f"={release}"
    parts = release.split(".")
    if len(parts) != 3 or not all(part.isdigit() for part in parts):
        raise InputError(f"cargo tree: unreadable version `{version}`")
    major, minor, patch = parts
    if major != "0":
        return major
    if minor != "0":
        return f"0.{minor}"
    return f"=0.0.{patch}"


def render_row(key: VariantKey, features: frozenset[str], renamed: bool) -> str:
    req = requirement(key.version)
    name = f"{key.name}-v{'-'.join(re.findall(r'[0-9]+', req))}" if renamed else key.name
    fields = [f'package = "{key.name}"'] if renamed else []
    fields.append(f'version = "{req}"')
    if "default" not in features:
        fields.append("default-features = false")
    listed = sorted(features - {"default"})
    if listed:
        fields.append("features = [" + ", ".join(f'"{f}"' for f in listed) + "]")
    return f"{name} = {{ {', '.join(fields)} }}"


def render_table(header: str, pins: Pins) -> str:
    versions: dict[str, int] = {}
    for key in pins:
        versions[key.name] = versions.get(key.name, 0) + 1
    rows = sorted(render_row(key, features, renamed=versions[key.name] > 1)
                  for key, features in pins.items())
    return "\n".join([header, *rows]) + "\n"


TABLE_OF_SIDE = {Side.TARGET: "dependencies", Side.HOST: "build-dependencies"}


def render_region(pins: Pins, host: str) -> str:
    tables = []
    for side, table in TABLE_OF_SIDE.items():
        on_side = {key: f for key, f in pins.items() if key.side is side}
        if on_side:
            tables.append(render_table(f"[target.{host}.{table}]", on_side))
    return "\n".join(tables)


def grown(pins: Pins, resolution: Resolution) -> Pins:
    """`pins` with every split key's features widened to the union of the
    workspace's variants of that key; a key the workspace never builds on that
    side stays unpinned."""
    in_workspace = features_by_key(resolution.workspace)
    result = dict(pins)
    for split in resolution.splits:
        key = split.variant.key
        if key in in_workspace:
            result[key] = result.get(key, frozenset()).union(*in_workspace[key])
    return result


# --- subcommands ------------------------------------------------------------

def fail(lines: Sequence[str], *trailer: str) -> Verdict:
    print("\n".join([*lines, *trailer]))
    print(f"workspace-feature-union: FAIL ({len(lines)} findings) — "
          f"python3 scripts/workspace-feature-union.py --help explains each line")
    return Verdict.FINDINGS


def run_check(root: Path) -> Verdict:
    ws = read_workspace(root, LockMode.FROZEN)
    found = resolve_splits(root, LockMode.FROZEN, host_triple(root), ws).splits
    lines = [*coverage_findings(ws), *map(str, found)]
    if lines:
        return fail(lines, *([GENERATE_REMEDY] if found else []))
    print(f"workspace-feature-union: OK ({len(ws.covered)} members resolve every external "
          f"package like --workspace)")
    return Verdict.OK


def run_generate(root: Path, max_iterations: int) -> Verdict:
    mode = LockMode.OFFLINE
    ws = read_workspace(root, mode)
    coverage = coverage_findings(ws)
    if coverage:
        return fail(coverage, "generate writes nothing until the coverage rule holds")
    if ws.union_manifest is None:
        raise InputError(f"cargo metadata: the workspace has no {UNION_CRATE} package")
    manifest = read_union_manifest(ws.union_manifest)
    host = host_triple(root)
    pins: Pins = {}
    manifest.write(render_region(pins, host))
    resolution = resolve_splits(root, mode, host, ws)
    for _ in range(max_iterations):
        if not resolution.splits:
            break
        widened = grown(pins, resolution)
        if widened == pins:
            break
        pins = widened
        manifest.write(render_region(pins, host))
        resolution = resolve_splits(root, mode, host, ws)
    if resolution.splits:
        return fail([str(split) for split in resolution.splits],
                    f"generate stopped with {len(pins)} pins in {manifest.path}: pinning "
                    f"cannot remove a split whose side --workspace never builds, nor one "
                    f"that persists past --max-iterations {max_iterations}")
    print(f"workspace-feature-union: generated {len(pins)} pins in {manifest.path}; "
          f"every member resolves like --workspace — now regenerate "
          f"crates/reify-eval/engine_hash_closure.txt with the command in its header")
    return Verdict.OK


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    check = commands.add_parser("check", help="report findings; never writes")
    generate = commands.add_parser(
        "generate", help="rewrite the union manifest's generated region to a fixpoint")
    generate.add_argument("--max-iterations", type=positive, default=6,
                          help="rewrites before giving up (default: 6)")
    for command in (check, generate):
        command.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1],
                             help="workspace root (default: this checkout)")
    return parser.parse_args(argv)


def positive(text: str) -> int:
    value = int(text)
    if value < 1:
        raise argparse.ArgumentTypeError(f"must be at least 1, found {value}")
    return value


def main(argv: Sequence[str]) -> int:
    args = parse_args(argv)
    try:
        if args.command == "generate":
            return run_generate(args.root, args.max_iterations)
        return run_check(args.root)
    except (InputError, OSError) as error:
        print(f"workspace-feature-union: {error}", file=sys.stderr)
        return Verdict.INPUT_ERROR


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
