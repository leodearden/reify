#!/usr/bin/env python3
"""
test_graph_task_conflation_census.py — stdlib unittest for
scripts/graph-task-conflation-census.py.

WHAT RUNS THIS: not the gate directly. run_all.sh discovers `test_*.sh` only,
so the discovered member is the thin wrapper
tests/infra/test_graph_task_conflation_census.sh, which invokes this file. A
bare .py here would be silently never run.

The tool's filename is hyphenated (repo script convention), so it is loaded by
path via importlib rather than by a bare `import`. Its sibling grammar module,
which these tests also exercise directly, is loaded first under its import
name, so the tool's `from task_reference_grammar import ...` resolves to it
without any sys.path edit. Every test drives the tool
in-process through its public seams (the reader Protocol and main()'s
`connect` parameter); this file spawns no subprocess and touches no live graph.
"""

import contextlib
import importlib.util
import io
import json
import re
import sys
import tempfile
import unittest
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPTS_DIR = REPO_ROOT / "scripts"


def _load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    # Register before exec: @dataclass resolves a field's type through
    # sys.modules[cls.__module__], which is None for an unregistered module.
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


grammar = _load_module("task_reference_grammar", SCRIPTS_DIR / "task_reference_grammar.py")
census_tool = _load_module("graph_task_conflation_census",
                           SCRIPTS_DIR / "graph-task-conflation-census.py")


class FactTaskRefsTest(unittest.TestCase):
    def assertLocal(self, text, expected):
        refs = grammar.fact_task_refs(text)
        self.assertEqual(refs.local, frozenset(expected), text)

    def test_single_mentions_are_case_insensitive(self):
        self.assertLocal("TASK 362 landed", {362})
        self.assertLocal("see task #967 for context", {967})
        self.assertLocal("Task: 12 is pending", {12})
        self.assertLocal("task id 1997 was cancelled", {1997})

    def test_plural_runs(self):
        self.assertLocal("Tasks 2621 and 2622 share a fixture", {2621, 2622})
        self.assertLocal("tasks (2109, 2110, 2111, 2112) were filed",
                         {2109, 2110, 2111, 2112})
        self.assertLocal("tasks 2621/2622 overlap", {2621, 2622})
        self.assertLocal("tasks 854+889 merged", {854, 889})
        self.assertLocal("tasks #12, #13 or #14", {12, 13, 14})

    def test_greek_suffixed_runs(self):
        self.assertLocal("tasks 4770α, 4771β, and 4772γ form a ladder",
                         {4770, 4771, 4772})

    def test_inclusive_range_expansion(self):
        expected = set(range(4841, 4848))
        self.assertLocal("tasks 4841-4847 retired the flake class", expected)
        self.assertLocal("tasks 4841–4847 retired the flake class", expected)
        self.assertLocal("tasks 4841 to 4847", expected)
        self.assertLocal("tasks 4841 through 4847", expected)

    def test_wide_range_keeps_endpoints_only(self):
        self.assertLocal("tasks 100-500 were swept", {100, 500})

    def test_word_glued_lookalikes_are_not_mentions(self):
        self.assertLocal("subtask 5 is done", set())
        self.assertLocal("multitask 3 pipeline", set())
        self.assertLocal("reify-task 12 label", set())
        self.assertLocal("taskforce 9 met", set())

    def test_branch_form(self):
        self.assertLocal("merged task/5026-land6 into main", {5026})

    def test_escalation_id_alone_is_not_a_mention(self):
        self.assertLocal("esc-4000-39 was resolved", set())

    def test_cross_project_refs_are_not_local(self):
        for text in ("dark_factory task 3673 repaired its graph",
                     "DF Task 3673 is the analogue",
                     "gated on dark_factory:3673",
                     "gated on dark-factory:3673"):
            refs = grammar.fact_task_refs(text)
            self.assertEqual(refs.cross_project, frozenset({3673}), text)
            self.assertEqual(refs.local, frozenset(), text)

    def test_task_word_is_not_a_project_qualifier(self):
        refs = grammar.fact_task_refs("Task: 12 is pending")
        self.assertEqual(refs.cross_project, frozenset())

    def test_newline_between_task_and_hash_is_not_a_mention(self):
        self.assertLocal("the last task\n#5 heading", set())


class MentionsNumberTest(unittest.TestCase):
    def test_broad_own_presence(self):
        for text in ("task 2590 landed",
                     "the 2590 pn1 model",
                     "see #2590",
                     "branch task-2590",
                     "2590's verify",
                     "task/2590",
                     "tasks 2590α, 2591β"):
            self.assertTrue(grammar.mentions_number(text, 2590), text)

    def test_digit_neighbours_and_decimals_are_not_presence(self):
        for text in ("task 12590", "task 25901", "ratio 2590.5", "no number here"):
            self.assertFalse(grammar.mentions_number(text, 2590), text)


class NodeTaskNumbersTest(unittest.TestCase):
    def test_task_named_nodes(self):
        cases = {
            "tasks 854+889": {854, 889},
            "Task #1004": {1004},
            "tasks 4770α/4771β/4772γ": {4770, 4771, 4772},
            "task 12 (content-hash caching)": {12},
            "task/540 branch": {540},
            "Task 2590": {2590},
            "tasks 4841–4847": {4841, 4847},
        }
        for name, expected in cases.items():
            self.assertEqual(grammar.node_task_numbers(name), frozenset(expected), name)

    def test_non_task_names(self):
        for name in ("Taskmaster", "task tree", "task_id", "taskset 5", "Orchestrator 12"):
            self.assertEqual(grammar.node_task_numbers(name), frozenset(), name)


class CanonicalTaskNumberTest(unittest.TestCase):
    def test_canonical_names(self):
        for name in ("Task 1997", "task 1997", "task #1997", "Task: 1997",
                     "task id 1997", "  Task 1997  "):
            self.assertEqual(grammar.canonical_task_number(name), 1997, name)

    def test_non_canonical_names(self):
        for name in ("task/1997", "tasks 1997/1998", "task 1997.2",
                     "task 1997 (x)", "Taskmaster", "task1997"):
            self.assertIsNone(grammar.canonical_task_number(name), name)


# ── Census classification over a fake graph ─────────────────────────────────

GENERATED_AT = "2026-10-04T00:00:00Z"


def _node_uuid(n):
    return f"{n:08x}-0000-4000-8000-000000000000"


