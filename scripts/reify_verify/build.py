"""build — one gate invocation planned: the stages, in scripts/verify.sh's order (PRD D1, D2).

plan_invocation runs every planning stage and returns either the Plan and the
diagnostics bash prints on its way there, in bash's order, or a refusal: the
lines bash would print instead and the exit code. It prints nothing itself.

γ1 plans every action whose plan has no test region. A test or all plan that
runs the Rust families has one (bash's _plan_has_test_passes); it is refused
until γ2 ports the region, because a plan printed without it would lie.

The steps come from EMITTERS, in bash's phase order. Each emitter declares
the gate paths its steps may name, and build_steps refuses a step whose
gate_path its emitter did not declare, so the declarations stay the complete
list a later --print-gates reads (PRD D12).
"""

import os
import shutil
from collections.abc import Callable, Mapping
from dataclasses import dataclass
from pathlib import Path

from reify_verify import childproc, environment, knobs, narrowing, scope, selection
from reify_verify.invocation import Action, Invocation
from reify_verify.narrowing import Narrowing
from reify_verify.plan import (
    Cargo,
    Chain,
    Guarded,
    NodeLaneJoin,
    NodeLaneStart,
    NodeProject,
    Packages,
    Phase,
    Plan,
    PlanHeader,
    Script,
    Selector,
    Step,
    StepKind,
    Workspace,
)
from reify_verify.policy import ROLE_TABLE, InfraTier, RolePolicy, resolve_prio
from reify_verify.scope import ScopeDecision
from reify_verify.selection import Selection

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
    prio = resolve_prio(
        role_policy,
        invocation.role.value,
        shutil.which("nice", path=env.get("PATH")) is not None,
        shutil.which("ionice", path=env.get("PATH")) is not None,
    )
    resolved = scope.resolve_scope(invocation, repo_root, env)
    step_env = environment.build(invocation.role, role_policy, env, repo_root)
    planning = step_env.planning
    decision, gather_warnings = scope.decide_resolved(resolved, repo_root, planning)
    if _has_test_region(invocation.action, decision):
        return _test_region_refusal(invocation.action)
    selected, selection_warnings = selection.select_infra(decision, repo_root, planning)
    narrowed, narrowing_warnings = narrowing.narrow(
        decision, invocation.narrow, repo_root, planning
    )
    profile_selection = invocation.profile_arg or role_policy.default_profile
    header = PlanHeader(
        action=invocation.action,
        profile_selection=profile_selection,
        profiles=profile_selection.profiles,
        scope=decision.mode,
        include_infra=invocation.include_infra,
        nextest=_nextest_available(planning, repo_root),
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
        environment=step_env.lines,
        steps=build_steps(
            PlanInputs(
                families=Families.of(invocation.action),
                include_infra=invocation.include_infra,
                decision=decision,
                narrowing=narrowed,
                selection=selected,
                role_policy=role_policy,
                cargo_prio=prio.prefix,
                walls=Walls.resolve(env),
                node_lane=NodeLane.compose(Families.of(invocation.action), decision),
            )
        ),
    )
    diagnostics = (
        *filter(None, (prio.warning,)),
        *resolved.notices,
        *step_env.warnings,
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


@dataclass(frozen=True)
class Families:
    """The check families an action runs: bash's DO_TEST, DO_LINT and DO_TYPECHECK."""

    test: bool
    lint: bool
    typecheck: bool

    @classmethod
    def of(cls, action: Action) -> "Families":
        return cls(
            test=action in (Action.TEST, Action.ALL),
            lint=action in (Action.LINT, Action.ALL),
            typecheck=action in (Action.TYPECHECK, Action.ALL),
        )


@dataclass(frozen=True)
class Walls:
    check: str
    clippy: str
    prebuild: str

    @classmethod
    def resolve(cls, env: Mapping[str, str]) -> "Walls":
        return cls(
            check=knobs.resolve(knobs.CHECK_TIMEOUT, env).value,
            clippy=knobs.resolve(knobs.CLIPPY_TIMEOUT, env).value,
            prebuild=knobs.resolve(knobs.PREBUILD_TIMEOUT, env).value,
        )


@dataclass(frozen=True)
class NodeLane:
    """The gui, sidecar and tree-sitter node projects, composed once per plan."""

    gui: NodeProject
    sidecar: NodeProject
    tree_sitter: NodeProject

    @property
    def projects(self) -> tuple[NodeProject, ...]:
        return (self.gui, self.sidecar, self.tree_sitter)

    @classmethod
    def compose(cls, families: Families, decision: ScopeDecision) -> "NodeLane | None":
        if not (decision.run_gui and (families.test or families.lint)):
            return None
        return cls(
            gui=NodeProject("-d", "gui", "gui", 15, ("npm ci", "npm run typecheck")),
            sidecar=NodeProject(
                "-f", "gui/sidecar/package-lock.json", "gui/sidecar", 10,
                ("npm ci", "npm run typecheck", "npm run typecheck:test"),
            ),
            tree_sitter=NodeProject(
                "-f", "tree-sitter-reify/package-lock.json", "tree-sitter-reify", 10, ("npm ci",),
            ),
        )


@dataclass(frozen=True)
class PlanInputs:
    families: Families
    include_infra: bool
    decision: ScopeDecision
    narrowing: Narrowing
    selection: Selection
    role_policy: RolePolicy
    cargo_prio: str
    walls: Walls
    node_lane: NodeLane | None

    @property
    def selector(self) -> Selector:
        packages = self.narrowing.packages
        return Packages(packages) if packages else Workspace()

    @property
    def lints_rust(self) -> bool:
        return self.families.lint and self.decision.run_rust


@dataclass(frozen=True)
class Emitter:
    name: str
    gate_paths: tuple[str, ...]
    emit: Callable[[PlanInputs], tuple[Step, ...]]


def _tool(phase: Phase, command, gate_path=None, guard_file=None) -> Step:
    return Step(StepKind.TOOL, phase, command, gate_path, guard_file)


STATIC_GATES = (
    Script("scripts/check-infra-classification-manifest.sh"),
    Script("scripts/check-harness-baseline-registration.sh", ("--from-git",)),
    Script("scripts/check-manifold-deps.sh"),
    Script("scripts/tree-sitter-generate.sh"),
    Script("scripts/tree-sitter-freshness.sh", ("ensure",)),
)
COMPILE_GATE = Script("scripts/verify.sh", ("compile-gate",))
FRESHNESS_CHECK = Script("scripts/tree-sitter-freshness.sh", ("check",))
GUI_SIDECAR_PLACEHOLDER = "scripts/ensure-gui-sidecar-placeholder.sh"
GUI_CRATE_MANIFEST = "gui/src-tauri/Cargo.toml"
LINT_INFRA_CHECKS = (
    ("scripts/test_pm_standardization.sh", "10m"),
    ("scripts/check_event_inventory.sh", "5m"),
    ("scripts/check-nan-safe-ordering.sh", "5m"),
    ("scripts/check-compute-trampoline-registration.sh", "5m"),
)


def _static_gates(inputs: PlanInputs) -> tuple[Step, ...]:
    if not inputs.decision.run_rust:
        return ()
    return tuple(_tool(Phase.PREFLIGHT, gate, gate.path) for gate in STATIC_GATES)


def _compile_gate(inputs: PlanInputs) -> tuple[Step, ...]:
    if not (inputs.decision.run_rust and (inputs.families.lint or inputs.families.typecheck)):
        return ()
    return (_tool(Phase.COMPILE_GATE, COMPILE_GATE, COMPILE_GATE.path),)


def _cargo_check(inputs: PlanInputs) -> tuple[Step, ...]:
    families = inputs.families
    if not (families.typecheck and not families.lint and inputs.decision.run_rust):
        return ()
    check = Cargo("check", inputs.selector, ("--tests",), inputs.walls.check, inputs.cargo_prio)
    return (Step(StepKind.CARGO, Phase.CARGO_CHECK, check, None, None),)


def _node_lane_start(inputs: PlanInputs) -> tuple[Step, ...]:
    if not (inputs.lints_rust and inputs.node_lane):
        return ()
    start = NodeLaneStart(inputs.node_lane.projects)
    return (Step(StepKind.MAIN_SHELL, Phase.GUI_NODE_LAUNCH, start, None, None),)


def _clippy(inputs: PlanInputs) -> tuple[Step, ...]:
    if not inputs.lints_rust:
        return ()
    clippy = Cargo(
        "clippy", inputs.selector, ("--all-targets", "--", "-D", "warnings"),
        inputs.walls.clippy, inputs.cargo_prio,
    )
    return (Step(StepKind.CARGO, Phase.CLIPPY, clippy, None, None),)


def _gui_feature_check(inputs: PlanInputs) -> tuple[Step, ...]:
    if not inputs.lints_rust:
        return ()
    check = Cargo(
        "check", Packages(("reify-gui",)), ("--features", "gui", "--tests"),
        inputs.walls.clippy, inputs.cargo_prio,
    )
    command = Guarded("-f", GUI_CRATE_MANIFEST, Chain((Script(GUI_SIDECAR_PLACEHOLDER), check)))
    return (
        Step(
            StepKind.CARGO, Phase.GUI_FEATURE_CHECK, command, GUI_SIDECAR_PLACEHOLDER,
            GUI_CRATE_MANIFEST,
        ),
    )


def _node_lane_join(inputs: PlanInputs) -> tuple[Step, ...]:
    if not (inputs.lints_rust and inputs.node_lane):
        return ()
    return (Step(StepKind.MAIN_SHELL, Phase.GUI_NODE_JOIN, NodeLaneJoin(), None, None),)


def _node_lane_plain(inputs: PlanInputs) -> tuple[Step, ...]:
    """Without a clippy pass to overlap, the node projects run in the foreground, one step each."""
    if inputs.node_lane is None or inputs.lints_rust:
        return ()
    return tuple(_tool(Phase.GUI_NODE, project) for project in inputs.node_lane.projects)


def _lint_infra_checks(inputs: PlanInputs) -> tuple[Step, ...]:
    if not (inputs.include_infra and inputs.lints_rust):
        return ()
    return tuple(
        _tool(
            Phase.INFRA_STATIC,
            Guarded(
                "-f", path, Script(path, wall=wall, via_bash=True),
                f"WARNING: {Path(path).name} not found, skipping",
            ),
            path,
            path,
        )
        for path, wall in LINT_INFRA_CHECKS
    )


def _test_region(inputs: PlanInputs) -> tuple[Step, ...]:
    """γ2's slot (#9099): the nextest passes and their semaphore region. γ1 refuses a plan that has them."""
    return ()


def _freshness_check(inputs: PlanInputs) -> tuple[Step, ...]:
    families = inputs.families
    if not (inputs.decision.run_rust and (families.lint or families.typecheck or families.test)):
        return ()
    return (_tool(Phase.POSTFLIGHT, FRESHNESS_CHECK, FRESHNESS_CHECK.path),)


EMITTERS = (
    Emitter("static gates", tuple(gate.path for gate in STATIC_GATES), _static_gates),
    Emitter("compile gate", (COMPILE_GATE.path,), _compile_gate),
    Emitter("cargo check", (), _cargo_check),
    Emitter("node lane start", (), _node_lane_start),
    Emitter("clippy", (), _clippy),
    Emitter("gui feature check", (GUI_SIDECAR_PLACEHOLDER,), _gui_feature_check),
    Emitter("node lane join", (), _node_lane_join),
    Emitter("node lane, plain", (), _node_lane_plain),
    Emitter("lint infra checks", tuple(path for path, _ in LINT_INFRA_CHECKS), _lint_infra_checks),
    Emitter("test region", (), _test_region),
    Emitter("freshness check", (FRESHNESS_CHECK.path,), _freshness_check),
)


def build_steps(inputs: PlanInputs, emitters=EMITTERS) -> tuple[Step, ...]:
    steps: list[Step] = []
    for emitter in emitters:
        for step in emitter.emit(inputs):
            if step.gate_path is not None and step.gate_path not in emitter.gate_paths:
                raise ValueError(
                    f"emitter {emitter.name!r} produced a step on gate path {step.gate_path!r}, "
                    f"which it does not declare (declared: {emitter.gate_paths})"
                )
            steps.append(step)
    return tuple(steps)
