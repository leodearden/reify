"""narrowing — the affected-crate closure, whether a run narrows to it, and the vitest lane (PRD D5).

narrow returns the Narrowing record and the narrowing stage's stderr lines.
The closure comes from affected-crates-lib.sh's affected_crates, called once
over every changed path and never reimplemented here (PRD D15). Its text, or
an operator override's, is parsed once into a Closure variant; narrowing and
closure_reaches_reify_gui match on the variant.
"""

import re
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path

from reify_verify import bashcall, knobs
from reify_verify.bashcall import BashCall
from reify_verify.invocation import Scope
from reify_verify.scope import ScopeDecision

AFFECTED_CRATES = BashCall("scripts/affected-crates-lib.sh", "affected_crates")
ALL_CRATES = "ALL"
GUI_CRATE = "reify-gui"
_CRATE_NAME = re.compile(r"[A-Za-z0-9_.-]+")


@dataclass(frozen=True)
class NotComputed:
    """No closure answers this run: none was read, or an override held no word."""

    @property
    def words(self) -> tuple[str, ...]:
        return ()

    @property
    def from_diff(self) -> bool:
        return False


@dataclass(frozen=True)
class Crates:
    """A crate list in affected_crates' order; empty only when this run's diff derived it."""

    names: tuple[str, ...]
    from_diff: bool

    def __post_init__(self):
        if not self.names and not self.from_diff:
            raise ValueError("Crates: an empty list is a closure only when derived from the diff")

    @property
    def words(self) -> tuple[str, ...]:
        return self.names


@dataclass(frozen=True)
class AllCrates:
    """affected_crates' ALL sentinel: every crate is affected."""

    from_diff: bool

    @property
    def words(self) -> tuple[str, ...]:
        return (ALL_CRATES,)


@dataclass(frozen=True)
class Malformed:
    """Words that are no crate list: a token outside cargo's package-name grammar, or a padded ALL."""

    words: tuple[str, ...]
    from_diff: bool


Closure = NotComputed | Crates | AllCrates | Malformed


def parse_closure(text: str, from_diff: bool) -> Closure:
    """The one reading of a closure's text, split as bash's default IFS splits it."""
    names = _ifs_split(text)
    if not names and not from_diff:
        return NotComputed()
    if text == ALL_CRATES:
        return AllCrates(from_diff)
    if names == (ALL_CRATES,) or not all(_CRATE_NAME.fullmatch(name) for name in names):
        return Malformed(names, from_diff)
    return Crates(names, from_diff)


def _ifs_split(text: str) -> tuple[str, ...]:
    """Split on space, tab and newline only, never globbed."""
    return tuple(word for word in text.replace("\t", " ").replace("\n", " ").split(" ") if word)


@dataclass(frozen=True)
class Narrowing:
    closure: Closure
    narrow_eligible: bool
    run_gui_vitest: bool

    @property
    def affected(self) -> tuple[str, ...]:
        """bash's AFFECTED: the closure's words when this run may narrow."""
        return self.closure.words if self.narrow_eligible else ()

    @property
    def narrow_active(self) -> bool:
        return self.narrow_eligible and _narrows(self.closure)

    @property
    def packages(self) -> tuple[str, ...]:
        return self.affected if self.narrow_active else ()

    def to_json(self) -> dict:
        return {
            "narrow_active": int(self.narrow_active),
            "affected": list(self.affected),
            "closure": list(self.closure.words),
            "from_diff": int(self.closure.from_diff),
            "packages": list(self.packages),
            "run_gui_vitest": int(self.run_gui_vitest),
        }


def _narrows(closure: Closure) -> bool:
    """As bash's: a closure holding a word narrows unless it is the exact ALL sentinel, so a
    malformed override narrows too (closure_reaches_reify_gui still fails it wide)."""
    match closure:
        case Crates(names=names):
            return bool(names)
        case Malformed():
            return True
        case NotComputed() | AllCrates():
            return False


def narrow(
    decision: ScopeDecision, narrow_flag: bool, repo_root: Path, env: Mapping[str, str]
) -> tuple[Narrowing, tuple[str, ...]]:
    closure, warnings = _closure(decision, repo_root, env)
    eligible = decision.run_rust and (
        decision.mode is Scope.BRANCH or (decision.mode is Scope.STAGED and narrow_flag)
    )
    retry_specs = knobs.resolve(knobs.GUI_RETRY_SPECS, env).value
    run_gui_vitest = decision.run_gui and (
        decision.gui_path_signal
        or bool(retry_specs)
        or closure_reaches_reify_gui(decision.mode, closure)
    )
    if retry_specs and not run_gui_vitest:
        warnings = (
            *warnings,
            f"verify.sh: WARNING — {knobs.GUI_RETRY_SPECS.name} is set but this run emits no "
            f"vitest lane (RUN_GUI={int(decision.run_gui)} RUN_GUI_VITEST=0); the requested "
            "specs will NOT run",
        )
    return Narrowing(closure, eligible, run_gui_vitest), warnings


def _closure(
    decision: ScopeDecision, repo_root: Path, env: Mapping[str, str]
) -> tuple[Closure, tuple[str, ...]]:
    """An operator override verbatim, else ONE affected_crates call over the changed paths."""
    if not (decision.run_rust and decision.mode in (Scope.STAGED, Scope.BRANCH)):
        return NotComputed(), ()
    override = knobs.resolve(knobs.AFFECTED_CRATES_OVERRIDE, env).value
    if override:
        return parse_closure(override, from_diff=False), ()
    if not decision.changed_paths:
        return NotComputed(), ()
    result = bashcall.run(AFFECTED_CRATES, decision.changed_paths, repo_root, env)
    return parse_closure(result.stdout.rstrip("\n"), from_diff=True), result.stderr_lines


def closure_reaches_reify_gui(mode: Scope, closure: Closure) -> bool:
    """bash's four named arms: MERGE-GATE, FAIL-WIDE, COMPUTED-EMPTY, MEMBERSHIP."""
    if mode is Scope.ALL:
        return True
    match closure:
        case NotComputed() | AllCrates() | Malformed():
            return True
        case Crates(names=()):
            return False
        case Crates(names=names):
            return GUI_CRATE in names
