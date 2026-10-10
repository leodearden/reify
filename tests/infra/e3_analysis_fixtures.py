"""
e3_analysis_fixtures.py — hermetic fixture builders for test_e3_analysis.py.

Not a test module (no test_ prefix, so no runner collects it). It builds the
small, self-consistent inputs the E3 analysis suite drives: a mini Protocol, a
fused-memory-shaped task snapshot, a temp runs.db carrying the live column
names, and a temp git repo with fixed committer dates. Everything it writes
goes under a caller-supplied temp directory.

The analysis directory must already be on sys.path (test_e3_analysis.py puts
it there) so e3_protocol imports by name.
"""

import json
import os
import sqlite3
import subprocess
from datetime import datetime, timedelta, timezone
from fractions import Fraction
from pathlib import Path

from e3_protocol import Amendment, Arm, ArmPrd, EraEvent, Protocol
from e3_sources import fetch_task_snapshot, write_task_snapshot

UTC = timezone.utc

MINI_PROJECT_ROOT = "/mini/reify"
MINI_FREEZE_CUTOFF = datetime(2026, 8, 28, 7, 7, tzinfo=UTC)
MINI_ERA_BOUNDARY = datetime(2026, 9, 1, tzinfo=UTC)
MINI_CARVE_OUTS = frozenset({150, 151})
MINI_INFRASTRUCTURE = frozenset({900, 901})
MINI_RETIRED = 110


def coarse_slug(index):
    return f"c{index}"


def standard_slug(index):
    return f"s{index}"


def coarse_ids(index):
    """(coalesced, reused, close) of coarse PRD c<index>: N01, N02 and N03."""
    base = 100 * index
    return frozenset({base + 1, base + 2}), frozenset({base + 3}), base + 3


def standard_ids(index):
    """(stamped_range, close, gate) of standard PRD s<index>: a 5-id range."""
    base = 100 * (10 + index)
    return (base + 1, base + 5), base + 5, base + 4


def _coarse_prd(index):
    coalesced, reused, close = coarse_ids(index)
    return ArmPrd(
        slug=coarse_slug(index),
        arm=Arm.COARSE,
        stratum="B" if index == 1 else "A",
        close_leaf=close,
        coalesced=coalesced,
        reused=reused,
        stamped_range=None,
        stamped_count=None,
        gate_leaves=frozenset(),
    )


def _standard_prd(index):
    stamped_range, close, gate = standard_ids(index)
    return ArmPrd(
        slug=standard_slug(index),
        arm=Arm.STANDARD,
        stratum="B" if index == 1 else "A",
        close_leaf=close,
        coalesced=frozenset(),
        reused=frozenset(),
        stamped_range=stamped_range,
        stamped_count=5,
        gate_leaves=frozenset({gate}),
    )


def mini_protocol(n_coarse=2, n_standard=2, **overrides):
    """A small valid Protocol: coarse PRDs c1..cN and standard PRDs s1..sM.

    c1 and s1 form stratum B. The expected freeze set is s1's first leaf, and
    s1's second leaf carries the one amendment. Any Protocol field can be
    overridden; construction re-runs every invariant check.
    """
    first_standard = standard_ids(1)[0][0]
    fields = dict(
        project_root=MINI_PROJECT_ROOT,
        prds=tuple(_coarse_prd(i) for i in range(1, n_coarse + 1))
        + tuple(_standard_prd(j) for j in range(1, n_standard + 1)),
        carve_outs=MINI_CARVE_OUTS,
        infrastructure=MINI_INFRASTRUCTURE,
        freeze_cutoff=MINI_FREEZE_CUTOFF,
        expected_freeze_set=frozenset({first_standard}),
        landing_floor_k=2,
        alpha=Fraction(1, 10),
        drain_window=timedelta(seconds=300),
        era_events=(EraEvent(MINI_ERA_BOUNDARY, "mini era boundary"),),
        amendments=(
            Amendment(first_standard + 1, "2026-09-11", "(c)", "mini scope add"),
        ),
        stratum_b_pair=(coarse_slug(1), standard_slug(1)),
        bootstrap_seed="mini-seed",
        bootstrap_resamples=200,
        interval_level=Fraction(9, 10),
    )
    fields.update(overrides)
    return Protocol(**fields)


SNAPSHOT_UPDATED_AT = "2026-10-01T12:00:00.000Z"
PROTOCOL_DOC_PATH = "docs/prds/v0_6/e3-decomposition-granularity-ab.md"


def _task(task_id, metadata, status="pending"):
    return {
        "id": task_id,
        "title": f"task {task_id}",
        "status": status,
        "updatedAt": SNAPSHOT_UPDATED_AT,
        "metadata": metadata,
    }


