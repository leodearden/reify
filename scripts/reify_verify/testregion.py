"""testregion — the test region's inputs, resolved once per plan, and its steps in add_test_passes' order (PRD D1, D2).

A plan has a test region when it runs the test family over the Rust crates
(bash's _plan_has_test_passes). resolve reads every knob the region needs, at
the tier bash reads it: the raw env for what verify.sh reads at load, the
planning env for what it reads while planning. steps is a pure function of
the TestRegion it returns.

release_declared runs at bash's load position for every action, since bash
computes the release-sensitive crate list before it knows the plan holds a
release pass.
"""

from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path

from reify_verify import bashcall, knobs
from reify_verify.bashcall import BashCall, BashCallError
from reify_verify.narrowing import GUI_CRATE
from reify_verify.nextest import (
    ConfirmRecording,
    PassGuard,
    PassSpec,
    TestRunner,
    emit_pass,
    nextest_config_path,
)
from reify_verify.plan import (
    CONFIRM_RECORDER,
    AttemptStamp,
    Guarded,
    Packages,
    Phase,
    Refused,
    Script,
    Selector,
    SemaphoreAcquire,
    SemaphoreRelease,
    Step,
    StepKind,
    Workspace,
)
from reify_verify.policy import HeavyPartition, Profile, ReleaseSelector, RolePolicy, heavy_partition

RELEASE_DECLARED_SET = BashCall("scripts/release-scope-lib.sh", "release_declared_set")
VERIFY_SH = "scripts/verify.sh"
PSI_GATE = Script(VERIFY_SH, ("psi-gate",))
COMPILE_GATE = Script(VERIFY_SH, ("compile-gate",))
GUI_SIDECAR_PLACEHOLDER = "scripts/ensure-gui-sidecar-placeholder.sh"
GUI_CRATE_MANIFEST = "gui/src-tauri/Cargo.toml"
GUI_FEATURES = ("gui",)
ATTEMPT_STAMP_DIR = "target"
RETRY_SCOPE_NARROWED = "failed_only"
GATE_PATHS = (VERIFY_SH, CONFIRM_RECORDER, GUI_SIDECAR_PLACEHOLDER)

PSI_GATE_ANNOTATION = (
    "# PSI gate: contended wait emits @@REIFY_CLOCK_STOP@@/HEARTBEAT/START@@ markers (reason=psi_pressure);",
    "#   the clock-stop span is excluded from verify_command_timeout_secs by dark_factory:1916 (task 4838).",
)
REGION_BEGINS = (
    "# >>> test-run semaphore: ACQUIRE held slot — clock-stop region BEGINS "
    "(TEST-EXECUTION gated, held in verify.sh)",
    "#     A contended wait emits @@REIFY_CLOCK_STOP@@/@@REIFY_CLOCK_HEARTBEAT@@/@@REIFY_CLOCK_START@@ markers",
    "#     to stderr (reason=test_slot_starvation). dark_factory:1916 excludes the marked wait span",
    "#     from verify_command_timeout_secs. REIFY_TEST_SEMAPHORE_WAIT=unlimited activates",
    "#     continuous blocking wait (clock-stop mode); task 4838 activates the DF seam.",
)
REGION_ENDS = (
    "# <<< test-run semaphore: RELEASE held slot — clock-stop region ENDS "
    "(TEST-EXECUTION gated region finished)",
)


def release_declared(repo_root: Path, planning: Mapping[str, str]) -> tuple[str, ...] | Refused:
    """The release-sensitive crates release-sensitive-crates.txt declares.

    bash assigns the list under `set -e`, so a failing call ends the run with
    the lib's own stderr and status.
    """
    try:
        return bashcall.run(RELEASE_DECLARED_SET, (), repo_root, planning).lines
    except BashCallError as err:
        return Refused(err.result.stderr_lines, err.result.returncode)


@dataclass(frozen=True)
class PassWalls:
    """The outer wall of the debug, release and gui-feature passes."""

    debug: str
    release: str
    gui_feature: str


@dataclass(frozen=True)
class TestRegion:
    """The region's passes and how they run; heavy_header is what the plan header reports."""

    attempt_stamp: AttemptStamp | None
    passes: tuple[PassSpec, ...]
    gui_feature: PassSpec | None
    runner: TestRunner
    heavy_header: HeavyPartition


