"""scope — which check families a verify run needs, decided from what changed (PRD D5).

resolve_scope settles the effective scope. decide returns the ScopeDecision
and the scope stage's stderr lines, in the order bash prints them.

Every git call goes through _git, under the caller's env unmodified: the
hook's GIT_* env is never scrubbed (consumer C18).
"""

import subprocess
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path

from reify_verify.invocation import Invocation, Scope
from reify_verify.policy import ROLE_TABLE

MERGE_HEAD_NOTICE = "verify.sh: MERGE_HEAD present — forcing --scope all (merge in progress)"
MERGE_BASE_WARNING = (
    "verify.sh: WARNING — --scope branch could not resolve 'git merge-base main HEAD' "
    "(detached HEAD / missing local main ref / merge-base failure) — failing WIDE to "
    "--scope all (contract C5)"
)


@dataclass(frozen=True)
class GitOutput:
    returncode: int
    lines: tuple[str, ...]


def _git(repo_root: Path, env: Mapping[str, str], *args: str) -> GitOutput:
    """`git -C repo_root args`: stdout lines verbatim (core.quotePath quoting included), stderr dropped."""
    proc = subprocess.run(
        ["git", "-C", str(repo_root), *args],
        env=dict(env),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    text = proc.stdout.decode("utf-8", "surrogateescape")
    return GitOutput(proc.returncode, tuple(text.removesuffix("\n").split("\n")) if text else ())


@dataclass(frozen=True)
class ResolvedScope:
    """The scope a run actually uses, and the notices that changed it."""

    mode: Scope
    merge_base: str | None
    notices: tuple[str, ...]


def resolve_scope(invocation: Invocation, repo_root: Path, env: Mapping[str, str]) -> ResolvedScope:
    """A merge in progress, then a force_scope_all role, force scope all; branch needs a merge base."""
    mode = invocation.scope
    notices = []
    if mode is not Scope.ALL and _merge_in_progress(repo_root, env):
        notices.append(MERGE_HEAD_NOTICE)
        mode = Scope.ALL
    if mode is not Scope.ALL and ROLE_TABLE[invocation.role].force_scope_all:
        notices.append(
            f"verify.sh: DF_VERIFY_ROLE={invocation.role.value} — forcing --scope all "
            "(integrity gate never narrows, contract C2)"
        )
        mode = Scope.ALL
    merge_base = None
    if mode is Scope.BRANCH:
        found = _git(repo_root, env, "merge-base", "main", "HEAD")
        if found.returncode == 0 and found.lines and found.lines[0]:
            merge_base = found.lines[0]
        else:
            notices.append(MERGE_BASE_WARNING)
            mode = Scope.ALL
    return ResolvedScope(mode, merge_base, tuple(notices))


def _merge_in_progress(repo_root: Path, env: Mapping[str, str]) -> bool:
    """A MERGE_HEAD file exists; a relative git path is resolved against repo_root, never the cwd."""
    found = _git(repo_root, env, "rev-parse", "--git-path", "MERGE_HEAD")
    if found.returncode != 0 or not found.lines or not found.lines[0]:
        return False
    return (repo_root / found.lines[0]).is_file()


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
