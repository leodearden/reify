"""scope — which check families a verify run needs, decided from what changed (PRD D5).

The stages, each a function of the one before: resolve_scope settles the
effective scope, gather_changed reads the changed paths, gather_facts reads
what the classifier needs to know about them, and the pure decide_scope maps
those facts to check families. decide runs the stages and returns the
ScopeDecision and the scope stage's stderr lines, in the order bash prints
them.

A changed-path git call that fails returns git's own stderr with its warning,
as bash leaves it on the terminal; resolve_scope drops it, as bash's
`2>/dev/null` does.
"""

import json
import re
from collections.abc import Mapping
from dataclasses import dataclass
from enum import Enum
from fnmatch import fnmatchcase
from functools import reduce
from pathlib import Path

from reify_path_attribution import PathFacts, Tree, attribute
from reify_verify import bashcall, gitcall
from reify_verify.bashcall import BashCall, CallResult, DataFile
from reify_verify.invocation import Invocation, Scope
from reify_verify.policy import ROLE_TABLE

OCCT_DECLARED_SET = BashCall("scripts/occt-scope-lib.sh", "occt_declared_set")
GUI_EVENT_CHANNELS = "docs/gui-event-channels.md"
RENAME_SOURCE = re.compile(r"tests/prd-gate/fixtures/.*\.ri|docs/gui-event-channels\.md")
TASK_DIR = ".task/"
FIXTURES_DIR = "tests/prd-gate/fixtures/"
FIXTURE_GLOB = "tests/prd-gate/fixtures/*.ri"
FIXTURE_REFERENCE = re.compile(r"tests/prd-gate/fixtures/[A-Za-z0-9_.-]+\.ri")
REVIEWED_MARKER = "pg-drift:allow"
GUI_PINS = DataFile("gui/src/__tests__/reifyGrammarCorpus.expected-clean.json")

MERGE_HEAD_NOTICE = "verify.sh: MERGE_HEAD present — forcing --scope all (merge in progress)"
MERGE_BASE_WARNING = (
    "verify.sh: WARNING — --scope branch could not resolve 'git merge-base main HEAD' "
    "(detached HEAD / missing local main ref / merge-base failure) — failing WIDE to "
    "--scope all (contract C5)"
)


class Widening(Enum):
    """Why a staged or branch request runs every check family, as scope all does."""

    MERGE_IN_PROGRESS = "merge_in_progress"
    ROLE_FORCES_ALL = "role_forces_all"
    NO_MERGE_BASE = "no_merge_base"
    CHANGED_PATHS_UNREAD = "changed_paths_unread"


@dataclass(frozen=True)
class ResolvedScope:
    """The scope a run actually uses, why it widened (None: it did not), and the notices saying so."""

    mode: Scope
    merge_base: str | None
    widened: Widening | None
    notices: tuple[str, ...]


def resolve_scope(invocation: Invocation, repo_root: Path, env: Mapping[str, str]) -> ResolvedScope:
    """A merge in progress, then a force_scope_all role, force scope all; branch needs a merge base."""
    mode = invocation.scope
    widened = None
    notices = []
    if mode is not Scope.ALL and _merge_in_progress(repo_root, env):
        notices.append(MERGE_HEAD_NOTICE)
        mode, widened = Scope.ALL, Widening.MERGE_IN_PROGRESS
    if mode is not Scope.ALL and ROLE_TABLE[invocation.role].force_scope_all:
        notices.append(
            f"verify.sh: DF_VERIFY_ROLE={invocation.role.value} — forcing --scope all "
            "(integrity gate never narrows, contract C2)"
        )
        mode, widened = Scope.ALL, Widening.ROLE_FORCES_ALL
    merge_base = None
    if mode is Scope.BRANCH:
        found = gitcall.run(repo_root, env, "merge-base", "main", "HEAD")
        if found.returncode == 0 and found.lines:
            merge_base = found.lines[0]
        else:
            notices.append(MERGE_BASE_WARNING)
            mode, widened = Scope.ALL, Widening.NO_MERGE_BASE
    return ResolvedScope(mode, merge_base, widened, tuple(notices))


def _merge_in_progress(repo_root: Path, env: Mapping[str, str]) -> bool:
    """A MERGE_HEAD file exists; a relative git path is resolved against repo_root, never the cwd."""
    found = gitcall.run(repo_root, env, "rev-parse", "--git-path", "MERGE_HEAD")
    if found.returncode != 0 or not found.lines:
        return False
    return (repo_root / found.lines[0]).is_file()


