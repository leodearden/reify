#!/usr/bin/env python3
"""
test_no_shared_tmp_python_helpers.py — a helper `.py` an infra member runs
must be minted inside a private `mktemp -d` directory, never as a bare
`mktemp ... .py` file.

Discovered through the thin wrapper
tests/infra/test_no_shared_tmp_python_helpers.sh (run_all.sh globs
`test_*.sh` only). Rationale and scope: tests/infra/README.md
"Python helper scripts live in a private directory".
"""

import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]

MKTEMP = re.compile(r"\bmktemp\b(?P<args>[^)|;&`]*)")
DIR_FLAG = re.compile(r"(?:^|\s)(?:-[a-zA-Z]*d[a-zA-Z]*|--directory)(?=\s|$)")
PY_FILE = re.compile(r"""\.py["']?(?=\s|$)""")


def offenders(text):
    """(lineno, stripped line) for every mktemp minting a bare .py file."""
    found = []
    for lineno, line in enumerate(text.splitlines(), start=1):
        stripped = line.strip()
        if stripped.startswith("#"):
            continue
        for match in MKTEMP.finditer(line):
            args = match.group("args")
            if PY_FILE.search(args) and not DIR_FLAG.search(args):
                found.append((lineno, stripped))
    return found


def tracked_shell_corpus():
    listing = subprocess.run(
        ["git", "-C", str(REPO_ROOT), "ls-files", "-z", "--", "*.sh", "hooks/*"],
        capture_output=True,
        check=True,
    ).stdout.decode()
    return [
        rel for rel in listing.split("\0")
        if rel and (REPO_ROOT / rel).is_file()
    ]


class PythonPremiseTest(unittest.TestCase):
    """Characterises Python, not repo code: a script's directory shadows imports."""

    @staticmethod
    def default_path_env():
        """Ambient env without PYTHONSAFEPATH (which hides the script's dir)."""
        return {k: v for k, v in os.environ.items() if k != "PYTHONSAFEPATH"}

    def test_module_beside_script_shadows_its_import(self):
        with tempfile.TemporaryDirectory() as script_dir, \
                tempfile.TemporaryDirectory() as elsewhere:
            (Path(script_dir) / "yaml.py").write_text("PLANTED = True\n")
            helper = Path(script_dir) / "helper.py"
            helper.write_text(
                "import yaml\nprint(getattr(yaml, 'PLANTED', False))\n"
            )
            proc = subprocess.run(
                [sys.executable, str(helper)],
                capture_output=True,
                text=True,
                cwd=elsewhere,
                env=self.default_path_env(),
            )
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(proc.stdout.strip(), "True")


class PredicateTest(unittest.TestCase):
    FLAGGED = (
        'X="$(mktemp /tmp/foo_parse_XXXXXX.py)"',
        'X="$(mktemp "${TMPDIR:-/tmp}/foo-XXXXXX.py")"',
        'X="$(mktemp "$_TMPBASE/foo-yaml-XXXXXX.py")"',
        "X=$(mktemp --suffix=.py)",
        "X=$(mktemp --suffix .py)",
        "X=$(mktemp -t foo.XXXXXX.py)",
    )
    CLEARED = (
        'D="$(mktemp -d /tmp/foo_parse_XXXXXX)"',
        'D="$(mktemp -d /tmp/foo_parse_XXXXXX.py)"',
        'D="$(mktemp -dt foo.XXXXXX.py)"',
        'D="$(mktemp -td foo.XXXXXX.py)"',
        'D="$(mktemp --directory /tmp/foo.XXXXXX.py)"',
        'X="$D/foo_parse.py"',
        'F="$(mktemp /tmp/fixture_XXXXXX.yaml)"',
        "# was: mktemp /tmp/foo_XXXXXX.py",
    )

    def test_bare_py_mints_are_flagged_once(self):
        for line in self.FLAGGED:
            with self.subTest(line=line):
                self.assertEqual(offenders(line), [(1, line)])

    def test_private_dir_and_non_py_mints_are_cleared(self):
        for line in self.CLEARED:
            with self.subTest(line=line):
                self.assertEqual(offenders(line), [])

    def test_reports_one_based_line_number(self):
        text = "\n".join(
            [self.CLEARED[0], self.CLEARED[1], "    " + self.FLAGGED[0], ""]
        )
        self.assertEqual(offenders(text), [(3, self.FLAGGED[0])])


class RealTreeTest(unittest.TestCase):
    """The guard's real job: the tracked shell corpus scans clean, LIVE."""

    CORPUS_FLOOR = 200
    KNOWN_MEMBERS = {
        "tests/infra/test_cpu_governance_config.sh",
        "tests/infra/test_warm_lane_audit.sh",
        "tests/infra/test_agent_cache_redirect.sh",
        "hooks/pre-commit",
    }

    def test_tracked_shell_corpus_is_clean(self):
        corpus = tracked_shell_corpus()
        self.assertGreaterEqual(len(corpus), self.CORPUS_FLOOR)
        self.assertLessEqual(self.KNOWN_MEMBERS, set(corpus))
        findings = [
            f"{rel}:{lineno}: {line}"
            for rel in corpus
            for lineno, line in offenders(
                (REPO_ROOT / rel).read_text(errors="replace")
            )
        ]
        self.assertEqual(findings, [], "\n" + "\n".join(findings))


if __name__ == "__main__":
    unittest.main()
