#!/usr/bin/env python3
"""
test_verify_ledger_lane_seed.py — the lane seed stamp and the agent cargo start log.

SeedStampTests EXECUTES the real scripts/seed-warm-lane.sh `<base> <lane>
--fresh-checkout` on a tempdir fixture (the run_helper_real recipe of
test_seed_warm_lane.sh: a `cp` stub that physically copies, a `git` stub with
an empty diff and a fixed rev-parse, a hand-written .warm-base-meta) and reads
the <lane>.seed-stamp.json it leaves beside the lane dir.

ShimStartLogTests EXECUTES scripts/agent-bin/cargo with a stub real cargo
(the hermetic-PATH recipe of test_agent_cargo_shim.sh) from a cwd inside a
fixture lane and reads <lane>/target/.reify-cargo-starts.jsonl.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its thin
wrapper tests/infra/test_verify_ledger_lane_seed.sh. Schema:
docs/notes/verify-instrumentation-ledger.md.
"""

import json
import os
import shutil
import stat
import subprocess
import tempfile
import time
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SEED = REPO_ROOT / "scripts" / "seed-warm-lane.sh"
SHIM_DIR = REPO_ROOT / "scripts" / "agent-bin"

STUB_CP = """#!/usr/bin/env bash
src=""
dest=""
for arg in "$@"; do
    case "$arg" in
        -*) ;;
        *) if [ -z "$src" ]; then src="$arg"; else dest="$arg"; fi ;;
    esac
done
[ -n "$src" ] && [ -n "$dest" ] && /bin/cp -a "$src" "$dest"
exit 0
"""

STUB_GIT = """#!/usr/bin/env bash
for arg in "$@"; do
    if [ "$arg" = "rev-parse" ]; then
        echo abc1234
        exit 0
    fi
done
exit 0
"""

STUB_CARGO = """#!/usr/bin/env bash
echo "STUB_CARGO $*"
exit "${STUB_EXIT_CODE:-0}"
"""

START_LOG = ".reify-cargo-starts.jsonl"


def write_executable(path, text):
    path.write_text(text)
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def stamp_path(lane):
    return Path(str(lane) + ".seed-stamp.json")


def read_stamp(lane):
    return json.loads(stamp_path(lane).read_text())


class SeedFixture:
    """A warm base, a pool dir holding lanes, and the PATH stubs seed needs."""

    def __init__(self, add_cleanup):
        self.root = Path(tempfile.mkdtemp(prefix="vl-seed-"))
        add_cleanup(shutil.rmtree, self.root, True)
        self.stubs = self.root / "stubs"
        self.pool = self.root / "pool"
        self.base = self.root / "base" / "target"
        for d in (self.stubs, self.pool, self.base / "debug"):
            d.mkdir(parents=True)
        (self.base / "debug" / "base_artifact.a").write_text("base artifact\n")
        (self.base.parent / ".warm-base-meta").write_text("RUSTFLAGS=\nINVOCATION=\n")
        write_executable(self.stubs / "cp", STUB_CP)
        write_executable(self.stubs / "git", STUB_GIT)

    def new_lane(self, name):
        lane = self.pool / name
        lane.mkdir()
        return lane

    def env(self):
        env = {
            k: v
            for k, v in os.environ.items()
            if not k.startswith(("GIT_", "REIFY_WARM_LANE_", "REIFY_LANE_"))
        }
        env.update(
            PATH=f"{self.stubs}{os.pathsep}{os.environ['PATH']}",
            RUSTFLAGS="",
            REIFY_TEST_REFLINK_OK="1",
            REIFY_WARM_LANE_RESEED_TRASH_SYNC="1",
            REIFY_WARM_LANE_MOUNT=str(self.root),
            REIFY_WARM_LANE_RERERE_ARM="0",
            REIFY_WARM_LANE_ALLOW_NO_BASE_COMMIT="1",
        )
        return env

    def seed(self, lane, base=None):
        return subprocess.run(
            ["bash", str(SEED), str(base or self.base), str(lane), "--fresh-checkout"],
            cwd=str(lane),
            env=self.env(),
            capture_output=True,
            text=True,
            timeout=300,
        )


class SeedStampTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture = SeedFixture(cls.addClassCleanup)
        cls.lane = cls.fixture.new_lane("lane-a")
        cls.before = time.time()
        cls.run_a = cls.fixture.seed(cls.lane)
        cls.after = time.time()

    def test_successful_seed_writes_a_sibling_stamp(self):
        self.assertEqual(self.run_a.returncode, 0, self.run_a.stderr)
        self.assertTrue(Path(str(self.lane) + ".lock").exists())
        stamp = read_stamp(self.lane)
        self.assertEqual(stamp["schema"], "reify.lane-seed/1")
        self.assertIsInstance(stamp["seed_id"], str)
        self.assertTrue(stamp["seed_id"])
        self.assertEqual(stamp["lane_dir"], os.path.realpath(self.lane))
        self.assertEqual(stamp["mode"], "fresh-checkout")
        self.assertLessEqual(self.before, stamp["started_at"])
        self.assertLessEqual(stamp["started_at"], stamp["exited_at"])
        self.assertLessEqual(stamp["exited_at"], self.after)

    def test_stamp_is_outside_the_lane(self):
        self.assertEqual(
            sorted(p.name for p in self.lane.iterdir() if "seed-stamp" in p.name), []
        )

    def test_stdout_is_still_only_the_lane_target(self):
        self.assertEqual(self.run_a.stdout.splitlines(), [str(self.lane / "target")])

    def test_refused_seed_writes_no_stamp(self):
        lane = self.fixture.new_lane("lane-refused")
        run = self.fixture.seed(lane, base=self.fixture.root / "no-such-base" / "target")
        self.assertEqual(run.returncode, 76, run.stderr)
        self.assertFalse(stamp_path(lane).exists())

    def test_refused_seed_leaves_an_older_stamp_untouched(self):
        lane = self.fixture.new_lane("lane-older")
        first = self.fixture.seed(lane)
        self.assertEqual(first.returncode, 0, first.stderr)
        older = stamp_path(lane).read_bytes()
        run = self.fixture.seed(lane, base=self.fixture.root / "no-such-base" / "target")
        self.assertEqual(run.returncode, 76, run.stderr)
        self.assertEqual(stamp_path(lane).read_bytes(), older)

    def test_second_seed_replaces_the_stamp_with_a_new_seed_id(self):
        lane = self.fixture.new_lane("lane-twice")
        first = self.fixture.seed(lane)
        self.assertEqual(first.returncode, 0, first.stderr)
        first_stamp = read_stamp(lane)
        second = self.fixture.seed(lane)
        self.assertEqual(second.returncode, 0, second.stderr)
        second_stamp = read_stamp(lane)
        self.assertNotEqual(first_stamp["seed_id"], second_stamp["seed_id"])
        self.assertLessEqual(first_stamp["exited_at"], second_stamp["started_at"])
        self.assertEqual(
            sorted(p.name for p in self.fixture.pool.iterdir() if p.name.startswith("lane-twice.")),
            ["lane-twice.lock", "lane-twice.seed-stamp.json"],
        )


class ShimFixture:
    """A pool holding one lane with a target dir, a cwd deep inside it, and a stub real cargo."""

    def __init__(self, add_cleanup):
        self.root = Path(tempfile.mkdtemp(prefix="vl-shim-"))
        add_cleanup(self._cleanup)
        self.stubs = self.root / "stubs"
        self.lane = self.root / "pool" / "lane"
        self.target = self.lane / "target"
        self.cwd = self.lane / "crates" / "some-crate"
        for d in (self.stubs, self.target, self.cwd):
            d.mkdir(parents=True)
        write_executable(self.stubs / "cargo", STUB_CARGO)

    def _cleanup(self):
        self.target.chmod(0o755)
        shutil.rmtree(self.root, ignore_errors=True)

    def write_stamp(self):
        stamp_path(self.lane).write_text(
            json.dumps({"schema": "reify.lane-seed/1", "seed_id": "fixture", "exited_at": 0.0})
        )

    @property
    def start_log(self):
        return self.target / START_LOG

    def start_rows(self):
        if not self.start_log.exists():
            return []
        return [json.loads(line) for line in self.start_log.read_text().splitlines()]

    def cargo(self, *args, **env_overrides):
        env = {
            k: v
            for k, v in os.environ.items()
            if not k.startswith(("REIFY_CPU_ADMIT_", "DF_VERIFY_"))
        }
        env.update(
            PATH=f"{SHIM_DIR}{os.pathsep}{self.stubs}{os.pathsep}/usr/bin{os.pathsep}/bin",
            REIFY_CPU_ADMIT_DISABLE="1",
        )
        env.update(env_overrides)
        return subprocess.run(
            ["cargo", *args],
            cwd=str(self.cwd),
            env=env,
            capture_output=True,
            text=True,
            timeout=120,
        )


class ShimStartLogTests(unittest.TestCase):
    def setUp(self):
        self.fixture = ShimFixture(self.addCleanup)

    def test_heavy_subcommand_in_a_seeded_lane_appends_one_start(self):
        self.fixture.write_stamp()
        before = time.time()
        run = self.fixture.cargo("build", "--release", STUB_EXIT_CODE="3")
        after = time.time()
        self.assertEqual(run.returncode, 3, run.stderr)
        self.assertEqual(run.stdout, "STUB_CARGO build --release\n")
        (row,) = self.fixture.start_rows()
        self.assertEqual(sorted(row), ["started_at", "subcommand"])
        self.assertEqual(row["subcommand"], "build")
        self.assertIsInstance(row["started_at"], float)
        self.assertLessEqual(before, row["started_at"])
        self.assertLessEqual(row["started_at"], after)

    def test_each_heavy_start_is_appended_with_its_own_subcommand(self):
        self.fixture.write_stamp()
        for args in (("build",), ("--offline", "test", "-p", "x"), ("+stable", "clippy")):
            run = self.fixture.cargo(*args)
            self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(
            [row["subcommand"] for row in self.fixture.start_rows()], ["build", "test", "clippy"]
        )

    def test_non_heavy_subcommand_appends_nothing(self):
        self.fixture.write_stamp()
        run = self.fixture.cargo("metadata", "--format-version", "1")
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(run.stdout, "STUB_CARGO metadata --format-version 1\n")
        self.assertFalse(self.fixture.start_log.exists())

    def test_without_a_stamp_nothing_is_written(self):
        run = self.fixture.cargo("build")
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(run.stdout, "STUB_CARGO build\n")
        self.assertFalse(self.fixture.start_log.exists())

    @unittest.skipIf(os.geteuid() == 0, "root ignores directory write permission")
    def test_unwritable_target_still_runs_cargo_unchanged(self):
        unstamped = self.fixture.cargo("build")
        self.fixture.write_stamp()
        self.fixture.target.chmod(0o555)
        run = self.fixture.cargo("build")
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(run.stdout, "STUB_CARGO build\n")
        self.assertEqual(run.stderr, unstamped.stderr)
        self.assertFalse(self.fixture.start_log.exists())


if __name__ == "__main__":
    unittest.main()
