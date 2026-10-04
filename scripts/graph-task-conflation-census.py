#!/usr/bin/env python3
"""graph-task-conflation-census.py — find, review and verify repairs of Task-N
entity conflations in a Graphiti graph stored in FalkorDB.

A conflation is a live RELATES_TO edge attached to a Task-N entity node whose
fact is really about a DIFFERENT task: the node's own number is absent from
the fact, and the fact names another local task number. Entity resolution
produced these while Graphiti's BM25 arm served nothing (FalkorDB FULLTEXT
indexes were missing), so adjacent task numbers were merged by fuzzy matching.

Subcommands
-----------
census         READ-ONLY. Confirms the graph key by node count, probes whether
               BM25 serves, collects every Task-N-named Entity node and every
               live edge touching it (UNDIRECTED: an outgoing-only probe misses
               incoming conflations), classifies candidates, and writes
               <out>/<graph>-task-conflations-<UTC stamp>.json plus a
               pre-filled <stem>.adjudication.json template. Refuses to
               overwrite.
review         OFFLINE. Validates an adjudication against its census, writes
               <stem>.review.md next to the census, and prints the repair list
               (keys mirror fused-memory reassign_edge's parameters) on stdout.
check-repairs  READ-ONLY. Reports, per adjudicated REPAIR row, whether the
               edge's endpoint is still pending, applied, moved elsewhere, gone,
               targets a missing node, or still needs a node minted; exits 0
               iff every row has the --require'd state.

Candidate rule (asymmetric on purpose, erring toward FEWER candidates because
every candidate feeds irreversible edge surgery): a row is a candidate iff the
fact names a local task number under the STRICT task-prefixed grammar
(fact_task_refs: runs, ranges and Greek suffixes included, project-qualified
refs excluded) AND none of the node's own numbers appears under the BROAD check
(mentions_number: any digit-bounded occurrence). Both live in the sibling
module scripts/task_reference_grammar.py. The tool proposes REPAIR or
RECORD_ONLY from structure; only a human adjudication assigns
NOT_A_CONFLATION.

Artifact schema (schema_version 1)
----------------------------------
census:       {schema_version, generator, graph_key, generated_at,
               key_confirmation{expected_node_count, observed_node_count},
               bm25_probe{token, fulltext_hits, name_contains, serving, error},
               summary{task_nodes, live_edge_rows_scanned, self_loops_skipped,
                       foreign_facts, adjacent_signature,
                       proposed{REPAIR, RECORD_ONLY, NOT_A_CONFLATION}},
               candidates[{candidate_id = '<edge_uuid>@<node_uuid>',
                           node_uuid, node_name, own_numbers, edge_uuid,
                           which_end ('source'|'target', the node's end),
                           other_uuid, other_name, fact, named_numbers,
                           signature ('adjacent'|'non_adjacent', +/-2),
                           created_at, first_episode, episode_count,
                           proposal{verdict, reason, target_numbers,
                                    target_node_uuids, mint_name}}]}
adjudication: {census: <census file name>,
               adjudications[{candidate_id, verdict, target_node_uuid,
                              mint_name, rationale}]}
               A REPAIR carries exactly one of target_node_uuid (an existing
               node, task-named or not) or mint_name ('Task N', canonical).
check report: {census, checked_at, require,
               rows[{candidate_id, edge_uuid, which_end, status}], counts}

Safety
------
Every graph access is GRAPH.RO_QUERY, issued from one method of FalkorReader;
the server refuses writes under that verb. Never run Graphiti's
build_indices_and_constraints or construct a graphiti driver from here.
Interpolated uuids and tokens are validated before they reach Cypher.

FalkorDB reply quirks the reader respects
-----------------------------------------
- Verbose replies render list values as STRINGS, so queries project scalars
  (e.episodes[0], size(e.episodes)) instead of returning lists.
- Booleans come back as 'true'/'false' strings.
- '=~' is unsupported (raises ResponseError).
- A hard per-query TIMEOUT of 1000 ms, so edge reads are batched by node uuid
  and a 'timed out' error is retried a bounded number of times.
- A SINGLE-quoted list-valued CYPHER parameter SIGSEGVs FalkorDB 4.18.0 and
  restarts the shared server (esc-6582-1). List parameters are therefore
  serialized as compact double-quoted JSON, byte-for-byte the form falkordb-py
  (and so production Graphiti) sends.

The live connection needs the `redis` package (dark-factory's venv python3);
the module itself is stdlib-only so --help and the tests run anywhere.
"""

