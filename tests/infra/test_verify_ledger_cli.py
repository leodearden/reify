#!/usr/bin/env python3
"""
test_verify_ledger_cli.py — stdlib unittest for scripts/verify-ledger.py.

Drives the tool only as a subprocess CLI (never imports its internals), the
same way scripts/verify.sh, tests/infra/run_all.sh and seed-warm-lane.sh
reach it. run_all.sh discovers `test_*.sh` only, so the gate runs this file
through its thin wrapper tests/infra/test_verify_ledger_cli.sh.

Schema and rationale: docs/notes/verify-instrumentation-ledger.md.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
TOOL = REPO_ROOT / "scripts" / "verify-ledger.py"

SEMAPHORE_ACQUIRE = "@@SEMAPHORE_ACQUIRE@@"
SEMAPHORE_RELEASE = "@@SEMAPHORE_RELEASE@@"
RELEASE_SKIP_MARKER = "echo 'RELEASE-PASS: skipped (delta-clean)'"

# Ambient state a nesting verify / run_all can hand this suite; each would
# change the plan the real verify.sh prints (role, delta-skip, re-entrancy).
_SCRUBBED_ENV = (
    "DF_VERIFY_ROLE",
    "REIFY_INFRA_SUITE_ACTIVE",
    "REIFY_RELEASE_DELTA_SKIP",
    "REIFY_VERIFY_RUN_ID",
    "REIFY_RUN_ALL_TIMING_OUT",
)


def hermetic_env(**overrides):
    env = {
        k: v
        for k, v in os.environ.items()
        if not k.startswith("GIT_") and k not in _SCRUBBED_ENV
    }
    env.update(overrides)
    return env


def print_plan(args, role, cwd=REPO_ROOT):
    """Non-comment lines of the REAL verify.sh --print-plan for `args`."""
    proc = subprocess.run(
        ["bash", "scripts/verify.sh", *args, "--print-plan"],
        cwd=str(cwd),
        env=hermetic_env(DF_VERIFY_ROLE=role, REIFY_RELEASE_DELTA_SKIP="0"),
        capture_output=True,
        text=True,
        timeout=300,
    )
    if proc.returncode != 0:
        raise AssertionError(
            f"verify.sh {args} --print-plan exited {proc.returncode}: {proc.stderr}"
        )
    return [line for line in proc.stdout.splitlines() if line and not line.startswith("#")]


def run_tool(*args, stdin=None, env=None, cwd=None):
    return subprocess.run(
        [sys.executable, str(TOOL), *args],
        input=stdin,
        env=env if env is not None else hermetic_env(),
        cwd=str(cwd) if cwd else None,
        capture_output=True,
        text=True,
        timeout=120,
    )


def classify(lines):
    proc = run_tool("classify", stdin="".join(line + "\0" for line in lines))
    if proc.returncode != 0:
        raise AssertionError(f"classify exited {proc.returncode}: {proc.stderr}")
    return json.loads(proc.stdout)


def git(cwd, *args):
    subprocess.run(
        ["git", "-c", "user.email=t@example.invalid", "-c", "user.name=t", *args],
        cwd=str(cwd),
        env=hermetic_env(),
        check=True,
        capture_output=True,
        timeout=120,
    )


class ClassifyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.merge_lines = print_plan(
            ["all", "--profile", "both", "--scope", "all"], role="merge"
        ) + [SEMAPHORE_ACQUIRE, SEMAPHORE_RELEASE, RELEASE_SKIP_MARKER]
        cls.merge_entries = classify(cls.merge_lines)

    def _flags_by_phase(self, entries):
        by_phase = {}
        for entry in entries:
            by_phase.setdefault(entry["phase"], set()).add(
                (entry["cargo"], entry["nextest"])
            )
        return by_phase

    def test_one_entry_per_line_in_order(self):
        self.assertEqual(
            [entry["index"] for entry in self.merge_entries],
            list(range(len(self.merge_lines))),
        )

    def test_merge_plan_required_phases_and_flags(self):
        expected = {
            "preflight": (False, False),
            "gui_node_launch": (False, False),
            "gui_node_join": (False, False),
            "clippy": (True, False),
            "gui_feature_check": (True, False),
            "release_prebuild": (True, False),
            "infra_pool": (False, False),
            "psi_gate": (False, False),
            "compile_gate": (False, False),
            "debug_nextest": (True, True),
            "release_nextest": (True, True),
            "gui_feature_nextest": (True, True),
            "test_slot_acquire": (False, False),
            "test_slot_release": (False, False),
            "release_skip_marker": (False, False),
        }
        by_phase = self._flags_by_phase(self.merge_entries)
        for phase, flags in expected.items():
            with self.subTest(phase=phase):
                self.assertIn(phase, by_phase)
                self.assertEqual(by_phase[phase], {flags})

    def test_marker_tokens_classified_exactly(self):
        phases = {
            self.merge_lines[e["index"]]: e["phase"] for e in self.merge_entries
        }
        self.assertEqual(phases[SEMAPHORE_ACQUIRE], "test_slot_acquire")
        self.assertEqual(phases[SEMAPHORE_RELEASE], "test_slot_release")
        self.assertEqual(phases[RELEASE_SKIP_MARKER], "release_skip_marker")

    def test_typecheck_plan_cargo_check(self):
        lines = print_plan(["typecheck"], role="background")
        by_phase = self._flags_by_phase(classify(lines))
        self.assertEqual(by_phase.get("cargo_check"), {(True, False)})

    def test_task_branch_selective_infra_lines(self):
        fixture = Path(tempfile.mkdtemp(prefix="vl-classify-"))
        self.addCleanup(shutil.rmtree, fixture, True)
        shutil.copytree(REPO_ROOT / "scripts", fixture / "scripts", symlinks=True)
        shutil.copytree(REPO_ROOT / ".config", fixture / ".config", symlinks=True)
        git(fixture, "init", "-q", "-b", "main")
        git(fixture, "add", "-A")
        git(fixture, "commit", "-qm", "base")
        git(fixture, "checkout", "-qb", "task")
        with open(fixture / "scripts" / "verify.sh", "a", encoding="utf-8") as fh:
            fh.write("# fixture edit\n")
        git(fixture, "commit", "-qam", "edit verify.sh")

        lines = print_plan(
            ["test", "--scope", "branch", "--include-infra"], role="task", cwd=fixture
        )
        selective = [i for i, line in enumerate(lines) if "for _vt in tests/infra/" in line]
        self.assertTrue(selective, "fixture plan carries no selective infra lines")
        entries = classify(lines)
        for i in selective:
            with self.subTest(line=lines[i][:120]):
                self.assertEqual(entries[i]["phase"], "infra_selective")
                self.assertFalse(entries[i]["cargo"])
                self.assertFalse(entries[i]["nextest"])

    def test_cargo_test_fallback_is_cargo_without_nextest(self):
        lines = [
            "timeout --kill-after=60 60m nice -n 5 cargo test --workspace -- --test-threads=1 9<&-",
            "timeout --kill-after=60 90m nice -n 5 cargo test -p reify-eval --release -- --test-threads=1 9<&-",
            "if test -f gui/src-tauri/Cargo.toml; then ./scripts/ensure-gui-sidecar-placeholder.sh"
            " && timeout --kill-after=60 45m nice -n 5 cargo test -p reify-gui --features gui"
            " -- --test-threads=1; fi 9<&-",
        ]
        entries = classify(lines)
        self.assertEqual(
            [e["phase"] for e in entries],
            ["debug_cargo_test", "release_cargo_test", "gui_feature_cargo_test"],
        )
        for entry in entries:
            self.assertTrue(entry["cargo"])
            self.assertFalse(entry["nextest"])

    def test_unrecognised_line_is_other(self):
        (entry,) = classify(["echo something-new"])
        self.assertEqual(entry, {"index": 0, "phase": "other", "cargo": False, "nextest": False})


PREFLIGHT_LINE = 'export LD_LIBRARY_PATH="${REIFY_AMBIENT_LD_LIBRARY_PATH-}"; ./scripts/check-manifold-deps.sh'
CLIPPY_LINE = "timeout --kill-after=60 45m nice -n 5 cargo clippy --workspace --all-targets -- -D warnings"
PSI_LINE = 'export LD_LIBRARY_PATH="${REIFY_AMBIENT_LD_LIBRARY_PATH-}"; ./scripts/verify.sh psi-gate'
DEBUG_NEXTEST_LINE = (
    "timeout --kill-after=60 60m nice -n 5 cargo nextest run --workspace"
    " --config-file /tmp/reify-nextest-occt.x 9<&-"
)
RELEASE_NEXTEST_LINE = (
    "timeout --kill-after=60 90m nice -n 5 cargo nextest run -p reify-eval --release"
    " --config-file /tmp/reify-nextest-occt.x 9<&-"
)
POSTFLIGHT_LINE = 'export LD_LIBRARY_PATH="${REIFY_AMBIENT_LD_LIBRARY_PATH-}"; ./scripts/tree-sitter-freshness.sh check'

BASE_PLAN = [
    PREFLIGHT_LINE,
    CLIPPY_LINE,
    PSI_LINE,
    SEMAPHORE_ACQUIRE,
    DEBUG_NEXTEST_LINE,
    RELEASE_NEXTEST_LINE,
    SEMAPHORE_RELEASE,
    POSTFLIGHT_LINE,
]
BASE_PLAN_CARGO_INDICES = [1, 4, 5]


def git_out(*args):
    return subprocess.run(
        ["git", *args],
        cwd=str(REPO_ROOT),
        env=hermetic_env(),
        check=True,
        capture_output=True,
        text=True,
        timeout=60,
    ).stdout.strip()


class JournalFixture:
    """A verify run's journal dir, written the way verify.sh writes it."""

    def __init__(self, test, plan, release_delta_skip="0", lane_dir=None):
        self.dir = Path(tempfile.mkdtemp(prefix="vl-journal-"))
        test.addCleanup(shutil.rmtree, self.dir, True)
        self.lane_dir = lane_dir or Path(tempfile.mkdtemp(prefix="vl-lane-"))
        test.addCleanup(shutil.rmtree, self.lane_dir, True)
        self.plan = plan
        self.run_id = f"run-{os.getpid()}-{id(self)}"
        (self.dir / "plan.nul").write_text("".join(line + "\0" for line in plan))
        self.begin = run_tool(
            "run-begin",
            "--journal", str(self.dir),
            "--run-id", self.run_id,
            "--role", "merge",
            "--role-raw", "merge",
            "--action", "test",
            "--profile", "both",
            "--scope", "all",
            "--repo-root", str(REPO_ROOT),
            "--lane-dir", str(self.lane_dir),
            "--release-delta-skip", release_delta_skip,
            "--started-at", "1000.0",
        )

    def event(self, **fields):
        with open(self.dir / "journal.jsonl", "a", encoding="utf-8") as fh:
            fh.write(json.dumps(fields) + "\n")

    def run_steps(self, count, failing_rc=0):
        for i in range(count):
            self.event(ev="step_begin", index=i, at=1000.0 + 10 * i)
            rc = failing_rc if i == count - 1 else 0
            self.event(ev="step_end", index=i, at=1000.0 + 10 * i + 5, rc=rc)

    def finalize(self, ledger_dir, exit_code, *extra):
        return run_tool(
            "finalize",
            "--journal", str(self.dir),
            "--ledger-dir", str(ledger_dir),
            "--exit-code", str(exit_code),
            "--ended-at", "2000.5",
            *extra,
        )


