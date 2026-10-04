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
  --state-file PATH     the last OBSERVED finding identity set. A missing
                        file is a first run; delete it to re-raise every
                        current finding.
  --dry-run             print the planned escalate_info and promote_to_l2
                        arguments instead of filing them; no network contact
                        and no state write.

Only a set holding a (detector, doc) identity absent from the last observed
set is raised; every fully-checked run records what it observed.

Output: a raise prints one JSON object on stdout, {member_id, l2_id,
l2_status, finding_count, doc_count, new_count}, and a human line on stderr.
A silent run prints nothing on stdout.

Exit codes:
  0    every member was checked, and the set was raised, correctly silent, or
       printed under --dry-run
  1    a raise was due but escalate_info or promote_to_l2 could not be filed
       (a reply naming no record id, or an accepted_unpersisted one, is not a
       filing), or the observed set could not be recorded (a booked sitting is
       still reported first); a failed raise leaves the state untouched, so
       the next run retries
  125  nothing was checked or raised: a member's run had no parseable findings
       array (detector failure, or a refusal of an empty task corpus), the
       state file is unreadable, or --mcp-config declares no endpoint

Rationale (family membership, cadence, identity, seam): docs/notes/docs-truth-sweep.md.
"""

import argparse
import http.client
import json
import os
import subprocess
import sys
import tempfile
from dataclasses import dataclass

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from escalation_mcp import (  # noqa: E402  (after sys.path manipulation)
    EscalationError,
    McpConfigError,
    McpSession,
    escalation_url_from_mcp_config,
)
from reify_audit_findings import detector_preamble, parse_findings, run_pattern  # noqa: E402

AGENT_ROLE = "docs-truth-sweep"
LOG_PREFIX = "docs-truth-sweep:"
SUBJECT = "audit"
CATEGORY = "risk_identified"
ROOT_CAUSE = "docs-truth sweep: documents awaiting a human docs-truth adjudication sitting"
DRY_RUN_MEMBER_ID = "<escalate_info id>"
STATE_VERSION = 1
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
            "dated snapshot: leave the body as authored; never retroactively edit it",
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


@dataclass(frozen=True, order=True)
class FindingIdentity:
    """What makes a finding the same finding from run to run: its detector and
    its doc. The ONLY silence key; never derived from summary text, which
    carries volatile line numbers and leaf counts."""

    pattern: str
    path: str


def identities(findings):
    return frozenset(FindingIdentity(finding.pattern, finding.path) for finding in findings)


def new_identities(current, seen):
    """The novelty rule: what this run observed that the last run did not."""
    return current - seen


class StateError(Exception):
    """The state file exists but does not hold a recorded observation."""


class StateWriteError(Exception):
    """The observed set could not be recorded."""


def load_seen(state_path):
    """The last observed identity set; empty when no run has recorded one yet."""
    try:
        with open(state_path, encoding="utf-8") as state_file:
            state = json.load(state_file)
    except FileNotFoundError:
        return frozenset()
    except (OSError, ValueError) as error:
        raise StateError(f"unreadable state file {state_path}: {error}") from error
    if not (
        isinstance(state, dict)
        and state.get("version") == STATE_VERSION
        and isinstance(state.get("seen"), list)
        and all(_is_identity_record(entry) for entry in state["seen"])
    ):
        raise StateError(
            f"state file {state_path} is not a version-{STATE_VERSION} observation "
            '{"version": 1, "seen": [{"pattern": str, "path": str}, ...]}; '
            "refusing rather than guessing (delete it to re-raise every current finding)"
        )
    return frozenset(FindingIdentity(entry["pattern"], entry["path"]) for entry in state["seen"])


def _is_identity_record(entry):
    return (
        isinstance(entry, dict)
        and isinstance(entry.get("pattern"), str)
        and isinstance(entry.get("path"), str)
    )


def save_seen(state_path, observed):
    """Record the observed identity set, atomically (sibling temp file + rename)."""
    state = {
        "version": STATE_VERSION,
        "seen": [{"pattern": i.pattern, "path": i.path} for i in sorted(observed)],
    }
    directory = os.path.dirname(os.path.abspath(state_path))
    try:
        os.makedirs(directory, exist_ok=True)
        descriptor, temporary = tempfile.mkstemp(dir=directory, prefix=".docs-truth-sweep.")
    except OSError as error:
        raise StateWriteError(f"could not record the observed set in {state_path}: {error}") from error
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as temporary_file:
            json.dump(state, temporary_file, indent=2)
            temporary_file.write("\n")
        os.replace(temporary, state_path)
    except OSError as error:
        os.unlink(temporary)
        raise StateWriteError(f"could not record the observed set in {state_path}: {error}") from error


@dataclass(frozen=True)
class MemberRun:
    """A member's checked run: the findings its single-token sweep reported."""

    member: FamilyMember
    findings: tuple[DocFinding, ...]