def resolve(
    *,
    role_policy: RolePolicy,
    profiles: tuple[Profile, ...],
    debug_selector: Selector,
    release_declared: tuple[str, ...],
    reaches_gui: bool,
    walls: PassWalls,
    prio: str,
    test_threads: int | None,
    heavy_filter: str,
    nextest: bool,
    executing: bool,
    raw_env: Mapping[str, str],
    planning_env: Mapping[str, str],
    repo_root: Path,
) -> TestRegion | Refused:
    """The region; a refusal when execute mode cannot generate the nextest config.

    The config is resolved once, and only when a nextest pass will name it.
    """
    heavy = heavy_partition(role_policy, raw_env)
    passes = tuple(
        _profile_pass(profile, role_policy, debug_selector, release_declared, walls, heavy)
        for profile in profiles
    )
    gui_feature = _gui_feature_pass(walls) if role_policy.gui_feature_test_pass and reaches_gui else None
    config = None
    if nextest and (passes or gui_feature):
        config = nextest_config_path(executing, raw_env, planning_env, test_threads, repo_root)
        if isinstance(config, Refused):
            return config
    runner = TestRunner(
        prio=prio,
        test_threads=test_threads,
        config=config,
        heavy_filter=heavy_filter,
        no_fail_fast=not role_policy.fail_fast,
        confirm=ConfirmRecording.resolve(raw_env) if role_policy.records_confirm_manifest else None,
    )
    return TestRegion(
        attempt_stamp=_attempt_stamp(role_policy, profiles, raw_env, planning_env),
        passes=passes,
        gui_feature=gui_feature,
        runner=runner,
        heavy_header=heavy if nextest else HeavyPartition.INCLUDED,
    )


def _profile_pass(
    profile: Profile,
    role_policy: RolePolicy,
    debug_selector: Selector,
    release_declared: tuple[str, ...],
    walls: PassWalls,
    heavy: HeavyPartition,
) -> PassSpec:
    """The debug pass takes the narrowed selector; the release pass the role's release selector."""
    if profile is Profile.DEBUG:
        return PassSpec(profile, debug_selector, walls.debug, heavy=heavy)
    if role_policy.release_selector is ReleaseSelector.WORKSPACE:
        selector = Workspace()
    else:
        selector = Packages(release_declared)
    return PassSpec(profile, selector, walls.release, heavy=heavy)


def _gui_feature_pass(walls: PassWalls) -> PassSpec:
    """-p reify-gui --features gui, never filtered: a filter would narrow away the gui-gated tests."""
    return PassSpec(
        Profile.DEBUG,
        Packages((GUI_CRATE,)),
        walls.gui_feature,
        features=GUI_FEATURES,
        heavy=HeavyPartition.INCLUDED,
        guard=PassGuard(GUI_CRATE_MANIFEST, Script(GUI_SIDECAR_PLACEHOLDER)),
    )


def _attempt_stamp(
    role_policy: RolePolicy,
    profiles: tuple[Profile, ...],
    raw_env: Mapping[str, str],
    planning_env: Mapping[str, str],
) -> AttemptStamp | None:
    """The attempt sidecar a recording role writes, except on the failed_only retry that reads it."""
    retrying = knobs.resolve(knobs.RETRY_SCOPE, planning_env).value == RETRY_SCOPE_NARROWED
    if not role_policy.attempt_sidecar or retrying:
        return None
    return AttemptStamp(str(knobs.resolve(knobs.ATTEMPT_SIDECAR, raw_env).value), profiles)


def steps(region: TestRegion) -> tuple[Step, ...]:
    """The stamp, the psi and compile gates, then the passes inside the semaphore region."""
    stamp = () if region.attempt_stamp is None else (
        Step(
            StepKind.TOOL, Phase.BOOKKEEPING, Guarded("-d", ATTEMPT_STAMP_DIR, region.attempt_stamp),
            None, None,
        ),
    )
    gui_feature = () if region.gui_feature is None else (emit_pass(region.gui_feature, region.runner),)
    return (
        *stamp,
        Step(StepKind.TOOL, Phase.PSI_GATE, PSI_GATE, VERIFY_SH, None, PSI_GATE_ANNOTATION),
        Step(StepKind.TOOL, Phase.COMPILE_GATE, COMPILE_GATE, VERIFY_SH, None),
        Step(StepKind.SENTINEL, Phase.TEST_SLOT_ACQUIRE, SemaphoreAcquire(), None, None, REGION_BEGINS),
        *(emit_pass(spec, region.runner) for spec in region.passes),
        *gui_feature,
        Step(StepKind.SENTINEL, Phase.TEST_SLOT_RELEASE, SemaphoreRelease(), None, None, REGION_ENDS),
    )
