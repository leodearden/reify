#!/usr/bin/env python3
"""pprdstatus-escalate.py — raise a non-empty PPRDSTATUS finding set as ONE
batched escalation.

Usage:
  pprdstatus-escalate.py (--reify-audit BIN [--project-root DIR] | --findings-file PATH)
                         --escalation-url URL [--dry-run]

Takes the detector's stderr from one of two sources:
  - --reify-audit runs `BIN --pattern PPRDSTATUS --no-jcodemunch --project-root
    DIR`, with DIR as the detector's working directory so its cwd-relative
    defaults (the runs-db) resolve inside the project. This is the standalone
    default;
  - --findings-file reads a run's stderr as already captured, so a caller that
    records its own run (the /audit skill) raises exactly the findings it
    recorded, from one corpus load.
It then parses the findings array in that stderr, keeps its PPRDSTATUS
findings, and does one of two things:
  - an EMPTY set files nothing and makes no network contact at all;
  - a non-empty set files exactly ONE escalate_info on URL, under the fixed
    subject "audit", carrying the finding count, the list of PRD docs, and
    the docs-truth triage sitting to run.
Whatever the detector wrote to stderr ahead of the array, or all of it when
there is no array, is forwarded to stderr.

Flags:
  --reify-audit BIN     the reify-audit binary to run. Callers resolve it
                        through scripts/reify-audit-freshness.sh
                        `reify_audit_guard`, so a stale binary is never run.
  --project-root DIR    the checkout --reify-audit audits (default: .).
  --findings-file PATH  the captured stderr of a `--pattern PPRDSTATUS` run.
                        Pass a run of that pattern alone: over an empty task
                        corpus it refuses with no findings array, which exits
                        125 here, whereas a mixed run only prints a "skipped"
                        breadcrumb, and an array without PPRDSTATUS findings
                        cannot say whether they were checked.
  --escalation-url URL  the escalation server's MCP endpoint (required, with
                        no default, so no caller can file into the live queue
                        by omission). Reify's queue listens on
                        `escalation.port` (8100) in
                        dark-factory-orchestrator.yaml.
  --dry-run             print the escalate_info arguments as JSON instead of
                        filing them.

Output: a filed escalation prints its record as one JSON object on stdout,
{"id", "status", "level", "finding_count"}, and a human line on stderr. An
empty set prints nothing on stdout.

Exit codes:
  0    a findings array was read: nothing to raise, or raised (or, under
       --dry-run, printed)
  1    the escalation could not be filed
  125  no parseable findings array, so nothing was checked or raised: the
       detector failed, or it refused an empty task corpus, which leaves
       PPRDSTATUS nothing to check, or the findings file could not be read

This is the one-shot raise primitive. Its recurring cadence, set-level dedupe
and docs-truth aggregation belong to task #6347.
"""

import argparse
import http.client
import json
import os
import subprocess
import sys
import urllib.request

PATTERN = "PPRDSTATUS"
FINDING_PATTERN = "PPrdStatus"
SITTING = "/audit --pattern PPRDSTATUS"
ERROR_PREFIX = "pprdstatus-escalate:"
EXIT_FILE_FAILED = 1
EXIT_NO_FINDINGS_ARRAY = 125
MCP_PROTOCOL_VERSION = "2024-11-05"
MCP_TIMEOUT_SECONDS = 30

SUGGESTED_ACTION = (
    f"Book a docs-truth triage sitting ({SITTING}) and judge each listed doc on "
    "its own: a still-active PRD whose prose needs correcting; a completed plan "
    "that needs a terminal stamp per .claude/skills/prd/project.md \"PRD terminal "
    "status\"; or a dated snapshot that must not be edited. Do NOT auto-file fix "
    "tasks from these findings."
)


class EscalationError(Exception):
    """The escalation server answered, but did not accept the filing."""


def run_detector(binary, project_root):
    """(stderr, returncode) of one offline PPRDSTATUS sweep of project_root."""
    completed = subprocess.run(
        [binary, "--pattern", PATTERN, "--no-jcodemunch", "--project-root", project_root],
        cwd=project_root,
        capture_output=True,
        encoding="utf-8",
        errors="replace",
    )
    return completed.stderr, completed.returncode


def captured_stderr(args):
    """The detector's stderr, and where it came from; OSError if unreadable."""
    if args.findings_file:
        with open(args.findings_file, encoding="utf-8", errors="replace") as captured:
            return captured.read(), f"findings file {args.findings_file}"
    stderr, returncode = run_detector(args.reify_audit, os.path.abspath(args.project_root))
    return stderr, f"{args.reify_audit} exited {returncode}"


def findings_array_start(stderr):
    """Offset of the findings array: the last line that opens with `[`."""
    newline_bracket = stderr.rfind("\n[")
    if newline_bracket >= 0:
        return newline_bracket + 1
    return 0 if stderr.startswith("[") else None


def parse_findings(stderr):
    """The findings list, or None when stderr carries no parseable array.

    This is the binary's documented JSON-on-stderr contract
    (.claude/skills/audit/references/cli-invocation.md §3.1). The exit code is
    never read as failure once an array parses: it is the High count, and 125
    High findings is a legal result.
    """
    start = findings_array_start(stderr)
    if start is None:
        return None
    try:
        findings = json.loads(stderr[start:])
    except json.JSONDecodeError:
        return None
    return findings if isinstance(findings, list) else None


def detector_preamble(stderr):
    """Everything the detector wrote ahead of its findings array."""
    start = findings_array_start(stderr)
    return stderr if start is None else stderr[:start]


