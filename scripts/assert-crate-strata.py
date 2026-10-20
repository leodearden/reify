#!/usr/bin/env python3
"""
assert-crate-strata.py — the workspace crate strata assertion.

The rule: every workspace package sits in exactly one stratum of
scripts/crate-strata.json. Every normal or build edge between workspace
packages (optional and target-specific ones included; dev-dependencies are
never counted) must satisfy the stratum table and every crate rule, except
edges named in the exception list. The exception list must equal the
violations: a new violation and a stale row are both red. One row excuses
every rule its edge breaks.

Edges are the DECLARED dependencies of `cargo metadata --no-deps`, so an
optional dependency counts whether or not any feature activates it.

Reading a failure:
    UNCLASSIFIED <pkg>          a workspace package no stratum lists. Classify
                                it in the same diff that adds it; its edges are
                                not checked until it is classified.
    STALE stratum <S> lists <pkg>
                                the table classifies a package the workspace
                                no longer has. Remove it from the table.
    VIOLATION <a> -> <b>: <rule>
                                the edge breaks the named rule, one line per
                                rule it breaks. Remove the edge. An edge that
                                must stay for now needs an exception row naming
                                the tracking task that drops it.
    STALE exception <a> -> <b> (#task): <why>
                                the row's edge breaks no rule. "not in the
                                graph": delete the row in the diff that drops
                                the edge. "allowed": a table edit loosened the
                                rule the row stood for; delete the row or undo
                                that edit.

The gate runs from crates/reify-build-utils/tests/crate_dag_assertion.rs
(`cargo test -p reify-build-utils --test crate_dag_assertion`). PRD:
docs/prds/v0_6/kernel-interface-typed-seam.md §8 γ.

Exit codes: 0 the graph matches the table; 1 a finding above; 2 an input
(table or cargo metadata) could not be trusted.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from collections.abc import Mapping, Sequence
from typing import NoReturn
from dataclasses import dataclass
from enum import Enum, IntEnum
from pathlib import Path


class Verdict(IntEnum):
    OK = 0
    VIOLATION = 1
    INPUT_ERROR = 2


class InputError(Exception):
    """An input the check cannot trust. Always names the input."""


@dataclass(frozen=True, order=True)
class Edge:
    source: str
    target: str

    def __str__(self) -> str:
        return f"{self.source} -> {self.target}"


@dataclass(frozen=True)
class Stratum:
    name: str
    may_depend_on: frozenset[str]
    packages: frozenset[str]


@dataclass(frozen=True)
class ExceptionRow:
    edge: Edge
    task: int
    note: str


def braces(names: frozenset[str] | set[str]) -> str:
    return "{" + ", ".join(sorted(names)) + "}"


class RuleKind(Enum):
    ONLY = "only"
    NEVER = "never"


@dataclass(frozen=True)
class CrateRule:
    name: str
    sources: frozenset[str]
    kind: RuleKind
    targets: frozenset[str]

    def breaks(self, edge: Edge) -> bool:
        if edge.source not in self.sources:
            return False
        inside = edge.target in self.targets
        return not inside if self.kind is RuleKind.ONLY else inside

    def describe(self, edge: Edge) -> str:
        if self.kind is RuleKind.ONLY:
            return (f"crate rule {self.name}: {edge.source} may depend only on "
                    f"{braces(self.targets)}")
        return f"crate rule {self.name}: {edge.source} must not depend on {edge.target}"


@dataclass(frozen=True)
class Table:
    strata: Mapping[str, Stratum]
    stratum_of: Mapping[str, str]
    rules: tuple[CrateRule, ...]
    exceptions: Mapping[Edge, ExceptionRow]


@dataclass(frozen=True)
class Workspace:
    packages: frozenset[str]
    edges: frozenset[Edge]


TABLE_SCHEMA = 1
COUNTED_DEP_KINDS = frozenset({None, "build"})
SKIPPED_DEP_KINDS = frozenset({"dev"})


# --- reading inputs ---------------------------------------------------------

def read_json(path: Path, what: str) -> object:
    try:
        raw = path.read_text(encoding="utf-8")
    except FileNotFoundError:
        raise InputError(f"{path}: missing {what}") from None
    except (OSError, UnicodeDecodeError) as error:
        raise InputError(f"{path}: unreadable {what}: {error}") from None
    return parse_json(raw, f"{path}: {what}")


def parse_json(text: str, source: str) -> object:
    try:
        return json.loads(text)
    except json.JSONDecodeError as error:
        raise InputError(f"{source} is not JSON: {error}") from None


def fail(where: str, problem: str) -> NoReturn:
    raise InputError(f"{where}: {problem}")


def fields(obj: object, where: str, required: Sequence[str],
           optional: Sequence[str] = ()) -> dict:
    """`obj` as a dict holding every `required` key and no unknown key."""
    if not isinstance(obj, dict):
        fail(where, f"expected an object, found {obj!r}")
    problems = [*(f"unknown key {key!r}" for key in sorted(set(obj) - {*required, *optional})),
                *(f"missing key {key!r}" for key in required if key not in obj)]
    if problems:
        fail(where, "; ".join(problems))
    return obj


def text(value: object, where: str) -> str:
    if not isinstance(value, str) or not value:
        fail(where, f"expected a non-empty string, found {value!r}")
    return value


def str_list(value: object, where: str) -> tuple[str, ...]:
    if not isinstance(value, list):
        fail(where, f"expected a list of strings, found {value!r}")
    items = tuple(text(item, where) for item in value)
    twice = sorted({item for item in items if items.count(item) > 1})
    if twice:
        fail(where, f"lists {', '.join(twice)} twice")
    return items


def object_list(value: object, where: str) -> list:
    if not isinstance(value, list):
        fail(where, f"expected a list of objects, found {value!r}")
    return value


# --- the table --------------------------------------------------------------

def named_where(path: Path, noun: str, collection: str, index: int, obj: object) -> str:
    """`<path>: <noun> <name>` when `obj` has a readable name, else its index."""
    name = obj.get("name") if isinstance(obj, dict) else None
    if isinstance(name, str) and name:
        return f"{path}: {noun} {name}"
    return f"{path}: {collection}[{index}]"


def load_stratum(path: Path, index: int, obj: object) -> Stratum:
    where = named_where(path, "stratum", "strata", index, obj)
    entry = fields(obj, where, required=("name", "may_depend_on", "packages"))
    name = text(entry["name"], f"{where}: name")
    return Stratum(name=name,
                   may_depend_on=frozenset(str_list(entry["may_depend_on"],
                                                    f"{where}: may_depend_on")),
                   packages=frozenset(str_list(entry["packages"], f"{where}: packages")))


def layering_problem(stratum: Stratum, dependency: str, earlier: Sequence[str],
                     defined: frozenset[str]) -> str | None:
    if dependency == stratum.name:
        return "itself; same-stratum edges are always allowed, so drop it"
    if dependency not in defined:
        return "a stratum the table does not define"
    if dependency not in earlier:
        return (f"a stratum listed after {stratum.name}; a stratum may depend "
                f"only on strata listed before it")
    return None


def load_strata(path: Path, value: object) -> dict[str, Stratum]:
    """The strata in table order, each depending only on earlier ones."""
    strata: dict[str, Stratum] = {}
    for index, obj in enumerate(object_list(value, f"{path}: strata")):
        stratum = load_stratum(path, index, obj)
        if stratum.name in strata:
            fail(f"{path}: stratum {stratum.name}", "is defined twice")
        strata[stratum.name] = stratum
    defined = frozenset(strata)
    for position, stratum in enumerate(strata.values()):
        earlier = list(strata)[:position]
        for dependency in sorted(stratum.may_depend_on):
            problem = layering_problem(stratum, dependency, earlier, defined)
            if problem:
                fail(f"{path}: stratum {stratum.name}",
                     f"may_depend_on names {dependency}, {problem}")
    return strata


def classify(path: Path, strata: Mapping[str, Stratum]) -> dict[str, str]:
    stratum_of: dict[str, str] = {}
    for stratum in strata.values():
        for package in sorted(stratum.packages):
            if package in stratum_of:
                fail(f"{path}: package {package}",
                     f"is listed in stratum {stratum_of[package]} and in stratum "
                     f"{stratum.name}; a package sits in exactly one")
            stratum_of[package] = stratum.name
    return stratum_of


def resolve_package_set(obj: object, where: str, strata: Mapping[str, Stratum],
                        stratum_of: Mapping[str, str]) -> frozenset[str]:
    """The named packages plus every member of the named strata."""
    entry = fields(obj, where, required=(), optional=("packages", "strata"))
    if not entry:
        fail(where, "names neither packages nor strata")
    packages = str_list(entry.get("packages", []), f"{where}: packages")
    names = str_list(entry.get("strata", []), f"{where}: strata")
    for package in packages:
        if package not in stratum_of:
            fail(where, f"names {package}, which no stratum lists")
    for name in names:
        if name not in strata:
            fail(where, f"names stratum {name}, which the table does not define")
    return frozenset(packages).union(*(strata[name].packages for name in names))


def load_rule(path: Path, index: int, obj: object, strata: Mapping[str, Stratum],
              stratum_of: Mapping[str, str]) -> CrateRule:
    where = named_where(path, "crate rule", "crate_rules", index, obj)
    entry = fields(obj, where, required=("name", "from"),
                   optional=tuple(kind.value for kind in RuleKind))
    name = text(entry["name"], f"{where}: name")
    kinds = [kind for kind in RuleKind if kind.value in entry]
    if len(kinds) != 1:
        found = " and ".join(kind.value for kind in kinds) or "neither"
        fail(where, f"needs exactly one of only/never, found {found}")
    (kind,) = kinds
    return CrateRule(
        name=name,
        sources=resolve_package_set(entry["from"], f"{where}: from", strata, stratum_of),
        kind=kind,
        targets=resolve_package_set(entry[kind.value], f"{where}: {kind.value}",
                                    strata, stratum_of))


def load_rules(path: Path, value: object, strata: Mapping[str, Stratum],
               stratum_of: Mapping[str, str]) -> tuple[CrateRule, ...]:
    rules: dict[str, CrateRule] = {}
    for index, obj in enumerate(object_list(value, f"{path}: crate_rules")):
        rule = load_rule(path, index, obj, strata, stratum_of)
        if rule.name in rules:
            fail(f"{path}: crate rule {rule.name}", "is defined twice")
        rules[rule.name] = rule
    return tuple(rules.values())


def load_exception(path: Path, index: int, obj: object,
                   stratum_of: Mapping[str, str]) -> ExceptionRow:
    entry = fields(obj, f"{path}: exceptions[{index}]",
                   required=("from", "to", "task", "note"))
    edge = Edge(text(entry["from"], f"{path}: exceptions[{index}]: from"),
                text(entry["to"], f"{path}: exceptions[{index}]: to"))
    where = f"{path}: exception {edge}"
    for package in (edge.source, edge.target):
        if package not in stratum_of:
            fail(where, f"names {package}, which no stratum lists")
    task = entry["task"]
    if type(task) is not int or task <= 0:
        fail(where, f"task must be a positive integer task id, found {task!r}")
    return ExceptionRow(edge=edge, task=task, note=text(entry["note"], f"{where}: note"))


def load_exceptions(path: Path, value: object,
                    stratum_of: Mapping[str, str]) -> dict[Edge, ExceptionRow]:
    rows: dict[Edge, ExceptionRow] = {}
    for index, obj in enumerate(object_list(value, f"{path}: exceptions")):
        row = load_exception(path, index, obj, stratum_of)
        if row.edge in rows:
            fail(f"{path}: exception {row.edge}", "is listed twice")
        rows[row.edge] = row
    return rows


def load_table(path: Path) -> Table:
    document = fields(read_json(path, "strata table"), str(path),
                      required=("schema", "strata", "crate_rules", "exceptions"))
    schema = document["schema"]
    if type(schema) is not int or schema != TABLE_SCHEMA:
        fail(f"{path}: schema", f"is {schema!r}; this checker reads schema {TABLE_SCHEMA}")
    strata = load_strata(path, document["strata"])
    stratum_of = classify(path, strata)
    return Table(strata=strata, stratum_of=stratum_of,
                 rules=load_rules(path, document["crate_rules"], strata, stratum_of),
                 exceptions=load_exceptions(path, document["exceptions"], stratum_of))


# --- the workspace ----------------------------------------------------------

def read_metadata(root: Path, metadata_path: Path | None) -> object:
    if metadata_path is not None:
        return read_json(metadata_path, "cargo metadata")
    command = [os.environ.get("CARGO", "cargo"), "metadata", "--no-deps",
               "--format-version", "1", "--manifest-path", str(root / "Cargo.toml")]
    result = subprocess.run(command, capture_output=True, text=True)
    if result.returncode != 0:
        tail = "\n".join(result.stderr.strip().splitlines()[-5:])
        raise InputError(f"`{' '.join(command)}` exited {result.returncode}: {tail}")
    return parse_json(result.stdout, f"`{' '.join(command)}` output")


def counted(package: dict, dependency: dict, source: str) -> bool:
    """Whether a dependency's kind is one the check counts (normal, build)."""
    kind = dependency["kind"]
    if kind in COUNTED_DEP_KINDS:
        return True
    if kind in SKIPPED_DEP_KINDS:
        return False
    fail(f"{source}: {package['name']} -> {dependency['name']}",
         f"unknown dependency kind {kind!r}; it is neither counted nor skipped")


