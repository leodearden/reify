#!/usr/bin/env python3
"""Tests for the adoption ratchet (tests/infra/adoption_ratchet.py).

The discovered member is the wrapper tests/infra/test_infra_adoption_ratchet.sh.
Contract: PRD docs/prds/v0_6/run-all-runner-infra-harness.md §3 D9; row grammar
in tests/infra/adoption-ratchet.manifest's header.
"""

import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
INFRA_DIR = Path(__file__).resolve().parent
ENGINE = INFRA_DIR / "adoption_ratchet.py"

sys.path.insert(0, str(INFRA_DIR))

import adoption_ratchet as ar  # noqa: E402

DISCOVERY_ROW = "inline-discovery-loop"


def parse(text):
    return ar.parse_manifest(text, "fixture.manifest")


class TempRoot(unittest.TestCase):
    """A per-test temp dir standing in for the repo root."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)

    def write(self, rel, text):
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return path

    def run_cli(self, manifest_text, *, files=()):
        manifest = self.write("ratchet.manifest", manifest_text)
        for rel, text in files:
            self.write(rel, text)
        return subprocess.run(
            [sys.executable, str(ENGINE), "--manifest", str(manifest),
             "--root", str(self.root)],
            capture_output=True, text=True, check=False)


class ParseManifest(unittest.TestCase):

    def test_well_formed_row(self):
        (row,) = parse("demo-row | foo[0-9]+ | tests/infra/a.sh | 3 | α #8678\n")
        self.assertEqual(row.pattern_id, "demo-row")
        self.assertTrue(row.pattern.search("foo42"))
        self.assertEqual(row.path_glob, "tests/infra/a.sh")
        self.assertEqual(row.ceiling, 3)
        self.assertEqual(row.owner, "α #8678")
        self.assertEqual(row.manifest_line, 1)

    def test_alternation_survives_the_column_separator(self):
        (row,) = parse("alt | reify_git_env_scrub (bash|env) | a.sh | 0 | o\n")
        self.assertEqual(row.pattern.pattern, "reify_git_env_scrub (bash|env)")
        (spaced,) = parse("spaced | left | right | a.sh | 0 | o\n")
        self.assertEqual(spaced.pattern.pattern, "left | right")

    def test_comments_and_blank_lines_are_ignored(self):
        rows = parse("# header | x | y | 0 | z\n\n   # indented\nreal | x | a.sh | 0 | o\n")
        self.assertEqual([r.pattern_id for r in rows], ["real"])
        self.assertEqual(rows[0].manifest_line, 4)

    def assert_rejected(self, row_line):
        text = "# header\ngood-row | x | a.sh | 0 | o\n" + row_line + "\n"
        with self.assertRaises(ar.ManifestError) as caught:
            parse(text)
        self.assertIn("fixture.manifest:3", str(caught.exception))

    def test_malformed_rows_name_their_line(self):
        cases = {
            "fewer than 5 fields": "short | x | a.sh | 0",
            "non-integer ceiling": "bad-ceiling | x | a.sh | many | o",
            "negative ceiling": "neg-ceiling | x | a.sh | -1 | o",
            "non-kebab id": "Bad_Id | x | a.sh | 0 | o",
            "uncompilable ERE": "bad-ere | ( | a.sh | 0 | o",
            "POSIX bracket class": "posix-class | foo[[:space:]]bar | a.sh | 0 | o",
            "backreference": r"backref | (a)\1 | a.sh | 0 | o",
            "class shorthand": r"shorthand | foo\d+ | a.sh | 0 | o",
            "word-boundary anchor": r"boundary | \bfoo | a.sh | 0 | o",
            "backslash inside brackets": r"bracket-escape | [\.]sh | a.sh | 0 | o",
            "extension group": "ext-group | (?:foo)bar | a.sh | 0 | o",
            "lazy quantifier": "lazy | fo*?o | a.sh | 0 | o",
            "duplicate id": "good-row | y | a.sh | 0 | o",
            "empty owner": "no-owner | x | a.sh | 0 |  ",
            "absolute path": "abs-path | x | /etc/passwd | 0 | o",
            "dot-dot path": "up-path | x | tests/../../a.sh | 0 | o",
        }
        for label, row_line in cases.items():
            with self.subTest(label):
                self.assert_rejected(row_line)

    def test_portable_constructs_are_accepted(self):
        for ere in (r"\$[{]?x[}]?", r"test_\*\.sh", "[.]sh", "[]a]", r"a\\d",
                    r"\(?x", "a{2,3}", "(foo|bar)+"):
            with self.subTest(ere):
                (row,) = parse(f"portable | {ere} | a.sh | 0 | o\n")
                self.assertEqual(row.pattern.pattern, ere)

    def test_zero_rows_is_rejected(self):
        with self.assertRaises(ar.ManifestError) as caught:
            parse("# only a header\n\n")
        self.assertIn("fixture.manifest", str(caught.exception))


class CodeLineHits(unittest.TestCase):

    def test_counts_code_lines_only(self):
        text = ("needle on code\n"
                "# needle in a comment\n"
                "    # needle in an indented comment\n"
                "echo needle  # with a trailing comment\n"
                "nothing here\n")
        pattern = re.compile("needle")
        self.assertEqual(ar.code_line_hits(text, pattern), (1, 4))


class Evaluate(TempRoot):

    def result(self, manifest_text):
        (result,) = ar.evaluate(parse(manifest_text), self.root)
        return result

    def test_count_at_ceiling_is_clean(self):
        self.write("a.sh", "needle\nneedle\n")
        result = self.result("r | needle | a.sh | 2 | owner-x\n")
        self.assertEqual(result.count, 2)
        self.assertIsNone(result.violation)

    def test_count_above_ceiling_names_row_owner_and_locations(self):
        self.write("a.sh", "needle SOURCE_TEXT_ONE\nquiet\nneedle SOURCE_TEXT_TWO\n")
        result = self.result("over-row | needle | a.sh | 1 | owner-x\n")
        self.assertEqual(result.count, 2)
        violation = result.violation
        self.assertIsNotNone(violation)
        for fragment in ("over-row", "owner-x", "2", "ceiling 1", "a.sh:1", "a.sh:3"):
            self.assertIn(fragment, violation)
        self.assertNotIn("SOURCE_TEXT", violation)

    def test_count_below_ceiling_is_clean(self):
        self.write("a.sh", "needle\n")
        self.assertIsNone(self.result("r | needle | a.sh | 5 | o\n").violation)

    def test_glob_path_sums_hits_across_files(self):
        self.write("tests/infra/one.sh", "needle\n")
        self.write("tests/infra/two.sh", "needle\nneedle\n")
        self.write("tests/infra/skip.txt", "needle\n")
        result = self.result("g | needle | tests/infra/*.sh | 3 | o\n")
        self.assertEqual(result.count, 3)
        self.assertEqual(result.files, ("tests/infra/one.sh", "tests/infra/two.sh"))
        self.assertIsNone(result.violation)

    def test_path_matching_no_file_is_a_violation(self):
        violation = self.result("stale-row | needle | gone/*.sh | 0 | o\n").violation
        self.assertIsNotNone(violation)
        self.assertIn("stale-row", violation)


class Cli(TempRoot):

    def test_clean_tree_exits_0(self):
        proc = self.run_cli("r | needle | a.sh | 1 | o\n", files=[("a.sh", "needle\n")])
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)

    def test_violation_exits_1_and_names_the_row(self):
        proc = self.run_cli("loud-row | needle | a.sh | 0 | o\n",
                            files=[("a.sh", "needle\n")])
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        self.assertIn("loud-row", proc.stdout)

    def test_malformed_manifest_exits_2(self):
        proc = self.run_cli("broken | ( | a.sh | 0 | o\n", files=[("a.sh", "x\n")])
        self.assertEqual(proc.returncode, 2, proc.stdout + proc.stderr)


class RealManifest(TempRoot):

    def discovery_row(self):
        rows = {row.pattern_id: row for row in ar.load_manifest(ar.DEFAULT_MANIFEST)}
        self.assertIn(DISCOVERY_ROW, rows)
        return rows[DISCOVERY_ROW]

    def test_discovery_row_holds_run_all_at_zero(self):
        row = self.discovery_row()
        self.assertEqual(row.path_glob, "tests/infra/run_all.sh")
        self.assertEqual(row.ceiling, 0)

    def test_discovery_row_matches_every_historical_loop_shape(self):
        pattern = self.discovery_row().pattern
        for shape in ('for _ra_co_file in "$INFRA_DIR"/test_*.sh; do',
                      'for _ra_subset_file in "$INFRA_DIR"/test_*.sh; do',
                      '    for test_file in "$INFRA_DIR"/test_*.sh; do',
                      'for f in "${INFRA_DIR}"/test_*.sh; do'):
            with self.subTest(shape):
                self.assertIsNotNone(pattern.search(shape))

    def test_discovery_row_matches_every_other_glob_form(self):
        pattern = self.discovery_row().pattern
        for shape in ('for f in "$INFRA_DIR/test_"*.sh; do',
                      "for f in \"$INFRA_DIR\"/test_'*'.sh; do",
                      'compgen -G "$INFRA_DIR/test_*.sh"',
                      "find \"$INFRA_DIR\" -maxdepth 1 -name 'test_*.sh'",
                      'ls "$INFRA_DIR"/test_*.sh',
                      'done < <(find "$INFRA_DIR" -name "test_*.sh" | sort)',
                      '    test_*.sh) members+=("$name") ;;'):
            with self.subTest(shape):
                self.assertIsNotNone(pattern.search(shape))

    def test_discovery_row_ignores_the_lib_producer(self):
        pattern = self.discovery_row().pattern
        self.assertIsNone(pattern.search('done < <(classification_discovered_set "$INFRA_DIR")'))

    def test_re_adding_inline_discovery_reds_the_row(self):
        row = self.discovery_row()
        target = self.root / "tests/infra/run_all.sh"
        target.parent.mkdir(parents=True)
        for added in ('for x in "$INFRA_DIR"/test_*.sh; do :; done\n',
                      'mapfile -t members < <(compgen -G "$INFRA_DIR/test_*.sh")\n'):
            with self.subTest(added):
                shutil.copyfile(REPO_ROOT / "tests/infra/run_all.sh", target)
                with target.open("a") as fh:
                    fh.write(added)
                (result,) = ar.evaluate((row,), self.root)
                self.assertEqual(result.count, 1)
                self.assertIsNotNone(result.violation)


class LiveTree(unittest.TestCase):

    def test_no_row_exceeds_its_ceiling(self):
        for result in ar.evaluate(ar.load_manifest(ar.DEFAULT_MANIFEST), REPO_ROOT):
            with self.subTest(result.row.pattern_id):
                self.assertIsNone(result.violation, result.violation)


if __name__ == "__main__":
    unittest.main()
