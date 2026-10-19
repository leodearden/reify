"""knobs — every environment knob scripts/verify.sh itself reads (PRD D1).

Each Knob names the parser its bash read form applies: a ${X:-d} read is TEXT,
a ${X-d} read keeps an explicit empty value (TEXT_UNSET_ONLY), a
`= "1"` comparison is FLAG_ONE, a `-n` test is FLAG_SET, and
_resolve_timeout_knob is DURATION. A default computed where the knob is used
is a DerivedDefault, and an unset knob with one resolves to None.

Not here, because something else owns them: DF_VERIFY_ROLE (policy.parse_role
reads it); the knobs verify.sh's bash libs read (REIFY_TEST_SEMAPHORE_*,
REIFY_CLOCK_HEARTBEAT_SECS, the reaper knobs); and the values verify.sh sets
for its children rather than reads (REIFY_AMBIENT_LD_LIBRARY_PATH,
REIFY_RUN_ALL_TIMING_OUT, REIFY_HEAVY_NEXTEST_FILTER, REIFY_VERIFY_EVENT_LOG,
which it passes through or removes).
"""

import re
from collections import Counter
from collections.abc import Mapping
from dataclasses import dataclass
from enum import Enum


class Parser(Enum):
    DURATION = "duration"
    TEXT = "text"
    TEXT_UNSET_ONLY = "text-unset-only"
    FLAG_ONE = "flag-one"
    FLAG_SET = "flag-set"


_DURATION = re.compile(r"[0-9]+[smhd]?", re.ASCII)


def is_duration(text: str) -> bool:
    return _DURATION.fullmatch(text) is not None


@dataclass(frozen=True)
class DerivedDefault:
    description: str


@dataclass(frozen=True)
class Knob:
    name: str
    parser: Parser
    default: str | DerivedDefault
    meaning: str

    def __post_init__(self):
        if self.parser is Parser.DURATION and not (
            isinstance(self.default, str) and is_duration(self.default)
        ):
            raise ValueError(
                f"knob {self.name}: default {self.default!r} is not a duration"
            )


@dataclass(frozen=True)
class Resolution:
    value: str | bool | None
    warning: str | None = None


def resolve(knob: Knob, env: Mapping[str, str], default: str | None = None) -> Resolution:
    """The knob's value under `env`; `default` replaces the knob's own default."""
    fallback = knob.default if default is None else default
    if isinstance(fallback, DerivedDefault):
        fallback = None
    raw = env.get(knob.name)
    match knob.parser:
        case Parser.TEXT_UNSET_ONLY:
            return Resolution(fallback if raw is None else raw)
        case Parser.DURATION if raw and not is_duration(raw):
            return Resolution(
                fallback,
                f"verify.sh: WARNING: invalid {knob.name}={raw}; using default {fallback}",
            )
        case Parser.FLAG_ONE:
            return Resolution((raw or fallback) == "1")
        case Parser.FLAG_SET:
            return Resolution(bool(raw or fallback))
        case _:
            return Resolution(raw or fallback)


TEST_TIMEOUT = Knob(
    "REIFY_VERIFY_TEST_TIMEOUT", Parser.DURATION, "60m",
    "outer wall of the debug (--workspace) cargo nextest pass",
)
TEST_TIMEOUT_RELEASE = Knob(
    "REIFY_VERIFY_TEST_TIMEOUT_RELEASE", Parser.DURATION, "90m",
    "outer wall of the release cargo nextest pass; a role's release_wall_default replaces this default",
)
PREBUILD_TIMEOUT = Knob(
    "REIFY_VERIFY_PREBUILD_TIMEOUT", Parser.DURATION, "45m",
    "outer wall of each merge-path release pre-build (cargo build --release -p reify-audit / -p reify-cli)",
)
CLIPPY_TIMEOUT = Knob(
    "REIFY_VERIFY_CLIPPY_TIMEOUT", Parser.DURATION, "45m",
    "outer wall of cargo clippy and of the gui-feature cargo check pass",
)
GUI_FEATURE_TEST_TIMEOUT = Knob(
    "REIFY_VERIFY_GUI_FEATURE_TEST_TIMEOUT", Parser.DURATION, "45m",
    "outer wall of the gui-feature test pass (-p reify-gui --features gui), build plus execution",
)
CHECK_TIMEOUT = Knob(
    "REIFY_VERIFY_CHECK_TIMEOUT", Parser.DURATION, "30m",
    "outer wall of cargo check --workspace --tests",
)