def workspace_of(metadata: object, source: str) -> Workspace:
    try:
        member_ids = frozenset(metadata["workspace_members"])
        members = [package for package in metadata["packages"]
                   if package["id"] in member_ids]
        names = frozenset(package["name"] for package in members)
        edges = frozenset(
            Edge(package["name"], dependency["name"])
            for package in members
            for dependency in package["dependencies"]
            if counted(package, dependency, source)
            and dependency["name"] in names
            and dependency["name"] != package["name"]
        )
    except (KeyError, TypeError) as error:
        raise InputError(f"{source}: malformed cargo metadata: {error!r}") from None
    return Workspace(packages=names, edges=edges)


# --- the check --------------------------------------------------------------

def broken_rules(table: Table, edge: Edge) -> list[str]:
    source = table.strata[table.stratum_of[edge.source]]
    target_stratum = table.stratum_of[edge.target]
    reasons = []
    if target_stratum != source.name and target_stratum not in source.may_depend_on:
        reasons.append(
            f"stratum rule: {source.name} may depend only on "
            f"{braces(source.may_depend_on | {source.name})}; "
            f"{edge.target} is {target_stratum}")
    reasons.extend(rule.describe(edge) for rule in table.rules if rule.breaks(edge))
    return reasons


def stale_exception(row: ExceptionRow, ws: Workspace) -> str:
    if row.edge in ws.edges:
        why = "the edge is allowed — delete the row, or undo the table edit that allowed it"
    else:
        why = "the edge is not in the graph — delete the row in the diff that drops the edge"
    return f"STALE exception {row.edge} (#{row.task}): {why}"


