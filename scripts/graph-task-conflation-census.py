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
import itertools
import re
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]

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
