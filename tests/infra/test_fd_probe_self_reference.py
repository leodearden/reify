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

    SCRIPT = "\n".join([
        "a=$(readlink /proc/self/fd/1)",
        "b=$(readlink /proc/$BASHPID/fd/1)",
        '_p=$BASHPID; c=$(readlink "/proc/$_p/fd/1")',
        "d=$(readlink /proc/self/fd/0)",
        "printf '%s\\n' \"$a\" \"$b\" \"$c\" \"$d\" >&2",
    ])

    def test_self_fd1_inside_substitution_reads_the_capture_pipe(self):
        tmp = self.make_tmpdir()
        stdin_file = tmp / "stdin.txt"
        stdin_file.write_text("probe stdin\n")
        out_file = tmp / "stdout.txt"
        with open(stdin_file) as stdin, open(out_file, "w") as stdout:
            proc = subprocess.run(
                ["bash", "-c", self.SCRIPT],
                stdin=stdin,
                stdout=stdout,
                stderr=subprocess.PIPE,
                text=True,
                check=True,
            )
        a, b, c, d = proc.stderr.splitlines()
        self.assertTrue(a.startswith("pipe:"), a)
        self.assertTrue(b.startswith("pipe:"), b)
        self.assertEqual(c, str(out_file.resolve()))
        self.assertEqual(d, str(stdin_file.resolve()))


class DetectionTest(CheckerTestCase):
    CASES = [
        ("#!/usr/bin/env bash", False),
        ("x=$(readlink /proc/self/fd/1)", True),
        ('y="$(readlink -f /proc/self/fd/1 2>/dev/null || true)"', True),
        ("z=`readlink /proc/self/fd/1`", True),
        ('w=$(readlink "/proc/$BASHPID/fd/1")', True),
        ("v=$(readlink /proc/${BASHPID}/fd/1)", True),
        ("printf '%s' \"$(basename \"$(readlink /proc/self/fd/1)\")\"", True),
        ("cat <(readlink /proc/self/fd/1)", True),
        ("bash -c 'u=$(readlink /proc/self/fd/1)'", True),
        ('_p=$BASHPID; t=$(readlink "/proc/$_p/fd/1")', False),
        ("t=$(readlink /proc/self/fd/0 2>/dev/null || echo unknown)", False),
        ("t=$(readlink /proc/self/fd/2)", False),
        ("t=$(readlink /proc/self/fd/10)", False),
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


if __name__ == "__main__":
    unittest.main()
