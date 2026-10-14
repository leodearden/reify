#!/usr/bin/env python3
"""
test_fd_probe_self_reference.py — stdlib unittest for
scripts/check-fd-probe-self-reference.py.

Discovered through the thin wrapper tests/infra/test_fd_probe_self_reference.sh
(run_all.sh globs `test_*.sh` only). The checker is driven strictly as a
black-box subprocess through its CLI/JSON contract. Rationale and scope:
tests/infra/README.md "Self-referential fd-probe guard".
"""

import json
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
CHECKER = REPO_ROOT / "scripts" / "check-fd-probe-self-reference.py"


def run_checker(*args, cwd=None):
    return subprocess.run(
        [sys.executable, str(CHECKER), *args],
        capture_output=True,
        text=True,
        cwd=cwd,
    )


class CheckerTestCase(unittest.TestCase):
    def make_tmpdir(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        return Path(tmp.name)

    def write_fixture(self, lines, name="fixture.sh"):
        path = self.make_tmpdir() / name
        path.write_text("".join(line + "\n" for line in lines))
        return path

    def run_json(self, *args, cwd=None):
        proc = run_checker("--json", *args, cwd=cwd)
        try:
            payload = json.loads(proc.stdout)
        except json.JSONDecodeError:
            self.fail(
                f"checker stdout is not JSON (exit {proc.returncode})\n"
                f"stdout: {proc.stdout!r}\nstderr: {proc.stderr!r}"
            )
        return proc, payload


class BashPremiseTest(CheckerTestCase):
    """Characterises bash, not repo code: which fd-1 spellings are vacuous."""

    VACUOUS_SPELLINGS = (
        "/proc/self/fd/1",
        "/proc/$BASHPID/fd/1",
        "/proc/thread-self/fd/1",
        "/dev/fd/1",
    )

    def setUp(self):
        tmp = self.make_tmpdir().resolve()
        self.stdin_file = tmp / "stdin.txt"
        self.stdin_file.write_text("probe stdin\n")
        self.out_file = tmp / "stdout.txt"

    def readback(self, assignment):
        """Run bash `assignment` (it sets $r) on file-backed stdin/stdout."""
        with open(self.stdin_file) as stdin, open(self.out_file, "w") as stdout:
            proc = subprocess.run(
                ["bash", "-c", f"{assignment}; printf '%s' \"$r\" >&2"],
                stdin=stdin,
                stdout=stdout,
                stderr=subprocess.PIPE,
                text=True,
                check=True,
            )
        return proc.stderr

    def test_self_fd1_inside_substitution_reads_the_capture_pipe(self):
        for spelling in self.VACUOUS_SPELLINGS:
            with self.subTest(spelling=spelling):
                seen = self.readback(f"r=$(readlink {spelling})")
                self.assertTrue(seen.startswith("pipe:"), seen)

    def test_pid_captured_outside_reads_the_real_stdout(self):
        seen = self.readback('_p=$BASHPID; r=$(readlink "/proc/$_p/fd/1")')
        self.assertEqual(seen, str(self.out_file))

    def test_fd0_is_inherited_into_the_substitution(self):
        seen = self.readback("r=$(readlink /proc/self/fd/0)")
        self.assertEqual(seen, str(self.stdin_file))


class DetectionTest(CheckerTestCase):
    CASES = [
        ("#!/usr/bin/env bash", False),
        ("x=$(readlink /proc/self/fd/1)", True),
        ('y="$(readlink -f /proc/self/fd/1 2>/dev/null || true)"', True),
        ("z=`readlink /proc/self/fd/1`", True),
        ('w=$(readlink "/proc/$BASHPID/fd/1")', True),
        ("v=$(readlink /proc/${BASHPID}/fd/1)", True),
        ("s=$(readlink /dev/fd/1)", True),
        ("s=$(readlink /proc/thread-self/fd/1)", True),
        ("printf '%s' \"$(basename \"$(readlink /proc/self/fd/1)\")\"", True),
        ("cat <(readlink /proc/self/fd/1)", True),
        ("bash -c 'u=$(readlink /proc/self/fd/1)'", True),
        ('_p=$BASHPID; t=$(readlink "/proc/$_p/fd/1")', False),
        ("t=$(readlink /proc/self/fd/0 2>/dev/null || echo unknown)", False),
        ("t=$(readlink /proc/self/fd/2)", False),
        ("t=$(readlink /proc/self/fd/10)", False),
        ("t=$(readlink /dev/fd/0)", False),
        ("t=$(readlink /dev/fd/10)", False),
        ('t=$(curl -so /dev/stdout "$url")', False),
        ("t=$(readlink /proc/$$/fd/1)", False),
        ("out=$(date +%s); printf '%s\\n' \"$out\" >/proc/self/fd/1", False),
        ("n=$(( 1 + 1 ))", False),
        ("    # never write $(readlink /proc/self/fd/1)", False),
        ("x=$(readlink /proc/self/fd/1)  # fdprobe:allow demonstration of the hazard", False),
    ]

    def test_flags_exactly_the_self_referential_lines(self):
        lines = [line for line, _ in self.CASES]
        fixture = self.write_fixture(lines)
        _, payload = self.run_json(str(fixture))

        expected = {i + 1 for i, (_, flagged) in enumerate(self.CASES) if flagged}
        found = {f["line"] for f in payload["findings"]}
        self.assertEqual(found, expected, json.dumps(payload["findings"], indent=2))

        for finding in payload["findings"]:
            self.assertEqual(finding["path"], str(fixture))
            self.assertEqual(finding["source"], lines[finding["line"] - 1])


class ExitContractTest(CheckerTestCase):
    DIRTY = ["#!/usr/bin/env bash", "x=$(readlink /proc/self/fd/1)", "echo ok",
             "y=`readlink /proc/self/fd/1`"]
    CLEAN = ["#!/usr/bin/env bash", '_p=$BASHPID; t=$(readlink "/proc/$_p/fd/1")']

    def test_findings_exit_1_with_json_shape(self):
        fixture = self.write_fixture(self.DIRTY)
        proc, payload = self.run_json(str(fixture))
        self.assertEqual(proc.returncode, 1, proc.stderr)
        self.assertIsInstance(payload, dict)
        self.assertEqual(set(payload), {"scanned", "findings"})
        self.assertEqual(payload["scanned"], [str(fixture)])
        self.assertEqual(len(payload["findings"]), 2)
        for finding in payload["findings"]:
            self.assertEqual(set(finding), {"path", "line", "source"})

    def test_clean_exit_0(self):
        fixture = self.write_fixture(self.CLEAN)
        proc, payload = self.run_json(str(fixture))
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(payload["findings"], [])
        self.assertEqual(payload["scanned"], [str(fixture)])

    def test_missing_path_exit_2_names_the_path(self):
        missing = self.make_tmpdir() / "does-not-exist.sh"
        proc = run_checker(str(missing))
        self.assertEqual(proc.returncode, 2, proc.stderr)
        self.assertIn(str(missing), proc.stderr)

    def test_human_output_one_prefix_per_finding(self):
        fixture = self.write_fixture(self.DIRTY)
        proc = run_checker(str(fixture))
        self.assertEqual(proc.returncode, 1, proc.stderr)
        prefixes = [f"{fixture}:{n}:" for n in (2, 4)]
        out_lines = proc.stdout.splitlines()
        for prefix in prefixes:
            matching = [line for line in out_lines if line.startswith(prefix)]
            self.assertEqual(len(matching), 1, f"{prefix!r} in {proc.stdout!r}")
        self.assertEqual(
            len([line for line in out_lines if line.startswith(f"{fixture}:")]),
            len(prefixes),
            proc.stdout,
        )


class DiscoveryScopeTest(CheckerTestCase):
    """Default discovery: tracked `*.sh` and `hooks/*` only, relative paths."""

    CENSUS_LINE = "x=$(readlink /proc/self/fd/1)"

    def git(self, root, *args):
        subprocess.run(["git", *args], cwd=root, check=True, capture_output=True)

    def test_scans_tracked_shell_corpus_only(self):
        root = self.make_tmpdir()
        self.git(root, "init", "-q")
        tracked = {
            "a.sh": [self.CENSUS_LINE],
            "hooks/pre-demo": ["#!/usr/bin/env bash", self.CENSUS_LINE],
            "lib/helper.py": [self.CENSUS_LINE],
            "notes/n.md": [self.CENSUS_LINE],
        }
        for rel, lines in tracked.items():
            path = root / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("".join(line + "\n" for line in lines))
        self.git(root, "add", *tracked)
        (root / "b.sh").write_text(self.CENSUS_LINE + "\n")

        proc, payload = self.run_json("--root", str(root))
        in_scope = {"a.sh", "hooks/pre-demo"}
        self.assertEqual(proc.returncode, 1, proc.stderr)
        self.assertEqual(set(payload["scanned"]), in_scope)
        self.assertEqual({f["path"] for f in payload["findings"]}, in_scope)


class RealTreeTest(CheckerTestCase):
    """The guard's real job: the tracked shell corpus scans clean, LIVE."""

    CORPUS_FLOOR = 200
    KNOWN_MEMBERS = {
        "tests/infra/test_run_gui_scripts.sh",
        "hooks/pre-commit",
        "tests/infra/test_seed_warm_lane.sh",
    }

    def test_tracked_shell_corpus_is_clean(self):
        proc, payload = self.run_json(cwd=self.make_tmpdir())
        self.assertEqual(
            proc.returncode, 0,
            json.dumps(payload["findings"], indent=2) + proc.stderr,
        )
        self.assertEqual(payload["findings"], [])
        self.assertGreaterEqual(len(payload["scanned"]), self.CORPUS_FLOOR)
        self.assertLessEqual(self.KNOWN_MEMBERS, set(payload["scanned"]))


class MutationControlTest(CheckerTestCase):
    """Reverting Block V's fix at its origin site must flag exactly once.

    Reads the live origin file on purpose, as the flock guard's Cycle 3 does:
    if the shim is reshaped so a reverted probe would escape the checker, this
    goes red. The fixed probe is found by shape, not by its variable's name.
    """

    ORIGIN = REPO_ROOT / "tests" / "infra" / "test_seed_warm_lane.sh"
    PID_VARIABLE_PROBE = re.compile(r"/proc/\$\w+/fd/1(?![0-9])")
    CENSUS_SPELLINGS = ("/proc/self/fd/1", "/proc/$BASHPID/fd/1")

    def fixed_line_index(self, lines):
        hits = [
            i for i, line in enumerate(lines)
            if self.PID_VARIABLE_PROBE.search(line)
            and not line.lstrip().startswith("#")
        ]
        self.assertEqual(
            len(hits), 1,
            f"precondition: expected exactly one non-comment line in "
            f"{self.ORIGIN} matching {self.PID_VARIABLE_PROBE.pattern!r} "
            f"(Block V's fixed shim), found {len(hits)}",
        )
        return hits[0]

    def test_reverted_fix_is_flagged_once(self):
        lines = self.ORIGIN.read_text().split("\n")
        index = self.fixed_line_index(lines)
        for spelling in self.CENSUS_SPELLINGS:
            with self.subTest(spelling=spelling):
                mutant = list(lines)
                mutant[index] = self.PID_VARIABLE_PROBE.sub(
                    lambda _: spelling, mutant[index]
                )
                path = self.make_tmpdir() / self.ORIGIN.name
                path.write_text("\n".join(mutant))
                _, payload = self.run_json(str(path))
                self.assertEqual(len(payload["findings"]), 1, payload["findings"])
                self.assertIn(spelling, payload["findings"][0]["source"])


if __name__ == "__main__":
    unittest.main()
