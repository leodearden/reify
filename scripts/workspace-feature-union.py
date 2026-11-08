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

A side is HOST for build-dependencies, proc-macros and everything below
them, TARGET otherwise; resolver 2 keeps one feature set per side. A member
is host-side when it is a build-dependency of another member or a
proc-macro. The rule, checked without compiling anything:
  - every member that is not host-side depends on workspace-feature-union (a
    normal dependency); a host-side member is exempt and is not resolved
    with `-p`, so `-p <host-side member>` stays split;
  - no host-side member has a normal or build dependency on
    workspace-feature-union or on a covered member, and no member has a
    build-dependency on workspace-feature-union, so the union never reaches
    the host side, where its target pins would become host units. This is
    read from the members' own `cargo metadata` edges. Optional and
    platform-specific path dependencies count even when inactive, so the
    rule errs loud;
  - for every covered member M, each external (name, version, side, features)
    variant that `-p M` compiles, read from `cargo tree -p M`, also occurs
    among those `--workspace` compiles. A dependency listed under a
    non-selected package's [dev-dependencies] is not compiled, and a member
    printed twice as a root is placed by cargo tree's printing rules.

Reading the output:
    UNCOVERED <m>: ...          add `workspace-feature-union.workspace = true`
                                under [dependencies] in the named manifest.
    HOST-UNION <m>: <path> ...  a normal or build dependency of <m> brings
                                workspace-feature-union, or a covered member
                                that must depend on it, onto the host side,
                                as <path> shows; remove the dependency the
                                line names from the named manifest. While
                                such a finding stands, check reports it
                                without resolving cargo tree.
    SPLIT <m> <name> v<ver> (<side>): -p resolves {..}; --workspace resolves {..}
                                `-p <m>` compiles a variant the base never
                                builds. Run
                                `python3 scripts/workspace-feature-union.py generate`,
                                then regenerate crates/reify-eval/engine_hash_closure.txt
                                with the command in its header.
    MEMBER-SPLIT <name> v<ver> (<side>): -p resolves {..} under <m>, ...; --workspace resolves {..}
                                advisory, never a finding and never fails:
                                `-p <m>` builds workspace member <name> with
                                features the base's `--workspace` build does
                                not, so <name> and its dependents rebuild in a
                                fresh lane. The union cannot pin a member.
                                Decision, classes and limits:
                                docs/notes/workspace-feature-union.md,
                                "Member features".

`check` is a lower bound: the union's own pins are part of `--workspace`, so a
pin no member needs any more passes it. `generate` starts from an empty
region, so it is what sheds a stale pin.

`check` resolves with `--frozen`: it never writes Cargo.lock and never touches
the network. `generate` rewrites the marked region of
crates/workspace-feature-union/Cargo.toml to a fixpoint and resolves with
`--offline`, so the rewrite may update Cargo.lock; if an input fails part-way
it puts both files back as it found them.

The gate runs from crates/reify-build-utils/tests/workspace_feature_union/
(`cargo test -p reify-build-utils --test workspace_feature_union`). Design,
limits and the probe procedure: docs/notes/workspace-feature-union.md.

Exit codes: 0 every member resolves like the workspace; 1 a finding above;
2 an input (cargo metadata, cargo tree, the union manifest) could not be
trusted, including a cargo tree output whose sides cannot be read.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shlex
import subprocess
import sys
from collections.abc import Mapping, Sequence
from dataclasses import dataclass, field
from enum import Enum, IntEnum
from pathlib import Path

UNION_CRATE = "workspace-feature-union"
GENERATE_REMEDY = ("remedy: run `python3 scripts/workspace-feature-union.py generate`, "
                   "then regenerate crates/reify-eval/engine_hash_closure.txt with the "
                   "command in its header")
HOST_UNION_TRAILER = ("note: SPLIT and MEMBER-SPLIT are not compared while a HOST-UNION "
                      "finding stands: remove it, then run check again")


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

    def sort_key(self) -> tuple[tuple[str, str, str], list[str]]:
        return (self.key.sort_key(), sorted(self.features))


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
class TreePackage:
    name: str
    version: str
    proc_macro: bool
    path_source: bool
    features: frozenset[str]

    @property
    def label(self) -> str:
        return f"{self.name} v{self.version}"


