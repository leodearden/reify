#!/usr/bin/env python3
"""
test_graph_task_conflation_census.py — stdlib unittest for
scripts/graph-task-conflation-census.py.

WHAT RUNS THIS: not the gate directly. run_all.sh discovers `test_*.sh` only,
so the discovered member is the thin wrapper
tests/infra/test_graph_task_conflation_census.sh, which invokes this file. A
bare .py here would be silently never run.

The tool's filename is hyphenated (repo script convention), so it is loaded by
path via importlib rather than by a bare `import`. Every test drives the tool
in-process through its public seams (the reader Protocol and main()'s
`connect` parameter); this file spawns no subprocess and touches no live graph.
"""

import importlib.util
import json
import sys
import unittest
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
TOOL_PATH = REPO_ROOT / "scripts" / "graph-task-conflation-census.py"


def _load_tool():
    spec = importlib.util.spec_from_file_location("graph_task_conflation_census", TOOL_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {TOOL_PATH}")
    module = importlib.util.module_from_spec(spec)
    # Register before exec: @dataclass resolves a field's type through
    # sys.modules[cls.__module__], which is None for an unregistered module.
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


census_tool = _load_tool()


class FactTaskRefsTest(unittest.TestCase):
    def assertLocal(self, text, expected):
        refs = census_tool.fact_task_refs(text)
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
            refs = census_tool.fact_task_refs(text)
            self.assertEqual(refs.cross_project, frozenset({3673}), text)
            self.assertEqual(refs.local, frozenset(), text)

    def test_task_word_is_not_a_project_qualifier(self):
        refs = census_tool.fact_task_refs("Task: 12 is pending")
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
            self.assertTrue(census_tool.mentions_number(text, 2590), text)

    def test_digit_neighbours_and_decimals_are_not_presence(self):
        for text in ("task 12590", "task 25901", "ratio 2590.5", "no number here"):
            self.assertFalse(census_tool.mentions_number(text, 2590), text)


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
            self.assertEqual(census_tool.node_task_numbers(name), frozenset(expected), name)

    def test_non_task_names(self):
        for name in ("Taskmaster", "task tree", "task_id", "taskset 5", "Orchestrator 12"):
            self.assertEqual(census_tool.node_task_numbers(name), frozenset(), name)


class CanonicalTaskNumberTest(unittest.TestCase):
    def test_canonical_names(self):
        for name in ("Task 1997", "task 1997", "task #1997", "Task: 1997",
                     "task id 1997", "  Task 1997  "):
            self.assertEqual(census_tool.canonical_task_number(name), 1997, name)

    def test_non_canonical_names(self):
        for name in ("task/1997", "tasks 1997/1998", "task 1997.2",
                     "task 1997 (x)", "Taskmaster", "task1997"):
            self.assertIsNone(census_tool.canonical_task_number(name), name)


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
        self.assertEqual(summary["task_nodes"], 10)
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


if __name__ == "__main__":
    unittest.main()
