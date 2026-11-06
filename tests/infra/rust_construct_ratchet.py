#!/usr/bin/env python3
"""Count regex occurrences in the production code of Rust files against
per-row baselines, read through the one lexer scripts/lib_rust_production_view.sh
exports; tested by tests/infra/test_rust_construct_ratchet.py.
"""

from __future__ import annotations

import os
import re
import subprocess
from collections import Counter
from dataclasses import dataclass
from enum import Enum
from pathlib import Path, PurePosixPath

LEXER_LIB = Path(__file__).resolve().parents[2] / "scripts" / "lib_rust_production_view.sh"

_SPAWN = 'source "$1" && prog="${!2}\n$3" && awk -v probes="$5" "$prog" "$4"'

_PRODUCTION_DRIVER = '{ printf "%d\\t%s\\n", FNR, code }'

_PROBE_DRIVER = r'''
BEGIN {
    n_probes = split(probes, probe_list, " ")
    for (p = 1; p <= n_probes; p++) {
        split(probe_list[p], probe_at, ":")
        probe_cols[probe_at[1]] = probe_cols[probe_at[1]] " " probe_at[2]
    }
}
function replay_to_line_start(   k) {
    _lexer_reset()
    for (k = 1; k <= n_since_balanced; k++) _strip_line(since_balanced[k])
}
# The lexer is balanced just before a byte that starts code, and lexing the
# line split there reads exactly as lexing it whole. A split inside a token
# hides the lexer's look-ahead (a char literal's closing quote), so it reads
# differently.
function starts_code_at(col,   split_code, split_state) {
    replay_to_line_start()
    split_code = _strip_line(substr($0, 1, col - 1))
    if (_lexer_open_state() != "" || comment_tail != "") return 0
    split_code = split_code _strip_line(substr($0, col))
    split_state = _lexer_open_state()
    replay_to_line_start()
    return split_code == _strip_line($0) && split_state == _lexer_open_state()
}
FNR in probe_cols {
    n_cols = split(probe_cols[FNR], line_cols, " ")
    for (p = 1; p <= n_cols; p++)
        if (starts_code_at(line_cols[p])) print FNR ":" line_cols[p]
    replay_to_line_start()
}
{
    _strip_line($0)
    if (_lexer_open_state() == "") n_since_balanced = 0
    else since_balanced[++n_since_balanced] = $0
}
'''


class LexError(RuntimeError):
    """A source file the shared lexer could not read faithfully."""


class View(Enum):
    """CODE matches the lexed line: comments dropped, literal contents blanked.
    STARTS_IN_CODE matches the raw line and counts a match whose first byte the
    lexer reads as production code, so a literal's opening quote counts and
    text inside a comment or another literal does not."""
    CODE = "code"
    STARTS_IN_CODE = "starts_in_code"


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
        prefix = f"construct ratchet: row {row.row_id} (lowered by {row.lowered_by})"
        parts = [f"{prefix}: path '{glob}' matches no file" for glob in self.missing_paths]
        measured = f"{prefix}: {self.count} occurrence(s)"
        if self.count > row.baseline:
            parts.append(f"{measured}, baseline {row.baseline} (target {row.target}): "
                         f"{', '.join(self.hits)}")
        elif self.count < row.baseline:
            parts.append(f"{measured}, below baseline {row.baseline} (target {row.target}); "
                         f"lower the row's baseline to {self.count}")
        return "\n".join(parts) or None

    @property
    def summary(self) -> str:
        row = self.row
        return (f"{row.row_id} {self.count}/{row.baseline} "
                f"(target {row.target}, lowered by {row.lowered_by})")