@dataclass(frozen=True)
class ScopeDecision:
    mode: Scope
    merge_base: str | None
    widened: Widening | None
    run_rust: bool
    run_gui: bool
    run_occt_gate: bool
    gui_path_signal: bool
    changed_paths: tuple[str, ...]
    rename_sources: tuple[str, ...]

    def __post_init__(self):
        if self.run_rust and not self.run_gui:
            raise ValueError(
                f"ScopeDecision: run_rust implies run_gui (run_rust={self.run_rust}, "
                f"run_gui={self.run_gui})"
            )
        if self.gui_path_signal and not self.run_gui:
            raise ValueError(
                f"ScopeDecision: gui_path_signal implies run_gui "
                f"(gui_path_signal={self.gui_path_signal}, run_gui={self.run_gui})"
            )
        if self.mode is Scope.ALL and (self.changed_paths or self.rename_sources):
            raise ValueError(
                f"ScopeDecision: scope all carries no paths (changed_paths="
                f"{self.changed_paths!r}, rename_sources={self.rename_sources!r})"
            )
        families = (self.run_rust, self.run_gui, self.run_occt_gate, self.gui_path_signal)
        has_paths = bool(self.changed_paths or self.rename_sources)
        if self.widened is not None and (has_paths or not all(families)):
            raise ValueError(
                f"ScopeDecision: a request widened by {self.widened.value} runs every family "
                f"over no paths ({self!r})"
            )

    @classmethod
    def wide(cls, resolved: ResolvedScope, widened: Widening | None) -> "ScopeDecision":
        """Every check family on, no paths: a requested scope all, or a request `widened` to it."""
        return cls(
            mode=resolved.mode,
            merge_base=resolved.merge_base,
            widened=widened,
            run_rust=True,
            run_gui=True,
            run_occt_gate=True,
            gui_path_signal=True,
            changed_paths=(),
            rename_sources=(),
        )

    def to_json(self) -> dict:
        return {
            "mode": self.mode.value,
            "widened": None if self.widened is None else self.widened.value,
            "merge_base": self.merge_base,
            "run_rust": int(self.run_rust),
            "run_gui": int(self.run_gui),
            "run_occt_gate": int(self.run_occt_gate),
            "gui_path_signal": int(self.gui_path_signal),
            "changed_paths": list(self.changed_paths),
            "rename_sources": list(self.rename_sources),
        }


@dataclass(frozen=True)
class ChangedSet:
    """The primary diff's paths (minus .task/) and the rename sources the classifier reads."""

    files: tuple[str, ...]
    rename_sources: tuple[str, ...]

    @property
    def classified(self) -> tuple[str, ...]:
        return (*self.files, *self.rename_sources)


@dataclass(frozen=True)
class GatherFailure:
    """git's own stderr lines, then the fail-wide warning: bash's order."""

    warnings: tuple[str, ...]


def gather_changed(
    resolved: ResolvedScope, repo_root: Path, env: Mapping[str, str]
) -> ChangedSet | GatherFailure:
    """Staged reads the index; branch diffs the merge base to the working tree.

    Every non-zero git status fails wide, the staged primary diff included
    (PRD D5, amended 2026-10-15).
    """
    mode = resolved.mode
    if mode is Scope.STAGED:
        before, after = ("--cached",), ()
    else:
        before, after = (), (resolved.merge_base,)
    primary = gitcall.run(repo_root, env, "diff", *before, "--name-only", "--diff-filter=ACMRD", *after)
    if primary.returncode != 0:
        return _fail_wide(mode, "git diff", primary)
    renames = gitcall.run(
        repo_root, env, "diff", *before, "--name-status", "--diff-filter=R", "-M", *after
    )
    if renames.returncode != 0:
        return _fail_wide(mode, "rename-source diff", renames)
    files = tuple(path for path in primary.lines if not path.startswith(TASK_DIR))
    sources = tuple(
        source for source in map(_second_field, renames.lines) if RENAME_SOURCE.fullmatch(source)
    )
    return ChangedSet(files, sources)


def _fail_wide(mode: Scope, what: str, found: CallResult) -> GatherFailure:
    return GatherFailure(
        (
            *found.stderr_lines,
            f"verify.sh: WARNING — --scope {mode.value} {what} failed — failing WIDE to "
            "--scope all (contract C5)",
        )
    )