@dataclass(frozen=True)
class Tree:
    """The variants a build of one `cargo tree` output's member roots
    compiles: the registry packages the union can pin, and the workspace
    members other than the union, which it cannot (a member pin would be a
    dependency cycle)."""
    variants: frozenset[Variant]
    members: frozenset[Variant]


@dataclass(frozen=True)
class TreeLine:
    """One package line. `node` is its `{p}|{f}` text without the `(*)`
    suffix; with the line's side it names one node of cargo's graph, because
    the tree is resolved for `--target <host>`. `section` is the section of
    its parent it is listed under, NORMAL for a root."""
    package: TreePackage
    node: str
    deduped: bool
    parent: int | None
    section: Section
    where: str


@dataclass(frozen=True)
class Root:
    """A depth-0 line and its block: its own line and every line below it,
    with `text` the block character for character."""
    index: int
    end: int
    text: str

    @property
    def lines(self) -> range:
        return range(self.index, self.end)


@dataclass(frozen=True)
class Forest:
    """A `cargo tree` output's package lines in print order, each linked to
    its parent; `children[i]` lists line i's children in print order."""
    lines: tuple[TreeLine, ...]
    children: tuple[tuple[int, ...], ...]
    roots: tuple[Root, ...]
    source: str

    def package(self, root: Root) -> TreePackage:
        return self.lines[root.index].package


RootSides = dict[int, Side]
Node = tuple[str, Side]


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


def parse_forest(text: str, source: str) -> Forest:
    """`cargo tree -f '{p}|{f}' --charset ascii` output as linked lines."""
    lines: list[TreeLine] = []
    children: list[list[int]] = []
    blocks: list[tuple[int, list[str]]] = []
    path: list[tuple[int, Section]] = []
    for number, raw in enumerate(text.splitlines(), start=1):
        if not raw.strip():
            path = []
            continue
        where = f"{source} line {number}: {raw!r}"
        depth, body = split_prefix(raw)
        if body.startswith("["):
            section = SECTION_HEADERS.get(body)
            if section is None:
                raise InputError(f"{where}: unknown section header")
            if depth >= len(path):
                raise InputError(f"{where}: section header with no package at its depth")
            del path[depth + 1:]
            path[depth] = (path[depth][0], section)
            blocks[-1][1].append(raw)
            continue
        if depth > len(path):
            raise InputError(f"{where}: deeper than its parent")
        package = parse_package(body, where)
        del path[depth:]
        parent, section = path[-1] if path else (None, Section.NORMAL)
        index = len(lines)
        lines.append(TreeLine(package=package, node=body.removesuffix(DEDUPED_SUFFIX),
                              deduped=body.endswith(DEDUPED_SUFFIX), parent=parent,
                              section=section, where=where))
        children.append([])
        if parent is None:
            blocks.append((index, []))
        else:
            children[parent].append(index)
        blocks[-1][1].append(raw)
        path.append((index, Section.NORMAL))
    ends = [start for start, _ in blocks[1:]] + [len(lines)]
    roots = tuple(Root(index=start, end=end, text="\n".join(block))
                  for (start, block), end in zip(blocks, ends))
    return Forest(lines=tuple(lines), children=tuple(tuple(c) for c in children),
                  roots=roots, source=source)


def block_sides(forest: Forest, root: Root, side: Side) -> dict[int, Side]:
    """The side of each line of `root`'s block, in print order, with the root
    on `side`: a proc-macro is HOST, and so is everything listed under a
    `[build-dependencies]` header or below a HOST line."""
    sides: dict[int, Side] = {}
    for index in root.lines:
        line = forest.lines[index]
        if line.package.proc_macro or line.section is Section.BUILD:
            sides[index] = Side.HOST
        elif line.parent is None:
            sides[index] = side
        else:
            sides[index] = sides[line.parent]
    return sides


