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
import fcntl
import json
import os
import re
import subprocess
import sys
from pathlib import Path

RECORD_SCHEMA = "reify.verify-run/1"
LEDGER_FILE = "verify-runs.jsonl"


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


# ---------------------------------------------------------------------------
# Journal: <journal>/plan.nul (verify.sh), meta.json (run-begin),
# journal.jsonl (step events from verify.sh, observations from boundary),
# step-<i>.run-all.jsonl (run_all.sh), errors.log (every instrument failure).
# ---------------------------------------------------------------------------


def journal_path(journal, name):
    return Path(journal) / name


def read_plan(journal):
    return split_nul(journal_path(journal, "plan.nul").read_text(encoding="utf-8"))


def read_meta(journal):
    return json.loads(journal_path(journal, "meta.json").read_text(encoding="utf-8"))


def load_journal(journal):
    """Events in write order; a malformed line (a writer killed mid-line) is skipped."""
    path = journal_path(journal, "journal.jsonl")
    if not path.exists():
        return []
    events = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if isinstance(event, dict):
            events.append(event)
    return events


def read_instrument_errors(journal):
    path = journal_path(journal, "errors.log")
    if not path.exists():
        return []
    text = path.read_text(encoding="utf-8", errors="replace")
    return [line for line in text.splitlines() if line.strip()]


def read_run_all(journal, index):
    path = journal_path(journal, f"step-{index}.run-all.jsonl")
    if not path.exists():
        return None
    summary, members = None, []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        try:
            row = json.loads(line)
        except ValueError:
            continue
        if not isinstance(row, dict):
            continue
        if row.get("kind") == "summary" and summary is None:
            summary = row
        elif row.get("kind") == "member":
            members.append(row)
    return {"summary": summary, "members": members}


# ---------------------------------------------------------------------------
# Record composition
# ---------------------------------------------------------------------------


def new_step(entry, cmd):
    return {
        "index": entry["index"],
        "phase": entry["phase"],
        "cmd": cmd,
        "began_at": None,
        "ended_at": None,
        "wall_secs": None,
        "rc": None,
    }


def fold_steps(meta, events, journal):
    steps = [new_step(entry, entry["cmd"]) for entry in meta["plan"]]
    for event in events:
        index = event.get("index")
        if not isinstance(index, int) or not 0 <= index < len(steps):
            continue
        step = steps[index]
        kind = event.get("ev")
        if kind == "step_begin":
            step["began_at"] = event.get("at")
        elif kind == "step_end":
            step["ended_at"] = event.get("at")
            step["rc"] = event.get("rc")
            if step["began_at"] is not None and step["ended_at"] is not None:
                step["wall_secs"] = round(step["ended_at"] - step["began_at"], 6)
    for step in steps:
        run_all = read_run_all(journal, step["index"])
        if run_all is not None:
            step["run_all"] = run_all
    return steps


def release_pass_of(meta):
    phases = {entry["phase"] for entry in meta["plan"]}
    if Phase.RELEASE_SKIP_MARKER in phases:
        decision = "skipped-delta-clean"
    elif phases & RELEASE_TEST_PHASES:
        decision = "planned"
    else:
        decision = "absent"
    return {"decision": decision, "release_delta_skip_raw": meta["release_delta_skip_raw"]}


_GIT_REPO_ENV = (
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    "GIT_PREFIX",
)


def git_value(repo_root, *args):
    env = {k: v for k, v in os.environ.items() if k not in _GIT_REPO_ENV}
    proc = subprocess.run(
        ["git", "-C", repo_root, *args],
        env=env,
        capture_output=True,
        text=True,
        timeout=30,
    )
    return proc.stdout.strip() if proc.returncode == 0 else None


def git_identity(repo_root):
    return {
        "branch": git_value(repo_root, "rev-parse", "--abbrev-ref", "HEAD"),
        "head": git_value(repo_root, "rev-parse", "HEAD"),
        "tree_oid": git_value(repo_root, "rev-parse", "HEAD:"),
    }


def compose_record(meta, journal, exit_code, ended_at, signal):
    record = {
        "schema": RECORD_SCHEMA,
        "run_id": meta["run_id"],
        "role": meta["role"],
        "role_raw": meta["role_raw"],
        "action": meta["action"],
        "profile": meta["profile"],
        "scope": meta["scope"],
        "lane": meta["lane_dir"],
    }
    record.update(git_identity(meta["repo_root"]))
    record.update(
        {
            "started_at": meta["started_at"],
            "ended_at": ended_at,
            "wall_secs": round(ended_at - meta["started_at"], 6),
            "exit_code": exit_code,
            "verdict": "pass" if exit_code == 0 else "fail",
            "signal": signal or None,
            "release_pass": release_pass_of(meta),
            "steps": fold_steps(meta, load_journal(journal), journal),
        }
    )
    record["instrument_errors"] = read_instrument_errors(journal)
    return record


