#!/usr/bin/env python3
"""Count regex occurrences over the production view of Rust files against
per-row baselines. The view is the one scripts/lib_rust_production_view.sh
exports; tested by tests/infra/test_compiler_decl_pass_ratchets.py.
"""

from __future__ import annotations

import os
import re
import subprocess
from dataclasses import dataclass
from enum import Enum
from pathlib import Path, PurePosixPath

LEXER_LIB = Path(__file__).resolve().parents[2] / "scripts" / "lib_rust_production_view.sh"

_SPAWN = 'source "$1" && prog="$RUST_PRODUCTION_VIEW_AWK\n$2" && awk "$prog" "$3"'
_DRIVER = '{ printf "%d\\t%d\\t%s\\n", FNR, length(comment_tail), code }'


class LexError(RuntimeError):
    """A source file the shared lexer could not read faithfully."""


class View(Enum):
    CODE = "code"
    CODE_WITH_LITERALS = "code_with_literals"


@dataclass(frozen=True)
class Row:
    row_id: str
    construct: re.Pattern
    view: View
    paths: tuple[str, ...]
    baseline: int
    target: int
    lowered_by: str

    def __post_init__(self):
        problem = _row_problem(self)
        if problem:
            raise ValueError(f"row {self.row_id!r}: {problem}")


def _row_problem(row: Row) -> str | None:
    if not row.row_id:
        return "row_id is empty"
    if not row.lowered_by:
        return f"lowered_by {row.lowered_by!r} is empty"
    if not row.paths:
        return f"paths {row.paths!r} is empty"
    for path in row.paths:
        pure = PurePosixPath(path)
        if not path or pure.is_absolute() or ".." in pure.parts:
            return f"path {path!r} must be a non-empty repo-relative glob without '..'"
    if not 0 <= row.target <= row.baseline:
        return f"needs 0 <= target <= baseline, got target {row.target}, baseline {row.baseline}"
    return None


@dataclass(frozen=True)
class RowResult:
    row: Row
    files: tuple[str, ...]
    missing_paths: tuple[str, ...]
    hits: tuple[str, ...]

    @property
    def count(self) -> int:
        return len(self.hits)

    @property
    def violation(self) -> str | None:
        row = self.row
        head = f"construct ratchet: row {row.row_id} (lowered by {row.lowered_by})"
        parts = [f"{head}: path '{glob}' matches no file" for glob in self.missing_paths]
        if self.count > row.baseline:
            parts.append(f"{head}: {self.count} occurrence(s), baseline {row.baseline} "
                         f"(target {row.target}): {', '.join(self.hits)}")
        return "\n".join(parts) or None

    @property
    def summary(self) -> str:
        row = self.row
        return (f"{row.row_id} {self.count}/{row.baseline} "
                f"(target {row.target}, lowered by {row.lowered_by})")


@dataclass(frozen=True)
class _ProductionLine:
    number: int
    code: str
    code_with_literals: str

    def text(self, view: View) -> str:
        return self.code if view is View.CODE else self.code_with_literals


def _production_lines(path: Path, raw: bytes) -> tuple[_ProductionLine, ...]:
    proc = subprocess.run(
        ["bash", "-c", _SPAWN, "bash", str(LEXER_LIB), _DRIVER, str(path)],
        capture_output=True, env={**os.environ, "LC_ALL": "C"})
    if proc.returncode != 0 or proc.stderr:
        raise LexError(f"{path}: shared lexer exited {proc.returncode}: "
                       f"{proc.stderr.decode('utf-8', 'replace').strip()}")
    raw_lines = raw.split(b"\n")
    lines = []
    for record in proc.stdout.decode("utf-8", "replace").split("\n")[:-1]:
        number, tail_len, code = record.split("\t", 2)
        line = raw_lines[int(number) - 1]
        literal_view = line[:len(line) - int(tail_len)].decode("utf-8", "replace")
        lines.append(_ProductionLine(int(number), code, literal_view))
    return tuple(lines)


def _scope(row: Row, root: Path) -> tuple[list[Path], tuple[str, ...]]:
    files: dict[Path, None] = {}
    missing = []
    for glob in row.paths:
        matched = sorted(p for p in root.glob(glob) if p.is_file())
        if not matched:
            missing.append(glob)
        files.update(dict.fromkeys(matched))
    return list(files), tuple(missing)


def _file_hits(row: Row, root: Path, path: Path,
               lexed: dict[Path, tuple[_ProductionLine, ...]]) -> tuple[str, ...]:
    raw = path.read_bytes()
    if not row.construct.search(raw.decode("utf-8", "replace")):
        return ()
    if path not in lexed:
        lexed[path] = _production_lines(path, raw)
    rel = path.relative_to(root).as_posix()
    return tuple(f"{rel}:{line.number}"
                 for line in lexed[path]
                 for _ in row.construct.finditer(line.text(row.view)))


def evaluate(rows, root: Path) -> tuple[RowResult, ...]:
    root = Path(root)
    lexed: dict[Path, tuple[_ProductionLine, ...]] = {}
    results = []
    for row in rows:
        paths, missing = _scope(row, root)
        hits = tuple(hit for path in paths for hit in _file_hits(row, root, path, lexed))
        files = tuple(p.relative_to(root).as_posix() for p in paths)
        results.append(RowResult(row, files, missing, hits))
    return tuple(results)
