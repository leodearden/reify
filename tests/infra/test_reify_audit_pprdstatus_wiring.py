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
the shared stubs of tests/infra/stub_escalation_mcp.py:
  - a stub reify-audit: an executable written into a tempdir that records its
    argv and replays a canned stderr and exit code, shaped like the real
    binary's JSON-on-stderr contract; or, under --findings-file, that same
    canned stderr written to a file, as the /audit skill captures it;
  - a stub escalation MCP server: a ThreadingHTTPServer on 127.0.0.1:0 that
    records every JSON-RPC request and its mcp-session-id header.

SAFETY: --escalation-url always names the stub or an unbound port, so no test
can file into a live queue.

Assertions are on observable outcomes only: the requests the stub received,
the stub binary's recorded argv, exit codes and output. Nothing here greps the
script's text.
"""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from stub_escalation_mcp import (  # noqa: E402  (imported after sys.path manipulation)
    EMPTY_CORPUS_REFUSAL,
    RUNS_DB_FAILURE,
    STUB_ESCALATION,
    STUB_SESSION,
    StubEscalationServer,
    StubReifyAudit,
    detector_stderr,
    finding,
)

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "pprdstatus-escalate.py"


class PprdstatusEscalateTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)
        self.project_root = self.tmp / "project"
        self.project_root.mkdir()
        self.detector = StubReifyAudit(self.tmp / "stub" / "reify-audit")

    def detector_returns(self, stderr, exit_code):
        self.detector.returns(stderr, exit_code)

    def escalation_server(self, **kwargs):
        server = StubEscalationServer(**kwargs)
        self.addCleanup(server.close)
        return server

    def run_script(self, url, *extra):
        return self.run_script_with(
            url, "--reify-audit", str(self.detector.path), "--project-root", str(self.project_root), *extra
        )

    def run_script_on_findings_file(self, url, stderr):
        findings_file = self.tmp / "captured-stderr.txt"
        findings_file.write_text(stderr)
        return self.run_script_with(url, "--findings-file", str(findings_file))

    def run_script_with(self, url, *source_and_extra):
        return subprocess.run(
            [sys.executable, str(SCRIPT), "--escalation-url", url, *source_and_extra],
            capture_output=True,
            text=True,
            timeout=120,
        )

    def detector_was_run(self):
        return bool(self.detector.invocations())

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
        self.assertEqual(
            json.loads(result.stdout), {**STUB_ESCALATION, "finding_count": len(paths)}
        )

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
        self.assertEqual(result.stdout, "")

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
        server = self.escalation_server(error_tools={"escalate_info"})
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

    def test_findings_file_raises_its_pprdstatus_findings_without_running_a_detector(self):
        server = self.escalation_server()
        recorded = detector_stderr(
            [
                finding("docs/prds/a.md"),
                finding("crates/x/src/lib.rs", "todo-untracked", pattern="PTodo"),
                finding("docs/prds/b.md", "cite-status-contradiction"),
            ],
            "reify-audit: a diagnostic line ahead of the findings array\n",
        )

        result = self.run_script_on_findings_file(server.url, recorded)

        self.assert_one_batched_escalation(server, result, ["docs/prds/a.md", "docs/prds/b.md"])
        self.assertFalse(self.detector_was_run())

    def test_findings_file_without_an_array_exits_125_and_raises_nothing(self):
        server = self.escalation_server()

        result = self.run_script_on_findings_file(server.url, EMPTY_CORPUS_REFUSAL)

        self.assertEqual(result.returncode, 125, result.stderr)
        self.assertIn(EMPTY_CORPUS_REFUSAL, result.stderr)
        self.assertEqual(server.requests, [])

    def test_unreadable_findings_file_exits_125_naming_it(self):
        server = self.escalation_server()
        missing = self.tmp / "no-such-capture.txt"

        result = self.run_script_with(server.url, "--findings-file", str(missing))

        self.assertEqual(result.returncode, 125, result.stderr)
        self.assertIn(str(missing), result.stderr)
        self.assertEqual(server.requests, [])

    def test_detector_is_run_as_an_offline_pprdstatus_sweep_of_the_project_root(self):
        server = self.escalation_server()
        self.detector_returns(detector_stderr([]), 0)

        self.run_script(server.url)

        [argv] = self.detector.invocations()
        pairs = list(zip(argv, argv[1:]))
        self.assertIn(("--pattern", "PPRDSTATUS"), pairs, argv)
        self.assertIn("--no-jcodemunch", argv)
        self.assertIn(("--project-root", str(self.project_root)), pairs, argv)


if __name__ == "__main__":
    unittest.main()
