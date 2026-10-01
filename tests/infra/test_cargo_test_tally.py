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
import json
import re
import subprocess
import sys
import tempfile
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

# cargo test with a filter matching nothing, throwaway crate, 2026-10-01
LIBTEST_NO_MATCH = """\
     Running unittests src/lib.rs (target/debug/deps/tallyprobe-a9e948e700f99f5f)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.00s

     Running tests/green.rs (target/debug/deps/green-edf36e2b58adcf2f)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.00s
"""

# cargo test, a test binary that aborts (SIGABRT), throwaway crate, 2026-10-01
LIBTEST_CRASH = """\
     Running tests/crash.rs (target/debug/deps/crash-83c1c8e636fd23ff)

running 1 test
error: test failed, to rerun pass `--test crash`

Caused by:
  process didn't exit successfully: `…/crash-83c1c8e636fd23ff` (signal: 6, SIGABRT: process abort signal)
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


# cargo nextest run --no-fail-fast, throwaway crate, 2026-10-01
NEXTEST_FAILED = """\
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.50s
────────────
 Nextest run ID ae9599b4-a118-462b-8a12-879c416171ac with nextest profile: default
    Starting 5 tests across 3 binaries (1 test skipped)
        FAIL [   0.063s] (1/5) tallyprobe::integ integ_fail
  stdout ───

    running 1 test
    test integ_fail ... FAILED
    test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.05s
  stderr ───
        PASS [   0.090s] (2/5) tallyprobe tests::ok_one
        PASS [   0.115s] (3/5) tallyprobe::green green_ok
        PASS [   0.126s] (4/5) tallyprobe::integ integ_ok
        PASS [   0.152s] (5/5) tallyprobe tests::ok_two
────────────
     Summary [   0.153s] 5 tests run: 4 passed, 1 failed, 1 skipped
        FAIL [   0.063s] (1/5) tallyprobe::integ integ_fail
error: test run failed
"""

# cargo nextest run --no-capture, throwaway crate, 2026-10-01
NEXTEST_NO_CAPTURE = """\
    Starting 2 tests across 1 binary
       START [         ] (1/2) tallyprobe tests::ok_one

running 1 test
test tests::ok_one ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out; finished in 0.01s

        PASS [   0.029s] (1/2) tallyprobe tests::ok_one
       START [         ] (2/2) tallyprobe tests::ok_two

running 1 test
test tests::ok_two ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out; finished in 0.01s

        PASS [   0.029s] (2/2) tallyprobe tests::ok_two
     Summary [   0.040s] 2 tests run: 2 passed, 0 skipped
"""

# cargo nextest run, one test past its timeout, throwaway crate, 2026-10-01
NEXTEST_TIMED_OUT = """\
    Starting 3 tests across 2 binaries
     TIMEOUT [   0.321s] (3/3) tallyprobe::timing hangs
     Summary [   0.323s] 3 tests run: 2 passed (1 slow), 1 timed out, 0 skipped
"""

# cargo nextest run (fail-fast), throwaway crate, 2026-10-01
NEXTEST_CANCELLED = """\
    Starting 5 tests across 3 binaries (1 test skipped)
  Cancelling due to test failure:
     Summary [   0.144s] 4/5 tests run: 3 passed, 1 failed, 1 skipped
warning: 1/5 tests were not run due to test failure (run with --no-fail-fast to run all tests, or run with --max-fail)
"""

# DERIVED from the measured N/M form above: a partial run with no failure
NEXTEST_INTERRUPTED = """\
    Starting 5 tests across 3 binaries
     Summary [   0.144s] 3/5 tests run: 3 passed, 0 skipped
"""

# cargo nextest run killed after its first test, throwaway crate, 2026-10-01
NEXTEST_KILLED = """\
    Starting 5 tests across 3 binaries
        PASS [   0.090s] (1/5) tallyprobe tests::ok_one
"""

# cargo nextest run with a filter matching nothing, throwaway crate, 2026-10-01
NEXTEST_NO_MATCH = """\
    Starting 0 tests across 3 binaries (6 tests skipped)
     Summary [   0.000s] 0 tests run: 0 passed, 6 skipped
error: no tests to run
"""

# cargo nextest run, one test, throwaway crate, 2026-10-01
NEXTEST_SINGULAR = """\
    Starting 1 test across 1 binary
     Summary [   0.132s] 1 test run: 1 passed, 0 skipped