def ledger_records(ledger_dir):
    path = Path(ledger_dir) / "verify-runs.jsonl"
    if not path.exists():
        return []
    return [json.loads(line) for line in path.read_text().splitlines()]


class RecordTests(unittest.TestCase):
    def setUp(self):
        self.ledger = Path(tempfile.mkdtemp(prefix="vl-ledger-"))
        self.addCleanup(shutil.rmtree, self.ledger, True)

    def test_run_begin_prints_cargo_indices_and_writes_meta(self):
        journal = JournalFixture(self, BASE_PLAN)
        self.assertEqual(journal.begin.returncode, 0, journal.begin.stderr)
        self.assertEqual(
            [int(tok) for tok in journal.begin.stdout.split()], BASE_PLAN_CARGO_INDICES
        )
        self.assertTrue((journal.dir / "meta.json").is_file())

    def test_failed_run_record(self):
        journal = JournalFixture(self, BASE_PLAN)
        journal.run_steps(5, failing_rc=1)
        with open(journal.dir / "journal.jsonl", "a", encoding="utf-8") as fh:
            fh.write('{"ev":"step_beg')  # a writer killed mid-line
        proc = journal.finalize(self.ledger, 1)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(proc.stdout, "")

        (record,) = ledger_records(self.ledger)
        self.assertEqual(record["schema"], "reify.verify-run/1")
        self.assertEqual(record["run_id"], journal.run_id)
        for key, value in {
            "role": "merge",
            "role_raw": "merge",
            "action": "test",
            "profile": "both",
            "scope": "all",
            "lane": str(journal.lane_dir),
            "exit_code": 1,
            "verdict": "fail",
            "signal": None,
            "started_at": 1000.0,
            "ended_at": 2000.5,
        }.items():
            with self.subTest(key=key):
                self.assertEqual(record[key], value)
        self.assertAlmostEqual(record["wall_secs"], 1000.5)
        self.assertEqual(record["head"], git_out("rev-parse", "HEAD"))
        self.assertEqual(record["tree_oid"], git_out("rev-parse", "HEAD:"))
        self.assertEqual(record["branch"], git_out("rev-parse", "--abbrev-ref", "HEAD"))

        steps = record["steps"]
        self.assertEqual([s["index"] for s in steps], list(range(len(BASE_PLAN))))
        self.assertEqual([s["cmd"] for s in steps], BASE_PLAN)
        self.assertEqual(steps[1]["phase"], "clippy")
        for step in steps[:5]:
            self.assertLessEqual(step["began_at"], step["ended_at"])
            self.assertAlmostEqual(step["wall_secs"], 5.0)
        self.assertEqual([s["rc"] for s in steps[:5]], [0, 0, 0, 0, 1])
        for step in steps[5:]:
            for key in ("began_at", "ended_at", "wall_secs", "rc"):
                self.assertIsNone(step[key])

    def test_release_pass_decision(self):
        cases = (
            (BASE_PLAN, "0", "planned"),
            ([PREFLIGHT_LINE, DEBUG_NEXTEST_LINE, RELEASE_SKIP_MARKER], "1", "skipped-delta-clean"),
            ([PREFLIGHT_LINE, DEBUG_NEXTEST_LINE], "0", "absent"),
        )
        for plan, raw, decision in cases:
            with self.subTest(decision=decision):
                ledger = Path(tempfile.mkdtemp(prefix="vl-ledger-"))
                self.addCleanup(shutil.rmtree, ledger, True)
                journal = JournalFixture(self, plan, release_delta_skip=raw)
                journal.finalize(ledger, 0)
                (record,) = ledger_records(ledger)
                self.assertEqual(
                    record["release_pass"],
                    {"decision": decision, "release_delta_skip_raw": raw},
                )
                self.assertEqual(record["verdict"], "pass")

    def test_signal_recorded(self):
        journal = JournalFixture(self, BASE_PLAN)
        journal.event(ev="step_begin", index=0, at=1000.0)
        journal.finalize(self.ledger, 143, "--signal", "TERM")
        (record,) = ledger_records(self.ledger)
        self.assertEqual(record["signal"], "TERM")
        self.assertEqual(record["steps"][0]["began_at"], 1000.0)
        self.assertIsNone(record["steps"][0]["ended_at"])

    def test_finalize_is_once_only(self):
        journal = JournalFixture(self, BASE_PLAN)
        journal.finalize(self.ledger, 0)
        second = journal.finalize(self.ledger, 0)
        self.assertEqual(second.returncode, 0)
        self.assertEqual(len(ledger_records(self.ledger)), 1)

    def test_errors_log_folded_into_record(self):
        journal = JournalFixture(self, BASE_PLAN)
        (journal.dir / "errors.log").write_text("boom: snapshot failed\n")
        journal.finalize(self.ledger, 0)
        (record,) = ledger_records(self.ledger)
        self.assertIn("boom: snapshot failed", record["instrument_errors"])

    def test_unwritable_ledger_is_fail_open(self):
        journal = JournalFixture(self, BASE_PLAN)
        blocker = self.ledger / "a-file"
        blocker.write_text("")
        proc = journal.finalize(blocker / "ledger", 1)
        self.assertEqual(proc.returncode, 0)
        self.assertEqual(proc.stdout, "")

    def test_concurrent_finalizes_append_intact_lines(self):
        long_plan = BASE_PLAN + [f"echo {'x' * 400} {i}" for i in range(60)]
        journals = [JournalFixture(self, long_plan) for _ in range(6)]
        for journal in journals:
            journal.run_steps(len(long_plan))
        procs = [
            subprocess.Popen(
                [sys.executable, str(TOOL), "finalize",
                 "--journal", str(journal.dir),
                 "--ledger-dir", str(self.ledger),
                 "--exit-code", "0",
                 "--ended-at", "2000.5"],
                env=hermetic_env(),
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            for journal in journals
        ]
        for proc in procs:
            self.assertEqual(proc.wait(timeout=120), 0)
        records = ledger_records(self.ledger)
        self.assertEqual(
            sorted(r["run_id"] for r in records), sorted(j.run_id for j in journals)
        )
        for record in records:
            self.assertEqual(len(record["steps"]), len(long_plan))


if __name__ == "__main__":
    unittest.main()
