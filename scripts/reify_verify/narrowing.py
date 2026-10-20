"""narrowing — the affected-crate closure, whether a run narrows to it, and the vitest lane (PRD D5).

narrow returns the Narrowing record and the narrowing stage's stderr lines.
"""

from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path

from reify_verify.scope import ScopeDecision


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
    """`text` split as bash's default IFS splits it: on space, tab and newline only."""
    return tuple(word for word in text.replace("\t", " ").replace("\n", " ").split(" ") if word)


def narrow(
    decision: ScopeDecision, narrow_flag: bool, repo_root: Path, env: Mapping[str, str]
) -> tuple[Narrowing, tuple[str, ...]]:
    closure = Closure("", False)
    return (
        Narrowing(
            narrow_active=False,
            affected_text="",
            closure=closure,
            packages=(),
            run_gui_vitest=decision.run_gui,
        ),
        (),
    )
