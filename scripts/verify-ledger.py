#!/usr/bin/env python3
"""verify-ledger.py — the verify instrumentation ledger.

Owns everything scripts/verify.sh, tests/infra/run_all.sh and
scripts/seed-warm-lane.sh record about a verify run: the phase taxonomy of
plan lines, the per-run journal and the record composed from it, the sccache
and concurrency snapshots taken at cargo step boundaries, the nextest JUnit
build/execution split, the lane seed stamp, the infra-harness ancestry check
the default ledger applies, and the append-only JSONL ledger.
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
import secrets
import subprocess
import sys
import tempfile
import time
import xml.etree.ElementTree as ElementTree
from datetime import datetime
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from verify_events_lib import read_events  # noqa: E402  (imported after sys.path manipulation)

RECORD_SCHEMA = "reify.verify-run/1"
LEDGER_FILE = "verify-runs.jsonl"
SEED_STAMP_SCHEMA = "reify.lane-seed/1"
SEED_STAMP_SUFFIX = ".seed-stamp.json"
AGENT_START_LOG = ".reify-cargo-starts.jsonl"
FINGERPRINT_PROFILES = ("debug", "release")


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
# step-<i>.run-all.jsonl (run_all.sh's copy of its C2 event records, through
# lib_event_log.sh's EVENT_LOG_COPY_TO), errors.log (every instrument failure).
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
    return fold_run_all(read_events(path).records)


def ns_to_secs(ns):
    """Epoch nanoseconds as epoch seconds; anything but an int is unknown."""
    return round(ns / 1_000_000_000, 6) if type(ns) is int else None


def fold_run_all(records):
    """run_all.sh's C2 records as the step's run_all block: {summary, members}."""
    return {
        "summary": run_all_summary(records),
        "members": [run_all_member(r) for r in records if r.get("event") == "member_finish"],
    }


def run_all_summary(records):
    """The first run_start's identity plus each phase's window; None without a run_start."""
    run_start = next((r for r in records if r.get("event") == "run_start"), None)
    if run_start is None:
        return None
    phases = {}
    for r in records:
        phase = r.get("phase")
        if r.get("event") == "phase_finish" and isinstance(phase, str) and phase not in phases:
            phases[phase] = {
                "began_at": ns_to_secs(r.get("started_ns")),
                "ended_at": ns_to_secs(r.get("ended_ns")),
            }
    return {
        "run_id": run_start.get("run_id"),
        "branch": run_start.get("branch"),
        "discovered": run_start.get("discovered"),
        "phases": phases,
    }


def run_all_member(record):
    """One member_finish record as a member row, its times in epoch seconds."""
    return {
        "name": record.get("member"),
        "attempt": record.get("attempt"),
        "verdict": record.get("verdict"),
        "rc": record.get("rc"),
        "queued_at": ns_to_secs(record.get("queued_ns")),
        "started_at": ns_to_secs(record.get("started_ns")),
        "ended_at": ns_to_secs(record.get("ended_ns")),
    }


# ---------------------------------------------------------------------------
# Boundary observations (taken at both edges of every cargo-bearing step)
# ---------------------------------------------------------------------------

VERIFY_ACTIONS = frozenset({"test", "lint", "typecheck", "all"})


def sccache_snapshot():
    """The daemon's raw stats object, or why there is none. Never a computed rate."""
    wrapper = os.environ.get("RUSTC_WRAPPER", "")
    if os.path.basename(wrapper) != "sccache":
        return {"available": False, "reason": "rustc-wrapper-not-sccache"}
    try:
        proc = subprocess.run(
            [wrapper, "--show-stats", "--stats-format=json"],
            capture_output=True,
            text=True,
            timeout=15,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        return {"available": False, "reason": type(exc).__name__}
    if proc.returncode != 0:
        return {"available": False, "reason": f"exit-{proc.returncode}"}
    try:
        return json.loads(proc.stdout)
    except ValueError:
        return {"available": False, "reason": "unparseable-stats"}


def proc_root():
    return Path(os.environ.get("REIFY_VERIFY_LEDGER_PROC_ROOT") or "/proc")


def read_process(pid_dir):
    """(ppid, comm, argv) from <pid>/stat and <pid>/cmdline; None if it vanished."""
    try:
        stat_text = (pid_dir / "stat").read_text(encoding="utf-8", errors="replace")
        cmdline = (pid_dir / "cmdline").read_bytes()
    except OSError:
        return None
    open_paren, close_paren = stat_text.find("("), stat_text.rfind(")")
    fields = stat_text[close_paren + 2 :].split()
    if open_paren < 0 or close_paren < 0 or len(fields) < 2:
        return None
    argv = [arg.decode("utf-8", errors="replace") for arg in cmdline.split(b"\0")]
    if argv and argv[-1] == "":
        argv.pop()
    return int(fields[1]), stat_text[open_paren + 1 : close_paren], argv


def read_process_table(root):
    table = {}
    for name in os.listdir(root):
        if name.isdigit():
            process = read_process(root / name)
            if process is not None:
                table[int(name)] = process
    return table


def is_verify_run(argv):
    if "--print-plan" in argv:
        return False
    for position, arg in enumerate(argv[:-1]):
        if os.path.basename(arg) == "verify.sh" and argv[position + 1] in VERIFY_ACTIONS:
            return True
    return False


def ancestry(table, pid):
    """pid, its parent, ... up to the first pid outside the table (cycle-safe)."""
    chain, seen = [], set()
    while pid in table and pid not in seen:
        seen.add(pid)
        chain.append(pid)
        pid = table[pid][0]
    return chain


def outermost_verify(table, chain):
    roots = [pid for pid in chain if is_verify_run(table[pid][2])]
    return roots[-1] if roots else None


# reify's infra harness: a tests/infra/ suite or run_all.sh. A verify.sh run
# beneath one is test traffic, which verify.sh's default ledger refuses.
HARNESS_SCRIPT = re.compile(r"(^|/)tests/infra/(run_all\.sh|test_[^/]+\.(sh|py))$")


def process_cwd(pid_dir):
    try:
        return os.readlink(pid_dir / "cwd")
    except OSError:
        return ""


def is_harness(argv, cwd):
    return any(HARNESS_SCRIPT.search(os.path.normpath(os.path.join(cwd, arg))) for arg in argv if arg)


def harness_ancestor(self_pid):
    """argv of the nearest process at or above self_pid that is reify's infra harness, or None."""
    root = proc_root()
    pid, seen = self_pid, set()
    while pid not in seen:
        seen.add(pid)
        process = read_process(root / str(pid))
        if process is None:
            return None
        ppid, _comm, argv = process
        if is_harness(argv, process_cwd(root / str(pid))):
            return argv
        pid = ppid
    return None


def concurrency_snapshot(self_pid):
    root = proc_root()
    table = read_process_table(root)
    other_roots, cargo_in_other, cargo_outside, rustc_total = set(), 0, 0, 0
    for pid, (_ppid, comm, _argv) in table.items():
        if comm == "rustc":
            rustc_total += 1
        chain = ancestry(table, pid)
        if self_pid in chain:
            continue
        verify_root = outermost_verify(table, chain)
        if verify_root is not None:
            other_roots.add(verify_root)
        if comm == "cargo":
            if verify_root is None:
                cargo_outside += 1
            else:
                cargo_in_other += 1
    return {
        "other_verify_runs": len(other_roots),
        "cargo_in_other_verifies": cargo_in_other,
        "cargo_outside_verifies": cargo_outside,
        "rustc_total": rustc_total,
        "host": host_load(root),
    }


def first_float(path, prefix=""):
    try:
        text = path.read_text(encoding="utf-8")
    except OSError:
        return None
    for line in text.splitlines():
        if line.startswith(prefix):
            token = line[len(prefix):].split()[0]
            try:
                return float(token.split("=", 1)[-1])
            except ValueError:
                return None
    return None


def host_load(root):
    return {
        "loadavg_1": first_float(root / "loadavg"),
        "psi_cpu_some_avg10": first_float(root / "pressure" / "cpu", "some "),
    }


def junit_report(path):
    """Root <testsuites> attributes of a nextest JUnit report, plus its mtime."""
    try:
        mtime = os.stat(path).st_mtime
    except FileNotFoundError:
        return {"path": path, "mtime": None}
    report = {"path": path, "mtime": mtime}
    try:
        _event, element = next(ElementTree.iterparse(path, events=("start",)))
        attrs = element.attrib
        report["exec_started_at"] = datetime.fromisoformat(attrs["timestamp"]).timestamp()
        report["exec_secs"] = float(attrs["time"])
        for key in ("tests", "failures", "errors"):
            report[key] = int(attrs[key]) if key in attrs else None
    except (ElementTree.ParseError, KeyError, ValueError, StopIteration):
        pass
    return report


def append_event(journal, event):
    with open(journal_path(journal, "journal.jsonl"), "a", encoding="utf-8") as fh:
        fh.write(json.dumps(event, separators=(",", ":")) + "\n")


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


def apply_step_begin(step, event):
    step["began_at"] = event.get("at")


def apply_step_end(step, event):
    step["ended_at"] = event.get("at")
    step["rc"] = event.get("rc")
    if step["began_at"] is not None and step["ended_at"] is not None:
        step["wall_secs"] = round(step["ended_at"] - step["began_at"], 6)


def apply_snapshot(step, event):
    edge = event.get("edge")
    step.setdefault("sccache", {})[edge] = event.get("sccache")
    step.setdefault("concurrency", {})[edge] = event.get("concurrency")


def apply_junit(step, event):
    step["_junit"] = event


_EVENT_APPLIERS = {
    "step_begin": apply_step_begin,
    "step_end": apply_step_end,
    "snapshot": apply_snapshot,
    "junit": apply_junit,
}


def nextest_split(step, junit):
    """Split one nextest step at the JUnit run start: build + test listing, execution, teardown."""
    if junit.get("mtime") is None:
        return {"available": False, "reason": "no-junit"}
    if step["began_at"] is None or junit["mtime"] < step["began_at"]:
        return {"available": False, "reason": "stale-junit"}
    if junit.get("exec_started_at") is None or junit.get("exec_secs") is None:
        return {"available": False, "reason": "unparseable-junit"}
    exec_end = junit["exec_started_at"] + junit["exec_secs"]
    post = None if step["ended_at"] is None else round(step["ended_at"] - exec_end, 6)
    return {
        "exec_started_at": junit["exec_started_at"],
        "exec_secs": junit["exec_secs"],
        "pre_exec_secs": round(junit["exec_started_at"] - step["began_at"], 6),
        "post_exec_secs": post,
        "tests": junit.get("tests"),
        "failures": junit.get("failures"),
        "errors": junit.get("errors"),
    }


def fold_steps(meta, events, journal):
    steps = [new_step(entry, entry["cmd"]) for entry in meta["plan"]]
    for event in events:
        index = event.get("index")
        applier = _EVENT_APPLIERS.get(event.get("ev"))
        if applier and isinstance(index, int) and 0 <= index < len(steps):
            applier(steps[index], event)
    for step in steps:
        junit = step.pop("_junit", None)
        if junit is not None:
            step["nextest"] = nextest_split(step, junit)
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
    steps = fold_steps(meta, load_journal(journal), journal)
    errors = []
    record.update(
        {
            "started_at": meta["started_at"],
            "ended_at": ended_at,
            "wall_secs": round(ended_at - meta["started_at"], 6),
            "exit_code": exit_code,
            "verdict": "pass" if exit_code == 0 else "fail",
            "signal": signal or None,
            "release_pass": release_pass_of(meta),
            "steps": steps,
            "lane_seed": lane_seed_or_none(meta, steps, errors),
        }
    )
    record["instrument_errors"] = read_instrument_errors(journal) + errors
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
# Lane seed stamp: <lane_dir>.seed-stamp.json, a SIBLING of the lane (beside
# seed-warm-lane.sh's <lane_dir>.lock), so it survives the reseed of
# <lane>/target and is invisible to lane enumerators that glob directories.
# ---------------------------------------------------------------------------


def seed_stamp_path(lane_dir):
    return Path(os.path.realpath(lane_dir) + SEED_STAMP_SUFFIX)


def write_seed_stamp(lane_dir, mode, started_at):
    path = seed_stamp_path(lane_dir)
    stamp = {
        "schema": SEED_STAMP_SCHEMA,
        "seed_id": secrets.token_hex(8),
        "lane_dir": os.path.realpath(lane_dir),
        "mode": mode,
        "started_at": started_at,
        "exited_at": time.time(),
    }
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix=f".{path.name}.", suffix=".tmp")
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as fh:
            fh.write(json.dumps(stamp) + "\n")
        os.replace(tmp, path)
    except BaseException:
        os.unlink(tmp)
        raise


