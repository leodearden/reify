#!/usr/bin/env python3
"""
test_verify_scope_record.py — the `scope` section of the Python --print-scope
record against live bash: the CLI surface, the effective scope, the scope
decision, the D6 fixture coupling and a failing bash library call (S18 β, task
9097; PRD docs/prds/v0_6/verify-structured-plan-and-role-table.md D5, D6, D11,
B7).

Every check drives a public seam as a subprocess: `python3 -I -B
scripts/verify.py ... --print-scope` and `bash scripts/verify.sh ...
--print-plan`. This module never imports reify_verify. Bash is the oracle: a
port's spec is today's behaviour, so most expectations are what bash prints in
the same tree. Where β deliberately differs from bash, the check asserts
python alone and its message names the reason.

The shared clone, the hermetic env and the side-by-side runner live in
tests/infra/scope_record_harness.py.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its
thin wrapper tests/infra/test_verify_scope_record.sh.
"""

import json
import re
import sys
import tempfile
import unittest
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(INFRA_DIR))
from scope_record_harness import (  # noqa: E402
    BRANCH_PLAN,
    BRANCH_SCOPE,
    CLOSURE_WITHOUT_GUI,
    DOCS_PATH,
    ERROR_PREFIX,
    GATHER_FAILURES,
    STAGED_PLAN,
    STAGED_SCOPE,
    FailingGit,
    InClone,
    base_env,
    c5_warning,
    discard_clone,
    env_with_only,
    run_bash,
    run_py,
    scope_record,
)

RECORD_KEYS = {"v", "scope", "narrowing", "infra_selection"}
SCOPE_FLAGS = ("run_rust", "run_gui", "run_occt_gate", "gui_path_signal")
MERGE_HEAD_NOTICE = "verify.sh: MERGE_HEAD present — forcing --scope all (merge in progress)"
MERGE_BASE_WARNING = (
    "verify.sh: WARNING — --scope branch could not resolve 'git merge-base main HEAD' "
    "(detached HEAD / missing local main ref / merge-base failure) — failing WIDE to "
    "--scope all (contract C5)"
)
FORCED_SCOPE_MARK = "forcing --scope all"
FAIL_WIDE_MARK = "failing WIDE"
FIXTURES_DIR = "tests/prd-gate/fixtures/"
FIXTURE_REFERENCE = r"tests/prd-gate/fixtures/[A-Za-z0-9_.-]+\.ri"
REVIEWED_MARKER = "pg-drift:allow"
GUI_PINS = "gui/src/__tests__/reifyGrammarCorpus.expected-clean.json"
OCCT_DECLARED_LIST = "scripts/occt-touching-crates.txt"


def tearDownModule():
    discard_clone()


def python_help():
    outcome = run_py(["--help"], base_env())
    assert outcome.returncode == 0, outcome.stderr
    return outcome.stdout


