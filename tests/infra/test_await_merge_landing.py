#!/usr/bin/env python3
"""
test_await_merge_landing.py — stdlib unittest for scripts/await-merge-landing.py.

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only; the member is the thin
wrapper tests/infra/test_await_merge_landing.sh.
"""

import contextlib
import functools
import importlib.util
import io
import json
import os
import shlex
import subprocess
import sys
import tempfile
import threading
import unittest
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
TOOL_PATH = REPO_ROOT / "scripts" / "await-merge-landing.py"
GIT_ENV_SCRUB_LIB = REPO_ROOT / "scripts" / "lib_git_env_scrub.sh"


def _load_tool():
    spec = importlib.util.spec_from_file_location("await_merge_landing", TOOL_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {TOOL_PATH}")
    module = importlib.util.module_from_spec(spec)
    # Register before exec: @dataclass resolves a field's type through
    # sys.modules[cls.__module__], which is None for an unregistered module.
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


aml = _load_tool()
Verdict = aml.Verdict

LIVE_STATES = ("queued", "verifying", "gate", "finalizing")
EPISTEMIC_STATES = ("unknown", "no_record", "stale_record", "journaled")


class ClassifyMergeStatusTests(unittest.TestCase):
    def test_done_is_landed(self):
        self.assertIs(aml.classify_merge_status({"state": "done"}), Verdict.LANDED)

    def test_conflict_and_blocked_are_blocked(self):
        for state in ("conflict", "blocked"):
            with self.subTest(state=state):
                self.assertIs(aml.classify_merge_status({"state": state}),
                              Verdict.BLOCKED)

    def test_abandoned_and_superseded_are_failed(self):
        for state in ("abandoned", "superseded"):
            with self.subTest(state=state):
                self.assertIs(aml.classify_merge_status({"state": state}),
                              Verdict.FAILED)

    def test_live_states_keep_waiting(self):
        for state in LIVE_STATES:
            with self.subTest(state=state):
                self.assertIsNone(aml.classify_merge_status({"state": state}))

    def test_epistemic_states_keep_waiting(self):
        for state in EPISTEMIC_STATES:
            with self.subTest(state=state):
                self.assertIsNone(aml.classify_merge_status({"state": state}))

    def test_unrecognised_state_fails_open(self):
        self.assertIsNone(aml.classify_merge_status({"state": "brand_new_state"}))

    def test_missing_state_keeps_waiting(self):
        self.assertIsNone(aml.classify_merge_status({"request_id": "mr-1"}))

    def test_verdict_values_are_the_exit_code_contract(self):
        self.assertEqual(Verdict.LANDED, 0)
        self.assertEqual(Verdict.BLOCKED, 3)
        self.assertEqual(Verdict.FAILED, 4)
        self.assertEqual(Verdict.PENDING, 75)

    def test_no_verdict_shares_a_crash_or_usage_exit_code(self):
        codes = [int(v) for v in Verdict]
        self.assertNotIn(1, codes, "1 is Python's uncaught-exception exit")
        self.assertNotIn(2, codes, "2 is argparse's usage exit")


class FakeClock:
    """`now` reads the fake time; `sleep` records the request and advances it.
    No real time passes, so nothing here bounds wall-clock."""

    def __init__(self):
        self.t = 0.0
        self.sleeps = []

    def now(self):
        return self.t

    def sleep(self, seconds):
        self.sleeps.append(seconds)
        self.t += seconds


class ScriptedProbe:
    """Returns (or raises, for an exception instance) the scripted items in
    order, then repeats the last one forever."""

    def __init__(self, *items):
        self.items = list(items)
        self.calls = 0

    def __call__(self):
        item = self.items[min(self.calls, len(self.items) - 1)]
        self.calls += 1
        if isinstance(item, BaseException):
            raise item
        return item


class AwaitLandingLoopTests(unittest.TestCase):
    def _await(self, is_landed, merge_status, *, budget=540, interval=30):
        clock = FakeClock()
        outcome = aml.await_landing(
            is_landed, merge_status, budget_seconds=budget,
            interval_seconds=interval, clock=clock.now, sleep=clock.sleep,
        )
        return outcome, clock

    def test_landed_on_first_round_short_circuits_merge_status(self):
        merge_status = ScriptedProbe({"state": "queued"})
        outcome, clock = self._await(ScriptedProbe(True), merge_status)
        self.assertIs(outcome.verdict, Verdict.LANDED)
        self.assertEqual(outcome.source, "git_ancestry")
        self.assertEqual(outcome.polls, 1)
        self.assertEqual(clock.sleeps, [])
        self.assertEqual(merge_status.calls, 0)

    def test_lands_after_two_waiting_rounds(self):
        outcome, clock = self._await(ScriptedProbe(False, False, True),
                                     ScriptedProbe({"state": "verifying"}))
        self.assertIs(outcome.verdict, Verdict.LANDED)
        self.assertEqual(outcome.source, "git_ancestry")
        self.assertEqual(outcome.polls, 3)
        self.assertEqual(clock.sleeps, [30, 30])

    def test_terminal_merge_status_ends_the_wait(self):
        for state, verdict in (("conflict", Verdict.BLOCKED),
                               ("abandoned", Verdict.FAILED)):
            with self.subTest(state=state):
                reply = {"state": state, "request_id": "mr-1"}
                outcome, _ = self._await(ScriptedProbe(False), ScriptedProbe(reply))
                self.assertIs(outcome.verdict, verdict)
                self.assertEqual(outcome.source, "merge_status")
                self.assertEqual(outcome.merge_status, reply)
                self.assertEqual(outcome.polls, 1)

    def test_done_lands_even_when_commit_is_not_an_ancestor(self):
        reply = {"state": "done", "merge_sha": "abc"}
        outcome, _ = self._await(ScriptedProbe(False), ScriptedProbe(reply))
        self.assertIs(outcome.verdict, Verdict.LANDED)
        self.assertEqual(outcome.source, "merge_status")
        self.assertEqual(outcome.merge_status, reply)

    def test_ends_exactly_at_the_budget_and_never_starts_a_round_after_it(self):
        outcome, clock = self._await(ScriptedProbe(False),
                                     ScriptedProbe({"state": "verifying"}),
                                     budget=540, interval=30)
        self.assertIs(outcome.verdict, Verdict.PENDING)
        self.assertIsNone(outcome.source)
        self.assertEqual(clock.t, 540)
        self.assertEqual(outcome.elapsed_seconds, 540)
        self.assertEqual(sum(clock.sleeps), 540)
        self.assertTrue(all(s <= 30 for s in clock.sleeps), clock.sleeps)
        self.assertEqual(outcome.polls, 19)

    def test_final_sleep_is_truncated_to_the_remaining_budget(self):
        outcome, clock = self._await(ScriptedProbe(False),
                                     ScriptedProbe({"state": "verifying"}),
                                     budget=545, interval=30)
        self.assertIs(outcome.verdict, Verdict.PENDING)
        self.assertEqual(clock.sleeps[-1], 5)
        self.assertEqual(clock.t, 545)
        self.assertEqual(outcome.elapsed_seconds, 545)
        self.assertEqual(outcome.polls, 20)

    def test_zero_budget_is_exactly_one_round(self):
        outcome, clock = self._await(ScriptedProbe(False),
                                     ScriptedProbe({"state": "queued"}), budget=0)
        self.assertIs(outcome.verdict, Verdict.PENDING)
        self.assertEqual(outcome.polls, 1)
        self.assertEqual(clock.sleeps, [])
        self.assertEqual(outcome.merge_status, {"state": "queued"})

    def test_unavailable_merge_status_degrades_to_pending(self):
        down = aml.ProbeUnavailable("merge_status: connection refused")
        outcome, _ = self._await(ScriptedProbe(False), ScriptedProbe(down), budget=60)
        self.assertIs(outcome.verdict, Verdict.PENDING)
        self.assertIn("connection refused", outcome.probe_error)
        self.assertIsNone(outcome.merge_status)
        self.assertEqual(outcome.polls, 3)

    def test_transient_probe_error_does_not_end_the_wait(self):
        flaky = ScriptedProbe(aml.ProbeUnavailable("git: index.lock"), True)
        outcome, _ = self._await(flaky, ScriptedProbe({"state": "verifying"}))
        self.assertIs(outcome.verdict, Verdict.LANDED)
        self.assertEqual(outcome.source, "git_ancestry")
        self.assertEqual(outcome.polls, 2)

    def test_unknown_state_is_not_terminal(self):
        outcome, _ = self._await(ScriptedProbe(False),
                                 ScriptedProbe({"state": "unknown"}), budget=90)
        self.assertIs(outcome.verdict, Verdict.PENDING)
        self.assertEqual(outcome.merge_status, {"state": "unknown"})
        self.assertEqual(outcome.polls, 4)


class OptionsAndRearmTests(unittest.TestCase):
    EVERY_FLAG = [
        "--commit", "abc", "--fetch", "origin", "--ref", "origin/main",
        "--escalation-url", "http://x/mcp", "--repo", "/tmp/r",
        "--request-id", "mr-1", "--interval", "15", "--host-timeout-ms", "7200000",
    ]

    def _assert_usage_error(self, argv):
        with contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as caught:
                aml.parse_options(argv)
        self.assertEqual(caught.exception.code, 2)

    def test_defaults(self):
        options = aml.parse_options(["--commit", "abc", "--task-id", "7960"])
        self.assertEqual(options.commit, "abc")
        self.assertEqual(options.host_timeout_ms, aml.MONITOR_TIMEOUT_CAP_MS)
        self.assertEqual(aml.MONITOR_TIMEOUT_CAP_MS, 600000)
        self.assertEqual(options.interval_seconds, 30)
        self.assertEqual(options.ref, "main")
        self.assertIsNone(options.fetch_remote)
        self.assertIsNone(options.escalation_url)
        self.assertEqual(options.repo, REPO_ROOT)
        self.assertEqual(options.selector,
                         aml.MergeSelector(aml.SelectorKind.TASK_ID, "7960"))
        self.assertEqual(options.selector.kind.value, "task_id")

    def test_monitor_cap_budget_leaves_the_probe_round_reserve(self):
        budget = aml.invocation_budget_seconds(600000)
        self.assertGreater(budget, 0)
        self.assertLess(budget, 600)
        self.assertEqual(budget + aml.PROBE_ROUND_RESERVE_SECONDS, 600)
        self.assertEqual(aml.invocation_budget_seconds(7200000),
                         7200 - aml.PROBE_ROUND_RESERVE_SECONDS)

    def test_host_deadline_inside_the_reserve_clamps_to_one_round(self):
        self.assertEqual(aml.invocation_budget_seconds(1000), 0)

    def test_usage_errors_exit_2(self):
        cases = {
            "no selector": ["--commit", "abc"],
            "two selectors": ["--commit", "abc", "--task-id", "1",
                              "--request-id", "mr-1"],
            "no commit": ["--task-id", "1"],
            "zero interval": ["--commit", "abc", "--task-id", "1", "--interval", "0"],
            "negative interval": ["--commit", "abc", "--task-id", "1",
                                  "--interval", "-5"],
            "negative host timeout": ["--commit", "abc", "--task-id", "1",
                                      "--host-timeout-ms", "-1"],
        }
        for label, argv in cases.items():
            with self.subTest(label):
                self._assert_usage_error(argv)

    def test_rearm_argv_round_trips_every_flag(self):
        options = aml.parse_options(self.EVERY_FLAG)
        self.assertEqual(aml.parse_options(aml.rearm_argv(options)), options)

    def test_rearm_argv_round_trips_defaults_explicitly(self):
        options = aml.parse_options(["--commit", "abc", "--branch", "task/7960"])
        argv = aml.rearm_argv(options)
        self.assertEqual(aml.parse_options(argv), options)
        self.assertIn("--repo", argv)
        self.assertIn(str(REPO_ROOT), argv)

    def test_rearm_command_runs_this_script_under_python3(self):
        options = aml.parse_options(self.EVERY_FLAG)
        words = shlex.split(aml.rearm_command(options))
        self.assertEqual(words[0], "python3")
        self.assertEqual(words[1], str(TOOL_PATH.resolve()))
        self.assertEqual(aml.parse_options(words[2:]), options)


class StubEscalationServer:
    """The escalation MCP endpoint's wire shapes, as measured live 2026-10-01:
    initialize answers SSE plus an `mcp-session-id` header,
    notifications/initialized answers 202, and tools/call merge_status answers
    SSE whose result carries both content text and structuredContent. An
    unknown session id answers 404, as after a server restart."""

    def __init__(self, reply=None):
        self.reply = reply if reply is not None else {"state": "queued"}
        self.reply_format = "sse"  # or "json-text-only"
        self.is_error = False
        self.raw_tool_result = None  # sent verbatim when set
        self.calls = []
        self._sessions = set()
        self._lock = threading.Lock()

    def __enter__(self):
        self._httpd = ThreadingHTTPServer(("127.0.0.1", 0), self._handler_class())
        self._thread = threading.Thread(target=self._httpd.serve_forever,
                                        kwargs={"poll_interval": 0.05}, daemon=True)
        self._thread.start()
        return self

    def __exit__(self, *exc_info):
        self._httpd.shutdown()
        self._httpd.server_close()
        self._thread.join()

    @property
    def url(self):
        return f"http://127.0.0.1:{self._httpd.server_address[1]}/mcp"

    def forget_sessions(self):
        with self._lock:
            self._sessions.clear()

    def tool_calls(self):
        return [params for method, params in self.calls if method == "tools/call"]

    def _tool_result(self):
        if self.raw_tool_result is not None:
            return self.raw_tool_result
        result = {"content": [{"type": "text", "text": json.dumps(self.reply)}],
                  "isError": self.is_error}
        if self.reply_format == "sse":
            result["structuredContent"] = self.reply
        return result

    def _respond(self, payload, session_id):
        """(status, extra headers, JSON-RPC result or None)."""
        method = payload.get("method")
        with self._lock:
            self.calls.append((method, payload.get("params")))
            if method == "initialize":
                session_id = uuid.uuid4().hex
                self._sessions.add(session_id)
                return 200, {"mcp-session-id": session_id}, {
                    "protocolVersion": "2025-03-26", "capabilities": {"tools": {}},
                    "serverInfo": {"name": "stub-escalation", "version": "0"},
                }
            if session_id not in self._sessions:
                return 404, {}, None
            if method == "notifications/initialized":
                return 202, {}, None
            if method == "tools/call" and payload["params"]["name"] == "merge_status":
                return 200, {}, self._tool_result()
        return 400, {}, None

    def _encode(self, payload, result):
        """(content type, body) for a JSON-RPC result."""
        rpc = json.dumps({"jsonrpc": "2.0", "id": payload.get("id"), "result": result})
        if self.reply_format == "json-text-only" and payload.get("method") == "tools/call":
            return "application/json", rpc.encode()
        return "text/event-stream", f"event: message\ndata: {rpc}\n\n".encode()

    def _handler_class(self):
        stub = self

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                length = int(self.headers.get("Content-Length", "0"))
                payload = json.loads(self.rfile.read(length))
                status, headers, result = stub._respond(
                    payload, self.headers.get("Mcp-Session-Id"))
                content_type, body = ("text/plain", b"") if result is None \
                    else stub._encode(payload, result)
                self.send_response(status)
                for name, value in headers.items():
                    self.send_header(name, value)
                self.send_header("Content-Type", content_type)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *args):
                pass

        return Handler