import argparse
import enum
import importlib
import itertools
import json
import re
import sys
import time
from collections import Counter
from collections.abc import Callable, Iterable, Sequence
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Protocol

from task_reference_grammar import (
    canonical_task_number,
    fact_task_refs,
    mentions_number,
    node_task_numbers,
)

REPO_ROOT = Path(__file__).resolve().parents[1]
SCHEMA_VERSION = 1
GENERATOR = "scripts/graph-task-conflation-census.py"
ADJACENCY_DISTANCE = 2

# ── Artifact vocabulary ─────────────────────────────────────────────────────


class Verdict(enum.Enum):
    REPAIR = "REPAIR"
    RECORD_ONLY = "RECORD_ONLY"
    NOT_A_CONFLATION = "NOT_A_CONFLATION"


class ProposalReason(enum.Enum):
    UNIQUE_TARGET = "unique_target"
    TARGET_NODE_ABSENT = "target_node_absent"
    AMBIGUOUS_TARGET_NODES = "ambiguous_target_nodes"
    AMBIGUOUS_TARGET_NUMBERS = "ambiguous_target_numbers"
    UNARY_ABOUT_OTHER_ENDPOINT = "unary_about_other_endpoint"


class KeyConfirmationError(Exception):
    """The graph key's node count disagrees with the count the caller expected."""


@dataclass(frozen=True)
class EdgeRow:
    """One live-edge row as seen from a Task-N node (which_end = that node's end)."""

    node_uuid: str
    edge_uuid: str
    fact: str
    which_end: str
    other_uuid: str
    other_name: str
    created_at: str | None
    invalid_at: str | None
    expired_at: str | None
    first_episode: str | None
    episode_count: int | None

    @property
    def is_tombstoned(self) -> bool:
        return self.invalid_at is not None or self.expired_at is not None


@dataclass(frozen=True)
class Bm25Probe:
    token: str
    fulltext_hits: int | None
    name_contains: int
    error: str | None

    @property
    def serving(self) -> bool:
        if self.fulltext_hits is not None and self.fulltext_hits > 0:
            return True
        return self.name_contains == 0 and self.error is None


@dataclass(frozen=True)
class EdgeEndpoints:
    source_uuid: str
    target_uuid: str
    invalid_at: str | None
    expired_at: str | None


class GraphReader(Protocol):
    def node_count(self) -> int: ...

    def task_named_nodes(self) -> list[tuple[str, str]]: ...

    def edges_touching(self, node_uuids: Sequence[str]) -> list[EdgeRow]: ...

    def bm25_probe(self, token: str) -> Bm25Probe: ...

    def edge_endpoints(self, edge_uuid: str) -> EdgeEndpoints | None: ...

    def node_exists(self, uuid: str) -> bool: ...


# ── Census ──────────────────────────────────────────────────────────────────


@dataclass(frozen=True)
class TaskNodeIndex:
    """The Task-N node population and the canonical-name index of REPAIR targets."""

    names: dict[str, str]
    own_numbers: dict[str, frozenset[int]]
    canonical: dict[int, tuple[str, ...]]

    @classmethod
    def from_nodes(cls, nodes: Iterable[tuple[str, str]]) -> "TaskNodeIndex":
        names: dict[str, str] = {}
        own_numbers: dict[str, frozenset[int]] = {}
        canonical: dict[int, list[str]] = {}
        for uuid, name in nodes:
            numbers = node_task_numbers(name)
            if numbers:
                names[uuid] = name
                own_numbers[uuid] = numbers
            number = canonical_task_number(name)
            if number is not None:
                canonical.setdefault(number, []).append(uuid)
        return cls(names, own_numbers,
                   {number: tuple(sorted(uuids)) for number, uuids in canonical.items()})


def _proposal(verdict: Verdict, reason: ProposalReason, numbers: list[int],
              target_node_uuids: Sequence[str] = (), mint_name: str | None = None) -> dict:
    return {"verdict": verdict.value, "reason": reason.value, "target_numbers": numbers,
            "target_node_uuids": list(target_node_uuids), "mint_name": mint_name}