def findings(table: Table, ws: Workspace) -> list[str]:
    classified = frozenset(table.stratum_of)
    unclassified = sorted(ws.packages - classified)
    stale_strata = sorted((table.stratum_of[package], package)
                          for package in classified - ws.packages)
    checked = sorted(edge for edge in ws.edges
                     if edge.source in classified and edge.target in classified)
    reasons = {edge: broken_rules(table, edge) for edge in checked}
    violating = frozenset(edge for edge, why in reasons.items() if why)
    return [
        *(f"UNCLASSIFIED {package}: no stratum in the table lists it — add it to one"
          for package in unclassified),
        *(f"STALE stratum {stratum} lists {package}, which is not a workspace "
          f"package — remove it" for stratum, package in stale_strata),
        *(f"VIOLATION {edge}: {reason}"
          for edge in checked if edge not in table.exceptions
          for reason in reasons[edge]),
        *(stale_exception(table.exceptions[edge], ws)
          for edge in sorted(table.exceptions) if edge not in violating),
    ]


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1],
                        help="workspace root (default: this checkout)")
    parser.add_argument("--table", type=Path,
                        help="strata table (default: <root>/scripts/crate-strata.json)")
    parser.add_argument("--metadata", type=Path,
                        help="a saved `cargo metadata --no-deps --format-version 1` "
                             "(default: run cargo over <root>/Cargo.toml)")
    return parser.parse_args(argv)


def run(root: Path, table_path: Path, metadata_path: Path | None) -> Verdict:
    table = load_table(table_path)
    source = str(metadata_path) if metadata_path is not None else "cargo metadata"
    ws = workspace_of(read_metadata(root, metadata_path), source)
    lines = findings(table, ws)
    if lines:
        print("\n".join(lines))
        print("crate-strata: FAIL — python3 scripts/assert-crate-strata.py --help "
              "explains each line")
        return Verdict.VIOLATION
    print(f"crate-strata: OK ({len(ws.packages)} packages in {len(table.strata)} strata, "
          f"{len(ws.edges)} edges, {len(table.exceptions)} excepted)")
    return Verdict.OK


def main(argv: Sequence[str]) -> int:
    args = parse_args(argv)
    table_path = args.table or args.root / "scripts" / "crate-strata.json"
    try:
        return run(args.root, table_path, args.metadata)
    except (InputError, OSError) as error:
        print(f"assert-crate-strata: {error}", file=sys.stderr)
        return Verdict.INPUT_ERROR


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
