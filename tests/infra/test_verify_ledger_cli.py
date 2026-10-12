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


if __name__ == "__main__":
    unittest.main()
