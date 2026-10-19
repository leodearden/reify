"""policy — the verify roles and profiles, one table each (PRD D3, D4).

A Role is parsed once from DF_VERIFY_ROLE. Every consumer reads one named field
of the role's RolePolicy, or of a Profile's ProfilePolicy, instead of comparing
the role or profile name.
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
