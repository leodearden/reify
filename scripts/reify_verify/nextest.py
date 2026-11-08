"""nextest — the one nextest-pass emitter and the config file every pass names (PRD D1, §12).

emit_pass builds every test pass, the gui-feature pass included: what varies
per pass is its PassSpec, and what the role sets for all of them is the
TestRunner. Without nextest the pass falls back to cargo test. A spec's
PassKind says which pass it is: its ledger phase, and whether it records
failing ids for --confirm-failed.

nextest_config_path is the package's only call of gen-nextest-config.sh. The
test region calls it once per plan and hands the value to every pass, so the
file is generated once, and only when a pass will name it.
"""

from collections.abc import Mapping
from dataclasses import dataclass
from enum import Enum
from pathlib import Path

from reify_verify import bashcall, knobs
from reify_verify.bashcall import ScriptCall
from reify_verify.gatepaths import CrateGuard
from reify_verify.plan import (
    CONFIRM_RECORDER,
    CargoTestRun,
    ConfirmRecorded,
    NextestFlags,
    NextestRun,
    Phase,
    Refused,
    Selector,
    SlotFdClosed,
    Step,
    StepKind,
    unquotable_knob,
)
from reify_verify.policy import PROFILE_TABLE, HeavyPartition, Profile

GEN_NEXTEST_CONFIG = ScriptCall("scripts/gen-nextest-config.sh")
CLI_TEST_THREADS = "REIFY_NEXTEST_CLI_TEST_THREADS"
PLACEHOLDER_NAME = "reify-nextest-occt.<print-plan-placeholder>"


class ConfigOrigin(Enum):
    """Who owns the config file: the caller, nobody (print mode), or the executor's cleanup."""

    OVERRIDE = "override"
    PLACEHOLDER = "placeholder"
    GENERATED = "generated"


@dataclass(frozen=True)
class NextestConfig:
    """The config file, plus the lines its generator printed while succeeding."""

    path: str
    origin: ConfigOrigin
    diagnostics: tuple[str, ...] = ()

    def discard(self) -> None:
        """Remove a generated file no plan will name; the caller's file and the placeholder stay."""
        if self.origin is ConfigOrigin.GENERATED:
            Path(self.path).unlink(missing_ok=True)


def nextest_config_path(
    executing: bool,
    raw_env: Mapping[str, str],
    planning_env: Mapping[str, str],
    test_threads: int | None,
    repo_root: Path,
) -> NextestConfig | Refused:
    """The --config-file every nextest pass of the plan names.

    The caller's REIFY_VERIFY_NEXTEST_CONFIG wins, read from the raw env as bash
    reads it at load. Print mode names a placeholder and runs nothing. Execute
    mode generates the file under the planning env, the CLI --test-threads
    reaching the generator alone, as bash's per-invocation prefix: an
    exported value would reach every nested infra suite.
    """
    override = knobs.resolve(knobs.NEXTEST_CONFIG, raw_env).value
    if override:
        return NextestConfig(str(override), ConfigOrigin.OVERRIDE)
    if not executing:
        tmpdir = planning_env.get("TMPDIR") or "/tmp"
        return NextestConfig(f"{tmpdir}/{PLACEHOLDER_NAME}", ConfigOrigin.PLACEHOLDER)
    generator_env = {**planning_env, CLI_TEST_THREADS: str(test_threads) if test_threads else ""}
    result = bashcall.run_script(GEN_NEXTEST_CONFIG, (), repo_root, generator_env)
    if result.returncode != 0:
        return Refused(
            (
                *result.stderr_lines,
                f"verify.sh: ERROR — {Path(GEN_NEXTEST_CONFIG.path).name} refused to generate "
                f"the nextest config (exit {result.returncode}); its diagnostic is immediately above",
            ),
            result.returncode,
        )
    return NextestConfig(
        result.stdout.rstrip("\n"), ConfigOrigin.GENERATED, tuple(result.stderr_lines)
    )


class PassKind(Enum):
    """Which pass a spec is: a profile's own test pass, or the gui-feature pass beside them."""

    PROFILE = "profile"
    GUI_FEATURE = "gui_feature"

    @property
    def records(self) -> bool:
        """Only a profile's pass records its failing ids, into that profile's confirm manifest."""
        return self is PassKind.PROFILE


@dataclass(frozen=True)
class PassSpec:
    """What one test pass runs: the facts that vary from pass to pass.

    The gui-feature pass never carries a filter: one would narrow away the
    gui-gated tests it exists to run.
    """

    kind: PassKind
    profile: Profile
    selector: Selector
    wall: str
    features: tuple[str, ...] = ()
    heavy: HeavyPartition = HeavyPartition.INCLUDED
    retry_ids: tuple[str, ...] = ()
    guard: CrateGuard | None = None

    def __post_init__(self):
        filtered = self.heavy is not HeavyPartition.INCLUDED or self.retry_ids
        if self.kind is PassKind.GUI_FEATURE and filtered:
            raise ValueError(
                f"PassSpec: the gui-feature pass carries no filter (heavy={self.heavy.value}, "
                f"retry_ids={self.retry_ids!r})"
            )


