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
import functools
import http.client
import itertools
import json
import shlex
import sys
import time
import urllib.request
from dataclasses import dataclass
from enum import IntEnum, StrEnum
from pathlib import Path
from types import MappingProxyType
from typing import Any, Callable, Mapping

# The Monitor tool schema: "Deadlines above 600000ms are capped" (measured
# 2026-09-27 and 2026-10-01).
MONITOR_TIMEOUT_CAP_MS = 600_000
PROBE_TIMEOUT_SECONDS = 10
_WORST_CASE_PROBE_CALLS = 5  # fetch, merge-base, then a cold MCP session's 3 POSTs
_STARTUP_SLACK_SECONDS = 10
PROBE_ROUND_RESERVE_SECONDS = (_WORST_CASE_PROBE_CALLS * PROBE_TIMEOUT_SECONDS
                               + _STARTUP_SLACK_SECONDS)
DEFAULT_INTERVAL_SECONDS = 30.0
SCRIPT_PATH = Path(__file__).resolve()
DEFAULT_REPO = SCRIPT_PATH.parents[1]


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
# merge_status over the escalation server's MCP streamable-HTTP endpoint
# ---------------------------------------------------------------------------

class SelectorKind(StrEnum):
    """Each value IS merge_status's argument name."""
    REQUEST_ID = "request_id"
    TASK_ID = "task_id"
    BRANCH = "branch"

    @property
    def flag(self) -> str:
        return "--" + self.value.replace("_", "-")


@dataclass(frozen=True)
class MergeSelector:
    kind: SelectorKind
    value: str

    def arguments(self) -> dict[str, str]:
        return {self.kind.value: self.value}


_MCP_HEADERS = MappingProxyType({
    "Content-Type": "application/json",
    "Accept": "application/json, text/event-stream",
})
_TRANSPORT_ERRORS = (OSError, ValueError, http.client.HTTPException)


def _jsonrpc_result(headers: http.client.HTTPMessage, body: bytes) -> dict[str, Any]:
    """The `result` object of a JSON or SSE (last `data:` line) reply."""
    text = body.decode("utf-8")
    if "text/event-stream" in headers.get("Content-Type", ""):
        data = [line[len("data:"):] for line in text.splitlines()
                if line.startswith("data:")]
        if not data:
            raise ValueError("SSE reply carried no data line")
        text = data[-1]
    reply = json.loads(text)
    result = reply.get("result") if isinstance(reply, dict) else None
    if not isinstance(result, dict):
        raise ValueError(f"no JSON-RPC result: {text[:200]}")
    return result


def _tool_payload(result: Mapping[str, Any]) -> Mapping[str, Any]:
    """structuredContent when present, else the JSON in content[0].text."""
    if result.get("isError"):
        raise ValueError(f"tool error: {result.get('content')}")
    structured = result.get("structuredContent")
    if isinstance(structured, dict):
        return structured
    content = result.get("content") or [{}]
    payload = json.loads(content[0].get("text", ""))
    if not isinstance(payload, dict):
        raise ValueError(f"tool result is not an object: {payload!r}")
    return payload


