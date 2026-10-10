"""
e3_sources.py — the read-only boundary readers for E3's three data sources.

Each reader parses its source into frozen records the pure model consumes:
- the task store, read through the fused-memory MCP (get_statuses, then
  get_task per known id) or replayed from a committed snapshot file; it never
  reads .taskmaster/tasks/tasks.db;
- data/orchestrator/runs.db, opened read-only, for attempts, invocations and
  escalations;
- git main history, for each task's first-parent merge and its LOC.

Nothing here decides what a record means for the protocol; e3_model does.
"""

import dataclasses
import json
import sqlite3
import sys
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

from e3_protocol import ProtocolViolation

REPO_ROOT = Path(__file__).resolve().parents[5]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

from escalation_mcp import McpSession  # noqa: E402  (after sys.path manipulation)

SNAPSHOT_SCHEMA = "e3-task-snapshot/1"
MCP_SOURCE = "fused-memory MCP get_statuses + get_task"
ATTEMPT_EVENTS = ("task_started", "task_completed", "escalation_created")


class SourceError(Exception):
    """A data source could not be read or does not parse as its contract says."""


@dataclass(frozen=True)
class TaskRecord:
    id: int
    title: str
    status: str
    prd_path: str | None
    x_e3_arm: str | None
    x_e3_prd: str | None
    x_e3_reused: bool
    constituents: tuple
    provenance_commit: str | None


@dataclass(frozen=True)
class TaskSnapshot:
    fetched_at: datetime
    project_root: str
    source: str
    tasks: tuple

    def task(self, task_id):
        for record in self.tasks:
            if record.id == task_id:
                return record
        return None


def _task_record(raw):
    metadata = raw.get("metadata") or {}
    provenance = metadata.get("done_provenance") or {}
    return TaskRecord(
        id=int(raw["id"]),
        title=raw.get("title", ""),
        status=raw["status"],
        prd_path=metadata.get("prd_path"),
        x_e3_arm=metadata.get("x_e3_arm"),
        x_e3_prd=metadata.get("x_e3_prd"),
        x_e3_reused=bool(metadata.get("x_e3_reused", False)),
        constituents=tuple(metadata.get("x_e3_constituents") or ()),
        provenance_commit=provenance.get("commit"),
    )


def fetch_task_snapshot(call_tool, project_root, candidate_ids, fetched_at, source=MCP_SOURCE):
    """Every candidate id the task store knows, as one TaskSnapshot.

    call_tool(name, arguments) -> dict is injected; open_fused_memory builds
    the live one. get_statuses names the ids that exist, so get_task is called
    only for those.
    """
    statuses = call_tool(
        "get_statuses",
        {"project_root": project_root, "ids": [str(i) for i in candidate_ids]},
    )["statuses"]
    records = []
    for task_id in sorted(int(key) for key in statuses):
        raw = call_tool("get_task", {"id": str(task_id), "project_root": project_root})
        if int(raw.get("id", -1)) != task_id:
            raise ProtocolViolation("snapshot record id", str(task_id), str(raw.get("id")))
        records.append(_task_record(raw))
    return TaskSnapshot(fetched_at, project_root, source, tuple(records))


