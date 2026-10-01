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

Never wait with a hand-rolled `sleep` loop. The two hosts this runs under,
and the exit code that is its verdict, are tabled at the end of --help.
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
import threading
import time
import urllib.request
from dataclasses import dataclass, replace
from enum import IntEnum, StrEnum
from pathlib import Path
from types import MappingProxyType
from typing import Any, Callable, Mapping

# The host tool schemas, measured 2026-09-27 and 2026-10-01.
MONITOR_TIMEOUT_CAP_MS = 600_000
MONITOR_DEFAULT_TIMEOUT_MS = 300_000
BASH_BACKGROUND_TIMEOUT_CAP_MS = 7_200_000
GIT_TIMEOUT_SECONDS = 10
FETCH_TIMEOUT_SECONDS = 60
MERGE_STATUS_TIMEOUT_SECONDS = 10  # the whole call: a cold session's 3 POSTs too
_STARTUP_SLACK_SECONDS = 10
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


def _within(seconds: float, call: Callable[[], Any]) -> Any:
    """call() on a daemon thread, abandoned with TimeoutError after `seconds`.
    urlopen's timeout bounds each socket operation, not the exchange, so a
    reply that trickles in would outlive it. Unlike an executor's thread, a
    daemon thread cannot hold the process open at exit."""
    finished: dict[str, Any] = {}

    def run() -> None:
        try:
            finished["value"] = call()
        except BaseException as exc:
            finished["error"] = exc

    worker = threading.Thread(target=run, daemon=True)
    worker.start()
    worker.join(seconds)
    if worker.is_alive():
        raise TimeoutError(f"no complete reply within {seconds:.1f}s")
    if "error" in finished:
        raise finished["error"]
    return finished["value"]


def _read_reply(response: http.client.HTTPResponse) -> Any:
    """The JSON-RPC response in a JSON body, or in the first SSE `data:` line
    that carries one. The stream is not read past it: a server may hold it
    open."""
    if "text/event-stream" not in response.headers.get("Content-Type", ""):
        return json.loads(response.read())
    for line in response:
        if line.startswith(b"data:"):
            message = json.loads(line[len(b"data:"):])
            if isinstance(message, dict) and ("result" in message or "error" in message):
                return message
    raise ValueError("SSE reply ended with no JSON-RPC response")


def _exchange(request: urllib.request.Request, timeout: float, *,
              expects_reply: bool) -> tuple[http.client.HTTPMessage, Any]:
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return response.headers, (_read_reply(response) if expects_reply else None)


def _jsonrpc_result(message: Any) -> Mapping[str, Any]:
    result = message.get("result") if isinstance(message, dict) else None
    if not isinstance(result, dict):
        raise ValueError(f"no JSON-RPC result: {message!r:.200}")
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
    """Calls merge_status for one selector, each call finished or failed
    within `timeout_seconds`. The session id is the only state; any failure
    drops it, so the next poll re-initializes (a restarted server forgets its
    sessions)."""

    def __init__(self, url: str, selector: MergeSelector,
                 timeout_seconds: float = MERGE_STATUS_TIMEOUT_SECONDS):
        self._url = url
        self._selector = selector
        self._timeout_seconds = timeout_seconds
        self._session_id: str | None = None
        self._ids = itertools.count(1)

    def status(self) -> Mapping[str, Any]:
        deadline = time.monotonic() + self._timeout_seconds
        try:
            self._ensure_session(deadline)
            _, reply = self._post(self._request("tools/call", {
                "name": "merge_status", "arguments": self._selector.arguments(),
            }), deadline)
            return _tool_payload(_jsonrpc_result(reply))
        except _TRANSPORT_ERRORS as exc:
            self._session_id = None
            raise ProbeUnavailable(f"merge_status: {exc}") from exc

    def _ensure_session(self, deadline: float) -> None:
        if self._session_id is not None:
            return
        headers, reply = self._post(self._request("initialize", {
            "protocolVersion": "2025-03-26", "capabilities": {},
            "clientInfo": {"name": "await-merge-landing", "version": "1"},
        }), deadline)
        _jsonrpc_result(reply)
        session_id = headers.get("mcp-session-id")
        if not session_id:
            raise ValueError("initialize returned no mcp-session-id")
        self._session_id = session_id
        self._post({"jsonrpc": "2.0", "method": "notifications/initialized"}, deadline)

    def _request(self, method: str, params: Mapping[str, Any]) -> dict[str, Any]:
        return {"jsonrpc": "2.0", "id": next(self._ids), "method": method,
                "params": params}

    def _post(self, payload: Mapping[str, Any],
              deadline: float) -> tuple[http.client.HTTPMessage, Any]:
        """(headers, the JSON-RPC reply or None for a notification), all
        received before `deadline`."""
        headers = dict(_MCP_HEADERS)
        if self._session_id is not None:
            headers["Mcp-Session-Id"] = self._session_id
        request = urllib.request.Request(self._url, data=json.dumps(payload).encode(),
                                         headers=headers, method="POST")
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("merge_status call deadline passed")
        return _within(remaining, functools.partial(
            _exchange, request, remaining, expects_reply="id" in payload))


# ---------------------------------------------------------------------------
# git ancestry
# ---------------------------------------------------------------------------

def _log(message: str) -> None:
    print(f"await-merge-landing: {message}", file=sys.stderr, flush=True)


def _run_git(repo: Path, *args: str,
             timeout: float = GIT_TIMEOUT_SECONDS) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(
            ["git", "-C", str(repo), *args], capture_output=True, text=True,
            stdin=subprocess.DEVNULL, env={**os.environ, "GIT_TERMINAL_PROMPT": "0"},
            timeout=timeout,
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
        result = _run_git(repo, "fetch", "--quiet", remote, timeout=FETCH_TIMEOUT_SECONDS)
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

def _host_table() -> str:
    return (
        "hosts (each passes its kill deadline as --host-timeout-ms):\n"
        f"  Monitor  timeout_ms: {MONITOR_TIMEOUT_CAP_MS}, the hard cap. The default is\n"
        f"           {MONITOR_DEFAULT_TIMEOUT_MS} and there is no `persistent` parameter. "
        "The waiter\n"
        "           ends itself first with a PENDING line; re-run its\n"
        "           `rearm_command` verbatim in the next Monitor call.\n"
        f"  Bash     run_in_background, timeout <= {BASH_BACKGROUND_TIMEOUT_CAP_MS}, "
        "for one call that\n"
        "           waits the whole time and notifies once.")


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
        description=__doc__, epilog=f"{_host_table()}\n\n{_exit_code_table()}",
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


def probe_round_reserve_seconds(*, fetching: bool) -> float:
    """The longest one poll round can take: every probe call at its own
    timeout, plus slack to start up and print."""
    return (GIT_TIMEOUT_SECONDS + MERGE_STATUS_TIMEOUT_SECONDS + _STARTUP_SLACK_SECONDS
            + (FETCH_TIMEOUT_SECONDS if fetching else 0))


def invocation_budget_seconds(host_timeout_ms: int, *, fetching: bool) -> float:
    """Poll budget that lets the last round finish before the host kills us."""
    return max(0.0, host_timeout_ms / 1000 - probe_round_reserve_seconds(fetching=fetching))


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
        budget_seconds=max(0.0, invocation_budget_seconds(
            options.host_timeout_ms, fetching=options.fetch_remote is not None)
            - start_up_seconds),
        interval_seconds=options.interval_seconds,
    )
    print(format_event(outcome, options), flush=True)
    return int(outcome.verdict)


if __name__ == "__main__":
    sys.exit(main())