def _edge_uuid(n):
    return f"{n:08x}-0000-4000-8000-eeeeeeeeeeee"


@dataclass(frozen=True)
class FakeEdge:
    uuid: str
    source: str
    target: str
    fact: str
    invalid_at: str | None = None
    expired_at: str | None = None


class FakeReader:
    """In-memory GraphReader: answers from a node map and an edge list.

    edges_touching mirrors the live UNDIRECTED match: one row per (edge,
    requested endpoint), and it does NOT drop tombstoned edges, so the
    census's own tombstone guard is what gets exercised.
    """

    def __init__(self, nodes, edges, node_count=None, bm25=None):
        self.nodes = dict(nodes)
        self.edges = list(edges)
        self.reported_node_count = len(self.nodes) if node_count is None else node_count
        self.bm25 = bm25 or census_tool.Bm25Probe(
            token="orchestrator", fulltext_hits=547, name_contains=92, error=None)
        self.endpoints = {edge.uuid: census_tool.EdgeEndpoints(edge.source, edge.target,
                                                               edge.invalid_at, edge.expired_at)
                          for edge in self.edges}
        self.existing = set(self.nodes)
        self.calls = []

    def node_count(self):
        self.calls.append("node_count")
        return self.reported_node_count

    def task_named_nodes(self):
        self.calls.append("task_named_nodes")
        return [(uuid, name) for uuid, name in self.nodes.items()
                if name.lower().startswith("task")]

    def edges_touching(self, node_uuids):
        self.calls.append("edges_touching")
        wanted = set(node_uuids)
        rows = []
        for edge in self.edges:
            for which_end, node, other in (("source", edge.source, edge.target),
                                           ("target", edge.target, edge.source)):
                if node not in wanted or (which_end == "target" and edge.source == edge.target):
                    continue
                rows.append(census_tool.EdgeRow(
                    node_uuid=node, edge_uuid=edge.uuid, fact=edge.fact,
                    which_end=which_end, other_uuid=other, other_name=self.nodes[other],
                    created_at="2026-08-01T00:00:00Z", invalid_at=edge.invalid_at,
                    expired_at=edge.expired_at, first_episode="ep-" + edge.uuid[:8],
                    episode_count=1))
        return rows

    def bm25_probe(self, token):
        self.calls.append("bm25_probe")
        return self.bm25

    def edge_endpoints(self, edge_uuid):
        self.calls.append(("edge_endpoints", edge_uuid))
        return self.endpoints.get(edge_uuid)

    def node_exists(self, uuid):
        self.calls.append(("node_exists", uuid))
        return uuid in self.existing


class FakeGraph:
    """Builder for a FakeReader's nodes and edges with uuid-shaped ids."""

    def __init__(self):
        self.nodes = {}
        self.edges = []

    def node(self, name):
        uuid = _node_uuid(len(self.nodes) + 1)
        self.nodes[uuid] = name
        return uuid

    def edge(self, source, target, fact, **tombstone):
        uuid = _edge_uuid(len(self.edges) + 1)
        self.edges.append(FakeEdge(uuid, source, target, fact, **tombstone))
        return uuid

    def reader(self, **kwargs):
        return FakeReader(self.nodes, self.edges, **kwargs)


def _build(reader, expected_node_count=None):
    expected = reader.reported_node_count if expected_node_count is None else expected_node_count
    return census_tool.build_census(reader, graph_key="reify", expected_node_count=expected,
                                    generated_at=GENERATED_AT)


def _candidates(census):
    return {c["candidate_id"]: c for c in census["candidates"]}


REPAIR = census_tool.Verdict.REPAIR.value
RECORD_ONLY = census_tool.Verdict.RECORD_ONLY.value
NOT_A_CONFLATION = census_tool.Verdict.NOT_A_CONFLATION.value


class ProposalWorld:
    """One fixture graph exercising every proposal reason."""

    def __init__(self):
        g = self.graph = FakeGraph()
        self.t3019 = g.node("Task 3019")
        self.t3017 = g.node("Task 3017")
        self.t2919 = g.node("Task 2919")
        self.e_unique_source = g.edge(self.t3019, self.t3017, "Task 3017 is not task 2919")
        self.t4100 = g.node("Task 4100")
        self.harness = g.node("Verify harness")
        self.e_unique_target = g.edge(self.harness, self.t4100, "task 2919 owns the verify harness")
        self.t2590 = g.node("Task 2590")
        self.t2591 = g.node("task 2591")
        self.e_unary = g.edge(self.t2590, self.t2591,
                              "Task 2591 was created as a new pending task")
        self.t500 = g.node("Task 500")
        self.orchestrator = g.node("Orchestrator")
        self.e_absent = g.edge(self.t500, self.orchestrator, "Task 502 restarted the orchestrator")
        self.t1999 = g.node("Task 1999")
        self.t1997_a = g.node("Task 1997")
        self.t1997_b = g.node("task 1997")
        self.widget = g.node("Widget")
        self.e_ambiguous_nodes = g.edge(self.t1999, self.widget, "Task 1997 owns the widget")
        self.t700 = g.node("Task 700")
        self.foo = g.node("Foo crate")
        self.e_ambiguous_numbers = g.edge(self.t700, self.foo, "Tasks 710 and 720 both touch foo")
        self.census = _build(g.reader())
        self.by_id = _candidates(self.census)

    def candidate(self, edge, node):
        return self.by_id[f"{edge}@{node}"]


class Bm25ProbeTest(unittest.TestCase):
    def probe(self, hits, contains, error=None):
        return census_tool.Bm25Probe(token="orchestrator", fulltext_hits=hits,
                                     name_contains=contains, error=error)

    def test_serving(self):
        self.assertTrue(self.probe(547, 92).serving)
        self.assertTrue(self.probe(0, 0).serving)
        self.assertFalse(self.probe(0, 92).serving)
        self.assertFalse(self.probe(None, 92, error="boom").serving)


class KeyConfirmationTest(unittest.TestCase):
    def test_mismatch_raises_before_any_other_read(self):
        g = FakeGraph()
        g.node("Task 1")
        reader = g.reader(node_count=31835)
        with self.assertRaises(census_tool.KeyConfirmationError) as ctx:
            _build(reader, expected_node_count=31834)
        self.assertIn("31835", str(ctx.exception))
        self.assertIn("31834", str(ctx.exception))
        self.assertEqual(reader.calls, ["node_count"])