# ---------------------------------------------------------------------------
# Lane seed forensics: the record's lane_seed block, read-only over the lane.
# Seed → first build is timed from the stamp's exited_at (S) to the first
# known cargo start after S: an agent start from the agent-bin/cargo log, or
# this run's first cargo step. Cargo rewrites fingerprint files in place, so
# a unit's last write is its newest FILE mtime (its dir mtime does not move).
# ---------------------------------------------------------------------------


def read_seed_stamp(lane_dir):
    path = seed_stamp_path(lane_dir)
    if not path.exists():
        return None
    stamp = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(stamp, dict):
        raise ValueError(f"{path}: not a JSON object")
    return stamp


def is_number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def read_agent_starts(lane_dir, seed_at, errors):
    path = Path(lane_dir) / "target" / AGENT_START_LOG
    if not path.exists():
        return []
    starts = []
    text = path.read_text(encoding="utf-8", errors="replace")
    for lineno, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        try:
            row = json.loads(line)
        except ValueError:
            row = None
        if not (isinstance(row, dict) and is_number(row.get("started_at")) and isinstance(row.get("subcommand"), str)):
            errors.append(f"lane_seed: malformed agent start line {lineno} in {path}")
        elif row["started_at"] > seed_at:
            starts.append({"source": "agent", "started_at": row["started_at"], "subcommand": row["subcommand"]})
    return starts