def propose(surviving_numbers: frozenset[int], index: TaskNodeIndex) -> dict:
    """Map the numbers a fact names beyond both endpoints to a proposed verdict."""
    numbers = sorted(surviving_numbers)
    if not numbers:
        return _proposal(Verdict.RECORD_ONLY, ProposalReason.UNARY_ABOUT_OTHER_ENDPOINT, numbers)
    if len(numbers) > 1:
        return _proposal(Verdict.RECORD_ONLY, ProposalReason.AMBIGUOUS_TARGET_NUMBERS, numbers)
    targets = index.canonical.get(numbers[0], ())
    if not targets:
        return _proposal(Verdict.REPAIR, ProposalReason.TARGET_NODE_ABSENT, numbers,
                         mint_name=f"Task {numbers[0]}")
    if len(targets) > 1:
        return _proposal(Verdict.RECORD_ONLY, ProposalReason.AMBIGUOUS_TARGET_NODES, numbers, targets)
    return _proposal(Verdict.REPAIR, ProposalReason.UNIQUE_TARGET, numbers, targets)


def _signature(own: frozenset[int], named: frozenset[int]) -> str:
    adjacent = any(abs(a - b) <= ADJACENCY_DISTANCE for a in own for b in named)
    return "adjacent" if adjacent else "non_adjacent"


def classify_row(row: EdgeRow, index: TaskNodeIndex) -> dict | None:
    """The candidate record for a live, non-self-loop row, or None if it is not foreign."""
    own = index.own_numbers[row.node_uuid]
    named = fact_task_refs(row.fact).local
    if not named or own & named or any(mentions_number(row.fact, n) for n in own):
        return None
    return {
        "candidate_id": f"{row.edge_uuid}@{row.node_uuid}",
        "node_uuid": row.node_uuid,
        "node_name": index.names[row.node_uuid],
        "own_numbers": sorted(own),
        "edge_uuid": row.edge_uuid,
        "which_end": row.which_end,
        "other_uuid": row.other_uuid,
        "other_name": row.other_name,
        "fact": row.fact,
        "named_numbers": sorted(named),
        "signature": _signature(own, named),
        "created_at": row.created_at,
        "first_episode": row.first_episode,
        "episode_count": row.episode_count,
        "proposal": propose(named - node_task_numbers(row.other_name), index),
    }


def summarise(index: TaskNodeIndex, live_rows: int, self_loops: int,
              candidates: list[dict]) -> dict:
    proposed = {verdict.value: 0 for verdict in Verdict}
    for candidate in candidates:
        proposed[candidate["proposal"]["verdict"]] += 1
    return {
        "task_nodes": len(index.own_numbers),
        "live_edge_rows_scanned": live_rows,
        "self_loops_skipped": self_loops,
        "foreign_facts": len(candidates),
        "adjacent_signature": sum(c["signature"] == "adjacent" for c in candidates),
        "proposed": proposed,
    }


def _probe_record(probe: Bm25Probe) -> dict:
    return {"token": probe.token, "fulltext_hits": probe.fulltext_hits,
            "name_contains": probe.name_contains, "serving": probe.serving,
            "error": probe.error}


def build_census(reader: GraphReader, graph_key: str, expected_node_count: int,
                 generated_at: str, bm25_token: str = "orchestrator") -> dict:
    """Read the graph through `reader` and return the census artifact."""
    observed = reader.node_count()
    if observed != expected_node_count:
        raise KeyConfirmationError(
            f"graph {graph_key!r} reports {observed} nodes but {expected_node_count} were "
            "expected; re-read get_status and retry")
    probe = reader.bm25_probe(bm25_token)
    index = TaskNodeIndex.from_nodes(reader.task_named_nodes())
    live = [row for row in reader.edges_touching(sorted(index.own_numbers))
            if not row.is_tombstoned]
    self_loops = sum(row.other_uuid == row.node_uuid for row in live)
    candidates = [candidate for row in live if row.other_uuid != row.node_uuid
                  if (candidate := classify_row(row, index)) is not None]
    candidates.sort(key=lambda c: (min(c["own_numbers"]), c["node_uuid"], c["edge_uuid"]))
    return {
        "schema_version": SCHEMA_VERSION,
        "generator": GENERATOR,
        "graph_key": graph_key,
        "generated_at": generated_at,
        "key_confirmation": {"expected_node_count": expected_node_count,
                             "observed_node_count": observed},
        "bm25_probe": _probe_record(probe),
        "summary": summarise(index, len(live), self_loops, candidates),
        "candidates": candidates,
    }


