#!/usr/bin/env python3
"""
test_verify_scope_record.py — the Python scope decision, narrowing and infra
selection against live bash (S18 β, task 9097; PRD
docs/prds/v0_6/verify-structured-plan-and-role-table.md D5, D6, D11, B7).

Every check drives a public seam as a subprocess: `python3 -I -B
scripts/verify.py ... --print-scope` and `bash scripts/verify.sh ...
--print-plan`. This module never imports reify_verify. Bash is the oracle: a
port's spec is today's behaviour, so most expectations are what bash prints in
the same tree. Where β deliberately differs from bash, the check asserts
python alone and its message names the reason.

Every child env is hermetic_env() with every REIFY_* and DF_* key also removed,
so a knob the gate's own verify_env sets cannot change a decision under test.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its
thin wrapper tests/infra/test_verify_scope_record.sh.
"""

import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(INFRA_DIR))
import reify_audit_inputs  # noqa: E402
from tree_under_test import scratch_git, shared_clone  # noqa: E402
from verify_ledger_env import hermetic_env  # noqa: E402

REPO_ROOT = INFRA_DIR.parents[1]
HANG_GUARD_SECS = 600
CONCURRENT_READS = 8
ERROR_PREFIX = "verify.sh: ERROR — "
RECORD_KEYS = {"v", "scope", "narrowing", "infra_selection"}
SCOPE_FLAGS = ("run_rust", "run_gui", "run_occt_gate", "gui_path_signal")
DOCS_PATH = "docs/getting-started.md"
MERGE_HEAD_NOTICE = "verify.sh: MERGE_HEAD present — forcing --scope all (merge in progress)"
MERGE_BASE_WARNING = (
    "verify.sh: WARNING — --scope branch could not resolve 'git merge-base main HEAD' "
    "(detached HEAD / missing local main ref / merge-base failure) — failing WIDE to "
    "--scope all (contract C5)"
)
FORCED_SCOPE_MARK = "forcing --scope all"
FAIL_WIDE_MARK = "failing WIDE"
SCOPE_HEADER = "# scope decision — "
BASH_FLAGS = {"run_rust": "RUN_RUST", "run_gui": "RUN_GUI", "run_occt_gate": "RUN_OCCT_GATE"}
FIXTURES_DIR = "tests/prd-gate/fixtures/"
FIXTURE_REFERENCE = r"tests/prd-gate/fixtures/[A-Za-z0-9_.-]+\.ri"
REVIEWED_MARKER = "pg-drift:allow"
GUI_PINS = "gui/src/__tests__/reifyGrammarCorpus.expected-clean.json"
STAGED_SCOPE = ["test", "--scope", "staged", "--print-scope"]
BRANCH_SCOPE = ["test", "--scope", "branch", "--print-scope"]
NARROWING_HEADER = re.compile(
    r"^# narrowing — NARROW_ACTIVE=(\d+) affected=(.*?) closure=(.*?) from_diff=(\d+)$",
    re.MULTILINE | re.DOTALL,
)
RETRY_SPECS_MARK = "REIFY_GUI_RETRY_SPECS"
CLOSURE_WITHOUT_GUI = "crates/reify-doc/src/lib.rs"
SELECTED_LEAF = re.compile(
    r'\( for _vt in (\S+); do \[ -f "\$_vt" \] \|\| continue; '
    r"((?:[A-Za-z_][A-Za-z0-9_]*=\S* )*)timeout "
)
KLOC_LEAF = "tests/infra/test_harness_kloc_cap.sh"
PTODO_LEAF = "tests/infra/test_reify_audit_ptodo.sh"
PDIAG_LEAF = "tests/infra/test_reify_audit_pdiag.sh"
PDOCCOVER_LEAF = "tests/infra/test_reify_audit_pdoccover.sh"
KLOC_LIB = "tests/infra/harness-layout-lib.sh"
CLOSURE_WITH_GUI = "crates/reify-eval/src/lib.rs"
OCCT_DECLARED_LIST = "scripts/occt-touching-crates.txt"


