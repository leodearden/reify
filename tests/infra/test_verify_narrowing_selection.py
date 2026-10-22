#!/usr/bin/env python3
"""
test_verify_narrowing_selection.py — the `narrowing` and `infra_selection`
sections of the Python --print-scope record against live bash, in B1's shared
clone (S18 β, task 9097; PRD
docs/prds/v0_6/verify-structured-plan-and-role-table.md D5, D6, D13, D14).

Every check drives a public seam as a subprocess: `python3 -I -B
scripts/verify.py ... --print-scope` and `bash scripts/verify.sh ...
--print-plan`. This module never imports reify_verify. Bash is the oracle: the
closure, the narrowing, the vitest lane and the selective-infra leaves,
degraded inputs included, are expected to be what bash plans in the same
tree. The kloc crate set is probed against harness-layout-lib.sh. The
reify-audit predicate copies are tests/infra/test_verify_selector_copies.py's.

The shared clone, the hermetic env and the side-by-side runner live in
tests/infra/scope_record_harness.py.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its
thin wrapper tests/infra/test_verify_narrowing_selection.sh.
"""

import re
import sys
import tempfile
import unittest
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(INFRA_DIR))
from scope_record_harness import (  # noqa: E402
    BRANCH_SCOPE,
    CLOSURE_WITHOUT_GUI,
    DOCS_PATH,
    ERROR_PREFIX,
    GATHER_FAILURES,
    STAGED_SCOPE,
    FailingGit,
    InClone,
    base_env,
    discard_clone,
    header_fields,
    leaf_globs,
    scope_record,
)

NARROWING_HEADER = re.compile(
    r"^# narrowing — NARROW_ACTIVE=(\d+) affected=(.*?) closure=(.*?) from_diff=(\d+)$",
    re.MULTILINE | re.DOTALL,
)
RETRY_SPECS_MARK = "REIFY_GUI_RETRY_SPECS"
SELECTED_LEAF = re.compile(
    r'\( for _vt in (\S+); do \[ -f "\$_vt" \] \|\| continue; '
    r"((?:[A-Za-z_][A-Za-z0-9_]*=\S* )*)timeout "
)
KLOC_LEAF = "tests/infra/test_harness_kloc_cap.sh"
KLOC_LIB = "tests/infra/harness-layout-lib.sh"
KLOC_ELIGIBLE_TEST = "crates/reify-eval/tests/zz_probe.rs"
INFRA_MAP = "scripts/verify-pipeline-infra-tests.txt"
MAPPED_ARTIFACT = "scripts/verify-pipeline-guard.sh"
CLOSURE_WITH_GUI = "crates/reify-eval/src/lib.rs"


def tearDownModule():
    discard_clone()


def bash_narrowing(stdout):
    """bash's `# narrowing —` header, which spans lines while a crate list holds newlines (defect 7)."""
    found = NARROWING_HEADER.findall(stdout)
    assert len(found) == 1, f"{len(found)} narrowing headers"
    narrow_active, affected, closure, from_diff = found[0]
    return {
        "narrow_active": int(narrow_active),
        "affected": affected.split(),
        "closure": closure.split(),
        "from_diff": int(from_diff),
        "run_gui_vitest": int(header_fields(stdout)["RUN_GUI_VITEST"]),
    }


def bash_leaves(stdout):
    """bash's selective-infra plan leaves, in plan order, as (glob, {name: value})."""
    return [
        (glob, dict(token.split("=", 1) for token in assignments.split()))
        for glob, assignments in SELECTED_LEAF.findall(stdout)
    ]


def python_leaves(record):
    return [(leaf["glob"], leaf["env"]) for leaf in record["infra_selection"]]


