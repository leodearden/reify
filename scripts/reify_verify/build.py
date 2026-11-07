"""build — one gate invocation planned: the stages, in scripts/verify.sh's order (PRD D1, D2).

plan_invocation runs the planning stages and returns either the Plan and the
diagnostics bash prints on its way there, in bash's order, or a refusal: the
lines bash would print instead and the exit code. It prints nothing itself.
Each stage returns its value with the diagnostics it adds, or a Refused, and
plan_invocation joins the diagnostics in stage order:

- start: bash's load-time checks, run after argv is parsed (a D8 listed
  divergence): the libs verify.sh sources must exist and the heavy filterset
  must be non-empty; then each timeout knob and the priority, with warnings;
- decide: the scope and the step environment, then the selection, the
  narrowing, the release-sensitive crate list and the release-delta skip,
  which bash reads and decides at load for every action;
- probe: nextest availability, under the planning env, as bash's probe runs;
- compose: the plan's inputs (the test region among them, when the plan runs
  the test family over the Rust crates), header and steps. A nextest config
  execute mode generated for the region is removed again if composing fails
  after it.

The steps come from EMITTERS, in bash's phase order. Each emitter declares
the gate paths its steps may name, and build_steps refuses a step whose
gate_path its emitter did not declare, so the declarations stay the complete
list a later --print-gates reads (PRD D12).
"""

import os
import re
import shutil
from collections.abc import Callable, Mapping
from dataclasses import dataclass
from pathlib import Path

from reify_verify import (
    bashcall,
    environment,
    knobs,
    narrowing,
    retry,
    scope,
    selection,
    testregion,
)
from reify_verify.bashcall import BashCall, BashExport
from reify_verify.environment import (
    NextestAvailable,
    NextestBroken,
    ProbeSleepFailed,
    StepEnvironment,
)
from reify_verify.gatepaths import COMPILE_GATE, GUI_CRATE_GUARD, RUN_ALL
from reify_verify.invocation import Action, Invocation
from reify_verify.narrowing import Narrowing
from reify_verify.plan import (
    ArtifactAssertion,
    Cargo,
    Guarded,
    HeadStamp,
    InfraLeafLoop,
    NodeLaneJoin,
    NodeLaneStart,
    NodeProject,
    Packages,
    Phase,
    Plan,
    PlanHeader,
    Refused,
    Script,
    Selector,
    Step,
    StepKind,
    Workspace,
)
from reify_verify.policy import (
    ROLE_TABLE,
    InfraTier,
    PrioResolution,
    Profile,
    RolePolicy,
    resolve_prio,
    test_wall,
)
from reify_verify.retry import NO_NEXTEST_SUBSETS
from reify_verify.scope import ScopeDecision
from reify_verify.selection import Selection
from reify_verify.testregion import PassWalls, RegionInputs, TestRegion

ERROR = "verify.sh: ERROR — "
GIT_ENV_SCRUB_PREFIX = BashCall("scripts/lib_git_env_scrub.sh", "reify_git_env_scrub_prefix")
HEAVY_FILTER = BashExport("scripts/heavy-test-filter-lib.sh", "REIFY_HEAVY_NEXTEST_FILTER")
# The libs verify.sh sources at load, in its order; the first missing one refuses the run.
LOAD_TIME_LIBS = tuple(
    f"scripts/{name}"
    for name in (
        "occt-scope-lib.sh",
        "release-scope-lib.sh",
        "affected-crates-lib.sh",
        "lib_test_semaphore.sh",
        "cpu-admit.sh",
        "lib_proc_reaper.sh",
        "heavy-test-filter-lib.sh",
        "lib_git_env_scrub.sh",
    )
)
EMPTY_HEAVY_FILTER = (
    f"{ERROR}{HEAVY_FILTER.name} empty after sourcing {Path(HEAVY_FILTER.lib).name}"
)


