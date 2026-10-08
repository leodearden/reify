#!/usr/bin/env python3
"""
test_rust_production_view_lib.py — stdlib unittest for
scripts/lib_rust_production_view.sh, the shared Rust literal/comment lexer.

The lib is driven only through its public surface: source it, then run awk
with one of its two exported programs plus a small driver rule.

  RUST_LEXER_AWK            _strip_line, _lexer_open_state, _lexer_reset
  RUST_PRODUCTION_VIEW_AWK  the lexer + the per-line production rule
                            (#[cfg(test)] mod skipper) + the END WARN

WHAT RUNS THIS: not the gate directly. run_all.sh discovers `test_*.sh` only,
so the discovered member is the thin wrapper
tests/infra/test_rust_production_view_lib.sh, which invokes this file. A bare
.py here would be silently never run.

Fixture files are written under tempfile dirs; no fixture file is committed.
"""

import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
LIB = REPO_ROOT / "scripts" / "lib_rust_production_view.sh"

# $1 = lib, $2 = driver; the remaining args are awk options. Input on stdin.
_LEXER_SPAWN = (
    'source "$1" && prog="$RUST_LEXER_AWK\n$2" && shift 2 && awk "$@" "$prog"'
)
# $1 = lib, $2 = file, $3 = driver; the remaining args are awk options.
_VIEW_SPAWN = (
    'source "$1" && prog="$RUST_PRODUCTION_VIEW_AWK\n$3" && file="$2" '
    '&& shift 3 && awk "$@" "$prog" "$file"'
)

PRINT_CODE = "{ print _strip_line($0) }"
PRINT_TAIL = "{ _strip_line($0); print comment_tail }"
PRINT_OPEN_STATE = '{ _strip_line($0) } END { print "<" _lexer_open_state() ">" }'

CODE_VIEW_CASES = [
    ("let x = a / b;", "let x = a / b;"),
    ('let s = "a // b"; x {', 'let s = ""; x {'),
    ("a /* x /* y */ z */ b", "a  b"),
    ('let e = "a\\"{"; }', 'let e = ""; }'),
    ('let r = r#"a"{"#; }', 'let r = r#""#; }'),
    (
        'let b1 = b"{"; let b2 = br#"{"#; let c1 = c"{"; let c2 = cr#"a"{"#;',
        'let b1 = b""; let b2 = br#""#; let c1 = c""; let c2 = cr#""#;',
    ),
    (
        "let c = '{'; let q = '\\''; let h = '\\x7b'; let u = '\\u{7b}'; "
        "let bc = b'{';",
        "let c = ''; let q = ''; let h = ''; let u = ''; let bc = b'';",
    ),
    ("fn f<'a>(x: &'a str) -> &'static str {", "fn f<'a>(x: &'a str) -> &'static str {"),
    ("'outer: loop { break 'outer; }", "'outer: loop { break 'outer; }"),
    ("foo(); // trailing {", "foo(); "),
]

PRODUCTION_VIEW_FIXTURE = """\
pub fn before() {}
#[cfg(test)]
mod tests {
    fn t() { let s = "{"; }
}
pub fn after() {}
#[cfg(test)]
mod more
{
    fn u() {}
}
#[cfg(test)]
fn helper() {}
pub fn last() {}
"""
PRODUCTION_VIEW_VISIBLE = [1, 2, 6, 7, 8, 12, 13, 14]


def run_lexer(driver, text, *awk_args, env=None):
    """RUST_LEXER_AWK + driver over `text` on stdin."""
    return subprocess.run(
        ["bash", "-c", _LEXER_SPAWN, "_", str(LIB), driver, *awk_args],
        input=text, capture_output=True, text=True, env=env,
    )


def run_view(driver, path, *awk_args, env=None):
    """RUST_PRODUCTION_VIEW_AWK + driver over a real FILE (FILENAME is set)."""
    return subprocess.run(
        ["bash", "-c", _VIEW_SPAWN, "_", str(LIB), str(path), driver, *awk_args],
        capture_output=True, text=True, env=env,
    )


def lines_of(result):
    return result.stdout.split("\n")[:-1]


class TempDirCase(unittest.TestCase):

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmpdir = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)

    def write(self, name, text):
        path = self.tmpdir / name
        path.write_text(text)
        return path

    def shadow_awk(self, target=None, script=None):
        """An env whose PATH resolves `awk` to `target` (a symlink) or `script`."""
        shim = self.tmpdir / "awk-shim"
        shim.mkdir()
        awk = shim / "awk"
        if target is not None:
            awk.symlink_to(target)
        else:
            awk.write_text(script)
            awk.chmod(0o755)
        env = dict(os.environ)
        env["PATH"] = f"{shim}{os.pathsep}{env.get('PATH', '')}"
        return env


class TestLexerCodeView(unittest.TestCase):

    def test_each_line_lexes_to_its_production_code(self):
        for raw, expected in CODE_VIEW_CASES:
            with self.subTest(raw=raw):
                result = run_lexer(PRINT_CODE, raw + "\n")
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(lines_of(result), [expected])