def _prefilled(candidate: dict) -> dict:
    proposal = candidate["proposal"]
    unique = proposal["reason"] == ProposalReason.UNIQUE_TARGET.value
    return {
        "candidate_id": candidate["candidate_id"],
        "verdict": proposal["verdict"],
        "target_node_uuid": proposal["target_node_uuids"][0] if unique else None,
        "mint_name": proposal["mint_name"],
        "rationale": "",
    }


def adjudication_template(census: dict, census_file_name: str) -> dict:
    """An adjudication pre-filled with every candidate's proposal and a blank rationale."""
    return {"census": census_file_name,
            "adjudications": [_prefilled(c) for c in census["candidates"]]}


# ── Live connection ─────────────────────────────────────────────────────────


_UUID_SHAPE = re.compile("[0-9a-fA-F-]{8,64}")
_PROBE_TOKEN = re.compile("[a-z0-9_]+")
_EDGES_QUERY = (
    "MATCH (n:Entity)-[e:RELATES_TO]-(m:Entity) "
    "WHERE n.uuid IN $uuids AND e.invalid_at IS NULL AND e.expired_at IS NULL "
    "RETURN n.uuid AS node_uuid, e.uuid AS edge_uuid, e.fact AS fact, "
    "startNode(e).uuid AS source_uuid, m.uuid AS other_uuid, m.name AS other_name, "
    "e.created_at AS created_at, e.invalid_at AS invalid_at, e.expired_at AS expired_at, "
    "e.episodes[0] AS first_episode, size(e.episodes) AS episode_count"
)


class GraphQueryTimeout(Exception):
    """A read query kept hitting FalkorDB's per-query TIMEOUT."""


class MissingDriverError(RuntimeError):
    """The `redis` package a live connection needs is not importable."""


def _checked_uuid(value: str) -> str:
    if not isinstance(value, str) or not _UUID_SHAPE.fullmatch(value):
        raise ValueError(f"refusing to interpolate a non-uuid value into Cypher: {value!r}")
    return value


def _list_parameter(values: Sequence[str]) -> str:
    return json.dumps(list(values), separators=(",", ":"))


