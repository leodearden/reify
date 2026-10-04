#!/usr/bin/env python3
"""docs-truth-sweep.py — run every docs-truth detector over a checkout and book
ONE human adjudication sitting at L2 for the findings.

Usage:
  docs-truth-sweep.py --reify-audit BIN --project-root DIR
                      (--escalation-url URL | --mcp-config PATH)
                      --state-file PATH [--dry-run]

Each FAMILY member runs as its own offline single-token reify-audit sweep of
DIR. A non-empty aggregate is raised as one escalate_info (the machine-readable
finding list) promoted to L2 in the same MCP session (the sitting's per-doc
judgement options).

Flags:
  --reify-audit BIN     the reify-audit binary (the deployed entrypoint
                        scripts/docs-truth-sweep.sh guards its freshness).
  --project-root DIR    the checkout to sweep.
  --escalation-url URL  the escalation MCP endpoint, or
  --mcp-config PATH     a .mcp.json declaring mcpServers.escalation.url.
                        One is required, with no default, so no caller can
                        file into the live queue by omission.
  --state-file PATH     where the last observed finding set is kept.
  --dry-run             print the planned escalate_info and promote_to_l2
                        arguments instead of filing them.

Output: a raise prints one JSON object on stdout, {member_id, l2_id,
l2_status, finding_count, doc_count, new_count}, and a human line on stderr.
A silent run prints nothing on stdout.

Exit codes:
  0    every member was checked: raised, or nothing to raise
  1    the sitting could not be filed
  125  a member's run had no parseable findings array, so nothing was raised

Rationale (family membership, cadence, identity, seam): docs/notes/docs-truth-sweep.md.
"""

import argparse
import json
import os
import subprocess
import sys
from dataclasses import dataclass

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from escalation_mcp import McpSession, escalation_url_from_mcp_config  # noqa: E402  (after sys.path manipulation)
from reify_audit_findings import detector_preamble, parse_findings, run_pattern  # noqa: E402

AGENT_ROLE = "docs-truth-sweep"
LOG_PREFIX = "docs-truth-sweep:"
SUBJECT = "audit"
CATEGORY = "risk_identified"
ROOT_CAUSE = "docs-truth sweep: documents awaiting a human docs-truth adjudication sitting"
DRY_RUN_MEMBER_ID = "<escalate_info id>"
EXIT_FILE_FAILED = 1
EXIT_UNCHECKED = 125


@dataclass(frozen=True)
class FamilyMember:
    """One docs-truth detector: its reify-audit token, the sitting a human runs
    to triage it, and the per-doc rulings that sitting chooses between."""

    token: str
    sitting: str
    judgements: tuple[str, ...]


FAMILY = (
    FamilyMember(
        token="PPRDSTATUS",
        sitting="/audit --pattern PPRDSTATUS",
        judgements=(
            "still-active PRD: correct its prose",
            "completed plan: stamp a terminal Status (SHIPPED / SUPERSEDED naming the "
            "successor / WITHDRAWN) per .claude/skills/prd/project.md \"PRD terminal status\"",
            "dated snapshot or capability manifest: leave the body as authored; never "
            "retroactively edit it",
        ),
    ),
    FamilyMember(
        token="PCITE",
        sitting="/audit --pattern PCITE",
        judgements=(
            "rotted manifest cite: correct the row so its grep evidence names a symbol "
            "that exists",
            "legitimately external cite: mark it `<!-- pcite:allow — <reason> -->`",
        ),
    ),
)


@dataclass(frozen=True)
class DocFinding:
    """One finding, reduced to the structured fields the sweep reads."""

    pattern: str
    path: str
    summary: str

    @classmethod
    def from_detector(cls, finding):
        return cls(pattern=finding["pattern"], path=finding["task_id"], summary=finding["summary"])


@dataclass(frozen=True)
class MemberRun:
    """A member's checked run: the findings its single-token sweep reported."""

    member: FamilyMember
    findings: tuple[DocFinding, ...]


class MemberUnchecked(Exception):
    """A member's run printed no parseable findings array."""

    def __init__(self, member, returncode):
        super().__init__(
            f"reify-audit --pattern {member.token} exited {returncode} with no parseable "
            "findings array; nothing was raised"
        )


def sweep_family(binary, project_root):
    """Every member's checked run; MemberUnchecked on the first unchecked one."""
    runs = []
    for member in FAMILY:
        stderr, returncode = run_pattern(binary, member.token, project_root)
        sys.stderr.write(detector_preamble(stderr))
        findings = parse_findings(stderr)
        if findings is None:
            raise MemberUnchecked(member, returncode)
        runs.append(MemberRun(member, tuple(DocFinding.from_detector(f) for f in findings)))
    return tuple(runs)


def all_findings(runs):
    return [finding for run in runs for finding in run.findings]


def flagged_members(runs):
    return [run.member for run in runs if run.findings]


def doc_count(findings):
    return len({finding.path for finding in findings})


def tally(runs, new_count):
    findings = all_findings(runs)
    return f"{len(findings)} finding(s) across {doc_count(findings)} doc(s), {new_count} new"