class TestPrintScopeCli(unittest.TestCase):
    """--print-scope is an action query on the same validation path (D11)."""

    @classmethod
    def setUpClass(cls):
        cls.help_text = python_help()

    def assert_int_flag(self, record, key, value):
        self.assertIs(type(record[key]), int, f"{key} is a JSON int: {record}")
        self.assertEqual(record[key], value, key)

    def test_scope_all_record(self):
        for argv in (["all", "--print-scope"], ["test", "--scope", "all", "--print-scope"]):
            with self.subTest(argv=argv):
                outcome = run_py(argv, base_env())
                record = scope_record(outcome)
                self.assertEqual(outcome.stderr, "")
                self.assertEqual(set(record), RECORD_KEYS)
                scope = record["scope"]
                self.assertEqual(scope["mode"], "all")
                self.assertIsNone(scope["widened"], "a requested scope all was not widened")
                for flag in SCOPE_FLAGS:
                    self.assert_int_flag(scope, flag, 1)
                self.assertEqual(scope["changed_paths"], [])
                self.assertEqual(scope["rename_sources"], [])
                narrowing = record["narrowing"]
                self.assert_int_flag(narrowing, "narrow_active", 0)
                self.assert_int_flag(narrowing, "from_diff", 0)
                self.assert_int_flag(narrowing, "run_gui_vitest", 1)
                self.assertEqual(record["infra_selection"], [])

    def assert_usage_error(self, argv, message, with_help=False):
        outcome = run_py(argv, base_env())
        self.assertEqual(outcome.returncode, 64, outcome.stderr)
        self.assertEqual(outcome.stdout, "")
        expected = f"{ERROR_PREFIX}{message}\n" + (self.help_text if with_help else "")
        self.assertEqual(outcome.stderr, expected)

    def test_missing_action(self):
        self.assert_usage_error(
            ["--print-scope"], "missing action (test|lint|typecheck|all)", with_help=True
        )

    def test_a_non_gate_action_is_refused(self):
        for action in ("psi-gate", "compile-gate"):
            with self.subTest(action=action):
                self.assert_usage_error(
                    [action, "--print-scope"],
                    f"query '--print-scope' takes action test|lint|typecheck|all "
                    f"(got '{action}')",
                )

    def test_a_conflicting_flag_is_refused(self):
        cases = (
            (["all", "--print-scope", "--print-plan"], "--print-plan"),
            (["test", "--print-scope", "--confirm-failed"], "--confirm-failed"),
        )
        for argv, flag in cases:
            with self.subTest(argv=argv):
                self.assert_usage_error(
                    argv, f"query '--print-scope' does not combine with {flag}"
                )

    def test_queries_are_exclusive(self):
        self.assert_usage_error(
            ["--print-knobs", "--print-scope"],
            "query already set to '--print-knobs', got '--print-scope'",
        )

    def test_the_same_validation_path(self):
        self.assert_usage_error(
            ["all", "--print-scope", "--profile=weird"],
            "invalid --profile 'weird' (want debug|release|both)",
        )

    def test_bash_rejects_the_query_flag(self):
        bash = run_bash(["--print-plan", "--print-scope"], base_env())
        self.assertEqual(bash.returncode, 64)
        self.assertEqual(
            bash.stderr.splitlines()[0],
            f"{ERROR_PREFIX}unknown argument '--print-scope'",
            "D8 listed divergence (fold-verifier V12): bash has no --print-scope",
        )

    def test_help_lists_the_query(self):
        self.assertIn("--print-scope", self.help_text)
        last = [line for line in self.help_text.splitlines() if line.strip()][-1]
        self.assertTrue(last.lstrip().startswith("-h|--help"), last)