class FalkorReader:
    """GraphReader over a redis-like connection; issues GRAPH.RO_QUERY only."""

    def __init__(self, conn, graph_key: str, batch_size: int = 200, max_attempts: int = 3,
                 retry_delay: float = 0.5):
        self._conn = conn
        self._graph_key = graph_key
        self._batch_size = batch_size
        self._max_attempts = max_attempts
        self._retry_delay = retry_delay

    def _ro(self, query: str) -> list[dict[str, Any]]:
        last_timeout: Exception | None = None
        for attempt in range(1, self._max_attempts + 1):
            try:
                reply = self._conn.execute_command("GRAPH.RO_QUERY", self._graph_key, query)
            except Exception as exc:
                # redis' ResponseError carries no structured code: FalkorDB's
                # per-query TIMEOUT is recognisable only by its message.
                if "timed out" not in str(exc):
                    raise
                last_timeout = exc
                if attempt < self._max_attempts:
                    time.sleep(self._retry_delay * attempt)
                continue
            header, rows = reply[0], reply[1]
            return [dict(zip(header, row)) for row in rows]
        raise GraphQueryTimeout(
            f"query on {self._graph_key!r} timed out {self._max_attempts} times") from last_timeout

    def _scalar(self, query: str) -> Any:
        return next(iter(self._ro(query)[0].values()))

    def node_count(self) -> int:
        return int(self._scalar("MATCH (n) RETURN count(n) AS nodes"))

    def task_named_nodes(self) -> list[tuple[str, str]]:
        rows = self._ro("MATCH (n:Entity) WHERE toLower(n.name) STARTS WITH 'task' "
                        "RETURN n.uuid AS uuid, n.name AS name")
        return [(row["uuid"], row["name"]) for row in rows]

    def edges_touching(self, node_uuids: Sequence[str]) -> list[EdgeRow]:
        uuids = [_checked_uuid(uuid) for uuid in node_uuids]
        rows: list[EdgeRow] = []
        for batch in itertools.batched(uuids, self._batch_size):
            records = self._ro(f"CYPHER uuids={_list_parameter(batch)} {_EDGES_QUERY}")
            rows.extend(_edge_row(record) for record in records)
        return rows

    def bm25_probe(self, token: str) -> Bm25Probe:
        if not _PROBE_TOKEN.fullmatch(token):
            raise ValueError(f"refusing to interpolate probe token {token!r} into Cypher")
        name_contains = int(self._scalar(
            f"MATCH (n:Entity) WHERE toLower(n.name) CONTAINS '{token}' RETURN count(n) AS nodes"))
        try:
            hits = int(self._scalar(f"CALL db.idx.fulltext.queryNodes('Entity', '{token}') "
                                    "YIELD node RETURN count(node) AS hits"))
        except Exception as exc:
            return Bm25Probe(token, None, name_contains, str(exc))
        return Bm25Probe(token, hits, name_contains, None)

    def edge_endpoints(self, edge_uuid: str) -> EdgeEndpoints | None:
        uuid = _checked_uuid(edge_uuid)
        rows = self._ro(f"MATCH (s:Entity)-[e:RELATES_TO]->(t:Entity) WHERE e.uuid = '{uuid}' "
                        "RETURN s.uuid AS source_uuid, t.uuid AS target_uuid, "
                        "e.invalid_at AS invalid_at, e.expired_at AS expired_at")
        if not rows:
            return None
        row = rows[0]
        return EdgeEndpoints(row["source_uuid"], row["target_uuid"], row["invalid_at"],
                             row["expired_at"])

    def node_exists(self, uuid: str) -> bool:
        checked = _checked_uuid(uuid)
        return int(self._scalar(
            f"MATCH (n:Entity) WHERE n.uuid = '{checked}' RETURN count(n) AS nodes")) > 0


def _edge_row(record: dict[str, Any]) -> EdgeRow:
    episode_count = record["episode_count"]
    return EdgeRow(
        node_uuid=record["node_uuid"],
        edge_uuid=record["edge_uuid"],
        fact=record["fact"] or "",
        which_end="source" if record["source_uuid"] == record["node_uuid"] else "target",
        other_uuid=record["other_uuid"],
        other_name=record["other_name"] or "",
        created_at=record["created_at"],
        invalid_at=record["invalid_at"],
        expired_at=record["expired_at"],
        first_episode=record["first_episode"],
        episode_count=None if episode_count is None else int(episode_count),
    )


def connect_falkor(url: str, import_module: Callable[[str], Any] = importlib.import_module):
    """A decode_responses redis connection to `url`; `redis` is imported only here."""
    try:
        redis = import_module("redis")
    except ModuleNotFoundError as exc:
        raise MissingDriverError(
            "a live run needs the 'redis' package; run this tool with dark-factory's venv "
            "python3, which has it") from exc
    return redis.Redis.from_url(url, decode_responses=True)


# ── Artifact files ──────────────────────────────────────────────────────────


class OutputExistsError(Exception):
    """An artifact path already exists; artifacts are never overwritten."""


def _refuse_existing(paths: Iterable[Path]) -> None:
    for path in paths:
        if path.exists():
            raise OutputExistsError(f"refusing to overwrite {path}")


def write_new_json(path: Path, data: dict) -> None:
    """Write `data` as indented UTF-8 JSON with a trailing newline; never overwrite."""
    try:
        with path.open("x", encoding="utf-8") as handle:
            handle.write(json.dumps(data, indent=2, ensure_ascii=False) + "\n")
    except FileExistsError as exc:
        raise OutputExistsError(f"refusing to overwrite {path}") from exc


