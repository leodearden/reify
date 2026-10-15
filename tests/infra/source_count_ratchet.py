#!/usr/bin/env python3
"""Count line constructs in Rust production files against an exact baseline.

The engine behind the C7 ratchet members of PRD
docs/prds/v0_6/compiler-module-namespace.md. Production files are every *.rs
under a source root except the files of a column-0 `mod NAME;` whose attribute
block directly above holds `#[cfg(test)]`; any other declaration shape counts as
production. Each counter counts the raw production lines its pattern matches.
"""

from __future__ import annotations

import re
from collections.abc import Iterator, Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path
from types import MappingProxyType


class RatchetInputError(Exception):
    """An input the ratchet cannot trust; the message names the input."""


@dataclass(frozen=True)
class Counter:
    name: str
    pattern: re.Pattern[str]
    allowed: frozenset[tuple[str, str]] = frozenset()

    def counts(self, rel: str, line: str) -> bool:
        return bool(self.pattern.search(line)) and (rel, line.strip()) not in self.allowed


@dataclass(frozen=True, order=True)
class Site:
    path: str
    line: int

    def __str__(self) -> str:
        return f"{self.path}:{self.line}"


@dataclass(frozen=True, order=True)
class CfgTestChild:
    path: str
    declared_at: Site


@dataclass(frozen=True)
class Census:
    hits: Mapping[str, tuple[Site, ...]]
    skipped: tuple[CfgTestChild, ...]


MOD_ROOTS = frozenset({"lib.rs", "main.rs", "mod.rs"})
MOD_DECL = re.compile(r"^(pub(\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;")
ATTRIBUTE_LINE = re.compile(r"^#\[.*\]\s*$")
PATH_ATTRIBUTE = re.compile(r"^#\[\s*path\s*=")


@dataclass(frozen=True)
class CfgTestModule:
    directory: Path
    declared_at: Site

    def covers(self, file: Path) -> bool:
        own_file = self.directory.with_name(f"{self.directory.name}.rs")
        return file == own_file or self.directory in file.parents


def relative(root: Path, path: Path) -> str:
    return path.relative_to(root).as_posix()


def source_lines(path: Path) -> list[str]:
    return path.read_text(encoding="utf-8", errors="replace").split("\n")


def rust_files(src: Path) -> list[Path]:
    if not src.is_dir():
        raise RatchetInputError(f"{src}: source root is not a directory")
    return sorted(src.rglob("*.rs"))


def moddir(path: Path) -> Path:
    return path.parent if path.name in MOD_ROOTS else path.parent / path.stem


def is_cfg_test(line: str) -> bool:
    return re.sub(r"\s+", "", line) == "#[cfg(test)]"


def attribute_block(lines: Sequence[str], index: int) -> list[str]:
    start = index
    while start > 0 and ATTRIBUTE_LINE.match(lines[start - 1]):
        start -= 1
    return list(lines[start:index])


def is_cfg_test_block(block: Sequence[str]) -> bool:
    return (any(is_cfg_test(line) for line in block)
            and not any(PATH_ATTRIBUTE.match(line) for line in block))


def cfg_test_declarations(lines: Sequence[str]) -> Iterator[tuple[str, int]]:
    for index, line in enumerate(lines):
        declaration = MOD_DECL.match(line)
        if declaration and is_cfg_test_block(attribute_block(lines, index)):
            yield declaration.group(3), index + 1


def cfg_test_modules(root: Path, path: Path) -> Iterator[CfgTestModule]:
    for name, line in cfg_test_declarations(source_lines(path)):
        yield CfgTestModule(moddir(path) / name, Site(relative(root, path), line))


def cfg_test_children(root: Path, src: Path) -> tuple[CfgTestChild, ...]:
    files = rust_files(src)
    modules = [module for path in files for module in cfg_test_modules(root, path)]
    children = []
    for file in files:
        covering = [module for module in modules if module.covers(file)]
        if covering:
            outermost = min(covering, key=lambda module: len(module.directory.parts))
            children.append(CfgTestChild(relative(root, file), outermost.declared_at))
    return tuple(sorted(children))


def production_files(root: Path, src: Path, skipped: Sequence[CfgTestChild]) -> list[Path]:
    excluded = {child.path for child in skipped}
    return [path for path in rust_files(src) if relative(root, path) not in excluded]


def file_sites(rel: str, lines: Sequence[str], counter: Counter) -> Iterator[Site]:
    for number, line in enumerate(lines, start=1):
        if counter.counts(rel, line):
            yield Site(rel, number)


def census(root: Path, src_roots: Sequence[Path], counters: Sequence[Counter]) -> Census:
    skipped = tuple(child for src in src_roots for child in cfg_test_children(root, src))
    sites: dict[str, list[Site]] = {counter.name: [] for counter in counters}
    for src in src_roots:
        for path in production_files(root, src, skipped):
            rel, lines = relative(root, path), source_lines(path)
            for counter in counters:
                sites[counter.name].extend(file_sites(rel, lines, counter))
    hits = {name: tuple(sorted(found)) for name, found in sites.items()}
    return Census(MappingProxyType(hits), skipped)