def append_record(ledger_dir, record):
    """One O_APPEND write per line, serialised across writers by flock."""
    os.makedirs(ledger_dir, exist_ok=True)
    path = os.path.join(ledger_dir, LEDGER_FILE)
    data = (json.dumps(record, separators=(",", ":")) + "\n").encode("utf-8")
    lock_fd = os.open(path + ".lock", os.O_CREAT | os.O_WRONLY, 0o644)
    try:
        fcntl.flock(lock_fd, fcntl.LOCK_EX)
        fd = os.open(path, os.O_WRONLY | os.O_APPEND | os.O_CREAT, 0o644)
        try:
            view = memoryview(data)
            while view:
                written = os.write(fd, view)
                view = view[written:]
        finally:
            os.close(fd)
    finally:
        os.close(lock_fd)


def claim_finalize(journal):
    try:
        fd = os.open(journal_path(journal, "finalized"), os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o644)
    except FileExistsError:
        return False
    os.close(fd)
    return True


# ---------------------------------------------------------------------------
# Subcommands
# ---------------------------------------------------------------------------


def cmd_classify(_args):
    lines = split_nul(sys.stdin.read())
    sys.stdout.write(json.dumps(classify_plan(lines)) + "\n")


def cmd_run_begin(args):
    lines = read_plan(args.journal)
    plan = classify_plan(lines)
    for entry, cmd in zip(plan, lines):
        entry["cmd"] = cmd
    meta = {
        "run_id": args.run_id,
        "role": args.role,
        "role_raw": args.role_raw or None,
        "action": args.action,
        "profile": args.profile,
        "scope": args.scope,
        "repo_root": args.repo_root,
        "lane_dir": args.lane_dir,
        "release_delta_skip_raw": args.release_delta_skip,
        "started_at": args.started_at,
        "plan": plan,
    }
    journal_path(args.journal, "meta.json").write_text(json.dumps(meta), encoding="utf-8")
    sys.stdout.write("".join(f"{entry['index']}\n" for entry in plan if entry["cargo"]))


def cmd_finalize(args):
    if not claim_finalize(args.journal):
        return
    meta = read_meta(args.journal)
    record = compose_record(meta, args.journal, args.exit_code, args.ended_at, args.signal)
    append_record(args.ledger_dir, record)


def build_parser():
    parser = argparse.ArgumentParser(prog="verify-ledger.py", description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)

    classify = sub.add_parser("classify", help="NUL-separated plan lines on stdin -> JSON phases")
    classify.set_defaults(func=cmd_classify)

    begin = sub.add_parser("run-begin", help="classify <journal>/plan.nul, write meta.json, print cargo step indices")
    begin.add_argument("--journal", required=True)
    begin.add_argument("--run-id", required=True)
    begin.add_argument("--role", required=True)
    begin.add_argument("--role-raw", default="")
    begin.add_argument("--action", required=True)
    begin.add_argument("--profile", required=True)
    begin.add_argument("--scope", required=True)
    begin.add_argument("--repo-root", required=True)
    begin.add_argument("--lane-dir", required=True)
    begin.add_argument("--release-delta-skip", default="")
    begin.add_argument("--started-at", type=float, required=True)
    begin.set_defaults(func=cmd_run_begin)

    finalize = sub.add_parser("finalize", help="compose the run record and append it to the ledger (once)")
    finalize.add_argument("--journal", required=True)
    finalize.add_argument("--ledger-dir", required=True)
    finalize.add_argument("--exit-code", type=int, required=True)
    finalize.add_argument("--ended-at", type=float, required=True)
    finalize.add_argument("--signal", default="")
    finalize.set_defaults(func=cmd_finalize)
    return parser


def record_instrument_error(args, exc):
    line = f"{args.command}: {type(exc).__name__}: {exc}"
    journal = getattr(args, "journal", None)
    try:
        if journal and os.path.isdir(journal):
            with open(journal_path(journal, "errors.log"), "a", encoding="utf-8") as fh:
                fh.write(line + "\n")
            return
    except OSError:
        pass
    sys.stderr.write(f"verify-ledger.py: {line}\n")


def main(argv=None):
    args = build_parser().parse_args(argv)
    try:
        args.func(args)
    except Exception as exc:  # fail-open: an instrument never changes a verify outcome
        record_instrument_error(args, exc)
    return 0


if __name__ == "__main__":
    sys.exit(main())
