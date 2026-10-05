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

This is the one-shot raise primitive. The recurring, set-deduplicated,
family-wide raise is scripts/docs-truth-sweep.py; its rationale is in
docs/notes/docs-truth-sweep.md.
"""

import argparse
import http.client
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from escalation_mcp import EscalationError, McpSession  # noqa: E402  (after sys.path manipulation)
from reify_audit_findings import detector_preamble, parse_findings, run_pattern  # noqa: E402

PATTERN = "PPRDSTATUS"
FINDING_PATTERN = "PPrdStatus"
SITTING = "/audit --pattern PPRDSTATUS"
ERROR_PREFIX = "pprdstatus-escalate:"
CLIENT_NAME = "pprdstatus-escalate"
EXIT_FILE_FAILED = 1
EXIT_NO_FINDINGS_ARRAY = 125

SUGGESTED_ACTION = (
    f"Book a docs-truth triage sitting ({SITTING}) and judge each listed doc on "
    "its own: a still-active PRD whose prose needs correcting; a completed plan "
    "that needs a terminal stamp per .claude/skills/prd/project.md \"PRD terminal "
    "status\"; or a dated snapshot that must not be edited. Do NOT auto-file fix "
    "tasks from these findings."
)


def captured_stderr(args):
    """The detector's stderr, and where it came from; OSError if unreadable."""
    if args.findings_file:
        with open(args.findings_file, encoding="utf-8", errors="replace") as captured:
            return captured.read(), f"findings file {args.findings_file}"
    stderr, returncode = run_pattern(args.reify_audit, PATTERN, os.path.abspath(args.project_root))
    return stderr, f"{args.reify_audit} exited {returncode}"


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


def file_escalation(url, arguments):
    """File escalate_info and return the server's record ({id, status, level})."""
    session = McpSession(url, CLIENT_NAME)
    session.open()
    return session.call_tool("escalate_info", arguments)


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
