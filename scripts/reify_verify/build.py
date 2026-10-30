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
import re
import shutil
from collections.abc import Callable, Mapping
from dataclasses import dataclass
from pathlib import Path

from reify_verify import bashcall, childproc, environment, knobs, narrowing, scope, selection
from reify_verify.bashcall import BashCall, DataFile
from reify_verify.invocation import Action, Invocation
from reify_verify.narrowing import Narrowing
from reify_verify.plan import (
    ArtifactAssertion,
    Cargo,
    Chain,
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
    RetryMarker,
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
GIT_ENV_SCRUB_PREFIX = BashCall("scripts/lib_git_env_scrub.sh", "reify_git_env_scrub_prefix")
RUN_ALL = DataFile("tests/infra/run_all.sh")


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
    families = Families.of(invocation.action)
    vitest, vitest_warnings = VitestArm.of(families, narrowed, planning)
    inputs = PlanInputs(
        families=families,
        include_infra=invocation.include_infra,
        decision=decision,
        narrowing=narrowed,
        selection=selected,
        role_policy=role_policy,
        cargo_prio=prio.prefix,
        walls=Walls.resolve(env),
        node_lane=NodeLane.compose(families, decision, vitest),
        infra_suite_active=knobs.resolve(knobs.INFRA_SUITE_ACTIVE, planning).value,
        git_env_scrub_prefix=lambda: _git_env_scrub_prefix(repo_root, planning),
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
    steps = build_steps(inputs)
    plan = Plan(
        retry_marker=_retry_marker(inputs.node_lane, TEST_REGION_UNPLANNED, planning, repo_root),
        header=header,
        selective_infra_note=invocation.include_infra
        and role_policy.infra_tier is InfraTier.SELECTIVE,
        scope=decision,
        narrowing=narrowed,
        heavy_partition=None,
        environment=step_env.lines,
        steps=steps,
    )
    diagnostics = (
        *filter(None, (prio.warning,)),
        *resolved.notices,
        *step_env.warnings,
        *gather_warnings,
        *selection_warnings,
        *narrowing_warnings,
        *vitest_warnings,
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
class NextestSubsetsApplied:
    """The within-ceiling nextest subset sizes the test region applied, per profile."""

    debug: int
    release: int


TEST_REGION_UNPLANNED = NextestSubsetsApplied(debug=0, release=0)
RETRY_SCOPE_NARROWED = "failed_only"
RUN_ALL_SUBSET_MATCHED = re.compile(r"@@REIFY_RUN_ALL_SUBSET_MATCHED=([0-9]+)@@")
IFS_WORDS = re.compile(r"[^ \t\n]+")


def _retry_marker(
    node_lane: "NodeLane | None",
    nextest: NextestSubsetsApplied,
    env: Mapping[str, str],
    repo_root: Path,
) -> RetryMarker | None:
    """The failed_only retry's applied subset sizes; None unless at least one suite really narrowed.

    `nextest` comes from the test region, which γ2 (#9099) ports; until then
    it is TEST_REGION_UNPLANNED.
    """
    if knobs.resolve(knobs.RETRY_SCOPE, env).value != RETRY_SCOPE_NARROWED:
        return None
    marker = RetryMarker(
        nextest_debug=nextest.debug,
        nextest_release=nextest.release,
        run_all=_run_all_subset_size(env, repo_root),
        gui=node_lane.gui_specs_applied if node_lane else 0,
    )
    narrowed = (marker.nextest_debug, marker.nextest_release, marker.run_all, marker.gui)
    return marker if any(narrowed) else None


def _run_all_subset_size(env: Mapping[str, str], repo_root: Path) -> int:
    """How many REIFY_RUN_ALL_MEMBER_SUBSET members run_all.sh's count-only probe matches.

    run_all.sh owns the member-skip predicate, so its count is the truth. Its
    stderr is dropped. Without the script, or without the count in its
    stdout, the subset's word count stands in; the words are never globbed.
    """
    subset = knobs.resolve(knobs.RUN_ALL_MEMBER_SUBSET, env).value
    if not subset:
        return 0
    if RUN_ALL.is_file(repo_root):
        probe = childproc.run(
            ["bash", RUN_ALL.path],
            repo_root,
            {**env, "REIFY_RUN_ALL_SUBSET_COUNT_ONLY": "1", knobs.RUN_ALL_MEMBER_SUBSET.name: subset},
        )
        matched = RUN_ALL_SUBSET_MATCHED.search(probe.stdout)
        if matched:
            return int(matched.group(1))
    return len(IFS_WORDS.findall(subset))


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
    infra_suite_active: bool
    git_env_scrub_prefix: Callable[[], str]

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
    """One loop per selected leaf glob, each member under the git-env scrub."""
    leaves = inputs.selection.leaves
    if not (
        inputs.role_policy.infra_tier is InfraTier.SELECTIVE
        and inputs.families.test
        and leaves
        and not inputs.infra_suite_active
    ):
        return ()
    scrub_prefix = inputs.git_env_scrub_prefix()
    return tuple(
        _tool(
            Phase.INFRA_SELECTIVE,
            InfraLeafLoop(leaf.glob, leaf.env, INFRA_LEAF_WALL, scrub_prefix, INFRA_LEAF_UNSET),
        )
        for leaf in leaves
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
    Emitter("node lane start", (GUI_VITEST_RUN,), _node_lane_start),
    Emitter("clippy", (), _clippy),
    Emitter("gui feature check", (GUI_SIDECAR_PLACEHOLDER,), _gui_feature_check),
    Emitter("node lane join", (), _node_lane_join),
    Emitter("node lane, plain", (GUI_VITEST_RUN,), _node_lane_plain),
    Emitter("test infra checks", tuple(path for path, _ in TEST_INFRA_CHECKS), _test_infra_checks),
    Emitter("lint infra checks", tuple(path for path, _ in LINT_INFRA_CHECKS), _lint_infra_checks),
    Emitter("infra pool", (RUN_ALL.path,), _infra_pool),
    Emitter("selective infra", (), _selective_infra),
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