def _coarse_tasks(prd):
    tasks = {}
    for position, task_id in enumerate(sorted(prd.coalesced)):
        constituents = [f"{task_id * 10 + n}:leaf{n}" for n in range(2 + position % 2)]
        tasks[task_id] = _task(task_id, {
            "prd_path": prd.prd_path, "x_e3_arm": "coarse", "x_e3_prd": prd.slug,
            "x_e3_constituents": constituents,
        })
    for task_id in sorted(prd.reused):
        tasks[task_id] = _task(task_id, {
            "prd_path": prd.prd_path, "x_e3_arm": "coarse", "x_e3_prd": prd.slug,
            "x_e3_reused": True,
        })
    return tasks


def _standard_tasks(prd):
    members = sorted(prd.stamped_ids())[: prd.stamped_count]
    return {task_id: _task(task_id, {"prd_path": prd.prd_path}) for task_id in members}


def _apply_override(tasks, task_id, change):
    if change is None:
        tasks.pop(task_id, None)
        return
    task = tasks.setdefault(task_id, _task(task_id, {}))
    for key, value in change.items():
        if key != "metadata":
            task[key] = value
            continue
        for meta_key, meta_value in value.items():
            if meta_value is None:
                task["metadata"].pop(meta_key, None)
            else:
                task["metadata"][meta_key] = meta_value


def snapshot_payload(protocol, overrides=None):
    """{id: get_task-shaped dict} consistent with protocol.

    Coalesced tasks carry 2-3 constituents, reused singletons x_e3_reused,
    standard leaves only their prd_path; one retired leaf, the carve-outs and
    the infrastructure tasks are present too. overrides maps an id to None
    (drop the task) or to top-level fields to set, where a "metadata" dict is
    merged key by key and a None value deletes that key.
    """
    tasks = {}
    first_coarse = protocol.prds_in(Arm.COARSE)[0]
    for prd in protocol.prds:
        tasks.update(_coarse_tasks(prd) if prd.arm is Arm.COARSE else _standard_tasks(prd))
    tasks[MINI_RETIRED] = _task(MINI_RETIRED, {
        "prd_path": first_coarse.prd_path, "x_e3_arm": "standard-retired",
        "x_e3_replacement": str(min(first_coarse.coalesced)),
    }, status="deferred")
    for task_id in sorted(protocol.carve_outs):
        tasks[task_id] = _task(task_id, {"prd_path": first_coarse.prd_path})
    for task_id in sorted(protocol.infrastructure):
        tasks[task_id] = _task(task_id, {
            "prd_path": PROTOCOL_DOC_PATH, "x_e3_arm": "infrastructure",
        })
    for task_id, change in (overrides or {}).items():
        _apply_override(tasks, task_id, change)
    return tasks


def done_override(commit):
    """The override that marks a task done by merge of commit."""
    provenance = {"kind": "merged", "commit": commit}
    return {"status": "done", "metadata": {"done_provenance": provenance}}


RUNS_DB_SCHEMA = """
CREATE TABLE runs (
    run_id TEXT PRIMARY KEY, project_id TEXT NOT NULL, prd_path TEXT,
    started_at TEXT NOT NULL, completed_at TEXT
);
CREATE TABLE events (
    id INTEGER PRIMARY KEY AUTOINCREMENT, timestamp TEXT NOT NULL,
    run_id TEXT NOT NULL, task_id TEXT, event_type TEXT NOT NULL, phase TEXT,
    role TEXT, data TEXT DEFAULT '{}', cost_usd REAL, duration_ms INTEGER
);
CREATE INDEX idx_events_run ON events(run_id);
CREATE TABLE invocations (
    id INTEGER PRIMARY KEY AUTOINCREMENT, run_id TEXT NOT NULL, task_id TEXT,
    project_id TEXT NOT NULL, account_name TEXT NOT NULL, model TEXT NOT NULL,
    role TEXT NOT NULL, cost_usd REAL NOT NULL DEFAULT 0.0,
    duration_ms INTEGER NOT NULL DEFAULT 0, capped INTEGER NOT NULL DEFAULT 0,
    started_at TEXT NOT NULL, completed_at TEXT NOT NULL
);
"""


def iso(moment):
    """A runs.db timestamp: ISO 8601 with an explicit +00:00 offset."""
    return moment.astimezone(UTC).isoformat()