def _census_report(census: dict, census_path: Path, adjudication_path: Path) -> str:
    key, probe, summary = census["key_confirmation"], census["bm25_probe"], census["summary"]
    proposed = " ".join(f"{verdict}={count}" for verdict, count in summary["proposed"].items())
    return "\n".join([
        f"census:       {census_path}",
        f"adjudication: {adjudication_path}",
        f"graph {census['graph_key']!r} at {census['generated_at']}: "
        f"{key['observed_node_count']} nodes (expected {key['expected_node_count']})",
        f"BM25 probe {probe['token']!r}: fulltext_hits={probe['fulltext_hits']} "
        f"name_contains={probe['name_contains']} serving={probe['serving']} error={probe['error']}",
        f"task nodes {summary['task_nodes']}, live edge rows scanned "
        f"{summary['live_edge_rows_scanned']}, self-loops skipped {summary['self_loops_skipped']}",
        f"foreign facts {summary['foreign_facts']} (adjacent signature "
        f"{summary['adjacent_signature']}); proposed {proposed}",
    ])


# ── Review ──────────────────────────────────────────────────────────────────

_CANONICAL_MINT_NAME = re.compile("Task [0-9]+")
_VERDICT_VALUES = frozenset(verdict.value for verdict in Verdict)


class ReviewRule(enum.Enum):
    CENSUS_MISMATCH = "census_mismatch"
    MISSING_ADJUDICATION = "missing_adjudication"
    UNKNOWN_CANDIDATE = "unknown_candidate"
    DUPLICATE_ADJUDICATION = "duplicate_adjudication"
    INVALID_VERDICT = "invalid_verdict"
    EMPTY_RATIONALE = "empty_rationale"
    REPAIR_TARGET_COUNT = "repair_target_count"
    REPAIR_SELF_LOOP = "repair_self_loop"
    TARGET_NOT_UUID = "target_not_uuid"
    MINT_NAME_NOT_CANONICAL = "mint_name_not_canonical"
    TARGET_ON_NON_REPAIR = "target_on_non_repair"


@dataclass(frozen=True)
class ReviewError:
    candidate_id: str | None
    rule: ReviewRule

    def __str__(self) -> str:
        return f"{self.candidate_id or '<adjudication>'}: {self.rule.value}"


def _repair_rules(entry: dict, candidate: dict) -> list[ReviewRule]:
    target, mint_name = entry.get("target_node_uuid"), entry.get("mint_name")
    if (target is None) == (mint_name is None):
        return [ReviewRule.REPAIR_TARGET_COUNT]
    if mint_name is not None:
        canonical = isinstance(mint_name, str) and _CANONICAL_MINT_NAME.fullmatch(mint_name)
        return [] if canonical else [ReviewRule.MINT_NAME_NOT_CANONICAL]
    if not isinstance(target, str) or not _UUID_SHAPE.fullmatch(target):
        return [ReviewRule.TARGET_NOT_UUID]
    if target in (candidate["node_uuid"], candidate["other_uuid"]):
        return [ReviewRule.REPAIR_SELF_LOOP]
    return []


def _entry_rules(entry: dict, candidate: dict) -> list[ReviewRule]:
    verdict = entry.get("verdict")
    if verdict not in _VERDICT_VALUES:
        return [ReviewRule.INVALID_VERDICT]
    rationale = entry.get("rationale")
    rules = [] if isinstance(rationale, str) and rationale.strip() else [ReviewRule.EMPTY_RATIONALE]
    if verdict == Verdict.REPAIR.value:
        return rules + _repair_rules(entry, candidate)
    if entry.get("target_node_uuid") is not None or entry.get("mint_name") is not None:
        rules.append(ReviewRule.TARGET_ON_NON_REPAIR)
    return rules


def validate_adjudication(census: dict, adjudication: dict,
                          census_file_name: str) -> list[ReviewError]:
    """Every way `adjudication` fails to be a complete, applicable verdict list for `census`."""
    errors = []
    if adjudication.get("census") != census_file_name:
        errors.append(ReviewError(None, ReviewRule.CENSUS_MISMATCH))
    candidates = {candidate["candidate_id"]: candidate for candidate in census["candidates"]}
    seen: set[str] = set()
    for entry in adjudication.get("adjudications", []):
        candidate_id = entry.get("candidate_id")
        if candidate_id not in candidates:
            errors.append(ReviewError(candidate_id, ReviewRule.UNKNOWN_CANDIDATE))
        elif candidate_id in seen:
            errors.append(ReviewError(candidate_id, ReviewRule.DUPLICATE_ADJUDICATION))
        else:
            seen.add(candidate_id)
            errors.extend(ReviewError(candidate_id, rule)
                          for rule in _entry_rules(entry, candidates[candidate_id]))
    errors.extend(ReviewError(candidate_id, ReviewRule.MISSING_ADJUDICATION)
                  for candidate_id in candidates if candidate_id not in seen)
    return errors