class MemberUnchecked(Exception):
    """A member's run printed no parseable findings array."""

    def __init__(self, member, outcome):
        super().__init__(f"reify-audit --pattern {member.token} {outcome}; nothing was raised")


def sweep_family(binary, project_root):
    """Every member's checked run; MemberUnchecked on the first unchecked one."""
    runs = []
    for member in FAMILY:
        try:
            stderr, returncode = run_pattern(binary, member.token, project_root)
        except OSError as error:
            raise MemberUnchecked(member, f"could not be run: {error}") from error
        sys.stderr.write(detector_preamble(stderr))
        findings = parse_findings(stderr)
        if findings is None:
            raise MemberUnchecked(member, f"exited {returncode} with no parseable findings array")
        runs.append(MemberRun(member, tuple(DocFinding.from_detector(f) for f in findings)))
    return tuple(runs)


def all_findings(runs):
    return [finding for run in runs for finding in run.findings]


def flagged_members(runs):
    return [run.member for run in runs if run.findings]


def count_docs(findings):
    return len({finding.path for finding in findings})


def tally(findings, new_count):
    return f"{len(findings)} finding(s) across {count_docs(findings)} doc(s), {new_count} new"


@dataclass(frozen=True)
class Sitting:
    """What one raise books: every member's checked run, how many identities
    are new since the last observation, and the HEAD it was measured at."""

    runs: tuple[MemberRun, ...]
    new_count: int
    head: str

    @property
    def findings(self):
        return all_findings(self.runs)

    @property
    def doc_count(self):
        return count_docs(self.findings)

    @property
    def tally(self):
        return tally(self.findings, self.new_count)

    @property
    def sittings(self):
        return "; ".join(member.sitting for member in flagged_members(self.runs))


def escalation_arguments(sitting):
    """The L0 member: the full machine-readable finding list, under subject "audit"."""
    rulings = " ".join(
        f"{member.sitting}: {'; '.join(member.judgements)}."
        for member in flagged_members(sitting.runs)
    )
    return {
        "task_id": SUBJECT,
        "agent_role": AGENT_ROLE,
        "category": CATEGORY,
        "severity": "info",
        "summary": f"[docs-truth] {sitting.tally} — book one adjudication sitting: {sitting.sittings}",
        "detail": json.dumps(
            [{"pattern": f.pattern, "path": f.path, "summary": f.summary} for f in sitting.findings]
        ),
        "suggested_action": (
            "Book ONE docs-truth adjudication sitting and rule on each listed doc on its own. "
            f"{rulings} Do NOT auto-file fix tasks from these findings."
        ),
        "evidence": [
            {
                "observation": f"docs-truth sweep: {sitting.tally}",
                "measured_at": f"HEAD={sitting.head}",
                "ref": AGENT_ROLE,
            }
        ],
        "terminal_state_is_the_bug": True,
    }


def promotion_arguments(sitting, member_id):
    """The L2 decision point: one sitting, with each flagged member's rulings as options."""
    documents = sorted({(finding.path, finding.pattern) for finding in sitting.findings})
    return {
        "task_id": SUBJECT,
        "agent_role": AGENT_ROLE,
        "category": CATEGORY,
        "member_ids": [member_id],
        "root_cause": ROOT_CAUSE,
        "options": [
            judgement for member in flagged_members(sitting.runs) for judgement in member.judgements
        ],
        "evidence": "\n".join(f"{path} ({pattern})" for path, pattern in documents),
        "summary": (
            f"Docs-truth adjudication sitting: {sitting.tally}. "
            f"Run {sitting.sittings} and rule on each doc."
        ),
    }