def make_runs_db(path, runs, events, invocations):
    """A runs.db at path with the live tables' column names.

    runs: dicts {run_id, started_at, completed_at, project_id='reify'};
    events: dicts {timestamp, run_id, task_id, event_type, data};
    invocations: dicts {run_id, task_id, role, cost_usd, started_at,
    project_id='reify'}. Datetimes are written as ISO '+00:00' strings.
    """
    connection = sqlite3.connect(path)
    try:
        connection.executescript(RUNS_DB_SCHEMA)
        connection.executemany(
            "INSERT INTO runs (run_id, project_id, started_at, completed_at) VALUES (?, ?, ?, ?)",
            [(r["run_id"], r.get("project_id", "reify"), iso(r["started_at"]),
              iso(r["completed_at"]) if r.get("completed_at") else None) for r in runs],
        )
        connection.executemany(
            "INSERT INTO events (timestamp, run_id, task_id, event_type, data)"
            " VALUES (?, ?, ?, ?, ?)",
            [(iso(e["timestamp"]), e["run_id"],
              None if e.get("task_id") is None else str(e["task_id"]),
              e["event_type"], json.dumps(e.get("data", {}))) for e in events],
        )
        connection.executemany(
            "INSERT INTO invocations (run_id, task_id, project_id, account_name, model, role,"
            " cost_usd, started_at, completed_at) VALUES (?, ?, ?, 'max-a', 'opus', ?, ?, ?, ?)",
            [(i["run_id"], str(i["task_id"]), i.get("project_id", "reify"), i["role"],
              i["cost_usd"], iso(i["started_at"]), iso(i["started_at"] + timedelta(minutes=5)))
             for i in invocations],
        )
        connection.commit()
    finally:
        connection.close()


def started(task_id, run_id, at):
    return {"timestamp": at, "run_id": run_id, "task_id": task_id,
            "event_type": "task_started", "data": {"title": f"task {task_id}"}}


def completed(task_id, run_id, at, outcome):
    return {"timestamp": at, "run_id": run_id, "task_id": task_id,
            "event_type": "task_completed", "data": {"outcome": outcome}}


def escalated(task_id, run_id, at, escalation_id, severity=None, level=None,
              category="review_issues"):
    data = {"escalation_id": escalation_id, "category": category}
    if severity is not None:
        data["severity"] = severity
    if level is not None:
        data["level"] = level
    return {"timestamp": at, "run_id": run_id, "task_id": task_id,
            "event_type": "escalation_created", "data": data}


BASE_FILE = "base.txt"
BASE_LINES = [f"base line {n}" for n in range(20)]
REPO_BASE_DATE = "2026-08-01T00:00:00+00:00"
REPO_TAIL_DATE = "2026-10-05T00:00:00+00:00"


def _git_env(date):
    """A hermetic git env: no inherited GIT_* state (a hook's GIT_DIR or
    GIT_INDEX_FILE would outrank `git -C`), no global or system config, a
    fixed identity, and author and committer dates pinned to date."""
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    env.update({
        "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_AUTHOR_NAME": "e3-fixture", "GIT_AUTHOR_EMAIL": "e3-fixture@example.test",
        "GIT_COMMITTER_NAME": "e3-fixture", "GIT_COMMITTER_EMAIL": "e3-fixture@example.test",
        "GIT_AUTHOR_DATE": date, "GIT_COMMITTER_DATE": date,
    })
    return env


def _git(repo, *args, date=REPO_BASE_DATE):
    completed = subprocess.run(
        ["git", "-C", str(repo), "-c", "core.hooksPath=/dev/null",
         "-c", "commit.gpgsign=false", *args],
        capture_output=True, text=True, env=_git_env(date),
    )
    if completed.returncode != 0:
        raise AssertionError(f"git {' '.join(args)} failed: {completed.stderr}")
    return completed.stdout.strip()


def _write_files(repo, files):
    for relative, content in files.items():
        target = Path(repo) / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(content, bytes):
            target.write_bytes(content)
        else:
            target.write_text(content)


def _merge_branch(repo, branch, subject, date, files):
    _git(repo, "checkout", "-q", "-B", branch, "main", date=date)
    _write_files(repo, files)
    _git(repo, "add", "-A", date=date)
    _git(repo, "commit", "-q", "-m", f"work on {branch}", date=date)
    _git(repo, "checkout", "-q", "main", date=date)
    _git(repo, "merge", "-q", "--no-ff", "-m", subject, branch, date=date)
    sha = _git(repo, "rev-parse", "HEAD", date=date)
    _git(repo, "branch", "-q", "-D", branch, date=date)
    return sha


