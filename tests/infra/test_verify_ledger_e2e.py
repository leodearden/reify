#!/usr/bin/env python3
"""
test_verify_ledger_e2e.py — the verify instrumentation ledger, end to end.

EXECUTES the real scripts/verify.sh `typecheck` (DF_VERIFY_ROLE=task) to
completion with stub `cargo` and `sccache` first on PATH, and reads the record
it appends to a private ledger. `typecheck` is the vehicle because its plan is
a few cheap repo checks plus one cargo leaf and it holds no test-run semaphore
slot, so it cannot deadlock against an outer verify that holds one. A
--print-plan oracle cannot observe an EXIT-trap write, hence the real run.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its thin
wrapper tests/infra/test_verify_ledger_e2e.sh. Schema and activation rules:
docs/notes/verify-instrumentation-ledger.md.
"""

import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from verify_ledger_env import hermetic_env  # noqa: E402  (imported after sys.path manipulation)

REPO_ROOT = Path(__file__).resolve().parents[2]

STUB_CARGO = """#!/usr/bin/env bash
case " $* " in
    *" check "*)
        printf '%s' "${REIFY_VERIFY_RUN_ID-<unset>}" > "$STUB_STATE/run_id"
        printf '%s' "${REIFY_RUN_ALL_TIMING_OUT-}" > "$STUB_STATE/timing_out"
        if [ -n "${REIFY_RUN_ALL_TIMING_OUT:-}" ]; then
            printf '{"kind":"summary","path":"stub"}\\n' > "$REIFY_RUN_ALL_TIMING_OUT"
        fi
        exit "${STUB_RC:-0}"
        ;;
esac
exit 0
"""

STUB_SCCACHE = """#!/usr/bin/env bash
n=$(( $(cat "$STUB_STATE/sccache_calls" 2>/dev/null || echo 0) + 1 ))
echo "$n" > "$STUB_STATE/sccache_calls"
printf '{"stats":{"requests_not_cacheable":%d,"cache_hits":{"counts":{"Rust":%d},"adv_counts":{}},"cache_misses":{"counts":{"Rust":%d},"adv_counts":{}}},"version":"stub"}\\n' "$n" "$((100 + n))" "$((50 + n))"
"""

def write_executable(path, text):
    path.write_text(text)
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


class StubbedVerify:
    """One private work dir: stub bin, stub state, ledger, fixture lane."""

    def __init__(self):
        self.work = Path(tempfile.mkdtemp(prefix="vl-e2e-"))
        self.bin = self.work / "bin"
        self.state = self.work / "state"
        self.lane = self.work / "lane"
        for d in (self.bin, self.state, self.lane):
            d.mkdir()
        write_executable(self.bin / "cargo", STUB_CARGO)
        write_executable(self.bin / "sccache", STUB_SCCACHE)

    def cleanup(self):
        shutil.rmtree(self.work, ignore_errors=True)

    def env(self, ledger_dir, **overrides):
        cargo_home_bin = Path.home() / ".cargo" / "bin"
        env = hermetic_env(
            PATH=f"{self.bin}{os.pathsep}{cargo_home_bin}{os.pathsep}{os.environ['PATH']}",
            DF_VERIFY_ROLE="task",
            REIFY_PSI_GATE_DISABLE="1",
            REIFY_COMPILE_GATE_DISABLE="1",
            REIFY_TS_FRESHNESS_TARGET_DIR=str(self.work / "ts-freshness-target"),
            REIFY_VERIFY_LEDGER_LANE_DIR=str(self.lane),
            STUB_STATE=str(self.state),
        )
        if ledger_dir is not None:
            env["REIFY_VERIFY_LEDGER_DIR"] = str(ledger_dir)
        env.update(overrides)
        return env

    def run(self, ledger_dir, *args, **env_overrides):
        for name in ("run_id", "timing_out"):
            (self.state / name).unlink(missing_ok=True)
        return subprocess.run(
            ["bash", "scripts/verify.sh", "typecheck", *args],
            cwd=str(REPO_ROOT),
            env=self.env(ledger_dir, **env_overrides),
            capture_output=True,
            text=True,
            timeout=600,
        )

    def seen(self, name):
        path = self.state / name
        return path.read_text() if path.exists() else None


def ledger_lines(ledger_dir):
    path = Path(ledger_dir) / "verify-runs.jsonl"
    return path.read_text().splitlines() if path.exists() else []


def executor_trace(stderr):
    return [line for line in stderr.splitlines() if line.startswith("verify.sh: ")]