PSI_GATE_THRESHOLD = Knob(
    "REIFY_PSI_GATE_THRESHOLD", Parser.TEXT, "50",
    "psi-gate CPU avg10 % ceiling; dispatch waits until pressure is below it",
)
PSI_GATE_WINDOW = Knob(
    "REIFY_PSI_GATE_WINDOW", Parser.TEXT, "20",
    "psi-gate minimum seconds between dispatches",
)
PSI_GATE_MAX_WAIT = Knob(
    "REIFY_PSI_GATE_MAX_WAIT", Parser.TEXT, "unlimited",
    "psi-gate give-up seconds, or unlimited for a continuous clock-stopped hold; a finite value exits 75",
)
PSI_GATE_POLL = Knob(
    "REIFY_PSI_GATE_POLL", Parser.TEXT, "5",
    "psi-gate recheck interval in seconds",
)
PSI_GATE_PROC_PATH = Knob(
    "REIFY_PSI_GATE_PROC_PATH", Parser.TEXT, "/proc/pressure/cpu",
    "psi-gate CPU PSI source (testability knob)",
)
PSI_GATE_DISPATCH_FILE = Knob(
    "REIFY_PSI_GATE_DISPATCH_FILE", Parser.TEXT, "/tmp/reify-verify-last-dispatch",
    "psi-gate shared dispatch timestamp file",
)
PSI_GATE_DISABLE = Knob(
    "REIFY_PSI_GATE_DISABLE", Parser.TEXT, "",
    "psi-gate break-glass: 1 bypasses the gate entirely",
)
PSI_GATE_MEM_PROC_PATH = Knob(
    "REIFY_PSI_GATE_MEM_PROC_PATH", Parser.TEXT, "/proc/pressure/memory",
    "psi-gate memory PSI source (testability knob)",
)
PSI_GATE_MEM_FULL_THRESHOLD = Knob(
    "REIFY_PSI_GATE_MEM_FULL_THRESHOLD", Parser.TEXT_UNSET_ONLY, "10",
    "psi-gate memfull avg10 % ceiling; an explicit empty value turns the memory dimension off",
)
PSI_GATE_MEM_SOME_THRESHOLD = Knob(
    "REIFY_PSI_GATE_MEM_SOME_THRESHOLD", Parser.TEXT, "",
    "psi-gate memsome avg10 % ceiling; empty is off",
)

COMPILE_GATE_THRESHOLD = Knob(
    "REIFY_COMPILE_GATE_THRESHOLD", Parser.TEXT, "85",
    "compile-gate CPU avg10 % ceiling for compile admission",
)
COMPILE_GATE_MAX_WAIT = Knob(
    "REIFY_COMPILE_GATE_MAX_WAIT", Parser.TEXT, "300",
    "compile-gate fallback wait, inoperative for the hold (task 4920)",
)
COMPILE_GATE_POLL = Knob(
    "REIFY_COMPILE_GATE_POLL", Parser.TEXT, "5",
    "compile-gate recheck interval in seconds",
)
COMPILE_GATE_PROC_PATH = Knob(
    "REIFY_COMPILE_GATE_PROC_PATH", Parser.TEXT, "/proc/pressure/cpu",
    "compile-gate CPU PSI source (testability knob)",
)
COMPILE_GATE_DISABLE = Knob(
    "REIFY_COMPILE_GATE_DISABLE", Parser.TEXT, "",
    "compile-gate break-glass: 1 bypasses the gate entirely",
)
COMPILE_GATE_MEM_PROC_PATH = Knob(
    "REIFY_COMPILE_GATE_MEM_PROC_PATH", Parser.TEXT, "/proc/pressure/memory",
    "compile-gate memory PSI source (testability knob)",
)
COMPILE_GATE_MEM_FULL_THRESHOLD = Knob(
    "REIFY_COMPILE_GATE_MEM_FULL_THRESHOLD", Parser.TEXT_UNSET_ONLY, "10",
    "compile-gate memfull avg10 % ceiling; an explicit empty value turns the memory dimension off",
)
COMPILE_GATE_MEM_SOME_THRESHOLD = Knob(
    "REIFY_COMPILE_GATE_MEM_SOME_THRESHOLD", Parser.TEXT, "",
    "compile-gate memsome avg10 % ceiling; empty is off",
)

GATE_EXCLUDE_HEAVY = Knob(
    "REIFY_GATE_EXCLUDE_HEAVY", Parser.FLAG_ONE, "",
    "exactly 1: task and merge nextest passes exclude the heavy filterset",
)
RELEASE_DELTA_SKIP = Knob(
    "REIFY_RELEASE_DELTA_SKIP", Parser.FLAG_ONE, "0",
    "exactly 1: a merge may skip its release pass when the delta cannot change release behaviour",
)

