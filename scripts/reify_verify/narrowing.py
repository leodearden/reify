"""narrowing — the affected-crate closure, whether a run narrows to it, and the vitest lane (PRD D5).

narrow returns the Narrowing record and the narrowing stage's stderr lines.
The closure comes from affected-crates-lib.sh's affected_crates, called once
over every changed path and never reimplemented here (PRD D15).
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
class Closure:
    """The affected-crate closure as bash holds it: text, and whether this run's diff derived it."""

    text: str
    from_diff: bool


@dataclass(frozen=True)
class Narrowing:
    narrow_active: bool
    affected_text: str
    closure: Closure
    packages: tuple[str, ...]
    run_gui_vitest: bool

    def to_json(self) -> dict:
        return {
            "narrow_active": int(self.narrow_active),
            "affected": list(words(self.affected_text)),
            "closure": list(words(self.closure.text)),
            "from_diff": int(self.closure.from_diff),
            "packages": list(self.packages),
            "run_gui_vitest": int(self.run_gui_vitest),
        }


def words(text: str) -> tuple[str, ...]:
    """`text` split as bash's default IFS splits it: on space, tab and newline only, never globbed."""
    return tuple(word for word in text.replace("\t", " ").replace("\n", " ").split(" ") if word)


def narrow(
    decision: ScopeDecision, narrow_flag: bool, repo_root: Path, env: Mapping[str, str]
) -> tuple[Narrowing, tuple[str, ...]]:
    closure, warnings = _closure(decision, repo_root, env)
    eligible = decision.run_rust and (
        decision.mode is Scope.BRANCH or (decision.mode is Scope.STAGED and narrow_flag)
    )
    affected_text = closure.text if eligible else ""
    affected = words(affected_text)
    narrow_active = bool(affected) and affected_text != ALL_CRATES
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
    return (
        Narrowing(
            narrow_active=narrow_active,
            affected_text=affected_text,
            closure=closure,
            packages=affected if narrow_active else (),
            run_gui_vitest=run_gui_vitest,
        ),
        warnings,
    )


def _closure(
    decision: ScopeDecision, repo_root: Path, env: Mapping[str, str]
) -> tuple[Closure, tuple[str, ...]]:
    """An operator override verbatim, else ONE affected_crates call over the changed paths."""
    if not (decision.run_rust and decision.mode in (Scope.STAGED, Scope.BRANCH)):
        return Closure("", False), ()
    override = knobs.resolve(knobs.AFFECTED_CRATES_OVERRIDE, env).value
    if override:
        return Closure(override, False), ()
    if not decision.changed_paths:
        return Closure("", False), ()
    result = bashcall.run(AFFECTED_CRATES, decision.changed_paths, repo_root, env)
    return Closure(result.stdout.rstrip("\n"), True), result.stderr_lines


def closure_reaches_reify_gui(mode: Scope, closure: Closure) -> bool:
    """bash's four named arms: MERGE-GATE, FAIL-WIDE, COMPUTED-EMPTY, MEMBERSHIP."""
    if mode is Scope.ALL:
        return True
    crates = words(closure.text)
    if any(not _CRATE_NAME.fullmatch(crate) for crate in crates) or crates == (ALL_CRATES,):
        return True
    if not crates:
        return not closure.from_diff
    return GUI_CRATE in crates
