"""Per-path routing facts and the compile closure: the one home of both.

``attribute(path)`` returns a ``PathFacts``: whether the path is a C4
workspace global, the crates it seeds, the crates it unions into a closure's
result (a manifest's crate-DAG gate), whether it is inert (documentation or
configuration), and which tree it lies in. The facts are independent;
precedence between them belongs to each consumer. ``compile_closure`` is the
cargo-accurate compile closure over ``cargo metadata``. Rules and rationale:
docs/prds/verify-scope-contract.md §3.

Consumers:
  - scripts/affected-crates-lib.sh — affected_crates, direct_crates_per_path
    and reify_is_inert_path (verify.sh's decide_scope), via ``attribute``;
  - scripts/occt-scope-lib.sh — _reify_compile_closure (occt_touching_set and
    affected_crates' reverse closure), via ``closure``;
  - scripts/reify_overlap_detector.py — imports ``attribute``.

CLI, run once per consumer invocation over its whole input, never once per
path:

    attribute [--format tsv|json] < paths    one record per input line, in order
    closure SEED... < metadata.json          affected member names, one per line

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


def compile_closure(metadata: dict, seeds: Iterable[str]) -> frozenset[str]:
    """The workspace members whose test compilation reaches a seed crate.

    ``metadata`` is ``cargo metadata --format-version 1`` output and seeds are
    crate NAMES; a name no package carries is skipped. A member's test
    compilation is the normal/build closure of the member itself plus that of
    each of its DIRECT dev-deps: cargo does not propagate dev-deps
    transitively. No try/except: malformed metadata raises, and each caller
    applies its own error policy.
    """
    seed_names = set(seeds)

    members = set(metadata["workspace_members"])
    id_to_name = {p["id"]: p["name"] for p in metadata["packages"]}
    name_to_ids: dict[str, list[str]] = {}
    for p in metadata["packages"]:
        name_to_ids.setdefault(p["name"], []).append(p["id"])

    # dep_kinds[].kind: null -> normal, 'build' -> build dep, 'dev' -> dev dep.
    # They must NOT be conflated: dev-deps of a transitive dep are never
    # compiled when testing a crate that only has a normal dep on it.
    adj_normal: dict[str, set[str]] = {}  # null or 'build': compiled transitively
    adj_dev: dict[str, set[str]] = {}  # 'dev': only the tested crate's DIRECT ones
    for node in metadata["resolve"]["nodes"]:
        adj_normal[node["id"]] = set()
        adj_dev[node["id"]] = set()
        for d in node["deps"]:
            kinds = {dk.get("kind") for dk in d.get("dep_kinds", [])}
            if None in kinds or "build" in kinds:
                adj_normal[node["id"]].add(d["pkg"])
            if "dev" in kinds:
                adj_dev[node["id"]].add(d["pkg"])

    def normal_closure(start: str) -> set[str]:
        """All packages reachable via normal/build edges only."""
        visited: set[str] = set()
        queue = [start]
        while queue:
            curr = queue.pop()
            if curr in visited:
                continue
            visited.add(curr)
            queue.extend(adj_normal.get(curr, []))
        return visited

    seed_ids: set[str] = set()
    for sn in seed_names:
        seed_ids.update(name_to_ids.get(sn, []))

    result = []
    for pkg_id in members:
        compiled = normal_closure(pkg_id)
        for dev_dep_id in adj_dev.get(pkg_id, []):
            compiled |= normal_closure(dev_dep_id)
        if compiled & seed_ids:
            result.append(id_to_name[pkg_id])

    return frozenset(result)


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


def _write_closure(seeds: Iterable[str], metadata: TextIO, out: TextIO) -> None:
    for name in sorted(compile_closure(json.load(metadata), seeds)):
        out.write(name + "\n")


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="reify_path_attribution.py")
    verbs = parser.add_subparsers(dest="verb", required=True)
    attribute_verb = verbs.add_parser(
        "attribute", help="per-path facts for the paths on stdin, one record per line"
    )
    attribute_verb.add_argument(
        "--format", choices=sorted(_RECORD_FORMATS), default="tsv"
    )
    attribute_verb.set_defaults(
        run=lambda args: _write_attributions(
            _RECORD_FORMATS[args.format], sys.stdin, sys.stdout
        )
    )
    closure_verb = verbs.add_parser(
        "closure",
        help="workspace members whose test compilation reaches a seed, "
        "over `cargo metadata` JSON on stdin",
    )
    closure_verb.add_argument("seeds", nargs="*", metavar="SEED")
    closure_verb.set_defaults(
        run=lambda args: _write_closure(args.seeds, sys.stdin, sys.stdout)
    )
    args = parser.parse_args(argv)
    args.run(args)
    return 0


if __name__ == "__main__":
    sys.exit(main())