def base_env(**overrides):
    """hermetic_env() without any REIFY_* or DF_* key, then `overrides`."""
    env = {
        k: v
        for k, v in hermetic_env().items()
        if not k.startswith(("REIFY_", "DF_"))
    }
    env.update(overrides)
    return env


@dataclass(frozen=True)
class Outcome:
    returncode: int
    stdout: str
    stderr: str


def _run(cmd, env, cwd):
    proc = subprocess.run(
        cmd,
        cwd=cwd,
        env=env,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=HANG_GUARD_SECS,
    )
    return Outcome(proc.returncode, proc.stdout.decode("utf-8"), proc.stderr.decode("utf-8"))


def run_py(argv, env, tree=REPO_ROOT, cwd=None):
    """`verify.py` of `tree`, run from `cwd` (default: the tree's root)."""
    verify_py = Path(tree) / "scripts" / "verify.py"
    return _run(
        [sys.executable, "-I", "-B", str(verify_py), *argv],
        env,
        tree if cwd is None else cwd,
    )


def run_bash(argv, env, tree=REPO_ROOT):
    return _run(
        ["bash", "scripts/verify.sh", *argv],
        {**env, "REIFY_NEXTEST_PROBE_RETRY_SLEEP": "0"},
        tree,
    )


def scope_record(outcome):
    """The one JSON record a successful --print-scope prints."""
    assert outcome.returncode == 0, outcome.stderr
    lines = outcome.stdout.splitlines()
    assert len(lines) == 1, outcome.stdout
    record = json.loads(lines[0])
    assert record["v"] == 1, record
    return record


def header_fields(stdout, prefix=SCOPE_HEADER):
    """The KEY=V tokens of bash's one header line starting `prefix`, by key."""
    lines = [line for line in stdout.splitlines() if line.startswith(prefix)]
    assert len(lines) == 1, f"{len(lines)} plan lines start {prefix!r}"
    return dict(token.split("=", 1) for token in lines[0][len(prefix):].split())


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


def leaf_globs(record):
    return [leaf["glob"] for leaf in record["infra_selection"]]


def is_subsequence(lines, of):
    """Every item of `lines` appears in `of`, in the same order."""
    remaining = iter(of)
    return all(line in remaining for line in lines)


def python_help():
    outcome = run_py(["--help"], base_env())
    assert outcome.returncode == 0, outcome.stderr
    return outcome.stdout


