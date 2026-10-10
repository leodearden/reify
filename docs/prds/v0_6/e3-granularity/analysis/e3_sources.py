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
import sys
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

from e3_protocol import ProtocolViolation

REPO_ROOT = Path(__file__).resolve().parents[5]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

from escalation_mcp import McpSession  # noqa: E402  (after sys.path manipulation)

SNAPSHOT_SCHEMA = "e3-task-snapshot/1"
MCP_SOURCE = "fused-memory MCP get_statuses + get_task"


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
