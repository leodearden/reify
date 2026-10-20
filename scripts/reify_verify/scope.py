"""scope — which check families a verify run needs, decided from what changed (PRD D5).

resolve_scope settles the effective scope. decide returns the ScopeDecision
and the scope stage's stderr lines, in the order bash prints them.
"""

from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path

from reify_verify.invocation import Invocation, Scope


@dataclass(frozen=True)
class ResolvedScope:
    """The scope a run actually uses, and the notices that changed it."""

    mode: Scope
    merge_base: str | None
    notices: tuple[str, ...]


def resolve_scope(invocation: Invocation, repo_root: Path, env: Mapping[str, str]) -> ResolvedScope:
    return ResolvedScope(invocation.scope, None, ())


@dataclass(frozen=True)
class ScopeDecision:
    mode: Scope
    merge_base: str | None
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

    @classmethod
    def wide(cls, resolved: ResolvedScope) -> "ScopeDecision":
        """Every check family on, no paths: scope all, or a fail-wide."""
        return cls(
            mode=resolved.mode,
            merge_base=resolved.merge_base,
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
            "merge_base": self.merge_base,
            "run_rust": int(self.run_rust),
            "run_gui": int(self.run_gui),
            "run_occt_gate": int(self.run_occt_gate),
            "gui_path_signal": int(self.gui_path_signal),
            "changed_paths": list(self.changed_paths),
            "rename_sources": list(self.rename_sources),
        }


def decide(
    invocation: Invocation, repo_root: Path, env: Mapping[str, str]
) -> tuple[ScopeDecision, tuple[str, ...]]:
    resolved = resolve_scope(invocation, repo_root, env)
    return ScopeDecision.wide(resolved), resolved.notices