class Clone:
    """B1's shared clone of the tree under test (PRD D13), reset between tests."""

    def __init__(self, root: Path):
        self.root = root
        self.base = self.git("rev-parse", "HEAD")
        self.main = self.git("rev-parse", "--verify", "refs/heads/main")

    def git(self, *args, stdin=None) -> str:
        return scratch_git(self.root, *args, stdin=stdin).decode("utf-8").strip()

    def reset(self):
        self.git("reset", "-q", "--hard", self.base)
        self.git("clean", "-fdqx")
        self.git("branch", "-f", "main", self.main)
        self.merge_head().unlink(missing_ok=True)

    def merge_head(self) -> Path:
        return self.root / self.git("rev-parse", "--git-path", "MERGE_HEAD")

    def append(self, path, text="\n"):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        with open(target, "a", encoding="utf-8") as handle:
            handle.write(text)

    def stage(self, path, text="\n"):
        """Append `text` to `path` (creating it) and stage it."""
        self.append(path, text)
        self.git("add", "--", path)

    def stage_ignored(self, path):
        self.append(path)
        self.git("add", "-f", "--", path)

    def commit(self, message):
        self.git("commit", "-q", "--allow-empty", "-m", message)

    def commit_change(self, path, text="\n"):
        self.stage(path, text)
        self.commit(f"scope-record probe: {path}")

    def start_branch(self):
        """Point main at HEAD, so `--scope branch` diffs from here to the working tree."""
        self.git("branch", "-f", "main", "HEAD")

    def name_list(self, *diff_args):
        return [line for line in self.git("diff", "--name-only", *diff_args).splitlines() if line]

    def py(self, argv, env, cwd=None):
        return run_py(argv, env, tree=self.root, cwd=cwd)

    def read_each_staged_alone(self, paths, argv):
        """{path: verify.py's outcome with only `path` staged, one byte appended}, read concurrently.

        Each run reads its own copy of the index through GIT_INDEX_FILE, the
        variable a hook runs verify under (consumer C18), so the runs share
        nothing mutable and the clone's own index is untouched.
        """
        index = self.root / self.git("rev-parse", "--git-path", "index")
        with tempfile.TemporaryDirectory() as scratch:
            blobs = self._appended_blobs(paths, Path(scratch))

            def read(numbered):
                n, path = numbered
                private = Path(scratch) / f"index-{n}"
                shutil.copyfile(index, private)
                env = base_env(GIT_INDEX_FILE=str(private))
                subprocess.run(
                    ["git", "-C", str(self.root), "update-index", "--add", "--cacheinfo",
                     f"100644,{blobs[n]},{path}"],
                    env=env,
                    stdin=subprocess.DEVNULL,
                    capture_output=True,
                    check=True,
                    timeout=HANG_GUARD_SECS,
                )
                return path, self.py(argv, env)

            with ThreadPoolExecutor(max_workers=CONCURRENT_READS) as pool:
                return dict(pool.map(read, enumerate(paths)))

    def _appended_blobs(self, paths, scratch):
        copies = []
        for n, path in enumerate(paths):
            source = self.root / path
            copy = scratch / f"blob-{n}"
            copy.write_bytes((source.read_bytes() if source.exists() else b"") + b"\n")
            copies.append(str(copy))
        stdin = "\n".join(copies).encode("utf-8")
        return self.git("hash-object", "-w", "--stdin-paths", stdin=stdin).splitlines()

    def bash(self, argv, env):
        return run_bash(argv, env, tree=self.root)

    def branch_leaves(self, paths):
        """The infra leaf globs verify.py selects for a branch that only appends to or creates `paths`."""
        self.reset()
        self.start_branch()
        for path in paths:
            self.append(path)
        self.git("add", "-f", "--", *paths)
        return leaf_globs(scope_record(self.py(BRANCH_SCOPE, base_env())))


_SCRATCH = None
_CLONE = None


def the_clone() -> Clone:
    """The module's one clone, built on first use (each `git clone` costs a checkout)."""
    global _SCRATCH, _CLONE
    if _CLONE is None:
        _SCRATCH = tempfile.TemporaryDirectory()
        _CLONE = Clone(shared_clone(Path(_SCRATCH.name) / "tree"))
    return _CLONE


def tearDownModule():
    if _SCRATCH is not None:
        _SCRATCH.cleanup()


