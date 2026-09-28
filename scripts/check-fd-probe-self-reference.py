#!/usr/bin/env python3
"""
check-fd-probe-self-reference.py — flag /proc probes of the current process's
fd 1 written inside an output-capturing construct.

Inside `$(...)`, backticks or `<(...)`, fd 1 IS the pipe bash uses to capture
the construct's output, so `readlink /proc/self/fd/1` (or an alias such as
`/dev/fd/1`, or `/proc/$BASHPID/fd/1` spelled inside the construct) reads back
`pipe:*` whatever the real fd 1 is.
A line is flagged when one of those self-fd-1 spellings sits inside such a
span; whole-line comments and lines carrying `fdprobe:allow` are exempt.

With no PATH, scans the tracked shell corpus under --root (default: this
repo), `git ls-files -- '*.sh' 'hooks/*'`, reporting root-relative paths.

Exit contract: 0 clean, 1 findings, 2 usage, git or I/O error.

--json prints {"scanned": [path, ...],
               "findings": [{"path": str, "line": int, "source": str}, ...]}.

Rationale, correct idiom and scope: tests/infra/README.md
"Self-referential fd-probe guard".
"""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

SELF_FD1 = re.compile(
    r"(?:/proc/(?:self|thread-self|\$BASHPID|\$\{BASHPID\})|/dev)/fd/1(?![0-9])"
)

ALLOW_TOKEN = "fdprobe:allow"

SHELL_CORPUS_PATHSPEC = ("*.sh", "hooks/*")

DEFAULT_ROOT = Path(__file__).resolve().parents[1]

FINDING_MESSAGE = (
    "self-referential fd-1 probe inside a command substitution reads the "
    "capture pipe; capture $BASHPID into a variable OUTSIDE the substitution, "
    "then probe /proc/$pid/fd/1 "
    '(tests/infra/README.md "Self-referential fd-probe guard")'
)


class CheckerError(Exception):
    pass


def paren_span(line, start):
    depth = 1
    for i in range(start, len(line)):
        if line[i] == "(":
            depth += 1
        elif line[i] == ")":
            depth -= 1
            if depth == 0:
                return line[start:i]
    return line[start:]


def substitution_spans(line):
    spans = []
    for i in range(len(line) - 1):
        opener = line[i:i + 2]
        is_command_sub = opener == "$(" and line[i + 2:i + 3] != "("
        if is_command_sub or opener == "<(":
            spans.append(paren_span(line, i + 2))
    spans.extend(line.split("`")[1::2])
    return spans


def line_is_exempt(line):
    return line.lstrip().startswith("#") or ALLOW_TOKEN in line


def scan_text(text):
    findings = []
    for lineno, line in enumerate(text.split("\n"), start=1):
        if line_is_exempt(line):
            continue
        if any(SELF_FD1.search(span) for span in substitution_spans(line)):
            findings.append((lineno, line))
    return findings


def tracked_shell_files(root):
    try:
        proc = subprocess.run(
            ["git", "-C", str(root), "ls-files", "-z", "--",
             *SHELL_CORPUS_PATHSPEC],
            capture_output=True,
        )
    except OSError as err:
        raise CheckerError(f"git: {err.strerror or err}") from err
    if proc.returncode != 0:
        stderr = proc.stderr.decode(errors="replace").strip()
        raise CheckerError(f"{root}: git ls-files failed: {stderr}")
    listing = proc.stdout.decode(errors="surrogateescape")
    return [rel for rel in listing.split("\0") if rel]


def scan_paths(paths, base=Path(".")):
    scanned, findings = [], []
    for path in paths:
        try:
            with open(base / path, encoding="utf-8", errors="replace") as handle:
                text = handle.read()
        except OSError as err:
            raise CheckerError(f"{path}: {err.strerror or err}") from err
        scanned.append(path)
        findings.extend(
            {"path": path, "line": lineno, "source": source}
            for lineno, source in scan_text(text)
        )
    return scanned, findings


def parse_args(argv):
    parser = argparse.ArgumentParser(
        description="Flag /proc/self/fd/1-style probes inside $(...), "
        "backticks or <(...), where fd 1 is the capture pipe."
    )
    parser.add_argument("paths", nargs="*", metavar="PATH",
                        help="shell files to scan (default: the tracked "
                        "shell corpus under --root)")
    parser.add_argument("--json", action="store_true",
                        help="print {scanned, findings} as JSON")
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT,
                        help="repo whose tracked shell corpus is scanned "
                        "when no PATH is given (default: this repo)")
    return parser.parse_args(argv)


def report(scanned, findings, as_json):
    if as_json:
        print(json.dumps({"scanned": scanned, "findings": findings}, indent=2))
        return
    for finding in findings:
        print(f"{finding['path']}:{finding['line']}: {FINDING_MESSAGE}")


def main(argv=None):
    args = parse_args(argv)
    try:
        if args.paths:
            scanned, findings = scan_paths(args.paths)
        else:
            scanned, findings = scan_paths(tracked_shell_files(args.root),
                                           base=args.root)
    except CheckerError as err:
        print(err, file=sys.stderr)
        return 2
    report(scanned, findings, args.json)
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main())
