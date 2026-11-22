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
output's. That needs the build root's mtimes to be real: a root carrying
seed-warm-lane.sh's 2020-01-01 bulk stamp is refused, since every unchanged
input there reads older than every output.

NOT JUDGED, because each lane's cargo already judges them correctly:
- inputs outside the build root, symlinks resolved: the seed never re-stamps
  them;
- runs of packages that are not workspace members: registry and git sources
  are immutable per unit hash;
- records with no RerunIfChanged entry.
cargo metadata is asked for member roots only when a record needs judging.

FAILS CLOSED: a record outside the shape below (KNOWN_LOCAL_KINDS, one
RerunIfChanged with a str `output` inside the target and list[str] `paths`,
a <package>-<16 lowercase hex> fingerprint dir), a cargo metadata failure or
timeout, a seed-stamped build root or a failed unlink exits 1. Every record is
judged before the first unlink.

Usage: evict-dirty-target-units.py --build-root DIR TARGET_DIR
Exit: 0 judged; 1 refused (one `evict-dirty-target-units: <path>: <reason>`
line); 2 usage. stdout stays empty; each eviction and a closing summary go to
stderr.
"""

import argparse
import glob
import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass
from datetime import datetime

PROG = "evict-dirty-target-units"
RECORD_GLOB_DEPTHS = (  # <profile>/ and <triple>/<profile>/
    "*/.fingerprint/*/run-build-script-*.json",
    "*/*/.fingerprint/*/run-build-script-*.json",
)
UNIT_DIR = re.compile(r"^(.+)-([0-9a-f]{16})$")
KNOWN_LOCAL_KINDS = frozenset({"RerunIfChanged", "RerunIfEnvChanged", "Precalculated",
                               "CheckDepInfo"})
METADATA_STDERR_TAIL_LINES = 5
METADATA_TIMEOUT_SECS = 120
# seed-warm-lane.sh's bulk stamp, `touch -d 2020-01-01T00:00:00`: local time,
# whole seconds, read as its own `date -d` check reads it.
SEED_STAMP_NS = int(datetime(2020, 1, 1).timestamp()) * 10 ** 9


class Refusal(Exception):
    """A condition under which nothing more is deleted and the script exits 1."""

    def __init__(self, path, reason):
        super().__init__(f"{path}: {reason}")
        self.path = path
        self.reason = reason


@dataclass(frozen=True)
class Rerun:
    """A record's RerunIfChanged entry: the freshness reference, relative to
    the target, and the inputs cargo compares against it."""
    output_rel: str
    inputs: tuple[str, ...]


@dataclass(frozen=True)
class RunRecord:
    json_path: str
    package: str
    rerun: Rerun | None


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
    unit_dir = os.path.basename(os.path.dirname(json_path))
    match = UNIT_DIR.match(unit_dir)
    if match is None:
        raise Refusal(json_path, f"fingerprint dir {unit_dir!r} is not "
                                 f"<package>-<16 lowercase hex>")
    return RunRecord(json_path, match.group(1),
                     rerun_if_changed(json_path, read_local(json_path)))


def read_local(json_path):
    try:
        with open(json_path, encoding="utf-8") as handle:
            record = json.load(handle)
    except (OSError, ValueError) as error:
        raise Refusal(json_path, f"not a readable JSON record ({error})") from None
    local = record.get("local") if isinstance(record, dict) else None
    if not isinstance(local, list):
        raise Refusal(json_path, "has no `local` list")
    return local


def rerun_if_changed(json_path, local):
    """The record's RerunIfChanged entry as a Rerun, or None."""
    entries = [entry for entry in local if local_kind(json_path, entry) == "RerunIfChanged"]
    if len(entries) > 1:
        raise Refusal(json_path, "has more than one RerunIfChanged entry")
    return validated_rerun(json_path, entries[0]["RerunIfChanged"]) if entries else None


def local_kind(json_path, entry):
    if not (isinstance(entry, dict) and len(entry) == 1):
        raise Refusal(json_path, f"`local` entry {entry!r} is not a single-key object")
    (kind,) = entry
    if kind not in KNOWN_LOCAL_KINDS:
        raise Refusal(json_path, f"unknown LocalFingerprint kind {kind!r}: "
                                 f"cargo's fingerprint format changed")
    return kind


def validated_rerun(json_path, rerun):
    output = rerun.get("output") if isinstance(rerun, dict) else None
    paths = rerun.get("paths") if isinstance(rerun, dict) else None
    if not (isinstance(output, str) and is_relative_inside(output)):
        raise Refusal(json_path, f"RerunIfChanged output {output!r} is not a "
                                 f"relative path inside the target")
    if not (isinstance(paths, list) and all(isinstance(path, str) for path in paths)):
        raise Refusal(json_path, "RerunIfChanged paths is not a list of strings")
    return Rerun(output, tuple(paths))


def is_relative_inside(path):
    norm = os.path.normpath(path)
    return not os.path.isabs(norm) and norm.split(os.sep)[0] not in (os.curdir, os.pardir)