@dataclass(frozen=True)
class Planned:
    plan: Plan
    diagnostics: tuple[str, ...]


def plan_invocation(
    invocation: Invocation, env: Mapping[str, str], repo_root: Path
) -> Planned | Refused:
    action = invocation.action
    if action is None:
        raise ValueError("plan_invocation: an invocation without an action has no plan")
    started = _start(invocation, env, repo_root)
    if isinstance(started, Refused):
        return started
    decided = _decide(invocation, action, started.role_policy, env, repo_root)
    if isinstance(decided, Refused):
        return Refused((*started.diagnostics, *decided.lines), decided.exit_code)
    diagnostics = (*started.diagnostics, *decided.diagnostics)
    nextest = _probe(decided.step_env.planning, repo_root)
    if isinstance(nextest, Refused):
        return Refused((*diagnostics, *nextest.lines), nextest.exit_code)
    composed = _compose(invocation, action, started, decided, nextest, env, repo_root)
    if isinstance(composed, Refused):
        return Refused((*diagnostics, *composed.lines), composed.exit_code)
    return Planned(composed.plan, (*diagnostics, *composed.diagnostics))


@dataclass(frozen=True)
class Started:
    """verify.sh loaded: its heavy filterset, and the role's walls and priority."""

    role_policy: RolePolicy
    heavy_filter: str
    walls: "Walls"
    prio: PrioResolution
    diagnostics: tuple[str, ...]


@dataclass(frozen=True)
class Decided:
    """What the run covers, decided under the step environment."""

    families: "Families"
    decision: ScopeDecision
    step_env: StepEnvironment
    selection: Selection
    narrowing: Narrowing
    release_declared: tuple[str, ...]
    release_delta_skips: bool
    diagnostics: tuple[str, ...]


def _start(invocation: Invocation, env: Mapping[str, str], repo_root: Path) -> Started | Refused:
    """bash's load-time checks, then the timeout knobs and the priority, under the raw env."""
    for lib in LOAD_TIME_LIBS:
        if not os.path.isfile(repo_root / lib):
            return Refused((f"{ERROR}{lib} not found next to verify.sh",), 1)
    heavy_filter = bashcall.read_export(HEAVY_FILTER, repo_root, env)
    if not heavy_filter:
        return Refused((EMPTY_HEAVY_FILTER,), 1)
    role_policy = ROLE_TABLE[invocation.role]
    walls, wall_warnings = Walls.resolve(role_policy, env)
    prio = resolve_prio(
        role_policy,
        invocation.role.value,
        shutil.which("nice", path=env.get("PATH")) is not None,
        shutil.which("ionice", path=env.get("PATH")) is not None,
    )
    return Started(
        role_policy, heavy_filter, walls, prio, (*wall_warnings, *filter(None, (prio.warning,)))
    )


def _decide(
    invocation: Invocation,
    action: Action,
    role_policy: RolePolicy,
    env: Mapping[str, str],
    repo_root: Path,
) -> Decided | Refused:
    """The scope (MERGE_HEAD and the merge base under the raw env), then everything else under
    the planning env. A refusal keeps the diagnostics printed before it."""
    resolved = scope.resolve_scope(invocation, repo_root, env)
    step_env = environment.build(invocation.role, role_policy, env, repo_root)
    planning = step_env.planning
    decision, gather_warnings = scope.decide_resolved(resolved, repo_root, planning)
    selected, selection_warnings = selection.select_infra(decision, repo_root, planning)
    narrowed, narrowing_warnings = narrowing.narrow(
        decision, invocation.narrow, repo_root, planning
    )
    diagnostics = (
        *resolved.notices,
        *step_env.warnings,
        *gather_warnings,
        *selection_warnings,
        *narrowing_warnings,
    )
    declared = testregion.release_declared(repo_root, planning)
    if isinstance(declared, Refused):
        return Refused((*diagnostics, *declared.lines), declared.exit_code)
    skips, delta_diagnostics = testregion.decide_release_delta(role_policy, planning, repo_root)
    return Decided(
        Families.of(action), decision, step_env, selected, narrowed, declared, skips,
        (*diagnostics, *delta_diagnostics),
    )