def sittings(runs):
    return "; ".join(member.sitting for member in flagged_members(runs))


def escalation_arguments(runs, new_count, head):
    """The L0 member: the full machine-readable finding list, under subject "audit"."""
    findings = all_findings(runs)
    rulings = " ".join(
        f"{member.sitting}: {'; '.join(member.judgements)}." for member in flagged_members(runs)
    )
    return {
        "task_id": SUBJECT,
        "agent_role": AGENT_ROLE,
        "category": CATEGORY,
        "severity": "info",
        "summary": f"[docs-truth] {tally(runs, new_count)} — book one adjudication sitting: {sittings(runs)}",
        "detail": json.dumps(
            [{"pattern": f.pattern, "path": f.path, "summary": f.summary} for f in findings]
        ),
        "suggested_action": (
            "Book ONE docs-truth adjudication sitting and rule on each listed doc on its own. "
            f"{rulings} Do NOT auto-file fix tasks from these findings."
        ),
        "evidence": [
            {
                "observation": f"docs-truth sweep: {tally(runs, new_count)}",
                "measured_at": f"HEAD={head}",
                "ref": AGENT_ROLE,
            }
        ],
        "terminal_state_is_the_bug": True,
    }


def promotion_arguments(runs, member_id, new_count):
    """The L2 decision point: one sitting, with each flagged member's rulings as options."""
    findings = all_findings(runs)
    documents = sorted({(finding.path, finding.pattern) for finding in findings})
    return {
        "task_id": SUBJECT,
        "agent_role": AGENT_ROLE,
        "category": CATEGORY,
        "member_ids": [member_id],
        "root_cause": ROOT_CAUSE,
        "options": [judgement for member in flagged_members(runs) for judgement in member.judgements],
        "evidence": "\n".join(f"{path} ({pattern})" for path, pattern in documents),
        "summary": (
            f"Docs-truth adjudication sitting: {tally(runs, new_count)}. "
            f"Run {sittings(runs)} and rule on each doc."
        ),
    }


def raise_sitting(url, runs, new_count, head):
    """escalate_info then promote_to_l2 in ONE session; both server records."""
    session = McpSession(url, AGENT_ROLE)
    session.open()
    member = session.call_tool("escalate_info", escalation_arguments(runs, new_count, head))
    promotion = session.call_tool("promote_to_l2", promotion_arguments(runs, member["id"], new_count))
    return member, promotion


def head_sha(project_root):
    completed = subprocess.run(
        ["git", "-C", project_root, "rev-parse", "HEAD"], capture_output=True, text=True
    )
    return completed.stdout.strip() if completed.returncode == 0 else "unknown"


def parse_args(argv):
    parser = argparse.ArgumentParser(
        description="Sweep every docs-truth detector and book one human adjudication sitting at L2."
    )
    parser.add_argument("--reify-audit", required=True, metavar="BIN", help="the reify-audit binary")
    parser.add_argument("--project-root", required=True, metavar="DIR", help="the checkout to sweep")
    endpoint = parser.add_mutually_exclusive_group(required=True)
    endpoint.add_argument("--escalation-url", metavar="URL", help="the escalation MCP endpoint")
    endpoint.add_argument(
        "--mcp-config", metavar="PATH", help="a .mcp.json declaring mcpServers.escalation.url"
    )
    parser.add_argument(
        "--state-file", required=True, metavar="PATH", help="the last observed finding set"
    )
    parser.add_argument(
        "--dry-run", action="store_true", help="print the planned arguments instead of filing"
    )
    return parser.parse_args(argv)


def main(argv=None):
    args = parse_args(argv)
    project_root = os.path.abspath(args.project_root)
    url = args.escalation_url or escalation_url_from_mcp_config(args.mcp_config)
    try:
        runs = sweep_family(args.reify_audit, project_root)
    except MemberUnchecked as unchecked:
        print(f"{LOG_PREFIX} {unchecked}", file=sys.stderr)
        return EXIT_UNCHECKED
    findings = all_findings(runs)
    if not findings:
        print(f"{LOG_PREFIX} no docs-truth findings; nothing to raise", file=sys.stderr)
        return 0
    new_count = len(findings)
    head = head_sha(project_root)
    if args.dry_run:
        planned = {
            "escalate_info": escalation_arguments(runs, new_count, head),
            "promote_to_l2": promotion_arguments(runs, DRY_RUN_MEMBER_ID, new_count),
        }
        print(json.dumps(planned, indent=2))
        return 0
    member, promotion = raise_sitting(url, runs, new_count, head)
    record = {
        "member_id": member.get("id"),
        "l2_id": promotion.get("id"),
        "l2_status": promotion.get("status"),
        "finding_count": len(findings),
        "doc_count": doc_count(findings),
        "new_count": new_count,
    }
    print(json.dumps(record))
    print(
        f"{LOG_PREFIX} raised {record['member_id']} -> L2 {record['l2_id']} "
        f"({record['l2_status']}) for {tally(runs, new_count)}",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
