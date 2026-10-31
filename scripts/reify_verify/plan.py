"""plan — the typed verify plan: its steps, their kinds and phases, and its header (PRD D2).

Records only. build composes a Plan, and render is the one producer of its
text and JSON projections. A step's kind decides how the executor runs it
(StepKind.execution) and whether its render carries the loader-path scrub
(StepKind.scrubs); KIND_COMMANDS is the one statement of which command
records each kind may hold, checked whenever a Step is built.

The environment section's lines are records here too (EnvLine); environment
builds them, so it imports this module, never the other way round.

A record whose text render puts inside single quotes refuses a single quote,
since the rendered shell line would otherwise break.
"""

from dataclasses import dataclass
from enum import Enum
from types import MappingProxyType

from reify_verify.invocation import Action, Scope
from reify_verify.narrowing import Narrowing
from reify_verify.policy import HeavyPartition, Profile, ProfileSelection, Role
from reify_verify.scope import ScopeDecision


JOBSERVER_VAR = "CARGO_MAKEFLAGS"


@dataclass(frozen=True)
class CargoEnvSourced:
    path: str


@dataclass(frozen=True)
class CargoEnvMissing:
    pass


@dataclass(frozen=True)
class Exported:
    name: str
    value: str


class UnsetCause(Enum):
    OFFLINE = "offline"
    STALE = "stale"
    MISSING = "missing"


@dataclass(frozen=True)
class JobserverUnset:
    """JOBSERVER_VAR left unset; fifo is None only when the role has no jobserver pool."""

    cause: UnsetCause
    fifo: str | None


EnvLine = CargoEnvSourced | CargoEnvMissing | Exported | JobserverUnset


def _refuse_single_quote(record: str, field: str, text: str | None) -> None:
    if text is not None and "'" in text:
        raise ValueError(
            f"{record}: {field} renders inside single quotes, so it cannot hold one ({text!r})"
        )


class Execution(Enum):
    """How the executor runs a step: its render through bash, or by the step's own structure."""

    RENDER = "render"
    STRUCTURAL = "structural"


class StepKind(Enum):
    CARGO = "cargo"
    TOOL = "tool"
    BUILTIN = "builtin"
    MAIN_SHELL = "main_shell"
    SENTINEL = "sentinel"

    @property
    def scrubs(self) -> bool:
        """A TOOL step's render starts with the loader-path scrub; no other kind's does."""
        return self is StepKind.TOOL

    @property
    def execution(self) -> Execution:
        """The main shell's own steps (the node lane, the semaphore sentinels) are structural."""
        if self in (StepKind.MAIN_SHELL, StepKind.SENTINEL):
            return Execution.STRUCTURAL
        return Execution.RENDER


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


@dataclass(frozen=True)
class Script:
    """A repo script: `[K=V ...] [timeout <wall>] (bash <path>|./<path>) [args] [2>&1]`."""

    path: str
    args: tuple[str, ...] = ()
    wall: str | None = None
    via_bash: bool = False
    env: tuple[tuple[str, str], ...] = ()
    stderr_to_stdout: bool = False


@dataclass(frozen=True)
class Workspace:
    pass


@dataclass(frozen=True)
class Packages:
    names: tuple[str, ...]

    def __post_init__(self):
        if not self.names:
            raise ValueError("Packages: a package selector names at least one package")


Selector = Workspace | Packages


@dataclass(frozen=True)
class Cargo:
    """`timeout <wall> <prio>cargo <subcommand> [--release] <selector> <args> [2>&1]`."""

    subcommand: str
    selector: Selector
    args: tuple[str, ...]
    wall: str
    prio: str
    release: bool = False
    stderr_to_stdout: bool = False


@dataclass(frozen=True)
class Chain:
    """Commands joined by `&&`."""

    parts: tuple["Command", ...]


@dataclass(frozen=True)
class Guarded:
    """`if test <flag> <path>; then <body>; [else echo '<warning>'; ]fi [2>&1]`."""

    test_flag: str
    path: str
    body: "Command"
    else_warning: str | None = None
    stderr_to_stdout: bool = False

    def __post_init__(self):
        if self.test_flag not in ("-f", "-d"):
            raise ValueError(f"Guarded: test flag {self.test_flag!r} is not -f or -d")
        _refuse_single_quote("Guarded", "else_warning", self.else_warning)


@dataclass(frozen=True)
class NodeProject:
    """One node project of the gui lane, guarded on `guard_path` and run in `dir`."""

    guard_flag: str
    guard_path: str
    dir: str
    minutes: int
    commands: tuple[str, ...]

    def __post_init__(self):
        """Each command is one simple command: the tuple, not the text, says when it is a compound."""
        if not self.commands:
            raise ValueError(f"NodeProject: {self.dir} runs no command")
        for command in self.commands:
            if "&&" in command:
                raise ValueError(f"NodeProject: command {command!r} is a compound; list its parts")
            _refuse_single_quote("NodeProject", "a command", command)


@dataclass(frozen=True)
class NodeLaneStart:
    """The node lane started in the background of the main shell, overlapping clippy."""

    projects: tuple[NodeProject, ...]


@dataclass(frozen=True)
class NodeLaneJoin:
    pass


@dataclass(frozen=True)
class HeadStamp:
    """`git rev-parse HEAD > <path> 2>/dev/null || true`: records the commit a build was made from."""

    path: str


@dataclass(frozen=True)
class ArtifactAssertion:
    """`if test -f <manifest> && [ ! -f <artifact> ]; then echo '<error>' >&2; false; fi 2>&1`."""

    manifest: str
    artifact: str
    error: str

    def __post_init__(self):
        _refuse_single_quote("ArtifactAssertion", "error", self.error)


@dataclass(frozen=True)
class InfraLeafLoop:
    """One selective-infra leaf: every file `glob` matches, run in turn under `env` and the scrub.

    `scrub_prefix` is the `env -u …` text the git-env scrub seam prints;
    `unset` names the further variables the member runs without.
    """

    glob: str
    env: tuple[tuple[str, str], ...]
    wall: str
    scrub_prefix: str
    unset: tuple[str, ...]


Command = (
    Script
    | Cargo
    | Chain
    | Guarded
    | NodeProject
    | NodeLaneStart
    | NodeLaneJoin
    | HeadStamp
    | ArtifactAssertion
    | InfraLeafLoop
)

KIND_COMMANDS = MappingProxyType(
    {
        StepKind.CARGO: (Cargo, Guarded),
        StepKind.TOOL: (Script, Guarded, NodeProject, InfraLeafLoop),
        StepKind.BUILTIN: (ArtifactAssertion,),
        StepKind.MAIN_SHELL: (NodeLaneStart, NodeLaneJoin),
        StepKind.SENTINEL: (),
    }
)


@dataclass(frozen=True)
class Step:
    kind: StepKind
    phase: Phase
    command: Command
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
    scope: Scope
    include_infra: bool
    nextest: bool
    role: Role

    @property
    def profiles(self) -> tuple[Profile, ...]:
        return self.profile_selection.profiles


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
    environment: tuple[EnvLine, ...] = ()
    steps: tuple[Step, ...] = ()