def sides_under(forest: Forest, root_sides: RootSides) -> tuple[Side, ...]:
    sides: dict[int, Side] = {}
    for root in forest.roots:
        sides.update(block_sides(forest, root, root_sides[root.index]))
    return tuple(sides[index] for index in range(len(forest.lines)))


@dataclass
class PrintState:
    """What the lines admitted so far fix under one side assignment, checked
    against two properties of cargo tree's printer and resolver 2. R1: a
    node's first line lists its dependencies, and every later line is `(*)`
    and lists none, except that a node without dependencies never gets
    `(*)`. R2: one feature set per package and side."""
    listed: dict[Node, bool] = field(default_factory=dict)
    features: dict[VariantKey, frozenset[str]] = field(default_factory=dict)

    def copy(self) -> PrintState:
        return PrintState(listed=dict(self.listed), features=dict(self.features))

    def admit(self, line: TreeLine, side: Side, lists_dependencies: bool) -> str | None:
        """Records `line` on `side`, or names the rule it breaks."""
        node = (line.node, side)
        if node not in self.listed:
            if line.deduped:
                return f"`{DEDUPED_SUFFIX}` before any line lists this package's dependencies"
            self.listed[node] = lists_dependencies
        elif lists_dependencies:
            return "a later line of this package lists dependencies; only its first line does"
        elif line.deduped != self.listed[node]:
            return (f"`{DEDUPED_SUFFIX}` on a package whose first line lists no dependencies"
                    if line.deduped else
                    f"no `{DEDUPED_SUFFIX}` on a later line of a package whose first line "
                    f"lists its dependencies")
        key = VariantKey(line.package.name, line.package.version, side)
        known = self.features.setdefault(key, line.package.features)
        if known != line.package.features:
            return (f"{line.package.label} already has features {braces(known)} on the "
                    f"{side.value} side")
        return None


def admit_block(forest: Forest, root: Root, side: Side, state: PrintState) -> str | None:
    """Admits `root`'s block with the root on `side`, or says where and why it
    breaks a printing rule."""
    for index, line_side in block_sides(forest, root, side).items():
        line = forest.lines[index]
        reason = state.admit(line, line_side, bool(forest.children[index]))
        if reason is not None:
            return f"{line.where}: {reason}"
    return None


def root_copies(forest: Forest) -> dict[str, tuple[Root, ...]]:
    """Each package printed as a root, with its roots in print order: one, or
    for a member reached on the host side, its target and its host copy."""
    grouped: dict[str, list[Root]] = {}
    for root in forest.roots:
        grouped.setdefault(forest.package(root).label, []).append(root)
    for label, roots in grouped.items():
        if forest.package(roots[0]).proc_macro and len(roots) > 1:
            raise InputError(f"{forest.source}: prints {len(roots)} roots of proc-macro "
                             f"{label}; cargo prints one, its host copy")
        if len(roots) > 2:
            raise InputError(f"{forest.source}: prints {len(roots)} roots of {label}; cargo "
                             f"prints at most two, its target and its host copy")
    return {label: tuple(roots) for label, roots in grouped.items()}


OPPOSITE = {Side.TARGET: Side.HOST, Side.HOST: Side.TARGET}


def sides_to_try(forest: Forest, root: Root, copies: tuple[Root, ...],
                 root_sides: RootSides) -> tuple[Side, ...]:
    """A root printed once is TARGET, or HOST for a proc-macro. A pair's first
    root is tried on both sides, and its second root takes the other side."""
    if len(copies) == 1:
        return (Side.HOST,) if forest.package(root).proc_macro else (Side.TARGET,)
    first, second = copies
    if root == second:
        return (OPPOSITE[root_sides[first.index]],)
    return (Side.TARGET, Side.HOST)


