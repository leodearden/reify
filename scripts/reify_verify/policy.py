"""policy — the verify roles and profiles, one table each (PRD D3, D4).

A Role is parsed once from DF_VERIFY_ROLE. Every consumer reads one named field
of the role's RolePolicy, or of a Profile's ProfilePolicy, instead of comparing
the role or profile name. The resolution functions at the end are pure: they
take the env and host facts they need as arguments.
"""

from collections.abc import Mapping
from dataclasses import dataclass
from enum import Enum
from types import MappingProxyType

from reify_verify import knobs


class Role(Enum):
    TASK = "task"
    MERGE = "merge"
    OFFLINE = "offline"
    BACKGROUND = "background"


class UnknownRoleError(ValueError):
    def __init__(self, raw: str):
        wanted = "|".join(role.value for role in Role)
        super().__init__(
            f"verify.sh: ERROR — unknown DF_VERIFY_ROLE '{raw}' (want {wanted})"
        )


def parse_role(raw: str | None) -> Role:
    """None or "" is task, as bash's ${DF_VERIFY_ROLE:-task}."""
    if not raw:
        return Role.TASK
    try:
        return Role(raw)
    except ValueError:
        raise UnknownRoleError(raw) from None


class Profile(Enum):
    DEBUG = "debug"
    RELEASE = "release"


class ProfileSelection(Enum):
    """The --profile argument, and a role's default_profile."""

    DEBUG = "debug"
    RELEASE = "release"
    BOTH = "both"

    @property
    def profiles(self) -> tuple[Profile, ...]:
        """The profiles to test, debug before release."""
        if self is ProfileSelection.BOTH:
            return (Profile.DEBUG, Profile.RELEASE)
        return (Profile(self.value),)


class JobserverPool(Enum):
    TASK = "task"
    MERGE = "merge"
    NONE = "none"


class HeavyFilter(Enum):
    """What the nextest passes do with the heavy filterset."""

    EXCLUDE_ELIGIBLE = "exclude-eligible"
    ONLY = "only"
    NONE = "none"


class ReleaseSelector(Enum):
    RELEASE_SENSITIVE = "release-sensitive"
    WORKSPACE = "workspace"


class InfraTier(Enum):
    SELECTIVE = "selective"
    WHOLESALE = "wholesale"


@dataclass(frozen=True)
class PrioClass:
    """A CARGO_PRIO prefix, and its form on a host without ionice."""

    name: str
    prefix: str
    nice_only_prefix: str
    uses_ionice: bool


TASK_PRIO = PrioClass("task", "nice -n 15 ionice -c 2 -n 7 ", "nice -n 15 ", True)
MERGE_PRIO = PrioClass("merge", "nice -n 5 ", "nice -n 5 ", False)
IDLE_PRIO = PrioClass("idle", "nice -n 19 ionice -c3 ", "nice -n 19 ", True)


@dataclass(frozen=True)
class RolePolicy:
    default_profile: ProfileSelection
    prio_class: PrioClass
    force_scope_all: bool
    jobserver: JobserverPool
    heavy: HeavyFilter
    fail_fast: bool
    records_confirm_manifest: bool
    release_wall_default: str
    release_selector: ReleaseSelector
    infra_tier: InfraTier
    attempt_sidecar: bool
    release_delta_skip_eligible: bool
    sweep_ledger: bool
    gui_feature_test_pass: bool
    admission_exempt: bool

    def __post_init__(self):
        if not knobs.is_duration(self.release_wall_default):
            raise ValueError(
                f"RolePolicy.release_wall_default {self.release_wall_default!r} "
                "is not a duration"
            )


@dataclass(frozen=True)
class ProfilePolicy:
    cargo_release_flag: str
    wall_knob: knobs.Knob
    narrowable: bool
    confirm_manifest: knobs.Knob
    confirm_sidecar: knobs.Knob
    retry_filter_env: knobs.Knob


ROLE_TABLE: Mapping[Role, RolePolicy] = MappingProxyType(
    {
        Role.TASK: RolePolicy(
            default_profile=ProfileSelection.DEBUG,
            prio_class=TASK_PRIO,
            force_scope_all=False,
            jobserver=JobserverPool.TASK,
            heavy=HeavyFilter.EXCLUDE_ELIGIBLE,
            fail_fast=True,
            records_confirm_manifest=False,
            release_wall_default=knobs.TEST_TIMEOUT_RELEASE.default,
            release_selector=ReleaseSelector.RELEASE_SENSITIVE,
            infra_tier=InfraTier.SELECTIVE,
            attempt_sidecar=False,
            release_delta_skip_eligible=False,
            sweep_ledger=False,
            gui_feature_test_pass=True,
            admission_exempt=False,
        ),
        Role.MERGE: RolePolicy(
            default_profile=ProfileSelection.BOTH,
            prio_class=MERGE_PRIO,
            force_scope_all=True,
            jobserver=JobserverPool.MERGE,
            heavy=HeavyFilter.EXCLUDE_ELIGIBLE,
            fail_fast=True,
            records_confirm_manifest=False,
            release_wall_default=knobs.TEST_TIMEOUT_RELEASE.default,
            release_selector=ReleaseSelector.RELEASE_SENSITIVE,
            infra_tier=InfraTier.WHOLESALE,
            attempt_sidecar=True,
            release_delta_skip_eligible=True,
            sweep_ledger=False,
            gui_feature_test_pass=True,
            admission_exempt=True,
        ),
        Role.OFFLINE: RolePolicy(
            default_profile=ProfileSelection.RELEASE,
            prio_class=IDLE_PRIO,
            force_scope_all=False,
            jobserver=JobserverPool.NONE,
            heavy=HeavyFilter.ONLY,
            fail_fast=False,
            records_confirm_manifest=True,
            release_wall_default="13h",
            release_selector=ReleaseSelector.WORKSPACE,
            infra_tier=InfraTier.SELECTIVE,
            attempt_sidecar=False,
            release_delta_skip_eligible=False,
            sweep_ledger=False,
            gui_feature_test_pass=False,
            admission_exempt=False,
        ),
        Role.BACKGROUND: RolePolicy(
            default_profile=ProfileSelection.BOTH,
            prio_class=IDLE_PRIO,
            force_scope_all=True,
            jobserver=JobserverPool.TASK,
            heavy=HeavyFilter.NONE,
            fail_fast=True,
            records_confirm_manifest=False,
            release_wall_default=knobs.TEST_TIMEOUT_RELEASE.default,
            release_selector=ReleaseSelector.RELEASE_SENSITIVE,
            infra_tier=InfraTier.WHOLESALE,
            attempt_sidecar=False,
            release_delta_skip_eligible=False,
            sweep_ledger=True,
            gui_feature_test_pass=True,
            admission_exempt=False,
        ),
    }
)

