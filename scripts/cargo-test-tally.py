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

The verdict is the exit code: GREEN 0, FAILED 1, INCOMPLETE 3 (no summary, an
unfinished binary, or a partial nextest run), NOTHING_RAN 4 (summaries, but no
test passed or failed). Exit 2 is an unreadable capture or bad arguments.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from enum import Enum, IntEnum
from typing import Iterable


class Dialect(str, Enum):
    LIBTEST = "libtest"
    NEXTEST = "nextest"
    SKIM = "skim"


class Verdict(IntEnum):
    GREEN = 0
    FAILED = 1
    INCOMPLETE = 3
    NOTHING_RAN = 4


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


_ANSI_CSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")
_LIBTEST_RESULT = re.compile(
    r"^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; "
    r"(\d+) measured; (\d+) filtered out"
)
_LIBTEST_BINARY_HEADER = re.compile(r"^\s+(?:Running \S.* \(.+\)|Doc-tests \S+)$")
_CARGO_TARGET_FAILED = re.compile(r"^error: test failed, to rerun pass ")


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


def tally_lines(lines: Iterable[str]) -> Tally:
    """Tally a capture in one pass; line N of `lines` is reported as line N."""
    summaries: list[Summary] = []
    cargo_target_failures = 0
    unfinished_binaries = 0
    awaiting_result = False
    for line_no, raw in enumerate(lines, start=1):
        line = _ANSI_CSI.sub("", raw).rstrip()
        summary = _match_libtest(line_no, line)
        if summary is not None:
            summaries.append(summary)
            awaiting_result = False
        elif _LIBTEST_BINARY_HEADER.match(line):
            if awaiting_result:
                unfinished_binaries += 1
            awaiting_result = True
        elif _CARGO_TARGET_FAILED.match(line):
            cargo_target_failures += 1
            awaiting_result = False
    if awaiting_result:
        unfinished_binaries += 1
    return Tally(tuple(summaries), cargo_target_failures, unfinished_binaries)


def tally_text(text: str) -> Tally:
    """Tally a whole capture. Lines split on '\\n' only, so `line` agrees
    with `grep -n` and `sed -n` even when a test printed a bare '\\r'."""
    return tally_lines(text.split("\n"))