class InClone(unittest.TestCase):
    """A test case that runs in the shared clone, reset to its base before each test."""

    @classmethod
    def setUpClass(cls):
        cls.clone = the_clone()

    def setUp(self):
        self.clone.reset()

    def side_by_side(self, scope, env=None, flags=()):
        """(python's record, python's outcome, bash's plan outcome) for one gate invocation."""
        env = base_env() if env is None else env
        py = self.clone.py(["test", "--scope", scope, *flags, "--print-scope"], env)
        bash = self.clone.bash(
            ["test", "--scope", scope, *flags, "--include-infra", "--print-plan"], env
        )
        self.assertEqual(bash.returncode, 0, f"premise: bash plans this scenario\n{bash.stderr}")
        return scope_record(py), py, bash

    def assert_flags_match_bash(self, record, bash, label=""):
        header = header_fields(bash.stdout)
        for flag, bash_key in BASH_FLAGS.items():
            self.assertEqual(record["scope"][flag], int(header[bash_key]), f"{label}: {flag}")

    def assert_stderr_within_bash(self, py, bash, label=""):
        self.assertTrue(
            is_subsequence(py.stderr.splitlines(), bash.stderr.splitlines()),
            f"{label}: python's stderr {py.stderr!r} is not an in-order subsequence of "
            f"bash's {bash.stderr!r}",
        )


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

    STAGED = ["test", "--scope", "staged", "--print-scope"]

    def setUp(self):
        super().setUp()
        self.clone.stage(DOCS_PATH)

    def test_merge_head_is_found_from_the_repo_root(self):
        self.clone.merge_head().write_text(self.clone.base + "\n", encoding="utf-8")
        with tempfile.TemporaryDirectory() as foreign:
            py = self.clone.py(self.STAGED, base_env(), cwd=foreign)
        self.assertEqual(scope_record(py)["scope"]["mode"], "all")
        self.assertIn(MERGE_HEAD_NOTICE, py.stderr.splitlines())
        bash = self.clone.bash(["test", "--scope", "staged", "--print-plan"], base_env())
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
                py = self.clone.py(self.STAGED, env)
                self.assertEqual(scope_record(py)["scope"]["mode"], "all" if forced else "staged")
                bash = self.clone.bash(["test", "--scope", "staged", "--print-plan"], env)
                self.assertEqual(bash.returncode, 0, bash.stderr)
                bash_notices = [l for l in bash.stderr.splitlines() if FORCED_SCOPE_MARK in l]
                py_notices = [l for l in py.stderr.splitlines() if FORCED_SCOPE_MARK in l]
                self.assertEqual(len(bash_notices), int(forced), bash.stderr)
                self.assertEqual(py_notices, bash_notices)

    def test_branch_without_main_fails_wide(self):
        self.clone.git("branch", "-D", "main")
        argv = ["test", "--scope", "branch", "--print-scope"]
        py = self.clone.py(argv, base_env())
        self.assertEqual(scope_record(py)["scope"]["mode"], "all")
        self.assertIn(MERGE_BASE_WARNING, py.stderr.splitlines())
        bash = self.clone.bash(["test", "--scope", "branch", "--print-plan"], base_env())
        self.assertEqual(bash.returncode, 0, bash.stderr)
        self.assertIn(MERGE_BASE_WARNING, bash.stderr.splitlines(), "premise: bash's exact C5 line")

    def test_branch_records_the_merge_base(self):
        self.clone.git("branch", "-f", "main", "HEAD")
        py = self.clone.py(["test", "--scope", "branch", "--print-scope"], base_env())
        scope = scope_record(py)["scope"]
        self.assertEqual(scope["mode"], "branch")
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


@dataclass(frozen=True)
class FailingGit:
    """One git argv shape that exits 128; every other git call runs the real git."""

    label: str
    scope: str
    match: str
    unless: str | None
    warning: str
    bash_fails_wide: bool

    def farm(self, dest: Path) -> Path:
        real = shutil.which("git", path=base_env()["PATH"])
        dest.mkdir()
        spared = f'    *" {self.unless} "*) exec {shlex.quote(real)} "$@" ;;\n' if self.unless else ""
        wrapper = dest / "git"
        wrapper.write_text(
            "#!/usr/bin/env bash\n"
            'case " $* " in\n'
            f"{spared}"
            f'    *" {self.match} "*) exit 128 ;;\n'
            "esac\n"
            f'exec {shlex.quote(real)} "$@"\n',
            encoding="utf-8",
        )
        wrapper.chmod(0o755)
        return dest

    def env(self, scratch: Path) -> dict:
        """base_env() with this case's farm, built under `scratch`, first on PATH."""
        farm = self.farm(scratch / self.label.replace(" ", "-"))
        return base_env(PATH=f"{farm}{os.pathsep}{base_env()['PATH']}")


def _c5(scope, what):
    return f"verify.sh: WARNING — --scope {scope} {what} failed — failing WIDE to --scope all (contract C5)"


FAILING_GIT = (
    FailingGit(
        "staged primary", "staged", "diff --cached --name-only", None,
        _c5("staged", "git diff"), bash_fails_wide=False,
    ),
    FailingGit(
        "staged rename source", "staged", "diff --cached --name-status", None,
        _c5("staged", "rename-source diff"), bash_fails_wide=True,
    ),
    FailingGit(
        "branch primary", "branch", "--name-only --diff-filter=ACMRD", "--cached",
        _c5("branch", "git diff"), bash_fails_wide=True,
    ),
    FailingGit(
        "branch rename source", "branch", "--name-status --diff-filter=R", "--cached",
        _c5("branch", "rename-source diff"), bash_fails_wide=True,
    ),
)


