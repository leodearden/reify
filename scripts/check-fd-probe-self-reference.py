#!/usr/bin/env python3
"""
check-fd-probe-self-reference.py — flag /proc probes of the current process's
fd 1 written inside an output-capturing construct.

Inside `$(...)`, backticks or `<(...)`, fd 1 IS the pipe bash uses to capture
the construct's output, so `readlink /proc/self/fd/1` (or `/proc/$BASHPID/fd/1`
spelled inside the construct) reads back `pipe:*` whatever the real fd 1 is.
A line is flagged when one of those self-fd-1 spellings sits inside such a
span; whole-line comments and lines carrying `fdprobe:allow` are exempt.

Exit contract: 0 clean, 1 findings, 2 usage or I/O error.

--json prints {"scanned": [path, ...],
               "findings": [{"path": str, "line": int, "source": str}, ...]}.

Rationale, correct idiom and scope: tests/infra/README.md
"Self-referential fd-probe guard".
"""

import argparse
import json
import re
import sys

SELF_FD1 = re.compile(r"/proc/(?:self|\$BASHPID|\$\{BASHPID\})/fd/1(?![0-9])")

ALLOW_TOKEN = "fdprobe:allow"

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


def scan_paths(paths):
    scanned, findings = [], []
    for path in paths:
        try:
            with open(path, encoding="utf-8", errors="replace") as handle:
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
                        help="shell files to scan")
    parser.add_argument("--json", action="store_true",
                        help="print {scanned, findings} as JSON")
    args = parser.parse_args(argv)
    if not args.paths:
        parser.error("no PATH given: pass one or more shell files to scan")
    return args


def report(scanned, findings, as_json):
    if as_json:
        print(json.dumps({"scanned": scanned, "findings": findings}, indent=2))
        return
    for finding in findings:
        print(f"{finding['path']}:{finding['line']}: {FINDING_MESSAGE}")


def main(argv=None):
    args = parse_args(argv)
    try:
        scanned, findings = scan_paths(args.paths)
    except CheckerError as err:
        print(err, file=sys.stderr)
        return 2
    report(scanned, findings, args.json)
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main())
