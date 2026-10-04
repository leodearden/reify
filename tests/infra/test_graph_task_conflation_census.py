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
import sys
import unittest
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


if __name__ == "__main__":
    unittest.main()