class CandidateExclusionTest(unittest.TestCase):
    def test_tombstoned_rows_are_dropped_and_not_counted(self):
        g = FakeGraph()
        t1 = g.node("Task 1")
        other = g.node("Widget")
        g.edge(t1, other, "Task 50 owns the widget", invalid_at="2026-09-01T00:00:00Z")
        g.edge(t1, other, "Task 51 owns the widget", expired_at="2026-09-01T00:00:00Z")
        live = g.edge(t1, other, "Task 52 owns the widget")
        census = _build(g.reader())
        self.assertEqual(census["summary"]["live_edge_rows_scanned"], 1)
        self.assertEqual(list(_candidates(census)), [f"{live}@{t1}"])

    def test_own_number_presence_excludes(self):
        g = FakeGraph()
        t100 = g.node("Task 100")
        t2622 = g.node("Task 2622")
        other = g.node("Widget")
        g.edge(t100, other, "Task 100 depends on task 200")
        g.edge(t100, other, "the 100 harness relates to task 200")
        g.edge(other, t100, "see #100; task 200 too")
        g.edge(t2622, other, "Tasks 2621 and 2622 share a fixture")
        census = _build(g.reader())
        self.assertEqual(census["candidates"], [])
        self.assertEqual(census["summary"]["live_edge_rows_scanned"], 4)

    def test_own_number_named_inside_a_range_excludes(self):
        g = FakeGraph()
        t4843 = g.node("Task 4843")
        g.edge(t4843, g.node("Flake ledger"), "tasks 4841-4847 retired the flake class")
        self.assertEqual(_build(g.reader())["candidates"], [])

    def test_facts_without_a_local_task_number_are_not_candidates(self):
        g = FakeGraph()
        t3670 = g.node("Task 3670")
        other = g.node("Widget")
        g.edge(t3670, other, "The orchestrator restarted")
        g.edge(t3670, other, "mirrors dark_factory task 3673")
        g.edge(t3670, other, "gated on dark_factory:3671")
        self.assertEqual(_build(g.reader())["candidates"], [])

    def test_self_loops_are_skipped_and_counted(self):
        g = FakeGraph()
        t1 = g.node("Task 1")
        g.edge(t1, t1, "Task 7 loops")
        census = _build(g.reader())
        self.assertEqual(census["candidates"], [])
        self.assertEqual(census["summary"]["self_loops_skipped"], 1)

    def test_non_task_names_are_not_in_the_population(self):
        g = FakeGraph()
        master = g.node("Taskmaster")
        other = g.node("Widget")
        g.edge(master, other, "Task 5 uses taskmaster")
        census = _build(g.reader())
        self.assertEqual(census["summary"]["task_nodes"], 0)
        self.assertEqual(census["candidates"], [])


class ProposalTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.world = ProposalWorld()

    def test_unique_target_on_the_source_end(self):
        w = self.world
        c = w.candidate(w.e_unique_source, w.t3019)
        self.assertEqual(c["which_end"], "source")
        self.assertEqual(c["other_uuid"], w.t3017)
        self.assertEqual(c["named_numbers"], [2919, 3017])
        self.assertEqual(c["own_numbers"], [3019])
        self.assertEqual(c["proposal"], {
            "verdict": REPAIR,
            "reason": census_tool.ProposalReason.UNIQUE_TARGET.value,
            "target_numbers": [2919],
            "target_node_uuids": [w.t2919],
            "mint_name": None,
        })

    def test_unique_target_on_the_target_end(self):
        w = self.world
        c = w.candidate(w.e_unique_target, w.t4100)
        self.assertEqual(c["which_end"], "target")
        self.assertEqual(c["proposal"]["verdict"], REPAIR)
        self.assertEqual(c["proposal"]["target_node_uuids"], [w.t2919])

    def test_unary_fact_about_the_other_endpoint(self):
        w = self.world
        c = w.candidate(w.e_unary, w.t2590)
        self.assertEqual(c["proposal"], {
            "verdict": RECORD_ONLY,
            "reason": census_tool.ProposalReason.UNARY_ABOUT_OTHER_ENDPOINT.value,
            "target_numbers": [],
            "target_node_uuids": [],
            "mint_name": None,
        })

    def test_absent_target_proposes_a_canonical_mint(self):
        w = self.world
        c = w.candidate(w.e_absent, w.t500)
        self.assertEqual(c["proposal"], {
            "verdict": REPAIR,
            "reason": census_tool.ProposalReason.TARGET_NODE_ABSENT.value,
            "target_numbers": [502],
            "target_node_uuids": [],
            "mint_name": "Task 502",
        })

    def test_two_canonical_nodes_are_ambiguous(self):
        w = self.world
        c = w.candidate(w.e_ambiguous_nodes, w.t1999)
        self.assertEqual(c["proposal"], {
            "verdict": RECORD_ONLY,
            "reason": census_tool.ProposalReason.AMBIGUOUS_TARGET_NODES.value,
            "target_numbers": [1997],
            "target_node_uuids": sorted([w.t1997_a, w.t1997_b]),
            "mint_name": None,
        })

    def test_two_surviving_numbers_are_ambiguous(self):
        w = self.world
        c = w.candidate(w.e_ambiguous_numbers, w.t700)
        self.assertEqual(c["proposal"], {
            "verdict": RECORD_ONLY,
            "reason": census_tool.ProposalReason.AMBIGUOUS_TARGET_NUMBERS.value,
            "target_numbers": [710, 720],
            "target_node_uuids": [],
            "mint_name": None,
        })

    def test_candidates_carry_their_evidence(self):
        w = self.world
        c = w.candidate(w.e_absent, w.t500)
        self.assertEqual(c["node_name"], "Task 500")
        self.assertEqual(c["edge_uuid"], w.e_absent)
        self.assertEqual(c["other_name"], "Orchestrator")
        self.assertEqual(c["fact"], "Task 502 restarted the orchestrator")
        self.assertEqual(c["first_episode"], "ep-" + w.e_absent[:8])
        self.assertEqual(c["episode_count"], 1)

    def test_signature_marks_adjacent_numbers(self):
        w = self.world
        signatures = {cid: c["signature"] for cid, c in w.by_id.items()}
        self.assertEqual(signatures[f"{w.e_unique_source}@{w.t3019}"], "adjacent")
        self.assertEqual(signatures[f"{w.e_unique_target}@{w.t4100}"], "non_adjacent")
        self.assertEqual(signatures[f"{w.e_unary}@{w.t2590}"], "adjacent")
        self.assertEqual(signatures[f"{w.e_absent}@{w.t500}"], "adjacent")
        self.assertEqual(signatures[f"{w.e_ambiguous_numbers}@{w.t700}"], "non_adjacent")

    def test_summary_counts(self):
        summary = self.world.census["summary"]
        self.assertEqual(summary["task_nodes"], 11)
        self.assertEqual(summary["live_edge_rows_scanned"], 8)
        self.assertEqual(summary["self_loops_skipped"], 0)
        self.assertEqual(summary["foreign_facts"], 6)
        self.assertEqual(summary["adjacent_signature"], 4)
        self.assertEqual(summary["proposed"],
                         {REPAIR: 3, RECORD_ONLY: 3, NOT_A_CONFLATION: 0})

    def test_header(self):
        census = self.world.census
        self.assertEqual(census["schema_version"], 1)
        self.assertEqual(census["generator"], "scripts/graph-task-conflation-census.py")
        self.assertEqual(census["graph_key"], "reify")
        self.assertEqual(census["generated_at"], GENERATED_AT)
        nodes = len(self.world.graph.nodes)
        self.assertEqual(census["key_confirmation"],
                         {"expected_node_count": nodes, "observed_node_count": nodes})
        self.assertEqual(census["bm25_probe"], {
            "token": "orchestrator", "fulltext_hits": 547, "name_contains": 92,
            "serving": True, "error": None,
        })