def collapse_identical_swaps(root: Root, copies: tuple[Root, ...],
                             surviving: list[tuple[RootSides, PrintState]]
                             ) -> list[tuple[RootSides, PrintState]]:
    """Keeps the first of the assignments that differ only in which copy of a
    character-identical pair is the host one: once the pair's second root is
    admitted, they have admitted the same (line text, side) pairs, so they
    hold the same PrintState and compile the same Tree."""
    if len(copies) != 2 or root != copies[1] or copies[0].text != copies[1].text:
        return surviving
    pair = {copy.index for copy in copies}
    kept: dict[frozenset[tuple[int, Side]], tuple[RootSides, PrintState]] = {}
    for assignment in surviving:
        root_sides, _ = assignment
        kept.setdefault(frozenset((index, side) for index, side in root_sides.items()
                                  if index not in pair), assignment)
    return list(kept.values())


MAX_OPEN_ASSIGNMENTS = 2 ** 6


def undecided_pairs(forest: Forest, open_assignments: Sequence[RootSides]) -> list[str]:
    """The packages printed twice whose first root has more than one side
    across `open_assignments`."""
    return list(dict.fromkeys(
        forest.lines[index].package.label for index in open_assignments[0]
        if len({sides[index] for sides in open_assignments}) > 1))


def side_assignments(forest: Forest) -> list[RootSides]:
    """Every side assignment of the roots, up to swapping a character-identical
    pair, under which each line, in print order, obeys cargo tree's printing
    rules (PrintState). Each pair that no line has decided yet doubles the
    open assignments, so more than MAX_OPEN_ASSIGNMENTS is an input error."""
    copies = root_copies(forest)
    open_assignments: list[tuple[RootSides, PrintState]] = [({}, PrintState())]
    last_break = ""
    for root in forest.roots:
        surviving = []
        for root_sides, state in open_assignments:
            sides = sides_to_try(forest, root, copies[forest.package(root).label], root_sides)
            for side in sides:
                branch = state.copy() if len(sides) > 1 else state
                broken = admit_block(forest, root, side, branch)
                if broken is None:
                    surviving.append(({**root_sides, root.index: side}, branch))
                else:
                    last_break = broken
        if not surviving:
            pairs = [label for label, roots in copies.items() if len(roots) == 2]
            if not pairs:
                raise InputError(last_break)
            raise InputError(f"{forest.source}: no side assignment of the roots printed twice "
                             f"({', '.join(pairs)}) obeys cargo tree's printing rules; the "
                             f"last one tried breaks at {last_break}")
        open_assignments = collapse_identical_swaps(root, copies[forest.package(root).label],
                                                    surviving)
        if len(open_assignments) > MAX_OPEN_ASSIGNMENTS:
            undecided = undecided_pairs(forest, [sides for sides, _ in open_assignments])
            raise InputError(
                f"{forest.lines[root.index].where}: {len(open_assignments)} side assignments "
                f"of the roots printed twice obey cargo tree's printing rules up to this "
                f"root, more than the {MAX_OPEN_ASSIGNMENTS} the check tries, because no "
                f"line so far decides which root of {', '.join(undecided)} is its host copy")
    return [root_sides for root_sides, _ in open_assignments]


def compiled(forest: Forest, sides: Sequence[Side]) -> Tree:
    """The variants a build of the forest's member roots compiles: every
    root but the host copy of a member printed twice, so each TARGET root and
    a proc-macro's one root, which is HOST. That is each such root's own
    dev-dependencies, plus everything normal and build edges reach. A node's
    dependencies are the children of its first line, which a `(*)` line
    stands for; a dev section below any other node lists dependencies the
    build never compiles."""
    first: dict[Node, int] = {}
    for index, line in enumerate(forest.lines):
        first.setdefault((line.node, sides[index]), index)
    starts = {(forest.lines[root.index].node, sides[root.index]) for root in forest.roots
              if sides[root.index] is Side.TARGET or forest.package(root).proc_macro}
    reached = set(starts)
    pending = list(starts)
    variants: set[Variant] = set()
    members: set[Variant] = set()
    while pending:
        node = pending.pop()
        index = first[node]
        package = forest.lines[index].package
        found = variant(package.name, package.version, node[1], package.features)
        if not package.path_source:
            variants.add(found)
        elif package.name != UNION_CRATE:
            members.add(found)
        for child in forest.children[index]:
            if forest.lines[child].section is Section.DEV and node not in starts:
                continue
            dependency = (forest.lines[child].node, sides[child])
            if dependency not in reached:
                reached.add(dependency)
                pending.append(dependency)
    return Tree(variants=frozenset(variants), members=frozenset(members))