class TestEffectiveScope(InClone):
    """MERGE_HEAD, force_scope_all and the merge base settle the scope before any diff."""

    def setUp(self):
        super().setUp()
        self.clone.stage(DOCS_PATH)

    def test_merge_head_is_found_from_the_repo_root(self):
        self.clone.merge_head().write_text(self.clone.base + "\n", encoding="utf-8")
        with tempfile.TemporaryDirectory() as foreign:
            py = self.clone.py(STAGED_SCOPE, base_env(), cwd=foreign)
        scope = scope_record(py)["scope"]
        self.assertEqual((scope["mode"], scope["widened"]), ("all", "merge_in_progress"))
        self.assertIn(MERGE_HEAD_NOTICE, py.stderr.splitlines())
        bash = self.clone.bash(STAGED_PLAN, base_env())
        self.assertEqual(bash.returncode, 0, bash.stderr)
        self.assertIn(
            MERGE_HEAD_NOTICE,
            bash.stderr.splitlines(),
            "premise: bash, run from the clone root, sees the same MERGE_HEAD",
        )

    def test_force_scope_all_roles(self):
        for role, forced in (("merge", True), ("background", True), ("task", False), ("offline", False)):
            with self.subTest(role=role):
                env = base_env(DF_VERIFY_ROLE=role)
                py = self.clone.py(STAGED_SCOPE, env)
                scope = scope_record(py)["scope"]
                self.assertEqual(
                    (scope["mode"], scope["widened"]),
                    ("all", "role_forces_all") if forced else ("staged", None),
                )
                bash = self.clone.bash(STAGED_PLAN, env)
                self.assertEqual(bash.returncode, 0, bash.stderr)
                bash_notices = [l for l in bash.stderr.splitlines() if FORCED_SCOPE_MARK in l]
                py_notices = [l for l in py.stderr.splitlines() if FORCED_SCOPE_MARK in l]
                self.assertEqual(len(bash_notices), int(forced), bash.stderr)
                self.assertEqual(py_notices, bash_notices)

    def test_branch_without_main_fails_wide(self):
        self.clone.git("branch", "-D", "main")
        py = self.clone.py(BRANCH_SCOPE, base_env())
        scope = scope_record(py)["scope"]
        self.assertEqual((scope["mode"], scope["widened"]), ("all", "no_merge_base"))
        self.assertIn(MERGE_BASE_WARNING, py.stderr.splitlines())
        bash = self.clone.bash(BRANCH_PLAN, base_env())
        self.assertEqual(bash.returncode, 0, bash.stderr)
        self.assertIn(MERGE_BASE_WARNING, bash.stderr.splitlines(), "premise: bash's exact C5 line")

    def test_branch_records_the_merge_base(self):
        self.clone.git("branch", "-f", "main", "HEAD")
        py = self.clone.py(BRANCH_SCOPE, base_env())
        scope = scope_record(py)["scope"]
        self.assertEqual((scope["mode"], scope["widened"]), ("branch", None))
        self.assertEqual(scope["merge_base"], self.clone.git("merge-base", "main", "HEAD"))


def _declare_reify_doc_occt(clone):
    clone.commit_change(OCCT_DECLARED_LIST, "reify-doc\n")
    clone.stage("crates/reify-doc/src/lib.rs")


STAGED_SCENARIOS = {
    "occt-declared crate": (lambda c: c.stage("crates/reify-eval/src/lib.rs"), {"run_occt_gate": 1}),
    "undeclared crate": (lambda c: c.stage("crates/reify-doc/src/lib.rs"), {"run_occt_gate": 0}),
    "depth-1 crates file": (lambda c: c.stage("crates/README.md"), {"run_rust": 1, "run_occt_gate": 0}),
    "gui crate": (lambda c: c.stage("gui/src-tauri/src/main.rs"), {}),
    "workspace manifest": (lambda c: c.stage("Cargo.toml"), {}),
    "lockfile": (lambda c: c.stage("Cargo.lock"), {}),
    "cargo config": (lambda c: c.stage(".cargo/config.toml"), {}),
    "toolchain file": (lambda c: c.stage("rust-toolchain.toml"), {}),
    "grammar": (lambda c: c.stage("tree-sitter-reify/grammar.js"), {}),
    "grammar readme (global beats inert)": (lambda c: c.stage("tree-sitter-reify/README.md"), {"run_rust": 1}),
    "frontend source": (lambda c: c.stage("gui/src/bridge.ts"), {}),
    "gui readme (gui beats inert)": (lambda c: c.stage("gui/README.md"), {"run_gui": 1}),
    "docs": (lambda c: c.stage(DOCS_PATH), {"run_rust": 0, "run_gui": 0}),
    "root markdown": (lambda c: c.stage("README.md"), {}),
    "yaml": (lambda c: c.stage("dark-factory-orchestrator.yaml"), {}),
    "gui event channels edited": (
        lambda c: c.stage("docs/gui-event-channels.md"),
        {"run_rust": 0, "run_gui": 1},
    ),
    "gui event channels renamed": (
        lambda c: c.git("mv", "docs/gui-event-channels.md", "docs/zz-scope-record-channels.md"),
        {"run_rust": 1},
    ),
    "example": (lambda c: c.stage("examples/anisotropic_bar.ri"), {}),
    "infra test": (lambda c: c.stage("tests/infra/test_helpers.sh"), {}),
    "script": (lambda c: c.stage("scripts/land.sh"), {}),
    "crate deletion": (lambda c: c.git("rm", "-q", "--", "crates/reify-doc/src/util.rs"), {"run_rust": 1}),
    ".task/ only": (
        lambda c: c.stage_ignored(".task/zz-scope-record"),
        {"run_rust": 0, "run_gui": 0, "run_occt_gate": 0},
    ),
    "docs and crate": (
        lambda c: (c.stage(DOCS_PATH), c.stage("crates/reify-eval/src/lib.rs")),
        {"run_rust": 1},
    ),
    "OCCT set read from its lib": (_declare_reify_doc_occt, {"run_occt_gate": 1}),
}

