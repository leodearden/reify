#!/usr/bin/env python3
"""
await-merge-landing.py — wait for a merge request to land, ending itself
before its host's kill deadline (task #7960).

Each poll round checks two things: whether --commit is an ancestor of --ref
(git, the authority), and the merge queue's own `merge_status` for the
request. The waiter stops at the first terminal answer, or when its host's
deadline is about to arrive, and prints exactly ONE JSON verdict line on
stdout. Diagnostics go to stderr only.

Two hosts, one command:

  Monitor     arm it with `timeout_ms: 600000`. That is the hard cap: there is
              no `persistent` parameter, and the default is 300000. The waiter
              ends itself first with a PENDING line whose `rearm_command` you
              re-run verbatim in the next Monitor call.

  Bash        `run_in_background` with `timeout` <= 7200000, passing the same
              number as --host-timeout-ms, for one call that waits the whole
              time and notifies once.

Never wait with a hand-rolled `sleep` loop.

The exit code is the verdict, tabled at the end of --help (generated from the
Verdict enum).
"""

from __future__ import annotations

import argparse
import sys
import time
from dataclasses import dataclass
from enum import IntEnum, StrEnum
from types import MappingProxyType
from typing import Any, Callable, Mapping


class Verdict(IntEnum):
    LANDED = 0
    FAILED = 1
    BLOCKED = 3
    PENDING = 75


_VERDICT_MEANINGS = {
    Verdict.LANDED: "commit is an ancestor of --ref, or merge_status reported done",
    Verdict.FAILED: "merge_status abandoned/superseded — the request will not land",
    Verdict.BLOCKED: "merge_status conflict/blocked — needs action before it can land",
    Verdict.PENDING: "host budget spent with no terminal state; re-run rearm_command",
}
_USAGE_EXIT = 2

# Vocabulary home: dark-factory shared/src/shared/merge_state.py::TERMINAL_STATES.
TERMINAL_MERGE_STATES: Mapping[str, Verdict] = MappingProxyType({
    "done": Verdict.LANDED,
    "conflict": Verdict.BLOCKED,
    "blocked": Verdict.BLOCKED,
    "abandoned": Verdict.FAILED,
    "superseded": Verdict.FAILED,
})


def classify_merge_status(merge_status: Mapping[str, Any]) -> Verdict | None:
    """The terminal verdict a merge_status reply carries, or None to keep
    waiting. Unrecognised states are non-terminal (fail-open)."""
    state = merge_status.get("state")
    if not isinstance(state, str):
        return None
    return TERMINAL_MERGE_STATES.get(state)


# ---------------------------------------------------------------------------
# The poll loop
# ---------------------------------------------------------------------------

class ProbeUnavailable(Exception):
    """A probe could not answer this round (a transport, protocol or git
    error). Never terminal: the next round asks again."""


class LandingSource(StrEnum):
    GIT_ANCESTRY = "git_ancestry"
    MERGE_STATUS = "merge_status"


@dataclass(frozen=True)
class Outcome:
    """`merge_status` and `probe_error` are the LAST raw reply and error seen."""
    verdict: Verdict
    source: LandingSource | None
    merge_status: Mapping[str, Any] | None
    probe_error: str | None
    polls: int
    elapsed_seconds: float


def _probe_round(
    is_landed: Callable[[], bool], merge_status: Callable[[], Mapping[str, Any]],
) -> tuple[Verdict | None, LandingSource | None, Mapping[str, Any] | None, str | None]:
    """One round: git ancestry first (the authority), then merge_status.
    Returns (verdict, source, merge_status reply, probe error)."""
    error = None
    try:
        if is_landed():
            return Verdict.LANDED, LandingSource.GIT_ANCESTRY, None, None
    except ProbeUnavailable as exc:
        error = str(exc)
    try:
        status = merge_status()
    except ProbeUnavailable as exc:
        return None, None, None, str(exc)
    verdict = classify_merge_status(status)
    return verdict, (LandingSource.MERGE_STATUS if verdict is not None else None), status, error


def await_landing(
    is_landed: Callable[[], bool],
    merge_status: Callable[[], Mapping[str, Any]],
    *,
    budget_seconds: float,
    interval_seconds: float,
    clock: Callable[[], float] = time.monotonic,
    sleep: Callable[[float], None] = time.sleep,
) -> Outcome:
    """Poll until a terminal verdict or the budget is spent. A round never
    starts after `budget_seconds`; the last sleep is truncated to land on it."""
    start = clock()
    deadline = start + budget_seconds
    polls, last_status, last_error = 0, None, None
    while True:
        polls += 1
        verdict, source, status, error = _probe_round(is_landed, merge_status)
        last_status = status if status is not None else last_status
        last_error = error if error is not None else last_error
        remaining = deadline - clock()
        if verdict is None and remaining <= 0:
            verdict = Verdict.PENDING
        if verdict is not None:
            return Outcome(verdict, source, last_status, last_error, polls,
                           clock() - start)
        sleep(min(interval_seconds, remaining))


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def _exit_code_table() -> str:
    entries = [(int(v), v.name, _VERDICT_MEANINGS[v]) for v in Verdict]
    entries.append((_USAGE_EXIT, "", "bad arguments, unresolvable --commit/--ref, "
                                     "or no escalation URL"))
    rows = [f"  {code:>2}  {name:<8}  {meaning}" for code, name, meaning in sorted(entries)]
    return "exit codes (the verdict):\n" + "\n".join(rows)


def build_parser() -> argparse.ArgumentParser:
    return argparse.ArgumentParser(
        description=__doc__, epilog=_exit_code_table(),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )


def main(argv: list[str] | None = None) -> int:
    build_parser().parse_args(argv)
    return _USAGE_EXIT


if __name__ == "__main__":
    sys.exit(main())
