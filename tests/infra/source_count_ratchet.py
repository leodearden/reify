#!/usr/bin/env python3
"""Count line constructs in Rust production files against an exact baseline.

The engine behind the C7 ratchet members of PRD
docs/prds/v0_6/compiler-module-namespace.md. Production files are every *.rs
under a source root except the files of a column-0 `mod NAME;` whose attribute
block directly above holds `#[cfg(test)]`; any other declaration shape counts as
production. Each counter counts the raw production lines its pattern matches,
and a count above or below its baseline row is a violation.
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


@dataclass(frozen=True)
class BaselineRow:
    name: str
    count: int
    line: int


@dataclass(frozen=True)
class Report:
    counts: tuple[tuple[str, int], ...]
    hits: Mapping[str, tuple[Site, ...]]
    violations: tuple[str, ...]
    skipped: tuple[CfgTestChild, ...]

    @property
    def count_lines(self) -> tuple[str, ...]:
        return tuple(f"{name} {count}" for name, count in self.counts)


ROW = re.compile(r"([a-z][a-z0-9_]*) (0|[1-9][0-9]*)")
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


def baseline_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as exc:
        raise RatchetInputError(f"{path}: cannot read: {exc}") from None


def parse_row(path: Path, number: int, line: str, names: Sequence[str]) -> BaselineRow:
    row = ROW.fullmatch(line)
    if not row:
        raise RatchetInputError(f"{path}:{number}: expected '<counter> <count>', got {line!r}")
    if row.group(1) not in names:
        raise RatchetInputError(f"{path}:{number}: unknown counter '{row.group(1)}'")
    return BaselineRow(row.group(1), int(row.group(2)), number)


def load_baseline(path: Path, names: Sequence[str]) -> dict[str, BaselineRow]:
    rows: dict[str, BaselineRow] = {}
    for number, raw in enumerate(baseline_text(path).split("\n"), start=1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        row = parse_row(path, number, line, names)
        if row.name in rows:
            raise RatchetInputError(
                f"{path}:{number}: duplicate row for counter '{row.name}' "
                f"(first at line {rows[row.name].line})")
        rows[row.name] = row
    for name in names:
        if name not in rows:
            raise RatchetInputError(f"{path}: no row for counter '{name}'")
    return rows


def display(path: Path, root: Path) -> str:
    return path.relative_to(root).as_posix() if path.is_relative_to(root) else str(path)


def verdict(row: BaselineRow, sites: Sequence[Site], baseline: str) -> str | None:
    count, where = len(sites), f"{baseline}:{row.line}"
    stated = f"baseline row '{row.name} {row.count}' ({where})"
    if count > row.count:
        every_line = "\n".join(f"  {site}" for site in sites)
        return f"{row.name}: {count} counted, above {stated}; every counted line:\n{every_line}"
    if count < row.count:
        return (f"{row.name}: {count} counted, below {stated}: stale row; "
                f"lower it to '{row.name} {count}' in the same diff")
    return None


def check(root: Path, src_roots: Sequence[Path], counters: Sequence[Counter],
          baseline: Path) -> Report:
    names = [counter.name for counter in counters]
    rows = load_baseline(baseline, names)
    found = census(root, src_roots, counters)
    where = display(baseline, root)
    verdicts = (verdict(rows[name], found.hits[name], where) for name in names)
    return Report(
        counts=tuple((name, len(found.hits[name])) for name in names),
        hits=found.hits,
        violations=tuple(violation for violation in verdicts if violation),
        skipped=found.skipped,
    )