PROFILE_TABLE: Mapping[Profile, ProfilePolicy] = MappingProxyType(
    {
        Profile.DEBUG: ProfilePolicy(
            cargo_release_flag="",
            wall_knob=knobs.TEST_TIMEOUT,
            narrowable=True,
            confirm_manifest=knobs.CONFIRM_MANIFEST_DEBUG,
            confirm_sidecar=knobs.CONFIRM_SIDECAR_DEBUG,
            retry_filter_env=knobs.RETRY_NEXTEST_FILTER_FILE_DEBUG,
        ),
        Profile.RELEASE: ProfilePolicy(
            cargo_release_flag="--release",
            wall_knob=knobs.TEST_TIMEOUT_RELEASE,
            narrowable=False,
            confirm_manifest=knobs.CONFIRM_MANIFEST_RELEASE,
            confirm_sidecar=knobs.CONFIRM_SIDECAR_RELEASE,
            retry_filter_env=knobs.RETRY_NEXTEST_FILTER_FILE_RELEASE,
        ),
    }
)


def _check_keys(table: Mapping, enum: type[Enum], name: str) -> None:
    missing = sorted(member.value for member in set(enum) - set(table))
    extra = sorted(str(key) for key in set(table) - set(enum))
    if missing or extra:
        raise ValueError(f"{name} keys differ from {enum.__name__}: missing {missing}, extra {extra}")


_check_keys(ROLE_TABLE, Role, "ROLE_TABLE")
_check_keys(PROFILE_TABLE, Profile, "PROFILE_TABLE")


def test_wall(role_policy: RolePolicy, profile: Profile, env: Mapping[str, str]) -> knobs.Resolution:
    """A test pass's outer wall; the role's release_wall_default replaces the release default."""
    default = role_policy.release_wall_default if profile is Profile.RELEASE else None
    return knobs.resolve(PROFILE_TABLE[profile].wall_knob, env, default)


@dataclass(frozen=True)
class PrioResolution:
    prefix: str
    warning: str | None


def resolve_prio(
    role_policy: RolePolicy, role_label: str, has_nice: bool, has_ionice: bool
) -> PrioResolution:
    """CARGO_PRIO with or without nice and ionice; `role_label` names the role in a warning."""
    prio = role_policy.prio_class
    if has_nice and (has_ionice or not prio.uses_ionice):
        return PrioResolution(prio.prefix, None)
    if has_nice:
        return PrioResolution(
            prio.nice_only_prefix,
            f"verify.sh: WARNING — ionice not found; {role_label} role using nice only (no IO throttle)",
        )
    missing = "nice/ionice" if prio.uses_ionice else "nice"
    return PrioResolution(
        "", f"verify.sh: WARNING — {missing} not found; {role_label} role running at normal priority"
    )


JOBSERVER_FIFO_KNOB: Mapping[JobserverPool, knobs.Knob] = MappingProxyType(
    {
        JobserverPool.TASK: knobs.JOBSERVER_TASK_FIFO,
        JobserverPool.MERGE: knobs.JOBSERVER_MERGE_FIFO,
    }
)


def jobserver_fifo(role_policy: RolePolicy, env: Mapping[str, str]) -> str | None:
    """The FIFO the role's jobserver pool names; whether it exists is a runtime question."""
    knob = JOBSERVER_FIFO_KNOB.get(role_policy.jobserver)
    return None if knob is None else knobs.resolve(knob, env).value


class HeavyPartition(Enum):
    """Which heavy tests the nextest passes receive."""

    INCLUDED = "included"
    EXCLUDED = "excluded"
    ONLY = "only"


def heavy_partition(role_policy: RolePolicy, env: Mapping[str, str]) -> HeavyPartition:
    if role_policy.heavy is HeavyFilter.ONLY:
        return HeavyPartition.ONLY
    if (
        role_policy.heavy is HeavyFilter.EXCLUDE_ELIGIBLE
        and knobs.resolve(knobs.GATE_EXCLUDE_HEAVY, env).value
    ):
        return HeavyPartition.EXCLUDED
    return HeavyPartition.INCLUDED
