"""build — one gate invocation planned: the stages, in scripts/verify.sh's order (PRD D1, D2).

plan_invocation runs every planning stage and returns either the Plan and the
diagnostics bash prints on its way there, in bash's order, or a refusal: the
lines bash would print instead and the exit code. It prints nothing itself.

γ1 plans every action whose plan has no test region. A test or all plan that
runs the Rust families has one (bash's _plan_has_test_passes); it is refused
until γ2 ports the region, because a plan printed without it would lie.
"""

import os
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path

from reify_verify import childproc, narrowing, scope, selection
from reify_verify.invocation import Action, Invocation
from reify_verify.plan import Plan, PlanHeader
from reify_verify.policy import ROLE_TABLE, InfraTier
from reify_verify.scope import ScopeDecision

ERROR = "verify.sh: ERROR — "
CUTOVER = "the S18 cutover (docs/prds/v0_6/verify-structured-plan-and-role-table.md)"
TEST_ACTIONS = (Action.TEST, Action.ALL)


@dataclass(frozen=True)
class Planned:
    plan: Plan
    diagnostics: tuple[str, ...]


@dataclass(frozen=True)
class Refused:
    lines: tuple[str, ...]
    exit_code: int


def plan_invocation(
    invocation: Invocation, env: Mapping[str, str], repo_root: Path
) -> Planned | Refused:
    role_policy = ROLE_TABLE[invocation.role]
    resolved = scope.resolve_scope(invocation, repo_root, env)
    decision, gather_warnings = scope.decide_resolved(resolved, repo_root, env)
    if _has_test_region(invocation.action, decision):
        return _test_region_refusal(invocation.action)
    _selected, selection_warnings = selection.select_infra(decision, repo_root, env)
    narrowed, narrowing_warnings = narrowing.narrow(decision, invocation.narrow, repo_root, env)
    profile_selection = invocation.profile_arg or role_policy.default_profile
    header = PlanHeader(
        action=invocation.action,
        profile_selection=profile_selection,
        profiles=profile_selection.profiles,
        scope=decision.mode,
        include_infra=invocation.include_infra,
        nextest=_nextest_available(env, repo_root),
        role=invocation.role,
    )
    plan = Plan(
        retry_marker=None,
        header=header,
        selective_infra_note=invocation.include_infra
        and role_policy.infra_tier is InfraTier.SELECTIVE,
        scope=decision,
        narrowing=narrowed,
        heavy_partition=None,
    )
    diagnostics = (
        *resolved.notices,
        *gather_warnings,
        *selection_warnings,
        *narrowing_warnings,
    )
    return Planned(plan, diagnostics)


def _has_test_region(action: Action, decision: ScopeDecision) -> bool:
    return action in TEST_ACTIONS and decision.run_rust


def _test_region_refusal(action: Action) -> Refused:
    return Refused(
        (
            f"{ERROR}scripts/verify.py does not plan the test region of action "
            f"'{action.value}' yet (RUN_RUST=1); scripts/verify.sh remains the gate until "
            f"{CUTOVER}",
        ),
        os.EX_UNAVAILABLE,
    )


def _nextest_available(env: Mapping[str, str], repo_root: Path) -> bool:
    return childproc.run(["cargo", "nextest", "--version"], repo_root, env).returncode == 0