class TestGatherFailsWide(InClone):
    """Every changed-path git failure fails wide under one policy (C5; D5 amended 2026-10-15)."""

    def test_each_failing_git_call(self):
        with tempfile.TemporaryDirectory() as scratch:
            for case in FAILING_GIT:
                with self.subTest(case=case.label):
                    self.clone.reset()
                    self.clone.start_branch()
                    self.clone.stage(DOCS_PATH)
                    record, py, bash = self.side_by_side(case.scope, case.env(Path(scratch)))
                    scope = record["scope"]
                    self.assertEqual(scope["mode"], case.scope)
                    for flag in SCOPE_FLAGS:
                        self.assertEqual(scope[flag], 1, flag)
                    self.assertEqual(scope["changed_paths"], [])
                    py_wide = [l for l in py.stderr.splitlines() if FAIL_WIDE_MARK in l]
                    self.assertEqual(py_wide, [case.warning])
                    bash_wide = [l for l in bash.stderr.splitlines() if FAIL_WIDE_MARK in l]
                    if case.bash_fails_wide:
                        self.assert_flags_match_bash(record, bash, case.label)
                        self.assertEqual(py_wide, bash_wide)
                    else:
                        self.assertEqual(
                            header_fields(bash.stdout)["RUN_RUST"],
                            "0",
                            "PRD D5's lead amendment of 2026-10-15 (#9170): the staged primary "
                            "diff fails wide in python, while bash still fails open",
                        )
                        self.assertEqual(bash_wide, [])


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
        self.commit_probe_rs()
        scope, _ = self.staged_alone_on_top(fixture(self.REFERENCED))
        self.assertEqual(flags(scope), (1, 1, 1))
        bash = self.clone.bash(["test", "--scope", "staged", "--print-plan"], base_env())
        self.assertEqual(bash.returncode, 0, bash.stderr)
        self.assertEqual(
            header_fields(bash.stdout)["RUN_RUST"],
            "0",
            "premise: bash classifies from its own list, which this probe never edits",
        )

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
        grep = FailingGit(
            "reference grep", "staged", "grep --cached -h -E", None,
            f"verify.sh: WARNING — git grep --cached -E '{FIXTURE_REFERENCE}' -- '*.rs' exited 128 "
            "— every prd-gate fixture counts as Rust-coupled (fail wide, contract C5)",
            bash_fails_wide=False,
        )
        self.clone.stage(fixture(self.UNREFERENCED))
        with tempfile.TemporaryDirectory() as scratch:
            py = self.clone.py(STAGED_SCOPE, grep.env(Path(scratch)))
        self.assertEqual(flags(scope_record(py)["scope"]), (1, 1, 1))
        self.assertIn(grep.warning, py.stderr.splitlines())

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
        rename_failure = next(case for case in FAILING_GIT if case.label == "staged rename source")
        with tempfile.TemporaryDirectory() as scratch:
            env = rename_failure.env(Path(scratch))
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
    "branch added top-level test": ("branch", branch_commit("crates/reify-eval/tests/zz_probe.rs")),
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