BRANCH_SCENARIOS = {
    "crate": "crates/reify-doc/src/lib.rs",
    "docs": DOCS_PATH,
    "frontend": "gui/src/bridge.ts",
}
UNCOMMITTED_EDIT = "README.md"


class TestScopeDecisionMatchesBash(InClone):
    """decide_scope over PathFacts classifies every arm as bash's decide_scope does."""

    def assert_expected(self, record, expected, label):
        for flag, value in expected.items():
            self.assertEqual(record["scope"][flag], value, f"{label}: {flag}")

    def test_staged(self):
        for label, (change, expected) in STAGED_SCENARIOS.items():
            with self.subTest(scenario=label):
                self.clone.reset()
                change(self.clone)
                record, py, bash = self.side_by_side("staged")
                self.assert_flags_match_bash(record, bash, label)
                self.assert_expected(record, expected, label)
                self.assertIsNone(record["scope"]["widened"], f"{label}: decided over its paths")
                staged = [p for p in self.clone.name_list("--cached") if not p.startswith(".task/")]
                self.assertEqual(record["scope"]["changed_paths"], staged, label)
                self.assert_stderr_within_bash(py, bash, label)

    def test_branch_runs_to_the_working_tree(self):
        for label, path in BRANCH_SCENARIOS.items():
            with self.subTest(scenario=label):
                self.clone.reset()
                self.clone.start_branch()
                self.clone.commit_change(path)
                self.clone.append(UNCOMMITTED_EDIT)
                record, py, bash = self.side_by_side("branch")
                self.assertEqual(record["scope"]["mode"], "branch")
                self.assert_flags_match_bash(record, bash, label)
                self.assertEqual(
                    record["scope"]["changed_paths"], sorted({path, UNCOMMITTED_EDIT}), label
                )
                self.assert_stderr_within_bash(py, bash, label)


def directly_before(lines, first, second):
    """`first` is immediately followed by `second` somewhere in `lines`."""
    return any(lines[i:i + 2] == [first, second] for i in range(len(lines) - 1))


class TestGatherFailsWide(InClone):
    """Every changed-path git failure fails wide under one policy (C5; D5 amended 2026-10-15)."""

    def test_each_failing_git_call(self):
        with tempfile.TemporaryDirectory() as scratch:
            for case in GATHER_FAILURES:
                with self.subTest(case=case.git.label):
                    self.clone.reset()
                    self.clone.start_branch()
                    self.clone.stage(DOCS_PATH)
                    record, py, bash = self.side_by_side(case.scope, case.git.env(Path(scratch)))
                    scope = record["scope"]
                    self.assertEqual(
                        (scope["mode"], scope["widened"]),
                        (case.scope, "changed_paths_unread"),
                        "the record says the request was widened, not decided over its paths",
                    )
                    for flag in SCOPE_FLAGS:
                        self.assertEqual(scope[flag], 1, flag)
                    self.assertEqual(scope["changed_paths"], [])
                    py_wide = [l for l in py.stderr.splitlines() if FAIL_WIDE_MARK in l]
                    self.assertEqual(py_wide, [case.warning])
                    self.assertTrue(
                        directly_before(py.stderr.splitlines(), case.git.reason, case.warning),
                        f"git's own stderr precedes the warning, as bash leaves it: {py.stderr!r}",
                    )
                    if case.bash_parity:
                        self.assert_flags_match_bash(record, bash, case.git.label)
                        bash_wide = [l for l in bash.stderr.splitlines() if FAIL_WIDE_MARK in l]
                        self.assertEqual(py_wide, bash_wide)
                        self.assertTrue(
                            directly_before(bash.stderr.splitlines(), case.git.reason, case.warning),
                            f"premise: bash keeps git's stderr: {bash.stderr!r}",
                        )

    def test_git_missing_from_path_fails_wide(self):
        self.clone.stage(DOCS_PATH)
        with tempfile.TemporaryDirectory() as scratch:
            py = self.clone.py(STAGED_SCOPE, env_with_only(Path(scratch)))
        scope = scope_record(py)["scope"]
        self.assertEqual((scope["mode"], scope["widened"]), ("staged", "changed_paths_unread"))
        lines = py.stderr.splitlines()
        self.assertEqual(len(lines), 2, py.stderr)
        self.assertIn("git", lines[0])
        self.assertIn("No such file or directory", lines[0])
        self.assertEqual(lines[1], c5_warning("staged", "git diff"))


