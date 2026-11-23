#!/usr/bin/env python3
"""
test_seed_depinfo_relocation.py — a seeded lane judges cargo's recorded
out-of-package inputs against its OWN files, not the base build root's
(task 10167; PRD §9.5 inv.15).

SeedRelocatesDepInfo EXECUTES the real scripts/seed-warm-lane.sh with real git
and real cargo. A throwaway repo's checkout plays _merge-verify: its cargo-built
target becomes the warm base gen, and each case seeds a linked-worktree lane
from that gen, then asks cargo whether the `embed` test is fresh and what its
binary embeds. `embed` include_str!s examples/x.ri, which lies outside its
package, so cargo records that input as an absolute path under the build root.

SeedWiringFailsClosed runs a COPY of the seed beside a failing or absent
relocation helper.

RelocatorCli drives scripts/relocate-depinfo-paths.py as a CLI over synthetic
records written by encode_depinfo, a fixture writer of cargo 1.96's encoding.

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_seed_depinfo_relocation.sh.
"""

import json
import os
import re
import shutil
import stat
import struct
import subprocess
import tempfile
import tomllib
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SEED = REPO_ROOT / "scripts" / "seed-warm-lane.sh"
HELPER_NAME = "relocate-depinfo-paths.py"
HELPER = REPO_ROOT / "scripts" / HELPER_NAME
TOOLCHAIN_PIN = REPO_ROOT / "rust-toolchain.toml"
SUBPROCESS_TIMEOUT_SECS = 600

# Its length differs from every lane path's, so relocating a recorded path
# must re-emit that path's length prefix.
BUILD_ROOT_NAME = "build-root-playing-merge-verify"

STRIPPED_ENV_PREFIXES = ("GIT_", "CARGO_", "RUSTC_", "REIFY_WARM_LANE_", "REIFY_LANE_")
STRIPPED_ENV_KEYS = frozenset({"RUSTC", "RUSTFLAGS", "RUSTDOCFLAGS"})

# The seed's reflink clone, as a physical copy: a tmp fs has no reflink.
STUB_CP = """#!/usr/bin/env bash
src=""
dest=""
for arg in "$@"; do
    case "$arg" in
        -*) ;;
        *) if [ -z "$src" ]; then src="$arg"; else dest="$arg"; fi ;;
    esac
done
exec /bin/cp -a "$src" "$dest"
"""

# No env!() anywhere: the seed's env!() relink would touch this file and
# rebuild the unit, hiding a stale recorded input.
WORKSPACE_FILES = {
    ".gitignore": "/target\n",
    "Cargo.toml": '[workspace]\nmembers = ["crates/alpha"]\nresolver = "2"\n',
    "crates/alpha/Cargo.toml":
        '[package]\nname = "alpha"\nversion = "0.1.0"\nedition = "2021"\n',
    "crates/alpha/src/lib.rs": "",
    "crates/alpha/tests/embed.rs":
        '#[test]\nfn embed() {\n'
        '    println!("EMBED={}", include_str!("../../../examples/x.ri").trim());\n'
        '}\n',
    "examples/x.ri": "v1\n",
}
EMBEDDED_INPUT = "examples/x.ri"


def pinned_toolchain():
    """The channel rust-toolchain.toml pins: the toolchain whose dep-info
    encoding production's _merge-verify records."""
    with TOOLCHAIN_PIN.open("rb") as handle:
        return tomllib.load(handle)["toolchain"]["channel"]


