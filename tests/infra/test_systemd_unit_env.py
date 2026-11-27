#!/usr/bin/env python3
"""Contract tests for tests/infra/systemd_unit_env.py, the reader of which
environment keys a systemd unit's tracked files set.

The discovered member is the wrapper tests/infra/test_systemd_unit_env.sh.
test_run_all_ambient_isolation.sh calls the module's CLI as its fourth live
source; the module's contract lives here.
"""

import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
CLI = SCRIPT_DIR / "systemd_unit_env.py"

sys.path.insert(0, str(SCRIPT_DIR))

import systemd_unit_env as sue  # noqa: E402


class UnitFixture(unittest.TestCase):

    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name)

    def write(self, relpath, text):
        path = self.root / relpath
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return path

    def keys_of(self, *files):
        return [(e.key, e.path) for e in sue.environment_keys(list(files))]

    def assert_unit_error(self, files, *fragments):
        with self.assertRaises(sue.UnitEnvError) as caught:
            sue.environment_keys(sue.unit_files(files))
        for fragment in fragments:
            self.assertIn(fragment, str(caught.exception))


class EnvironmentKeys(UnitFixture):

    def test_service_assignment_reports_its_key_and_file(self):
        unit = self.write("a.conf", "[Service]\nEnvironment=FOO=1\n")
        entries = sue.environment_keys([unit])
        self.assertEqual([(e.key, e.path, e.line) for e in entries],
                         [("FOO", unit, 2)])

    def test_one_line_carries_several_assignments_with_quoting(self):
        unit = self.write("a.conf", '[Service]\nEnvironment="A=x y" B=2\n')
        self.assertEqual(self.keys_of(unit), [("A", unit), ("B", unit)])

    def test_environment_outside_service_contributes_nothing(self):
        unit = self.write("a.conf",
                          "[Unit]\nEnvironment=FOO=1\n"
                          "[Install]\nEnvironment=BAZ=1\n"
                          "[Service]\nExecStart=/bin/true\n")
        self.assertEqual(self.keys_of(unit), [])

    def test_comments_and_blank_lines_are_ignored(self):
        unit = self.write("a.conf",
                          "[Service]\n"
                          "# Environment=BAR=1\n"
                          "; Environment=BAR=1\n"
                          "   # Environment=BAR=1\n"
                          "\n"
                          "Environment=FOO=1\n")
        self.assertEqual(self.keys_of(unit), [("FOO", unit)])

    def test_backslash_continues_onto_the_next_line(self):
        unit = self.write("a.conf",
                          "[Service]\nEnvironment=FOO=1 \\\n  BAR=2\n")
        self.assertEqual(self.keys_of(unit), [("BAR", unit), ("FOO", unit)])

    def test_later_drop_in_wins_a_key_origin(self):
        first = self.write("u.service.d/10-a.conf",
                           "[Service]\nEnvironment=SHARED=a ONLY_A=1\n")
        second = self.write("u.service.d/20-b.conf",
                            "[Service]\nEnvironment=SHARED=b\n")
        files = sue.unit_files([self.root / "u.service.d"])
        self.assertEqual(files, [first, second])
        self.assertEqual(self.keys_of(*files),
                         [("ONLY_A", first), ("SHARED", second)])

    def test_empty_assignment_resets_every_earlier_key(self):
        self.write("u.service.d/10-a.conf",
                   "[Service]\nEnvironment=FROM_EARLIER_FILE=1\n")
        last = self.write("u.service.d/20-b.conf",
                          "[Service]\nEnvironment=BEFORE=1\n"
                          "Environment=\n"
                          "Environment=AFTER=1\n")
        files = sue.unit_files([self.root / "u.service.d"])
        self.assertEqual(self.keys_of(*files), [("AFTER", last)])

    def test_out_of_tree_directives_fail_closed(self):
        for directive in ("EnvironmentFile", "PassEnvironment",
                          "UnsetEnvironment"):
            with self.subTest(directive):
                unit = self.write(f"{directive}.conf",
                                  "[Service]\nEnvironment=FOO=1\n"
                                  f"{directive}=SOMETHING\n")
                self.assert_unit_error([unit], directive, f"{unit}:3")

    def test_malformed_assignments_fail_closed(self):
        cases = {
            "unbalanced-quote": 'Environment="FOO=1\n',
            "no-equals": "Environment=FOO\n",
            "bad-key": "Environment=1FOO=1\n",
            "bad-key-char": "Environment=FOO-BAR=1\n",
        }
        for name, line in cases.items():
            with self.subTest(name):
                unit = self.write(f"{name}.conf", "[Service]\n" + line)
                self.assert_unit_error([unit], f"{unit}:2")


class UnitFiles(UnitFixture):

    def test_directory_expands_to_conf_files_in_byte_order(self):
        names = ["b.conf", "Z.conf", "10-x.conf", "a.conf"]
        for name in names:
            self.write(f"d/{name}", "[Service]\n")
        self.write("d/README", "[Service]\nEnvironment=NOT_READ=1\n")
        self.write("d/x.conf.bak", "[Service]\nEnvironment=NOT_READ=1\n")
        files = sue.unit_files([self.root / "d"])
        self.assertEqual([f.name for f in files],
                         ["10-x.conf", "Z.conf", "a.conf", "b.conf"])

    def test_plain_file_stays_as_given(self):
        unit = self.write("x.service", "[Service]\n")
        self.assertEqual(sue.unit_files([unit]), [unit])

    def test_directory_without_conf_fails_closed(self):
        empty = self.root / "empty.d"
        empty.mkdir()
        self.write("empty.d/notes.txt", "")
        self.assert_unit_error([empty], str(empty))

    def test_nonexistent_path_fails_closed(self):
        missing = self.root / "no-such.service.d"
        self.assert_unit_error([missing], str(missing))


class Cli(UnitFixture):

    def run_cli(self, *args, env=None):
        return subprocess.run([sys.executable, str(CLI), *args],
                              capture_output=True, text=True, check=False,
                              env=env)

    def test_prints_sorted_key_and_assignment_site_lines(self):
        self.write("u.service.d/10-a.conf",
                   "[Service]\nEnvironment=ZED=1 ALPHA=1\n")
        self.write("u.service.d/20-b.conf",
                   "[Service]\n# a comment\nEnvironment=MID=1\n")
        drop_in = self.root / "u.service.d"
        result = self.run_cli(str(drop_in))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout,
                         f"ALPHA\t{drop_in}/10-a.conf:2\n"
                         f"MID\t{drop_in}/20-b.conf:3\n"
                         f"ZED\t{drop_in}/10-a.conf:2\n")

    def test_reads_utf8_whatever_the_locale_encoding(self):
        unit = self.write("a.conf",
                          "# warm-lane — ── oracle\n"
                          "[Service]\nEnvironment=FOO=1\n")
        ascii_locale = dict(os.environ, LC_ALL="C", PYTHONCOERCECLOCALE="0",
                            PYTHONUTF8="0")
        result = self.run_cli(str(unit), env=ascii_locale)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, f"FOO\t{unit}:3\n")

    def test_unit_error_exits_1_with_one_diagnostic_line(self):
        unit = self.write("bad.conf",
                          "[Service]\nEnvironmentFile=/etc/foo.env\n")
        result = self.run_cli(str(unit))
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, "")
        lines = result.stderr.splitlines()
        self.assertEqual(len(lines), 1, result.stderr)
        self.assertIn(str(unit), lines[0])
        self.assertIn("EnvironmentFile", lines[0])

    def test_no_arguments_is_a_usage_error(self):
        result = self.run_cli()
        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout, "")


if __name__ == "__main__":
    unittest.main()