def _client(url, kind=None, value="7960"):
    selector = aml.MergeSelector(kind or aml.SelectorKind.TASK_ID, value)
    return aml.MergeStatusClient(url, selector, timeout_seconds=10)


class MergeStatusClientTests(unittest.TestCase):
    def test_sse_reply_returns_structured_content(self):
        reply = {"state": "verifying", "request_id": "mr-1"}
        with StubEscalationServer(reply) as stub:
            self.assertEqual(_client(stub.url).status(), reply)
            self.assertEqual(stub.tool_calls(), [
                {"name": "merge_status", "arguments": {"task_id": "7960"}},
            ])

    def test_selector_is_forwarded_under_its_own_argument_name(self):
        for kind, value in ((aml.SelectorKind.REQUEST_ID, "mr-1"),
                            (aml.SelectorKind.BRANCH, "task/7960")):
            with self.subTest(kind=kind), StubEscalationServer() as stub:
                _client(stub.url, kind, value).status()
                self.assertEqual(stub.tool_calls(), [
                    {"name": "merge_status", "arguments": {kind.value: value}},
                ])

    def test_json_text_only_reply_falls_back_to_content_text(self):
        reply = {"state": "gate"}
        with StubEscalationServer(reply) as stub:
            stub.reply_format = "json-text-only"
            self.assertEqual(_client(stub.url).status(), reply)

    def test_tool_error_is_unavailable(self):
        with StubEscalationServer() as stub:
            stub.is_error = True
            with self.assertRaises(aml.ProbeUnavailable):
                _client(stub.url).status()

    def test_malformed_tool_result_is_unavailable(self):
        malformed = {
            "content is a string": {"content": "not-a-list"},
            "content item is not an object": {"content": [1]},
            "content text is null": {"content": [{"type": "text", "text": None}]},
            "content text is not JSON": {"content": [{"type": "text", "text": "{"}]},
            "content text is a JSON list": {"content": [{"type": "text", "text": "[]"}]},
            "no content at all": {},
        }
        for label, raw in malformed.items():
            with self.subTest(label), StubEscalationServer() as stub:
                stub.raw_tool_result = raw
                with self.assertRaises(aml.ProbeUnavailable):
                    _client(stub.url).status()

    def test_lost_session_reinitializes_on_the_next_poll(self):
        reply = {"state": "verifying"}
        with StubEscalationServer(reply) as stub:
            client = _client(stub.url)
            self.assertEqual(client.status(), reply)
            stub.forget_sessions()
            with self.assertRaises(aml.ProbeUnavailable):
                client.status()
            self.assertEqual(client.status(), reply)

    def test_nothing_listening_is_unavailable(self):
        with StubEscalationServer() as stub:
            url = stub.url
        with self.assertRaises(aml.ProbeUnavailable):
            _client(url).status()