def write_file(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


def write_executable(path, text):
    write_file(path, text)
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def run(argv, cwd, env):
    return subprocess.run([str(a) for a in argv], cwd=cwd, env=env, capture_output=True,
                          text=True, check=False, timeout=SUBPROCESS_TIMEOUT_SECS)


def hermetic_env(tmp, stub_dir):
    """The caller's env minus every git, cargo, rustc and warm-lane knob it
    could leak in, plus a private git identity, CARGO_HOME and toolchain pin."""
    env = {key: value for key, value in os.environ.items()
           if not key.startswith(STRIPPED_ENV_PREFIXES) and key not in STRIPPED_ENV_KEYS}
    empty_git_config = tmp / "empty-gitconfig"
    empty_git_config.touch()
    env.update(
        PATH=f"{stub_dir}{os.pathsep}{os.environ['PATH']}",
        GIT_CONFIG_GLOBAL=str(empty_git_config),
        GIT_CONFIG_NOSYSTEM="1",
        GIT_CEILING_DIRECTORIES=str(tmp),
        GIT_AUTHOR_NAME="Fixture", GIT_AUTHOR_EMAIL="fixture@example.test",
        GIT_COMMITTER_NAME="Fixture", GIT_COMMITTER_EMAIL="fixture@example.test",
        CARGO_HOME=str(tmp / "cargo-home"),
        CARGO_INCREMENTAL="0",
        RUSTC_WRAPPER="",
        RUSTFLAGS="",
        RUSTUP_TOOLCHAIN=pinned_toolchain(),
        REIFY_WARM_LANE_RERERE_ARM="0",
        REIFY_WARM_LANE_RESEED_TRASH_SYNC="1",
    )
    return env


class Pool:
    """A throwaway repo whose checkout plays _merge-verify, one warm base gen
    stamped as refresh-warm-base.sh stamps it, and linked-worktree lanes of
    the repo seeded from that gen."""

    def __init__(self, tmp):
        self.build_root = tmp / BUILD_ROOT_NAME
        self.gen = tmp / "base" / "target.gen.1"
        self.lanes = tmp / "lanes"
        stub_dir = tmp / "stubs"
        write_executable(stub_dir / "cp", STUB_CP)
        self.env = hermetic_env(tmp, stub_dir)
        self.base_commit = None

    def checked(self, argv, cwd, env=None):
        result = run(argv, cwd, env or self.env)
        if result.returncode != 0:
            raise AssertionError(f"{argv} exited {result.returncode}:\n{result.stderr}")
        return result

    def git(self, cwd, *args):
        return self.checked(["git", *args], cwd).stdout.strip()

    def write_build_root(self, files):
        for rel, text in files.items():
            write_file(self.build_root / rel, text)

    def commit_build_root(self):
        self.git(self.build_root, "init", "-q", "-b", "main")
        self.git(self.build_root, "add", "-A")
        self.git(self.build_root, "commit", "-qm", "fixture")
        self.base_commit = self.git(self.build_root, "rev-parse", "HEAD")

    def stamp_gen(self, target):
        """Clone `target` as the gen, with the stamps the seed reads."""
        self.gen.parent.mkdir(parents=True)
        self.checked(["/bin/cp", "-a", target, self.gen], self.gen.parent)
        Path(f"{self.gen}.buildroot").write_text(f"{self.build_root}\n")
        Path(f"{self.gen}.basecommit").write_text(f"{self.base_commit}\n")
        (self.gen.parent / ".warm-base-meta").write_text("RUSTFLAGS=\nINVOCATION=\n")

    def add_lane(self, name, branch=None):
        lane = self.lanes / name
        if len(str(lane)) == len(str(self.build_root)):
            raise ValueError(f"lane {lane} must differ in length from {self.build_root}")
        placement = ["-b", branch] if branch else ["--detach"]
        self.git(self.build_root, "worktree", "add", "-q", *placement, lane, self.base_commit)
        return lane

    def seed(self, lane, seed_script=SEED):
        return run(["bash", seed_script, self.gen, lane, "--fresh-checkout"], lane, self.env)


class SeedRelocatesDepInfo(unittest.TestCase):
    """One cargo-built base, shared; every case seeds its own lane from it."""

    @classmethod
    def setUpClass(cls):
        tmp = Path(tempfile.mkdtemp(prefix="test-sdr-")).resolve()
        cls.addClassCleanup(shutil.rmtree, tmp, True)
        cls.pool = Pool(tmp)
        cls.pool.write_build_root(WORKSPACE_FILES)
        cls.cargo(cls.pool.build_root, "generate-lockfile")
        cls.pool.commit_build_root()
        cls.cargo(cls.pool.build_root, "test", "--no-run")
        cls.pool.stamp_gen(cls.pool.build_root / "target")

    @classmethod
    def cargo(cls, workspace, *args):
        env = {**cls.pool.env, "CARGO_TARGET_DIR": str(workspace / "target")}
        return cls.pool.checked(["cargo", *args, "--offline"], workspace, env)

    def seed_ok(self, lane):
        seeded = self.pool.seed(lane)
        self.assertEqual(seeded.returncode, 0, seeded.stderr)
        self.assertEqual(seeded.stdout, f"{lane / 'target'}\n", seeded.stderr)

    def seeded_lane(self, name):
        lane = self.pool.add_lane(name)
        self.seed_ok(lane)
        return lane

    def build_embed(self, lane):
        """(cargo's `fresh` verdict on the lane's `embed` test, what it embeds).
        Every other unit must come back fresh, proving the seed left the lane
        warm: a cold lane would rebuild `embed` too, for the wrong reason."""
        built = self.cargo(lane, "test", "--no-run", "--message-format=json")
        artifacts = [message for message in map(json.loads, built.stdout.splitlines())
                     if message.get("reason") == "compiler-artifact"]
        embed = [artifact for artifact in artifacts if artifact["target"]["name"] == "embed"]
        others = [artifact for artifact in artifacts if artifact["target"]["name"] != "embed"]
        self.assertEqual(len(embed), 1, built.stdout)
        self.assertNotEqual(others, [], built.stdout)
        self.assertEqual([other["target"]["name"] for other in others if not other["fresh"]],
                         [], "the seeded lane was not warm")
        ran = run([embed[0]["executable"], "--nocapture"], lane, self.pool.env)
        self.assertEqual(ran.returncode, 0, ran.stderr)
        embedded = re.search(r"EMBED=(\S+)", ran.stdout)
        self.assertIsNotNone(embedded, ran.stdout)
        return embed[0]["fresh"], embedded.group(1)

    def test_unchanged_input_stays_fresh(self):
        lane = self.seeded_lane("_lane-1")

        self.assertEqual(self.build_embed(lane), (True, "v1"))

    def test_lane_edit_after_seed_rebuilds_the_unit(self):
        lane = self.seeded_lane("_lane-2")
        (lane / EMBEDDED_INPUT).write_text("v2\n")

        self.assertEqual(self.build_embed(lane), (False, "v2"))

    def test_build_root_churn_does_not_dirty_the_lane(self):
        lane = self.seeded_lane("_lane-3")
        churned = self.pool.build_root / EMBEDDED_INPUT
        before = churned.stat()
        self.addCleanup(os.utime, churned, ns=(before.st_atime_ns, before.st_mtime_ns))
        os.utime(churned)

        self.assertEqual(self.build_embed(lane), (True, "v1"))

    def test_seed_time_delta_rebuilds_the_unit(self):
        lane = self.pool.add_lane("_lane-4", branch="task/delta")
        (lane / EMBEDDED_INPUT).write_text("v3\n")
        self.pool.git(lane, "commit", "-qam", "x.ri v3")
        self.seed_ok(lane)

        self.assertEqual(self.build_embed(lane), (False, "v3"))


class SeedWiringFailsClosed(unittest.TestCase):
    """A seed whose relocation helper fails or is absent cannot vouch for the
    lane's recorded inputs, so it aborts with empty stdout, naming the helper."""

    def setUp(self):
        tmp = Path(tempfile.mkdtemp(prefix="test-sdr-wiring-")).resolve()
        self.addCleanup(shutil.rmtree, tmp, True)
        self.pool = Pool(tmp)
        self.pool.write_build_root({EMBEDDED_INPUT: "v1\n"})
        self.pool.commit_build_root()
        built = tmp / "built-target"
        write_file(built / "debug" / ".fingerprint" / "alpha-1" / "dep-lib-alpha", "")
        self.pool.stamp_gen(built)
        self.scripts = tmp / "scripts"
        self.scripts.mkdir()
        shutil.copy2(SEED, self.scripts / SEED.name)
        self.lane = self.pool.add_lane("_lane-1")

    def assert_seed_refused_naming_the_helper(self):
        seeded = self.pool.seed(self.lane, seed_script=self.scripts / SEED.name)

        self.assertNotEqual(seeded.returncode, 0, seeded.stderr)
        self.assertEqual(seeded.stdout, "")
        self.assertIn(HELPER_NAME, seeded.stderr)

    def test_a_failing_helper_aborts_the_seed(self):
        (self.scripts / HELPER_NAME).write_text("import sys\nsys.exit(1)\n")

        self.assert_seed_refused_naming_the_helper()

    def test_a_missing_helper_aborts_the_seed(self):
        self.assert_seed_refused_naming_the_helper()


def u32(value):
    return struct.pack("<I", value)


def sized(text):
    data = text.encode()
    return u32(len(data)) + data


def encode_depinfo(files, env=()):
    """cargo 1.96's dep-info bytes. `files` holds (path_type, path, checksum)
    with checksum None or (file_length, digest); `env` holds (key, value or
    None)."""
    parts = [u32(1), b"\xff", b"\x01", u32(len(files))]
    for path_type, path, checksum in files:
        parts += [bytes([path_type]), sized(path)]
        if checksum is None:
            parts.append(b"\x00")
        else:
            parts += [b"\x01", struct.pack("<Q", checksum[0]), sized(checksum[1])]
    parts.append(u32(len(env)))
    for key, value in env:
        parts += [sized(key), b"\x00" if value is None else b"\x01" + sized(value)]
    return b"".join(parts)


FIXTURE_NS = 1_600_000_000 * 10 ** 9
ODD_NS = FIXTURE_NS + 123_456_789
DEP_RECORD = "debug/.fingerprint/alpha-0123456789abcdef/dep-test-integration-test-embed"
VERSION_BYTE = 5
MISMATCH_WARNING = "relocate-depinfo-paths: warning:"


def snapshot(path):
    return path.read_bytes(), path.stat().st_mtime_ns


class RelocatorCli(unittest.TestCase):
    """The helper over a synthetic lane target. Roots need not exist: the
    helper only rewrites recorded bytes."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory(prefix="test-sdr-cli-")
        self.addCleanup(tmp.cleanup)
        root = Path(tmp.name).resolve()
        self.foreign = f"{root}/{BUILD_ROOT_NAME}"
        self.lane = f"{root}/lanes/_lane-1"
        self.target = root / "lane-target"

    def write_record(self, rel, data, mtime_ns=FIXTURE_NS):
        path = self.target / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        os.utime(path, ns=(mtime_ns, mtime_ns))
        return path

    def run_helper(self, from_root, to_root):
        return run(["python3", HELPER, "--from-root", from_root, "--to-root", to_root,
                    self.target], self.target.parent, dict(os.environ))

    def relocate(self):
        result = self.run_helper(self.foreign, self.lane)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "")
        return result

    def moved_record(self, rel=DEP_RECORD):
        """A record naming examples/x.ri under the foreign root, and what it
        reads once relocated."""
        def record(root):
            return encode_depinfo([(1, f"{root}/examples/x.ri", None)])

        return self.write_record(rel, record(self.foreign)), record(self.lane)

    def test_only_paths_under_the_foreign_root_move(self):
        record = self.write_record(DEP_RECORD, encode_depinfo([
            (0, "tests/a.rs", None),
            (1, f"{self.foreign}/examples/x.ri", None),
            (1, f"{self.foreign}-sibling/y", None),
        ]))

        self.relocate()

        self.assertEqual(record.read_bytes(), encode_depinfo([
            (0, "tests/a.rs", None),
            (1, f"{self.lane}/examples/x.ri", None),
            (1, f"{self.foreign}-sibling/y", None),
        ]))

    def test_the_env_section_is_carried_verbatim(self):
        env = [("CARGO_PKG_NAME", "alpha"), ("ROOT", f"{self.foreign}/examples"),
               ("UNSET", None)]
        record = self.write_record(DEP_RECORD, encode_depinfo(
            [(1, f"{self.foreign}/examples/x.ri", None)], env))

        self.relocate()

        self.assertEqual(record.read_bytes(), encode_depinfo(
            [(1, f"{self.lane}/examples/x.ri", None)], env))

    def test_a_checksum_is_carried_with_its_moved_path(self):
        checksum = (3, "sha256=" + "ab" * 32)
        record = self.write_record(DEP_RECORD, encode_depinfo(
            [(1, f"{self.foreign}/examples/x.ri", checksum)]))

        self.relocate()

        self.assertEqual(record.read_bytes(), encode_depinfo(
            [(1, f"{self.lane}/examples/x.ri", checksum)]))

    def test_a_relocated_record_keeps_its_mtime(self):
        record, expected = self.moved_record()
        os.utime(record, ns=(ODD_NS, ODD_NS))

        self.relocate()

        self.assertEqual(snapshot(record), (expected, ODD_NS))

    def test_a_cross_compile_record_is_relocated(self):
        record, expected = self.moved_record(
            "x86_64-unknown-linux-gnu/debug/.fingerprint/u-0123456789abcdef/dep-lib-u")

        self.relocate()

        self.assertEqual(record.read_bytes(), expected)

    def test_files_outside_fingerprint_records_are_untouched(self):
        record, expected = self.moved_record()
        depfile = self.write_record("debug/deps/alpha-0123456789abcdef.d",
                                    f"{self.foreign}/examples/x.ri:\n".encode())
        output = self.write_record("debug/build/p-0123456789abcdef/output",
                                   f"cargo:rustc-env=X={self.foreign}/y\n".encode())
        before = {path: snapshot(path) for path in (depfile, output)}

        self.relocate()

        self.assertEqual(record.read_bytes(), expected)
        self.assertEqual({path: snapshot(path) for path in (depfile, output)}, before)

    def test_an_undecodable_record_naming_the_foreign_root_is_evicted(self):
        valid = encode_depinfo([(1, f"{self.foreign}/examples/x.ri", None)])
        undecodable = {
            "unknown version": valid[:VERSION_BYTE] + b"\x02" + valid[VERSION_BYTE + 1:],
            "truncated": valid[:-3],
            "trailing bytes": valid + b"\x00",
        }
        for why, data in undecodable.items():
            with self.subTest(why):
                record = self.write_record(DEP_RECORD, data)

                result = self.relocate()

                self.assertFalse(record.exists())
                self.assertIn(str(record), result.stderr)

    def test_an_undecodable_record_without_the_foreign_root_is_untouched(self):
        valid = encode_depinfo([(1, "/elsewhere/examples/x.ri", None)])
        record = self.write_record(
            DEP_RECORD, valid[:VERSION_BYTE] + b"\x02" + valid[VERSION_BYTE + 1:])
        before = snapshot(record)

        self.relocate()

        self.assertEqual(snapshot(record), before)

    def test_a_record_without_the_foreign_root_is_untouched(self):
        record = self.write_record(DEP_RECORD, encode_depinfo(
            [(0, "tests/a.rs", None), (1, "/elsewhere/examples/x.ri", None)]))
        before = snapshot(record)

        self.relocate()

        self.assertEqual(snapshot(record), before)

    def test_a_root_that_is_not_an_absolute_unslashed_path_is_refused(self):
        bad_roots = {
            "empty --from-root": ("", self.lane),
            "filesystem-root --from-root": ("/", self.lane),
            "relative --from-root": (BUILD_ROOT_NAME, self.lane),
            "trailing-slash --from-root": (f"{self.foreign}/", self.lane),
            "empty --to-root": (self.foreign, ""),
            "relative --to-root": (self.foreign, "lanes/_lane-1"),
            "trailing-slash --to-root": (self.foreign, f"{self.lane}/"),
        }
        for why, (from_root, to_root) in bad_roots.items():
            with self.subTest(why):
                record, _ = self.moved_record()
                before = snapshot(record)

                result = self.run_helper(from_root, to_root)

                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertEqual(snapshot(record), before)

    @unittest.skipIf(os.geteuid() == 0, "root writes through a read-only directory")
    def test_a_record_it_cannot_replace_fails_closed(self):
        record, _ = self.moved_record()
        before = snapshot(record)
        record.parent.chmod(0o555)
        self.addCleanup(record.parent.chmod, 0o755)

        result = self.run_helper(self.foreign, self.lane)

        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertEqual(snapshot(record), before)

    def test_records_none_of_which_name_the_foreign_root_warn(self):
        self.write_record(DEP_RECORD, encode_depinfo([(1, "/elsewhere/examples/x.ri", None)]))

        result = self.relocate()

        self.assertIn(MISMATCH_WARNING, result.stderr)

    def test_no_mismatch_warning_without_records_or_with_a_foreign_one(self):
        with self.subTest("no records"):
            self.assertNotIn(MISMATCH_WARNING, self.relocate().stderr)

        with self.subTest("one record of two names the foreign root"):
            self.moved_record()
            self.write_record("debug/.fingerprint/u-0123456789abcdef/dep-lib-u",
                              encode_depinfo([(1, "/elsewhere/examples/x.ri", None)]))

            self.assertNotIn(MISMATCH_WARNING, self.relocate().stderr)


if __name__ == "__main__":
    unittest.main()