def _paired(census: dict, adjudication: dict) -> list[tuple[dict, dict]]:
    entries = {entry["candidate_id"]: entry for entry in adjudication["adjudications"]}
    return [(candidate, entries[candidate["candidate_id"]]) for candidate in census["candidates"]]


def repair_list(census: dict, adjudication: dict) -> list[dict]:
    """The REPAIR rows of a valid adjudication, keyed like reassign_edge's parameters."""
    repairs = []
    for candidate, entry in _paired(census, adjudication):
        if entry["verdict"] != Verdict.REPAIR.value:
            continue
        repair = {"candidate_id": candidate["candidate_id"], "edge_uuid": candidate["edge_uuid"],
                  "which_end": candidate["which_end"], "from_node_uuid": candidate["node_uuid"]}
        if entry.get("target_node_uuid") is not None:
            repair["new_endpoint_uuid"] = entry["target_node_uuid"]
        else:
            repair["mint_name"] = entry["mint_name"]
        repairs.append(repair)
    return repairs


def _cell(value: Any) -> str:
    if isinstance(value, list):
        value = ", ".join(str(item) for item in value)
    text = "-" if value is None or value == "" else str(value)
    return text.replace("|", "\\|").replace("\r", " ").replace("\n", " ")


def _row(cells: Iterable[Any]) -> str:
    return "| " + " | ".join(_cell(cell) for cell in cells) + " |"


def _target_text(entry: dict) -> str | None:
    if entry.get("mint_name"):
        return f"mint {entry['mint_name']}"
    return entry.get("target_node_uuid")


def _candidate_row(number: int, candidate: dict, entry: dict) -> str:
    proposal = candidate["proposal"]
    return _row([
        number, f"{candidate['node_name']} ({candidate['node_uuid']})", candidate["edge_uuid"],
        candidate["which_end"], f"{candidate['other_name']} ({candidate['other_uuid']})",
        candidate["fact"], candidate["named_numbers"], candidate["signature"],
        f"{proposal['verdict']} ({proposal['reason']})", entry["verdict"], _target_text(entry),
        entry["rationale"],
    ])


def render_review(census: dict, adjudication: dict) -> str:
    """The markdown review sheet for a valid adjudication of `census`."""
    key, probe, summary = census["key_confirmation"], census["bm25_probe"], census["summary"]
    pairs = _paired(census, adjudication)
    adjudicated = Counter(entry["verdict"] for _, entry in pairs)
    mints = [(n, c, e) for n, (c, e) in enumerate(pairs, 1) if e.get("mint_name")]
    columns = ["#", "node", "edge", "node end", "other endpoint", "fact", "named", "signature",
               "proposed", "adjudicated", "target", "rationale"]
    return "\n".join([
        f"# Task-N conflation review: {adjudication['census']}",
        "",
        f"- graph `{census['graph_key']}`, generated {census['generated_at']} by "
        f"`{census['generator']}` (schema {census['schema_version']})",
        f"- key confirmation: {key['observed_node_count']} nodes observed, "
        f"{key['expected_node_count']} expected",
        f"- BM25 probe `{probe['token']}`: fulltext_hits {probe['fulltext_hits']}, name_contains "
        f"{probe['name_contains']}, serving {probe['serving']}, error {probe['error']}",
        f"- task nodes {summary['task_nodes']}, live edge rows scanned "
        f"{summary['live_edge_rows_scanned']}, self-loops skipped {summary['self_loops_skipped']}",
        f"- foreign facts {summary['foreign_facts']}, adjacent signature "
        f"{summary['adjacent_signature']}",
        "",
        "## Verdicts",
        "",
        _row(["verdict", "proposed", "adjudicated"]),
        _row(["---"] * 3),
        *(_row([v.value, summary["proposed"][v.value], adjudicated[v.value]]) for v in Verdict),
        "",
        "## REPAIRs that need a node minted first",
        "",
        *([f"- row {n}: {e['mint_name']} for edge {c['edge_uuid']}" for n, c, e in mints]
          or ["- none"]),
        "",
        "## Candidates",
        "",
        _row(columns),
        _row(["---"] * len(columns)),
        *(_candidate_row(number, c, e) for number, (c, e) in enumerate(pairs, 1)),
    ]) + "\n"


