"""environment — the process environment the plan's steps and planning subprocesses run under (PRD D9).

build ports scripts/verify.sh's apply_env and _jobserver_owner_live. It
returns a frozen StepEnvironment holding two envs, never touching os.environ:

- planning: every planning subprocess after this stage runs under it. It is
  the ambient env with DF_VERIFY_ROLE normalised, the ~/.cargo/env delta,
  RUSTC_WRAPPER, CARGO_INCREMENTAL, the jobserver's CARGO_MAKEFLAGS when the
  FIFO is adopted, and REIFY_AMBIENT_LD_LIBRARY_PATH. It carries no plan
  loader path: bash exports that only at the phase boundary.
- steps: planning plus the plan LD_LIBRARY_PATH, without BASH_ENV and ENV,
  which a fresh non-interactive bash per step would otherwise read.

The plan loader path comes from lib_native_ld_path.sh's cargo-target class;
no loader directory is named here (PRD D9, D15).

nextest_probe asks, under the planning env, whether cargo-nextest runs, as
bash's availability probe does before it plans.
"""

import os
import re
import shutil
import stat
from collections.abc import Mapping
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from types import MappingProxyType

from reify_verify import bashcall, childproc, knobs
from reify_verify.bashcall import BashCall, SourcedFile
from reify_verify.childproc import CallResult
from reify_verify.policy import Role, RolePolicy, jobserver_fifo

CARGO_ENV = SourcedFile(".cargo/env")
LOADER_DIRS = BashCall("scripts/lib_native_ld_path.sh", "native_ld_path_dirs")
LOADER_CLASS = "cargo-target"
BOOT_ID = Path("/proc/sys/kernel/random/boot_id")
PROC = Path("/proc")
UNKNOWN_BOOT = "-"
STEP_DROPPED_ENV = ("BASH_ENV", "ENV")
NEXTEST_PROBE = ("cargo", "nextest", "--version")
NEXTEST_PROBE_RETRIES = 3
SIGNALLED = 128
_DIGITS = re.compile(r"[0-9]+", re.ASCII)
_IFS_WHITESPACE = " \t\n"


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
    """CARGO_MAKEFLAGS left unset; fifo is None only when the role has no jobserver pool."""

    cause: UnsetCause
    fifo: str | None


EnvLine = CargoEnvSourced | CargoEnvMissing | Exported | JobserverUnset


@dataclass(frozen=True)
class StepEnvironment:
    planning: Mapping[str, str]
    steps: Mapping[str, str]
    lines: tuple[EnvLine, ...]
    warnings: tuple[str, ...]


class Liveness(Enum):
    LIVE = "live"
    STALE = "stale"
    UNKNOWN = "unknown"


@dataclass(frozen=True)
class OwnerProbe:
    liveness: Liveness
    pid: str | None = None


def build(
    role: Role, role_policy: RolePolicy, env: Mapping[str, str], repo_root: Path
) -> StepEnvironment:
    planning = {**env, "DF_VERIFY_ROLE": role.value}
    lines: list[EnvLine] = []
    warnings: list[str] = []
    cargo_env = CARGO_ENV.path(planning)
    if os.path.isfile(cargo_env):
        planning = bashcall.source_delta(CARGO_ENV, repo_root, planning).applied_to(planning)
        lines.append(CargoEnvSourced(cargo_env))
    else:
        lines.append(CargoEnvMissing())
    for name, value in (("RUSTC_WRAPPER", "sccache"), ("CARGO_INCREMENTAL", "0")):
        planning[name] = value
        lines.append(Exported(name, value))
    jobserver_line, warning = _jobserver(role_policy, planning)
    if isinstance(jobserver_line, Exported):
        planning[jobserver_line.name] = jobserver_line.value
    lines.append(jobserver_line)
    warnings.extend(filter(None, (warning,)))
    ambient = planning.get("LD_LIBRARY_PATH", "")
    planning["REIFY_AMBIENT_LD_LIBRARY_PATH"] = ambient
    lines.append(Exported("REIFY_AMBIENT_LD_LIBRARY_PATH", ambient))
    plan_loader_path = _plan_loader_path(ambient, repo_root, planning)
    lines.append(Exported("LD_LIBRARY_PATH", plan_loader_path))
    steps = {
        name: value for name, value in planning.items() if name not in STEP_DROPPED_ENV
    }
    steps["LD_LIBRARY_PATH"] = plan_loader_path
    return StepEnvironment(
        MappingProxyType(planning), MappingProxyType(steps), tuple(lines), tuple(warnings)
    )


def _jobserver(role_policy: RolePolicy, env: Mapping[str, str]) -> tuple[EnvLine, str | None]:
    """The role's FIFO adopted iff it is a FIFO whose owner stamp is not provably stale."""
    fifo = jobserver_fifo(role_policy, env)
    if fifo is None:
        return JobserverUnset(UnsetCause.OFFLINE, None), None
    if not _is_fifo(fifo):
        return JobserverUnset(UnsetCause.MISSING, fifo), None
    probe = OwnerProbe(Liveness.UNKNOWN)
    if not knobs.resolve(knobs.JOBSERVER_SKIP_LIVENESS_PROBE, env).value:
        probe = owner_liveness(fifo)
    if probe.liveness is Liveness.STALE:
        warning = (
            f"verify.sh: WARNING — stale jobserver FIFO {fifo} (owner pid {probe.pid or '?'} "
            "not alive / boot mismatch; balancer appears down) — falling back to plain cargo -j"
        )
        return JobserverUnset(UnsetCause.STALE, fifo), warning
    return Exported("CARGO_MAKEFLAGS", f"--jobserver-auth=fifo:{fifo}"), None


