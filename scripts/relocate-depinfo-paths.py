#!/usr/bin/env python3
"""
relocate-depinfo-paths.py — rewrite the input paths a CoW-seeded lane's cargo
dep-info records name under the base's build root, so they name the same
files under the lane (task 10167; PRD §9.5 inv.15 states the invariant).

RULE: after a seed, no dep-info record under the lane's target names an input
under the foreign build root. Each such path becomes the same path under the
lane, and the record keeps its mtime: cargo compares every input against the
record's own mtime, so a record rewritten at seed time would read newer than
the sources the seed delta-touched and hide them.

WHY: cargo stores an input outside the unit's package (an include_str! of
examples/, another crate's file, the root Cargo.toml clippy tracks) as an
absolute, non-canonical path. In a clone of _merge-verify's target that path
names _merge-verify's copy, so a lane edit to it goes unseen and
_merge-verify's churn dirties the unit in every lane.

WHY NOT A TEXT REWRITE: every path carries a u32 length prefix, and the two
roots differ in length, so a byte substitution corrupts the record.

ENCODING (cargo 1.96, all integers little-endian), the only one accepted:
  u32 1, u8 0xFF, u8 version 1, u32 nfiles;
  per file: u8 type (0 package-relative, 1 target-relative or absolute),
    u32 len + path, u8 has_checksum (1: u64 file length + u32 len + string);
  u32 nenv; per entry: u32 len + key, u8 tag (0, or 1 + u32 len + value);
  then end of file.

NOT TOUCHED: the env section (cargo compares it with the build's environment,
not the filesystem); deps/*.d makefile depfiles, which cargo's freshness check
never reads; fingerprint JSON. Only a path that starts with `<foreign>/`
changes, so a sibling root such as `<foreign>-2` does not match.

FAILS CLOSED: a read or write failure exits 1. A record that names the
foreign root but does not decode is left as it is and counted.

Usage: relocate-depinfo-paths.py --from-root FOREIGN --to-root LANE TARGET_DIR
Exit: 0 done (a missing TARGET_DIR holds no records); 1 I/O failure;
2 usage. stdout stays empty; a summary goes to stderr.
"""

import argparse
import dataclasses
import enum
import glob
import os
import stat
import struct
import sys
import tempfile
from collections import Counter
from dataclasses import dataclass

PROG = "relocate-depinfo-paths"
DEPINFO_GLOB_DEPTHS = (  # <profile>/ and <triple>/<profile>/
    "*/.fingerprint/*/dep-*",
    "*/*/.fingerprint/*/dep-*",
)
MAGIC = struct.pack("<IB", 1, 0xFF)
VERSION = 1
PATH_TYPES = frozenset({0, 1})


class Undecodable(Exception):
    """The bytes are not a dep-info record in the encoding above."""


class Outcome(enum.Enum):
    NO_FOREIGN_ROOT = "no foreign root"
    UNCHANGED = "unchanged"
    REWROTE = "rewrote"
    UNDECODABLE = "undecodable"


@dataclass(frozen=True)
class PathEntry:
    path_type: int
    path: bytes
    checksum: bytes  # the has_checksum byte and what it announces, verbatim


@dataclass(frozen=True)
class DepInfo:
    entries: tuple[PathEntry, ...]
    env: bytes  # the env section, from its count to end of file, verbatim


class Cursor:
    def __init__(self, data):
        self.data = data
        self.pos = 0

    def take(self, size):
        end = self.pos + size
        if end > len(self.data):
            raise Undecodable(f"truncated at byte {self.pos}")
        chunk = self.data[self.pos:end]
        self.pos = end
        return chunk

    def u8(self):
        return self.take(1)[0]

    def u32(self):
        return struct.unpack("<I", self.take(4))[0]

    def sized(self):
        return self.take(self.u32())


def decode_entry(cursor):
    path_type = cursor.u8()
    if path_type not in PATH_TYPES:
        raise Undecodable(f"path type {path_type}")
    path = cursor.sized()
    checksum_start = cursor.pos
    has_checksum = cursor.u8()
    if has_checksum == 1:
        cursor.take(8)
        cursor.sized()
    elif has_checksum != 0:
        raise Undecodable(f"has_checksum {has_checksum}")
    return PathEntry(path_type, path, cursor.data[checksum_start:cursor.pos])