def derived_rust_set(clone):
    """PG-DRIFT's predicate (PRD D6): fixture basenames named on unmarked lines of tracked *.rs."""
    lines = clone.git("grep", "-h", "-E", FIXTURE_REFERENCE, "--", "*.rs").splitlines()
    return {
        match.rsplit("/", 1)[1]
        for line in lines
        if REVIEWED_MARKER not in line
        for match in re.findall(FIXTURE_REFERENCE, line)
    }


def pinned_gui_set(clone):
    """Basenames of the prd-gate fixtures the vitest ledger pins (PRD D6)."""
    pins = json.loads((clone.root / GUI_PINS).read_text(encoding="utf-8"))
    return {pin.rsplit("/", 1)[1] for pin in pins if pin.startswith(FIXTURES_DIR)}


def fixture(name):
    return FIXTURES_DIR + name


def flags(scope):
    return scope["run_rust"], scope["run_gui"], scope["run_occt_gate"]


class TestDerivedFixtureCoupling(InClone):
    """B7: a fixture's coupling is derived from the tree, with no list edit (PRD D6)."""

    UNREFERENCED = "zz_scope_record_unreferenced.ri"
    MARKED = "zz_scope_record_marked.ri"
    REFERENCED = "zz_scope_record_referenced.ri"
    PINNED = "zz_scope_record_pinned.ri"
    MOVED = "zz_scope_record_moved.ri"
    PROBE_RS = "crates/reify-doc/tests/zz_scope_record_probe.rs"

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.clone.reset()
        cls.rust = derived_rust_set(cls.clone)
        cls.gui = pinned_gui_set(cls.clone)
        on_disk = {path.name for path in (cls.clone.root / FIXTURES_DIR).glob("*.ri")}
        cls.rust_on_disk = sorted(cls.rust & on_disk)
        cls.uncoupled_on_disk = sorted(on_disk - cls.rust - cls.gui)

    def test_premise_the_sets_are_not_vacuous(self):
        self.assertTrue(self.rust, "the derived Rust set is empty: the predicate found nothing")
        self.assertTrue(self.gui, "the JSON pins name no prd-gate fixture")
        self.assertTrue(self.rust_on_disk and self.uncoupled_on_disk)

    def each_staged_alone(self, names):
        """{name: its scope record}, each fixture staged alone."""
        outcomes = self.clone.read_each_staged_alone([fixture(name) for name in names], STAGED_SCOPE)
        scopes = {}
        for path, outcome in outcomes.items():
            scope = scope_record(outcome)["scope"]
            self.assertEqual(scope["changed_paths"], [path], "the run read its own index")
            scopes[path.rsplit("/", 1)[1]] = scope
        return scopes

    def test_every_rust_coupled_fixture_runs_rust(self):
        for name, scope in self.each_staged_alone(sorted(self.rust)).items():
            with self.subTest(fixture=name):
                self.assertEqual(flags(scope), (1, 1, 1))

    def test_every_pinned_fixture_runs_gui(self):
        for name, scope in self.each_staged_alone(sorted(self.gui)).items():
            with self.subTest(fixture=name):
                self.assertEqual(scope["run_gui"], 1)
                if name not in self.rust:
                    self.assertEqual(flags(scope), (0, 1, 0))

    def test_an_unreferenced_unpinned_fixture_runs_nothing(self):
        self.clone.stage(fixture(self.UNREFERENCED))
        record, py, bash = self.side_by_side("staged")
        self.assertEqual(flags(record["scope"]), (0, 0, 0))
        self.assert_flags_match_bash(record, bash)
        self.assertEqual(py.stderr, "")

    def commit_probe_rs(self):
        self.clone.commit_change(
            self.PROBE_RS,
            f"// {fixture(self.MARKED)} ({REVIEWED_MARKER} — scope-record probe: prose only)\n"
            f"// {fixture(self.REFERENCED)} is read by this probe\n",
        )

    def test_the_reviewed_marker_drops_a_reference(self):
        self.commit_probe_rs()
        scope, _ = self.staged_alone_on_top(fixture(self.MARKED))
        self.assertEqual(scope["run_rust"], 0, "a pg-drift:allow line is not a reference")

    def test_a_new_reference_couples_with_no_list_edit(self):
        before, _ = self.staged_alone_on_top(fixture(self.REFERENCED))
        self.assertEqual(flags(before), (0, 0, 0), "premise: no tracked *.rs names the fixture yet")
        self.clone.reset()
        self.commit_probe_rs()
        scope, _ = self.staged_alone_on_top(fixture(self.REFERENCED))
        self.assertEqual(flags(scope), (1, 1, 1))

    def test_a_new_pin_couples_with_no_list_edit(self):
        self.commit_pin(self.PINNED)
        scope, _ = self.staged_alone_on_top(fixture(self.PINNED))
        self.assertEqual(flags(scope), (0, 1, 0))

    def test_staged_reads_references_from_the_index(self):
        self.commit_probe_rs()
        (self.clone.root / self.PROBE_RS).unlink()
        scope, _ = self.staged_alone_on_top(fixture(self.REFERENCED))
        self.assertEqual(
            flags(scope),
            (1, 1, 1),
            "the commit still holds the referencing test: an unstaged deletion does not uncouple",
        )

    def test_staged_reads_pins_from_the_index(self):
        self.commit_pin(self.PINNED)
        (self.clone.root / GUI_PINS).write_text("[]\n", encoding="utf-8")
        scope, py = self.staged_alone_on_top(fixture(self.PINNED))
        self.assertEqual(flags(scope), (0, 1, 0), "an unstaged unpin does not uncouple")
        self.assertEqual(py.stderr, "")

    def commit_pin(self, name):
        pins_path = self.clone.root / GUI_PINS
        pins = json.loads(pins_path.read_text(encoding="utf-8"))
        pins_path.write_text(json.dumps([*pins, fixture(name)], indent=2) + "\n", encoding="utf-8")
        self.clone.git("add", "--", GUI_PINS)
        self.clone.commit("scope-record probe: pin a fixture")

    def staged_alone_on_top(self, path):
        """Stage `path` over whatever the test committed (no reset)."""
        self.clone.stage(path)
        py = self.clone.py(STAGED_SCOPE, base_env())
        return scope_record(py)["scope"], py

    def test_a_renamed_rust_fixture_still_runs_rust(self):
        source = fixture(self.rust_on_disk[0])
        self.clone.git("mv", source, fixture(self.MOVED))
        scope = scope_record(self.clone.py(STAGED_SCOPE, base_env()))["scope"]
        self.assertEqual(scope["rename_sources"], [source])
        self.assertEqual(scope["run_rust"], 1)

    def test_a_renamed_uncoupled_fixture_runs_nothing(self):
        self.clone.git("mv", fixture(self.uncoupled_on_disk[0]), fixture(self.MOVED))
        scope = scope_record(self.clone.py(STAGED_SCOPE, base_env()))["scope"]
        self.assertEqual((scope["run_rust"], scope["run_gui"]), (0, 0))

    def test_a_failing_reference_grep_fails_wide(self):
        grep = FailingGit("reference grep", "grep --cached -h -E")
        warning = (
            f"verify.sh: WARNING — git grep --cached -E '{FIXTURE_REFERENCE}' -- '*.rs' exited 128 "
            f"({grep.reason}) — every prd-gate fixture counts as Rust-coupled (fail wide, contract C5)"
        )
        self.clone.stage(fixture(self.UNREFERENCED))
        with tempfile.TemporaryDirectory() as scratch:
            py = self.clone.py(STAGED_SCOPE, grep.env(Path(scratch)))
        self.assertEqual(flags(scope_record(py)["scope"]), (1, 1, 1))
        self.assertIn(warning, py.stderr.splitlines())

    def test_a_failing_staged_pins_read_fails_wide(self):
        read = FailingGit("pins read", "cat-file blob")
        self.clone.stage(fixture(self.UNREFERENCED))
        with tempfile.TemporaryDirectory() as scratch:
            py = self.clone.py(STAGED_SCOPE, read.env(Path(scratch)))
        self.assertEqual(scope_record(py)["scope"]["run_gui"], 1, "an unreadable pins blob fails wide")
        self.assertIn(
            f"{GUI_PINS} is unreadable (git cat-file blob :{GUI_PINS} exited 128 ({read.reason}))",
            py.stderr,
        )

    def test_the_derivation_is_lazy_and_fails_wide(self):
        (self.clone.root / GUI_PINS).write_text("{ not json\n", encoding="utf-8")
        self.clone.git("add", "--", GUI_PINS)
        self.clone.commit("scope-record probe: a malformed pins file")
        docs_scope, docs = self.staged_alone_on_top(DOCS_PATH)
        self.assertEqual(flags(docs_scope), (0, 0, 0))
        self.assertNotIn(GUI_PINS, docs.stderr, "the derivation ran for a non-fixture change")
        self.clone.git("reset", "-q", "--", DOCS_PATH)
        scope, py = self.staged_alone_on_top(fixture(self.UNREFERENCED))
        self.assertEqual(scope["run_gui"], 1, "an unreadable pins file fails wide")
        self.assertEqual(len([line for line in py.stderr.splitlines() if GUI_PINS in line]), 1, py.stderr)


