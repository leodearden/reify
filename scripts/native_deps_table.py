#!/usr/bin/env python3
"""
native_deps_table.py — crates/reify-build-utils/native-deps.toml as bash
declarations, for scripts/check-manifold-deps.sh to eval.

    python3 -I scripts/native_deps_table.py <table.toml>

Run it on the caller's own (scrubbed) loader path, never under the conda
LD_LIBRARY_PATH of /opt/reify-deps: the interpreter must not load that
prefix's libraries.

Output contract (stdout, every value shlex-quoted):

    NATIVE_DEP_ROWS=(<row keys in file order>)

then, per row, with <KEY> the upper-cased row key:

    <KEY>_INCLUDE_ENV  <KEY>_LIB_ENV  <KEY>_CFG
    <KEY>_INCLUDE_SENTINEL  <KEY>_LIB_SENTINEL
    <KEY>_INCLUDE_CANDIDATES=(..)  <KEY>_LIB_CANDIDATES=(..)
    <KEY>_XDE_TOOLKITS=(..)
    <KEY>_SNAP_ROOT  <KEY>_SNAP_INCLUDE_SUBDIR  <KEY>_SNAP_LIB_SUBDIR

An absent optional column (`snap`, `xde_toolkits`) is emitted EMPTY, so a
same-named variable in the caller's environment never survives the eval.

Exit contract: 0 with the declarations. 1 with nothing on stdout and one
`native_deps_table.py: <path>: <message>` line on stderr when the file is
missing or not TOML, a row key is not [a-z][a-z0-9]*, a required column is
missing or mistyped, an env or cfg value is not an identifier, or a column is
unknown. 2 on a usage error.

Stdlib only: the gate reaches /usr/bin/python3, which has no third-party
packages.
"""

from __future__ import annotations

import re
import shlex
import sys
import tomllib

ROW_KEY = re.compile(r"[a-z][a-z0-9]*")
IDENTIFIER = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")

IDENTIFIER_COLUMNS = ("include_env", "lib_env", "cfg")
STRING_COLUMNS = IDENTIFIER_COLUMNS + ("include_sentinel", "lib_sentinel")
LIST_COLUMNS = ("include_candidates", "lib_candidates")
OPTIONAL_COLUMNS = ("xde_toolkits", "snap")
SNAP_COLUMNS = ("root", "include_subdir", "lib_subdir")

Declaration = tuple[str, "str | list[str]"]


class TableError(Exception):
    """A table defect. The message names the row and column at fault."""


def load(path: str) -> dict:
    try:
        with open(path, "rb") as f:
            return tomllib.load(f)
    except OSError as e:
        raise TableError(f"cannot read: {e.strerror}") from e
    except tomllib.TOMLDecodeError as e:
        raise TableError(f"malformed TOML: {e}") from e


def _require_columns(where: str, table: dict, required: tuple, allowed: tuple) -> None:
    unknown = [c for c in table if c not in allowed]
    if unknown:
        raise TableError(f"{where} unknown column(s): {', '.join(unknown)}")
    missing = [c for c in required if c not in table]
    if missing:
        raise TableError(f"{where} missing required column(s): {', '.join(missing)}")


def _string_list(where: str, column: str, value: object) -> list[str]:
    if not isinstance(value, list) or not all(isinstance(v, str) for v in value):
        raise TableError(f"{where} {column} must be a list of strings")
    return value


def _snap(where: str, snap: object) -> dict[str, str]:
    if snap is None:
        return {c: "" for c in SNAP_COLUMNS}
    if not isinstance(snap, dict):
        raise TableError(f"{where} snap must be a table")
    _require_columns(f"{where} snap", snap, SNAP_COLUMNS, SNAP_COLUMNS)
    for column in SNAP_COLUMNS:
        if not isinstance(snap[column], str):
            raise TableError(f"{where} snap.{column} must be a string")
    return snap


def check_row(key: str, row: object) -> list[Declaration]:
    """Validate one row and return its declarations, in output order."""
    if not ROW_KEY.fullmatch(key):
        raise TableError(f"row key {key!r} is not [a-z][a-z0-9]*")
    where = f"[{key}]"
    if not isinstance(row, dict):
        raise TableError(f"{where} must be a table")
    required = STRING_COLUMNS + LIST_COLUMNS
    _require_columns(where, row, required, required + OPTIONAL_COLUMNS)
    for column in STRING_COLUMNS:
        if not isinstance(row[column], str):
            raise TableError(f"{where} {column} must be a string")
    for column in IDENTIFIER_COLUMNS:
        if not IDENTIFIER.fullmatch(row[column]):
            raise TableError(f"{where} {column} {row[column]!r} is not an identifier")
    include_candidates = _string_list(where, "include_candidates", row["include_candidates"])
    lib_candidates = _string_list(where, "lib_candidates", row["lib_candidates"])
    xde_toolkits = _string_list(where, "xde_toolkits", row.get("xde_toolkits", []))
    snap = _snap(where, row.get("snap"))

    prefix = key.upper()
    return [
        (f"{prefix}_INCLUDE_ENV", row["include_env"]),
        (f"{prefix}_LIB_ENV", row["lib_env"]),
        (f"{prefix}_CFG", row["cfg"]),
        (f"{prefix}_INCLUDE_SENTINEL", row["include_sentinel"]),
        (f"{prefix}_LIB_SENTINEL", row["lib_sentinel"]),
        (f"{prefix}_INCLUDE_CANDIDATES", include_candidates),
        (f"{prefix}_LIB_CANDIDATES", lib_candidates),
        (f"{prefix}_XDE_TOOLKITS", xde_toolkits),
        (f"{prefix}_SNAP_ROOT", snap["root"]),
        (f"{prefix}_SNAP_INCLUDE_SUBDIR", snap["include_subdir"]),
        (f"{prefix}_SNAP_LIB_SUBDIR", snap["lib_subdir"]),
    ]


def render(declarations: list[Declaration]) -> str:
    lines = []
    for name, value in declarations:
        if isinstance(value, list):
            lines.append(f"{name}=({' '.join(shlex.quote(v) for v in value)})")
        else:
            lines.append(f"{name}={shlex.quote(value)}")
    return "\n".join(lines) + "\n"


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: native_deps_table.py <table.toml>", file=sys.stderr)
        return 2
    path = argv[1]
    try:
        table = load(path)
        rows = [(key, check_row(key, row)) for key, row in table.items()]
    except TableError as e:
        print(f"native_deps_table.py: {path}: {e}", file=sys.stderr)
        return 1
    declarations: list[Declaration] = [("NATIVE_DEP_ROWS", [key for key, _ in rows])]
    for _, row_declarations in rows:
        declarations.extend(row_declarations)
    sys.stdout.write(render(declarations))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
