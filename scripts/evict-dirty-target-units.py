#!/usr/bin/env python3
"""
evict-dirty-target-units.py — drop, from a cargo target dir about to be
promoted, every build-script run that the build root's cargo would rerun
(task 10152).

RULE: a promoted target must carry no build-script run that cargo would rerun
in the build root at the landed commit. Such a run loses the freshness
reference cargo compares its inputs against, <target>/<RerunIfChanged.output>,
so every lane reruns it and rebuilds its consumers.

WHY NOT THE RECORD: measured with cargo 1.96, deleting only the
run-build-script fingerprint reruns the script while the lib and its
dependents stay Fresh with the old rustc-env. A missing `output` changes the
run's local fingerprint, and that propagates to every consumer.

JUDGED AS CARGO JUDGES: an input is stale when it is missing, or when its
mtime (for a directory, the newest mtime under it) is strictly newer than the
output's.

Usage: evict-dirty-target-units.py --build-root DIR TARGET_DIR
stdout stays empty; each eviction and a closing summary go to stderr.
"""

import argparse
import glob
import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass

PROG = "evict-dirty-target-units"
RECORD_GLOB_DEPTHS = ("*/.fingerprint/*/run-build-script-*.json",)
UNIT_DIR = re.compile(r"^(.+)-([0-9a-f]{16})$")


@dataclass(frozen=True)
class RunRecord:
    json_path: str
    package: str
    output_rel: str | None
    inputs: tuple[str, ...] | None


@dataclass(frozen=True)
class Eviction:
    output_rel: str
    offending: str
    why: str


def discover(target):
    found = set()
    for pattern in RECORD_GLOB_DEPTHS:
        found.update(glob.glob(os.path.join(glob.escape(target), pattern)))
    return sorted(found)


def parse_record(json_path):
    package = UNIT_DIR.match(os.path.basename(os.path.dirname(json_path))).group(1)
    with open(json_path, encoding="utf-8") as handle:
        local = json.load(handle)["local"]
    for entry in local:
        if "RerunIfChanged" in entry:
            rerun = entry["RerunIfChanged"]
            return RunRecord(json_path, package, rerun["output"], tuple(rerun["paths"]))
    return RunRecord(json_path, package, None, None)


def member_roots(build_root):
    """Workspace member name -> realpath of its package root, per cargo."""
    result = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps", "--offline",
         "--manifest-path", os.path.join(build_root, "Cargo.toml")],
        capture_output=True, text=True, check=True)
    return {package["name"]: os.path.realpath(os.path.dirname(package["manifest_path"]))
            for package in json.loads(result.stdout)["packages"]}


def newest_mtime_ns(path):
    """cargo's mtime_recursive: a directory counts as the newest entry under it."""
    newest = os.stat(path).st_mtime_ns
    if not os.path.isdir(path):
        return newest
    for dirpath, dirnames, filenames in os.walk(path, followlinks=True):
        for name in dirnames + filenames:
            newest = max(newest, entry_mtime_ns(os.path.join(dirpath, name)))
    return newest


def entry_mtime_ns(path):
    """A symlink counts as the newer of itself and its target, as in cargo."""
    try:
        own = os.lstat(path).st_mtime_ns
    except OSError:
        return 0
    try:
        return max(own, os.stat(path).st_mtime_ns)
    except OSError:
        return own


def offending_input(record, pkg_root, reference_ns):
    """The first input cargo would call stale against `reference_ns`, or None."""
    for raw in record.inputs:
        path = os.path.normpath(os.path.join(pkg_root, raw))
        try:
            mtime = newest_mtime_ns(path)
        except OSError:
            return Eviction(record.output_rel, path, "is missing")
        if mtime > reference_ns:
            return Eviction(record.output_rel, path, "is newer than it")
    return None


def judge(record, roots, target):
    """The eviction `record` needs, or None when it is fresh or has no output."""
    try:
        reference_ns = os.stat(os.path.join(target, record.output_rel)).st_mtime_ns
    except FileNotFoundError:
        return None
    return offending_input(record, roots[record.package], reference_ns)


def parse_args(argv):
    parser = argparse.ArgumentParser(prog=f"{PROG}.py", description=(
        "Unlink the freshness reference of every build-script run the build "
        "root's cargo would rerun."))
    parser.add_argument("--build-root", required=True, metavar="DIR",
                        help="the worktree the target was built from")
    parser.add_argument("target", metavar="TARGET_DIR", help="the cargo target dir to trim")
    return parser.parse_args(argv)


def main(argv=None):
    args = parse_args(argv)
    records = [parse_record(path) for path in discover(args.target)]
    rerun_records = [record for record in records if record.inputs is not None]
    roots = member_roots(args.build_root)
    evictions = [eviction for eviction in (judge(record, roots, args.target)
                                           for record in rerun_records)
                 if eviction is not None]
    for eviction in evictions:
        os.unlink(os.path.join(args.target, eviction.output_rel))
        print(f"evicted {eviction.output_rel}: {eviction.offending} {eviction.why}",
              file=sys.stderr)
    print(f"{PROG}: records={len(records)} judged={len(rerun_records)} "
          f"evicted={len(evictions)}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
