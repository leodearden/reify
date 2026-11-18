#!/usr/bin/env python3
"""
test_migration_note_v0_2.py — keeps the v0.2-alpha migration note true
(docs/prds/alpha-spine.md D8 / boundary row B10; introduced by task 9734).

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered
member is the thin wrapper tests/infra/test_migration_note_v0_2.sh, which
invokes this file.

The note docs/releases/v0.2-alpha.md is the single source of every claim
checked here; nothing below restates an expectation. Its grammar, inside the
section whose heading starts `## Breaking changes` (up to the next `## `), is
one `### <title>` entry per breaking change holding:
  * one fenced block with info string `text`: the diagnostic lines `reify
    check` prints for the old form, each starting `error: ` or `Parse error: `
    with no path prefix;
  * a line naming `tests/prd-gate/fixtures/migration_v0_2_<slug>_before.ri`,
    followed by a fenced snippet, and the same for `_after.ri`. A fence's
    subject is the nearest preceding non-blank line;
  * a `**Fix:**` paragraph, which is prose and not asserted.

Always asserted: each snippet equals its fixture file (modulo one trailing
newline), the on-disk migration_v0_2_*.ri set equals the set the note names,
and the note carries exactly one `Closes:` line whose issue set is the one
scripts/release-alpha.sh must close.

With a reify binary vouched for by scripts/reify-bin-freshness.sh (REIFY_BIN
is honoured there), each before fixture must fail `reify check` printing
exactly the quoted error lines, and each after fixture must pass printing
none. With no vouched binary those tests skip and print why.
"""

import re
import subprocess
import unittest
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
NOTE = REPO_ROOT / "docs/releases/v0.2-alpha.md"
FIXTURE_DIR = REPO_ROOT / "tests/prd-gate/fixtures"
FIXTURE_GLOB = "migration_v0_2_*.ri"
FRESHNESS = REPO_ROOT / "scripts/reify-bin-freshness.sh"
EXPECTED_CLOSES = {1, 2, 3, 4}

CHECK_HANG_GUARD_SECS = 300
ERROR_PREFIXES = ("error: ", "Parse error: ")
EXPECTED_INFO = "text"

SECTION_HEADING = "## Breaking changes"
ENTRY_HEADING = "### "
FENCE_OPEN = re.compile(r"^```(?P<info>\S*)\s*$")
FENCE_CLOSE = re.compile(r"^```\s*$")
FIXTURE_REF = re.compile(
    r"(?P<path>tests/prd-gate/fixtures/migration_v0_2_(?P<slug>[a-z0-9_]+?)"
    r"_(?P<side>before|after)\.ri)"
)
LOCATION_PREFIX = re.compile(r"^.*?\.ri(?::\d+:\d+)?: ")
CLOSES_LINE = re.compile(r"^closes:", re.IGNORECASE)
ISSUE_REF = re.compile(r"#([0-9]+)")


@dataclass(frozen=True)
class Entry:
    title: str
    slug: str
    before_path: Path
    after_path: Path
    before_snippet: str
    after_snippet: str
    expected_errors: tuple


@dataclass(frozen=True)
class _Fence:
    subject: str
    info: str
    body: str


def _breaking_section(lines):
    starts = [i for i, line in enumerate(lines) if line.startswith(SECTION_HEADING)]
    if len(starts) != 1:
        raise ValueError(f"expected one `{SECTION_HEADING}` heading, found {len(starts)}")
    body = lines[starts[0] + 1:]
    for i, line in enumerate(body):
        if line.startswith("## "):
            return body[:i]
    return body


def _split_entries(section):
    entries = []
    for line in section:
        if line.startswith(ENTRY_HEADING):
            entries.append((line[len(ENTRY_HEADING):].strip(), []))
        elif entries:
            entries[-1][1].append(line)
    return entries


def _fences(title, lines):
    fences, subject, i = [], "", 0
    while i < len(lines):
        opening = FENCE_OPEN.match(lines[i])
        if not opening:
            if lines[i].strip():
                subject = lines[i]
            i += 1
            continue
        end = next((j for j in range(i + 1, len(lines)) if FENCE_CLOSE.match(lines[j])), None)
        if end is None:
            raise ValueError(f"entry {title!r}: unclosed fence after {subject!r}")
        fences.append(_Fence(subject, opening.group("info"), "\n".join(lines[i + 1:end])))
        subject, i = "", end + 1
    return fences


