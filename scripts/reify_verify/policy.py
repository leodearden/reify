"""policy — the verify roles and profile selections (PRD D3, D4).

A Role is parsed once from DF_VERIFY_ROLE; every consumer reads the role's
policy rather than comparing the role name.
"""

from enum import Enum


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


class ProfileSelection(Enum):
    """The --profile argument, and a role's default_profile."""

    DEBUG = "debug"
    RELEASE = "release"
    BOTH = "both"