def _probe(planning: Mapping[str, str], repo_root: Path) -> bool | Refused:
    """Whether the plan runs nextest; a cargo-nextest that is installed yet never runs refuses."""
    match environment.nextest_probe(planning, repo_root):
        case NextestBroken() as broken:
            return Refused((broken.error,), 1)
        case ProbeSleepFailed(result=result) as failed:
            return Refused(result.stderr_lines, failed.exit_code)
        case probe:
            return isinstance(probe, NextestAvailable)


def _compose(
    invocation: Invocation,
    action: Action,
    started: Started,
    decided: Decided,
    nextest: bool,
    env: Mapping[str, str],
    repo_root: Path,
) -> Planned | Refused:
    """The plan; its diagnostics are the warnings composing its steps prints.

    The test region refuses an unrenderable knob value, and execute mode
    refuses when the nextest config cannot be generated. Once the region holds
    a generated config, any failure composing the rest removes the file: no
    plan will name it, so no executor will.
    """
    planning = decided.step_env.planning
    families, role_policy = decided.families, started.role_policy
    vitest, vitest_warnings = VitestArm.of(families, decided.narrowing, planning)
    header = PlanHeader(
        action=action,
        profile_selection=invocation.profile_arg or role_policy.default_profile,
        scope=decided.decision.mode,
        include_infra=invocation.include_infra,
        nextest=nextest,
        role=invocation.role,
    )
    region = None
    if _has_test_region(families, decided.decision):
        region = testregion.resolve(_region_inputs(invocation, header, started, decided, env, repo_root))
        if isinstance(region, Refused):
            return Refused((*vitest_warnings, *region.lines), region.exit_code)
    try:
        plan = _assemble(invocation, header, started, decided, vitest, region, repo_root)
    except BaseException:
        if region is not None:
            region.discard_config()
        raise
    return Planned(plan, (*vitest_warnings, *(region.diagnostics if region else ())))


def _assemble(
    invocation: Invocation,
    header: PlanHeader,
    started: Started,
    decided: Decided,
    vitest: "VitestArm | None",
    region: TestRegion | None,
    repo_root: Path,
) -> Plan:
    """The plan's inputs, retry marker and steps around the region already resolved."""
    planning = decided.step_env.planning
    families, role_policy = decided.families, started.role_policy
    node_lane = NodeLane.compose(families, decided.decision, vitest)
    infra_suite_active = knobs.resolve(knobs.INFRA_SUITE_ACTIVE, planning).value
    runs_selective = _runs_selective_infra(
        families, role_policy, decided.selection, infra_suite_active
    )
    inputs = PlanInputs(
        families=families,
        include_infra=invocation.include_infra,
        decision=decided.decision,
        narrowing=decided.narrowing,
        selection=decided.selection,
        role_policy=role_policy,
        cargo_prio=started.prio.prefix,
        walls=started.walls,
        heavy_filter=started.heavy_filter,
        node_lane=node_lane,
        infra_suite_active=infra_suite_active,
        git_env_scrub_prefix=_git_env_scrub_prefix(repo_root, planning) if runs_selective else None,
        test_region=region,
    )
    return Plan(
        retry_marker=retry.marker(
            region.retry_applied if region else NO_NEXTEST_SUBSETS,
            node_lane.gui_specs_applied if node_lane else 0,
            planning,
            repo_root,
        ),
        header=header,
        selective_infra_note=invocation.include_infra
        and role_policy.infra_tier is InfraTier.SELECTIVE,
        scope=decided.decision,
        narrowing=decided.narrowing,
        heavy_partition=region.heavy_header if region else None,
        environment=decided.step_env.lines,
        steps=build_steps(inputs),
    )