class TestNarrowingMatchesBash(InClone):
    """The closure, narrowing and the vitest lane as bash's Phase-2 block computes them."""

    def assert_narrowing(self, scope, change=None, env=None, flags=(), label=""):
        if change is not None:
            change(self.clone)
        record, py, bash = self.side_by_side(scope, env, flags)
        expected = bash_narrowing(bash.stdout)
        narrowing = record["narrowing"]
        for key, value in expected.items():
            self.assertEqual(narrowing[key], value, f"{label}: {key}")
        self.assert_stderr_within_bash(py, bash, label)
        return narrowing, py, bash

    def test_staged_crate_computes_the_closure(self):
        narrowing, _, _ = self.assert_narrowing("staged", lambda c: c.stage(CLOSURE_WITHOUT_GUI))
        self.assertEqual((narrowing["from_diff"], narrowing["narrow_active"]), (1, 0))
        self.assertIn("reify-doc", narrowing["closure"])
        self.assertEqual(narrowing["run_gui_vitest"], 0, "a closure without reify-gui skips vitest")

    def test_staged_narrow_narrows_to_the_closure(self):
        narrowing, _, _ = self.assert_narrowing(
            "staged", lambda c: c.stage(CLOSURE_WITHOUT_GUI), flags=("--narrow",)
        )
        self.assertEqual(narrowing["narrow_active"], 1)
        self.assertEqual(narrowing["affected"], narrowing["closure"])
        self.assertEqual(narrowing["packages"], narrowing["closure"])

    def test_branch_narrows(self):
        def change(clone):
            clone.start_branch()
            clone.commit_change(CLOSURE_WITHOUT_GUI)

        narrowing, _, _ = self.assert_narrowing("branch", change)
        self.assertEqual(narrowing["narrow_active"], 1)

    def test_computed_empty_closure(self):
        narrowing, _, _ = self.assert_narrowing("staged", lambda c: c.stage("tests/infra/test_helpers.sh"))
        self.assertEqual((narrowing["from_diff"], narrowing["closure"]), (1, []))
        self.assertEqual(narrowing["run_gui_vitest"], 1, "gui_path_signal still runs vitest")

    def test_a_closure_reaching_reify_gui_runs_vitest(self):
        narrowing, _, _ = self.assert_narrowing("staged", lambda c: c.stage(CLOSURE_WITH_GUI))
        self.assertIn("reify-gui", narrowing["closure"])
        self.assertEqual(narrowing["run_gui_vitest"], 1)

    def test_the_override_shapes(self):
        for override in ("reify-eval", "   ", "*", "ALL", "reify-gui"):
            with self.subTest(override=override):
                self.clone.reset()
                narrowing, _, _ = self.assert_narrowing(
                    "staged",
                    lambda c: c.stage(CLOSURE_WITHOUT_GUI),
                    env=base_env(REIFY_AFFECTED_CRATES_OVERRIDE=override),
                    flags=("--narrow",),
                    label=f"override {override!r}",
                )
                self.assertEqual(narrowing["from_diff"], 0)

    def test_retry_specs_without_a_vitest_lane_warn(self):
        env = base_env(REIFY_GUI_RETRY_SPECS="src/__tests__/bridge.test.ts")
        narrowing, py, bash = self.assert_narrowing("staged", lambda c: c.stage(DOCS_PATH), env=env)
        self.assertEqual(narrowing["run_gui_vitest"], 0)
        py_warnings = [line for line in py.stderr.splitlines() if RETRY_SPECS_MARK in line]
        bash_warnings = [line for line in bash.stderr.splitlines() if RETRY_SPECS_MARK in line]
        self.assertEqual(len(py_warnings), 1, py.stderr)
        self.assertEqual(py_warnings, bash_warnings)

    def test_retry_specs_force_the_vitest_lane(self):
        env = base_env(REIFY_GUI_RETRY_SPECS="src/__tests__/bridge.test.ts")
        narrowing, py, _ = self.assert_narrowing("staged", lambda c: c.stage(CLOSURE_WITHOUT_GUI), env=env)
        self.assertEqual(narrowing["run_gui_vitest"], 1)
        self.assertNotIn(RETRY_SPECS_MARK, py.stderr)

    def test_no_closure_without_rust(self):
        narrowing, _, _ = self.assert_narrowing("staged", lambda c: c.stage(DOCS_PATH))
        self.assertEqual((narrowing["from_diff"], narrowing["closure"]), (0, []))

    def test_scope_all(self):
        narrowing, _, _ = self.assert_narrowing("all")
        self.assertEqual(narrowing["run_gui_vitest"], 1)

    def test_a_fail_wide_scope_never_reads_a_closure(self):
        rename_failure = next(case for case in GATHER_FAILURES if case.git.label == "staged rename source")
        with tempfile.TemporaryDirectory() as scratch:
            env = rename_failure.git.env(Path(scratch))
            narrowing, _, _ = self.assert_narrowing(
                "staged", lambda c: c.stage(CLOSURE_WITHOUT_GUI), env=env
            )
        self.assertEqual((narrowing["from_diff"], narrowing["run_gui_vitest"]), (0, 1))


