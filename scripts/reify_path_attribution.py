"""Independent per-path facts for routing a changed repo path.

``attribute(path)`` returns a ``PathFacts``: whether the path is a C4
workspace global, the crates it seeds, the crates it unions into a closure's
result (a manifest's crate-DAG gate), whether it is inert (documentation or
configuration), and which tree it lies in. The facts are independent;
precedence between them belongs to each consumer. Rules and rationale:
docs/prds/verify-scope-contract.md §3.

CLI, run once per consumer invocation over its whole path list, never once
per path:

    attribute [--format tsv|json] < paths    one record per input line, in order

Stdlib-only, so the gate's /usr/bin/python3 (no dark-factory venv) runs it,
including under ``-S``.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections.abc import Callable, Iterable, Sequence
from dataclasses import dataclass
from enum import StrEnum
from fnmatch import fnmatchcase
from typing import TextIO

RI_CORPUS_CRATES: frozenset[str] = frozenset(
    {
        "reify-cli",
        "reify-compiler",
        "reify-eval",
        "reify-eval-fea-tests",
        "reify-gui",
        "reify-test-support",
    }
)

DAG_GATE_CRATE = "reify-build-utils"


class Tree(StrEnum):
    CRATES = "crates"
    GUI_TAURI = "gui_tauri"
    GUI_SRC = "gui_src"
    GUI_OTHER = "gui_other"
    TESTS_INFRA = "tests_infra"
    EXAMPLES = "examples"
    OTHER = "other"


# Globs are matched with fnmatchcase: `*` spans `/` and the match is
# case-sensitive, like a bash `case` glob.
_GLOBAL_GLOBS = (
    "Cargo.toml",
    "Cargo.lock",
    ".cargo/*",
    "tree-sitter-reify/*",
    "rust-toolchain*",
)
_CRATE_MANIFEST_GLOBS = ("crates/*/Cargo.toml", "gui/src-tauri/Cargo.toml")
_CRATE_DIR_GLOB = "crates/*/*"
_GUI_CRATE_GLOB = "gui/src-tauri/*"
_GUI_CRATE = "reify-gui"
_RI_CORPUS_GLOB = "examples/*.ri"
_INERT_GLOBS = ("docs/*", "*.md", "*.yaml", "*.yml")
# First match wins.
_TREE_GLOBS: tuple[tuple[str, Tree], ...] = (
    ("crates/*", Tree.CRATES),
    (_GUI_CRATE_GLOB, Tree.GUI_TAURI),
    ("gui/src/*", Tree.GUI_SRC),
    ("gui/*", Tree.GUI_OTHER),
    ("tests/infra/*", Tree.TESTS_INFRA),
    ("examples/*", Tree.EXAMPLES),
)


@dataclass(frozen=True)
class PathFacts:
    """What one path contributes: a C4 global, closure seeds, crates unioned
    into the closure's result without being seeded into it, the inert class,
    and the tree the path lies in."""

    is_global: bool
    seed_crates: frozenset[str]
    unioned_crates: frozenset[str]
    inert: bool
    tree: Tree


def attribute(path: str) -> PathFacts:
    """Classify one repo-relative path."""
    return PathFacts(
        is_global=_matches_any(path, _GLOBAL_GLOBS),
        seed_crates=_seed_crates(path),
        unioned_crates=_unioned_crates(path),
        inert=_matches_any(path, _INERT_GLOBS),
        tree=_tree(path),
    )


def _matches_any(path: str, globs: Iterable[str]) -> bool:
    return any(fnmatchcase(path, glob) for glob in globs)


def _seed_crates(path: str) -> frozenset[str]:
    """The first matching rule wins."""
    if fnmatchcase(path, _CRATE_DIR_GLOB):
        # `crates//x` matches with an empty name, which seeds nothing.
        name = path.removeprefix("crates/").split("/", 1)[0]
        return frozenset({name}) if name else frozenset()
    if fnmatchcase(path, _GUI_CRATE_GLOB):
        return frozenset({_GUI_CRATE})
    if fnmatchcase(path, _RI_CORPUS_GLOB):
        return RI_CORPUS_CRATES
    return frozenset()


def _unioned_crates(path: str) -> frozenset[str]:
    """A per-crate manifest contributes the crate-DAG gate's host crate."""
    if _matches_any(path, _CRATE_MANIFEST_GLOBS):
        return frozenset({DAG_GATE_CRATE})
    return frozenset()


def _tree(path: str) -> Tree:
    for glob, tree in _TREE_GLOBS:
        if fnmatchcase(path, glob):
            return tree
    return Tree.OTHER


_TSV_EMPTY_SET = "-"


def _tsv_crates(crates: frozenset[str]) -> str:
    return ",".join(sorted(crates)) or _TSV_EMPTY_SET


def _tsv_record(path: str, facts: PathFacts) -> str:
    """`<is_global>\\t<inert>\\t<tree>\\t<seed_crates>\\t<unioned_crates>\\t<path>`:
    0/1 flags, sorted comma-joined crate sets (`-` when empty), path last."""
    return "\t".join(
        (
            str(int(facts.is_global)),
            str(int(facts.inert)),
            facts.tree.value,
            _tsv_crates(facts.seed_crates),
            _tsv_crates(facts.unioned_crates),
            path,
        )
    )


def _json_record(path: str, facts: PathFacts) -> str:
    return json.dumps(
        {
            "path": path,
            "is_global": facts.is_global,
            "inert": facts.inert,
            "tree": facts.tree.value,
            "seed_crates": sorted(facts.seed_crates),
            "unioned_crates": sorted(facts.unioned_crates),
        }
    )


_RECORD_FORMATS: dict[str, Callable[[str, PathFacts], str]] = {
    "tsv": _tsv_record,
    "json": _json_record,
}


def _write_attributions(
    record: Callable[[str, PathFacts], str], paths: TextIO, out: TextIO
) -> None:
    for line in paths:
        path = line.rstrip("\n")
        out.write(record(path, attribute(path)) + "\n")


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="reify_path_attribution.py")
    verbs = parser.add_subparsers(dest="verb", required=True)
    attribute_verb = verbs.add_parser(
        "attribute", help="per-path facts for the paths on stdin, one record per line"
    )
    attribute_verb.add_argument(
        "--format", choices=sorted(_RECORD_FORMATS), default="tsv"
    )
    args = parser.parse_args(argv)
    _write_attributions(_RECORD_FORMATS[args.format], sys.stdin, sys.stdout)
    return 0


if __name__ == "__main__":
    sys.exit(main())