def _has_test_region(families: "Families", decision: ScopeDecision) -> bool:
    """bash's _plan_has_test_passes: the test family over the Rust crates."""
    return families.test and decision.run_rust


def _selector(narrowed: Narrowing) -> Selector:
    """The narrowed packages, or the workspace when the run does not narrow."""
    return Packages(narrowed.packages) if narrowed.packages else Workspace()


def _region_inputs(
    invocation: Invocation,
    header: PlanHeader,
    started: Started,
    decided: Decided,
    env: Mapping[str, str],
    repo_root: Path,
) -> RegionInputs:
    walls, narrowed = started.walls, decided.narrowing
    return RegionInputs(
        role_policy=started.role_policy,
        profiles=header.profiles,
        debug_selector=_selector(narrowed),
        release_declared=decided.release_declared,
        release_delta_skips=decided.release_delta_skips,
        reaches_gui=narrowing.closure_reaches_reify_gui(decided.decision.mode, narrowed.closure),
        walls=PassWalls(walls.test, walls.test_release, walls.gui_feature_test),
        prio=started.prio.prefix,
        test_threads=invocation.test_threads,
        heavy_filter=started.heavy_filter,
        nextest=header.nextest,
        executing=invocation.plan_format is None,
        raw_env=env,
        planning_env=decided.step_env.planning,
        repo_root=repo_root,
    )


def _git_env_scrub_prefix(repo_root: Path, env: Mapping[str, str]) -> str:
    """The `env -u …` prefix, as bash's `$(reify_git_env_scrub_prefix)` captures it."""
    return bashcall.run(GIT_ENV_SCRUB_PREFIX, (), repo_root, env).stdout.rstrip("\n")


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
    """The outer `timeout` of each pass, one per timeout knob."""

    test: str
    test_release: str
    prebuild: str
    clippy: str
    gui_feature_test: str
    check: str

    @classmethod
    def resolve(cls, role_policy: RolePolicy, env: Mapping[str, str]) -> tuple["Walls", tuple[str, ...]]:
        """The walls, and one warning per malformed knob in bash's resolution order.

        The release wall takes the role's default, so offline warns once
        where bash warns twice (a D8 listed divergence).
        """
        resolutions = {
            "test": test_wall(role_policy, Profile.DEBUG, env),
            "test_release": test_wall(role_policy, Profile.RELEASE, env),
            "prebuild": knobs.resolve(knobs.PREBUILD_TIMEOUT, env),
            "clippy": knobs.resolve(knobs.CLIPPY_TIMEOUT, env),
            "gui_feature_test": knobs.resolve(knobs.GUI_FEATURE_TEST_TIMEOUT, env),
            "check": knobs.resolve(knobs.CHECK_TIMEOUT, env),
        }
        walls = cls(**{name: resolution.value for name, resolution in resolutions.items()})
        return walls, tuple(r.warning for r in resolutions.values() if r.warning)


GUI_VITEST_RUN = "scripts/gui-vitest-run.sh"
VITEST_SPEC_CHARS = re.compile(r"[A-Za-z0-9._/ -]+", re.ASCII)
GUI_SPECS_REJECTED = (
    f"verify.sh: WARNING — {knobs.GUI_RETRY_SPECS.name} contains characters outside "
    "[A-Za-z0-9._/ -] or a token beginning with '-'; ignoring the subset and running the full gui suite"
)