def verify_starts(meta, steps, seed_at):
    return [
        {"source": "verify", "started_at": step["began_at"], "index": step["index"], "phase": step["phase"]}
        for entry, step in zip(meta["plan"], steps)
        if entry["cargo"] and step["began_at"] is not None and step["began_at"] > seed_at
    ]


def newest_file_mtime(unit_dir):
    newest = None
    with os.scandir(unit_dir) as entries:
        for entry in entries:
            if entry.is_file(follow_symlinks=False):
                mtime = entry.stat(follow_symlinks=False).st_mtime
                newest = mtime if newest is None else max(newest, mtime)
    return newest


def scan_fingerprints(target):
    """{"<profile>/<unit>": newest file mtime or None} over every profile's .fingerprint dir."""
    units = {}
    for profile in FINGERPRINT_PROFILES:
        root = Path(target) / profile / ".fingerprint"
        if not root.is_dir():
            continue
        with os.scandir(root) as entries:
            for entry in entries:
                if entry.is_dir(follow_symlinks=False):
                    units[f"{profile}/{entry.name}"] = newest_file_mtime(entry.path)
    return units


def write_span(mtimes):
    return {
        "units": len(mtimes),
        "first_write_at": min(mtimes) if mtimes else None,
        "last_write_at": max(mtimes) if mtimes else None,
    }