def _second_field(line: str) -> str:
    """`cut -f2`: the second tab field, or the whole line when it has no tab."""
    fields = line.split("\t")
    return fields[1] if len(fields) > 1 else line


@dataclass(frozen=True)
class CoupledSet:
    """Fixture basenames; `everything` = the derivation failed, so every fixture counts (fail wide)."""

    basenames: frozenset[str]
    everything: bool = False

    def __contains__(self, basename: str) -> bool:
        return self.everything or basename in self.basenames


@dataclass(frozen=True)
class FixtureCoupling:
    rust: CoupledSet
    gui: CoupledSet


def derive_fixture_coupling(
    mode: Scope, repo_root: Path, env: Mapping[str, str]
) -> tuple[FixtureCoupling, tuple[str, ...]]:
    """The prd-gate fixtures a compiled test reads, and those the vitest ledger pins (PRD D6).

    Both are read from the content the scope classifies: the index under
    staged, which is what the commit holds, and otherwise the working tree.
    """
    rust, rust_warnings = _rust_coupled(mode, repo_root, env)
    gui, gui_warnings = _gui_pinned(mode, repo_root, env)
    return FixtureCoupling(rust, gui), (*rust_warnings, *gui_warnings)


def _rust_coupled(
    mode: Scope, repo_root: Path, env: Mapping[str, str]
) -> tuple[CoupledSet, tuple[str, ...]]:
    """PG-DRIFT's predicate: fixtures named on tracked *.rs lines not carrying the reviewed marker."""
    source = ("--cached",) if mode is Scope.STAGED else ()
    found = gitcall.run(
        repo_root, env, "grep", *source, "-h", "-E", FIXTURE_REFERENCE.pattern, "--", "*.rs"
    )
    if found.returncode == 1:
        return CoupledSet(frozenset()), ()
    if found.returncode != 0:
        command = " ".join(("git grep", *source, f"-E '{FIXTURE_REFERENCE.pattern}' -- '*.rs'"))
        return CoupledSet(frozenset(), everything=True), (
            f"verify.sh: WARNING — {found.exit_summary(command)} — every prd-gate fixture "
            "counts as Rust-coupled (fail wide, contract C5)",
        )
    return CoupledSet(
        frozenset(
            _basename(reference)
            for line in found.lines
            if REVIEWED_MARKER not in line
            for reference in FIXTURE_REFERENCE.findall(line)
        )
    ), ()


def _gui_pinned(
    mode: Scope, repo_root: Path, env: Mapping[str, str]
) -> tuple[CoupledSet, tuple[str, ...]]:
    """The pins file's tests/prd-gate/fixtures/ entries; it must be a JSON list of paths."""
    try:
        pins = json.loads(_pins_text(mode, repo_root, env))
        problem = None if _is_path_list(pins) else "not a JSON list of strings"
    except (OSError, ValueError) as err:
        problem = str(err)
    if problem is not None:
        return CoupledSet(frozenset(), everything=True), (
            f"verify.sh: WARNING — {GUI_PINS.path} is unreadable ({problem}) — every prd-gate "
            "fixture counts as GUI-pinned (fail wide, contract C5)",
        )
    return CoupledSet(frozenset(_basename(pin) for pin in pins if pin.startswith(FIXTURES_DIR))), ()


def _pins_text(mode: Scope, repo_root: Path, env: Mapping[str, str]) -> str:
    if mode is not Scope.STAGED:
        return GUI_PINS.read_text(repo_root)
    found = gitcall.run(repo_root, env, "cat-file", "blob", f":{GUI_PINS.path}")
    if found.returncode != 0:
        raise OSError(found.exit_summary(f"git cat-file blob :{GUI_PINS.path}"))
    return found.stdout


def _is_path_list(value: object) -> bool:
    return isinstance(value, list) and all(isinstance(item, str) for item in value)


def _basename(path: str) -> str:
    """`${path##*/}`."""
    return path.rsplit("/", 1)[-1]


@dataclass(frozen=True)
class ScopeFacts:
    """What decide_scope reads about the classified paths; None = not gathered."""

    paths: Mapping[str, PathFacts]
    occt_declared: frozenset[str] | None
    coupling: FixtureCoupling | None


