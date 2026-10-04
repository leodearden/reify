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
(mentions_number: any digit-bounded occurrence). The tool proposes REPAIR or
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

The live connection needs the `redis` package (dark-factory's venv python3);
the module itself is stdlib-only so --help and the tests run anywhere.
"""

import argparse
import enum
import itertools
import re
import sys
from collections.abc import Iterable, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Protocol

REPO_ROOT = Path(__file__).resolve().parents[1]
SCHEMA_VERSION = 1
GENERATOR = "scripts/graph-task-conflation-census.py"
ADJACENCY_DISTANCE = 2

# ── Task-reference grammar ──────────────────────────────────────────────────
#
# Re-expresses the measured rules of fused-memory's canonical_labels.py (word-
# glue lookbehind, '[ \t]'-padded '#'/':' separators, letter-start >=3-char
# project qualifier, single-number node-name anchor), extended with the run,
# range and Greek-suffix forms a census of free-text facts needs.

_NUMBER = "[0-9]+(?![0-9])[A-Za-z\u0370-\u03ff]?"
_RUN_JOINER = (
    "(?:[ \t]*[,/+&\\-\u2013][ \t]*(?:(?:and|or)[ \t]+)?"
    "|[ \t]+(?:and|or|to|through)[ \t]+)"
)
_MENTION_PATTERN = re.compile(
    "(?P<project>(?<![\\w-])(?:dark[ _-]?factory|df)[ \t]+)?"
    "(?<![\\w:-])tasks?(?:\\s+id)?(?:[ \t]*[#:/][ \t]*|\\s+)\\(?"
    f"(?P<run>{_NUMBER}(?:{_RUN_JOINER}#?{_NUMBER})*)",
    re.IGNORECASE,
)
_RANGE_JOINER = re.compile(
    "[A-Za-z\u0370-\u03ff]?[ \t]*(?:-|\u2013|to|through)[ \t]*#?", re.IGNORECASE
)
_MAX_RANGE_SPAN = 100
_QUALIFIED_REF_PATTERN = re.compile(
    r"(?<![\w:/.-])([A-Za-z][A-Za-z0-9_-]{2,})[ \t]*:[ \t]*([0-9]+)(?![0-9])"
)
_TASK_VOCABULARY = frozenset({"task", "tasks"})
_INTEGER = re.compile("[0-9]+")
_TASK_NODE_PREFIX = re.compile(r"\s*tasks?(?![^\W\d])", re.IGNORECASE)
_CANONICAL_TASK_NAME = re.compile(
    r"\s*tasks?(?:[ \t]*[#:][ \t]*|\s+)(?:id\s+)?([0-9]+)\s*", re.IGNORECASE
)


@dataclass(frozen=True)
class TaskRefs:
    """Task numbers a fact names: reify-local ones, and project-qualified ones."""

    local: frozenset[int]
    cross_project: frozenset[int]


def _run_numbers(run: str) -> set[int]:
    matches = list(_INTEGER.finditer(run))
    numbers = {int(m.group()) for m in matches}
    for left, right in itertools.pairwise(matches):
        if _RANGE_JOINER.fullmatch(run[left.end():right.start()]):
            low, high = int(left.group()), int(right.group())
            if 0 < high - low <= _MAX_RANGE_SPAN:
                numbers.update(range(low, high + 1))
    return numbers


def fact_task_refs(text: str) -> TaskRefs:
    """Task numbers named by task-prefixed mentions in `text` (STRICT)."""
    local: set[int] = set()
    cross_project: set[int] = set()
    for mention in _MENTION_PATTERN.finditer(text):
        bucket = cross_project if mention.group("project") else local
        bucket.update(_run_numbers(mention.group("run")))
    for ref in _QUALIFIED_REF_PATTERN.finditer(text):
        if ref.group(1).lower() not in _TASK_VOCABULARY:
            cross_project.add(int(ref.group(2)))
    return TaskRefs(local=frozenset(local), cross_project=frozenset(cross_project))


def mentions_number(text: str, number: int) -> bool:
    """Whether `number` occurs digit-bounded in `text` (BROAD), not as a decimal's head."""
    return re.search(f"(?<![0-9]){number}(?![0-9])(?!\\.[0-9])", text) is not None


def node_task_numbers(name: str) -> frozenset[int]:
    """Every integer in a Task-N-shaped node name; empty for any other name."""
    if not _TASK_NODE_PREFIX.match(name):
        return frozenset()
    return frozenset(int(digits) for digits in _INTEGER.findall(name))


def canonical_task_number(name: str) -> int | None:
    """The number of a single-number canonical task node name ('Task 1997'), else None."""
    match = _CANONICAL_TASK_NAME.fullmatch(name)
    return int(match.group(1)) if match else None


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


def connect_falkor(url: str):
    raise NotImplementedError("live connection lands with the census subcommand")


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


def main(argv: list[str] | None = None, connect=connect_falkor) -> int:
    args = build_parser().parse_args(argv)
    raise NotImplementedError(f"subcommand {args.command!r} is not implemented yet")


if __name__ == "__main__":
    sys.exit(main())
