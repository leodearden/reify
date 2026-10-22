"""
scope_record_harness.py — the shared clone and the side-by-side runner the
--print-scope members drive (S18 β, task 9097; PRD
docs/prds/v0_6/verify-structured-plan-and-role-table.md D13).

Not a test module itself (no `test_` prefix): run_all.sh never discovers it,
and the members import it via the sys.path idiom. It imports nothing from them.

Every child env is hermetic_env() with every REIFY_* and DF_* key also removed,
so a knob the gate's own verify_env sets cannot change a decision under test.
Each process builds one clone on first use; a member's tearDownModule discards
it.
"""

import json
import os
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
from tree_under_test import scratch_git, shared_clone  # noqa: E402
from verify_ledger_env import hermetic_env  # noqa: E402

REPO_ROOT = INFRA_DIR.parents[1]
HANG_GUARD_SECS = 600
CONCURRENT_READS = 8
ERROR_PREFIX = "verify.sh: ERROR — "
SCOPE_HEADER = "# scope decision — "
BASH_FLAGS = {"run_rust": "RUN_RUST", "run_gui": "RUN_GUI", "run_occt_gate": "RUN_OCCT_GATE"}
STAGED_SCOPE = ["test", "--scope", "staged", "--print-scope"]
BRANCH_SCOPE = ["test", "--scope", "branch", "--print-scope"]
DOCS_PATH = "docs/getting-started.md"
CLOSURE_WITHOUT_GUI = "crates/reify-doc/src/lib.rs"


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


def leaf_globs(record):
    return [leaf["glob"] for leaf in record["infra_selection"]]


def is_subsequence(lines, of):
    """Every item of `lines` appears in `of`, in the same order."""
    remaining = iter(of)
    return all(line in remaining for line in lines)


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
    """The process's one clone, built on first use (each `git clone` costs a checkout)."""
    global _SCRATCH, _CLONE
    if _CLONE is None:
        _SCRATCH = tempfile.TemporaryDirectory()
        _CLONE = Clone(shared_clone(Path(_SCRATCH.name) / "tree"))
    return _CLONE


def discard_clone():
    """Delete the process's clone, if one was built; the next the_clone() builds afresh."""
    global _SCRATCH, _CLONE
    if _SCRATCH is not None:
        _SCRATCH.cleanup()
    _SCRATCH = None
    _CLONE = None


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


@dataclass(frozen=True)
class FailingGit:
    """One git argv shape that prints `reason` to stderr and exits 128; every other git call runs the real git."""

    label: str
    match: str
    unless: str | None = None

    @property
    def reason(self) -> str:
        return f"fatal: scope-record probe: {self.label}"

    def farm(self, dest: Path) -> Path:
        real = shutil.which("git", path=base_env()["PATH"])
        dest.mkdir()
        spared = f'    *" {self.unless} "*) exec {shlex.quote(real)} "$@" ;;\n' if self.unless else ""
        wrapper = dest / "git"
        wrapper.write_text(
            "#!/usr/bin/env bash\n"
            'case " $* " in\n'
            f"{spared}"
            f'    *" {self.match} "*) echo {shlex.quote(self.reason)} >&2; exit 128 ;;\n'
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


@dataclass(frozen=True)
class FailWideCase:
    """A changed-path git call that fails, so a `scope` request goes wide with `warning`.

    bash_parity: bash fails wide on it too, so bash's plan is an oracle for it.
    Where python deliberately diverges, only python's side is asserted, so the
    case stays green when bash converges.
    """

    git: FailingGit
    scope: str
    warning: str
    bash_parity: bool


def _c5(scope, what):
    return f"verify.sh: WARNING — --scope {scope} {what} failed — failing WIDE to --scope all (contract C5)"


GATHER_FAILURES = (
    FailWideCase(
        FailingGit("staged primary", "diff --cached --name-only"), "staged",
        _c5("staged", "git diff"), bash_parity=False,
    ),
    FailWideCase(
        FailingGit("staged rename source", "diff --cached --name-status"), "staged",
        _c5("staged", "rename-source diff"), bash_parity=True,
    ),
    FailWideCase(
        FailingGit("branch primary", "--name-only --diff-filter=ACMRD", "--cached"), "branch",
        _c5("branch", "git diff"), bash_parity=True,
    ),
    FailWideCase(
        FailingGit("branch rename source", "--name-status --diff-filter=R", "--cached"), "branch",
        _c5("branch", "rename-source diff"), bash_parity=True,
    ),
)