@dataclass(frozen=True)
class VitestArm:
    """The gui project's vitest run: the validated retry specs verbatim, or "" for the full suite."""

    specs: str

    @property
    def spec_count(self) -> int:
        return len(self.specs.split())

    @property
    def command(self) -> str:
        return f"../{GUI_VITEST_RUN} {self.specs}" if self.specs else f"../{GUI_VITEST_RUN}"

    @classmethod
    def of(
        cls, families: Families, narrowed: Narrowing, env: Mapping[str, str]
    ) -> tuple["VitestArm | None", tuple[str, ...]]:
        """The test side's arm when the node lane runs vitest, and the warning a rejected spec list prints.

        The specs reach a `bash -c '…'` string raw, so they must hold only
        path characters, and no token may read as an option.
        """
        if not (families.test and narrowed.run_gui_vitest):
            return None, ()
        specs = knobs.resolve(knobs.GUI_RETRY_SPECS, env).value
        if not specs:
            return cls(""), ()
        if VITEST_SPEC_CHARS.fullmatch(specs) and not any(
            token.startswith("-") for token in specs.split()
        ):
            return cls(specs), ()
        return cls(""), (GUI_SPECS_REJECTED,)


@dataclass(frozen=True)
class NodeLane:
    """The gui, sidecar and tree-sitter node projects, composed once per plan."""

    gui: NodeProject
    sidecar: NodeProject
    tree_sitter: NodeProject
    vitest: VitestArm | None

    @property
    def projects(self) -> tuple[NodeProject, ...]:
        return (self.gui, self.sidecar, self.tree_sitter)

    @property
    def gui_specs_applied(self) -> int:
        """The validated retry spec count the retry marker reports as gui=."""
        return self.vitest.spec_count if self.vitest else 0

    @property
    def gui_gate_path(self) -> str | None:
        return GUI_VITEST_RUN if self.vitest else None

    @classmethod
    def compose(
        cls, families: Families, decision: ScopeDecision, vitest: VitestArm | None
    ) -> "NodeLane | None":
        if not (decision.run_gui and (families.test or families.lint)):
            return None
        vitest_run = (vitest.command,) if vitest else ()
        return cls(
            gui=NodeProject("-d", "gui", "gui", 15, ("npm ci", "npm run typecheck", *vitest_run)),
            sidecar=NodeProject(
                "-f", "gui/sidecar/package-lock.json", "gui/sidecar", 10,
                ("npm ci", "npm run typecheck", "npm run typecheck:test"),
            ),
            tree_sitter=NodeProject(
                "-f", "tree-sitter-reify/package-lock.json", "tree-sitter-reify", 10, ("npm ci",),
            ),
            vitest=vitest,
        )


def _runs_selective_infra(
    families: Families, role_policy: RolePolicy, selection: Selection, infra_suite_active: bool
) -> bool:
    """A SELECTIVE role's test side runs its selected leaves, unless the infra suite is running."""
    return (
        role_policy.infra_tier is InfraTier.SELECTIVE
        and families.test
        and bool(selection.leaves)
        and not infra_suite_active
    )