def _run_lexer(path: Path, export: str, driver: str, probes: str = "") -> list[str]:
    proc = subprocess.run(
        ["bash", "-c", _SPAWN, "bash", str(LEXER_LIB), export, driver, str(path), probes],
        capture_output=True, env={**os.environ, "LC_ALL": "C"})
    if proc.returncode != 0 or proc.stderr:
        raise LexError(f"{path}: shared lexer exited {proc.returncode}: "
                       f"{proc.stderr.decode('utf-8', 'replace').strip()}")
    return proc.stdout.decode("utf-8", "surrogateescape").split("\n")[:-1]


@dataclass(frozen=True)
class _SourceFile:
    path: Path
    lines: tuple[str, ...]
    production_code: tuple[tuple[int, str], ...]

    @classmethod
    def lex(cls, path: Path, text: str) -> _SourceFile:
        records = (record.split("\t", 1)
                   for record in _run_lexer(path, "RUST_PRODUCTION_VIEW_AWK", _PRODUCTION_DRIVER))
        return cls(path, tuple(text.split("\n")),
                   tuple((int(number), code) for number, code in records))

    def code_hits(self, construct: re.Pattern) -> list[int]:
        return [number for number, code in self.production_code
                for _ in construct.finditer(code)]

    def starts_in_code_hits(self, construct: re.Pattern) -> list[int]:
        candidates = [(number, _byte_column(self.lines[number - 1], match.start()))
                      for number, _ in self.production_code
                      for match in construct.finditer(self.lines[number - 1])]
        if not candidates:
            return []
        probes = " ".join(f"{number}:{column}" for number, column in candidates)
        in_code = {tuple(int(field) for field in record.split(":"))
                   for record in _run_lexer(self.path, "RUST_LEXER_AWK", _PROBE_DRIVER, probes)}
        return [number for number, column in candidates if (number, column) in in_code]


def _byte_column(line: str, index: int) -> int:
    return len(line[:index].encode("utf-8", "surrogateescape")) + 1


def _scope(row: Row, root: Path) -> tuple[list[Path], tuple[str, ...]]:
    files: dict[Path, None] = {}
    missing = []
    for glob in row.paths:
        matched = sorted(p for p in root.glob(glob) if p.is_file())
        if not matched:
            missing.append(glob)
        files.update(dict.fromkeys(matched))
    return list(files), tuple(missing)


def _needs_lexing(row: Row, text: str) -> bool:
    """A STARTS_IN_CODE hit is a raw-line match, so a file with no matching raw
    line has none. A CODE line can spell text the raw file does not (a blanked
    literal reads ""), so CODE always lexes."""
    if row.view is View.CODE:
        return True
    return any(row.construct.search(line) for line in text.split("\n"))


def _file_hits(row: Row, root: Path, path: Path,
               lexed: dict[Path, _SourceFile]) -> tuple[str, ...]:
    text = path.read_bytes().decode("utf-8", "surrogateescape")
    if not _needs_lexing(row, text):
        return ()
    if path not in lexed:
        lexed[path] = _SourceFile.lex(path, text)
    source = lexed[path]
    numbers = (source.code_hits(row.construct) if row.view is View.CODE
               else source.starts_in_code_hits(row.construct))
    rel = path.relative_to(root).as_posix()
    return tuple(f"{rel}:{number}" for number in numbers)


def _reject_duplicate_ids(rows: tuple[Row, ...]) -> None:
    duplicates = sorted(row_id for row_id, uses in Counter(row.row_id for row in rows).items()
                        if uses > 1)
    if duplicates:
        raise ValueError(f"duplicate row_id(s): {', '.join(map(repr, duplicates))}")


def evaluate(rows, root: Path) -> tuple[RowResult, ...]:
    rows = tuple(rows)
    _reject_duplicate_ids(rows)
    root = Path(root)
    lexed: dict[Path, _SourceFile] = {}
    results = []
    for row in rows:
        paths, missing = _scope(row, root)
        hits = tuple(hit for path in paths for hit in _file_hits(row, root, path, lexed))
        files = tuple(p.relative_to(root).as_posix() for p in paths)
        results.append(RowResult(row, files, missing, hits))
    return tuple(results)
