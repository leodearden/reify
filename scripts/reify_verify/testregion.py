"""testregion — the test region's inputs, resolved once per plan, and its steps in add_test_passes' order (PRD D1, D2).

A plan has a test region when it runs the test family over the Rust crates
(bash's _plan_has_test_passes). resolve reads every knob the region needs, at
the tier bash reads it: the raw env for what verify.sh reads at load, the
planning env for what it reads while planning. steps is a pure function of
the TestRegion it returns.

release_declared and decide_release_delta run at bash's load position for
every action, since bash computes the release-sensitive crate list and decides
the merge's release-delta skip before it knows the plan holds a release pass.
A skipped release pass is the RELEASE_SKIP_MARKER line in its place.

On a failed_only retry the region also decides, through retry, whether each
nextest profile pass narrows to its subset, and collects the lines each
refusal prints.
"""

from collections.abc import Mapping
from dataclasses import dataclass, replace
from pathlib import Path

from reify_verify import bashcall, gitcall, knobs, retry, scope
from reify_verify.bashcall import BashCall
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
    Marker,
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
from reify_verify.retry import NO_NEXTEST_SUBSETS, NextestSubsetsApplied

RELEASE_DECLARED_SET = BashCall("scripts/release-scope-lib.sh", "release_declared_set")
RELEASE_DELTA_REQUIRES_PASS = BashCall("scripts/release-scope-lib.sh", "release_delta_requires_pass")
RELEASE_SKIP_MARKER = Marker("RELEASE-PASS: skipped (delta-clean)")
VERIFY_SH = "scripts/verify.sh"
PSI_GATE = Script(VERIFY_SH, ("psi-gate",))
COMPILE_GATE = Script(VERIFY_SH, ("compile-gate",))
GUI_SIDECAR_PLACEHOLDER = "scripts/ensure-gui-sidecar-placeholder.sh"
GUI_CRATE_MANIFEST = "gui/src-tauri/Cargo.toml"
GUI_FEATURES = ("gui",)
ATTEMPT_STAMP_DIR = "target"
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
    result = bashcall.ask(RELEASE_DECLARED_SET, (), repo_root, planning)
    if result.returncode != 0:
        return Refused(result.stderr_lines, result.returncode)
    return result.lines


def decide_release_delta(
    role_policy: RolePolicy, planning: Mapping[str, str], repo_root: Path
) -> tuple[bool, tuple[str, ...]]:
    """Whether the run skips its release pass, and the predicate's stderr lines.

    Only an eligible role with REIFY_RELEASE_DELTA_SKIP=1 asks. The caller's
    affected-crate override is the predicate's whole input; without one, the
    merge's changed files are, and a delta verify cannot derive keeps the pass
    without asking. A non-zero answer skips.
    """
    if not (role_policy.release_delta_skip_eligible and knobs.resolve(knobs.RELEASE_DELTA_SKIP, planning).value):
        return False, ()
    overridden = knobs.resolve(knobs.AFFECTED_CRATES_OVERRIDE, planning).value
    changed = () if overridden else _merge_delta(repo_root, planning)
    if changed is None:
        return False, ()
    answer = bashcall.ask(RELEASE_DELTA_REQUIRES_PASS, changed, repo_root, planning)
    return answer.returncode != 0, answer.stderr_lines


def _merge_delta(repo_root: Path, planning: Mapping[str, str]) -> tuple[str, ...] | None:
    """The files the merge changes, or None when that cannot be derived.

    A merge in progress diffs HEAD against MERGE_HEAD; a merge commit at HEAD
    diffs against its first parent; anything else, or any git failure, is
    underivable. git's stderr is dropped, as bash's `2>/dev/null` drops it.
    """
    if scope.merge_in_progress(repo_root, planning):
        diff = gitcall.run(repo_root, planning, "diff", "--name-only", "HEAD", "MERGE_HEAD")
    else:
        parents = gitcall.run(repo_root, planning, "rev-list", "--parents", "-n1", "HEAD")
        if parents.returncode != 0 or len(parents.stdout.split()) < 3:
            return None
        diff = gitcall.run(repo_root, planning, "diff", "--name-only", "HEAD^1", "HEAD")
    return diff.lines if diff.returncode == 0 else None


@dataclass(frozen=True)
class PassWalls:
    """The outer wall of the debug, release and gui-feature passes."""

    debug: str
    release: str
    gui_feature: str