class VerifyLedgerE2ETests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.vehicle = StubbedVerify()
        cls.ledger_pass = cls.vehicle.work / "ledger-pass"
        cls.pass_run = cls.vehicle.run(cls.ledger_pass)
        cls.pass_seen = {n: cls.vehicle.seen(n) for n in ("run_id", "timing_out")}

    @classmethod
    def tearDownClass(cls):
        cls.vehicle.cleanup()

    def pass_record(self):
        lines = ledger_lines(self.ledger_pass)
        self.assertEqual(len(lines), 1, self.pass_run.stderr)
        return json.loads(lines[0])

    def test_a_passing_run_appends_one_record(self):
        self.assertEqual(self.pass_run.returncode, 0, self.pass_run.stderr)
        record = self.pass_record()
        self.assertEqual(record["schema"], "reify.verify-run/1")
        self.assertEqual(record["role"], "task")
        self.assertEqual(record["role_raw"], "task")
        self.assertEqual(record["action"], "typecheck")
        self.assertEqual(record["exit_code"], 0)
        self.assertEqual(record["verdict"], "pass")
        self.assertEqual(record["lane"], str(self.vehicle.lane))
        self.assertEqual(record["instrument_errors"], [])

    def test_a_every_step_timed(self):
        steps = self.pass_record()["steps"]
        self.assertTrue(steps)
        for step in steps:
            with self.subTest(index=step["index"], phase=step["phase"]):
                self.assertIsNotNone(step["began_at"])
                self.assertLessEqual(step["began_at"], step["ended_at"])
                self.assertEqual(step["rc"], 0)
        phases = {step["phase"] for step in steps}
        self.assertIn("preflight", phases)
        self.assertIn("cargo_check", phases)

    def test_a_cargo_step_carries_snapshots_and_run_all(self):
        (cargo_step,) = [s for s in self.pass_record()["steps"] if s["phase"] == "cargo_check"]
        before, after = cargo_step["sccache"]["before"], cargo_step["sccache"]["after"]
        self.assertEqual(before["version"], "stub")
        self.assertEqual(after["version"], "stub")
        self.assertLess(
            before["stats"]["requests_not_cacheable"], after["stats"]["requests_not_cacheable"]
        )
        for edge in ("before", "after"):
            self.assertIn("other_verify_runs", cargo_step["concurrency"][edge])
        self.assertEqual(cargo_step["run_all"]["summary"], {"kind": "summary", "path": "stub"})

    def test_a_children_see_the_run_id_and_the_journal_is_removed(self):
        record = self.pass_record()
        self.assertEqual(self.pass_seen["run_id"], record["run_id"])
        timing_out = self.pass_seen["timing_out"]
        self.assertTrue(timing_out)
        self.assertFalse(Path(timing_out).parent.exists())

    def test_b_failing_run_records_fail_and_unrun_steps(self):
        ledger = self.vehicle.work / "ledger-fail"
        run = self.vehicle.run(ledger, STUB_RC="1")
        self.assertEqual(run.returncode, 1)
        self.assertIn("verify.sh: FAILED (exit 1): ", run.stderr)
        (line,) = ledger_lines(ledger)
        record = json.loads(line)
        self.assertEqual(record["exit_code"], 1)
        self.assertEqual(record["verdict"], "fail")
        steps = record["steps"]
        (cargo_index,) = [s["index"] for s in steps if s["phase"] == "cargo_check"]
        self.assertEqual(steps[cargo_index]["rc"], 1)
        later = steps[cargo_index + 1 :]
        self.assertTrue(later)
        for step in later:
            self.assertIsNone(step["began_at"])
            self.assertIsNone(step["ended_at"])

    def test_c_nested_run_records_nothing(self):
        ledger = self.vehicle.work / "ledger-nested"
        run = self.vehicle.run(ledger, REIFY_VERIFY_RUN_ID="outer-id")
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(ledger_lines(ledger), [])
        self.assertEqual(self.vehicle.seen("run_id"), "outer-id")

    def test_d_print_plan_is_untouched(self):
        ledger = self.vehicle.work / "ledger-plan"
        with_ledger = self.vehicle.run(ledger, "--print-plan")
        without = subprocess.run(
            ["bash", "scripts/verify.sh", "typecheck", "--print-plan"],
            cwd=str(REPO_ROOT),
            env=self.vehicle.env(None),
            capture_output=True,
            text=True,
            timeout=600,
        )
        self.assertEqual(with_ledger.returncode, 0)
        self.assertEqual(with_ledger.stdout, without.stdout)
        self.assertFalse(ledger.exists() and any(ledger.iterdir()))

    def test_e_unwritable_ledger_changes_nothing(self):
        blocker = self.vehicle.work / "not-a-dir"
        blocker.write_text("")
        run = self.vehicle.run(blocker / "ledger")
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(executor_trace(run.stderr), executor_trace(self.pass_run.stderr))
        self.assertNotIn("verify-ledger", run.stdout + run.stderr)
        self.assertNotIn("verify-ledger", self.pass_run.stdout + self.pass_run.stderr)


if __name__ == "__main__":
    unittest.main()