def member_roots(build_root):
    """Workspace member name -> realpath of its package root, per cargo."""
    manifest = os.path.join(build_root, "Cargo.toml")
    argv = ["cargo", "metadata", "--format-version", "1", "--no-deps", "--offline",
            "--manifest-path", manifest]
    try:
        result = subprocess.run(argv, capture_output=True, text=True, check=False,
                                timeout=METADATA_TIMEOUT_SECS)
    except OSError as error:
        raise Refusal(manifest, f"cargo metadata could not run ({error})") from None
    except subprocess.TimeoutExpired:
        raise Refusal(manifest, f"cargo metadata did not finish within "
                                f"{METADATA_TIMEOUT_SECS}s") from None
    if result.returncode != 0:
        tail = " | ".join(result.stderr.strip().splitlines()[-METADATA_STDERR_TAIL_LINES:])
        raise Refusal(manifest, f"cargo metadata exited {result.returncode}: {tail}")
    try:
        packages = json.loads(result.stdout)["packages"]
        return {package["name"]: os.path.realpath(os.path.dirname(package["manifest_path"]))
                for package in packages}
    except (ValueError, KeyError, TypeError) as error:
        raise Refusal(manifest, f"cargo metadata printed no package list ({error!r})") from None


def require_real_mtimes(build_root, roots):
    """Refuse a build root whose workspace or member manifest carries the seed's
    bulk stamp: there no input could be judged stale."""
    manifests = [os.path.join(build_root, "Cargo.toml"),
                 *(os.path.join(root, "Cargo.toml") for root in sorted(roots.values()))]
    for manifest in manifests:
        try:
            stamped = os.stat(manifest).st_mtime_ns == SEED_STAMP_NS
        except OSError as error:
            raise Refusal(manifest, f"cannot stat ({error})") from None
        if stamped:
            raise Refusal(manifest, "carries seed-warm-lane.sh's 2020-01-01 bulk stamp, "
                                    "so the build root's mtimes are not real")


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


def is_inside(path, root):
    return os.path.commonpath([root, path]) == root


def eviction_for(rerun, pkg_root, build_root, reference_ns):
    """The Eviction of `rerun`'s output, naming its first input inside
    `build_root` that cargo would call stale against `reference_ns`; None when
    every such input is fresh."""
    for raw in rerun.inputs:
        path = os.path.normpath(os.path.join(pkg_root, raw))
        if not is_inside(os.path.realpath(path), build_root):
            continue
        try:
            mtime = newest_mtime_ns(path)
        except OSError:
            return Eviction(rerun.output_rel, path, "is missing")
        if mtime > reference_ns:
            return Eviction(rerun.output_rel, path, "is newer than it")
    return None


def judge(rerun, pkg_root, build_root, target):
    """The eviction `rerun` needs, or None when it is fresh or has no output."""
    output = os.path.join(target, rerun.output_rel)
    try:
        reference_ns = os.stat(output).st_mtime_ns
    except FileNotFoundError:
        return None
    except OSError as error:
        raise Refusal(output, f"cannot stat ({error})") from None
    return eviction_for(rerun, pkg_root, build_root, reference_ns)


def judge_target(target, build_root):
    """(records found, member runs judged, the evictions they need)."""
    records = [parse_record(path) for path in discover(target)]
    rerun_records = [record for record in records if record.rerun is not None]
    roots = {}
    if rerun_records:
        roots = member_roots(build_root)
        require_real_mtimes(build_root, roots)
    member_records = [record for record in rerun_records if record.package in roots]
    real_root = os.path.realpath(build_root)
    evictions = [eviction for eviction in (
        judge(record.rerun, roots[record.package], real_root, target)
        for record in member_records) if eviction is not None]
    return len(records), len(member_records), evictions


def evict(target, eviction):
    output = os.path.join(target, eviction.output_rel)
    try:
        os.unlink(output)
    except OSError as error:
        raise Refusal(output, f"unlink failed ({error})") from None
    print(f"evicted {eviction.output_rel}: {eviction.offending} {eviction.why}",
          file=sys.stderr)


def parse_args(argv):
    parser = argparse.ArgumentParser(prog=f"{PROG}.py", description=(
        "Unlink the freshness reference of every build-script run the build "
        "root's cargo would rerun."))
    parser.add_argument("--build-root", required=True, metavar="DIR",
                        help="the worktree the target was built from")
    parser.add_argument("target", metavar="TARGET_DIR", help="the cargo target dir to trim")
    args = parser.parse_args(argv)
    for flag, path in (("--build-root", args.build_root), ("TARGET_DIR", args.target)):
        if not os.path.isdir(path):
            parser.error(f"{flag} {path} is not a directory")
    return args


def main(argv=None):
    args = parse_args(argv)
    try:
        found, judged, evictions = judge_target(args.target, args.build_root)
        for eviction in evictions:
            evict(args.target, eviction)
    except Refusal as refusal:
        print(f"{PROG}: {refusal.path}: {refusal.reason}", file=sys.stderr)
        return 1
    print(f"{PROG}: records={found} judged={judged} evicted={len(evictions)}",
          file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