@dataclass(frozen=True)
class ConfirmPaths:
    manifest: str
    sidecar: str


@dataclass(frozen=True)
class ConfirmRecording:
    """Where a recording run's passes write their failing ids, for a later --confirm-failed."""

    junit: str
    per_profile: tuple[tuple[Profile, ConfirmPaths], ...]

    @classmethod
    def resolve(
        cls, raw_env: Mapping[str, str], profiles: tuple[Profile, ...]
    ) -> "ConfirmRecording | Refused":
        """The paths `profiles`' passes record to, as bash reads them at load, from the raw env.

        A path the recorder's quotes cannot hold refuses the plan.
        """
        read = {knobs.CONFIRM_JUNIT.name: str(knobs.resolve(knobs.CONFIRM_JUNIT, raw_env).value)}
        for profile in profiles:
            policy = PROFILE_TABLE[profile]
            for knob in (policy.confirm_manifest, policy.confirm_sidecar):
                read[knob.name] = str(knobs.resolve(knob, raw_env).value)
        refused = unquotable_knob(read.items(), ConfirmRecorded.PATH_QUOTE)
        if refused is not None:
            return refused
        return cls(
            read[knobs.CONFIRM_JUNIT.name],
            tuple(
                (
                    profile,
                    ConfirmPaths(
                        read[PROFILE_TABLE[profile].confirm_manifest.name],
                        read[PROFILE_TABLE[profile].confirm_sidecar.name],
                    ),
                )
                for profile in profiles
            ),
        )

    def paths(self, profile: Profile) -> ConfirmPaths:
        return dict(self.per_profile)[profile]


@dataclass(frozen=True)
class TestRunner:
    """What the role sets for every pass alike.

    nextest says whether cargo-nextest or the cargo-test fallback runs the
    passes; config is the file every nextest pass names, None when no pass
    will name one. A recording run keeps going past a failure, so the manifest it records
    lists every failing test, not only the first (bash's write gate).
    """

    prio: str
    test_threads: int | None
    nextest: bool
    config: NextestConfig | None
    heavy_filter: str
    no_fail_fast: bool
    confirm: ConfirmRecording | None

    def __post_init__(self):
        if self.confirm is not None and not self.no_fail_fast:
            raise ValueError("TestRunner: a run that records a confirm manifest runs --no-fail-fast")
        if not self.nextest and self.config is not None:
            raise ValueError(
                f"TestRunner: the cargo-test fallback names no nextest config (config={self.config!r})"
            )
        if not self.nextest and self.confirm is not None:
            raise ValueError(
                f"TestRunner: the cargo-test fallback records no confirm manifest (confirm={self.confirm!r})"
            )


def emit_pass(spec: PassSpec, runner: TestRunner) -> Step:
    """The one test pass `spec` describes, run as `runner` runs every pass.

    A nextest pass names the runner's config, so a nextest runner holding none
    is refused. A recording runner wraps a nextest pass of a recording kind in
    the confirm recorder; the cargo-test fallback records nothing. The pass
    runs without the semaphore slot's descriptor.
    """
    release = bool(PROFILE_TABLE[spec.profile].cargo_release_flag)
    gate_path = None
    if not runner.nextest:
        run = CargoTestRun(
            spec.selector, release, spec.features, spec.wall, runner.prio, runner.test_threads or 1
        )
    elif runner.config is None:
        raise ValueError(
            "emit_pass: a nextest runner names its --config-file, and this one holds none "
            f"(spec kind={spec.kind.value}, profile={spec.profile.value})"
        )
    else:
        flags = NextestFlags(
            spec.heavy, runner.heavy_filter, runner.no_fail_fast, runner.test_threads, spec.retry_ids
        )
        run = NextestRun(
            spec.selector, release, spec.features, spec.wall, runner.prio, flags, runner.config.path
        )
        if runner.confirm is not None and spec.kind.records:
            paths = runner.confirm.paths(spec.profile)
            run = ConfirmRecorded(run, runner.confirm.junit, paths.manifest, paths.sidecar, spec.profile)
            gate_path = CONFIRM_RECORDER
    guard_file = None
    if spec.guard is not None:
        run = spec.guard.around(run)
        gate_path, guard_file = spec.guard.setup.path, spec.guard.manifest
    phase = _phase(spec, nextest=runner.nextest)
    return Step(StepKind.CARGO, phase, SlotFdClosed(run), gate_path, guard_file)


def _phase(spec: PassSpec, nextest: bool) -> Phase:
    """The ledger phase: the gui-feature pass, else the profile's pass; run by nextest or cargo test."""
    if spec.kind is PassKind.GUI_FEATURE:
        return Phase.GUI_FEATURE_NEXTEST if nextest else Phase.GUI_FEATURE_CARGO_TEST
    if spec.profile is Profile.RELEASE:
        return Phase.RELEASE_NEXTEST if nextest else Phase.RELEASE_CARGO_TEST
    return Phase.DEBUG_NEXTEST if nextest else Phase.DEBUG_CARGO_TEST