def _load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def run_review(args: argparse.Namespace) -> int:
    census, adjudication = _load_json(args.census), _load_json(args.adjudication)
    errors = validate_adjudication(census, adjudication, args.census.name)
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    review_path = args.census.with_name(args.census.name.removesuffix(".json") + ".review.md")
    review_path.write_text(render_review(census, adjudication), encoding="utf-8")
    print(f"review sheet: {review_path}", file=sys.stderr)
    print(json.dumps(repair_list(census, adjudication), indent=2, ensure_ascii=False))
    return 0


# ── Command line ────────────────────────────────────────────────────────────


def _add_graph_options(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--graph", default="reify", help="FalkorDB graph key (default: reify)")
    parser.add_argument("--redis-url", default="redis://localhost:6379",
                        help="FalkorDB redis URL (default: redis://localhost:6379)")


def _add_artifact_options(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--census", type=Path, required=True, help="census JSON file")
    parser.add_argument("--adjudication", type=Path, required=True,
                        help="adjudication JSON file for that census")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="graph-task-conflation-census.py",
        description=__doc__.splitlines()[0],
        epilog="See the module docstring for the artifact schema and safety rules.",
    )
    commands = parser.add_subparsers(dest="command", required=True)

    census = commands.add_parser("census", help="read-only census of conflation candidates")
    _add_graph_options(census)
    census.add_argument("--expect-node-count", type=int, required=True,
                        help="node count the graph key must report (get_status graphiti_nodes)")
    census.add_argument("--out-dir", type=Path, default=REPO_ROOT / "data" / "graph-census",
                        help="directory for the census and adjudication template")
    census.add_argument("--bm25-token", default="orchestrator",
                        help="token for the BM25 serving probe (default: orchestrator)")
    census.add_argument("--batch-size", type=int, default=200,
                        help="node uuids per edge query (default: 200)")

    review = commands.add_parser("review", help="validate an adjudication and render the review sheet")
    _add_artifact_options(review)

    check = commands.add_parser("check-repairs", help="read-only status of every adjudicated REPAIR")
    _add_graph_options(check)
    _add_artifact_options(check)
    check.add_argument("--require", choices=("pending", "applied"), required=True,
                       help="state every REPAIR row must be in for exit 0")
    check.add_argument("--out", type=Path, help="also write the report here (never overwritten)")
    return parser


def _utc_now() -> datetime:
    return datetime.now(timezone.utc)


def run_census(args: argparse.Namespace, connect, clock: Callable[[], datetime]) -> int:
    now = clock().astimezone(timezone.utc)
    stem = f"{args.graph}-task-conflations-{now:%Y-%m-%dT%H-%M-%SZ}"
    census_path = args.out_dir / f"{stem}.json"
    adjudication_path = args.out_dir / f"{stem}.adjudication.json"
    _refuse_existing([census_path, adjudication_path])
    reader = FalkorReader(connect(args.redis_url), args.graph, batch_size=args.batch_size)
    census = build_census(reader, args.graph, args.expect_node_count,
                          f"{now:%Y-%m-%dT%H:%M:%SZ}", args.bm25_token)
    args.out_dir.mkdir(parents=True, exist_ok=True)
    write_new_json(census_path, census)
    write_new_json(adjudication_path, adjudication_template(census, census_path.name))
    print(_census_report(census, census_path, adjudication_path))
    return 0


_REPORTED_ERRORS = (KeyConfirmationError, GraphQueryTimeout, MissingDriverError,
                    OutputExistsError, FileNotFoundError, json.JSONDecodeError)


def main(argv: list[str] | None = None, connect=connect_falkor,
         clock: Callable[[], datetime] = _utc_now) -> int:
    args = build_parser().parse_args(argv)
    try:
        if args.command == "census":
            return run_census(args, connect, clock)
        if args.command == "review":
            return run_review(args)
        raise NotImplementedError(f"subcommand {args.command!r} is not implemented yet")
    except _REPORTED_ERRORS as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
