"""
stub_escalation_mcp.py — hermetic stand-ins for the two external parties a
reify escalation script talks to, shared by the infra suites that drive those
scripts as real subprocesses:

  - StubEscalationServer: a recording ThreadingHTTPServer on 127.0.0.1:0 that
    speaks just enough streamable-HTTP MCP (initialize, notifications/initialized,
    tools/call) and answers each tool from a per-tool-name canned payload;
  - StubReifyAudit: an executable reify-audit written to a chosen path, which
    logs its argv and replays a canned stderr and exit code per `--pattern`
    token, shaped like the real binary's JSON-on-stderr contract;
  - finding() / detector_stderr(): builders for that contract's findings.

Not a test module itself (no `test_` prefix): run_all.sh never discovers it,
and the suites import it via the sys.path idiom.
"""

import json
import stat
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

STUB_SESSION = "stub-session"
STUB_ESCALATION = {"id": "esc-audit-1", "status": "queued", "level": 0}
STUB_PROMOTION = {
    "id": "esc-audit-l2-1",
    "status": "created",
    "members": [STUB_ESCALATION["id"]],
    "severity": "info",
}
STUB_TOOL_FAILURE = "stub tool failure"

DEFAULT_PAYLOADS = {"escalate_info": STUB_ESCALATION, "promote_to_l2": STUB_PROMOTION}

# Two ways the binary exits 125 with no findings array: an infrastructure
# failure, and its refusal of a PPRDSTATUS-only run over an empty task corpus.
RUNS_DB_FAILURE = "reify-audit: error opening runs-db 'x': unable to open\n"
EMPTY_CORPUS_REFUSAL = (
    "reify-audit: the task corpus is empty and every selected detector needs it; "
    "refusing rather than reporting an unchecked run as clean "
    "(check --project-root and --fused-memory-url, or --tasks-file)\n"
)


def finding(path, kind="stale-status-header", pattern="PPrdStatus", severity="High"):
    """One finding, shaped like the binary's serde output."""
    return {
        "pattern": pattern,
        "severity": severity,
        "task_id": path,
        "summary": f"{kind}: {path} — a canned summary",
        "evidence": [{"File": {"path": path}}],
    }


def detector_stderr(findings, preamble=""):
    """The binary's stderr: optional diagnostic lines, then the pretty array."""
    return preamble + json.dumps(findings, indent=2, ensure_ascii=False) + "\n"


class StubEscalationServer:
    """A recording stand-in for the escalation server's streamable-HTTP MCP.

    payloads overrides the canned record a tool replies with; error_tools names
    the tools that reply isError=True instead.
    """

    def __init__(self, sse=False, error_tools=(), payloads=None):
        self.sse = sse
        self.error_tools = frozenset(error_tools)
        self.payloads = {**DEFAULT_PAYLOADS, **(payloads or {})}
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

    def calls_to(self, name):
        """The arguments of every tools/call naming the tool `name`, in order."""
        return [
            r["body"]["params"]["arguments"]
            for r in self.tool_calls()
            if r["body"]["params"].get("name") == name
        ]

    def tool_result(self, name):
        if name in self.error_tools:
            return {"content": [{"type": "text", "text": STUB_TOOL_FAILURE}], "isError": True}
        payload = self.payloads.get(name, {"error": f"stub has no canned reply for {name}"})
        return {"content": [{"type": "text", "text": json.dumps(payload)}], "isError": False}

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
                    result = stub.tool_result(body.get("params", {}).get("name"))
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


STUB_REIFY_AUDIT = """#!{python}
import json, pathlib, sys
here = pathlib.Path(__file__).resolve().parent
argv = sys.argv[1:]
with open(here / {argv_log!r}, "a", encoding="utf-8") as log:
    log.write(json.dumps(argv) + "\\n")
responses = json.loads((here / {config!r}).read_text(encoding="utf-8"))
token = argv[argv.index("--pattern") + 1] if "--pattern" in argv else None
response = responses.get(token, responses.get({fallback!r}))
if response is None:
    sys.stderr.write(f"stub reify-audit: no canned response for --pattern {{token}}\\n")
    sys.exit(99)
sys.stderr.write(response["stderr"])
sys.exit(response["exit"])
"""


class StubReifyAudit:
    """An executable reify-audit at `path` that replays canned responses.

    Its config and argv log live beside it, so a stub placed at a project's
    target/release/reify-audit is self-contained there.
    """

    ARGV_LOG = "stub-argv.jsonl"
    CONFIG = "stub-config.json"
    ANY_PATTERN = "*"

    def __init__(self, path):
        self.path = Path(path)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self._responses = {}
        self.path.write_text(
            STUB_REIFY_AUDIT.format(
                python=sys.executable,
                argv_log=self.ARGV_LOG,
                config=self.CONFIG,
                fallback=self.ANY_PATTERN,
            )
        )
        self.path.chmod(self.path.stat().st_mode | stat.S_IXUSR)
        self._write_config()

    def returns(self, stderr, exit_code, pattern=ANY_PATTERN):
        """Replay stderr and exit_code for `--pattern pattern` (default: any)."""
        self._responses[pattern] = {"stderr": stderr, "exit": exit_code}
        self._write_config()

    def invocations(self):
        """Every argv the stub was run with, oldest first."""
        log = self.path.parent / self.ARGV_LOG
        if not log.exists():
            return []
        return [json.loads(line) for line in log.read_text(encoding="utf-8").splitlines()]

    def _write_config(self):
        (self.path.parent / self.CONFIG).write_text(json.dumps(self._responses), encoding="utf-8")