JOBSERVER_TASK_FIFO = Knob(
    "REIFY_JOBSERVER_TASK_FIFO", Parser.TEXT, "/tmp/reify-jobserver-task",
    "jobserver FIFO for the task pool (task and background roles)",
)
JOBSERVER_MERGE_FIFO = Knob(
    "REIFY_JOBSERVER_MERGE_FIFO", Parser.TEXT, "/tmp/reify-jobserver-merge",
    "jobserver FIFO for the merge pool",
)
JOBSERVER_SKIP_LIVENESS_PROBE = Knob(
    "REIFY_JOBSERVER_SKIP_LIVENESS_PROBE", Parser.FLAG_ONE, "",
    "exactly 1: adopt an existing FIFO without probing its owner (break-glass)",
)

NEXTEST_PROBE_RETRY_SLEEP = Knob(
    "REIFY_NEXTEST_PROBE_RETRY_SLEEP", Parser.TEXT, "2",
    "seconds between cargo nextest --version availability probe retries",
)

RETRY_SCOPE = Knob(
    "REIFY_VERIFY_RETRY_SCOPE", Parser.TEXT, "",
    "failed_only narrows a merge retry to the did-not-pass tests; anything else runs everything",
)
RETRY_NEXTEST_FILTER_FILE = Knob(
    "REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE", Parser.TEXT, "",
    "newline-delimited exact nextest test ids a failed_only retry runs",
)
RETRY_NEXTEST_FILTER_FILE_DEBUG = Knob(
    "REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE_DEBUG", Parser.TEXT,
    DerivedDefault("REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE"),
    "the debug pass's own retry filter file",
)
RETRY_NEXTEST_FILTER_FILE_RELEASE = Knob(
    "REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE_RELEASE", Parser.TEXT,
    DerivedDefault("REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE"),
    "the release pass's own retry filter file",
)
RETRY_TREE_OID = Knob(
    "REIFY_VERIFY_RETRY_TREE_OID", Parser.TEXT, "",
    "the tree a retry targets; must equal the attempt-0 sidecar's tree_oid",
)
RETRY_MAX_SUBSET = Knob(
    "REIFY_VERIFY_RETRY_MAX_SUBSET", Parser.TEXT, "5000",
    "largest retry subset accepted; a larger one is refused as a construction bug",
)
ATTEMPT_SIDECAR = Knob(
    "REIFY_VERIFY_ATTEMPT_SIDECAR", Parser.TEXT, "target/reify-verify-attempt.json",
    "attempt-0 sidecar recording the tree a merge built from",
)

CONFIRM_MANIFEST_DEBUG = Knob(
    "REIFY_VERIFY_CONFIRM_MANIFEST_DEBUG", Parser.TEXT, "target/reify-confirm-failed-debug.txt",
    "failing debug test ids a recording run wrote, for --confirm-failed",
)
CONFIRM_MANIFEST_RELEASE = Knob(
    "REIFY_VERIFY_CONFIRM_MANIFEST_RELEASE", Parser.TEXT, "target/reify-confirm-failed-release.txt",
    "failing release test ids a recording run wrote, for --confirm-failed",
)
CONFIRM_SIDECAR_DEBUG = Knob(
    "REIFY_VERIFY_CONFIRM_SIDECAR_DEBUG", Parser.TEXT, "target/reify-confirm-failed-debug.json",
    "tree OID the debug manifest was recorded against",
)
CONFIRM_SIDECAR_RELEASE = Knob(
    "REIFY_VERIFY_CONFIRM_SIDECAR_RELEASE", Parser.TEXT, "target/reify-confirm-failed-release.json",
    "tree OID the release manifest was recorded against",
)
CONFIRM_JUNIT = Knob(
    "REIFY_VERIFY_CONFIRM_JUNIT", Parser.TEXT, "target/nextest/default/reify-confirm.xml",
    "JUnit report nextest writes for the recording and confirm passes",
)
CONFIRM_LOG = Knob(
    "REIFY_VERIFY_CONFIRM_LOG", Parser.TEXT,
    DerivedDefault("<repo root>/target/reify-confirm-failed.log"),
    "private log that takes every --confirm-failed diagnostic",
)
NEXTEST_CONFIG = Knob(
    "REIFY_VERIFY_NEXTEST_CONFIG", Parser.TEXT, "",
    "nextest config file to use instead of generating one",
)

