#!/usr/bin/env python3
"""
cargo-test-tally.py — one captured test log in, every count out (task #7896).

Capture a test run ONCE, then derive every number from that file. Never re-run
a suite to get a second metric: that doubles the wall clock and the exposure to
an exit-137 kill.

    env cargo test -p <crate> > /tmp/<task>-test.log 2>&1; echo "rc=$?"
    python3 scripts/cargo-test-tally.py /tmp/<task>-test.log

`env` bypasses the host's skim cargo wrapper, which condenses the output to one
line; a skim capture is still understood, but it carries no binary count.

Dialects recognised: libtest (`cargo test`), nextest (`cargo nextest run`) and
skim. Libtest lines inside a nextest run are replayed or per-test noise and are
not counted.

The exit code is the verdict, tabled at the end of --help (generated from the
Verdict enum).
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import asdict, dataclass
from enum import Enum, IntEnum
from pathlib import Path
from typing import Any, Iterable


class Dialect(str, Enum):
    LIBTEST = "libtest"
    NEXTEST = "nextest"
    SKIM = "skim"


class Verdict(IntEnum):
    GREEN = 0
    FAILED = 1
    INCOMPLETE = 3
    NOTHING_RAN = 4


_VERDICT_MEANINGS = {
    Verdict.GREEN: "every recognised run finished, tests ran, none failed",
    Verdict.FAILED: "a test failed, timed out or crashed",
    Verdict.INCOMPLETE: ("no summary line, a binary that never reported, or a "
                         "partial nextest run"),
    Verdict.NOTHING_RAN: ("summaries, but no test passed or failed (a filter "
                          "matched nothing)"),
}
_UNREADABLE_EXIT = 2


@dataclass(frozen=True)
class Summary:
    """One recognised summary line. `line` is 1-based in the capture."""
    dialect: Dialect
    line: int
    passed: int
    failed: int
    skipped: int
    filtered_out: int
    binaries: int | None
    reported_failure: bool
    complete: bool = True


@dataclass(frozen=True)
class Tally:
    summaries: tuple[Summary, ...]
    cargo_target_failures: int
    unfinished_binaries: int

    @property
    def passed(self) -> int:
        return sum(s.passed for s in self.summaries)

    @property
    def failed(self) -> int:
        return sum(s.failed for s in self.summaries)

    @property
    def skipped(self) -> int:
        return sum(s.skipped for s in self.summaries)

    @property
    def filtered_out(self) -> int:
        return sum(s.filtered_out for s in self.summaries)

    @property
    def binaries(self) -> int | None:
        counts = [s.binaries for s in self.summaries]
        if None in counts:
            return None
        return sum(counts)

    @property
    def verdict(self) -> Verdict:
        if self.cargo_target_failures or any(
                s.reported_failure or s.failed > 0 for s in self.summaries):
            return Verdict.FAILED
        if (not self.summaries or self.unfinished_binaries
                or not all(s.complete for s in self.summaries)):
            return Verdict.INCOMPLETE
        if self.passed + self.failed == 0:
            return Verdict.NOTHING_RAN
        return Verdict.GREEN

    def to_dict(self) -> dict[str, Any]:
        return {
            "verdict": self.verdict.name,
            "exit_code": int(self.verdict),
            "passed": self.passed,
            "failed": self.failed,
            "skipped": self.skipped,
            "filtered_out": self.filtered_out,
            "binaries": self.binaries,
            "cargo_target_failures": self.cargo_target_failures,
            "unfinished_binaries": self.unfinished_binaries,
            "summaries": [{**asdict(s), "dialect": s.dialect.value}
                          for s in self.summaries],
        }


_ANSI_CSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")
_LIBTEST_RESULT = re.compile(
    r"^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; "
    r"(\d+) measured; (\d+) filtered out"
)
_LIBTEST_BINARY_HEADER = re.compile(r"^\s+(?:Running \S.* \(.+\)|Doc-tests \S+)$")
_CARGO_TARGET_FAILED = re.compile(r"^error: test failed, to rerun pass ")
_NEXTEST_START = re.compile(r"^\s*Starting (\d+) tests? across (\d+) binar(?:y|ies)\b")
_NEXTEST_SUMMARY = re.compile(
    r"^\s*Summary \[\s*[\d.]+s\]\s+(?:(\d+)/)?(\d+) tests? run: (.*)$"
)
_NEXTEST_PARENTHETICAL = re.compile(r"\s*\([^)]*\)")
_NEXTEST_OUTCOME = re.compile(r"^(\d+) (.+)$")
_SKIM_SUMMARY = re.compile(r"^PASS: (\d+) \| FAIL: (\d+) \| SKIP: (\d+)$")


def _match_libtest(line_no: int, line: str) -> Summary | None:
    match = _LIBTEST_RESULT.match(line)
    if match is None:
        return None
    status = match.group(1)
    passed, failed, ignored, _measured, filtered_out = map(int, match.groups()[1:])
    return Summary(
        dialect=Dialect.LIBTEST, line=line_no, passed=passed, failed=failed,
        skipped=ignored, filtered_out=filtered_out, binaries=1,
        reported_failure=(status == "FAILED" or failed > 0),
    )


def _nextest_outcomes(tail: str) -> dict[str, int] | None:
    """`2 passed (1 slow), 1 timed out` -> {'passed': 2, 'timed out': 1}.

    Parentheticals annotate passed tests and are not outcomes. A part that
    does not parse rejects the whole line, so the run reads as unfinished.
    """
    outcomes: dict[str, int] = {}
    for part in _NEXTEST_PARENTHETICAL.sub("", tail).split(", "):
        match = _NEXTEST_OUTCOME.match(part)
        if match is None:
            return None
        label = match.group(2)
        outcomes[label] = outcomes.get(label, 0) + int(match.group(1))
    return outcomes


def _match_nextest_summary(line_no: int, line: str,
                           binaries: int | None) -> Summary | None:
    match = _NEXTEST_SUMMARY.match(line)
    if match is None:
        return None
    outcomes = _nextest_outcomes(match.group(3))
    if outcomes is None:
        return None
    run_count, total = match.group(1), int(match.group(2))
    passed = outcomes.pop("passed", 0)
    skipped = outcomes.pop("skipped", 0)
    failed = sum(outcomes.values())
    return Summary(
        dialect=Dialect.NEXTEST, line=line_no, passed=passed, failed=failed,
        skipped=skipped, filtered_out=0, binaries=binaries,
        reported_failure=failed > 0,
        complete=(run_count is None or int(run_count) == total),
    )


def _match_skim(line_no: int, line: str) -> Summary | None:
    match = _SKIM_SUMMARY.match(line)
    if match is None:
        return None
    passed, failed, skipped = map(int, match.groups())
    return Summary(
        dialect=Dialect.SKIM, line=line_no, passed=passed, failed=failed,
        skipped=skipped, filtered_out=0, binaries=None,
        reported_failure=failed > 0,
    )


class _TallyPass:
    """Mutable state of one pass over a capture; its product is a frozen Tally.

    A libtest binary header awaits its result line. A nextest `Starting`
    header opens a region that its `Summary` line closes; libtest lines inside
    the region are replayed output or per-test noise, so they are ignored.
    """

    def __init__(self) -> None:
        self.summaries: list[Summary] = []
        self.cargo_target_failures = 0
        self.unfinished_binaries = 0
        self.awaiting_libtest_result = False
        self.in_nextest_region = False
        self.nextest_binaries: int | None = None

    def feed(self, line_no: int, line: str) -> None:
        if start := _NEXTEST_START.match(line):
            self._settle_unfinished()
            self.in_nextest_region = True
            self.nextest_binaries = int(start.group(2))
        elif summary := _match_nextest_summary(line_no, line, self.nextest_binaries):
            self.summaries.append(summary)
            self.in_nextest_region = False
            self.nextest_binaries = None
        elif summary := _match_skim(line_no, line):
            self.summaries.append(summary)
        elif self.in_nextest_region:
            return
        elif summary := _match_libtest(line_no, line):
            self.summaries.append(summary)
            self.awaiting_libtest_result = False
        elif _LIBTEST_BINARY_HEADER.match(line):
            self._settle_unfinished()
            self.awaiting_libtest_result = True
        elif _CARGO_TARGET_FAILED.match(line):
            self.cargo_target_failures += 1
            self.awaiting_libtest_result = False

    def finish(self) -> Tally:
        self._settle_unfinished()
        return Tally(tuple(self.summaries), self.cargo_target_failures,
                     self.unfinished_binaries)

    def _settle_unfinished(self) -> None:
        """Count a binary or nextest run that a new one (or EOF) cut short."""
        if self.awaiting_libtest_result or self.in_nextest_region:
            self.unfinished_binaries += 1
        self.awaiting_libtest_result = False
        self.in_nextest_region = False


def tally_lines(lines: Iterable[str]) -> Tally:
    """Tally a capture in one pass; line N of `lines` is reported as line N."""
    tally_pass = _TallyPass()
    for line_no, raw in enumerate(lines, start=1):
        tally_pass.feed(line_no, _ANSI_CSI.sub("", raw).rstrip())
    return tally_pass.finish()


def tally_text(text: str) -> Tally:
    """Tally a whole capture. Lines split on '\\n' only, so `line` agrees
    with `grep -n` and `sed -n` even when a test printed a bare '\\r'."""
    return tally_lines(text.split("\n"))


# ---------------------------------------------------------------------------
# Presentation
# ---------------------------------------------------------------------------

def _format_summary(index: int, summary: Summary) -> str:
    binaries = "?" if summary.binaries is None else summary.binaries
    suffix = "" if summary.complete else "  (incomplete)"
    return (f"  {index:>2}  {summary.dialect.value:<7}  line {summary.line:<7}"
            f"  passed {summary.passed}  failed {summary.failed}"
            f"  skipped {summary.skipped}  filtered {summary.filtered_out}"
            f"  binaries {binaries}{suffix}")


def _incomplete_reason(tally: Tally) -> str:
    if not tally.summaries:
        return ("no summary line recognised: the run was killed, timed out, "
                "failed to compile, or hit the skim 300s cap")
    if tally.unfinished_binaries:
        return (f"{tally.unfinished_binaries} binary or nextest run(s) never "
                "reported a result")
    return "a partial nextest run (N/M tests run)"


def _verdict_reason(tally: Tally) -> str:
    verdict = tally.verdict
    if verdict is Verdict.FAILED:
        return (f"{tally.failed} failed test(s), "
                f"{tally.cargo_target_failures} cargo target failure(s)")
    if verdict is Verdict.INCOMPLETE:
        return _incomplete_reason(tally)
    if verdict is Verdict.NOTHING_RAN:
        return "the filter matched zero tests"
    return f"{tally.passed} passed, none failed"


def format_text(tally: Tally) -> str:
    binaries = "unknown" if tally.binaries is None else tally.binaries
    rows = [_format_summary(i, s) for i, s in enumerate(tally.summaries, start=1)]
    rows.append(f"total  summaries {len(tally.summaries)}  binaries {binaries}"
                f"  passed {tally.passed}  failed {tally.failed}"
                f"  skipped {tally.skipped}  filtered out {tally.filtered_out}")
    rows.append(f"verdict  {tally.verdict.name} (exit {int(tally.verdict)}): "
                f"{_verdict_reason(tally)}")
    return "\n".join(rows) + "\n"


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def _exit_code_table() -> str:
    rows = [f"  {int(v)}  {v.name:<11}  {_VERDICT_MEANINGS[v]}" for v in Verdict]
    rows.append(f"  {_UNREADABLE_EXIT}  {'':<11}  unreadable capture or bad arguments")
    return "exit codes (the verdict):\n" + "\n".join(rows)


def _read_capture(source: str) -> str:
    """Bytes decoded leniently: a panic message can carry non-UTF-8 bytes, and
    no newline translation, so a bare '\r' never shifts reported lines."""
    raw = sys.stdin.buffer.read() if source == "-" else Path(source).read_bytes()
    return raw.decode("utf-8", errors="replace")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, epilog=_exit_code_table(),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "log", nargs="?", default="-",
        help="Captured test log. '-' or omitted reads stdin.",
    )
    parser.add_argument(
        "--json", action="store_true", dest="emit_json",
        help="Emit the tally as JSON instead of human-readable text.",
    )
    args = parser.parse_args(argv)

    try:
        text = _read_capture(args.log)
    except OSError as exc:
        print(f"ERROR: cannot read capture: {args.log}: {exc.strerror or exc}",
              file=sys.stderr)
        return _UNREADABLE_EXIT

    tally = tally_text(text)
    if args.emit_json:
        print(json.dumps(tally.to_dict(), indent=2))
    else:
        print(format_text(tally), end="")
    return int(tally.verdict)


if __name__ == "__main__":
    sys.exit(main())