@dataclass(frozen=True)
class PlanInputs:
    """Everything the emitters read, already resolved: a plan's steps are a function of it.

    git_env_scrub_prefix is the `env -u …` text bash's `$(reify_git_env_scrub_prefix)`
    captures, held exactly when selective infra runs: bash forks for it only then.
    """

    families: Families
    include_infra: bool
    decision: ScopeDecision
    narrowing: Narrowing
    selection: Selection
    role_policy: RolePolicy
    cargo_prio: str
    walls: Walls
    heavy_filter: str
    node_lane: NodeLane | None
    infra_suite_active: bool
    git_env_scrub_prefix: str | None
    test_region: TestRegion | None

    def __post_init__(self):
        runs = _runs_selective_infra(
            self.families, self.role_policy, self.selection, self.infra_suite_active
        )
        if runs != (self.git_env_scrub_prefix is not None):
            raise ValueError(
                "PlanInputs: the git-env scrub prefix is held exactly when selective infra runs "
                f"(runs={runs}, git_env_scrub_prefix={self.git_env_scrub_prefix!r})"
            )
        has_region = _has_test_region(self.families, self.decision)
        if has_region != (self.test_region is not None):
            held = "None" if self.test_region is None else "a TestRegion"
            plan = "has a test region" if has_region else "has no test region"
            raise ValueError(
                "PlanInputs: a test region is held exactly when the plan runs the test family "
                f"over RUN_RUST (the plan {plan}; test_region={held})"
            )

    @property
    def selector(self) -> Selector:
        return _selector(self.narrowing)

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
FRESHNESS_CHECK = Script("scripts/tree-sitter-freshness.sh", ("check",))
TEST_INFRA_CHECKS = (("tests/sync_comments_test.sh", "10m"),)
LINT_INFRA_CHECKS = (
    ("scripts/test_pm_standardization.sh", "10m"),
    ("scripts/check_event_inventory.sh", "5m"),
    ("scripts/check-nan-safe-ordering.sh", "5m"),
    ("scripts/check-compute-trampoline-registration.sh", "5m"),
)
RELEASE_PREBUILDS = ("reify-audit", "reify-cli")
AUDIT_MANIFEST = "crates/reify-audit/Cargo.toml"
AUDIT_BINARY_MISSING = ArtifactAssertion(
    AUDIT_MANIFEST,
    "target/release/reify-audit",
    "ERROR(#4624): reify-audit binary missing after pre-build step — PTODO gate will silently SKIP; "
    "restore the pre-step above or remove this check deliberately",
)
CLI_BINARY = "target/release/reify"
CLI_BINARY_STAMP = HeadStamp("target/.reify-bin-sha")
RUN_ALL_POOL_ENV = (
    ("REIFY_AUDIT_NO_COLD_BUILD", "1"),
    ("REIFY_RUN_ALL_EXCLUDE_HOST_INFRA", "1"),
    ("REIFY_RUN_ALL_CONTENT_SKIP", "1"),
)
RUN_ALL_POOL_WALL = "60m"
INFRA_LEAF_WALL = "10m"
INFRA_LEAF_UNSET = ("REIFY_VERIFY_EVENT_LOG",)


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
    lane = inputs.node_lane
    if not (inputs.lints_rust and lane):
        return ()
    start = NodeLaneStart(lane.projects)
    return (Step(StepKind.MAIN_SHELL, Phase.GUI_NODE_LAUNCH, start, lane.gui_gate_path, None),)


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
        "check", Packages((narrowing.GUI_CRATE,)), ("--features", "gui", "--tests"),
        inputs.walls.clippy, inputs.cargo_prio,
    )
    guard = GUI_CRATE_GUARD
    return (
        Step(
            StepKind.CARGO, Phase.GUI_FEATURE_CHECK, guard.around(check), guard.setup.path,
            guard.manifest,
        ),
    )


def _node_lane_join(inputs: PlanInputs) -> tuple[Step, ...]:
    if not (inputs.lints_rust and inputs.node_lane):
        return ()
    return (Step(StepKind.MAIN_SHELL, Phase.GUI_NODE_JOIN, NodeLaneJoin(), None, None),)


def _node_lane_plain(inputs: PlanInputs) -> tuple[Step, ...]:
    """Without a clippy pass to overlap, the node projects run in the foreground, one step each."""
    lane = inputs.node_lane
    if lane is None or inputs.lints_rust:
        return ()
    return (
        _tool(Phase.GUI_NODE, lane.gui, lane.gui_gate_path),
        _tool(Phase.GUI_NODE, lane.sidecar),
        _tool(Phase.GUI_NODE, lane.tree_sitter),
    )


def _infra_checks(checks) -> tuple[Step, ...]:
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
        for path, wall in checks
    )


def _test_infra_checks(inputs: PlanInputs) -> tuple[Step, ...]:
    if not (inputs.include_infra and inputs.decision.run_rust and inputs.families.test):
        return ()
    return _infra_checks(TEST_INFRA_CHECKS)


def _lint_infra_checks(inputs: PlanInputs) -> tuple[Step, ...]:
    if not (inputs.include_infra and inputs.lints_rust):
        return ()
    return _infra_checks(LINT_INFRA_CHECKS)


