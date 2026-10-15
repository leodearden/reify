#!/usr/bin/env python3
"""Count line constructs in Rust production files against an exact baseline.

The engine behind the C7 ratchet members of PRD
docs/prds/v0_6/compiler-module-namespace.md. Production files are every *.rs
under a source root. Each counter counts the raw lines its pattern matches.
"""

from __future__ import annotations

import re
from collections.abc import Mapping, Sequence
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


@dataclass(frozen=True)
class Census:
    hits: Mapping[str, tuple[Site, ...]]


def source_lines(path: Path) -> list[str]:
    return path.read_text(encoding="utf-8", errors="replace").split("\n")


def rust_files(src: Path) -> list[Path]:
    if not src.is_dir():
        raise RatchetInputError(f"{src}: source root is not a directory")
    return sorted(src.rglob("*.rs"))


def production_files(src: Path) -> list[Path]:
    return rust_files(src)


def census(root: Path, src_roots: Sequence[Path], counters: Sequence[Counter]) -> Census:
    sites: dict[str, list[Site]] = {counter.name: [] for counter in counters}
    for src in src_roots:
        for path in production_files(src):
            rel = path.relative_to(root).as_posix()
            for number, line in enumerate(source_lines(path), start=1):
                for counter in counters:
                    if counter.counts(rel, line):
                        sites[counter.name].append(Site(rel, number))
    return Census(MappingProxyType({name: tuple(sorted(found)) for name, found in sites.items()}))