def staged(*paths):
    def change(clone):
        for path in paths:
            clone.stage(path)

    return change


def branch_commit(*paths, removed=()):
    """Start a branch at HEAD, then commit `paths` (appended or created) and `removed`."""

    def change(clone):
        clone.start_branch()
        for path in paths:
            clone.stage(path)
        for path in removed:
            clone.git("rm", "-q", "--", path)
        clone.commit("scope-record probe: branch change")

    return change


SELECTION_SCENARIOS = {
    "staged infra-map artifact": ("staged", staged("scripts/verify-pipeline-guard.sh")),
    "staged crate source": ("staged", staged("crates/reify-doc/src/lib.rs")),
    "staged docs only": ("staged", staged(DOCS_PATH)),
    "staged upper-case extension": ("staged", staged("docs/Probe.TS")),
    "staged deletion": ("staged", lambda c: c.git("rm", "-q", "--", "crates/reify-doc/src/util.rs")),
    "staged .task/ only": ("staged", lambda c: c.stage_ignored(".task/zz-scope-record")),
    "branch added top-level test": ("branch", branch_commit(KLOC_ELIGIBLE_TEST)),
    "branch modified harness root": ("branch", branch_commit("crates/reify-eval/tests/harness_cache.rs")),
    "branch modified standalone": ("branch", branch_commit("crates/reify-eval/tests/achieved_repr_tol.rs")),
    "branch added nested non-harness": ("branch", branch_commit("crates/reify-eval/tests/common/zz.rs")),
    "branch added non-consolidatable crate test": ("branch", branch_commit("crates/reify-doc/tests/zz.rs")),
    "branch new compiler source": ("branch", branch_commit("crates/reify-compiler/src/zz.rs")),
    "branch new compiler _tests source": ("branch", branch_commit("crates/reify-compiler/src/zz_tests.rs")),
    "branch pdiag baseline": ("branch", branch_commit("crates/reify-audit/pdiag-baseline.txt")),
    "branch deleted pdoccover baseline": (
        "branch",
        branch_commit(removed=("crates/reify-audit/pdoccover-baseline.txt",)),
    ),
    "branch-only rust change": ("branch", branch_commit("crates/reify-doc/src/lib.rs")),
    "scope all": ("all", None),
}


