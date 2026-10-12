#!/usr/bin/env python3
"""verify-ledger.py — the verify instrumentation ledger.

Owns everything scripts/verify.sh, tests/infra/run_all.sh and
scripts/seed-warm-lane.sh record about a verify run: the phase taxonomy of
plan lines, the per-run journal and the record composed from it, the sccache
and concurrency snapshots taken at cargo step boundaries, the nextest JUnit
build/execution split, the lane seed stamp, and the append-only JSONL ledger.
Callers only write numeric timing events and invoke these subcommands; they
never interpret a command line themselves. Schema, activation rules and
consumer rules: docs/notes/verify-instrumentation-ledger.md.
"""

import argparse
import enum
import json
import re
import sys


class Phase(enum.StrEnum):
    TEST_SLOT_ACQUIRE = "test_slot_acquire"
    TEST_SLOT_RELEASE = "test_slot_release"
    RELEASE_SKIP_MARKER = "release_skip_marker"
    INFRA_POOL = "infra_pool"
    INFRA_SELECTIVE = "infra_selective"
    INFRA_STATIC = "infra_static"
    GUI_FEATURE_NEXTEST = "gui_feature_nextest"
    RELEASE_NEXTEST = "release_nextest"
    DEBUG_NEXTEST = "debug_nextest"
    GUI_FEATURE_CARGO_TEST = "gui_feature_cargo_test"
    RELEASE_CARGO_TEST = "release_cargo_test"
    DEBUG_CARGO_TEST = "debug_cargo_test"
    CLIPPY = "clippy"
    GUI_FEATURE_CHECK = "gui_feature_check"
    CARGO_CHECK = "cargo_check"
    RELEASE_PREBUILD = "release_prebuild"
    CARGO_OTHER = "cargo_other"
    PSI_GATE = "psi_gate"
    COMPILE_GATE = "compile_gate"
    GUI_NODE_LAUNCH = "gui_node_launch"
    GUI_NODE_JOIN = "gui_node_join"
    GUI_NODE = "gui_node"
    POSTFLIGHT = "postflight"
    BOOKKEEPING = "bookkeeping"
    PREFLIGHT = "preflight"
    OTHER = "other"


NEXTEST_PHASES = frozenset(
    {Phase.DEBUG_NEXTEST, Phase.RELEASE_NEXTEST, Phase.GUI_FEATURE_NEXTEST}
)

CARGO_PHASES = NEXTEST_PHASES | frozenset(
    {
        Phase.DEBUG_CARGO_TEST,
        Phase.RELEASE_CARGO_TEST,
        Phase.GUI_FEATURE_CARGO_TEST,
        Phase.CLIPPY,
        Phase.GUI_FEATURE_CHECK,
        Phase.CARGO_CHECK,
        Phase.RELEASE_PREBUILD,
        Phase.CARGO_OTHER,
    }
)

RELEASE_TEST_PHASES = frozenset({Phase.RELEASE_NEXTEST, Phase.RELEASE_CARGO_TEST})


def _equals(token):
    return lambda line: line == token


def _contains(*needles):
    return lambda line: all(needle in line for needle in needles)


def _matches(pattern):
    compiled = re.compile(pattern)
    return lambda line: compiled.search(line) is not None


def _all_of(*predicates):
    return lambda line: all(predicate(line) for predicate in predicates)


def _any_of(*predicates):
    return lambda line: any(predicate(line) for predicate in predicates)


_RELEASE_FLAG = _matches(r"(^|\s)--release(\s|$)")
_GUI_FEATURE = _contains("--features gui")
_NEXTEST = _contains("cargo nextest run")
_CARGO_TEST = _matches(r"\bcargo test(\s|$)")

# Ordered (phase, predicate) rules over one plan line; the first match wins,
# so the most specific shape comes first.
PHASE_RULES = (
    (Phase.TEST_SLOT_ACQUIRE, _equals("@@SEMAPHORE_ACQUIRE@@")),
    (Phase.TEST_SLOT_RELEASE, _equals("@@SEMAPHORE_RELEASE@@")),
    (Phase.RELEASE_SKIP_MARKER, _contains("RELEASE-PASS: skipped (delta-clean)")),
    (Phase.INFRA_POOL, _contains("bash tests/infra/run_all.sh")),
    (Phase.INFRA_SELECTIVE, _contains("for _vt in tests/infra/")),
    (Phase.GUI_FEATURE_NEXTEST, _all_of(_NEXTEST, _GUI_FEATURE)),
    (Phase.RELEASE_NEXTEST, _all_of(_NEXTEST, _RELEASE_FLAG)),
    (Phase.DEBUG_NEXTEST, _NEXTEST),
    (Phase.GUI_FEATURE_CARGO_TEST, _all_of(_CARGO_TEST, _GUI_FEATURE)),
    (Phase.RELEASE_CARGO_TEST, _all_of(_CARGO_TEST, _RELEASE_FLAG)),
    (Phase.DEBUG_CARGO_TEST, _CARGO_TEST),
    (Phase.CLIPPY, _contains("cargo clippy")),
    (Phase.GUI_FEATURE_CHECK, _contains("cargo check -p reify-gui --features gui")),
    (Phase.CARGO_CHECK, _matches(r"\bcargo check(\s|$)")),
    (Phase.RELEASE_PREBUILD, _matches(r"\bcargo build --release(\s|$)")),
    (Phase.CARGO_OTHER, _matches(r"(^|[\s;&|(])cargo\s+[a-z]")),
    (Phase.PSI_GATE, _contains("scripts/verify.sh psi-gate")),
    (Phase.COMPILE_GATE, _contains("scripts/verify.sh compile-gate")),
    (Phase.GUI_NODE_LAUNCH, _contains("_VERIFY_NODE_BG_PID=$!")),
    (Phase.GUI_NODE_JOIN, _contains('wait "$_VERIFY_NODE_BG_PID"')),
    (Phase.GUI_NODE, _contains("npm ci")),
    (Phase.INFRA_STATIC, _matches(r"\bbash (tests|scripts)/[\w./-]+\.sh")),
    (Phase.POSTFLIGHT, _contains("./scripts/tree-sitter-freshness.sh check")),
    (
        Phase.BOOKKEEPING,
        _any_of(
            _contains("target/.reify-bin-sha"),
            _contains("reify-verify-attempt.json"),
            _contains("reify-audit binary missing"),
        ),
    ),
    (Phase.PREFLIGHT, _matches(r"\./scripts/[\w.-]+\.sh")),
)


def phase_of(line):
    for phase, predicate in PHASE_RULES:
        if predicate(line):
            return phase
    return Phase.OTHER


def classify_plan(lines):
    entries = []
    for index, line in enumerate(lines):
        phase = phase_of(line)
        entries.append(
            {
                "index": index,
                "phase": phase.value,
                "cargo": phase in CARGO_PHASES,
                "nextest": phase in NEXTEST_PHASES,
            }
        )
    return entries


def split_nul(text):
    parts = text.split("\0")
    if parts and parts[-1] == "":
        parts.pop()
    return parts


def cmd_classify(_args):
    lines = split_nul(sys.stdin.read())
    sys.stdout.write(json.dumps(classify_plan(lines)) + "\n")


def build_parser():
    parser = argparse.ArgumentParser(prog="verify-ledger.py", description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)

    classify = sub.add_parser("classify", help="NUL-separated plan lines on stdin -> JSON phases")
    classify.set_defaults(func=cmd_classify)
    return parser


def main(argv=None):
    args = build_parser().parse_args(argv)
    args.func(args)
    return 0


if __name__ == "__main__":
    sys.exit(main())
