#!/usr/bin/env python3
"""
test_cargo_test_tally.py — stdlib unittest for scripts/cargo-test-tally.py.

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered
member is the thin wrapper tests/infra/test_cargo_test_tally.sh, which invokes
this file.

Every fixture is real captured output (cargo stable, cargo-nextest 0.9.136,
the host's skim), trimmed of Compiling/Finished noise.
"""

import importlib.util
import sys
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
TOOL_PATH = REPO_ROOT / "scripts" / "cargo-test-tally.py"


def _load_tool():
    spec = importlib.util.spec_from_file_location("cargo_test_tally", TOOL_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {TOOL_PATH}")
    module = importlib.util.module_from_spec(spec)
    # Register before exec: @dataclass resolves a field's type through
    # sys.modules[cls.__module__], which is None for an unregistered module.
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


ctt = _load_tool()


# cargo test --no-fail-fast, throwaway crate, 2026-10-01
LIBTEST_GREEN = """\
     Running unittests src/lib.rs (target/debug/deps/tallyprobe-a9e948e700f99f5f)

running 3 tests
test tests::ignored_one ... ignored
test tests::ok_two ... ok
test tests::ok_one ... ok

test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/green.rs (target/debug/deps/green-edf36e2b58adcf2f)

running 1 test
test green_ok ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

   Doc-tests tallyprobe

running 1 test
test src/lib.rs - documented (line 8) ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

all doctests ran in 3.65s; merged doctests compilation took 3.63s
"""

# cargo test --no-fail-fast, throwaway crate, 2026-10-01
LIBTEST_FAILED = """\
     Running tests/green.rs (target/debug/deps/green-edf36e2b58adcf2f)

running 1 test
test green_ok ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running tests/integ.rs (target/debug/deps/integ-4cd64b097533eb85)

running 2 tests
test integ_ok ... ok
test integ_fail ... FAILED

failures:

---- integ_fail stdout ----

thread 'integ_fail' (49094) panicked at tests/integ.rs:2:27:

failures:
    integ_fail

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

error: test failed, to rerun pass `--test integ`
"""

# cargo test -- nomatch, throwaway crate, 2026-10-01
LIBTEST_NO_MATCH = """\
     Running unittests src/lib.rs (target/debug/deps/tallyprobe-a9e948e700f99f5f)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.00s

     Running tests/green.rs (target/debug/deps/green-edf36e2b58adcf2f)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.00s
"""

# cargo test --test crash (SIGABRT), throwaway crate, 2026-10-01
LIBTEST_CRASH = """\
     Running tests/crash.rs (target/debug/deps/crash-83c1c8e636fd23ff)

running 1 test
error: test failed, to rerun pass `--test crash`

Caused by:
  process didn't exit successfully: `/tmp/tallyprobe/target/debug/deps/crash-83c1c8e636fd23ff` (signal: 6, SIGABRT: process abort signal)
"""

# LIBTEST_GREEN's first binary, then a run killed mid-binary
LIBTEST_TRUNCATED = """\
     Running unittests src/lib.rs (target/debug/deps/tallyprobe-a9e948e700f99f5f)

running 3 tests
test tests::ignored_one ... ignored
test tests::ok_two ... ok
test tests::ok_one ... ok

test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/integ.rs (target/debug/deps/integ-4cd64b097533eb85)

running 2 tests
test integ_ok ... ok
"""

# skim cargo wrapper, 300s cap (exits 0), 2026-10-01
SKIM_TIMEOUT = "Error: command timed out after 300s\n"

# cargo test, throwaway crate with a compile error, 2026-10-01
COMPILE_FAILURE = """\
error[E0425]: cannot find value `x` in this scope
error: could not compile `tallyprobe` (test "integ") due to 1 previous error
"""

# reify's own .ri test runner (examples/m11_annotations.ri)
REIFY_RI_RUNNER = "test result: ok. 0 passed; 0 failed; 1 indeterminate\n"

# a nextest-replayed libtest line, alone
INDENTED_RESULT = (
    "    test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; "
    "0 filtered out; finished in 0.00s\n"
)


class TestLibtest(unittest.TestCase):

    def test_green_run_totals_come_from_one_tally(self):
        tally = ctt.tally_text(LIBTEST_GREEN)
        self.assertEqual(len(tally.summaries), 3)
        self.assertEqual({s.dialect for s in tally.summaries},
                         {ctt.Dialect.LIBTEST})
        self.assertEqual(tally.passed, 4)
        self.assertEqual(tally.failed, 0)
        self.assertEqual(tally.skipped, 1)
        self.assertEqual(tally.filtered_out, 0)
        self.assertEqual(tally.binaries, 3)
        self.assertEqual(tally.verdict, ctt.Verdict.GREEN)

    def test_summary_records_its_one_based_line(self):
        lines = LIBTEST_GREEN.splitlines()
        first_result = next(i for i, line in enumerate(lines)
                            if line.startswith("test result:"))
        tally = ctt.tally_text(LIBTEST_GREEN)
        self.assertEqual(tally.summaries[0].line, first_result + 1)

    def test_failing_binary_is_failed(self):
        tally = ctt.tally_text(LIBTEST_FAILED)
        self.assertEqual(tally.verdict, ctt.Verdict.FAILED)
        self.assertEqual(tally.passed, 2)
        self.assertEqual(tally.failed, 1)

    def test_zero_match_filter_is_nothing_ran(self):
        tally = ctt.tally_text(LIBTEST_NO_MATCH)
        self.assertEqual(tally.verdict, ctt.Verdict.NOTHING_RAN)
        self.assertEqual(tally.filtered_out, 4)
        self.assertEqual(tally.passed, 0)

    def test_crashed_binary_is_failed_without_a_summary(self):
        tally = ctt.tally_text(LIBTEST_CRASH)
        self.assertEqual(tally.summaries, ())
        self.assertEqual(tally.verdict, ctt.Verdict.FAILED)

    def test_binary_header_without_result_is_incomplete(self):
        tally = ctt.tally_text(LIBTEST_TRUNCATED)
        self.assertEqual(tally.verdict, ctt.Verdict.INCOMPLETE)

    def test_no_summary_is_incomplete(self):
        for text in ("", SKIM_TIMEOUT, COMPILE_FAILURE):
            with self.subTest(text=text):
                self.assertEqual(ctt.tally_text(text).verdict,
                                 ctt.Verdict.INCOMPLETE)

    def test_only_strict_column_zero_libtest_lines_count(self):
        for text in (REIFY_RI_RUNNER, INDENTED_RESULT):
            with self.subTest(text=text):
                tally = ctt.tally_text(text)
                self.assertEqual(tally.summaries, ())
                self.assertEqual(tally.verdict, ctt.Verdict.INCOMPLETE)

    def test_sgr_colour_escapes_are_ignored(self):
        plain = next(line for line in LIBTEST_GREEN.splitlines()
                     if line.startswith("test result:"))
        coloured = "\x1b[1m" + plain.replace(
            "ok.", "\x1b[32mok\x1b[0m.", 1) + "\x1b[0m"
        tally = ctt.tally_text(coloured)
        self.assertEqual(tally.passed, 2)
        self.assertEqual(tally.verdict, ctt.Verdict.GREEN)

    def test_verdict_values_are_exit_codes(self):
        self.assertEqual(
            {v.name: int(v) for v in ctt.Verdict},
            {"GREEN": 0, "FAILED": 1, "INCOMPLETE": 3, "NOTHING_RAN": 4},
        )


if __name__ == "__main__":
    unittest.main()