class DeterminismTest(unittest.TestCase):
    def test_order_and_ids_are_input_order_independent(self):
        world = ProposalWorld()
        g = world.graph
        reversed_reader = FakeReader(dict(reversed(list(g.nodes.items()))),
                                     list(reversed(g.edges)))
        again = _build(reversed_reader)
        self.assertEqual(json.dumps(again, sort_keys=True),
                         json.dumps(world.census, sort_keys=True))
        keys = [(min(c["own_numbers"]), c["node_uuid"], c["edge_uuid"])
                for c in world.census["candidates"]]
        self.assertEqual(keys, sorted(keys))
        for c in world.census["candidates"]:
            self.assertEqual(c["candidate_id"], f"{c['edge_uuid']}@{c['node_uuid']}")


class AdjudicationTemplateTest(unittest.TestCase):
    def test_template_prefills_proposals(self):
        world = ProposalWorld()
        census = world.census
        template = census_tool.adjudication_template(census, "reify-task-conflations-x.json")
        self.assertEqual(template["census"], "reify-task-conflations-x.json")
        entries = template["adjudications"]
        self.assertEqual([e["candidate_id"] for e in entries],
                         [c["candidate_id"] for c in census["candidates"]])
        by_id = {e["candidate_id"]: e for e in entries}
        self.assertEqual(by_id[f"{world.e_unique_source}@{world.t3019}"], {
            "candidate_id": f"{world.e_unique_source}@{world.t3019}",
            "verdict": REPAIR, "target_node_uuid": world.t2919,
            "mint_name": None, "rationale": "",
        })
        self.assertEqual(by_id[f"{world.e_absent}@{world.t500}"]["mint_name"], "Task 502")
        self.assertIsNone(by_id[f"{world.e_absent}@{world.t500}"]["target_node_uuid"])
        ambiguous = by_id[f"{world.e_ambiguous_nodes}@{world.t1999}"]
        self.assertEqual(ambiguous["verdict"], RECORD_ONLY)
        self.assertIsNone(ambiguous["target_node_uuid"])
        self.assertIsNone(ambiguous["mint_name"])


# ── Live-reader seam over a fake redis-like connection ──────────────────────

_UUID_LITERAL = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
_LIST_PARAMETER = re.compile(r"CYPHER uuids=(\S+) ")


class FakeResponseError(Exception):
    """Stands in for redis.exceptions.ResponseError, which carries only a message."""


def _table(records):
    header = list(records[0]) if records else []
    return header, [[record[column] for column in header] for record in records]


class FakeFalkorConnection:
    """A redis-like connection answering GRAPH.RO_QUERY from a FakeGraph.

    Replies have FalkorDB's verbose shape [header, rows, stats]. Routing on
    Cypher keywords below is fixture plumbing only: tests assert on the verb
    and key of each recorded call, never on query text. The one exception is
    deliberate: a list parameter must parse as JSON, because a single-quoted
    list parameter crashed the shared FalkorDB 4.18.0 server.
    """

    def __init__(self, graph, fulltext_error=None, failures=()):
        self.graph = graph
        self.fulltext_error = fulltext_error
        self.failures = list(failures)
        self.calls = []

    def execute_command(self, *args):
        self.calls.append(args)
        if self.failures:
            raise self.failures.pop(0)
        header, rows = self._answer(args[2])
        return [header, rows, ["Cached execution: 0",
                               "Query internal execution time: 0.1 milliseconds"]]

    def _answer(self, query):
        if "db.idx.fulltext.queryNodes" in query:
            if self.fulltext_error:
                raise FakeResponseError(self.fulltext_error)
            return ["hits"], [[547]]
        if "CONTAINS" in query:
            return ["contains"], [[92]]
        if "STARTS WITH" in query:
            return _table([{"uuid": uuid, "name": name} for uuid, name in self.graph.nodes.items()
                           if name.lower().startswith("task")])
        parameter = _LIST_PARAMETER.match(query)
        if parameter:
            return _table(self._edge_rows(set(json.loads(parameter.group(1)))))
        literals = _UUID_LITERAL.findall(query)
        if "RELATES_TO" in query:
            return _table(self._endpoints(literals[0]))
        if literals:
            return ["count"], [[int(literals[0] in self.graph.nodes)]]
        return ["count"], [[len(self.graph.nodes)]]

    def _edge_rows(self, wanted):
        rows = []
        for edge in self.graph.edges:
            if edge.invalid_at or edge.expired_at:
                continue
            ends = {(edge.source, edge.target), (edge.target, edge.source)}
            for node, other in sorted(ends):
                if node in wanted:
                    rows.append({
                        "node_uuid": node, "edge_uuid": edge.uuid, "fact": edge.fact,
                        "source_uuid": edge.source, "other_uuid": other,
                        "other_name": self.graph.nodes[other],
                        "created_at": "2026-08-01T00:00:00Z", "invalid_at": None,
                        "expired_at": None, "first_episode": "ep-" + edge.uuid[:8],
                        "episode_count": 2})
        return rows

    def _endpoints(self, edge_uuid):
        return [{"source_uuid": e.source, "target_uuid": e.target,
                 "invalid_at": e.invalid_at, "expired_at": e.expired_at}
                for e in self.graph.edges if e.uuid == edge_uuid]


