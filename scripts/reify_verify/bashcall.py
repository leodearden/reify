"""bashcall — a bash library function or data file verify reads, as a typed record (PRD D15).

Each module declares the BashCall and DataFile constants it uses, so a later
--print-gates (PRD D15) can collect them from the modules. run forks ONE
`bash -c` per call. The function name and its arguments travel as
argv and are never interpolated into the script text.
"""

import subprocess
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path

_SOURCE_AND_CALL = 'source "$1" || exit; shift; "$@"'
_DROPPED_ENV = ("BASH_ENV", "ENV")


@dataclass(frozen=True)
class BashCall:
    """A function of a bash library, the library path relative to the repo root."""

    lib: str
    function: str

    def __str__(self) -> str:
        return f"{self.lib}::{self.function}"


@dataclass(frozen=True)
class DataFile:
    """A data file a module reads, relative to the repo root."""

    path: str

    def read_text(self, repo_root: Path) -> str:
        return (repo_root / self.path).read_text(encoding="utf-8")


@dataclass(frozen=True)
class CallResult:
    """A finished child process: its exit status, its stdout, and its stderr as lines."""

    returncode: int
    stdout: str
    stderr_lines: tuple[str, ...]

    @classmethod
    def of(cls, proc: subprocess.CompletedProcess) -> "CallResult":
        return cls(
            proc.returncode,
            proc.stdout.decode("utf-8", "surrogateescape"),
            tuple(proc.stderr.decode("utf-8", "surrogateescape").splitlines()),
        )

    @property
    def lines(self) -> tuple[str, ...]:
        """stdout's non-empty lines, verbatim."""
        return tuple(line for line in self.stdout.split("\n") if line)

    def exit_summary(self, command: str) -> str:
        """`command exited N (stderr lines)`: why a failed call failed, on one line."""
        stderr = "; ".join(self.stderr_lines) or "no stderr"
        return f"{command} exited {self.returncode} ({stderr})"


class BashCallError(Exception):
    def __init__(self, call: BashCall, result: CallResult):
        super().__init__(result.exit_summary(str(call)))
        self.call = call
        self.result = result


def run(call: BashCall, args: Sequence[str], repo_root: Path, env: Mapping[str, str]) -> CallResult:
    """The call's stdout and stderr lines; a non-zero exit raises BashCallError.

    cwd is repo_root, as under verify.sh's `cd "$REPO_ROOT"`, and stderr is
    captured so the caller returns it in bash's stderr order.
    """
    result = CallResult.of(
        subprocess.run(
            ["bash", "-c", _SOURCE_AND_CALL, "bashcall", str(repo_root / call.lib), call.function, *args],
            cwd=repo_root,
            env={name: value for name, value in env.items() if name not in _DROPPED_ENV},
            stdin=subprocess.DEVNULL,
            capture_output=True,
        )
    )
    if result.returncode != 0:
        raise BashCallError(call, result)
    return result
