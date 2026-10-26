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

LaneForensicsTests drives scripts/verify-ledger.py run-begin + finalize as a
CLI against a fixture lane (stamp, fingerprint mtimes, agent start log) and
reads the record's lane_seed block; its last case EXECUTES the real
scripts/verify.sh `typecheck` with stub cargo/sccache in that lane.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its thin
wrapper tests/infra/test_verify_ledger_lane_seed.sh. Schema:
docs/notes/verify-instrumentation-ledger.md.
"""

import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from verify_ledger_env import hermetic_env  # noqa: E402  (imported after sys.path manipulation)

REPO_ROOT = Path(__file__).resolve().parents[2]
SEED = REPO_ROOT / "scripts" / "seed-warm-lane.sh"
SHIM_DIR = REPO_ROOT / "scripts" / "agent-bin"
TOOL = REPO_ROOT / "scripts" / "verify-ledger.py"

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


# Seed exit time of every forensics fixture; whole seconds keep every mtime exact.
S = 1_700_000_000.0
FORENSICS_PLAN = [
    "./scripts/check-manifold-deps.sh",
    "cargo clippy --workspace --all-targets -- -D warnings",
    "cargo nextest run --workspace",
]
# (index, began_at, ended_at, rc) for each executed step of FORENSICS_PLAN.
FORENSICS_STEPS = [(0, S + 90, S + 95, 0), (1, S + 100, S + 120, 0), (2, S + 130, S + 150, 0)]
VERIFY_STARTS = [
    {"source": "verify", "started_at": S + 100, "index": 1, "phase": "clippy"},
    {"source": "verify", "started_at": S + 130, "index": 2, "phase": "debug_nextest"},
]

def run_tool(*args):
    return subprocess.run(
        ["python3", str(TOOL), *args], env=hermetic_env(), capture_output=True, text=True, timeout=120
    )


def tree_state(root):
    return {
        str(p): (p.read_bytes() if p.is_file() else None, p.stat().st_mtime_ns)
        for p in sorted(Path(root).rglob("*"))
    }


class LaneFixture:
    """A pool holding one lane (its target, fingerprints, agent log) plus journals and a ledger."""

    def __init__(self, add_cleanup):
        self.root = Path(tempfile.mkdtemp(prefix="vl-forensics-"))
        add_cleanup(shutil.rmtree, self.root, True)
        self.pool = self.root / "pool"
        self.lane = self.pool / "lane"
        self.target = self.lane / "target"
        self.ledger = self.root / "ledger"
        self.target.mkdir(parents=True)

    def write_stamp(self):
        stamp = {
            "schema": "reify.lane-seed/1",
            "seed_id": "0123456789abcdef",
            "lane_dir": os.path.realpath(self.lane),
            "mode": "fresh-checkout",
            "started_at": S - 30,
            "exited_at": S,
        }
        stamp_path(self.lane).write_text(json.dumps(stamp) + "\n")
        return stamp

    def unit(self, profile, name, *mtimes):
        unit_dir = self.target / profile / ".fingerprint" / name
        unit_dir.mkdir(parents=True)
        for position, mtime in enumerate(mtimes):
            path = unit_dir / f"fingerprint-{position}"
            path.write_text("fp")
            os.utime(path, (mtime, mtime))

    def agent_log(self, *lines):
        (self.target / START_LOG).write_text("".join(line + "\n" for line in lines))

    def record(self, steps=FORENSICS_STEPS):
        """run-begin, the step events verify.sh would write, finalize; the appended record."""
        journal = Path(tempfile.mkdtemp(prefix="journal-", dir=self.root))
        (journal / "plan.nul").write_text("".join(line + "\0" for line in FORENSICS_PLAN))
        begin = run_tool(
            "run-begin", "--journal", str(journal), "--run-id", journal.name,
            "--role", "task", "--action", "test", "--profile", "debug", "--scope", "branch",
            "--repo-root", str(REPO_ROOT), "--lane-dir", str(self.lane),
            "--started-at", str(S + 80),
        )
        if begin.returncode != 0:
            raise AssertionError(f"run-begin exited {begin.returncode}: {begin.stderr}")
        with open(journal / "journal.jsonl", "a", encoding="utf-8") as fh:
            for index, began_at, ended_at, rc in steps:
                fh.write(json.dumps({"ev": "step_begin", "index": index, "at": began_at}) + "\n")
                fh.write(json.dumps({"ev": "step_end", "index": index, "at": ended_at, "rc": rc}) + "\n")
        finalize = run_tool(
            "finalize", "--journal", str(journal), "--ledger-dir", str(self.ledger),
            "--exit-code", "0", "--ended-at", str(S + 160),
        )
        if finalize.returncode != 0:
            raise AssertionError(f"finalize exited {finalize.returncode}: {finalize.stderr}")
        lines = (self.ledger / "verify-runs.jsonl").read_text().splitlines()
        return json.loads(lines[-1])


class LaneForensicsTests(unittest.TestCase):
    def setUp(self):
        self.fixture = LaneFixture(self.addCleanup)

    def agent_fixture(self):
        stamp = self.fixture.write_stamp()
        self.fixture.unit("debug", "old-a", S - 100)
        self.fixture.unit("debug", "old-b", S - 100, S - 90)
        self.fixture.unit("debug", "pre", S + 5)
        self.fixture.unit("debug", "w1", S - 100, S + 11)
        self.fixture.unit("release", "w2", S + 12)
        self.fixture.unit("debug", "w3", S + 13)
        self.fixture.unit("debug", "late", S + 35)
        self.fixture.agent_log(
            json.dumps({"started_at": S - 50, "subcommand": "build"}),
            json.dumps({"started_at": S + 10, "subcommand": "build"}),
            '{"started_at": not-json',
            json.dumps({"started_at": S + 30, "subcommand": "test"}),
        )
        return stamp

    def test_agent_first_build_window(self):
        stamp = self.agent_fixture()
        record = self.fixture.record()
        lane_seed = record["lane_seed"]
        self.assertEqual(lane_seed["seed"], stamp)
        self.assertEqual(
            lane_seed["cargo_starts"],
            [
                {"source": "agent", "started_at": S + 10, "subcommand": "build"},
                {"source": "agent", "started_at": S + 30, "subcommand": "test"},
                *VERIFY_STARTS,
            ],
        )
        self.assertEqual(
            lane_seed["fingerprint"],
            {
                "units_total": 7,
                "units_written_since_seed": 5,
                "first_write_at": S + 5,
                "last_write_at": S + 35,
            },
        )
        self.assertEqual(
            lane_seed["unattributed_writes"],
            {"units": 1, "first_write_at": S + 5, "last_write_at": S + 5},
        )
        self.assertEqual(
            lane_seed["first_build"],
            {
                "source": "agent",
                "started_at": S + 10,
                "window_end_at": S + 30,
                "ended_by": "next-cargo-start",
                "units_written": 3,
                "last_unit_write_at": S + 13,
                "seed_to_first_build_secs": 13.0,
                "basis": "fingerprint-mtime",
            },
        )

    def test_malformed_agent_line_is_an_instrument_error_not_a_failure(self):
        self.agent_fixture()
        record = self.fixture.record()
        self.assertEqual(len(record["instrument_errors"]), 1, record["instrument_errors"])
        self.assertIsNotNone(record["lane_seed"]["first_build"])

    def test_verify_first_build_without_an_agent_log(self):
        self.fixture.write_stamp()
        self.fixture.unit("debug", "old", S - 100)
        self.fixture.unit("debug", "pre1", S + 5)
        self.fixture.unit("release", "pre2", S + 8)
        self.fixture.unit("debug", "during", S + 105)
        lane_seed = self.fixture.record()["lane_seed"]
        self.assertEqual(lane_seed["cargo_starts"], VERIFY_STARTS)
        self.assertEqual(
            lane_seed["fingerprint"],
            {
                "units_total": 4,
                "units_written_since_seed": 3,
                "first_write_at": S + 5,
                "last_write_at": S + 105,
            },
        )
        self.assertEqual(
            lane_seed["unattributed_writes"],
            {"units": 2, "first_write_at": S + 5, "last_write_at": S + 8},
        )
        self.assertEqual(
            lane_seed["first_build"],
            {
                "source": "verify",
                "started_at": S + 100,
                "index": 1,
                "phase": "clippy",
                "exited_at": S + 120,
                "rc": 0,
                "seed_to_first_build_secs": 120.0,
                "basis": "verify-step-end",
            },
        )

    def test_no_stamp_no_lane_seed(self):
        self.fixture.unit("debug", "w1", S + 11)
        self.fixture.agent_log(json.dumps({"started_at": S + 10, "subcommand": "build"}))
        record = self.fixture.record()
        self.assertIsNone(record["lane_seed"])
        self.assertEqual(record["instrument_errors"], [])

    def test_finalize_never_touches_the_lane_and_every_run_carries_the_seed(self):
        stamp = self.agent_fixture()
        before = tree_state(self.fixture.pool)
        first = self.fixture.record()
        second = self.fixture.record()
        self.assertEqual(tree_state(self.fixture.pool), before)
        self.assertEqual(first["lane_seed"]["seed"]["seed_id"], stamp["seed_id"])
        self.assertEqual(second["lane_seed"]["seed"]["seed_id"], stamp["seed_id"])

    def test_real_verify_run_in_a_seeded_lane(self):
        written = run_tool(
            "seed-stamp", "--lane-dir", str(self.fixture.lane),
            "--mode", "fresh-checkout", "--started-at", str(time.time()),
        )
        self.assertEqual(written.returncode, 0, written.stderr)
        stamp = read_stamp(self.fixture.lane)
        stubs = self.fixture.root / "stubs"
        stubs.mkdir()
        for name in ("cargo", "sccache"):
            write_executable(stubs / name, "#!/usr/bin/env bash\nexit 0\n")
        cargo_home_bin = Path.home() / ".cargo" / "bin"
        env = hermetic_env(
            PATH=f"{stubs}{os.pathsep}{cargo_home_bin}{os.pathsep}{os.environ['PATH']}",
            DF_VERIFY_ROLE="task",
            REIFY_PSI_GATE_DISABLE="1",
            REIFY_COMPILE_GATE_DISABLE="1",
            REIFY_TS_FRESHNESS_TARGET_DIR=str(self.fixture.root / "ts-freshness-target"),
            REIFY_VERIFY_LEDGER_DIR=str(self.fixture.ledger),
            REIFY_VERIFY_LEDGER_LANE_DIR=str(self.fixture.lane),
        )
        run = subprocess.run(
            ["bash", "scripts/verify.sh", "typecheck"],
            cwd=str(REPO_ROOT),
            env=env,
            capture_output=True,
            text=True,
            timeout=600,
        )
        self.assertEqual(run.returncode, 0, run.stderr)
        (line,) = (self.fixture.ledger / "verify-runs.jsonl").read_text().splitlines()
        record = json.loads(line)
        self.assertEqual(record["instrument_errors"], [])
        self.assertEqual(record["lane_seed"]["seed"]["seed_id"], stamp["seed_id"])
        first_build = record["lane_seed"]["first_build"]
        self.assertEqual((first_build["source"], first_build["phase"]), ("verify", "cargo_check"))
        self.assertEqual(first_build["rc"], 0)


if __name__ == "__main__":
    unittest.main()
