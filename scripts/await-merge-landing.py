#!/usr/bin/env python3
"""
await-merge-landing.py — wait for a merge request to land, ending itself
before its host's kill deadline (task #7960).

Each poll round checks two things: whether --commit is an ancestor of --ref
(git, the authority), and the merge queue's own `merge_status`. The waiter
stops at the first terminal answer, or when its host's deadline is about to
arrive, and prints exactly ONE JSON verdict line on stdout. Diagnostics go to
stderr only.

Only --request-id names one request. --task-id and --branch name the most
recent request for that key, which may not carry --commit, so their terminal
merge_status is reported as UNATTRIBUTED unless git confirms the landing.

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
import os
import shlex
import subprocess
import sys
import time
import urllib.request
from dataclasses import dataclass, replace
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
    """The exit codes. No verdict uses 1 (an uncaught exception's exit) or 2
    (argparse's), so a crash or a usage error never reads as a verdict."""
    LANDED = 0
    BLOCKED = 3
    FAILED = 4
    UNATTRIBUTED = 5
    PENDING = 75


_VERDICT_MEANINGS = {
    Verdict.LANDED: "commit is an ancestor of --ref, or the --request-id "
                    "merge_status is done",
    Verdict.BLOCKED: "the --request-id merge_status is conflict/blocked — needs "
                     "action before it can land",
    Verdict.FAILED: "the --request-id merge_status is abandoned/superseded — it "
                    "will not land",
    Verdict.UNATTRIBUTED: "a --task-id/--branch merge_status is terminal but git "
                          "does not confirm: it is the most recent request's, "
                          "maybe not this commit's",
    Verdict.PENDING: "host budget spent with no terminal state; re-run rearm_command",
}
_INTERNAL_ERROR_EXIT = 1
_USAGE_EXIT = 2

# Vocabulary home: dark-factory shared/src/shared/merge_state.py::TERMINAL_STATES.
TERMINAL_MERGE_STATES: Mapping[str, Verdict] = MappingProxyType({
    "done": Verdict.LANDED,
    "conflict": Verdict.BLOCKED,
    "blocked": Verdict.BLOCKED,
    "abandoned": Verdict.FAILED,
    "superseded": Verdict.FAILED,
})


def classify_merge_status(merge_status: Mapping[str, Any], *,
                          attributed: bool) -> Verdict | None:
    """The terminal verdict a merge_status reply carries, or None to keep
    waiting. Unrecognised states are non-terminal (fail-open). A terminal
    reply not `attributed` to --commit's own request is UNATTRIBUTED."""
    state = merge_status.get("state")
    if not isinstance(state, str) or state not in TERMINAL_MERGE_STATES:
        return None
    return TERMINAL_MERGE_STATES[state] if attributed else Verdict.UNATTRIBUTED


# ---------------------------------------------------------------------------
# The poll loop
# ---------------------------------------------------------------------------

class ProbeUnavailable(Exception):
    """A probe could not answer this round (a transport, protocol or git
    error). Never terminal: the next round asks again."""


class Probe(StrEnum):
    GIT_ANCESTRY = "git_ancestry"
    MERGE_STATUS = "merge_status"


@dataclass(frozen=True)
class Outcome:
    """`source` is the probe whose answer ended the wait. `merge_status` is
    the LAST raw reply seen; `probe_errors` holds each probe's LAST error."""
    verdict: Verdict
    source: Probe | None
    merge_status: Mapping[str, Any] | None
    probe_errors: Mapping[Probe, str]
    polls: int
    elapsed_seconds: float


@dataclass(frozen=True)
class _Round:
    verdict: Verdict | None
    source: Probe | None
    merge_status: Mapping[str, Any] | None
    errors: Mapping[Probe, str]


def _probe_round(
    is_landed: Callable[[], bool], merge_status: Callable[[], Mapping[str, Any]],
    merge_status_attributed: bool,
) -> _Round:
    """One round: git ancestry first (the authority), then merge_status."""
    errors: dict[Probe, str] = {}
    try:
        if is_landed():
            return _Round(Verdict.LANDED, Probe.GIT_ANCESTRY, None, errors)
    except ProbeUnavailable as exc:
        errors[Probe.GIT_ANCESTRY] = str(exc)
    try:
        status = merge_status()
    except ProbeUnavailable as exc:
        errors[Probe.MERGE_STATUS] = str(exc)
        return _Round(None, None, None, errors)
    verdict = classify_merge_status(status, attributed=merge_status_attributed)
    return _Round(verdict, Probe.MERGE_STATUS if verdict is not None else None,
                  status, errors)


def await_landing(
    is_landed: Callable[[], bool],
    merge_status: Callable[[], Mapping[str, Any]],
    *,
    merge_status_attributed: bool,
    budget_seconds: float,
    interval_seconds: float,
    clock: Callable[[], float] = time.monotonic,
    sleep: Callable[[float], None] = time.sleep,
) -> Outcome:
    """Poll until a terminal verdict or the budget is spent. A round never
    starts after `budget_seconds`; the last sleep is truncated to land on it."""
    start = clock()
    deadline = start + budget_seconds
    polls, last_status, last_errors = 0, None, {}
    while True:
        polls += 1
        probed = _probe_round(is_landed, merge_status, merge_status_attributed)
        last_status = probed.merge_status if probed.merge_status is not None \
            else last_status
        last_errors.update(probed.errors)
        remaining = deadline - clock()
        verdict = probed.verdict
        if verdict is None and remaining <= 0:
            verdict = Verdict.PENDING
        if verdict is not None:
            return Outcome(verdict, probed.source, last_status,
                           MappingProxyType(last_errors), polls, clock() - start)
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

    @property
    def names_one_request(self) -> bool:
        """task_id and branch name the most recent request, which may
        predate --commit."""
        return self is SelectorKind.REQUEST_ID


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
    content = result.get("content")
    first = content[0] if isinstance(content, list) and content else None
    text = first.get("text") if isinstance(first, dict) else None
    if not isinstance(text, str):
        raise ValueError(f"tool result has neither structuredContent nor "
                         f"content[0].text: {result!r:.200}")
    payload = json.loads(text)
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
# git ancestry
# ---------------------------------------------------------------------------

def _log(message: str) -> None:
    print(f"await-merge-landing: {message}", file=sys.stderr, flush=True)


def _run_git(repo: Path, *args: str) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(
            ["git", "-C", str(repo), *args], capture_output=True, text=True,
            stdin=subprocess.DEVNULL, env={**os.environ, "GIT_TERMINAL_PROMPT": "0"},
            timeout=PROBE_TIMEOUT_SECONDS,
        )
    except (subprocess.TimeoutExpired, OSError) as exc:
        raise ProbeUnavailable(f"git {args[0]}: {exc}") from exc


def _resolve_commit(repo: Path, rev: str) -> str | None:
    try:
        result = _run_git(repo, "rev-parse", "--verify", "--quiet", f"{rev}^{{commit}}")
    except ProbeUnavailable:
        return None
    return result.stdout.strip() if result.returncode == 0 else None


def _fetch(repo: Path, remote: str) -> None:
    """Best effort: a failed fetch is reported and the check uses the refs
    already fetched."""
    try:
        result = _run_git(repo, "fetch", "--quiet", remote)
    except ProbeUnavailable as exc:
        _log(str(exc))
        return
    if result.returncode != 0:
        _log(f"git fetch {remote}: exit {result.returncode}: {result.stderr.strip()}")


@dataclass(frozen=True)
class GitAncestryProbe:
    repo: Path
    commit_sha: str
    ref: str
    fetch_remote: str | None

    def __call__(self) -> bool:
        if self.fetch_remote is not None:
            _fetch(self.repo, self.fetch_remote)
        result = _run_git(self.repo, "merge-base", "--is-ancestor", self.commit_sha,
                          self.ref)
        if result.returncode not in (0, 1):
            raise ProbeUnavailable(f"git merge-base: exit {result.returncode}: "
                                   f"{result.stderr.strip()}")
        return result.returncode == 0


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def _exit_code_table() -> str:
    entries = [(int(v), v.name, _VERDICT_MEANINGS[v]) for v in Verdict]
    entries.append((_INTERNAL_ERROR_EXIT, "", "internal error: an uncaught exception, "
                                              "no verdict line"))
    entries.append((_USAGE_EXIT, "", "bad arguments, unresolvable --commit/--ref, "
                                     "or no escalation URL"))
    width = max(len(v.name) for v in Verdict)
    rows = [f"  {code:>2}  {name:<{width}}  {meaning}"
            for code, name, meaning in sorted(entries)]
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
                               help=f"merge_status selector ({kind.value})"
                                    + ("." if kind.names_one_request else
                                       ": the most recent request (see "
                                       "UNATTRIBUTED)."))
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


def resolve_escalation_url(options: Options) -> str | None:
    if options.escalation_url is not None:
        return options.escalation_url
    try:
        config = json.loads((options.repo / ".mcp.json").read_text())
        url = config["mcpServers"]["escalation"]["url"]
    except (OSError, ValueError, KeyError, TypeError):
        return None
    return url if isinstance(url, str) and url else None


def format_event(outcome: Outcome, options: Options) -> str:
    """The one stdout line: the verdict as JSON."""
    event = {
        "verdict": outcome.verdict.name,
        "commit": options.commit,
        "ref": options.ref,
        "source": outcome.source,
        "merge_status": None if outcome.merge_status is None else dict(outcome.merge_status),
        "probe_errors": dict(outcome.probe_errors),
        "polls": outcome.polls,
        "elapsed_seconds": round(outcome.elapsed_seconds, 1),
    }
    if outcome.verdict is Verdict.PENDING:
        event["rearm_command"] = rearm_command(options)
    return json.dumps(event)


class _UsageError(Exception):
    pass


def _prepare(options: Options) -> tuple[Options, str]:
    """Options with --commit resolved to a SHA, plus the escalation URL."""
    sha = _resolve_commit(options.repo, options.commit)
    if sha is None:
        raise _UsageError(f"cannot resolve --commit {options.commit!r} to a commit "
                          f"in {options.repo}")
    url = resolve_escalation_url(options)
    if url is None:
        raise _UsageError("no escalation URL: pass --escalation-url or provide "
                          "<repo>/.mcp.json mcpServers.escalation.url")
    if options.fetch_remote is not None:
        _fetch(options.repo, options.fetch_remote)
    if _resolve_commit(options.repo, options.ref) is None:
        raise _UsageError(f"cannot resolve --ref {options.ref!r} in {options.repo}")
    return replace(options, commit=sha), url


def _logged(probe: Callable[[], Any]) -> Callable[[], Any]:
    def call() -> Any:
        try:
            return probe()
        except ProbeUnavailable as exc:
            _log(str(exc))
            raise
    return call


def main(argv: list[str] | None = None) -> int:
    started = time.monotonic()
    try:
        options, url = _prepare(parse_options(argv))
    except _UsageError as exc:
        _log(str(exc))
        return _USAGE_EXIT
    start_up_seconds = time.monotonic() - started
    outcome = await_landing(
        _logged(GitAncestryProbe(options.repo, options.commit, options.ref,
                                 options.fetch_remote)),
        _logged(MergeStatusClient(url, options.selector).status),
        merge_status_attributed=options.selector.kind.names_one_request,
        budget_seconds=max(0.0, invocation_budget_seconds(options.host_timeout_ms)
                           - start_up_seconds),
        interval_seconds=options.interval_seconds,
    )
    print(format_event(outcome, options), flush=True)
    return int(outcome.verdict)


if __name__ == "__main__":
    sys.exit(main())
