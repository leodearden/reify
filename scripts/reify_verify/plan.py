"""plan — the typed verify plan: its steps, their kinds and phases, and its header (PRD D2).

Records only. build composes a Plan, and render is the one producer of its
text and JSON projections. A step's kind decides how the executor runs it and
whether its render carries the loader-path scrub; KIND_COMMANDS is the one
statement of which command records each kind may hold, checked whenever a
Step is built.
"""

from dataclasses import dataclass
from enum import Enum
from types import MappingProxyType

from reify_verify.invocation import Action, Scope
from reify_verify.narrowing import Narrowing
from reify_verify.policy import HeavyPartition, Profile, ProfileSelection, Role
from reify_verify.scope import ScopeDecision


class StepKind(Enum):
    CARGO = "cargo"
    TOOL = "tool"
    BUILTIN = "builtin"
    MAIN_SHELL = "main_shell"
    SENTINEL = "sentinel"


class Phase(Enum):
    """A step's ledger phase; the values are scripts/verify-ledger.py's Phase values, verbatim."""

    TEST_SLOT_ACQUIRE = "test_slot_acquire"
    TEST_SLOT_RELEASE = "test_slot_release"
    RELEASE_SKIP_MARKER = "release_skip_marker"
    INFRA_POOL = "infra_pool"
    INFRA_SELECTIVE = "infra_selective"
    INFRA_STATIC = "infra_static"
    GUI_FEATURE_NEXTEST = "gui_feature_nextest"
    RELEASE_NEXTEST = "release_nextest"
    DEBUG_NEXTEST = "debug_nextest"
    GUI_FEATURE_CARGO_TEST = "gui_feature_cargo_test"
    RELEASE_CARGO_TEST = "release_cargo_test"
    DEBUG_CARGO_TEST = "debug_cargo_test"
    CLIPPY = "clippy"
    GUI_FEATURE_CHECK = "gui_feature_check"
    CARGO_CHECK = "cargo_check"
    RELEASE_PREBUILD = "release_prebuild"
    CARGO_OTHER = "cargo_other"
    PSI_GATE = "psi_gate"
    COMPILE_GATE = "compile_gate"
    GUI_NODE_LAUNCH = "gui_node_launch"
    GUI_NODE_JOIN = "gui_node_join"
    GUI_NODE = "gui_node"
    POSTFLIGHT = "postflight"
    BOOKKEEPING = "bookkeeping"
    PREFLIGHT = "preflight"
    OTHER = "other"


KIND_COMMANDS = MappingProxyType({kind: () for kind in StepKind})


@dataclass(frozen=True)
class Step:
    kind: StepKind
    phase: Phase
    command: object
    gate_path: str | None
    guard_file: str | None
    annotation: tuple[str, ...] = ()

    def __post_init__(self):
        allowed = KIND_COMMANDS[self.kind]
        if not isinstance(self.command, allowed):
            names = ", ".join(sorted(t.__name__ for t in allowed)) or "none"
            raise ValueError(
                f"Step: a {self.kind.value} step cannot hold a "
                f"{type(self.command).__name__} command (allowed: {names})"
            )


@dataclass(frozen=True)
class PlanHeader:
    action: Action
    profile_selection: ProfileSelection
    profiles: tuple[Profile, ...]
    scope: Scope
    include_infra: bool
    nextest: bool
    role: Role


@dataclass(frozen=True)
class RetryMarker:
    """The failed_only retry's applied subset counts, printed before the header."""

    nextest_debug: int
    nextest_release: int
    run_all: int
    gui: int


@dataclass(frozen=True)
class Plan:
    retry_marker: RetryMarker | None
    header: PlanHeader
    selective_infra_note: bool
    scope: ScopeDecision
    narrowing: Narrowing
    heavy_partition: HeavyPartition | None
    environment: tuple = ()
    steps: tuple[Step, ...] = ()