# Command-line config and identity travel to children in these; an inherited
# value outranks the fixture's own repo, as pre-merge-commit's relative
# GIT_INDEX_FILE outranks `git -C` (task #7106).
_GIT_CONFIG_INJECTION_PREFIXES = ("GIT_CONFIG_KEY_", "GIT_CONFIG_VALUE_")
_GIT_CONFIG_INJECTION_KEYS = frozenset({"GIT_CONFIG_PARAMETERS", "GIT_CONFIG_COUNT"})
_GIT_IDENTITY_PREFIXES = ("GIT_AUTHOR_", "GIT_COMMITTER_")


@functools.cache
def repo_redirect_vars():
    """The one list of repo-redirect git variables, read from its bash home."""
    completed = subprocess.run(
        ["bash", "-c", 'source "$1"; printf %s "$REIFY_GIT_ENV_SCRUB_VARS"',
         "_", str(GIT_ENV_SCRUB_LIB)],
        capture_output=True, text=True, check=True,
    )
    names = frozenset(completed.stdout.split())
    if not names:
        raise RuntimeError(f"{GIT_ENV_SCRUB_LIB} yielded no REIFY_GIT_ENV_SCRUB_VARS")
    return names


def _is_ambient_git_state(key):
    return (
        key in repo_redirect_vars()
        or key in _GIT_CONFIG_INJECTION_KEYS
        or key.startswith(_GIT_CONFIG_INJECTION_PREFIXES)
        or key.startswith(_GIT_IDENTITY_PREFIXES)
    )