def parse_tree(text: str, source: str) -> Tree:
    """What a build of `cargo tree -f '{p}|{f}' --charset ascii` output's
    member roots compiles, each variant on the side cargo resolves it for."""
    forest = parse_forest(text, source)
    assignments = side_assignments(forest)
    trees = {compiled(forest, sides_under(forest, a)) for a in assignments}
    if len(trees) == 1:
        return trees.pop()
    undecided = undecided_pairs(forest, assignments)
    raise InputError(f"{source}: cannot tell which root of {', '.join(undecided)} is its host "
                     f"copy: {len(assignments)} side assignments obey cargo tree's printing "
                     f"rules and resolve different packages")


def resolve(root: Path, mode: LockMode, host: str, selection: Sequence[str]) -> Tree:
    args = ["tree", mode.value, *selection, "-e", "normal,build,dev", "-f", "{p}|{f}",
            "--charset", "ascii", "--target", host]
    return parse_tree(cargo(root, args), f"`cargo tree {' '.join(selection)}`")


# --- cargo metadata ---------------------------------------------------------

@dataclass(frozen=True)
class Dependency:
    """One dependency edge of a member, keyed by package name."""
    name: str
    section: Section


@dataclass(frozen=True)
class Member:
    name: str
    manifest_path: str
    dependencies: frozenset[Dependency]
    build_dependents: frozenset[str]
    proc_macro: bool

    @property
    def depends_on_union(self) -> bool:
        return Dependency(UNION_CRATE, Section.NORMAL) in self.dependencies

    @property
    def host_side(self) -> bool:
        """Compiled for the host wherever another member uses it."""
        return self.proc_macro or bool(self.build_dependents)


@dataclass(frozen=True)
class Workspace:
    members: tuple[Member, ...]
    union_manifest: Path | None

    @property
    def covered(self) -> tuple[Member, ...]:
        """The members that must depend on the union crate."""
        return tuple(m for m in self.members if not m.host_side)

    def member(self, name: str) -> Member | None:
        return next((m for m in self.members if m.name == name), None)


DEPENDENCY_SECTIONS = {None: Section.NORMAL, "build": Section.BUILD, "dev": Section.DEV}


def dependencies_of(package: dict, source: str) -> frozenset[Dependency]:
    edges = set()
    for dependency in package["dependencies"]:
        section = DEPENDENCY_SECTIONS.get(dependency["kind"])
        if section is None:
            raise InputError(f"{source}: {package['name']} -> {dependency['name']}: "
                             f"unknown dependency kind {dependency['kind']!r}")
        edges.add(Dependency(dependency["name"], section))
    return frozenset(edges)


