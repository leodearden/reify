#!/usr/bin/env python3
"""
assert-crate-strata.py — the workspace crate strata assertion.

The rule: every workspace package sits in exactly one stratum of
scripts/crate-strata.json. Every normal or build edge between workspace
packages (optional and target-specific ones included; dev-dependencies are
never counted) must satisfy the stratum table, except edges named in the
exception list. The exception list must equal the violations: a new violation
and a stale row are both red.

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
from dataclasses import dataclass
from enum import IntEnum
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


@dataclass(frozen=True)
class Table:
    strata: Mapping[str, Stratum]
    stratum_of: Mapping[str, str]
    exceptions: Mapping[Edge, ExceptionRow]


@dataclass(frozen=True)
class Workspace:
    packages: frozenset[str]
    edges: frozenset[Edge]


COUNTED_DEP_KINDS = frozenset({None, "build"})


# --- the table --------------------------------------------------------------

def read_json(path: Path, what: str) -> object:
    try:
        text = path.read_text(encoding="utf-8")
    except FileNotFoundError:
        raise InputError(f"{path}: missing {what}") from None
    except (OSError, UnicodeDecodeError) as error:
        raise InputError(f"{path}: unreadable {what}: {error}") from None
    return parse_json(text, f"{path}: {what}")


def parse_json(text: str, source: str) -> object:
    try:
        return json.loads(text)
    except json.JSONDecodeError as error:
        raise InputError(f"{source} is not JSON: {error}") from None


def load_stratum(obj: dict) -> Stratum:
    return Stratum(name=obj["name"],
                   may_depend_on=frozenset(obj["may_depend_on"]),
                   packages=frozenset(obj["packages"]))


def load_exception(obj: dict) -> ExceptionRow:
    return ExceptionRow(edge=Edge(obj["from"], obj["to"]),
                        task=obj["task"], note=obj["note"])


def load_table(path: Path) -> Table:
    document = read_json(path, "strata table")
    try:
        strata = [load_stratum(obj) for obj in document["strata"]]
        if not isinstance(document["crate_rules"], list):
            raise InputError(f"{path}: crate_rules must be a list")
        exceptions = [load_exception(obj) for obj in document["exceptions"]]
    except (KeyError, TypeError) as error:
        raise InputError(f"{path}: malformed strata table: {error!r}") from None
    return Table(
        strata={stratum.name: stratum for stratum in strata},
        stratum_of={package: stratum.name
                    for stratum in strata for package in stratum.packages},
        exceptions={row.edge: row for row in exceptions},
    )


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
            if dependency["name"] in names
            and dependency["name"] != package["name"]
            and dependency["kind"] in COUNTED_DEP_KINDS
        )
    except (KeyError, TypeError) as error:
        raise InputError(f"{source}: malformed cargo metadata: {error!r}") from None
    return Workspace(packages=names, edges=edges)


# --- the check --------------------------------------------------------------

def braces(names: frozenset[str] | set[str]) -> str:
    return "{" + ", ".join(sorted(names)) + "}"


def broken_rules(table: Table, edge: Edge) -> list[str]:
    source = table.strata[table.stratum_of[edge.source]]
    target_stratum = table.stratum_of[edge.target]
    reasons = []
    if target_stratum != source.name and target_stratum not in source.may_depend_on:
        reasons.append(
            f"stratum rule: {source.name} may depend only on "
            f"{braces(source.may_depend_on | {source.name})}; "
            f"{edge.target} is {target_stratum}")
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
