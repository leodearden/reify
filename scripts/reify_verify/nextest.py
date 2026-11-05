"""nextest — the one nextest-pass emitter and the config file every pass names (PRD D1, §12).

emit_pass builds every test pass, the gui-feature pass included: what varies
per pass is its PassSpec, and what the role sets for all of them is the
TestRunner. Without nextest the pass falls back to cargo test.

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
from reify_verify.plan import (
    CONFIRM_RECORDER,
    CargoTestRun,
    Chain,
    ConfirmRecorded,
    Guarded,
    NextestFlags,
    NextestRun,
    Phase,
    Refused,
    Script,
    Selector,
    SlotFdClosed,
    Step,
    StepKind,
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


@dataclass(frozen=True)
class PassGuard:
    """The pass runs only when `manifest` exists, after `setup`."""

    manifest: str
    setup: Script


@dataclass(frozen=True)
class PassSpec:
    """What one test pass runs: the facts that vary from pass to pass."""

    profile: Profile
    selector: Selector
    wall: str
    features: tuple[str, ...] = ()
    heavy: HeavyPartition = HeavyPartition.INCLUDED
    retry_ids: tuple[str, ...] = ()
    guard: PassGuard | None = None


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
    def resolve(cls, raw_env: Mapping[str, str]) -> "ConfirmRecording":
        """The paths as bash reads them at load, from the raw env."""
        return cls(
            str(knobs.resolve(knobs.CONFIRM_JUNIT, raw_env).value),
            tuple(
                (
                    profile,
                    ConfirmPaths(
                        str(knobs.resolve(policy.confirm_manifest, raw_env).value),
                        str(knobs.resolve(policy.confirm_sidecar, raw_env).value),
                    ),
                )
                for profile, policy in PROFILE_TABLE.items()
            ),
        )

    def paths(self, profile: Profile) -> ConfirmPaths:
        return dict(self.per_profile)[profile]


@dataclass(frozen=True)
class TestRunner:
    """What the role sets for every pass alike; config None means no nextest, so cargo test runs."""

    prio: str
    test_threads: int | None
    config: NextestConfig | None
    heavy_filter: str
    no_fail_fast: bool
    confirm: ConfirmRecording | None


def emit_pass(spec: PassSpec, runner: TestRunner) -> Step:
    """The one test pass `spec` describes, run as `runner` runs every pass.

    A recording runner wraps a nextest pass in the confirm recorder; the
    cargo-test fallback records nothing. The pass runs without the semaphore
    slot's descriptor.
    """
    release = bool(PROFILE_TABLE[spec.profile].cargo_release_flag)
    gate_path = None
    if runner.config is None:
        run = CargoTestRun(
            spec.selector, release, spec.features, spec.wall, runner.prio, runner.test_threads or 1
        )
    else:
        flags = NextestFlags(
            spec.heavy, runner.heavy_filter, runner.no_fail_fast, runner.test_threads, spec.retry_ids
        )
        run = NextestRun(
            spec.selector, release, spec.features, spec.wall, runner.prio, flags, runner.config.path
        )
        if runner.confirm is not None:
            paths = runner.confirm.paths(spec.profile)
            run = ConfirmRecorded(run, runner.confirm.junit, paths.manifest, paths.sidecar, spec.profile)
            gate_path = CONFIRM_RECORDER
    guard_file = None
    if spec.guard is not None:
        run = Guarded("-f", spec.guard.manifest, Chain((spec.guard.setup, run)))
        gate_path, guard_file = spec.guard.setup.path, spec.guard.manifest
    phase = _phase(spec, nextest=runner.config is not None)
    return Step(StepKind.CARGO, phase, SlotFdClosed(run), gate_path, guard_file)


def _phase(spec: PassSpec, nextest: bool) -> Phase:
    """The ledger phase: the gui-feature pass, else the profile's pass; run by nextest or cargo test."""
    if spec.features:
        return Phase.GUI_FEATURE_NEXTEST if nextest else Phase.GUI_FEATURE_CARGO_TEST
    if spec.profile is Profile.RELEASE:
        return Phase.RELEASE_NEXTEST if nextest else Phase.RELEASE_CARGO_TEST
    return Phase.DEBUG_NEXTEST if nextest else Phase.DEBUG_CARGO_TEST