"""

# data/verify-logs/6979, two nextest runs in one verify log
VERIFY_LOG_TWO_RUNS = """\
    Starting 22745 tests across 526 binaries (62 tests and 6 binaries skipped)
     Summary [ 594.200s] 22745 tests run: 22745 passed (9 slow, 1 leaky), 62 skipped
    Starting 1165 tests across 3 binaries
     Summary [  23.415s] 1165 tests run: 1165 passed, 0 skipped
"""

# cargo nextest run; cargo test --doc, throwaway crate, 2026-10-01
NEXTEST_THEN_DOCTEST = NEXTEST_SINGULAR + """\
   Doc-tests tallyprobe

running 1 test
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
"""

# skim cargo wrapper, throwaway crate, 2026-10-01
SKIM_FAILED = "PASS: 5 | FAIL: 1 | SKIP: 1\n"
SKIM_GREEN = "PASS: 1 | FAIL: 0 | SKIP: 0\n"
SKIM_EMPTY = "PASS: 0 | FAIL: 0 | SKIP: 0\n"


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


class TestNextest(unittest.TestCase):

    def test_replayed_libtest_line_is_not_a_second_summary(self):
        tally = ctt.tally_text(NEXTEST_FAILED)
        self.assertEqual(len(tally.summaries), 1)
        self.assertEqual(tally.summaries[0].dialect, ctt.Dialect.NEXTEST)
        self.assertEqual(tally.passed, 4)
        self.assertEqual(tally.failed, 1)
        self.assertEqual(tally.skipped, 1)
        self.assertEqual(tally.binaries, 3)
        self.assertEqual(tally.verdict, ctt.Verdict.FAILED)

    def test_no_capture_per_test_libtest_lines_are_not_counted(self):
        tally = ctt.tally_text(NEXTEST_NO_CAPTURE)
        self.assertEqual(len(tally.summaries), 1)
        self.assertEqual(tally.passed, 2)
        self.assertEqual(tally.binaries, 1)
        self.assertEqual(tally.verdict, ctt.Verdict.GREEN)

    def test_timed_out_is_a_failure_and_slow_is_not_a_count(self):
        tally = ctt.tally_text(NEXTEST_TIMED_OUT)
        self.assertEqual(tally.passed, 2)
        self.assertEqual(tally.failed, 1)
        self.assertEqual(tally.verdict, ctt.Verdict.FAILED)

    def test_cancelled_run_is_failed_and_incomplete(self):
        tally = ctt.tally_text(NEXTEST_CANCELLED)
        self.assertEqual(tally.verdict, ctt.Verdict.FAILED)
        self.assertFalse(tally.summaries[0].complete)

    def test_partial_run_without_failure_is_incomplete(self):
        tally = ctt.tally_text(NEXTEST_INTERRUPTED)
        self.assertEqual(tally.verdict, ctt.Verdict.INCOMPLETE)
        self.assertEqual(tally.passed, 3)

    def test_region_open_at_eof_is_incomplete(self):
        tally = ctt.tally_text(NEXTEST_KILLED)
        self.assertEqual(tally.summaries, ())
        self.assertEqual(tally.verdict, ctt.Verdict.INCOMPLETE)

    def test_zero_match_filter_is_nothing_ran(self):
        tally = ctt.tally_text(NEXTEST_NO_MATCH)
        self.assertEqual(tally.verdict, ctt.Verdict.NOTHING_RAN)
        self.assertEqual(tally.skipped, 6)

    def test_singular_forms(self):
        tally = ctt.tally_text(NEXTEST_SINGULAR)
        self.assertEqual(tally.passed, 1)
        self.assertEqual(tally.binaries, 1)
        self.assertEqual(tally.verdict, ctt.Verdict.GREEN)

    def test_two_runs_in_one_log_are_summed(self):
        tally = ctt.tally_text(VERIFY_LOG_TWO_RUNS)
        self.assertEqual(len(tally.summaries), 2)
        self.assertEqual(tally.passed, 23910)
        self.assertEqual(tally.skipped, 62)
        self.assertEqual(tally.binaries, 529)
        self.assertEqual(tally.verdict, ctt.Verdict.GREEN)

    def test_closed_region_does_not_swallow_later_libtest(self):
        tally = ctt.tally_text(NEXTEST_THEN_DOCTEST)
        self.assertEqual([s.dialect for s in tally.summaries],
                         [ctt.Dialect.NEXTEST, ctt.Dialect.LIBTEST])
        self.assertEqual(tally.passed, 2)


class TestSkim(unittest.TestCase):

    def test_skim_line_is_a_summary_without_binaries(self):
        tally = ctt.tally_text(SKIM_FAILED)
        self.assertEqual(len(tally.summaries), 1)
        summary = tally.summaries[0]
        self.assertEqual(summary.dialect, ctt.Dialect.SKIM)
        self.assertEqual((summary.passed, summary.failed, summary.skipped),
                         (5, 1, 1))
        self.assertIsNone(summary.binaries)
        self.assertIsNone(tally.binaries)
        self.assertEqual(tally.verdict, ctt.Verdict.FAILED)

    def test_skim_verdicts(self):
        self.assertEqual(ctt.tally_text(SKIM_GREEN).verdict, ctt.Verdict.GREEN)
        self.assertEqual(ctt.tally_text(SKIM_EMPTY).verdict,
                         ctt.Verdict.NOTHING_RAN)

    def test_unknown_binaries_make_the_total_unknown_not_the_counts(self):
        tally = ctt.tally_text(SKIM_GREEN + LIBTEST_GREEN)
        self.assertIsNone(tally.binaries)
        self.assertEqual(tally.passed, 5)


class TestCli(unittest.TestCase):
    """Drives the tool as a subprocess, the way an agent uses it."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmpdir = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)

    def write_capture(self, text, name="test.log"):
        path = self.tmpdir / name
        path.write_text(text)
        return path

    def run_cli(self, *args, stdin_text=None):
        return subprocess.run(
            [sys.executable, str(TOOL_PATH), *args],
            input=stdin_text, capture_output=True, text=True,
        )

    def test_exit_code_is_the_verdict(self):
        cases = [(LIBTEST_GREEN, 0), (LIBTEST_FAILED, 1), ("", 3),
                 (LIBTEST_NO_MATCH, 4), (SKIM_FAILED, 1)]
        for text, code in cases:
            with self.subTest(text=text[:40], code=code):
                result = self.run_cli(str(self.write_capture(text)))
                self.assertEqual(result.returncode, code, result.stderr)

    def test_text_output_names_verdict_totals_and_summaries(self):
        result = self.run_cli(str(self.write_capture(LIBTEST_GREEN)))
        out = result.stdout
        self.assertIn("GREEN", out)
        self.assertRegex(out, r"passed\s+4\b")
        self.assertRegex(out, r"binaries\s+3\b")
        self.assertEqual(sum("libtest" in row for row in out.splitlines()), 3)

    def test_json_output(self):
        result = self.run_cli("--json", str(self.write_capture(LIBTEST_FAILED)))
        self.assertEqual(result.returncode, 1)
        report = json.loads(result.stdout)
        self.assertEqual(report["verdict"], "FAILED")
        self.assertEqual(report["exit_code"], 1)
        for key in ("passed", "failed", "skipped", "filtered_out"):
            with self.subTest(key=key):
                self.assertIsInstance(report[key], int)
        self.assertEqual(report["binaries"], 2)
        self.assertEqual(len(report["summaries"]), 2)
        for summary in report["summaries"]:
            self.assertEqual(summary["dialect"], "libtest")
            self.assertIsInstance(summary["line"], int)

    def test_json_binaries_null_when_unknown(self):
        result = self.run_cli("--json", str(self.write_capture(SKIM_GREEN)))
        self.assertEqual(result.returncode, 0)
        self.assertIsNone(json.loads(result.stdout)["binaries"])

    def test_stdin_capture(self):
        from_file = json.loads(self.run_cli(
            "--json", str(self.write_capture(LIBTEST_GREEN))).stdout)
        for args in (("--json", "-"), ("--json",)):
            with self.subTest(args=args):
                result = self.run_cli(*args, stdin_text=LIBTEST_GREEN)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(result.stdout)["passed"],
                                 from_file["passed"])

    def test_empty_capture_is_reported_incomplete(self):
        result = self.run_cli(str(self.write_capture("")))
        self.assertEqual(result.returncode, 3)
        self.assertIn("INCOMPLETE", result.stdout + result.stderr)

    def test_missing_file_is_exit_2(self):
        missing = self.tmpdir / "no-such.log"
        result = self.run_cli(str(missing))
        self.assertEqual(result.returncode, 2)
        self.assertIn(str(missing), result.stderr)
        self.assertEqual(result.stdout, "")

    def test_help_exits_0(self):
        self.assertEqual(self.run_cli("--help").returncode, 0)


if __name__ == "__main__":
    unittest.main()