def escalation_arguments(findings):
    """The one batched escalate_info, built from structured fields only.

    The subject is the fixed "audit": each finding is keyed by a repo path, and
    a path cannot mint an escalation id (severity-routing.md §1).
    """
    docs = list(dict.fromkeys(finding["task_id"] for finding in findings))
    return {
        "task_id": "audit",
        "agent_role": "audit",
        "category": "risk_identified",
        "summary": (
            f"[PPrdStatus] {len(findings)} PPRDSTATUS finding(s) across {len(docs)} "
            f"PRD doc(s) — book a docs-truth triage sitting: {SITTING}"
        ),
        "detail": json.dumps(
            [{"path": finding["task_id"], "summary": finding["summary"]} for finding in findings]
        ),
        "suggested_action": SUGGESTED_ACTION,
        "terminal_state_is_the_bug": True,
    }


class McpSession:
    """A minimal streamable-HTTP MCP client: initialize, then tools/call.

    Keeps the server's mcp-session-id and sends it on every later post.
    Accepts both plain-JSON and SSE (`data:` line) reply bodies.
    """

    def __init__(self, url):
        self.url = url
        self.session_id = None
        self.next_id = 0

    def open(self):
        self._post(
            self._request(
                "initialize",
                {
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {"name": "pprdstatus-escalate", "version": "1"},
                },
            )
        )
        self._post({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def call_tool(self, name, arguments):
        return self._post(self._request("tools/call", {"name": name, "arguments": arguments}))

    def _request(self, method, params):
        self.next_id += 1
        return {"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params}

    def _post(self, payload):
        headers = {
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
        }
        if self.session_id:
            headers["mcp-session-id"] = self.session_id
        request = urllib.request.Request(
            self.url, data=json.dumps(payload).encode(), headers=headers
        )
        with urllib.request.urlopen(request, timeout=MCP_TIMEOUT_SECONDS) as response:
            self.session_id = response.headers.get("mcp-session-id") or self.session_id
            body = response.read().decode("utf-8")
        return reply_message(body)


def reply_message(body):
    """The JSON-RPC message in a reply body, or None for an empty (202) reply."""
    data_lines = [line[len("data:"):].strip() for line in body.splitlines() if line.startswith("data:")]
    if data_lines:
        return json.loads(data_lines[-1])
    return json.loads(body) if body.strip() else None


def file_escalation(url, arguments):
    """File escalate_info and return the server's record ({id, status, level})."""
    session = McpSession(url)
    session.open()
    return escalation_record(session.call_tool("escalate_info", arguments))


def escalation_record(reply):
    """The escalation record in a tools/call reply; EscalationError if refused."""
    if reply is None:
        raise EscalationError("empty tools/call reply")
    if "error" in reply:
        raise EscalationError(f"JSON-RPC error: {reply['error']}")
    result = reply.get("result") or {}
    text = "".join(
        item.get("text", "") for item in result.get("content", []) if item.get("type") == "text"
    )
    if result.get("isError"):
        raise EscalationError(f"escalate_info returned an error: {text}")
    try:
        record = json.loads(text)
    except json.JSONDecodeError as error:
        raise EscalationError(f"unparseable escalate_info payload {text!r}: {error}") from error
    if not isinstance(record, dict) or "error" in record:
        raise EscalationError(f"escalate_info refused the filing: {record}")
    return record


def parse_args(argv):
    parser = argparse.ArgumentParser(
        description="Raise a non-empty reify-audit PPRDSTATUS finding set as one batched escalation."
    )
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--reify-audit", metavar="BIN", help="the reify-audit binary to run")
    source.add_argument(
        "--findings-file",
        metavar="PATH",
        help="the captured stderr of a `--pattern PPRDSTATUS` run, instead of running one",
    )
    parser.add_argument(
        "--project-root", default=".", metavar="DIR", help="the checkout --reify-audit audits"
    )
    parser.add_argument(
        "--escalation-url", required=True, metavar="URL", help="the escalation MCP endpoint"
    )
    parser.add_argument(
        "--dry-run", action="store_true", help="print the escalate_info arguments instead of filing"
    )
    return parser.parse_args(argv)


def main(argv=None):
    args = parse_args(argv)
    try:
        stderr, origin = captured_stderr(args)
    except OSError as error:
        print(f"{ERROR_PREFIX} could not read the detector's findings: {error}", file=sys.stderr)
        return EXIT_NO_FINDINGS_ARRAY
    sys.stderr.write(detector_preamble(stderr))
    all_findings = parse_findings(stderr)
    if all_findings is None:
        print(
            f"{ERROR_PREFIX} no parseable findings array ({origin}); nothing was raised",
            file=sys.stderr,
        )
        return EXIT_NO_FINDINGS_ARRAY
    findings = [finding for finding in all_findings if finding.get("pattern") == FINDING_PATTERN]
    if not findings:
        print(f"{ERROR_PREFIX} no PPRDSTATUS findings; nothing to raise", file=sys.stderr)
        return 0
    arguments = escalation_arguments(findings)
    if args.dry_run:
        print(json.dumps(arguments, indent=2))
        return 0
    try:
        record = file_escalation(args.escalation_url, arguments)
    except (EscalationError, OSError, ValueError, http.client.HTTPException) as error:
        print(
            f"{ERROR_PREFIX} could not file the escalation at {args.escalation_url}: {error}",
            file=sys.stderr,
        )
        return EXIT_FILE_FAILED
    filed = {
        "id": record.get("id"),
        "status": record.get("status"),
        "level": record.get("level"),
        "finding_count": len(findings),
    }
    print(json.dumps(filed))
    print(
        f"filed {filed['id']} (status={filed['status']}, level={filed['level']}) "
        f"for {len(findings)} PPRDSTATUS finding(s)",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