def _release_prebuild(inputs: PlanInputs, package: str) -> Step:
    manifest = f"crates/{package}/Cargo.toml"
    build = Cargo(
        "build", Packages((package,)), (), inputs.walls.prebuild, inputs.cargo_prio,
        release=True, stderr_to_stdout=True,
    )
    return Step(StepKind.CARGO, Phase.RELEASE_PREBUILD, Guarded("-f", manifest, build), None, manifest)


def _infra_pool(inputs: PlanInputs) -> tuple[Step, ...]:
    """The wholesale run_all.sh pool, after release pre-builds of the binaries its members drive."""
    if not (
        inputs.role_policy.infra_tier is InfraTier.WHOLESALE
        and inputs.decision.run_rust
        and inputs.families.test
        and not inputs.infra_suite_active
    ):
        return ()
    audit, cli = (_release_prebuild(inputs, package) for package in RELEASE_PREBUILDS)
    run_all = Script(
        RUN_ALL.path, wall=RUN_ALL_POOL_WALL, via_bash=True, env=RUN_ALL_POOL_ENV,
        stderr_to_stdout=True,
    )
    return (
        audit,
        Step(StepKind.BUILTIN, Phase.BOOKKEEPING, AUDIT_BINARY_MISSING, None, AUDIT_MANIFEST),
        cli,
        _tool(Phase.BOOKKEEPING, Guarded("-f", CLI_BINARY, CLI_BINARY_STAMP), guard_file=CLI_BINARY),
        _tool(Phase.INFRA_POOL, Guarded("-f", RUN_ALL.path, run_all), RUN_ALL.path, RUN_ALL.path),
    )


def _selective_infra(inputs: PlanInputs) -> tuple[Step, ...]:
    """One loop per selected leaf glob, each member under the git-env scrub.

    PlanInputs holds the scrub prefix exactly when selective infra runs.
    """
    scrub_prefix = inputs.git_env_scrub_prefix
    if scrub_prefix is None:
        return ()
    return tuple(
        _tool(
            Phase.INFRA_SELECTIVE,
            InfraLeafLoop(leaf.glob, leaf.env, INFRA_LEAF_WALL, scrub_prefix, INFRA_LEAF_UNSET),
        )
        for leaf in inputs.selection.leaves
    )


def _test_region(inputs: PlanInputs) -> tuple[Step, ...]:
    """The nextest passes and their semaphore region; PlanInputs holds one exactly when the plan has it."""
    region = inputs.test_region
    return () if region is None else testregion.steps(region)


def _freshness_check(inputs: PlanInputs) -> tuple[Step, ...]:
    families = inputs.families
    if not (inputs.decision.run_rust and (families.lint or families.typecheck or families.test)):
        return ()
    return (_tool(Phase.POSTFLIGHT, FRESHNESS_CHECK, FRESHNESS_CHECK.path),)


EMITTERS = (
    Emitter("static gates", tuple(gate.path for gate in STATIC_GATES), _static_gates),
    Emitter("compile gate", (COMPILE_GATE.path,), _compile_gate),
    Emitter("cargo check", (), _cargo_check),
    Emitter("node lane start", (GUI_VITEST_RUN,), _node_lane_start),
    Emitter("clippy", (), _clippy),
    Emitter("gui feature check", (GUI_CRATE_GUARD.setup.path,), _gui_feature_check),
    Emitter("node lane join", (), _node_lane_join),
    Emitter("node lane, plain", (GUI_VITEST_RUN,), _node_lane_plain),
    Emitter("test infra checks", tuple(path for path, _ in TEST_INFRA_CHECKS), _test_infra_checks),
    Emitter("lint infra checks", tuple(path for path, _ in LINT_INFRA_CHECKS), _lint_infra_checks),
    Emitter("infra pool", (RUN_ALL.path,), _infra_pool),
    Emitter("selective infra", (), _selective_infra),
    Emitter("test region", testregion.GATE_PATHS, _test_region),
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