def since(at, start):
    return None if at is None else round(at - start, 6)


def agent_first_build(seed_at, first, later_starts, written):
    end = later_starts[0]["started_at"] if later_starts else None
    in_window = [m for m in written if m >= first["started_at"] and (end is None or m < end)]
    last = max(in_window) if in_window else None
    return {
        "source": "agent",
        "started_at": first["started_at"],
        "window_end_at": end,
        "ended_by": "next-cargo-start" if later_starts else "no-later-cargo-start",
        "units_written": len(in_window),
        "last_unit_write_at": last,
        "seed_to_first_build_secs": since(last, seed_at),
        "basis": "fingerprint-mtime",
    }


def verify_first_build(seed_at, first, step):
    return {
        "source": "verify",
        "started_at": first["started_at"],
        "index": first["index"],
        "phase": first["phase"],
        "exited_at": step["ended_at"],
        "rc": step["rc"],
        "seed_to_first_build_secs": since(step["ended_at"], seed_at),
        "basis": "verify-step-end",
    }


def first_build_window(seed_at, starts, written, steps):
    if not starts:
        return None
    first = starts[0]
    if first["source"] == "agent":
        return agent_first_build(seed_at, first, starts[1:], written)
    return verify_first_build(seed_at, first, steps[first["index"]])


