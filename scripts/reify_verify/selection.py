"""selection — the per-artifact infra tests a task-tier run selects from what changed (PRD D5, D14).

select_infra returns the Selection, in bash's selector order, and the
selection stage's stderr lines.
"""

from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path

from reify_verify.scope import ScopeDecision


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
    return Selection(()), ()