FIXED_NOW = datetime(2026, 10, 4, 5, 6, 7, tzinfo=timezone.utc)
CENSUS_NAME = "reify-task-conflations-2026-10-04T05-06-07Z.json"
ADJUDICATION_NAME = "reify-task-conflations-2026-10-04T05-06-07Z.adjudication.json"


def _run_main(argv, connection, clock=lambda: FIXED_NOW):
    stdout, stderr = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
        code = census_tool.main(argv, connect=lambda url: connection, clock=clock)
    return code, stdout.getvalue(), stderr.getvalue()


class FalkorReaderTest(unittest.TestCase):
    def setUp(self):
        self.world = ProposalWorld()
        self.conn = FakeFalkorConnection(self.world.graph)
        self.reader = census_tool.FalkorReader(self.conn, graph_key="reify", batch_size=2,
                                               retry_delay=0)

    def assertEveryCallIsReadOnly(self, calls):
        self.assertTrue(calls)
        for call in calls:
            self.assertEqual(call[:2], ("GRAPH.RO_QUERY", "reify"), call)

    def test_every_public_method_and_the_census_cli_issue_only_ro_queries(self):
        w = self.world
        self.assertEqual(self.reader.node_count(), len(w.graph.nodes))
        self.assertIn((w.t2590, "Task 2590"), self.reader.task_named_nodes())
        rows = self.reader.edges_touching([w.t3019, w.t3017, w.t2590])
        self.assertEqual({(r.node_uuid, r.which_end) for r in rows if r.edge_uuid == w.e_unique_source},
                         {(w.t3019, "source"), (w.t3017, "target")})
        self.assertTrue(self.reader.bm25_probe("orchestrator").serving)
        self.assertEqual(self.reader.edge_endpoints(w.e_unary),
                         census_tool.EdgeEndpoints(w.t2590, w.t2591, None, None))
        self.assertIsNone(self.reader.edge_endpoints(_edge_uuid(999)))
        self.assertTrue(self.reader.node_exists(w.t2919))
        self.assertFalse(self.reader.node_exists(_node_uuid(999)))
        with tempfile.TemporaryDirectory() as out:
            code, _, _ = _run_main(["census", "--expect-node-count", str(len(w.graph.nodes)),
                                    "--out-dir", out], self.conn)
            self.assertEqual(code, 0)
            census_path = Path(out) / CENSUS_NAME
            adjudication_path = Path(out) / ADJUDICATION_NAME
            adjudication = json.loads(adjudication_path.read_text(encoding="utf-8"))
            for entry in adjudication["adjudications"]:
                entry["rationale"] = "checked"
            adjudication_path.write_text(json.dumps(adjudication), encoding="utf-8")
            calls_before = len(self.conn.calls)
            _run_main(["check-repairs", "--census", str(census_path),
                       "--adjudication", str(adjudication_path), "--require", "pending"],
                      self.conn)
            self.assertGreater(len(self.conn.calls), calls_before)
        self.assertEveryCallIsReadOnly(self.conn.calls)

    def test_edges_are_read_in_batches(self):
        w = self.world
        before = len(self.conn.calls)
        rows = self.reader.edges_touching([w.t3019, w.t3017, w.t2590, w.t500, w.t700])
        self.assertEqual(len(self.conn.calls) - before, 3)
        self.assertEqual({r.edge_uuid for r in rows},
                         {w.e_unique_source, w.e_unary, w.e_absent, w.e_ambiguous_numbers})
        unary = next(r for r in rows if r.edge_uuid == w.e_unary)
        self.assertEqual((unary.first_episode, unary.episode_count), ("ep-" + w.e_unary[:8], 2))

    def test_list_parameters_are_double_quoted_json(self):
        self.reader.edges_touching([self.world.t3019, self.world.t2590])
        queries = [call[2] for call in self.conn.calls]
        self.assertTrue(queries)
        for query in queries:
            parameter = _LIST_PARAMETER.match(query)
            if parameter is None:
                self.fail(f"no list parameter in {query!r}")
            self.assertNotIn("'", parameter.group(1))

    def test_a_timeout_is_retried(self):
        self.conn.failures = [FakeResponseError("Query timed out")]
        self.assertEqual(self.reader.node_count(), len(self.world.graph.nodes))
        self.assertEqual(len(self.conn.calls), 2)

    def test_persistent_timeouts_raise_after_max_attempts(self):
        self.conn.failures = [FakeResponseError("Query timed out")] * 5
        with self.assertRaises(census_tool.GraphQueryTimeout):
            self.reader.node_count()
        self.assertEqual(len(self.conn.calls), 3)

    def test_other_errors_propagate_without_retry(self):
        self.conn.failures = [FakeResponseError("FalkorDB does not currently support =~")]
        with self.assertRaises(FakeResponseError):
            self.reader.node_count()
        self.assertEqual(len(self.conn.calls), 1)

    def test_untrusted_uuids_are_refused_before_any_query(self):
        for bad in ('abc") RETURN 1 //', "deadbeef' OR 1=1", ""):
            with self.assertRaises(ValueError):
                self.reader.edges_touching([self.world.t3019, bad])
            with self.assertRaises(ValueError):
                self.reader.node_exists(bad)
            with self.assertRaises(ValueError):
                self.reader.edge_endpoints(bad)
        self.assertEqual(self.conn.calls, [])

    def test_untrusted_probe_token_is_refused_before_any_query(self):
        with self.assertRaises(ValueError):
            self.reader.bm25_probe("x') RETURN 1 //")
        self.assertEqual(self.conn.calls, [])

    def test_a_failing_fulltext_call_is_recorded_not_fatal(self):
        self.conn.fulltext_error = "Procedure db.idx.fulltext.queryNodes failed"
        probe = self.reader.bm25_probe("orchestrator")
        self.assertIsNone(probe.fulltext_hits)
        self.assertFalse(probe.serving)
        self.assertIn("queryNodes failed", probe.error)
        census = census_tool.build_census(self.reader, "reify", len(self.world.graph.nodes),
                                          GENERATED_AT)
        self.assertFalse(census["bm25_probe"]["serving"])
        self.assertEqual(census["summary"]["foreign_facts"], 6)


class CensusCliTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.out = Path(self._tmp.name) / "graph-census"
        g = self.graph = FakeGraph()
        self.t10 = g.node("Task 10")
        self.t12 = g.node("Task 12")
        g.edge(self.t10, g.node("Ladder"), "tasks 12α, 13β form the ladder")
        self.conn = FakeFalkorConnection(g)

    def census(self, expect=None, clock=lambda: FIXED_NOW):
        expect = len(self.graph.nodes) if expect is None else expect
        return _run_main(["census", "--expect-node-count", str(expect),
                          "--out-dir", str(self.out), "--batch-size", "2"], self.conn, clock)

    def test_writes_the_census_and_its_adjudication_template(self):
        code, stdout, _ = self.census()
        self.assertEqual(code, 0)
        self.assertEqual(sorted(p.name for p in self.out.iterdir()),
                         sorted([CENSUS_NAME, ADJUDICATION_NAME]))
        census_text = (self.out / CENSUS_NAME).read_text(encoding="utf-8")
        adjudication_text = (self.out / ADJUDICATION_NAME).read_text(encoding="utf-8")
        for text in (census_text, adjudication_text):
            self.assertTrue(text.endswith("\n"))
        self.assertIn("12α, 13β", census_text)
        census = json.loads(census_text)
        self.assertEqual(census["generated_at"], "2026-10-04T05:06:07Z")
        self.assertEqual(census["summary"]["foreign_facts"], 1)
        self.assertEqual(json.loads(adjudication_text)["census"], CENSUS_NAME)
        self.assertIn(CENSUS_NAME, stdout)

    def test_a_key_mismatch_writes_nothing_and_names_both_counts(self):
        nodes = len(self.graph.nodes)
        code, _, stderr = self.census(expect=nodes + 1)
        self.assertNotEqual(code, 0)
        self.assertFalse(self.out.exists() and any(self.out.iterdir()))
        self.assertIn(str(nodes), stderr)
        self.assertIn(str(nodes + 1), stderr)

    def test_existing_output_is_never_overwritten(self):
        self.assertEqual(self.census()[0], 0)
        before = {p.name: p.read_bytes() for p in self.out.iterdir()}
        self.graph.edge(self.t10, self.t12, "Task 14 also appears")
        code, _, stderr = self.census()
        self.assertNotEqual(code, 0)
        self.assertIn(CENSUS_NAME, stderr)
        self.assertEqual({p.name: p.read_bytes() for p in self.out.iterdir()}, before)


class ReviewFixture(unittest.TestCase):
    """A ProposalWorld census plus a fully valid adjudication of it."""

    census_name = "reify-task-conflations-2026-10-04T05-06-07Z.json"

    def setUp(self):
        self.world = ProposalWorld()
        self.census = self.world.census
        self.adjudication = census_tool.adjudication_template(self.census, self.census_name)
        for entry in self.adjudication["adjudications"]:
            entry["rationale"] = "fact is about the named task, checked against its episode"
        self.unique_id = f"{self.world.e_unique_source}@{self.world.t3019}"
        self.mint_id = f"{self.world.e_absent}@{self.world.t500}"
        self.unary_id = f"{self.world.e_unary}@{self.world.t2590}"

    def entry(self, candidate_id):
        return next(e for e in self.adjudication["adjudications"]
                    if e["candidate_id"] == candidate_id)

    def rules(self):
        errors = census_tool.validate_adjudication(self.census, self.adjudication, self.census_name)
        return {(error.candidate_id, error.rule) for error in errors}


Rule = census_tool.ReviewRule


class ValidateAdjudicationTest(ReviewFixture):
    def test_a_fully_valid_adjudication_has_no_errors(self):
        self.assertEqual(self.rules(), set())

    def test_census_mismatch(self):
        self.adjudication["census"] = "reify-task-conflations-other.json"
        self.assertEqual(self.rules(), {(None, Rule.CENSUS_MISMATCH)})

    def test_missing_adjudication(self):
        self.adjudication["adjudications"].remove(self.entry(self.unary_id))
        self.assertEqual(self.rules(), {(self.unary_id, Rule.MISSING_ADJUDICATION)})

    def test_unknown_candidate(self):
        stray = dict(self.entry(self.unary_id), candidate_id="feedface-0000@deadbeef-0000")
        self.adjudication["adjudications"].append(stray)
        self.assertEqual(self.rules(), {("feedface-0000@deadbeef-0000", Rule.UNKNOWN_CANDIDATE)})

    def test_duplicate_adjudication(self):
        self.adjudication["adjudications"].append(dict(self.entry(self.unary_id)))
        self.assertEqual(self.rules(), {(self.unary_id, Rule.DUPLICATE_ADJUDICATION)})

    def test_invalid_verdict(self):
        self.entry(self.unary_id)["verdict"] = "FIX"
        self.assertEqual(self.rules(), {(self.unary_id, Rule.INVALID_VERDICT)})

    def test_empty_rationale(self):
        self.entry(self.unary_id)["rationale"] = "  \n\t"
        self.entry(self.mint_id)["rationale"] = ""
        self.assertEqual(self.rules(), {(self.unary_id, Rule.EMPTY_RATIONALE),
                                        (self.mint_id, Rule.EMPTY_RATIONALE)})

    def test_repair_needs_exactly_one_target(self):
        self.entry(self.unique_id)["mint_name"] = "Task 2919"
        self.entry(self.mint_id)["mint_name"] = None
        self.assertEqual(self.rules(), {(self.unique_id, Rule.REPAIR_TARGET_COUNT),
                                        (self.mint_id, Rule.REPAIR_TARGET_COUNT)})

    def test_repair_onto_either_endpoint_is_a_self_loop(self):
        self.entry(self.unique_id)["target_node_uuid"] = self.world.t3017
        self.entry(self.mint_id).update(mint_name=None, target_node_uuid=self.world.t500)
        self.assertEqual(self.rules(), {(self.unique_id, Rule.REPAIR_SELF_LOOP),
                                        (self.mint_id, Rule.REPAIR_SELF_LOOP)})

    def test_repair_may_target_a_non_task_node(self):
        self.entry(self.mint_id).update(mint_name=None, target_node_uuid=self.world.harness)
        self.assertEqual(self.rules(), set())

    def test_repair_target_must_be_uuid_shaped(self):
        self.entry(self.unique_id)["target_node_uuid"] = "Task 2919"
        self.assertEqual(self.rules(), {(self.unique_id, Rule.TARGET_NOT_UUID)})

    def test_mint_name_must_be_canonical(self):
        for name in ("task #502", "Task 502 ", "task 502", "Task  502"):
            self.entry(self.mint_id)["mint_name"] = name
            self.assertEqual(self.rules(), {(self.mint_id, Rule.MINT_NAME_NOT_CANONICAL)}, name)

    def test_target_on_non_repair(self):
        self.entry(self.unary_id)["target_node_uuid"] = self.world.t2919
        self.entry(self.mint_id).update(verdict=NOT_A_CONFLATION)
        self.assertEqual(self.rules(), {(self.unary_id, Rule.TARGET_ON_NON_REPAIR),
                                        (self.mint_id, Rule.TARGET_ON_NON_REPAIR)})