@functools.cache
def scrubbed_env():
    env = {k: v for k, v in os.environ.items() if not _is_ambient_git_state(k)}
    env["GIT_CONFIG_GLOBAL"] = os.devnull
    env["GIT_CONFIG_NOSYSTEM"] = "1"
    return env


def git(repo, *args):
    completed = subprocess.run(
        ["git", "-C", str(repo), "-c", "user.name=t", "-c", "user.email=t@example.test",
         *args],
        capture_output=True, text=True, env=scrubbed_env(),
    )
    if completed.returncode != 0:
        raise AssertionError(f"git {' '.join(args)} failed:{_diag(completed)}")
    return completed.stdout.strip()


def _diag(completed):
    return (f"\n--- exit {completed.returncode} ---\n--- stdout ---\n"
            f"{completed.stdout}\n--- stderr ---\n{completed.stderr}")


def _commit_file(repo, name, message):
    (repo / name).write_text(f"{name}\n")
    git(repo, "add", "--", name)
    git(repo, "commit", "-q", "-m", message)


class EndToEndTests(unittest.TestCase):
    """The real script as a subprocess, against a throwaway repo and the stub.
    Every run passes --host-timeout-ms 1000: budget 0, one round, no sleep."""

    SELECTOR = ("--task-id", "7960")

    def setUp(self):
        tmp = tempfile.TemporaryDirectory(prefix="await-merge-landing-")
        self.addCleanup(tmp.cleanup)
        self.tmp = Path(tmp.name).resolve()
        self.stub = self.enterContext(StubEscalationServer())
        self.repo, self.sha_b = self._make_repo(self.tmp / "repo")

    def _make_repo(self, repo):
        """main has commit A; `feature` has commit B, not merged."""
        repo.mkdir()
        git(repo, "init", "-q", "-b", "main")
        _commit_file(repo, "a.txt", "A")
        git(repo, "switch", "-q", "-c", "feature")
        _commit_file(repo, "b.txt", "B")
        sha_b = git(repo, "rev-parse", "HEAD")
        git(repo, "switch", "-q", "main")
        (repo / ".mcp.json").write_text(json.dumps(
            {"mcpServers": {"escalation": {"type": "http", "url": self.stub.url}}}))
        return repo, sha_b

    def _spawn(self, argv):
        return subprocess.run(
            argv, capture_output=True, text=True, env=scrubbed_env(), cwd=self.tmp,
            timeout=120,
        )

    def run_tool(self, *args):
        return self._spawn(["python3", str(TOOL_PATH), "--repo", str(self.repo),
                            "--host-timeout-ms", "1000", *args])

    def event(self, completed, exit_code):
        self.assertEqual(completed.returncode, exit_code, _diag(completed))
        lines = [line for line in completed.stdout.splitlines() if line.strip()]
        self.assertEqual(len(lines), 1, _diag(completed))
        return json.loads(lines[0])

    def assert_usage_error(self, completed):
        self.assertEqual(completed.returncode, 2, _diag(completed))
        self.assertEqual(completed.stdout, "", _diag(completed))
        self.assertNotEqual(completed.stderr.strip(), "", _diag(completed))

    def test_unmerged_commit_is_pending_with_a_rearm_command(self):
        event = self.event(self.run_tool("--commit", self.sha_b, *self.SELECTOR), 75)
        self.assertEqual(event["verdict"], "PENDING")
        self.assertEqual(event["commit"], self.sha_b)
        self.assertEqual(event["ref"], "main")
        self.assertEqual(event["merge_status"]["state"], "queued")
        self.assertIn("rearm_command", event)
        self.assertEqual(self.stub.tool_calls(), [
            {"name": "merge_status", "arguments": {"task_id": "7960"}},
        ])

    def test_rearm_command_reruns_the_identical_wait(self):
        first = self.event(self.run_tool("--commit", self.sha_b, *self.SELECTOR), 75)
        again = self.event(self._spawn(shlex.split(first["rearm_command"])), 75)
        self.assertEqual(again["verdict"], "PENDING")
        self.assertEqual(again["commit"], self.sha_b)
        self.assertEqual(again["rearm_command"], first["rearm_command"])

    def test_merged_commit_is_landed_by_git_ancestry(self):
        git(self.repo, "merge", "-q", "--no-ff", "-m", "merge feature", "feature")
        event = self.event(self.run_tool("--commit", self.sha_b, *self.SELECTOR), 0)
        self.assertEqual(event["verdict"], "LANDED")
        self.assertEqual(event["source"], "git_ancestry")
        self.assertNotIn("rearm_command", event)

    def test_branch_commit_is_resolved_once_so_rearm_survives_branch_deletion(self):
        first = self.event(self.run_tool("--commit", "feature", *self.SELECTOR), 75)
        self.assertEqual(first["commit"], self.sha_b)
        git(self.repo, "tag", "keep", "feature")
        git(self.repo, "branch", "-q", "-D", "feature")
        again = self.event(self._spawn(shlex.split(first["rearm_command"])), 75)
        self.assertEqual(again["commit"], self.sha_b)

    def test_terminal_merge_status_sets_the_exit_code(self):
        for state, verdict, exit_code in (("blocked", "BLOCKED", 3),
                                          ("abandoned", "FAILED", 4)):
            with self.subTest(state=state):
                self.stub.reply = {"state": state}
                event = self.event(
                    self.run_tool("--commit", self.sha_b, *self.SELECTOR), exit_code)
                self.assertEqual(event["verdict"], verdict)
                self.assertEqual(event["source"], "merge_status")

    def test_malformed_merge_status_reply_is_pending_not_a_crash(self):
        self.stub.raw_tool_result = {"content": [{"type": "text", "text": None}]}
        event = self.event(self.run_tool("--commit", self.sha_b, *self.SELECTOR), 75)
        self.assertEqual(event["verdict"], "PENDING")
        self.assertIsNotNone(event["probe_error"])

    def test_escalation_server_down_degrades_to_git_only(self):
        with StubEscalationServer() as stopped:
            closed_url = stopped.url
        event = self.event(self.run_tool("--commit", self.sha_b, *self.SELECTOR,
                                         "--escalation-url", closed_url), 75)
        self.assertEqual(event["verdict"], "PENDING")
        self.assertIsNotNone(event["probe_error"])

    def test_fetch_sees_a_landing_the_local_remote_ref_has_not(self):
        origin, work = self.tmp / "origin.git", self.tmp / "work"
        git(self.tmp, "clone", "-q", "--bare", str(self.repo), str(origin))
        git(self.repo, "remote", "add", "origin", str(origin))
        git(self.repo, "fetch", "-q", "origin")
        git(self.tmp, "clone", "-q", str(origin), str(work))
        git(work, "merge", "-q", "--no-ff", "-m", "merge feature", "origin/feature")
        git(work, "push", "-q", "origin", "main")
        args = ("--commit", self.sha_b, *self.SELECTOR, "--ref", "origin/main")
        stale = self.event(self.run_tool(*args), 75)
        self.assertEqual(stale["verdict"], "PENDING")
        fetched = self.event(self.run_tool(*args, "--fetch", "origin"), 0)
        self.assertEqual(fetched["verdict"], "LANDED")
        self.assertEqual(fetched["source"], "git_ancestry")

    def test_unresolvable_commit_is_a_usage_error(self):
        self.assert_usage_error(self.run_tool("--commit", "deadbeef" * 5, *self.SELECTOR))

    def test_unresolvable_ref_is_a_usage_error(self):
        self.assert_usage_error(self.run_tool("--commit", self.sha_b, *self.SELECTOR,
                                              "--ref", "nosuchref"))

    def test_no_escalation_url_is_a_usage_error(self):
        (self.repo / ".mcp.json").unlink()
        self.assert_usage_error(self.run_tool("--commit", self.sha_b, *self.SELECTOR))

    def test_missing_selector_is_a_usage_error(self):
        self.assert_usage_error(self.run_tool("--commit", self.sha_b))


if __name__ == "__main__":
    unittest.main()