def _entry(title, lines):
    expected, snippets = None, {}
    for fence in _fences(title, lines):
        if fence.info == EXPECTED_INFO:
            if expected is not None:
                raise ValueError(f"entry {title!r}: more than one `{EXPECTED_INFO}` block")
            expected = tuple(line for line in fence.body.splitlines() if line.strip())
            continue
        ref = FIXTURE_REF.search(fence.subject)
        if ref is None:
            raise ValueError(f"entry {title!r}: fence after {fence.subject!r} names no fixture")
        if ref.group("side") in snippets:
            raise ValueError(f"entry {title!r}: more than one {ref.group('side')} snippet")
        snippets[ref.group("side")] = (ref.group("slug"), ref.group("path"), fence.body)
    for side in ("before", "after"):
        if side not in snippets:
            raise ValueError(f"entry {title!r}: no {side} snippet")
    if not expected:
        raise ValueError(f"entry {title!r}: no `{EXPECTED_INFO}` block of expected errors")
    (before_slug, before_path, before), (after_slug, after_path, after) = (
        snippets["before"], snippets["after"])
    if before_slug != after_slug:
        raise ValueError(f"entry {title!r}: before slug {before_slug!r} != after slug {after_slug!r}")
    return Entry(title, before_slug, Path(before_path), Path(after_path), before, after, expected)


def parse_entries(text):
    """The note's breaking-change entries; ValueError names a malformed one."""
    section = _breaking_section(text.splitlines())
    return [_entry(title, lines) for title, lines in _split_entries(section)]


def error_lines(output):
    """The error lines of `reify check` output, location prefix stripped."""
    found = set()
    for raw in output.splitlines():
        line = raw.rstrip()
        if not line.startswith(ERROR_PREFIXES):
            located = LOCATION_PREFIX.match(line)
            line = line[located.end():] if located else ""
        if line.startswith(ERROR_PREFIXES):
            found.add(line)
    return found


def closes_issue_numbers(text):
    """One issue-number set per `Closes:` line, read as scripts/release-alpha.sh does."""
    return [{int(n) for n in ISSUE_REF.findall(line)}
            for line in text.splitlines() if CLOSES_LINE.match(line)]


def _without_one_trailing_newline(text):
    return text[:-1] if text.endswith("\n") else text


def _note_text(test):
    test.assertTrue(NOTE.is_file(), f"{NOTE.relative_to(REPO_ROOT)} does not exist")
    return NOTE.read_text(encoding="utf-8")


class NoteStructureTests(unittest.TestCase):
    def test_note_exists(self):
        _note_text(self)

    def test_note_has_entries(self):
        self.assertTrue(parse_entries(_note_text(self)), "no `### ` entry under breaking changes")

    def test_one_closes_line_naming_the_expected_issues(self):
        self.assertEqual(closes_issue_numbers(_note_text(self)), [EXPECTED_CLOSES])

    def test_expected_lines_are_error_lines(self):
        for entry in parse_entries(_note_text(self)):
            for line in entry.expected_errors:
                with self.subTest(entry=entry.title, line=line):
                    self.assertTrue(line.startswith(ERROR_PREFIXES), line)

    def test_snippets_equal_their_fixtures(self):
        for entry in parse_entries(_note_text(self)):
            for path, snippet in ((entry.before_path, entry.before_snippet),
                                  (entry.after_path, entry.after_snippet)):
                with self.subTest(fixture=str(path)):
                    fixture = REPO_ROOT / path
                    self.assertTrue(fixture.is_file(), f"{path} does not exist")
                    self.assertEqual(
                        snippet, _without_one_trailing_newline(fixture.read_text(encoding="utf-8")))

    def test_fixture_set_equals_the_named_set(self):
        named = {REPO_ROOT / path for entry in parse_entries(_note_text(self))
                 for path in (entry.before_path, entry.after_path)}
        on_disk = set(FIXTURE_DIR.glob(FIXTURE_GLOB))
        self.assertEqual(sorted(map(str, on_disk - named)), [], "fixtures the note does not name")
        self.assertEqual(sorted(map(str, named - on_disk)), [], "named fixtures missing on disk")

    def test_slugs_are_unique(self):
        slugs = [entry.slug for entry in parse_entries(_note_text(self))]
        self.assertEqual(sorted({s for s in slugs if slugs.count(s) > 1}), [])


