"""selection — the per-artifact infra tests a task-tier run selects from what changed (PRD D5, D14).

select_infra runs bash's six selectors in bash's order and returns the
Selection and the selection stage's stderr lines. Each selector appends a
test glob at most once (bash's whole-token dedup).

The kloc guard's crate set is read from its one owner, harness-layout-lib.sh.
The ptodo, pdiag and pdoccover predicates are copies of reify-audit's
(crates/reify-audit/src/ptodo.rs::is_swept_ext, pdiag.rs::is_swept_path and
SCOPE_EXCLUDE_PREFIXES, pdoccover.rs::is_chunk_path and in_oracle_scope,
pdoccover_baseline.rs::BASELINE_PATH), as bash's are: test_verify_scope.sh's
PT-DRIFT, PDIAG-DRIFT and PDOCCOVER-DRIFT guard bash's copies, and this
module is held equal to bash's (PRD §2.3, §8).
"""

import re
from collections.abc import Callable, Mapping
from dataclasses import dataclass
from fnmatch import fnmatchcase
from pathlib import Path

from reify_verify import bashcall, scope
from reify_verify.bashcall import BashCall, DataFile
from reify_verify.invocation import Scope
from reify_verify.scope import ScopeDecision

INFRA_MAP = DataFile("scripts/verify-pipeline-infra-tests.txt")
KLOC_CRATES = BashCall("tests/infra/harness-layout-lib.sh", "harness_layout_consolidatable_crates")

KLOC_LEAF = "tests/infra/test_harness_kloc_cap.sh"
PTODO_LEAF = "tests/infra/test_reify_audit_ptodo.sh"
PDIAG_LEAF = "tests/infra/test_reify_audit_pdiag.sh"
PDOCCOVER_LEAF = "tests/infra/test_reify_audit_pdoccover.sh"
CITED_TEST_PATH_LEAF = "tests/infra/test_cited_test_paths_resolve.sh"

PTODO_SWEPT_EXTENSIONS = (".rs", ".ri", ".sh", ".py", ".ts", ".tsx", ".js")
PDIAG_BASELINE = "crates/reify-audit/pdiag-baseline.txt"
PDIAG_EXCLUDED_PREFIXES = ("crates/reify-audit/", "crates/reify-test-support/")
PDIAG_SOURCE = re.compile(r"crates/[^/]*/src/")
PDIAG_GUI_SOURCE_PREFIX = "gui/src-tauri/src/"
TESTS_SEGMENT = re.compile(r"(^|/)tests/")
PDOCCOVER_INPUTS = (
    "crates/reify-mcp/src/tools/chunks/*.md",
    "crates/reify-compiler/src/*.rs",
    "crates/reify-compiler/stdlib/*.ri",
    "crates/reify-stdlib/src/*.rs",
    "crates/reify-audit/pdoccover-baseline.txt",
)
KLOC_TEST_GLOB = "crates/*/tests/*.rs"

_MAP_SKIPPED = re.compile(r"\s*(#.*)?", re.ASCII)
_FIELD_SEPARATOR = re.compile(r"[ \t]+")


@dataclass(frozen=True)
class SelectedLeaf:
    """One selective-infra plan leaf: a test glob and the env it runs with."""

    glob: str
    env: tuple[tuple[str, str], ...] = ()


@dataclass(frozen=True)
class Selection:
    leaves: tuple[SelectedLeaf, ...]

    def to_json(self) -> list:
        return [{"glob": leaf.glob, "env": dict(leaf.env)} for leaf in self.leaves]


def select_infra(
    decision: ScopeDecision, repo_root: Path, env: Mapping[str, str]
) -> tuple[Selection, tuple[str, ...]]:
    globs = list(_mapped_globs(decision.changed_paths, repo_root))
    kloc, warnings = _harness_kloc(decision, repo_root, env)
    candidates = (
        *kloc,
        *_ptodo(decision),
        *_branch_ratchet(decision, repo_root, env, PDIAG_LEAF, _pdiag_input_path, "--diff-filter=AM"),
        *_branch_ratchet(decision, repo_root, env, PDOCCOVER_LEAF, _pdoccover_input_path),
        *_cited_test_paths(decision),
    )
    for glob in candidates:
        if glob not in globs:
            globs.append(glob)
    leaves = tuple(SelectedLeaf(glob, _leaf_env(glob, decision.mode)) for glob in globs)
    return Selection(leaves), warnings


def _leaf_env(glob: str, mode: Scope) -> tuple[tuple[str, str], ...]:
    """selected_infra_leaf_env: the staged ptodo leaf requires its ratchet."""
    if glob == PTODO_LEAF and mode is Scope.STAGED:
        return (("REIFY_PTODO_RATCHET_REQUIRED", "1"),)
    return ()


