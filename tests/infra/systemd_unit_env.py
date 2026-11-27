#!/usr/bin/env python3
"""Which environment keys a systemd unit's tracked files set.

Reads unit files and drop-in directories as text, merged in the order systemd
merges them, and reports every key that Environment= leaves in effect in the
[Service] section, with the file that last set it. It fails closed on anything
those files alone cannot answer: EnvironmentFile=, PassEnvironment= and
UnsetEnvironment= draw keys from outside the tracked tree, so they raise
UnitEnvError, as do a malformed assignment, a missing path and a drop-in
directory holding no *.conf file.

CLI: systemd_unit_env.py <unit-file-or-drop-in-dir>... prints one
`KEY<TAB>PATH` line per key, sorted by key; exit 1 on UnitEnvError (one
diagnostic line on stderr), 2 on a usage error.
"""

import os
import re
import shlex
import sys
from dataclasses import dataclass
from pathlib import Path

ENVIRONMENT_SECTION = "Service"
OUT_OF_TREE_DIRECTIVES = frozenset(
    {"EnvironmentFile", "PassEnvironment", "UnsetEnvironment"})
KEY_PATTERN = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")


@dataclass(frozen=True)
class UnitEnvKey:
    key: str
    path: Path
    line: int


class UnitEnvError(Exception):
    pass


def unit_files(paths):
    files = []
    for path in map(Path, paths):
        if path.is_dir():
            files.extend(_drop_in_confs(path))
        elif path.is_file():
            files.append(path)
        else:
            raise UnitEnvError(f"{path}: no such unit file or drop-in directory")
    return files


def _drop_in_confs(directory):
    confs = sorted(
        (child for child in directory.iterdir() if child.name.endswith(".conf")),
        key=lambda child: os.fsencode(child.name))
    if not confs:
        raise UnitEnvError(f"{directory}: drop-in directory holds no *.conf file")
    return confs


def environment_keys(files):
    in_effect = {}
    for path in files:
        for line, section, directive, value in _directives(path):
            if section == ENVIRONMENT_SECTION:
                _apply(in_effect, path, line, directive, value)
    return sorted(in_effect.values(), key=lambda entry: entry.key)


def _logical_lines(path):
    try:
        text = path.read_text()
    except (OSError, UnicodeDecodeError) as err:
        raise UnitEnvError(f"{path}: unreadable: {err}") from err
    start, parts = None, []
    for number, raw in enumerate(text.splitlines(), start=1):
        stripped = raw.strip()
        if stripped.startswith(("#", ";")) or (start is None and not stripped):
            continue
        start = start or number
        if stripped.endswith("\\"):
            parts.append(stripped[:-1])
            continue
        parts.append(stripped)
        yield start, " ".join(parts)
        start, parts = None, []
    if start is not None:
        yield start, " ".join(parts)


def _directives(path):
    section = None
    for line, text in _logical_lines(path):
        if text.startswith("[") and text.endswith("]"):
            section = text[1:-1]
            continue
        directive, sep, value = text.partition("=")
        if sep:
            yield line, section, directive.strip(), value.strip()


def _apply(in_effect, path, line, directive, value):
    where = f"{path}:{line}"
    if directive in OUT_OF_TREE_DIRECTIVES:
        raise UnitEnvError(
            f"{where}: {directive}= draws keys from outside the tracked unit files")
    if directive != "Environment":
        return
    if not value:
        in_effect.clear()
        return
    for key in _assigned_keys(value, where):
        in_effect[key] = UnitEnvKey(key, path, line)


def _assigned_keys(value, where):
    try:
        tokens = shlex.split(value, posix=True)
    except ValueError as err:
        raise UnitEnvError(f"{where}: Environment={value}: {err}") from err
    keys = []
    for token in tokens:
        key, sep, _ = token.partition("=")
        if not sep or not KEY_PATTERN.fullmatch(key):
            raise UnitEnvError(
                f"{where}: Environment= assignment {token!r} is not KEY=VALUE")
        keys.append(key)
    return keys


def main(argv):
    if not argv:
        print("usage: systemd_unit_env.py <unit-file-or-drop-in-dir>...",
              file=sys.stderr)
        return 2
    try:
        entries = environment_keys(unit_files(argv))
    except UnitEnvError as err:
        print(f"systemd_unit_env.py: {err}", file=sys.stderr)
        return 1
    for entry in entries:
        print(f"{entry.key}\t{entry.path}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