class TestCommentTail(unittest.TestCase):

    def test_tail_holds_the_dropped_comment_and_never_leaks(self):
        result = run_lexer(
            PRINT_TAIL, 'foo(); // trailing {\nbar();\nlet m = "x // y";\n'
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(lines_of(result), ["// trailing {", "", ""])


class TestCrossLineState(unittest.TestCase):

    def test_block_comment_and_string_state_carry_across_lines(self):
        text = 'x /* a\nstill {\nend */ y\nlet s = "one\ntwo {\nthree";\n'
        result = run_lexer('{ code = _strip_line($0); print carried_in "|" code }', text)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            lines_of(result),
            ["0|x ", "1|", "1| y", '0|let s = "', "1|", '1|";'],
        )


class TestOpenState(unittest.TestCase):

    def test_each_unterminated_construct_names_its_state(self):
        cases = [
            ('let s = "open', "<string>"),
            ('let r = r#"open', "<raw_string>"),
            ("x /* open", "<block_comment>"),
            ('let s = "closed"; /* c */ let r = r#"x"#; {}', "<>"),
        ]
        for raw, expected in cases:
            with self.subTest(raw=raw):
                result = run_lexer(PRINT_OPEN_STATE, raw + "\n")
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(lines_of(result), [expected])


class TestReset(unittest.TestCase):

    def test_reset_clears_carried_state(self):
        driver = (
            '$0 == "@@reset@@" { _lexer_reset(); next } '
            + PRINT_CODE
            + ' END { print "<" _lexer_open_state() ">" }'
        )
        result = run_lexer(driver, 'let s = "open\n@@reset@@\nlet y = 1; {\n')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(lines_of(result), ['let s = "', "let y = 1; {", "<>"])


class TestProductionView(TempDirCase):

    def test_cfg_test_mod_bodies_are_skipped_and_bare_items_are_not(self):
        path = self.write("view.rs", PRODUCTION_VIEW_FIXTURE)
        result = run_view("{ print FNR }", path)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([int(n) for n in lines_of(result)], PRODUCTION_VIEW_VISIBLE)
        self.assertEqual(result.stderr, "")

    def test_lines_wholly_inside_a_carried_construct_are_skipped(self):
        path = self.write(
            "carried.rs",
            'fn a() {}\n/* start {\n   inside {\nend */ fn b() {}\n'
            'let s = "one\ntwo\nthree";\n',
        )
        result = run_view("{ print FNR }", path)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([int(n) for n in lines_of(result)], [1, 2, 4, 5, 7])
        self.assertEqual(result.stderr, "")

    def test_consumer_rule_reads_the_lexed_code(self):
        path = self.write("code.rs", 'let s = "{";\n')
        result = run_view("{ print code }", path)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(lines_of(result), ['let s = "";'])

    def test_unbalanced_file_warns_at_eof_naming_the_file(self):
        path = self.write("open.rs", "pub fn open() {\n")
        result = run_view("{ }", path)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("lexer state unbalanced at EOF", result.stderr)
        self.assertIn(str(path), result.stderr)

    def test_quiet_eof_silences_the_warning(self):
        path = self.write("open.rs", "pub fn open() {\n")
        result = run_view("{ }", path, "-v", "quiet_eof=1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "")

    def test_balanced_file_does_not_warn(self):
        path = self.write("closed.rs", "pub fn closed() {\n}\n")
        result = run_view("{ }", path)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "")


class TestSourcingIsSideEffectFree(TempDirCase):
    """The gates' awk-failure checks rely on awk first failing at SCAN time."""

    def test_sourcing_runs_no_awk_and_defines_both_programs(self):
        env = self.shadow_awk(script="#!/bin/sh\nexit 1\n")
        result = subprocess.run(
            [
                "bash", "-c",
                'set -euo pipefail; source "$1"; '
                '[[ -n "$RUST_LEXER_AWK" && -n "$RUST_PRODUCTION_VIEW_AWK" ]]',
                "_", str(LIB),
            ],
            capture_output=True, text=True, env=env,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertEqual(result.stderr, "")


@unittest.skipUnless(shutil.which("mawk"), "mawk is not on PATH on this host")
class TestPortability(TempDirCase):
    """POSIX-awk-only: mawk must agree with the default awk."""

    def setUp(self):
        super().setUp()
        self.mawk_env = self.shadow_awk(target=shutil.which("mawk"))

    def test_code_view_agrees_under_mawk(self):
        for raw, expected in CODE_VIEW_CASES:
            with self.subTest(raw=raw):
                default = run_lexer(PRINT_CODE, raw + "\n")
                mawk = run_lexer(PRINT_CODE, raw + "\n", env=self.mawk_env)
                self.assertEqual(mawk.returncode, 0, mawk.stderr)
                self.assertEqual(mawk.stdout, default.stdout)
                self.assertEqual(lines_of(mawk), [expected])

    def test_production_view_agrees_under_mawk(self):
        path = self.write("view.rs", PRODUCTION_VIEW_FIXTURE)
        default = run_view("{ print FNR }", path)
        mawk = run_view("{ print FNR }", path, env=self.mawk_env)
        self.assertEqual(mawk.returncode, 0, mawk.stderr)
        self.assertEqual(mawk.stdout, default.stdout)
        self.assertEqual([int(n) for n in lines_of(mawk)], PRODUCTION_VIEW_VISIBLE)

    def test_the_shim_really_selects_mawk(self):
        result = subprocess.run(
            ["bash", "-c", "awk -W version 2>&1"],
            capture_output=True, text=True, env=self.mawk_env,
        )
        self.assertIn("mawk", result.stdout)


if __name__ == "__main__":
    unittest.main()