def lane_seed_block(meta, steps, errors):
    stamp = read_seed_stamp(meta["lane_dir"])
    if stamp is None:
        return None
    seed_at = stamp["exited_at"]
    starts = sorted(
        read_agent_starts(meta["lane_dir"], seed_at, errors) + verify_starts(meta, steps, seed_at),
        key=lambda start: start["started_at"],
    )
    units = scan_fingerprints(Path(meta["lane_dir"]) / "target")
    written = sorted(m for m in units.values() if m is not None and m > seed_at)
    first_start = starts[0]["started_at"] if starts else None
    span = write_span(written)
    return {
        "seed": stamp,
        "cargo_starts": starts,
        "fingerprint": {
            "units_total": len(units),
            "units_written_since_seed": span["units"],
            "first_write_at": span["first_write_at"],
            "last_write_at": span["last_write_at"],
        },
        "unattributed_writes": write_span([m for m in written if first_start is None or m < first_start]),
        "first_build": first_build_window(seed_at, starts, written, steps),
    }


def lane_seed_or_none(meta, steps, errors):
    try:
        return lane_seed_block(meta, steps, errors)
    except Exception as exc:  # fail-open: a bad stamp or lane costs the block, never the record
        errors.append(f"lane_seed: {type(exc).__name__}: {exc}")
        return None


# ---------------------------------------------------------------------------
# sccache report (ruling D10): the counters are host-global windows, so a
# delta is only read inside its concurrency bucket, the zero bucket first,
# and a thin bucket is marked inconclusive.
# ---------------------------------------------------------------------------