_SYNTHETIC_ENTRY = """\
# Title
## Breaking changes since v0.1-alpha
Intro.
### A rule
```text
error: m
```
Before (`tests/prd-gate/fixtures/migration_v0_2_rule_before.ri`):
```
old
```
After (`tests/prd-gate/fixtures/migration_v0_2_rule_after.ri`):
```
new
```
**Fix:** edit.
## Later
```
ignored
```
"""


class MatcherTests(unittest.TestCase):
    def test_located_and_bare_error_lines_match(self):
        self.assertEqual(error_lines("x.ri:3:5: error: m\n"), {"error: m"})
        self.assertEqual(error_lines("error: m\n"), {"error: m"})
        self.assertEqual(error_lines("/a b/x.ri: error: m\n"), {"error: m"})

    def test_parse_error_line_is_kept(self):
        line = "Parse error: 4:13: syntax error in trait body"
        self.assertEqual(error_lines(line + "\n"), {line})

    def test_warnings_and_mid_text_errors_are_ignored(self):
        self.assertEqual(error_lines("warning: W_X: error: m\n"), set())
        self.assertEqual(error_lines("note: saw error: m\n"), set())
        self.assertEqual(error_lines("x.ri:1:1: warning: error: m\n"), set())

    def test_closes_lines_are_read_case_insensitively(self):
        self.assertEqual(closes_issue_numbers("closes: #1, #22\n Closes: #3\nCloses: #4"),
                         [{1, 22}, {4}])

    def test_well_formed_entry_parses(self):
        (entry,) = parse_entries(_SYNTHETIC_ENTRY)
        self.assertEqual(entry, Entry(
            "A rule", "rule",
            Path("tests/prd-gate/fixtures/migration_v0_2_rule_before.ri"),
            Path("tests/prd-gate/fixtures/migration_v0_2_rule_after.ri"),
            "old", "new", ("error: m",),
        ))

    def test_malformed_entries_name_the_entry(self):
        after_block = "After (`tests/prd-gate/fixtures/migration_v0_2_rule_after.ri`):\n```\nnew\n```\n"
        mutants = {
            "no after snippet": (_SYNTHETIC_ENTRY.replace(after_block, ""), "no after snippet"),
            "no expected block": (_SYNTHETIC_ENTRY.replace("```text\nerror: m\n```\n", ""),
                                  "no `text` block"),
            "unbacked fence": (_SYNTHETIC_ENTRY.replace("**Fix:**", "```\nx\n```\n**Fix:**"),
                               "names no fixture"),
            "slug mismatch": (_SYNTHETIC_ENTRY.replace("rule_after", "other_after"), "slug"),
        }
        for what, (text, reason) in mutants.items():
            with self.subTest(what):
                with self.assertRaisesRegex(ValueError, f"'A rule'.*{re.escape(reason)}"):
                    parse_entries(text)


class FixtureBehaviourTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        proc = subprocess.run(
            ["bash", str(FRESHNESS), "resolve", str(REPO_ROOT)],
            capture_output=True, text=True, check=False,
        )
        if proc.returncode in (1, 2):
            raise unittest.SkipTest(proc.stdout.strip())
        if proc.returncode != 0:
            raise RuntimeError(f"{FRESHNESS} resolve exited {proc.returncode}: {proc.stderr}")
        cls.reify = proc.stdout.strip()

    def _check(self, path):
        try:
            proc = subprocess.run(
                [self.reify, "check", str(REPO_ROOT / path)],
                capture_output=True, text=True, timeout=CHECK_HANG_GUARD_SECS,
                cwd=REPO_ROOT, check=False,
            )
        except subprocess.TimeoutExpired:
            self.fail(f"{path}: reify check exceeded the {CHECK_HANG_GUARD_SECS}s hang guard")
        output = "\n".join((proc.stdout, proc.stderr))
        return proc.returncode, error_lines(output), f"{path} rc={proc.returncode}\n{output}"

    def test_before_fixtures_fail_with_the_quoted_errors(self):
        for entry in parse_entries(NOTE.read_text(encoding="utf-8")):
            with self.subTest(fixture=str(entry.before_path)):
                rc, errors, context = self._check(entry.before_path)
                self.assertNotEqual(rc, 0, context)
                self.assertEqual(errors, set(entry.expected_errors), context)

    def test_after_fixtures_pass_without_errors(self):
        for entry in parse_entries(NOTE.read_text(encoding="utf-8")):
            with self.subTest(fixture=str(entry.after_path)):
                rc, errors, context = self._check(entry.after_path)
                self.assertEqual(rc, 0, context)
                self.assertEqual(errors, set(), context)


if __name__ == "__main__":
    unittest.main()
