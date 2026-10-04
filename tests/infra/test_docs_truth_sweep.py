#!/usr/bin/env python3
"""
test_docs_truth_sweep.py — stdlib unittest for scripts/docs-truth-sweep.py,
the recurring sweep that runs every docs-truth detector (PPRDSTATUS, PCITE)
and books ONE human adjudication sitting at L2 when the finding set holds
something new.

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_docs_truth_sweep.sh, which invokes this
file.

HOW THE SCRIPT IS DRIVEN: every test runs the REAL script as a subprocess, with
the shared stubs of tests/infra/stub_escalation_mcp.py: a stub reify-audit that
logs its argv and replays a canned stderr and exit code per `--pattern` token,
and a stub escalation MCP server that records every JSON-RPC request.

SAFETY: --escalation-url always names the stub or an unbound port, so no test
can file into a live queue.

Assertions are on observable outcomes only: the requests the stub received,
the stub binary's recorded argv, exit codes, output and the state file.
"""

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from stub_escalation_mcp import (  # noqa: E402  (imported after sys.path manipulation)
    EMPTY_CORPUS_REFUSAL,
    STUB_ESCALATION,
    STUB_PROMOTION,
    STUB_SESSION,
    StubEscalationServer,
    StubReifyAudit,
    detector_stderr,
    finding,
)

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "docs-truth-sweep.py"

FAMILY_TOKENS = ("PPRDSTATUS", "PCITE")


def prd_finding(path, kind="stale-status-header"):
    return finding(path, kind)


def cite_finding(path, kind="fabricated-cite"):
    return finding(path, kind, pattern="PManifestCite", severity="Medium")


def hermetic_env():
    """The caller's environment minus GIT_*, so no hook context leaks a repo in."""
    return {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}


class DocsTruthSweepTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)
        self.project_root = self.tmp / "project"
        self.project_root.mkdir()
        self.state_file = self.tmp / "state" / "docs-truth-sweep.json"
        self.detector = StubReifyAudit(self.tmp / "stub" / "reify-audit")
        self.corpus()

    def corpus(self, prd=(), cite=()):
        """Each member's findings this run; an empty member is a clean array."""
        self.detector.returns(detector_stderr(list(prd)), len(prd), pattern="PPRDSTATUS")
        self.detector.returns(detector_stderr(list(cite)), 0, pattern="PCITE")

    def escalation_server(self, **kwargs):
        server = StubEscalationServer(**kwargs)
        self.addCleanup(server.close)
        return server

    def sweep(self, prd=(), cite=(), **server_kwargs):
        """One run against a fresh stub server, so its requests are this run's alone."""
        server = self.escalation_server(**server_kwargs)
        self.corpus(prd=prd, cite=cite)
        return server, self.run_sweep(server.url)

    def run_sweep(self, url, *extra):
        return self.run_sweep_with("--escalation-url", url, *extra)

    def run_sweep_with(self, *endpoint_and_extra):
        return subprocess.run(
            [
                sys.executable,
                str(SCRIPT),
                "--reify-audit",
                str(self.detector.path),
                "--project-root",
                str(self.project_root),
                "--state-file",
                str(self.state_file),
                *endpoint_and_extra,
            ],
            capture_output=True,
            text=True,
            timeout=120,
            env=hermetic_env(),
        )

    def assert_one_sitting_raised(self, server, result):
        """Exactly one escalate_info then one promote_to_l2, on one session."""
        self.assertEqual(result.returncode, 0, result.stderr)
        tool_calls = server.tool_calls()
        self.assertEqual(
            [call["body"]["params"]["name"] for call in tool_calls],
            ["escalate_info", "promote_to_l2"],
            server.methods(),
        )
        self.assertTrue(all(call["session"] == STUB_SESSION for call in tool_calls))
        methods = server.methods()
        self.assertLess(methods.index("initialize"), methods.index("tools/call"), methods)
        [member] = server.calls_to("escalate_info")
        [promotion] = server.calls_to("promote_to_l2")
        return member, promotion, json.loads(result.stdout)

    def assert_silent(self, server, result):
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(server.requests, [])
        self.assertEqual(result.stdout, "")

    def test_first_raise_aggregates_the_family_into_one_l2_sitting(self):
        server = self.escalation_server()
        self.corpus(
            prd=[prd_finding("docs/prds/a.md"), prd_finding("docs/prds/b.md")],
            cite=[cite_finding("docs/prds/x.capability-manifest.md")],
        )

        result = self.run_sweep(server.url)

        member, promotion, record = self.assert_one_sitting_raised(server, result)
        self.assertEqual(member["task_id"], "audit")
        self.assertEqual(member["agent_role"], "docs-truth-sweep")
        self.assertEqual(member["category"], "risk_identified")
        self.assertEqual(member["severity"], "info")
        self.assertIs(member["terminal_state_is_the_bug"], True)
        self.assertIn("3 finding", member["summary"])
        self.assertIn("3 doc", member["summary"])
        self.assertEqual(
            sorted((entry["pattern"], entry["path"]) for entry in json.loads(member["detail"])),
            [
                ("PManifestCite", "docs/prds/x.capability-manifest.md"),
                ("PPrdStatus", "docs/prds/a.md"),
                ("PPrdStatus", "docs/prds/b.md"),
            ],
        )
        self.assertTrue(all(entry["summary"] for entry in json.loads(member["detail"])))
        for sitting in ("/audit --pattern PPRDSTATUS", "/audit --pattern PCITE"):
            self.assertIn(sitting, member["suggested_action"])
        [evidence] = member["evidence"]
        self.assertTrue(evidence["measured_at"].startswith("HEAD="), evidence)

        self.assertEqual(promotion["member_ids"], [STUB_ESCALATION["id"]])
        self.assertTrue(promotion["root_cause"].strip())
        self.assertIsInstance(promotion["options"], list)
        self.assertTrue(promotion["options"])
        self.assertIn("3", promotion["summary"])

        self.assertEqual(
            record,
            {
                "member_id": STUB_ESCALATION["id"],
                "l2_id": STUB_PROMOTION["id"],
                "l2_status": STUB_PROMOTION["status"],
                "finding_count": 3,
                "doc_count": 3,
                "new_count": 3,
            },
        )

    def test_each_member_is_its_own_single_token_offline_run(self):
        server = self.escalation_server()

        self.run_sweep(server.url)

        invocations = self.detector.invocations()
        self.assertEqual(
            sorted(argv[argv.index("--pattern") + 1] for argv in invocations),
            sorted(FAMILY_TOKENS),
        )
        for argv in invocations:
            pairs = list(zip(argv, argv[1:]))
            self.assertIn("--no-jcodemunch", argv)
            self.assertIn(("--project-root", str(self.project_root)), pairs, argv)

    def test_clean_corpus_is_silent(self):
        self.assert_silent(*self.sweep())

    # ── Silence discipline: raise only when the set holds a new (pattern, doc) ──

    def test_unchanged_set_is_not_raised_twice(self):
        findings = [prd_finding("docs/prds/a.md"), prd_finding("docs/prds/b.md")]
        self.assert_one_sitting_raised(*self.sweep(prd=findings))

        self.assert_silent(*self.sweep(prd=findings))

    def test_summary_churn_on_the_same_docs_is_not_news(self):
        self.assert_one_sitting_raised(
            *self.sweep(prd=[prd_finding("docs/prds/a.md"), prd_finding("docs/prds/b.md")])
        )
        churned = [
            {**prd_finding("docs/prds/a.md"), "summary": "stale-status-header: line 14, 6/9 leaves"},
            {**prd_finding("docs/prds/b.md"), "summary": "cite-status-contradiction: line 3"},
        ]

        self.assert_silent(*self.sweep(prd=churned))

    def test_a_shrink_is_silent_and_a_regression_is_news(self):
        a, b = prd_finding("docs/prds/a.md"), prd_finding("docs/prds/b.md")
        self.assert_one_sitting_raised(*self.sweep(prd=[a, b]))
        self.assert_silent(*self.sweep(prd=[a]))

        _, _, record = self.assert_one_sitting_raised(*self.sweep(prd=[a, b]))

        self.assertEqual((record["new_count"], record["finding_count"]), (1, 2))

    def test_growth_raises_the_full_set_counting_only_the_new_doc(self):
        a, c = prd_finding("docs/prds/a.md"), prd_finding("docs/prds/c.md")
        self.assert_one_sitting_raised(*self.sweep(prd=[a]))

        member, _, record = self.assert_one_sitting_raised(*self.sweep(prd=[a, c]))

        self.assertEqual(
            sorted(entry["path"] for entry in json.loads(member["detail"])),
            ["docs/prds/a.md", "docs/prds/c.md"],
        )
        self.assertEqual((record["new_count"], record["finding_count"]), (1, 2))

    def test_an_empty_set_is_recorded_so_a_reappearance_is_news(self):
        a = prd_finding("docs/prds/a.md")
        self.assert_one_sitting_raised(*self.sweep(prd=[a]))
        self.assert_silent(*self.sweep())

        _, _, record = self.assert_one_sitting_raised(*self.sweep(prd=[a]))

        self.assertEqual(record["new_count"], 1)

    def test_the_same_doc_under_a_different_detector_is_news(self):
        self.assert_one_sitting_raised(*self.sweep(prd=[prd_finding("docs/prds/a.md")]))

        _, _, record = self.assert_one_sitting_raised(
            *self.sweep(prd=[prd_finding("docs/prds/a.md")], cite=[cite_finding("docs/prds/a.md")])
        )

        self.assertEqual((record["new_count"], record["finding_count"]), (1, 2))

    def test_unreadable_state_refuses_before_any_work_and_keeps_the_file(self):
        for label, content in (
            ("not JSON", "{not json"),
            ("a bare list", "[]"),
            ("an unknown version", '{"version": 2, "seen": []}'),
            ("seen is not a list", '{"version": 1, "seen": "docs/prds/a.md"}'),
            ("an identity missing its path", '{"version": 1, "seen": [{"pattern": "PPrdStatus"}]}'),
            ("a non-string field", '{"version": 1, "seen": [{"pattern": 1, "path": "a.md"}]}'),
        ):
            with self.subTest(label):
                self.state_file.parent.mkdir(parents=True, exist_ok=True)
                self.state_file.write_text(content)
                runs_before = len(self.detector.invocations())

                server, result = self.sweep(prd=[prd_finding("docs/prds/a.md")])

                self.assertEqual(result.returncode, 125, result.stderr)
                self.assertIn(str(self.state_file), result.stderr)
                self.assertEqual(server.requests, [])
                self.assertEqual(len(self.detector.invocations()), runs_before)
                self.assertEqual(self.state_file.read_text(), content)

    def test_a_missing_state_file_is_a_first_run_that_records_the_set(self):
        self.assertFalse(self.state_file.parent.exists())

        self.assert_one_sitting_raised(*self.sweep(prd=[prd_finding("docs/prds/a.md")]))

        self.assertTrue(self.state_file.is_file())

    # ── Failure semantics: all-or-nothing, and state moves only on success ──

    def seed_state(self):
        """A valid recorded observation, whose bytes a failed run must not touch."""
        self.state_file.parent.mkdir(parents=True, exist_ok=True)
        self.state_file.write_text(
            '{"version": 1, "seen": [{"pattern": "PPrdStatus", "path": "docs/prds/old.md"}]}\n'
        )
        return self.state_file.read_bytes()

    def assert_state(self, expected_bytes):
        if expected_bytes is None:
            self.assertFalse(self.state_file.exists())
        else:
            self.assertEqual(self.state_file.read_bytes(), expected_bytes)

    def test_one_unchecked_member_raises_nothing_and_keeps_the_state(self):
        state = self.seed_state()
        server = self.escalation_server()
        self.corpus(cite=[cite_finding("docs/prds/x.capability-manifest.md")])
        self.detector.returns(EMPTY_CORPUS_REFUSAL, 125, pattern="PPRDSTATUS")

        result = self.run_sweep(server.url)

        self.assertEqual(result.returncode, 125, result.stderr)
        self.assertIn(EMPTY_CORPUS_REFUSAL, result.stderr)
        self.assertIn("PPRDSTATUS", result.stderr.replace(EMPTY_CORPUS_REFUSAL, ""))
        self.assertEqual(server.requests, [])
        self.assert_state(state)

    def test_a_failed_filing_keeps_the_state_so_the_next_run_retries(self):
        findings = [prd_finding("docs/prds/a.md")]
        server, result = self.sweep(prd=findings, error_tools={"escalate_info"})

        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn(server.url, result.stderr)
        self.assertEqual(server.calls_to("promote_to_l2"), [])
        self.assert_state(None)

        self.assert_one_sitting_raised(*self.sweep(prd=findings))

    def test_a_failed_promotion_keeps_the_state(self):
        state = self.seed_state()
        server, result = self.sweep(
            prd=[prd_finding("docs/prds/a.md")], error_tools={"promote_to_l2"}
        )

        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn(server.url, result.stderr)
        self.assertEqual(len(server.calls_to("escalate_info")), 1)
        self.assert_state(state)

    def test_an_unpersisted_filing_is_not_filed_and_is_not_promoted(self):
        state = self.seed_state()
        unpersisted = {**STUB_ESCALATION, "status": "accepted_unpersisted", "persist_check": "x"}
        server, result = self.sweep(
            prd=[prd_finding("docs/prds/a.md")], payloads={"escalate_info": unpersisted}
        )

        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn("accepted_unpersisted", result.stderr)
        self.assertEqual(server.calls_to("promote_to_l2"), [])
        self.assert_state(state)

    def test_an_unreachable_endpoint_exits_1_naming_the_url(self):
        url = "http://127.0.0.1:0/mcp"
        self.corpus(prd=[prd_finding("docs/prds/a.md")])

        result = self.run_sweep(url)

        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn(url, result.stderr)
        self.assert_state(None)

    def test_dry_run_prints_the_planned_raise_and_touches_nothing(self):
        server = self.escalation_server()
        self.corpus(prd=[prd_finding("docs/prds/a.md")])

        result = self.run_sweep(server.url, "--dry-run")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(server.requests, [])
        planned = json.loads(result.stdout)
        self.assertEqual(set(planned), {"escalate_info", "promote_to_l2"})
        self.assertEqual(
            [entry["path"] for entry in json.loads(planned["escalate_info"]["detail"])],
            ["docs/prds/a.md"],
        )
        [placeholder] = planned["promote_to_l2"]["member_ids"]
        self.assertNotEqual(placeholder, STUB_ESCALATION["id"])
        self.assert_state(None)

    def write_mcp_config(self, servers):
        config = self.tmp / ".mcp.json"
        config.write_text(json.dumps({"mcpServers": servers}))
        return config

    def test_mcp_config_names_the_endpoint(self):
        server = self.escalation_server()
        config = self.write_mcp_config({"escalation": {"type": "http", "url": server.url}})
        self.corpus(prd=[prd_finding("docs/prds/a.md")])

        result = self.run_sweep_with("--mcp-config", str(config))

        self.assert_one_sitting_raised(server, result)

    def test_mcp_config_without_an_escalation_endpoint_refuses_before_any_work(self):
        server = self.escalation_server()
        config = self.write_mcp_config({"other": {"type": "http", "url": server.url}})
        self.corpus(prd=[prd_finding("docs/prds/a.md")])

        result = self.run_sweep_with("--mcp-config", str(config))

        self.assertEqual(result.returncode, 125, result.stderr)
        self.assertIn(str(config), result.stderr)
        self.assertEqual(server.requests, [])
        self.assertEqual(self.detector.invocations(), [])
        self.assert_state(None)


if __name__ == "__main__":
    unittest.main()