class TestInfraSelectionMatchesBash(InClone):
    """bash's selectors pick bash's selective-infra leaves, in bash's order, with their env."""

    def test_each_selector(self):
        for label, (scope, change) in SELECTION_SCENARIOS.items():
            with self.subTest(scenario=label):
                self.clone.reset()
                if change is not None:
                    change(self.clone)
                record, py, bash = self.side_by_side(scope)
                self.assertEqual(
                    python_leaves(record),
                    bash_leaves(bash.stdout),
                    f"{label}: python's infra_selection differs from bash's selective-infra leaves — "
                    "mirror the scripts/verify.sh selector change in scripts/reify_verify/selection.py "
                    "in the same diff (PRD D13)",
                )
                self.assert_stderr_within_bash(py, bash, label)

    def test_premise_the_scenarios_select_something(self):
        self.clone.reset()
        branch_commit("crates/reify-compiler/src/zz.rs")(self.clone)
        _, _, bash = self.side_by_side("branch")
        self.assertGreaterEqual(len(bash_leaves(bash.stdout)), 2, "the leaf reader reads bash's plan")

    def test_a_failing_selector_diff_selects_no_leaf(self):
        branch_commit(KLOC_ELIGIBLE_TEST)(self.clone)
        self.assertIn(
            KLOC_LEAF,
            leaf_globs(scope_record(self.clone.py(BRANCH_SCOPE, base_env()))),
            "premise: the branch selects the kloc guard",
        )
        diff = FailingGit("kloc selector diff", "diff --name-status --diff-filter=AM")
        with tempfile.TemporaryDirectory() as scratch:
            record, py, bash = self.side_by_side("branch", diff.env(Path(scratch)))
        self.assertNotIn(KLOC_LEAF, leaf_globs(record))
        self.assertEqual(python_leaves(record), bash_leaves(bash.stdout))
        self.assertNotIn(diff.reason, py.stderr, "the selectors drop git's stderr, as bash's 2>/dev/null does")
        self.assert_stderr_within_bash(py, bash)

    def test_a_missing_infra_map_selects_no_mapped_leaf(self):
        self.clone.stage(MAPPED_ARTIFACT)
        mapped = set(leaf_globs(scope_record(self.clone.py(STAGED_SCOPE, base_env()))))
        self.clone.reset()
        self.clone.git("rm", "-q", "--", INFRA_MAP)
        self.clone.commit("scope-record probe: no infra map")
        self.clone.stage(MAPPED_ARTIFACT)
        record, py, bash = self.side_by_side("staged")
        self.assertLess(set(leaf_globs(record)), mapped, "premise: the map's rows selected a leaf")
        self.assertEqual(python_leaves(record), bash_leaves(bash.stdout))
        self.assert_stderr_within_bash(py, bash)


class TestHarnessKlocSetIsTheLibs(InClone):
    """The kloc guard's crate set has one owner, harness-layout-lib.sh (PRD D6, §2.3)."""

    PROBE = "crates/reify-doc/tests/zz.rs"

    def test_a_crate_added_to_the_lib_is_selected(self):
        self.assertNotIn(
            KLOC_LEAF, self.clone.branch_leaves([self.PROBE]), "premise: reify-doc is not consolidatable yet"
        )
        self.clone.reset()
        lib = self.clone.root / KLOC_LIB
        text = lib.read_text(encoding="utf-8")
        declared = "declare -ga _HL_CRATES=("
        self.assertEqual(text.count(declared), 1, "premise: the lib's one _HL_CRATES definition")
        lib.write_text(text.replace(declared, declared + "reify-doc "), encoding="utf-8")
        self.clone.git("add", "--", KLOC_LIB)
        self.clone.commit("scope-record probe: reify-doc is consolidatable")
        branch_commit(self.PROBE)(self.clone)
        record = scope_record(self.clone.py(BRANCH_SCOPE, base_env()))
        self.assertIn(
            (KLOC_LEAF, {}),
            python_leaves(record),
            "python reads the crate set from harness_layout_consolidatable_crates (D6, §2.3)",
        )

    def test_a_failing_crate_set_call_ends_the_query(self):
        self.clone.git("rm", "-q", "--", KLOC_LIB)
        self.clone.commit("scope-record probe: no harness-layout lib")
        branch_commit(self.PROBE)(self.clone)
        py = self.clone.py(BRANCH_SCOPE, base_env())
        self.assertEqual((py.returncode, py.stdout), (1, ""), py.stderr)
        lines = py.stderr.splitlines()
        self.assertEqual(len(lines), 1, py.stderr)
        self.assertTrue(
            lines[0].startswith(f"{ERROR_PREFIX}{KLOC_LIB}::harness_layout_consolidatable_crates exited 1 ("),
            lines[0],
        )
        self.assertIn("No such file or directory", lines[0])


if __name__ == "__main__":
    unittest.main(verbosity=2)