def make_merge_repo(path, merges):
    """A git repo at path whose main carries one `Merge task/<id> into main`
    --no-ff merge per (task_id, committer_date_with_offset, {path: content})
    spec, in order, plus one `Merge branch x` and one direct commit on main.

    base.txt (20 lines) is committed first so a spec can edit existing lines.
    Returns [(task_id, merge sha)] in spec order.
    """
    Path(path).mkdir(parents=True, exist_ok=True)
    _git(path, "init", "-q", "-b", "main")
    _write_files(path, {BASE_FILE: "\n".join(BASE_LINES) + "\n"})
    _git(path, "add", "-A")
    _git(path, "commit", "-q", "-m", "base")
    shas = [
        (task_id, _merge_branch(path, f"task/{task_id}", f"Merge task/{task_id} into main",
                                date, files))
        for task_id, date, files in merges
    ]
    _merge_branch(path, "x", "Merge branch x", REPO_TAIL_DATE, {"x.txt": "x\n"})
    _write_files(path, {"direct.txt": "direct\n"})
    _git(path, "add", "-A", date=REPO_TAIL_DATE)
    _git(path, "commit", "-q", "-m", "direct commit on main", date=REPO_TAIL_DATE)
    return shas


def base_with_edits(edited):
    """base.txt's content with the given line indexes rewritten."""
    lines = [f"edited {n}" if n in edited else line for n, line in enumerate(BASE_LINES)]
    return "\n".join(lines) + "\n"


def fake_task_store(payload):
    """(call_tool, calls): an in-memory fused-memory answering get_statuses and
    get_task from payload, recording every (tool, arguments) call."""
    calls = []

    def call_tool(name, arguments):
        calls.append((name, arguments))
        if name == "get_statuses":
            wanted = [int(i) for i in arguments["ids"]]
            return {"statuses": {str(i): payload[i]["status"] for i in wanted if i in payload}}
        if name == "get_task":
            return json.loads(json.dumps(payload[int(arguments["id"])]))
        raise AssertionError(f"unexpected tool {name}")

    return call_tool, calls


SNAPSHOT_FETCHED_AT = datetime(2026, 10, 10, 20, 0, tzinfo=UTC)


def task_snapshot(protocol, overrides=None, fetched_at=SNAPSHOT_FETCHED_AT):
    """A TaskSnapshot of snapshot_payload(protocol, overrides), read through
    the public fetch path against the in-memory task store."""
    call_tool, _ = fake_task_store(snapshot_payload(protocol, overrides))
    return fetch_task_snapshot(
        call_tool, protocol.project_root, protocol.snapshot_candidate_ids(), fetched_at)


CLI_RUN_START = datetime(2026, 8, 27, 0, 0, tzinfo=UTC)
LOCAL_OFFSET = timezone(timedelta(hours=1))


def build_cli_inputs(directory, protocol, landings, extra_starts=(),
                     fetched_at=SNAPSHOT_FETCHED_AT):
    """The three CLI inputs under directory, mutually consistent.

    landings: (task_id, merged_at, loc, cost) — each is a `Merge task/<id>
    into main` of a loc-line file committed at merged_at (+01:00 local), a
    done-by-merge snapshot record, one done attempt and one invocation of
    cost. Every id in protocol.expected_freeze_set gets an attempt started an
    hour before the freeze cutoff; extra_starts adds (task_id, started_at)
    task_started events. Returns {snapshot, runs_db, repo} paths.
    """
    directory = Path(directory)
    repo = directory / "repo"
    shas = dict(make_merge_repo(repo, [
        (task_id, merged_at.astimezone(LOCAL_OFFSET).isoformat(), {f"t{task_id}.txt": "x\n" * loc})
        for task_id, merged_at, loc, _ in sorted(landings, key=lambda landing: landing[1])
    ]))
    snapshot_path = directory / "tasks-snapshot.json"
    call_tool, _ = fake_task_store(snapshot_payload(
        protocol, {task_id: done_override(sha) for task_id, sha in shas.items()}))
    write_task_snapshot(fetch_task_snapshot(
        call_tool, protocol.project_root, protocol.snapshot_candidate_ids(), fetched_at),
        snapshot_path)
    events, invocations = [], []
    for task_id, merged_at, _, cost in landings:
        events += [started(task_id, "r1", merged_at - timedelta(hours=2)),
                   completed(task_id, "r1", merged_at - timedelta(hours=1), "done")]
        invocations.append({"run_id": "r1", "task_id": task_id, "role": "implementer",
                            "cost_usd": cost, "started_at": merged_at - timedelta(hours=2)})
    for task_id in sorted(protocol.expected_freeze_set):
        events.append(started(task_id, "r1", protocol.freeze_cutoff - timedelta(hours=1)))
    events += [started(task_id, "r1", started_at) for task_id, started_at in extra_starts]
    runs_db = directory / "runs.db"
    make_runs_db(runs_db, [{"run_id": "r1", "started_at": CLI_RUN_START, "completed_at": None}],
                 events, invocations)
    return {"snapshot": snapshot_path, "runs_db": runs_db, "repo": repo}