AFFECTED_CRATES_OVERRIDE = Knob(
    "REIFY_AFFECTED_CRATES_OVERRIDE", Parser.TEXT, "",
    "crate list used verbatim as the staged/branch affected-crate closure",
)
GUI_RETRY_SPECS = Knob(
    "REIFY_GUI_RETRY_SPECS", Parser.TEXT, "",
    "vitest spec subset a retry re-runs; forces the gui vitest lane on",
)
INFRA_SUITE_ACTIVE = Knob(
    "REIFY_INFRA_SUITE_ACTIVE", Parser.FLAG_SET, "",
    "non-empty when verify runs nested in an infra suite; the plan then omits the infra pool steps",
)
RUN_ALL_MEMBER_SUBSET = Knob(
    "REIFY_RUN_ALL_MEMBER_SUBSET", Parser.TEXT, "",
    "run_all.sh members a retry re-runs",
)

BACKGROUND_SWEEP_LEDGER = Knob(
    "REIFY_BACKGROUND_SWEEP_LEDGER", Parser.TEXT,
    DerivedDefault("<main checkout>/data/orchestrator/verify-background-sweeps.jsonl"),
    "where a completed background run appends its verdict record",
)
LEDGER_DIR = Knob(
    "REIFY_VERIFY_LEDGER_DIR", Parser.TEXT,
    DerivedDefault("<main checkout>/data/verify-logs, refused beneath an infra suite"),
    "where a top-level executed run appends its instrumentation record",
)
LEDGER_LANE_DIR = Knob(
    "REIFY_VERIFY_LEDGER_LANE_DIR", Parser.TEXT, DerivedDefault("<repo root>"),
    "the lane whose seed stamp the instrumentation record reads",
)
RUN_ID = Knob(
    "REIFY_VERIFY_RUN_ID", Parser.TEXT, "",
    "a recording run's id; a run that inherits one is nested and records nothing",
)

KNOBS: tuple[Knob, ...] = (
    TEST_TIMEOUT,
    TEST_TIMEOUT_RELEASE,
    PREBUILD_TIMEOUT,
    CLIPPY_TIMEOUT,
    GUI_FEATURE_TEST_TIMEOUT,
    CHECK_TIMEOUT,
    PSI_GATE_THRESHOLD,
    PSI_GATE_WINDOW,
    PSI_GATE_MAX_WAIT,
    PSI_GATE_POLL,
    PSI_GATE_PROC_PATH,
    PSI_GATE_DISPATCH_FILE,
    PSI_GATE_DISABLE,
    PSI_GATE_MEM_PROC_PATH,
    PSI_GATE_MEM_FULL_THRESHOLD,
    PSI_GATE_MEM_SOME_THRESHOLD,
    COMPILE_GATE_THRESHOLD,
    COMPILE_GATE_MAX_WAIT,
    COMPILE_GATE_POLL,
    COMPILE_GATE_PROC_PATH,
    COMPILE_GATE_DISABLE,
    COMPILE_GATE_MEM_PROC_PATH,
    COMPILE_GATE_MEM_FULL_THRESHOLD,
    COMPILE_GATE_MEM_SOME_THRESHOLD,
    GATE_EXCLUDE_HEAVY,
    RELEASE_DELTA_SKIP,
    JOBSERVER_TASK_FIFO,
    JOBSERVER_MERGE_FIFO,
    JOBSERVER_SKIP_LIVENESS_PROBE,
    NEXTEST_PROBE_RETRY_SLEEP,
    RETRY_SCOPE,
    RETRY_NEXTEST_FILTER_FILE,
    RETRY_NEXTEST_FILTER_FILE_DEBUG,
    RETRY_NEXTEST_FILTER_FILE_RELEASE,
    RETRY_TREE_OID,
    RETRY_MAX_SUBSET,
    ATTEMPT_SIDECAR,
    CONFIRM_MANIFEST_DEBUG,
    CONFIRM_MANIFEST_RELEASE,
    CONFIRM_SIDECAR_DEBUG,
    CONFIRM_SIDECAR_RELEASE,
    CONFIRM_JUNIT,
    CONFIRM_LOG,
    NEXTEST_CONFIG,
    AFFECTED_CRATES_OVERRIDE,
    GUI_RETRY_SPECS,
    INFRA_SUITE_ACTIVE,
    RUN_ALL_MEMBER_SUBSET,
    BACKGROUND_SWEEP_LEDGER,
    LEDGER_DIR,
    LEDGER_LANE_DIR,
    RUN_ID,
)


def _check_unique_names(knobs: tuple[Knob, ...]) -> None:
    counts = Counter(knob.name for knob in knobs)
    duplicates = sorted(name for name, count in counts.items() if count > 1)
    if duplicates:
        raise ValueError(f"duplicate knob names in KNOBS: {', '.join(duplicates)}")


_check_unique_names(KNOBS)
