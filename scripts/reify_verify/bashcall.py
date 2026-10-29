"""bashcall — the gate's seams: the bash library functions and data files verify reads (PRD D1, D15).

Each seam is a typed record: a BashCall for a library function, a DataFile
for a data file. Each module declares the BashCall and DataFile constants it
uses, so a later --print-gates (PRD D15) can collect them from the modules.
run forks ONE `bash -c` per call. The function name and its arguments travel
as argv and are never interpolated into the script text.

A data file that cannot be read raises DataFileUnreadable whatever the cause.
Each reader turns that into its own conservative value and a stderr warning.
"""

from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from os.path import isfile
from pathlib import Path

from reify_verify import childproc
from reify_verify.childproc import CallResult

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

    def is_file(self, repo_root: Path) -> bool:
        """bash's `[ -f ]`: a regular file, after symlinks; False whenever that cannot be told."""
        return isfile(repo_root / self.path)

    def read_text(self, repo_root: Path) -> str:
        """The file as UTF-8 text; any failure to read it raises DataFileUnreadable."""
        try:
            return (repo_root / self.path).read_text(encoding="utf-8")
        except (OSError, ValueError) as err:
            raise DataFileUnreadable(self, str(err)) from err


class DataFileUnreadable(Exception):
    def __init__(self, file: DataFile, reason: str):
        super().__init__(f"{file.path} is unreadable ({reason})")
        self.file = file


class BashCallError(Exception):
    def __init__(self, call: BashCall, result: CallResult):
        super().__init__(result.exit_summary(str(call)))
        self.call = call
        self.result = result


def run(call: BashCall, args: Sequence[str], repo_root: Path, env: Mapping[str, str]) -> CallResult:
    """The call's stdout and stderr lines; a non-zero exit raises BashCallError, as does bash
    itself failing to start.

    cwd is repo_root, as under verify.sh's `cd "$REPO_ROOT"`, and stderr is
    captured so the caller returns it in bash's stderr order.
    """
    result = childproc.run(
        ["bash", "-c", _SOURCE_AND_CALL, "bashcall", str(repo_root / call.lib), call.function, *args],
        repo_root,
        {name: value for name, value in env.items() if name not in _DROPPED_ENV},
    )
    if result.returncode != 0:
        raise BashCallError(call, result)
    return result