@dataclass(frozen=True)
class TestRegion:
    """The region's passes and how they run; heavy_header is what the plan header reports.

    A profile whose pass the release-delta decision skips holds its Marker.
    retry_applied is what the retry marker reports; diagnostics are the lines
    resolving the region prints.
    """

    attempt_stamp: AttemptStamp | None
    passes: tuple[PassSpec | Marker, ...]
    gui_feature: PassSpec | None
    runner: TestRunner
    heavy_header: HeavyPartition
    retry_applied: NextestSubsetsApplied
    diagnostics: tuple[str, ...]


def resolve(
    *,
    role_policy: RolePolicy,
    profiles: tuple[Profile, ...],
    debug_selector: Selector,
    release_declared: tuple[str, ...],
    release_delta_skips: bool,
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

    The config is resolved once, and only when a nextest pass will name it. A
    retry decides its eligibility first, so the tree-drift line precedes every
    other line the region prints.
    """
    heavy = heavy_partition(role_policy, raw_env)
    passes = tuple(
        RELEASE_SKIP_MARKER if profile is Profile.RELEASE and release_delta_skips
        else _profile_pass(profile, role_policy, debug_selector, release_declared, walls, heavy)
        for profile in profiles
    )
    gui_feature = _gui_feature_pass(walls) if role_policy.gui_feature_test_pass and reaches_gui else None
    eligible, diagnostics = False, ()
    if retry.is_retry(planning_env):
        sidecar = repo_root / str(knobs.resolve(knobs.ATTEMPT_SIDECAR, raw_env).value)
        eligible, diagnostics = retry.eligibility(sidecar, planning_env)
    config = None
    if nextest and (any(isinstance(spec, PassSpec) for spec in passes) or gui_feature):
        config = nextest_config_path(executing, raw_env, planning_env, test_threads, repo_root)
        if isinstance(config, Refused):
            return Refused((*diagnostics, *config.lines), config.exit_code)
        diagnostics = (*diagnostics, *config.diagnostics)
    applied = NO_NEXTEST_SUBSETS
    if eligible:
        passes, applied, refusals = _narrow(passes, nextest, raw_env, planning_env, repo_root)
        diagnostics = (*diagnostics, *refusals)
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
        retry_applied=applied,
        diagnostics=diagnostics,
    )


def _narrow(
    passes: tuple[PassSpec | Marker, ...],
    nextest: bool,
    raw_env: Mapping[str, str],
    planning_env: Mapping[str, str],
    repo_root: Path,
) -> tuple[tuple[PassSpec | Marker, ...], NextestSubsetsApplied, tuple[str, ...]]:
    """An eligible retry's profile passes, each narrowed to its subset where it can be.

    Returns the passes, the subset sizes applied and the refusals, in pass
    order. The cargo-test fallback cannot filter, so each of its passes
    refuses; a skipped pass resolves nothing.
    """
    max_subset = str(knobs.resolve(knobs.RETRY_MAX_SUBSET, raw_env).value)
    narrowed, applied, refusals = [], {}, []
    for spec in passes:
        if isinstance(spec, PassSpec) and not nextest:
            refusals.append(retry.NO_NEXTEST_REFUSAL)
        elif isinstance(spec, PassSpec):
            ids, refused = retry.subset(spec.profile, planning_env, max_subset, repo_root)
            refusals.extend(refused)
            if ids:
                spec = replace(spec, retry_ids=ids)
                applied[spec.profile] = len(ids)
        narrowed.append(spec)
    sizes = NextestSubsetsApplied(applied.get(Profile.DEBUG, 0), applied.get(Profile.RELEASE, 0))
    return tuple(narrowed), sizes, tuple(refusals)


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
    if not role_policy.attempt_sidecar or retry.is_retry(planning_env):
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
        *(_pass_step(spec, region.runner) for spec in region.passes),
        *gui_feature,
        Step(StepKind.SENTINEL, Phase.TEST_SLOT_RELEASE, SemaphoreRelease(), None, None, REGION_ENDS),
    )


def _pass_step(spec: PassSpec | Marker, runner: TestRunner) -> Step:
    if isinstance(spec, Marker):
        return Step(StepKind.BUILTIN, Phase.RELEASE_SKIP_MARKER, spec, None, None)
    return emit_pass(spec, runner)