def _is_fifo(path: str) -> bool:
    """bash's `[ -p ]`: a FIFO after symlinks; False whenever that cannot be told."""
    try:
        return stat.S_ISFIFO(os.stat(path).st_mode)
    except (OSError, ValueError):
        return False


def owner_liveness(fifo: str) -> OwnerProbe:
    """The balancer's `<fifo>.owner` stamp, `<pid> <boot_id>`, read as bash's `read -r pid boot`.

    STALE needs proof: the pid is not alive, or the stamp's boot_id differs
    from a readable host boot_id. A missing, unreadable, unterminated or
    malformed stamp is UNKNOWN, which adopts the FIFO as LIVE does.
    """
    stamp = f"{fifo}.owner"
    if not os.path.exists(stamp):
        return OwnerProbe(Liveness.UNKNOWN)
    first_line = _first_terminated_line(stamp)
    if first_line is None:
        return OwnerProbe(Liveness.UNKNOWN)
    pid, boot = _read_two_fields(first_line)
    if not _DIGITS.fullmatch(pid):
        return OwnerProbe(Liveness.UNKNOWN)
    if not (PROC / pid).is_dir():
        return OwnerProbe(Liveness.STALE, pid)
    host_boot = _host_boot_id()
    if boot and boot != UNKNOWN_BOOT and host_boot and boot != host_boot:
        return OwnerProbe(Liveness.STALE, pid)
    return OwnerProbe(Liveness.LIVE)


def _first_terminated_line(path: str) -> str | None:
    """The first line, when a newline ends it: bash's `read` fails on an unterminated one."""
    try:
        with open(path, "rb") as handle:
            line = handle.readline()
    except OSError:
        return None
    if not line.endswith(b"\n"):
        return None
    return line[:-1].replace(b"\0", b"").decode("utf-8", "surrogateescape")


def _read_two_fields(line: str) -> tuple[str, str]:
    """`read -r a b` under the default IFS: the first field, then the trimmed rest."""
    stripped = line.strip(_IFS_WHITESPACE)
    for index, char in enumerate(stripped):
        if char in _IFS_WHITESPACE:
            return stripped[:index], stripped[index:].strip(_IFS_WHITESPACE)
    return stripped, ""


def _host_boot_id() -> str:
    """bash's `$(cat boot_id || true)`: the text without trailing newlines, or ""."""
    try:
        return BOOT_ID.read_text(encoding="utf-8", errors="surrogateescape").rstrip("\n")
    except OSError:
        return ""


def _plan_loader_path(ambient: str, repo_root: Path, planning: Mapping[str, str]) -> str:
    dirs = bashcall.run(LOADER_DIRS, (LOADER_CLASS,), repo_root, planning).lines
    return ":".join((*dirs, *((ambient,) if ambient else ())))


@dataclass(frozen=True)
class NextestAvailable:
    pass


@dataclass(frozen=True)
class NextestAbsent:
    """cargo nextest fails and no cargo-nextest is on PATH: the plan falls back to cargo test."""


@dataclass(frozen=True)
class NextestBroken:
    """cargo-nextest is on PATH, yet every retry of the probe failed; the last one's status and stderr."""

    attempts: int
    returncode: int
    stderr: str

    @property
    def error(self) -> str:
        return (
            "verify.sh: ERROR — cargo-nextest is present on PATH but the availability probe "
            f"(`{' '.join(NEXTEST_PROBE)}`) failed persistently across "
            f"{self.attempts}/{NEXTEST_PROBE_RETRIES} retries ({self.attempts + 1} probes total, "
            f"including the initial attempt; last retry rc={self.returncode}) — refusing to "
            "silently fall back to the cargo-test plan (no -E support) while cargo-nextest is "
            f"installed. Last probe stderr: {self.stderr}"
        )


@dataclass(frozen=True)
class ProbeSleepFailed:
    """The pause between two retries failed, as `sleep` fails on a value it cannot read."""

    result: CallResult

    @property
    def exit_code(self) -> int:
        return _shell_status(self.result.returncode)


NextestProbe = NextestAvailable | NextestAbsent | NextestBroken | ProbeSleepFailed


def nextest_probe(env: Mapping[str, str], repo_root: Path) -> NextestProbe:
    """Whether cargo nextest runs under `env` (the planning env).

    A failed probe is ambiguous: nextest may be absent, or a fork may have
    failed under host pressure. A cargo-nextest binary on PATH says which.
    Present, the probe is retried up to NEXTEST_PROBE_RETRIES times, with
    coreutils `sleep` given REIFY_NEXTEST_PROBE_RETRY_SLEEP between retries.
    A retry's stderr is kept without its trailing newlines, as $(…) keeps it.
    """
    if childproc.run(NEXTEST_PROBE, repo_root, env).returncode == 0:
        return NextestAvailable()
    if shutil.which("cargo-nextest", path=env.get("PATH", os.defpath)) is None:
        return NextestAbsent()
    pause = knobs.resolve(knobs.NEXTEST_PROBE_RETRY_SLEEP, env).value
    for attempt in range(1, NEXTEST_PROBE_RETRIES + 1):
        retry = childproc.run(NEXTEST_PROBE, repo_root, env)
        if retry.returncode == 0:
            return NextestAvailable()
        if attempt < NEXTEST_PROBE_RETRIES:
            slept = childproc.run(("sleep", pause), repo_root, env)
            if slept.returncode != 0:
                return ProbeSleepFailed(slept)
    return NextestBroken(NEXTEST_PROBE_RETRIES, _shell_status(retry.returncode), retry.stderr.rstrip("\n"))


def _shell_status(returncode: int) -> int:
    """A child's exit status as $? reports it: 128 + N for death by signal N."""
    return returncode if returncode >= 0 else SIGNALLED - returncode
