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


@dataclass(frozen=True, order=True)
class VariantKey:
    name: str
    version: str
    side: str

    @property
    def side_enum(self) -> Side:
        return Side(self.side)


@dataclass(frozen=True)
class Variant:
    key: VariantKey
    features: frozenset[str]


def variant(name: str, version: str, side: Side, features: frozenset[str]) -> Variant:
    return Variant(VariantKey(name, version, side.value), features)


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
            there = f"no {key.side} variant"
        return (f"SPLIT {self.member} {key.name} v{key.version} ({key.side}): "
                f"-p resolves {braces(self.variant.features)}; --workspace resolves {there}")


def features_by_key(variants: frozenset[Variant]) -> dict[VariantKey, tuple[frozenset[str], ...]]:
    grouped: dict[VariantKey, list[frozenset[str]]] = {}
    for v in variants:
        grouped.setdefault(v.key, []).append(v.features)
    return {key: tuple(sorted(sets, key=sorted)) for key, sets in grouped.items()}


def splits(root: Path, mode: LockMode, ws: Workspace) -> list[Split]:
    host = host_triple(root)
    workspace = resolve(root, mode, host, ["--workspace"])
    in_workspace = features_by_key(workspace)
    found = []
    for member in ws.covered:
        selected = resolve(root, mode, host, ["-p", member.name])
        for v in sorted(selected - workspace, key=lambda v: (v.key, sorted(v.features))):
            found.append(Split(member.name, v, in_workspace.get(v.key, ())))
    return found


# --- subcommands ------------------------------------------------------------

def report(lines: Sequence[str], any_split: bool, checked: int) -> Verdict:
    if lines:
        print("\n".join(lines))
        if any_split:
            print(GENERATE_REMEDY)
        print(f"workspace-feature-union: FAIL ({len(lines)} findings) — "
              f"python3 scripts/workspace-feature-union.py --help explains each line")
        return Verdict.FINDINGS
    print(f"workspace-feature-union: OK ({checked} members resolve every external "
          f"package like --workspace)")
    return Verdict.OK


def run_check(root: Path) -> Verdict:
    ws = read_workspace(root, LockMode.FROZEN)
    found = splits(root, LockMode.FROZEN, ws)
    return report([*coverage_findings(ws), *map(str, found)], bool(found), len(ws.covered))


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    check = commands.add_parser("check", help="report findings; never writes")
    check.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1],
                       help="workspace root (default: this checkout)")
    return parser.parse_args(argv)


def main(argv: Sequence[str]) -> int:
    args = parse_args(argv)
    try:
        return run_check(args.root)
    except (InputError, OSError) as error:
        print(f"workspace-feature-union: {error}", file=sys.stderr)
        return Verdict.INPUT_ERROR


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