class MergeStatusClient:
    """Calls merge_status for one selector. The session id is the only state;
    any failure drops it, so the next poll re-initializes (a restarted server
    forgets its sessions)."""

    def __init__(self, url: str, selector: MergeSelector,
                 timeout_seconds: float = PROBE_TIMEOUT_SECONDS):
        self._url = url
        self._selector = selector
        self._timeout_seconds = timeout_seconds
        self._session_id: str | None = None
        self._ids = itertools.count(1)

    def status(self) -> Mapping[str, Any]:
        try:
            self._ensure_session()
            headers, body = self._post(self._request("tools/call", {
                "name": "merge_status", "arguments": self._selector.arguments(),
            }))
            return _tool_payload(_jsonrpc_result(headers, body))
        except _TRANSPORT_ERRORS as exc:
            self._session_id = None
            raise ProbeUnavailable(f"merge_status: {exc}") from exc

    def _ensure_session(self) -> None:
        if self._session_id is not None:
            return
        headers, body = self._post(self._request("initialize", {
            "protocolVersion": "2025-03-26", "capabilities": {},
            "clientInfo": {"name": "await-merge-landing", "version": "1"},
        }))
        _jsonrpc_result(headers, body)
        session_id = headers.get("mcp-session-id")
        if not session_id:
            raise ValueError("initialize returned no mcp-session-id")
        self._session_id = session_id
        self._post({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def _request(self, method: str, params: Mapping[str, Any]) -> dict[str, Any]:
        return {"jsonrpc": "2.0", "id": next(self._ids), "method": method,
                "params": params}

    def _post(self, payload: Mapping[str, Any]) -> tuple[http.client.HTTPMessage, bytes]:
        headers = dict(_MCP_HEADERS)
        if self._session_id is not None:
            headers["Mcp-Session-Id"] = self._session_id
        request = urllib.request.Request(self._url, data=json.dumps(payload).encode(),
                                         headers=headers, method="POST")
        with urllib.request.urlopen(request, timeout=self._timeout_seconds) as response:
            return response.headers, response.read()


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def _exit_code_table() -> str:
    entries = [(int(v), v.name, _VERDICT_MEANINGS[v]) for v in Verdict]
    entries.append((_USAGE_EXIT, "", "bad arguments, unresolvable --commit/--ref, "
                                     "or no escalation URL"))
    rows = [f"  {code:>2}  {name:<8}  {meaning}" for code, name, meaning in sorted(entries)]
    return "exit codes (the verdict):\n" + "\n".join(rows)


@dataclass(frozen=True)
class Options:
    commit: str
    selector: MergeSelector
    host_timeout_ms: int
    interval_seconds: float
    ref: str
    fetch_remote: str | None
    repo: Path
    escalation_url: str | None


def _positive_seconds(text: str) -> float:
    seconds = float(text)
    if not seconds > 0:
        raise argparse.ArgumentTypeError(f"must be > 0 seconds: {text}")
    return seconds


def _non_negative_ms(text: str) -> int:
    ms = int(text)
    if ms < 0:
        raise argparse.ArgumentTypeError(f"must be >= 0 ms: {text}")
    return ms


def _resolved_path(text: str) -> Path:
    return Path(text).resolve()


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=__doc__, epilog=_exit_code_table(),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--commit", required=True, metavar="REV",
                        help="Commit to wait for; resolved to a SHA once at start.")
    selectors = parser.add_mutually_exclusive_group(required=True)
    for kind in SelectorKind:
        selectors.add_argument(kind.flag, dest="selector", metavar=kind.name,
                               type=functools.partial(MergeSelector, kind),
                               help=f"merge_status selector ({kind.value}).")
    parser.add_argument("--host-timeout-ms", type=_non_negative_ms,
                        default=MONITOR_TIMEOUT_CAP_MS, metavar="MS",
                        help="The host's kill deadline: Monitor timeout_ms or Bash "
                             "timeout, the same number (default: %(default)s, the "
                             "Monitor cap).")
    parser.add_argument("--interval", dest="interval_seconds", type=_positive_seconds,
                        default=DEFAULT_INTERVAL_SECONDS, metavar="SECONDS",
                        help="Seconds between poll rounds (default: %(default)s).")
    parser.add_argument("--ref", default="main",
                        help="Ref the commit must reach (default: %(default)s).")
    parser.add_argument("--fetch", dest="fetch_remote", metavar="REMOTE",
                        help="git fetch REMOTE before each ancestry check "
                             "(use with --ref REMOTE/main).")
    parser.add_argument("--repo", type=_resolved_path, default=DEFAULT_REPO,
                        help="Repository to check (default: this script's checkout).")
    parser.add_argument("--escalation-url", metavar="URL",
                        help="Escalation MCP URL (default: <repo>/.mcp.json "
                             "mcpServers.escalation.url).")
    return parser


def parse_options(argv: list[str] | None = None) -> Options:
    return Options(**vars(build_parser().parse_args(argv)))


def invocation_budget_seconds(host_timeout_ms: int) -> float:
    """Poll budget that lets the last round finish before the host kills us."""
    return max(0.0, host_timeout_ms / 1000 - PROBE_ROUND_RESERVE_SECONDS)


def rearm_argv(options: Options) -> list[str]:
    """Every option spelled out, so a re-arm is the identical command."""
    argv = [
        "--commit", options.commit,
        options.selector.kind.flag, options.selector.value,
        "--host-timeout-ms", str(options.host_timeout_ms),
        "--interval", str(options.interval_seconds),
        "--ref", options.ref,
        "--repo", str(options.repo),
    ]
    if options.fetch_remote is not None:
        argv += ["--fetch", options.fetch_remote]
    if options.escalation_url is not None:
        argv += ["--escalation-url", options.escalation_url]
    return argv


def rearm_command(options: Options) -> str:
    return shlex.join(["python3", str(SCRIPT_PATH), *rearm_argv(options)])


def main(argv: list[str] | None = None) -> int:
    parse_options(argv)
    return _USAGE_EXIT


if __name__ == "__main__":
    sys.exit(main())
