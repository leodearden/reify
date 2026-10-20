"""bashcall — a bash library function or data file verify reads, as a typed record (PRD D15).

Each module declares the BashCall and DataFile constants it uses, so the set
of seams is collected from the modules and never kept as a hand list. run
forks ONE `bash -c` per call. The function name and its arguments travel as
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
    returncode: int
    stdout: str
    stderr_lines: tuple[str, ...]


class BashCallError(Exception):
    def __init__(self, call: BashCall, result: CallResult):
        stderr = "; ".join(result.stderr_lines) or "no stderr"
        super().__init__(f"{call} exited {result.returncode} ({stderr})")
        self.call = call
        self.result = result


def run(call: BashCall, args: Sequence[str], repo_root: Path, env: Mapping[str, str]) -> CallResult:
    """The call's stdout and stderr lines; a non-zero exit raises BashCallError.

    cwd is repo_root, as under verify.sh's `cd "$REPO_ROOT"`, and stderr is
    captured so the caller returns it in bash's stderr order.
    """
    proc = subprocess.run(
        ["bash", "-c", _SOURCE_AND_CALL, "bashcall", str(repo_root / call.lib), call.function, *args],
        cwd=repo_root,
        env={name: value for name, value in env.items() if name not in _DROPPED_ENV},
        stdin=subprocess.DEVNULL,
        capture_output=True,
    )
    result = CallResult(
        proc.returncode,
        proc.stdout.decode("utf-8", "surrogateescape"),
        tuple(proc.stderr.decode("utf-8", "surrogateescape").splitlines()),
    )
    if result.returncode != 0:
        raise BashCallError(call, result)
    return result