def gather_facts(
    mode: Scope, changed: ChangedSet, repo_root: Path, env: Mapping[str, str]
) -> tuple[ScopeFacts, tuple[str, ...]]:
    """PathFacts for every classified path in one pass; the OCCT set only if a crate path
    seeds, and the fixture coupling only if a path is a prd-gate fixture (PRD D6)."""
    paths = {path: attribute(path) for path in changed.classified}
    occt_declared = None
    coupling = None
    warnings: list[str] = []
    if any(facts.tree is Tree.CRATES and facts.seed_crates for facts in paths.values()):
        result = bashcall.run(OCCT_DECLARED_SET, (), repo_root, env)
        occt_declared = frozenset(result.lines)
        warnings.extend(result.stderr_lines)
    if any(fnmatchcase(path, FIXTURE_GLOB) for path in paths):
        coupling, coupling_warnings = derive_fixture_coupling(mode, repo_root, env)
        warnings.extend(coupling_warnings)
    return ScopeFacts(paths, occt_declared, coupling), tuple(warnings)


@dataclass(frozen=True)
class Signals:
    rust: bool = False
    gui: bool = False
    gate: bool = False

    def __or__(self, other: "Signals") -> "Signals":
        return Signals(self.rust or other.rust, self.gui or other.gui, self.gate or other.gate)


NOTHING = Signals()
GUI = Signals(gui=True)
RUST_AND_GUI = Signals(rust=True, gui=True)
EVERYTHING = Signals(rust=True, gui=True, gate=True)


def decide_scope(resolved: ResolvedScope, changed: ChangedSet, facts: ScopeFacts) -> ScopeDecision:
    """The check families the changed paths need: an OR over one classification per path."""
    signals = reduce(Signals.__or__, (_classify(path, facts) for path in changed.classified), NOTHING)
    rust = signals.rust or GUI_EVENT_CHANNELS in changed.rename_sources
    return ScopeDecision(
        mode=resolved.mode,
        merge_base=resolved.merge_base,
        widened=None,
        run_rust=rust,
        run_gui=rust or signals.gui,
        run_occt_gate=signals.gate,
        gui_path_signal=signals.gui,
        changed_paths=changed.files,
        rename_sources=changed.rename_sources,
    )


def _classify(path: str, facts: ScopeFacts) -> Signals:
    """Verify's policy arms first, then the arms over the path's facts (S22 ruling)."""
    if path == GUI_EVENT_CHANNELS:
        return GUI
    if fnmatchcase(path, FIXTURE_GLOB):
        return _fixture_signals(path, facts)
    path_facts = facts.paths.get(path)
    if path_facts is None:
        raise ValueError(f"decide_scope: no PathFacts gathered for {path!r}")
    if path_facts.tree is Tree.CRATES:
        return Signals(rust=True, gate=_seeds_an_occt_crate(path, path_facts, facts))
    if path_facts.tree is Tree.GUI_TAURI:
        return RUST_AND_GUI
    if path_facts.is_global:
        return EVERYTHING
    if path_facts.tree in (Tree.GUI_SRC, Tree.GUI_OTHER):
        return GUI
    if path_facts.inert:
        return NOTHING
    return EVERYTHING


def _fixture_signals(path: str, facts: ScopeFacts) -> Signals:
    """A compiled test's input runs everything; a vitest pin runs the gui; any other, nothing."""
    if facts.coupling is None:
        raise ValueError(f"decide_scope: {path!r} is a prd-gate fixture but no coupling was gathered")
    name = _basename(path)
    if name in facts.coupling.rust:
        return EVERYTHING
    if name in facts.coupling.gui:
        return GUI
    return NOTHING


def _seeds_an_occt_crate(path: str, path_facts: PathFacts, facts: ScopeFacts) -> bool:
    if not path_facts.seed_crates:
        return False
    if facts.occt_declared is None:
        raise ValueError(f"decide_scope: {path!r} seeds a crate but the OCCT set was not gathered")
    return bool(path_facts.seed_crates & facts.occt_declared)


def decide(
    invocation: Invocation, repo_root: Path, env: Mapping[str, str]
) -> tuple[ScopeDecision, tuple[str, ...]]:
    resolved = resolve_scope(invocation, repo_root, env)
    if resolved.mode is Scope.ALL:
        return ScopeDecision.wide(resolved, resolved.widened), resolved.notices
    changed = gather_changed(resolved, repo_root, env)
    if isinstance(changed, GatherFailure):
        return ScopeDecision.wide(resolved, Widening.CHANGED_PATHS_UNREAD), (
            *resolved.notices,
            *changed.warnings,
        )
    facts, warnings = gather_facts(resolved.mode, changed, repo_root, env)
    return decide_scope(resolved, changed, facts), (*resolved.notices, *warnings)
