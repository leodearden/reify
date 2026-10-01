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
from enum import IntEnum
from types import MappingProxyType
from typing import Any, Mapping


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