class TestSelectorCopiesMatchReifyAudit(InClone):
    """selection.py's copies of reify-audit's predicates select what the Rust source reads.

    The corpora are derived as test_verify_scope.sh's PT-DRIFT, PDIAG-DRIFT
    and PDOCCOVER-DRIFT derive them for bash's copies, so neither copy can
    drift from ptodo.rs, pdiag.rs or pdoccover.rs alone. Each path that must
    select a leaf runs alone; the paths that must not run together, since any
    one of them selecting it is the failure.
    """

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.clone.reset()
        cls.extensions = reify_audit_inputs.ptodo_swept_extensions(cls.clone.root)
        cls.pdiag = reify_audit_inputs.pdiag_corpus(cls.clone.root)
        cls.pdoccover = reify_audit_inputs.pdoccover_inputs(cls.clone.root)

    def test_premise_the_corpora_are_not_vacuous(self):
        self.assertTrue(self.extensions, "is_swept_ext yielded no extension")
        self.assertTrue(self.pdiag.swept and self.pdiag.not_swept, self.pdiag)
        self.assertEqual(self.pdiag.read_calls, self.pdiag.test_calls, "an is_swept_path test the scrape cannot read")
        self.assertTrue(self.pdiag.excluded_prefixes, self.pdiag)
        self.assertTrue(self.pdoccover.roots, self.pdoccover)
        self.assertEqual(len(self.pdoccover.exact), 2, self.pdoccover)
        self.assertEqual(
            self.pdoccover.oracle_pairs, self.pdoccover.oracle_starts, "an in_oracle_scope root the scrape cannot read"
        )

    def test_every_swept_extension_selects_the_ptodo_gate(self):
        paths = [f"docs/pt_probe.{extension}" for extension in self.extensions]
        for path, outcome in self.clone.read_each_staged_alone(paths, STAGED_SCOPE).items():
            with self.subTest(path=path):
                self.assertIn(PTODO_LEAF, leaf_globs(scope_record(outcome)))

    def test_is_swept_path_corpus_selects_the_pdiag_ratchet(self):
        for path in self.pdiag.swept:
            with self.subTest(swept=path):
                self.assertIn(PDIAG_LEAF, self.clone.branch_leaves([path]))
        refused = [
            *self.pdiag.not_swept,
            *(f"{prefix}src/pdiag_drift_probe.rs" for prefix in self.pdiag.excluded_prefixes),
        ]
        self.assertNotIn(
            PDIAG_LEAF, self.clone.branch_leaves(refused), f"one of {refused} selected the PDIAG ratchet"
        )

    def test_every_pdoccover_input_selects_the_pdoccover_ratchet(self):
        inputs = [
            *(f"{prefix}pdoccover_drift/probe{suffix}" for _, prefix, suffix in self.pdoccover.roots),
            *(path for _, path in self.pdoccover.exact),
        ]
        for path in inputs:
            with self.subTest(input=path):
                self.assertIn(PDOCCOVER_LEAF, self.clone.branch_leaves([path]))
        refused = [
            *(f"{prefix}pdoccover_drift_probe{suffix}.orig" for _, prefix, suffix in self.pdoccover.roots),
            *(f"{path}.orig" for _, path in self.pdoccover.exact),
        ]
        self.assertNotIn(
            PDOCCOVER_LEAF, self.clone.branch_leaves(refused), f"one of {refused} selected the PDOCCOVER ratchet"
        )


class TestHarnessKlocSetIsTheLibs(InClone):
    """The kloc guard's crate set has one owner, harness-layout-lib.sh (PRD D6, §2.3)."""

    def test_a_crate_added_to_the_lib_is_selected(self):
        lib = self.clone.root / KLOC_LIB
        text = lib.read_text(encoding="utf-8")
        declared = "declare -ga _HL_CRATES=("
        self.assertEqual(text.count(declared), 1, "premise: the lib's one _HL_CRATES definition")
        lib.write_text(text.replace(declared, declared + "reify-doc "), encoding="utf-8")
        self.clone.git("add", "--", KLOC_LIB)
        self.clone.commit("scope-record probe: reify-doc is consolidatable")
        branch_commit("crates/reify-doc/tests/zz.rs")(self.clone)
        record, _, bash = self.side_by_side("branch")
        self.assertIn(
            (KLOC_LEAF, {}),
            python_leaves(record),
            "python reads the crate set from harness_layout_consolidatable_crates (D6, §2.3)",
        )
        self.assertNotIn(
            KLOC_LEAF,
            [glob for glob, _ in bash_leaves(bash.stdout)],
            "premise: bash's select_harness_kloc_guard still holds its own literal copy",
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
