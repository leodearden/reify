"""reify_audit_findings.py — reify-audit's JSON-on-stderr contract.

run_pattern runs one offline single-token detector sweep of a checkout;
parse_findings reads the findings array the binary prints last on stderr, and
detector_preamble is whatever it wrote ahead of that array. The contract is
.claude/skills/audit/references/cli-invocation.md §3.1.
"""

import json
import subprocess


def run_pattern(binary, token, project_root):
    """(stderr, returncode) of one offline `--pattern token` sweep of project_root.

    project_root is also the working directory, so the binary's cwd-relative
    defaults (the runs-db) resolve inside the project.
    """
    completed = subprocess.run(
        [binary, "--pattern", token, "--no-jcodemunch", "--project-root", project_root],
        cwd=project_root,
        capture_output=True,
        encoding="utf-8",
        errors="replace",
    )
    return completed.stderr, completed.returncode


def findings_array_start(stderr):
    """Offset of the findings array: the last line that opens with `[`."""
    newline_bracket = stderr.rfind("\n[")
    if newline_bracket >= 0:
        return newline_bracket + 1
    return 0 if stderr.startswith("[") else None


def parse_findings(stderr):
    """The findings list, or None when stderr carries no parseable array.

    The exit code is never read as failure once an array parses: it is the
    High count, and 125 High findings is a legal result.
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