def planned_raise(sitting):
    return {
        "escalate_info": escalation_arguments(sitting),
        "promote_to_l2": promotion_arguments(sitting, DRY_RUN_MEMBER_ID),
    }


def filed_record(tool_name, reply):
    """reply, if it names a record the server confirmed storing; else EscalationError.

    An accepted_unpersisted reply is not a filing: promoting or recording an id
    the server could not confirm would book a sitting on a record that may not
    exist.
    """
    record_id = reply.get("id")
    if not isinstance(record_id, str) or not record_id:
        raise EscalationError(f"{tool_name} reply names no record id: {reply}")
    if reply.get("status") == "accepted_unpersisted":
        raise EscalationError(f"{tool_name} was accepted_unpersisted, not filed: {reply}")
    return reply


def raise_sitting(url, sitting):
    """escalate_info then promote_to_l2 in ONE session; both filed server records."""
    session = McpSession(url, AGENT_ROLE)
    session.open()
    member = filed_record(
        "escalate_info", session.call_tool("escalate_info", escalation_arguments(sitting))
    )
    promotion = filed_record(
        "promote_to_l2",
        session.call_tool("promote_to_l2", promotion_arguments(sitting, member["id"])),
    )
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


def endpoint_url(args):
    return args.escalation_url or escalation_url_from_mcp_config(args.mcp_config)


def stay_silent(args, runs, observed):
    """Nothing new: record the observation (so a shrink or an empty set counts)."""
    if not args.dry_run:
        save_seen(args.state_file, observed)
    print(f"{LOG_PREFIX} {tally(all_findings(runs), 0)}; nothing new to raise", file=sys.stderr)
    return 0


def raise_and_record(url, sitting, state_path, observed):
    """Book the sitting and report it, then record the observation; a failed
    raise records nothing. The report precedes the state write, so a booked
    sitting is never lost from the log to a write failure."""
    try:
        member, promotion = raise_sitting(url, sitting)
    except (EscalationError, OSError, ValueError, http.client.HTTPException) as error:
        print(
            f"{LOG_PREFIX} could not book the sitting at {url}: {error}; "
            "the state is untouched, so the next run retries",
            file=sys.stderr,
        )
        return EXIT_FILE_FAILED
    record = {
        "member_id": member["id"],
        "l2_id": promotion["id"],
        "l2_status": promotion.get("status"),
        "finding_count": len(sitting.findings),
        "doc_count": sitting.doc_count,
        "new_count": sitting.new_count,
    }
    print(json.dumps(record), flush=True)
    print(
        f"{LOG_PREFIX} raised {record['member_id']} -> L2 {record['l2_id']} "
        f"({record['l2_status']}) for {sitting.tally}",
        file=sys.stderr,
    )
    save_seen(state_path, observed)
    return 0


def sweep(args):
    project_root = os.path.abspath(args.project_root)
    seen = load_seen(args.state_file)
    url = endpoint_url(args)
    runs = sweep_family(args.reify_audit, project_root)
    observed = identities(all_findings(runs))
    new_count = len(new_identities(observed, seen))
    if not new_count:
        return stay_silent(args, runs, observed)
    sitting = Sitting(runs, new_count, head_sha(project_root))
    if args.dry_run:
        print(json.dumps(planned_raise(sitting), indent=2))
        return 0
    return raise_and_record(url, sitting, args.state_file, observed)


def main(argv=None):
    args = parse_args(argv)
    try:
        return sweep(args)
    except (StateError, McpConfigError, MemberUnchecked) as unchecked:
        print(f"{LOG_PREFIX} {unchecked}", file=sys.stderr)
        return EXIT_UNCHECKED
    except StateWriteError as error:
        print(f"{LOG_PREFIX} {error}", file=sys.stderr)
        return EXIT_FILE_FAILED


if __name__ == "__main__":
    sys.exit(main())