def _mapped_globs(changed_paths: tuple[str, ...], repo_root: Path) -> list[str]:
    """select_infra_tests: each map row, in file order, whose artifact is a changed path."""
    if not changed_paths:
        return []
    try:
        text = INFRA_MAP.read_text(repo_root)
    except FileNotFoundError:
        return []
    changed = set(changed_paths)
    globs: list[str] = []
    for artifact, glob in _map_rows(text):
        if artifact in changed and glob not in globs:
            globs.append(glob)
    return globs


def _map_rows(text: str) -> list[tuple[str, str]]:
    """Non-blank, non-comment lines as (first field, rest), as bash's `read -r _artifact _glob`."""
    rows = []
    for line in text.split("\n"):
        if _MAP_SKIPPED.fullmatch(line):
            continue
        fields = _FIELD_SEPARATOR.split(line.strip(" \t"), maxsplit=1)
        if len(fields) == 2 and fields[0] and fields[1]:
            rows.append((fields[0], fields[1]))
    return rows


def _harness_kloc(
    decision: ScopeDecision, repo_root: Path, env: Mapping[str, str]
) -> tuple[tuple[str, ...], tuple[str, ...]]:
    """select_harness_kloc_guard: a branch adds or edits a consolidatable crate's test unit."""
    records = scope.merge_base_diff(decision, repo_root, env, "--name-status", "--diff-filter=AM")
    crates = None
    warnings: tuple[str, ...] = ()
    for record in records or ():
        status, _, path = record.partition("\t")
        crate = _kloc_candidate_crate(status, path.rstrip("\t"))
        if crate is None:
            continue
        if crates is None:
            result = bashcall.run(KLOC_CRATES, (), repo_root, env)
            crates = frozenset(line for line in result.stdout.split("\n") if line)
            warnings = result.stderr_lines
        if crate in crates:
            return (KLOC_LEAF,), warnings
    return (), warnings


def _kloc_candidate_crate(status: str, path: str) -> str | None:
    """The crate of a test unit the C2 accounting measures: a harness module (A or M) or a
    top-level standalone (A only); None for anything else."""
    if not path or not fnmatchcase(path, KLOC_TEST_GLOB):
        return None
    crate = path.removeprefix("crates/").split("/", 1)[0]
    tail = path.removeprefix(f"crates/{crate}/tests/")
    if fnmatchcase(tail, "harness_*/*") or fnmatchcase(tail, "harness_*.rs"):
        return crate
    if "/" in tail:
        return None
    return crate if status == "A" else None


def _ptodo(decision: ScopeDecision) -> tuple[str, ...]:
    """select_cheap_ptodo_gate: a staged path with a swept extension, case-folded."""
    if decision.mode is Scope.STAGED and any(
        path.lower().endswith(PTODO_SWEPT_EXTENSIONS) for path in decision.changed_paths
    ):
        return (PTODO_LEAF,)
    return ()


def _branch_ratchet(
    decision: ScopeDecision,
    repo_root: Path,
    env: Mapping[str, str],
    leaf: str,
    is_input: Callable[[str], bool],
    *filters: str,
) -> tuple[str, ...]:
    """select_branch_ratchet_leaf: a branch diff path the ratchet reads."""
    paths = scope.merge_base_diff(decision, repo_root, env, "--name-only", "--no-renames", *filters)
    if any(path and is_input(path) for path in paths or ()):
        return (leaf,)
    return ()


def _pdiag_input_path(path: str) -> bool:
    return path == PDIAG_BASELINE or _pdiag_swept_path(path)


def _pdiag_swept_path(path: str) -> bool:
    if not path.endswith(".rs") or path.startswith(PDIAG_EXCLUDED_PREFIXES):
        return False
    if not (PDIAG_SOURCE.match(path) or path.startswith(PDIAG_GUI_SOURCE_PREFIX)):
        return False
    if TESTS_SEGMENT.search(path):
        return False
    stem = path.rsplit("/", 1)[-1].removesuffix(".rs")
    return stem != "tests" and not stem.endswith("_tests")


def _pdoccover_input_path(path: str) -> bool:
    return any(fnmatchcase(path, glob) for glob in PDOCCOVER_INPUTS)


def _cited_test_paths(decision: ScopeDecision) -> tuple[str, ...]:
    """select_cited_test_path_gate: any staged change."""
    if decision.mode is Scope.STAGED and decision.changed_paths:
        return (CITED_TEST_PATH_LEAF,)
    return ()
