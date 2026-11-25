#!/usr/bin/env python3
"""
drop-canon-invocation-files.py — remove canonical-build-path invocation files
from a copied target directory (task 10442;
docs/prds/warm-lane-pool-cow-seeding.md §9.5 inv.17).

RULE: a copied target must start with no history and no session of the tree
it was copied from. So every file scripts/canon_cache.py's is_invocation_file
accepts is removed wherever it lies in the copy, whatever pid it names: the
invocation journal of any build directory (an in-tree CARGO_TARGET_DIR can sit
below target/), and a private sccache server's pid-named session files, socket
nodes included. A copy's session file never belongs to a live session of the
copy, because a live server listens on the original's inode in another tree;
a live tree's own stale session files are canon_cache.open_session's sweep.

NO SYMLINK IS FOLLOWED: a symlink with an invocation file's name is removed
itself, never its target, and a symlinked directory is never entered.

CALLERS: refresh-warm-base.sh Step 3a, on the staging copy of a gen before it
is promoted (never on the advancing target, whose journal is its own history),
and seed-warm-lane.sh right after its clone, in both modes. Each treats a
missing or failing run as fatal.

Usage: drop-canon-invocation-files.py DIR
Exit: 0 DIR now holds no invocation file (a DIR that does not exist holds
none); 1 a walk or unlink error, each printed after the walk finishes as one
`drop-canon-invocation-files: <path>: <error>` line; 2 usage, or DIR exists
and is not a directory. stdout stays empty; a closing summary goes to stderr.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import canon_cache  # noqa: E402  (imported after sys.path manipulation)

PROG = "drop-canon-invocation-files"


def error_line(path: str, error: OSError) -> str:
    return f"{PROG}: {path}: {error.strerror or error}"


def parse_args(argv: list[str]) -> str | None:
    """The one DIR argument, or None once the usage line is printed."""
    if len(argv) != 1:
        print(f"{PROG}: usage: {PROG}.py DIR", file=sys.stderr)
        return None
    return argv[0]


def drop_named(directory: str, names: list[str], errors: list[str]) -> int:
    """Unlink each of names in directory that the rule accepts; returns the count, appending failures to errors."""
    removed = 0
    for name in names:
        if not canon_cache.is_invocation_file(name):
            continue
        path = os.path.join(directory, name)
        try:
            os.unlink(path)
        except FileNotFoundError:
            continue
        except OSError as error:
            errors.append(error_line(path, error))
            continue
        removed += 1
    return removed


def drop_tree(root: str) -> tuple[int, list[str]]:
    """(files removed, error lines) for one walk of root that enters real directories only."""
    errors: list[str] = []
    removed = 0
    walk = os.walk(root, followlinks=False, onerror=lambda error: errors.append(error_line(error.filename, error)))
    for directory, dirnames, filenames in walk:
        symlinked_dirs = {name for name in dirnames if os.path.islink(os.path.join(directory, name))}
        dirnames[:] = [name for name in dirnames if name not in symlinked_dirs]
        removed += drop_named(directory, [*filenames, *sorted(symlinked_dirs)], errors)
    return removed, errors


def main(argv: list[str] | None = None) -> int:
    root = parse_args(sys.argv[1:] if argv is None else argv)
    if root is None:
        return 2
    if not os.path.lexists(root):
        print(f"{PROG}: {root}: does not exist, so there is nothing to drop", file=sys.stderr)
        return 0
    if not os.path.isdir(root):
        print(f"{PROG}: {root}: not a directory", file=sys.stderr)
        return 2
    removed, errors = drop_tree(root)
    for line in errors:
        print(line, file=sys.stderr)
    if errors:
        return 1
    print(f"{PROG}: removed {removed} file(s) under {root}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
