"""invocation — one verify run's request: parsed argv plus the role.

A record only; cli does the parsing. It sits below cli so that every later
module can take an Invocation without importing cli.
"""

from dataclasses import dataclass
from enum import Enum

from reify_verify.policy import ProfileSelection, Role


class Action(Enum):
    TEST = "test"
    LINT = "lint"
    TYPECHECK = "typecheck"
    ALL = "all"
    PSI_GATE = "psi-gate"
    COMPILE_GATE = "compile-gate"


class Scope(Enum):
    ALL = "all"
    STAGED = "staged"
    BRANCH = "branch"


class PlanFormat(Enum):
    """The plan projection a run prints instead of executing; its value is the flag."""

    TEXT = "--print-plan"
    JSON = "--print-plan=json"


class Query(Enum):
    """A report printed instead of running an action; its value is the flag."""

    KNOBS = "--print-knobs"
    ROLE_POLICY = "--print-role-policy"
    SCOPE = "--print-scope"


@dataclass(frozen=True)
class Invocation:
    action: Action | None
    profile_arg: ProfileSelection | None
    scope: Scope
    narrow: bool
    include_infra: bool
    plan_format: PlanFormat | None
    confirm_failed: bool
    test_threads: int | None
    role: Role
    query: Query | None