class RenderReviewTest(ReviewFixture):
    def test_review_sheet_carries_header_counts_and_every_row(self):
        self.entry(self.unary_id)["rationale"] = "unary | about the other endpoint\nno target"
        sheet = census_tool.render_review(self.census, self.adjudication)
        for value in ("reify", GENERATED_AT, "orchestrator", "547", self.census_name):
            self.assertIn(value, sheet)
        for candidate in self.census["candidates"]:
            self.assertIn(candidate["edge_uuid"], sheet)
            self.assertIn(candidate["node_name"], sheet)
        self.assertIn("Task 502 restarted the orchestrator", sheet)
        self.assertIn("Task 502", sheet)
        self.assertIn(self.world.t2919, sheet)
        self.assertIn(census_tool.ProposalReason.UNARY_ABOUT_OTHER_ENDPOINT.value, sheet)

    def test_cells_cannot_break_the_table(self):
        self.entry(self.unary_id)["rationale"] = "unary | about the other endpoint\nno target"
        sheet = census_tool.render_review(self.census, self.adjudication)
        lines = [line for line in sheet.splitlines() if line.startswith("|")]
        row = next(line for line in lines if self.world.e_unary in line)
        plain_row = next(line for line in lines if self.world.e_absent in line)
        self.assertIn("unary \\| about the other endpoint", row)
        self.assertIn("no target", row)
        self.assertEqual(row.replace("\\|", "").count("|"), plain_row.count("|"))


class RepairListTest(ReviewFixture):
    def test_only_repair_rows_with_reassign_edge_shaped_keys(self):
        repairs = census_tool.repair_list(self.census, self.adjudication)
        by_id = {r["candidate_id"]: r for r in repairs}
        self.assertEqual(set(by_id), {self.unique_id, self.mint_id,
                                      f"{self.world.e_unique_target}@{self.world.t4100}"})
        self.assertEqual(by_id[self.unique_id], {
            "candidate_id": self.unique_id, "edge_uuid": self.world.e_unique_source,
            "which_end": "source", "from_node_uuid": self.world.t3019,
            "new_endpoint_uuid": self.world.t2919,
        })
        self.assertEqual(by_id[self.mint_id], {
            "candidate_id": self.mint_id, "edge_uuid": self.world.e_absent,
            "which_end": "source", "from_node_uuid": self.world.t500, "mint_name": "Task 502",
        })


class ReviewCliTest(ReviewFixture):
    def setUp(self):
        super().setUp()
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.dir = Path(self._tmp.name)
        self.census_path = self.dir / self.census_name
        self.adjudication_path = self.dir / self.census_name.replace(".json", ".adjudication.json")
        self.review_path = self.dir / self.census_name.replace(".json", ".review.md")
        census_tool.write_new_json(self.census_path, self.census)

    def review(self):
        self.adjudication_path.write_text(json.dumps(self.adjudication), encoding="utf-8")
        return _run_main(["review", "--census", str(self.census_path),
                          "--adjudication", str(self.adjudication_path)], connection=None)

    def test_valid_adjudication_writes_the_sheet_and_prints_repairs(self):
        code, stdout, _ = self.review()
        self.assertEqual(code, 0)
        self.assertIn(self.world.e_absent, self.review_path.read_text(encoding="utf-8"))
        self.assertEqual(json.loads(stdout),
                         census_tool.repair_list(self.census, self.adjudication))

    def test_review_re_renders_after_an_edit(self):
        self.assertEqual(self.review()[0], 0)
        self.entry(self.unary_id)["rationale"] = "re-adjudicated after reading the episode"
        self.assertEqual(self.review()[0], 0)
        self.assertIn("re-adjudicated after reading the episode",
                      self.review_path.read_text(encoding="utf-8"))

    def test_invalid_adjudication_lists_every_error_and_writes_no_sheet(self):
        self.entry(self.unary_id)["verdict"] = "FIX"
        self.entry(self.mint_id)["rationale"] = ""
        code, stdout, stderr = self.review()
        self.assertEqual(code, 1)
        self.assertEqual(stdout, "")
        self.assertFalse(self.review_path.exists())
        self.assertIn(self.unary_id, stderr)
        self.assertIn(Rule.INVALID_VERDICT.value, stderr)
        self.assertIn(self.mint_id, stderr)
        self.assertIn(Rule.EMPTY_RATIONALE.value, stderr)


State = census_tool.RepairState