def workspace_of(metadata: object) -> Workspace:
    source = "cargo metadata"
    try:
        member_ids = frozenset(metadata["workspace_members"])
        packages = [p for p in metadata["packages"] if p["id"] in member_ids]
        edges = {p["name"]: dependencies_of(p, source) for p in packages}
        build_dependents = {
            name: frozenset(dependent for dependent, theirs in edges.items()
                            if Dependency(name, Section.BUILD) in theirs)
            for name in edges}
        members = tuple(sorted(
            (Member(name=p["name"], manifest_path=p["manifest_path"],
                    dependencies=edges[p["name"]],
                    build_dependents=build_dependents[p["name"]],
                    proc_macro=any("proc-macro" in t["kind"] for t in p["targets"]))
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
    return [f"UNCOVERED {m.name}: does not depend on {UNION_CRATE} — add "
            f"`{UNION_CRATE}.workspace = true` under [dependencies] in {m.manifest_path}"
            for m in ws.covered if not m.depends_on_union]


EDGE_NOUN = {Section.NORMAL: "dependency", Section.BUILD: "build-dependency",
             Section.DEV: "dev-dependency"}
EDGE_ARROW = {Section.NORMAL: " -> ", Section.BUILD: " -[build]-> ", Section.DEV: " -[dev]-> "}


def path_head(parent: Member) -> str:
    """The head of a HOST-UNION path: how the edge's parent is reached on the
    host side, or its bare name when it is covered and its own build edge is
    what reaches the host."""
    if parent.proc_macro:
        return f"{parent.name} (proc-macro)"
    if parent.build_dependents:
        return f"{min(parent.build_dependents)}{EDGE_ARROW[Section.BUILD]}{parent.name}"
    return parent.name


def compiled_for_host(parent: Member, edge: Dependency) -> bool:
    """Whether building `parent` as a dependency compiles `edge` for the host."""
    return edge.section is not Section.DEV and (parent.host_side or edge.section is Section.BUILD)


UNION_ON_HOST = (f"resolves {UNION_CRATE} on the host side, where its target pins would "
                 f"become host units")


def path_tail(ws: Workspace, edge: Dependency) -> str:
    """The end of a HOST-UNION path from `edge` on, and what it puts on the
    host side: the union itself, or a covered member, which reaches the union
    once the coverage rule holds."""
    child = ws.member(edge.name)
    if child is None:
        return f"{edge.name} {UNION_ON_HOST}"
    if child.depends_on_union:
        return f"{edge.name}{EDGE_ARROW[Section.NORMAL]}{UNION_CRATE} {UNION_ON_HOST}"
    return (f"{edge.name} puts covered member {edge.name} on the host side, where the "
            f"dependency on {UNION_CRATE} it must add would make the union's target pins "
            f"host units")


def host_union_findings(ws: Workspace) -> list[str]:
    """One finding per dependency edge that brings the union, or a covered
    member, onto the host side, read from the workspace's own dependency edges
    so that cargo tree's print order and its cross-root `(*)` dedupe cannot
    hide one."""
    brings_union = {m.name for m in ws.covered} | {UNION_CRATE}
    found = []
    for parent in ws.members:
        for edge in sorted(parent.dependencies, key=lambda d: (d.name, d.section.value)):
            if edge.name not in brings_union or not compiled_for_host(parent, edge):
                continue
            found.append(
                f"HOST-UNION {parent.name}: {path_head(parent)}{EDGE_ARROW[edge.section]}"
                f"{path_tail(ws, edge)} — remove {parent.name}'s {EDGE_NOUN[edge.section]} "
                f"on {edge.name} from {parent.manifest_path}")
    return found


def resolved_by_workspace(side: Side, workspace_features: tuple[frozenset[str], ...]) -> str:
    if workspace_features:
        return " and ".join(braces(f) for f in workspace_features)
    return f"no {side.value} variant"


@dataclass(frozen=True)
class Split:
    member: str
    variant: Variant
    workspace_features: tuple[frozenset[str], ...]

    def __str__(self) -> str:
        key = self.variant.key
        there = resolved_by_workspace(key.side, self.workspace_features)
        return (f"SPLIT {self.member} {key.name} v{key.version} ({key.side.value}): "
                f"-p resolves {braces(self.variant.features)}; --workspace resolves {there}")


@dataclass(frozen=True)
class MemberSplit:
    """Advisory, never a finding: a workspace-member variant that the `-p`
    builds of `selections` resolve and `--workspace` never builds."""
    variant: Variant
    selections: tuple[str, ...]
    workspace_features: tuple[frozenset[str], ...]

    def __str__(self) -> str:
        key = self.variant.key
        there = resolved_by_workspace(key.side, self.workspace_features)
        return (f"MEMBER-SPLIT {key.name} v{key.version} ({key.side.value}): -p resolves "
                f"{braces(self.variant.features)} under {', '.join(self.selections)}; "
                f"--workspace resolves {there}")


def features_by_key(variants: frozenset[Variant]) -> dict[VariantKey, tuple[frozenset[str], ...]]:
    grouped: dict[VariantKey, list[frozenset[str]]] = {}
    for v in variants:
        grouped.setdefault(v.key, []).append(v.features)
    return {key: tuple(sorted(sets, key=sorted)) for key, sets in grouped.items()}


def member_splits(workspace: frozenset[Variant],
                  selected: Mapping[str, frozenset[Variant]]) -> tuple[MemberSplit, ...]:
    """One advisory per member variant that some selection resolves and the
    workspace never builds, naming every selection that resolves it."""
    in_workspace = features_by_key(workspace)
    resolving: dict[Variant, list[str]] = {}
    for selection, members in selected.items():
        for v in members - workspace:
            resolving.setdefault(v, []).append(selection)
    return tuple(MemberSplit(v, tuple(sorted(resolving[v])), in_workspace.get(v.key, ()))
                 for v in sorted(resolving, key=Variant.sort_key))


@dataclass(frozen=True)
class Resolution:
    workspace: frozenset[Variant]
    splits: tuple[Split, ...]
    member_splits: tuple[MemberSplit, ...]


def resolve_splits(root: Path, mode: LockMode, host: str, ws: Workspace) -> Resolution:
    workspace = resolve(root, mode, host, ["--workspace"])
    in_workspace = features_by_key(workspace.variants)
    found = []
    selected_members: dict[str, frozenset[Variant]] = {}
    for member in ws.covered:
        selected = resolve(root, mode, host, ["-p", member.name])
        selected_members[member.name] = selected.members
        for v in sorted(selected.variants - workspace.variants, key=Variant.sort_key):
            found.append(Split(member.name, v, in_workspace.get(v.key, ())))
    return Resolution(workspace=workspace.variants, splits=tuple(found),
                      member_splits=member_splits(workspace.members, selected_members))


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
    coverage = coverage_findings(ws)
    host_union = host_union_findings(ws)
    if host_union:
        return fail([*coverage, *host_union], HOST_UNION_TRAILER)
    resolution = resolve_splits(root, LockMode.FROZEN, host_triple(root), ws)
    advisories = resolution.member_splits
    for advisory in advisories:
        print(advisory)
    found = resolution.splits
    lines = [*coverage, *map(str, found)]
    if lines:
        return fail(lines, *([GENERATE_REMEDY] if found else []))
    advised = f"; {len(advisories)} advisory MEMBER-SPLIT lines above" if advisories else ""
    print(f"workspace-feature-union: OK ({len(ws.covered)} members resolve every external "
          f"package like --workspace{advised})")
    return Verdict.OK


@dataclass(frozen=True)
class Snapshot:
    """Files as they were before generate wrote anything; a missing file is
    recorded as None."""
    contents: tuple[tuple[Path, bytes | None], ...]

    @classmethod
    def take(cls, *paths: Path) -> Snapshot:
        return cls(tuple((path, path.read_bytes() if path.exists() else None)
                         for path in paths))

    def restore(self) -> str:
        for path, data in self.contents:
            if data is None:
                path.unlink(missing_ok=True)
            else:
                path.write_bytes(data)
        return " and ".join(str(path) for path, _ in self.contents)


def run_generate(root: Path, max_iterations: int) -> Verdict:
    mode = LockMode.OFFLINE
    ws = read_workspace(root, mode)
    if ws.union_manifest is None:
        raise InputError(f"cargo metadata: the workspace has no {UNION_CRATE} package")
    manifest = read_union_manifest(ws.union_manifest)
    host = host_triple(root)
    snapshot = Snapshot.take(manifest.path, root / "Cargo.lock")
    try:
        return converge(root, mode, host, ws, manifest, max_iterations)
    except (InputError, OSError) as error:
        restored = snapshot.restore()
        raise InputError(f"{error}\ngenerate restored {restored} as it found them") from None


def converge(root: Path, mode: LockMode, host: str, ws: Workspace, manifest: UnionManifest,
             max_iterations: int) -> Verdict:
    refusals = [*coverage_findings(ws), *host_union_findings(ws)]
    if refusals:
        return fail(refusals, "generate writes nothing until the coverage rule holds")
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
