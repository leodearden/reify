#!/usr/bin/env python3
"""Count source constructs in Rust production files against an exact baseline.

The engine behind the C7 ratchet members of PRD
docs/prds/v0_6/compiler-module-namespace.md. Every rule reads code text: a
file with its `//` comments removed. The removal is lexical, so `/* */` comments
and string, char and raw-string literals stay in code text and every rule reads
their contents as code, and a `//` inside a literal removes the rest of its line.
Production files are every *.rs under a source root except the files of a
column-0 `mod NAME;` whose attributes include `#[cfg(test)]` and no `#[path]`,
read from its own line and from the attribute and blank lines directly above
it; any other declaration shape counts as production. Each counter counts the
production lines on which a match of its pattern starts, and a count above or
below its baseline row is a violation.
"""

from __future__ import annotations

import re
from collections.abc import Iterator, Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path
from types import MappingProxyType


class RatchetInputError(Exception):
    """An input the ratchet cannot trust; the message names the input."""


@dataclass(frozen=True, order=True)
class Site:
    path: str
    line: int

    def __str__(self) -> str:
        return f"{self.path}:{self.line}"


@dataclass(frozen=True)
class Counter:
    """`pattern` is searched MULTILINE over a file's whole code text, so `^` anchors
    at every line start and a match may run onto the lines that follow. `allowed`
    holds (root-relative path, stripped code of the line a match starts on)."""

    name: str
    pattern: str
    allowed: frozenset[tuple[str, str]] = frozenset()

    def sites(self, rel: str, text: str) -> Iterator[Site]:
        lines = text.split("\n")
        for match in re.finditer(self.pattern, text, re.MULTILINE):
            number = text.count("\n", 0, match.start()) + 1
            if (rel, lines[number - 1].strip()) not in self.allowed:
                yield Site(rel, number)


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
    violations: tuple[str, ...]
    skipped: tuple[CfgTestChild, ...]

    @property
    def count_lines(self) -> tuple[str, ...]:
        return tuple(f"{name} {count}" for name, count in self.counts)


ROW = re.compile(r"([a-z][a-z0-9_]*) (0|[1-9][0-9]*)")
MOD_ROOTS = frozenset({"lib.rs", "main.rs", "mod.rs"})
LINE_COMMENT = re.compile(r"//[^\n]*")
MOD_DECL = re.compile(
    r"^((?P<attributes>#\[.*\])\s*)?(pub(\([^)]*\))?\s+)?mod\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*;")
ATTRIBUTE_LINE = re.compile(r"^#\[.*\]\s*$")
ATTRIBUTE_BOUNDARY = re.compile(r"(?<=\])\s*(?=#\[)")
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


def code_text(path: Path) -> str:
    return LINE_COMMENT.sub("", path.read_text(encoding="utf-8", errors="replace"))


def rust_files(src: Path) -> list[Path]:
    if not src.is_dir():
        raise RatchetInputError(f"{src}: source root is not a directory")
    return sorted(src.rglob("*.rs"))


def moddir(path: Path) -> Path:
    return path.parent if path.name in MOD_ROOTS else path.parent / path.stem


def attributes_in(text: str) -> list[str]:
    return [attribute for attribute in ATTRIBUTE_BOUNDARY.split(text.strip()) if attribute]


def attributes_above(lines: Sequence[str], index: int) -> list[str]:
    attributes: list[str] = []
    for line in reversed(lines[:index]):
        if ATTRIBUTE_LINE.match(line):
            attributes += attributes_in(line)
        elif line.strip():
            break
    return attributes


def is_cfg_test(attribute: str) -> bool:
    return re.sub(r"\s+", "", attribute) == "#[cfg(test)]"


def is_cfg_test_block(attributes: Sequence[str]) -> bool:
    return (any(is_cfg_test(attribute) for attribute in attributes)
            and not any(PATH_ATTRIBUTE.match(attribute) for attribute in attributes))


def cfg_test_declarations(lines: Sequence[str]) -> Iterator[tuple[str, int]]:
    for index, line in enumerate(lines):
        declaration = MOD_DECL.match(line)
        if declaration:
            own_line = attributes_in(declaration["attributes"] or "")
            if is_cfg_test_block([*attributes_above(lines, index), *own_line]):
                yield declaration["name"], index + 1


def cfg_test_modules(root: Path, path: Path, text: str) -> Iterator[CfgTestModule]:
    for name, line in cfg_test_declarations(text.split("\n")):
        yield CfgTestModule(moddir(path) / name, Site(relative(root, path), line))


def cfg_test_children(root: Path, texts: Mapping[Path, str]) -> tuple[CfgTestChild, ...]:
    modules = [module for path, text in texts.items()
               for module in cfg_test_modules(root, path, text)]
    children = []
    for file in texts:
        covering = [module for module in modules if module.covers(file)]
        if covering:
            outermost = min(covering, key=lambda module: len(module.directory.parts))
            children.append(CfgTestChild(relative(root, file), outermost.declared_at))
    return tuple(sorted(children))


def census(root: Path, src_roots: Sequence[Path], counters: Sequence[Counter]) -> Census:
    texts = {path: code_text(path) for src in src_roots for path in rust_files(src)}
    skipped = cfg_test_children(root, texts)
    excluded = {child.path for child in skipped}
    sites: dict[str, set[Site]] = {counter.name: set() for counter in counters}
    for path, text in texts.items():
        rel = relative(root, path)
        if rel not in excluded:
            for counter in counters:
                sites[counter.name].update(counter.sites(rel, text))
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
        violations=tuple(violation for violation in verdicts if violation),
        skipped=found.skipped,
    )
