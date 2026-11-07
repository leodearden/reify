#!/usr/bin/env python3
"""Evaluate adoption-ratchet.manifest rows against the tree.

PRD docs/prds/v0_6/run-all-runner-infra-harness.md §3 D9. The row grammar lives
in the manifest's header; test_infra_adoption_ratchet.sh is the gate member.
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path, PurePosixPath

DEFAULT_MANIFEST = Path(__file__).with_name("adoption-ratchet.manifest")
DEFAULT_ROOT = Path(__file__).resolve().parents[2]

_SEPARATOR = " | "
_PATTERN_ID = re.compile(r"[a-z0-9][a-z0-9-]*")
_CEILING = re.compile(r"[0-9]+")


class ManifestError(ValueError):
    """The manifest breaks the row grammar."""


@dataclass(frozen=True)
class Row:
    pattern_id: str
    pattern: re.Pattern
    path_glob: str
    ceiling: int
    owner: str
    manifest_line: int


@dataclass(frozen=True)
class RowResult:
    row: Row
    files: tuple[str, ...]
    hits: tuple[str, ...]

    @property
    def count(self) -> int:
        return len(self.hits)

    @property
    def violation(self) -> str | None:
        head = f"adoption ratchet: row '{self.row.pattern_id}' (owner {self.row.owner})"
        if not self.files:
            return f"{head}: path '{self.row.path_glob}' matches no file"
        if self.count > self.row.ceiling:
            return (f"{head}: {self.count} code line(s) match, ceiling "
                    f"{self.row.ceiling}: {', '.join(self.hits)}")
        return None


def _bracket_end(ere: str, start: int) -> int:
    i = start + 1
    if ere[i:i + 1] == "^":
        i += 1
    if ere[i:i + 1] == "]":
        i += 1
    close = ere.find("]", i)
    return len(ere) if close < 0 else close + 1


def _non_portable_construct(ere: str) -> str | None:
    """Name the first construct POSIX ERE and Python re read differently."""
    i = 0
    while i < len(ere):
        char, following = ere[i], ere[i + 1:i + 2]
        if char == "\\":
            if following.isalnum():
                return f"the escape \\{following} (a class shorthand, anchor or backreference)"
            i += 2
        elif char == "[":
            end = _bracket_end(ere, i)
            body = ere[i + 1:end]
            if "\\" in body:
                return "a backslash inside [...]"
            if any(f"[{mark}" in body for mark in ":.="):
                return "a POSIX [:class:], [.coll.] or [=equiv=] bracket"
            i = end
        elif char == "(" and following == "?":
            return "a (?...) group"
        elif char in "*+?}" and following in {"?", "+"}:
            return "a lazy or possessive quantifier"
        else:
            i += 1
    return None


def _parse_row(line: str, where: str, line_no: int) -> Row:
    parts = line.split(_SEPARATOR)
    if len(parts) < 5:
        raise ManifestError(
            f"{where}: expected 'pattern-id | ERE | path | ceiling | owner', "
            f"got {len(parts)} field(s)")
    pattern_id = parts[0].strip()
    ere = _SEPARATOR.join(parts[1:-3])
    path_glob, ceiling, owner = (part.strip() for part in parts[-3:])
    if not _PATTERN_ID.fullmatch(pattern_id):
        raise ManifestError(f"{where}: pattern-id {pattern_id!r} is not kebab-case")
    construct = _non_portable_construct(ere)
    if construct:
        raise ManifestError(
            f"{where}: ERE uses {construct}, which POSIX ERE and Python re read differently")
    try:
        pattern = re.compile(ere)
    except re.error as exc:
        raise ManifestError(f"{where}: ERE {ere!r} does not compile: {exc}") from None
    path = PurePosixPath(path_glob)
    if not path_glob or path.is_absolute() or ".." in path.parts:
        raise ManifestError(
            f"{where}: path {path_glob!r} must be a non-empty repo-relative glob without '..'")
    if not _CEILING.fullmatch(ceiling):
        raise ManifestError(f"{where}: ceiling {ceiling!r} is not a non-negative integer")
    if not owner:
        raise ManifestError(f"{where}: owner is empty")
    return Row(pattern_id, pattern, path_glob, int(ceiling), owner, line_no)


def parse_manifest(text: str, source: str) -> tuple[Row, ...]:
    rows: list[Row] = []
    seen: set[str] = set()
    for line_no, line in enumerate(text.split("\n"), start=1):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        where = f"{source}:{line_no}"
        row = _parse_row(line, where, line_no)
        if row.pattern_id in seen:
            raise ManifestError(f"{where}: duplicate pattern-id {row.pattern_id!r}")
        seen.add(row.pattern_id)
        rows.append(row)
    if not rows:
        raise ManifestError(f"{source}: no rows")
    return tuple(rows)


def load_manifest(path: Path) -> tuple[Row, ...]:
    try:
        text = Path(path).read_text(encoding="utf-8")
    except OSError as exc:
        raise ManifestError(f"{path}: cannot read: {exc}") from None
    return parse_manifest(text, str(path))


def code_line_hits(text: str, pattern: re.Pattern) -> tuple[int, ...]:
    return tuple(line_no for line_no, line in enumerate(text.split("\n"), start=1)
                 if not line.lstrip().startswith("#") and pattern.search(line))


def evaluate(rows, root: Path) -> tuple[RowResult, ...]:
    root = Path(root)
    results = []
    for row in rows:
        paths = sorted(p for p in root.glob(row.path_glob) if p.is_file())
        files = tuple(p.relative_to(root).as_posix() for p in paths)
        hits = tuple(
            f"{rel}:{line_no}"
            for path, rel in zip(paths, files)
            for line_no in code_line_hits(
                path.read_text(encoding="utf-8", errors="replace"), row.pattern))
        results.append(RowResult(row, files, hits))
    return tuple(results)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(
        description="Evaluate adoption-ratchet.manifest rows against the tree.")
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    args = parser.parse_args(argv)
    try:
        rows = load_manifest(args.manifest)
    except ManifestError as exc:
        print(f"adoption ratchet: {exc}", file=sys.stderr)
        return 2
    results = evaluate(rows, args.root)
    for result in results:
        row = result.row
        print(f"{row.pattern_id} {result.count}/{row.ceiling} {row.path_glob} (owner {row.owner})")
    violations = [result.violation for result in results if result.violation]
    for violation in violations:
        print(violation)
    return 1 if violations else 0


if __name__ == "__main__":
    sys.exit(main())
