#!/usr/bin/env python3
"""
test_reify_audit_pprdstatus_wiring.py — stdlib unittest for
scripts/pprdstatus-escalate.py, the one-shot consumer that raises a non-empty
`reify-audit --pattern PPRDSTATUS` finding set to the escalation queue as ONE
batched escalate_info, and raises nothing when the set is empty.

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_reify_audit_pprdstatus_wiring.sh, which
invokes this file.

HOW THE SCRIPT IS DRIVEN: every test runs the REAL script as a subprocess, with
  - a stub reify-audit: an executable written into a tempdir that records its
    argv and replays a canned stderr and exit code, shaped like the real
    binary's JSON-on-stderr contract;
  - a stub escalation MCP server: a ThreadingHTTPServer on 127.0.0.1:0 that
    records every JSON-RPC request and its mcp-session-id header.

SAFETY: --escalation-url always names the stub or an unbound port, so no test
can file into a live queue.

Assertions are on observable outcomes only: the requests the stub received,
the stub binary's recorded argv, exit codes and output. Nothing here greps the
script's text.
"""

import json
import stat
import subprocess
import sys
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "pprdstatus-escalate.py"

STUB_SESSION = "stub-session"
STUB_ESCALATION = {"id": "esc-audit-1", "status": "queued", "level": 0}

STUB_REIFY_AUDIT = """#!{python}
import json, pathlib, sys
here = pathlib.Path(__file__).resolve().parent
(here / "argv.json").write_text(json.dumps(sys.argv[1:]))
config = json.loads((here / "stub-config.json").read_text())
sys.stderr.write(config["stderr"])
sys.exit(config["exit"])
"""

# Two ways the binary exits 125 with no findings array: an infrastructure
# failure, and its refusal of a PPRDSTATUS-only run over an empty task corpus.
RUNS_DB_FAILURE = "reify-audit: error opening runs-db 'x': unable to open\n"
EMPTY_CORPUS_REFUSAL = (
    "reify-audit: the task corpus is empty and every selected detector needs it; "
    "refusing rather than reporting an unchecked run as clean "
    "(check --project-root and --fused-memory-url, or --tasks-file)\n"
)


def finding(path, kind="stale-status-header"):
    """One PPRDSTATUS finding, shaped like the binary's serde output."""
    return {
        "pattern": "PPrdStatus",
        "severity": "High",
        "task_id": path,
        "summary": f"{kind}: {path} — a canned summary",
        "evidence": [{"File": {"path": path}}],
    }


def detector_stderr(findings, preamble=""):
    """The binary's stderr: optional diagnostic lines, then the pretty array."""
    return preamble + json.dumps(findings, indent=2, ensure_ascii=False) + "\n"


class StubEscalationServer:
    """A recording stand-in for the escalation server's streamable-HTTP MCP."""

    def __init__(self, sse=False, tool_error=False):
        self.sse = sse
        self.tool_error = tool_error
        self.requests = []
        self._lock = threading.Lock()
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), self._handler_class())
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    @property
    def url(self):
        return f"http://127.0.0.1:{self.server.server_address[1]}/mcp"

    def close(self):
        self.server.shutdown()
        self.server.server_close()

    def record(self, body, session):
        with self._lock:
            self.requests.append({"body": body, "session": session})

    def tool_calls(self):
        return [r for r in self.requests if r["body"].get("method") == "tools/call"]

    def methods(self):
        return [r["body"].get("method") for r in self.requests]

    def tool_result(self):
        if self.tool_error:
            return {"content": [{"type": "text", "text": "stub tool failure"}], "isError": True}
        return {
            "content": [{"type": "text", "text": json.dumps(STUB_ESCALATION)}],
            "isError": False,
        }

    def _handler_class(self):
        stub = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def do_POST(self):
                length = int(self.headers.get("Content-Length", "0"))
                body = json.loads(self.rfile.read(length) or b"{}")
                stub.record(body, self.headers.get("mcp-session-id"))
                method = body.get("method")
                if method == "notifications/initialized":
                    self.send_response(202)
                    self.send_header("Content-Length", "0")
                    self.end_headers()
                    return
                if method == "initialize":
                    result = {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {},
                        "serverInfo": {"name": "stub-escalation", "version": "0"},
                    }
                else:
                    result = stub.tool_result()
                self.reply({"jsonrpc": "2.0", "id": body.get("id"), "result": result})

            def reply(self, message):
                if stub.sse:
                    payload = f"event: message\ndata: {json.dumps(message)}\n\n".encode()
                    content_type = "text/event-stream"
                else:
                    payload = json.dumps(message).encode()
                    content_type = "application/json"
                self.send_response(200)
                self.send_header("Content-Type", content_type)
                self.send_header("Content-Length", str(len(payload)))
                self.send_header("mcp-session-id", STUB_SESSION)
                self.end_headers()
                self.wfile.write(payload)

        return Handler


class PprdstatusEscalateTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)
        self.project_root = self.tmp / "project"
        self.project_root.mkdir()
        self.stub_dir = self.tmp / "stub"
        self.stub_dir.mkdir()
        self.stub = self.stub_dir / "reify-audit"
        self.stub.write_text(STUB_REIFY_AUDIT.format(python=sys.executable))
        self.stub.chmod(self.stub.stat().st_mode | stat.S_IXUSR)

    def detector_returns(self, stderr, exit_code):
        config = {"stderr": stderr, "exit": exit_code}
        (self.stub_dir / "stub-config.json").write_text(json.dumps(config))

    def escalation_server(self, **kwargs):
        server = StubEscalationServer(**kwargs)
        self.addCleanup(server.close)
        return server

    def run_script(self, url, *extra):
        return subprocess.run(
            [
                sys.executable,
                str(SCRIPT),
                "--reify-audit",
                str(self.stub),
                "--project-root",
                str(self.project_root),
                "--escalation-url",
                url,
                *extra,
            ],
            capture_output=True,
            text=True,
            timeout=120,
        )

    def assert_one_batched_escalation(self, server, result, paths):
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = server.tool_calls()
        self.assertEqual(len(calls), 1, server.methods())
        call = calls[0]
        self.assertEqual(call["body"]["params"]["name"], "escalate_info")
        self.assertEqual(call["session"], STUB_SESSION)
        methods = server.methods()
        self.assertLess(methods.index("initialize"), methods.index("tools/call"), methods)

        arguments = call["body"]["params"]["arguments"]
        self.assertEqual(arguments["task_id"], "audit")
        self.assertEqual(arguments["agent_role"], "audit")
        self.assertEqual(arguments["category"], "risk_identified")
        for needle in ("PPRDSTATUS", str(len(paths)), "/audit --pattern PPRDSTATUS"):
            self.assertIn(needle, arguments["summary"])
        detail = json.loads(arguments["detail"])
        self.assertEqual([entry["path"] for entry in detail], paths)
        self.assertTrue(all(entry["summary"] for entry in detail), detail)
        self.assertIn(STUB_ESCALATION["id"], result.stdout)

    def test_two_findings_file_exactly_one_escalation(self):
        server = self.escalation_server()
        self.detector_returns(
            detector_stderr(
                [finding("docs/prds/a.md"), finding("docs/prds/b.md", "cite-status-contradiction")]
            ),
            2,
        )

        result = self.run_script(server.url)

        self.assert_one_batched_escalation(server, result, ["docs/prds/a.md", "docs/prds/b.md"])

    def test_empty_finding_set_makes_no_request(self):
        server = self.escalation_server()
        self.detector_returns(detector_stderr([]), 0)

        result = self.run_script(server.url)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(server.requests, [])

    def test_diagnostics_ahead_of_the_array_are_forwarded_and_the_array_still_parses(self):
        server = self.escalation_server()
        preamble = "reify-audit: a diagnostic line ahead of the findings array\n"
        self.detector_returns(detector_stderr([finding("docs/prds/a.md")], preamble), 1)

        result = self.run_script(server.url)

        self.assert_one_batched_escalation(server, result, ["docs/prds/a.md"])
        self.assertIn(preamble, result.stderr)

    def test_unchecked_run_exits_125_forwards_the_reason_and_raises_nothing(self):
        for label, stderr in (
            ("infrastructure failure", RUNS_DB_FAILURE),
            ("empty-corpus refusal", EMPTY_CORPUS_REFUSAL),
        ):
            with self.subTest(label):
                server = self.escalation_server()
                self.detector_returns(stderr, 125)

                result = self.run_script(server.url)

                self.assertEqual(result.returncode, 125, result.stderr)
                self.assertIn(stderr, result.stderr)
                self.assertEqual(server.requests, [])

    def test_nonzero_detector_exit_with_findings_is_a_high_count_not_a_failure(self):
        server = self.escalation_server()
        paths = ["docs/prds/a.md", "docs/prds/b.md", "docs/prds/c.md"]
        self.detector_returns(detector_stderr([finding(p) for p in paths]), 3)

        result = self.run_script(server.url)

        self.assert_one_batched_escalation(server, result, paths)

    def test_sse_framed_replies_are_understood(self):
        server = self.escalation_server(sse=True)
        self.detector_returns(
            detector_stderr([finding("docs/prds/a.md"), finding("docs/prds/b.md")]), 2
        )

        result = self.run_script(server.url)

        self.assert_one_batched_escalation(server, result, ["docs/prds/a.md", "docs/prds/b.md"])

    def test_tool_error_is_a_failed_filing(self):
        server = self.escalation_server(tool_error=True)
        self.detector_returns(detector_stderr([finding("docs/prds/a.md")]), 1)

        result = self.run_script(server.url)

        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn("pprdstatus-escalate:", result.stderr)
        self.assertIn("stub tool failure", result.stderr)

    def test_unreachable_escalation_server_exits_1_naming_the_url(self):
        url = "http://127.0.0.1:0/mcp"
        self.detector_returns(detector_stderr([finding("docs/prds/a.md")]), 1)

        result = self.run_script(url)

        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn(url, result.stderr)

    def test_dry_run_prints_the_arguments_and_makes_no_request(self):
        server = self.escalation_server()
        self.detector_returns(detector_stderr([finding("docs/prds/a.md")]), 1)

        result = self.run_script(server.url, "--dry-run")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(server.requests, [])
        arguments = json.loads(result.stdout)
        self.assertEqual(arguments["task_id"], "audit")
        self.assertEqual(
            [entry["path"] for entry in json.loads(arguments["detail"])], ["docs/prds/a.md"]
        )

    def test_detector_is_run_as_an_offline_pprdstatus_sweep_of_the_project_root(self):
        server = self.escalation_server()
        self.detector_returns(detector_stderr([]), 0)

        self.run_script(server.url)

        argv = json.loads((self.stub_dir / "argv.json").read_text())
        pairs = list(zip(argv, argv[1:]))
        self.assertIn(("--pattern", "PPRDSTATUS"), pairs, argv)
        self.assertIn("--no-jcodemunch", argv)
        self.assertIn(("--project-root", str(self.project_root)), pairs, argv)


if __name__ == "__main__":
    unittest.main()