class TestBashCallFailureEndsTheQuery(InClone):
    """A failing bash library call is an error: one stderr line naming the call, exit 1 (PRD D15)."""

    def test_a_missing_occt_declared_list(self):
        self.clone.git("rm", "-q", "--", OCCT_DECLARED_LIST)
        self.clone.commit("scope-record probe: no declared OCCT list")
        self.clone.stage(CLOSURE_WITHOUT_GUI)
        py = self.clone.py(STAGED_SCOPE, base_env())
        self.assertEqual((py.returncode, py.stdout), (1, ""), py.stderr)
        lines = py.stderr.splitlines()
        self.assertEqual(len(lines), 1, py.stderr)
        self.assertTrue(
            lines[0].startswith(
                f"{ERROR_PREFIX}scripts/occt-scope-lib.sh::occt_declared_set exited 1 ("
            ),
            lines[0],
        )
        self.assertIn("occt-touching-crates.txt not found", lines[0])

    def test_bash_missing_from_path(self):
        self.clone.stage(CLOSURE_WITHOUT_GUI)
        with tempfile.TemporaryDirectory() as scratch:
            py = self.clone.py(STAGED_SCOPE, env_with_only(Path(scratch), "git"))
        self.assertEqual((py.returncode, py.stdout), (1, ""), py.stderr)
        lines = py.stderr.splitlines()
        self.assertEqual(len(lines), 1, py.stderr)
        self.assertTrue(
            lines[0].startswith(
                f"{ERROR_PREFIX}scripts/occt-scope-lib.sh::occt_declared_set exited 127 ("
            ),
            lines[0],
        )
        self.assertIn("No such file or directory", lines[0])


if __name__ == "__main__":
    unittest.main(verbosity=2)