def iter_ledger(ledger_dir, discarded):
    path = Path(ledger_dir) / LEDGER_FILE
    if not path.exists():
        return
    with open(path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            try:
                record = json.loads(line)
            except ValueError:
                record = None
            if isinstance(record, dict):
                yield record
            else:
                discarded["malformed-record"] = discarded.get("malformed-record", 0) + 1


def counter_totals(snapshot):
    stats = snapshot["stats"]
    return (
        sum(stats["cache_hits"]["counts"].values()),
        sum(stats["cache_misses"]["counts"].values()),
        stats["requests_not_cacheable"],
    )


def sccache_sample(step):
    """(bucket, (hits, misses, not_cacheable) deltas), a discard reason, or None if not a sample."""
    pair = step.get("sccache")
    if not isinstance(pair, dict):
        return None
    before, after = pair.get("before"), pair.get("after")
    if not all(isinstance(side, dict) and "stats" in side for side in (before, after)):
        return "sccache-unavailable"
    try:
        concurrency = step["concurrency"]
        bucket = max(concurrency["before"]["other_verify_runs"], concurrency["after"]["other_verify_runs"])
    except (KeyError, TypeError):
        return "no-concurrency"
    try:
        deltas = tuple(a - b for a, b in zip(counter_totals(after), counter_totals(before)))
    except (KeyError, TypeError, AttributeError):
        return "unparseable-stats"
    if any(delta < 0 for delta in deltas):
        return "counter-reset"
    return bucket, deltas


def sccache_report(ledger_dir, min_samples):
    discarded, totals = {}, {}
    for record in iter_ledger(ledger_dir, discarded):
        for step in record.get("steps") or []:
            sample = sccache_sample(step) if isinstance(step, dict) else None
            if isinstance(sample, str):
                discarded[sample] = discarded.get(sample, 0) + 1
            elif sample is not None:
                bucket, deltas = sample
                count, sums = totals.get(bucket, (0, (0, 0, 0)))
                totals[bucket] = (count + 1, tuple(a + b for a, b in zip(sums, deltas)))
    buckets = []
    for bucket in sorted(totals):
        samples, (hits, misses, not_cacheable) = totals[bucket]
        buckets.append(
            {
                "concurrency": bucket,
                "samples": samples,
                "hits": hits,
                "misses": misses,
                "not_cacheable": not_cacheable,
                "hit_rate": hits / (hits + misses) if hits + misses else None,
                "conclusive": samples >= min_samples,
            }
        )
    return {"min_samples": min_samples, "buckets": buckets, "discarded": discarded}


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


def cmd_boundary(args):
    at = args.at
    append_event(
        args.journal,
        {
            "ev": "snapshot",
            "index": args.index,
            "edge": args.edge,
            "at": at,
            "sccache": sccache_snapshot(),
            "concurrency": concurrency_snapshot(args.self_pid),
        },
    )
    if args.edge == "after" and args.junit:
        entry = read_meta(args.journal)["plan"][args.index]
        if entry["nextest"]:
            append_event(args.journal, {"ev": "junit", "index": args.index, "at": at, **junit_report(args.junit)})


def cmd_harness_ancestor(args):
    argv = harness_ancestor(args.self_pid)
    if argv is not None:
        sys.stdout.write(json.dumps(argv) + "\n")


def cmd_sccache_report(args):
    sys.stdout.write(json.dumps(sccache_report(args.ledger_dir, args.min_samples), indent=2) + "\n")


def cmd_seed_stamp(args):
    write_seed_stamp(args.lane_dir, args.mode, args.started_at)


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

    boundary = sub.add_parser("boundary", help="snapshot sccache + concurrency at a cargo step edge")
    boundary.add_argument("--journal", required=True)
    boundary.add_argument("--index", type=int, required=True)
    boundary.add_argument("--edge", choices=("before", "after"), required=True)
    boundary.add_argument("--at", type=float, required=True)
    boundary.add_argument("--self-pid", type=int, required=True)
    boundary.add_argument("--junit", default="")
    boundary.set_defaults(func=cmd_boundary)

    finalize = sub.add_parser("finalize", help="compose the run record and append it to the ledger (once)")
    finalize.add_argument("--journal", required=True)
    finalize.add_argument("--ledger-dir", required=True)
    finalize.add_argument("--exit-code", type=int, required=True)
    finalize.add_argument("--ended-at", type=float, required=True)
    finalize.add_argument("--signal", default="")
    finalize.set_defaults(func=cmd_finalize, consumes_journal=True)

    harness = sub.add_parser("harness-ancestor", help="print the argv of the nearest infra-harness ancestor, if any")
    harness.add_argument("--self-pid", type=int, required=True)
    harness.set_defaults(func=cmd_harness_ancestor)

    report = sub.add_parser("sccache-report", help="sccache counter deltas bucketed by concurrency (read-only)")
    report.add_argument("--ledger-dir", required=True)
    report.add_argument("--min-samples", type=int, default=10)
    report.set_defaults(func=cmd_sccache_report)

    seed = sub.add_parser("seed-stamp", help="atomically (re)write <lane_dir>.seed-stamp.json after a seed")
    seed.add_argument("--lane-dir", required=True)
    seed.add_argument("--mode", choices=("fresh-checkout", "reset-in-place"), required=True)
    seed.add_argument("--started-at", type=float, required=True)
    seed.set_defaults(func=cmd_seed_stamp)
    return parser


def append_journal_error(journal, line):
    """True once `line` is in the journal's errors.log."""
    if not journal or not os.path.isdir(journal):
        return False
    try:
        with open(journal_path(journal, "errors.log"), "a", encoding="utf-8") as fh:
            fh.write(line + "\n")
    except OSError:
        return False
    return True


def unrecorded_journal_errors(journal):
    try:
        return read_instrument_errors(journal)
    except OSError:
        return []


def record_instrument_error(args, exc):
    """To the journal's errors.log while the journal outlives the command, else stderr.

    finalize consumes its journal (verify.sh deletes it next), so a failed
    finalize reports on stderr, with the journal errors its unwritten record
    would have carried.
    """
    line = f"{args.command}: {type(exc).__name__}: {exc}"
    journal = getattr(args, "journal", None)
    if getattr(args, "consumes_journal", False):
        sys.stderr.write(f"verify-ledger.py: {line} (no ledger record written)\n")
        for pending in unrecorded_journal_errors(journal):
            sys.stderr.write(f"verify-ledger.py: unrecorded instrument error: {pending}\n")
    elif not append_journal_error(journal, line):
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