def write_task_snapshot(snapshot, path):
    document = {
        "schema": SNAPSHOT_SCHEMA,
        "fetched_at": snapshot.fetched_at.isoformat(),
        "project_root": snapshot.project_root,
        "source": snapshot.source,
        "tasks": [dataclasses.asdict(record) for record in snapshot.tasks],
    }
    Path(path).write_text(json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def read_task_snapshot(path):
    document = json.loads(Path(path).read_text(encoding="utf-8"))
    if document.get("schema") != SNAPSHOT_SCHEMA:
        raise ValueError(
            f"{path}: snapshot schema {document.get('schema')!r}, expected {SNAPSHOT_SCHEMA!r}"
        )
    return TaskSnapshot(
        fetched_at=datetime.fromisoformat(document["fetched_at"]),
        project_root=document["project_root"],
        source=document["source"],
        tasks=tuple(
            TaskRecord(**{**task, "constituents": tuple(task["constituents"])})
            for task in document["tasks"]
        ),
    )


def fused_memory_url(mcp_config_path):
    """mcpServers['fused-memory'].url of a .mcp.json-shaped file."""
    try:
        config = json.loads(Path(mcp_config_path).read_text(encoding="utf-8"))
    except ValueError as error:
        raise ValueError(f"{mcp_config_path}: unparseable MCP config: {error}") from error
    url = config
    for key in ("mcpServers", "fused-memory", "url"):
        url = url.get(key) if isinstance(url, dict) else None
    if not isinstance(url, str) or not url:
        raise ValueError(f"{mcp_config_path} declares no mcpServers['fused-memory'].url")
    return url


def open_fused_memory(url):
    """A live call_tool bound to an initialized fused-memory MCP session."""
    session = McpSession(url, "e3-analysis")
    session.open()
    return session.call_tool


@dataclass(frozen=True)
class Attempt:
    """One dispatch: a task_started and the first later task_completed of the
    same task in the same run (ended_at/outcome None when the run never
    completed it). run_ended_at is None while the run is live."""

    task_id: int
    run_id: str
    started_at: datetime
    ended_at: datetime | None
    outcome: str | None
    run_ended_at: datetime | None


@dataclass(frozen=True)
class Invocation:
    task_id: int
    role: str
    started_at: datetime
    cost_usd: float


@dataclass(frozen=True)
class Escalation:
    task_id: int
    escalation_id: str
    created_at: datetime
    severity: str | None
    level: int | None
    category: str | None


@dataclass(frozen=True)
class RunsData:
    attempts: tuple
    invocations: tuple
    escalations: tuple


@dataclass(frozen=True)
class _Event:
    at: datetime
    run_id: str
    task_id: int
    event_type: str
    data: dict


def utc_timestamp(text):
    """An ISO 8601 timestamp with an explicit offset, as an aware UTC datetime."""
    moment = datetime.fromisoformat(text)
    if moment.tzinfo is None:
        raise SourceError(f"timestamp {text!r} carries no UTC offset")
    return moment.astimezone(timezone.utc)


def _placeholders(values):
    return ", ".join("?" for _ in values)


def _task_events(connection, task_ids, project_id):
    ids = [str(i) for i in sorted(task_ids)]
    rows = connection.execute(
        "SELECT e.timestamp, e.run_id, e.task_id, e.event_type, e.data FROM events e"
        " JOIN runs r ON r.run_id = e.run_id"
        f" WHERE r.project_id = ? AND e.event_type IN ({_placeholders(ATTEMPT_EVENTS)})"
        f" AND e.task_id IN ({_placeholders(ids)})"
        " ORDER BY e.timestamp, e.id",
        [project_id, *ATTEMPT_EVENTS, *ids],
    ).fetchall()
    return [
        _Event(utc_timestamp(at), run_id, int(task_id), event_type, json.loads(data or "{}"))
        for at, run_id, task_id, event_type, data in rows
    ]


def _run_ends(connection, run_ids, project_id):
    """run_id -> when it ended: completed_at; else, for a run a later run
    followed (it crashed), its last event; else None (still live)."""
    runs = {
        run_id: (utc_timestamp(started), utc_timestamp(completed) if completed else None)
        for run_id, started, completed in connection.execute(
            "SELECT run_id, started_at, completed_at FROM runs WHERE project_id = ?",
            [project_id],
        )
    }
    latest = max(runs, key=lambda run_id: runs[run_id][0]) if runs else None
    ends = {}
    for run_id in run_ids:
        end = runs[run_id][1]
        if end is None and run_id != latest:
            (last,) = connection.execute(
                "SELECT max(timestamp) FROM events WHERE run_id = ?", [run_id]
            ).fetchone()
            end = utc_timestamp(last)
        ends[run_id] = end
    return ends


def _pair_attempts(events, run_ends):
    """Each start closed by the first later completion in its (task, run)."""
    open_starts = {}
    closed = []
    for event in events:
        key = (event.task_id, event.run_id)
        if event.event_type == "task_started":
            open_starts.setdefault(key, []).append(event)
        elif event.event_type == "task_completed" and open_starts.get(key):
            start = open_starts[key].pop(0)
            closed.append((start, event))
    unclosed = [(start, None) for starts in open_starts.values() for start in starts]
    attempts = [
        Attempt(
            task_id=start.task_id,
            run_id=start.run_id,
            started_at=start.at,
            ended_at=end.at if end else None,
            outcome=end.data.get("outcome") if end else None,
            run_ended_at=run_ends[start.run_id],
        )
        for start, end in closed + unclosed
    ]
    return tuple(sorted(attempts, key=lambda a: (a.started_at, a.task_id)))


def _escalations(events):
    return tuple(
        Escalation(
            task_id=event.task_id,
            escalation_id=event.data.get("escalation_id"),
            created_at=event.at,
            severity=event.data.get("severity"),
            level=event.data.get("level"),
            category=event.data.get("category"),
        )
        for event in events
        if event.event_type == "escalation_created"
    )


def _invocations(connection, task_ids, project_id):
    ids = [str(i) for i in sorted(task_ids)]
    rows = connection.execute(
        "SELECT task_id, role, started_at, cost_usd FROM invocations"
        f" WHERE project_id = ? AND task_id IN ({_placeholders(ids)})"
        " ORDER BY started_at, id",
        [project_id, *ids],
    ).fetchall()
    return tuple(
        Invocation(int(task_id), role, utc_timestamp(started), float(cost))
        for task_id, role, started, cost in rows
    )


def read_runs_db(path, task_ids, project_id="reify"):
    """Attempts, invocations and escalations of task_ids, read-only."""
    connection = sqlite3.connect(Path(path).absolute().as_uri() + "?mode=ro", uri=True)
    try:
        events = _task_events(connection, task_ids, project_id)
        run_ends = _run_ends(connection, {event.run_id for event in events}, project_id)
        invocations = _invocations(connection, task_ids, project_id)
    finally:
        connection.close()
    return RunsData(
        attempts=_pair_attempts(events, run_ends),
        invocations=invocations,
        escalations=_escalations(events),
    )