class CheckRepairsTest(ReviewFixture):
    """One case per RepairState, on both which_end values, over a FakeReader."""

    def setUp(self):
        super().setUp()
        self.reader = self.world.graph.reader()
        self.target_id = f"{self.world.e_unique_target}@{self.world.t4100}"

    def statuses(self):
        rows = census_tool.check_repairs(self.reader, self.census, self.adjudication,
                                         self.census_name)
        return {row.candidate_id: row.status for row in rows}

    def move(self, edge, source, target, **tombstone):
        self.reader.endpoints[edge] = census_tool.EdgeEndpoints(
            source, target, tombstone.get("invalid_at"), tombstone.get("expired_at"))

    def test_pending_on_both_ends_and_unminted_without_a_query(self):
        statuses = self.statuses()
        self.assertEqual(statuses, {self.unique_id: State.PENDING,
                                    self.target_id: State.PENDING,
                                    self.mint_id: State.UNMINTED})
        queried = {arg for name, arg in (c for c in self.reader.calls if isinstance(c, tuple))}
        self.assertNotIn(self.world.e_absent, queried)

    def test_applied_on_both_ends(self):
        w = self.world
        self.move(w.e_unique_source, w.t2919, w.t3017)
        self.move(w.e_unique_target, w.harness, w.t2919)
        statuses = self.statuses()
        self.assertEqual((statuses[self.unique_id], statuses[self.target_id]),
                         (State.APPLIED, State.APPLIED))

    def test_moved_elsewhere_on_both_ends(self):
        w = self.world
        self.move(w.e_unique_source, w.t1999, w.t3017)
        self.move(w.e_unique_target, w.harness, w.t700)
        statuses = self.statuses()
        self.assertEqual((statuses[self.unique_id], statuses[self.target_id]),
                         (State.MOVED_ELSEWHERE, State.MOVED_ELSEWHERE))

    def test_edge_gone_when_absent_invalidated_or_expired(self):
        w = self.world
        for tombstone in ({"invalid_at": "2026-10-04T00:00:00Z"},
                          {"expired_at": "2026-10-04T00:00:00Z"}):
            self.move(w.e_unique_source, w.t3019, w.t3017, **tombstone)
            del self.reader.endpoints[w.e_unique_target]
            statuses = self.statuses()
            self.assertEqual((statuses[self.unique_id], statuses[self.target_id]),
                             (State.EDGE_GONE, State.EDGE_GONE), tombstone)
            self.reader.endpoints[w.e_unique_target] = census_tool.EdgeEndpoints(
                w.harness, w.t4100, None, None)

    def test_target_missing_is_checked_before_the_endpoint(self):
        self.reader.existing.discard(self.world.t2919)
        self.move(self.world.e_unique_source, self.world.t2919, self.world.t3017)
        statuses = self.statuses()
        self.assertEqual((statuses[self.unique_id], statuses[self.target_id]),
                         (State.TARGET_MISSING, State.TARGET_MISSING))

    def test_an_invalid_adjudication_is_refused(self):
        self.entry(self.unary_id)["rationale"] = ""
        with self.assertRaises(ValueError) as ctx:
            self.statuses()
        self.assertIn(Rule.EMPTY_RATIONALE.value, str(ctx.exception))
        self.assertEqual(self.reader.calls, [])


class CheckRepairsCliTest(ReviewFixture):
    def setUp(self):
        super().setUp()
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.dir = Path(self._tmp.name)
        self.census_path = self.dir / self.census_name
        self.adjudication_path = self.dir / self.census_name.replace(".json", ".adjudication.json")
        census_tool.write_new_json(self.census_path, self.census)
        self.entry(self.mint_id).update(verdict=RECORD_ONLY, mint_name=None,
                                        rationale="no node to mint; recorded only")
        self.conn = FakeFalkorConnection(self.world.graph)

    def check(self, require, *extra):
        self.adjudication_path.write_text(json.dumps(self.adjudication), encoding="utf-8")
        return _run_main(["check-repairs", "--census", str(self.census_path),
                          "--adjudication", str(self.adjudication_path),
                          "--require", require, *extra], self.conn)

    def apply_repairs(self):
        w, edges = self.world, self.world.graph.edges
        for index, edge in enumerate(edges):
            if edge.uuid == w.e_unique_source:
                edges[index] = FakeEdge(edge.uuid, w.t2919, edge.target, edge.fact)
            if edge.uuid == w.e_unique_target:
                edges[index] = FakeEdge(edge.uuid, edge.source, w.t2919, edge.fact)

    def test_pending_report_and_exit_code(self):
        code, stdout, _ = self.check("pending")
        self.assertEqual(code, 0)
        report = json.loads(stdout)
        self.assertEqual(report["census"], self.census_name)
        self.assertEqual(report["require"], "pending")
        self.assertEqual(report["checked_at"], "2026-10-04T05:06:07Z")
        self.assertEqual({row["candidate_id"]: row["status"] for row in report["rows"]},
                         {self.unique_id: "pending",
                          f"{self.world.e_unique_target}@{self.world.t4100}": "pending"})
        self.assertEqual({r["which_end"] for r in report["rows"]}, {"source", "target"})
        self.assertEqual(report["counts"]["pending"], 2)
        self.assertEqual(self.check("applied")[0], 1)

    def test_applied_after_the_repairs_land(self):
        self.apply_repairs()
        code, stdout, _ = self.check("applied")
        self.assertEqual(code, 0)
        self.assertEqual(json.loads(stdout)["counts"]["applied"], 2)
        self.assertEqual(self.check("pending")[0], 1)

    def test_an_unminted_row_fails_either_requirement(self):
        self.entry(self.mint_id).update(verdict=REPAIR, mint_name="Task 502")
        self.assertEqual(self.check("pending")[0], 1)

    def test_out_writes_the_report_and_never_overwrites(self):
        out = self.dir / "pre.json"
        code, stdout, _ = self.check("pending", "--out", str(out))
        self.assertEqual(code, 0)
        self.assertEqual(json.loads(out.read_text(encoding="utf-8")), json.loads(stdout))
        before = out.read_bytes()
        code, _, stderr = self.check("pending", "--out", str(out))
        self.assertNotEqual(code, 0)
        self.assertIn("pre.json", stderr)
        self.assertEqual(out.read_bytes(), before)

    def test_an_invalid_adjudication_exits_non_zero_with_its_errors(self):
        self.entry(self.unary_id)["verdict"] = "FIX"
        code, _, stderr = self.check("pending")
        self.assertNotEqual(code, 0)
        self.assertIn(Rule.INVALID_VERDICT.value, stderr)
        self.assertEqual(self.conn.calls, [])


class ConnectFalkorTest(unittest.TestCase):
    def test_missing_redis_package_is_named(self):
        def no_module(name):
            raise ModuleNotFoundError(f"No module named {name!r}", name=name)

        with self.assertRaises(census_tool.MissingDriverError) as ctx:
            census_tool.connect_falkor("redis://localhost:6379", import_module=no_module)
        self.assertIn("redis", str(ctx.exception))


if __name__ == "__main__":
    unittest.main()