def skip_env(cursor):
    for _ in range(cursor.u32()):
        cursor.sized()
        tag = cursor.u8()
        if tag == 1:
            cursor.sized()
        elif tag != 0:
            raise Undecodable(f"env value tag {tag}")


def decode(data):
    cursor = Cursor(data)
    if cursor.take(len(MAGIC)) != MAGIC:
        raise Undecodable("not a dep-info header")
    version = cursor.u8()
    if version != VERSION:
        raise Undecodable(f"version {version}")
    entries = tuple(decode_entry(cursor) for _ in range(cursor.u32()))
    env_start = cursor.pos
    skip_env(cursor)
    if cursor.pos != len(data):
        raise Undecodable(f"{len(data) - cursor.pos} trailing byte(s)")
    return DepInfo(entries, data[env_start:])


def u32(value):
    return struct.pack("<I", value)


def encode(info):
    parts = [MAGIC, bytes([VERSION]), u32(len(info.entries))]
    for entry in info.entries:
        parts += [bytes([entry.path_type]), u32(len(entry.path)), entry.path, entry.checksum]
    parts.append(info.env)
    return b"".join(parts)


def relocated(info, foreign_prefix, lane_prefix):
    def moved(entry):
        if not entry.path.startswith(foreign_prefix):
            return entry
        return dataclasses.replace(entry, path=lane_prefix + entry.path[len(foreign_prefix):])

    return dataclasses.replace(info, entries=tuple(moved(entry) for entry in info.entries))


def replace_keeping_times(path, data, original):
    """Swap `data` in for `path`'s content, keeping its mode, atime and mtime."""
    fd, temp = tempfile.mkstemp(dir=os.path.dirname(path), prefix=f".{PROG}-")
    try:
        with os.fdopen(fd, "wb") as handle:
            handle.write(data)
        os.chmod(temp, stat.S_IMODE(original.st_mode))
        os.utime(temp, ns=(original.st_atime_ns, original.st_mtime_ns))
        os.replace(temp, path)
    except BaseException:
        if os.path.lexists(temp):
            os.unlink(temp)
        raise


def relocate_file(path, foreign_prefix, lane_prefix):
    original = os.stat(path)
    with open(path, "rb") as handle:
        data = handle.read()
    if foreign_prefix not in data:
        return Outcome.NO_FOREIGN_ROOT
    try:
        info = decode(data)
    except Undecodable:
        return Outcome.UNDECODABLE
    rewritten = encode(relocated(info, foreign_prefix, lane_prefix))
    if rewritten == data:
        return Outcome.UNCHANGED
    replace_keeping_times(path, rewritten, original)
    return Outcome.REWROTE


def discover(target):
    found = set()
    for pattern in DEPINFO_GLOB_DEPTHS:
        found.update(glob.glob(os.path.join(glob.escape(target), pattern)))
    return sorted(found)


def parse_args(argv):
    parser = argparse.ArgumentParser(prog=f"{PROG}.py", description=(
        "Rewrite cargo dep-info input paths under FOREIGN to the same paths under LANE."))
    parser.add_argument("--from-root", required=True, metavar="FOREIGN",
                        help="the build root the records name (the base's .buildroot stamp)")
    parser.add_argument("--to-root", required=True, metavar="LANE",
                        help="the lane the target now belongs to")
    parser.add_argument("target", metavar="TARGET_DIR", help="the lane's cargo target dir")
    args = parser.parse_args(argv)
    for flag, root in (("--from-root", args.from_root), ("--to-root", args.to_root)):
        if not os.path.isabs(root) or root.endswith(os.sep):
            parser.error(f"{flag} {root!r} must be absolute with no trailing slash")
    return args


def main(argv=None):
    args = parse_args(argv)
    foreign_prefix = os.fsencode(args.from_root + os.sep)
    lane_prefix = os.fsencode(args.to_root + os.sep)
    try:
        outcomes = Counter(relocate_file(path, foreign_prefix, lane_prefix)
                           for path in discover(args.target))
    except OSError as error:
        print(f"{PROG}: {error}", file=sys.stderr)
        return 1
    print(f"{PROG}: rewrote {outcomes[Outcome.REWROTE]} dep-info file(s) from "
          f"{args.from_root} to {args.to_root}; "
          f"undecodable={outcomes[Outcome.UNDECODABLE]}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
