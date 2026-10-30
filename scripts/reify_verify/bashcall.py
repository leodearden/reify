"""bashcall — the gate's seams: the bash library functions and data files verify reads (PRD D1, D15).

Each seam is a typed record: a BashCall for a library function, a BashExport
for a variable a library exports, a DataFile for a data file, a SourcedFile
for a shell file sourced for what it exports. Each module declares the
constants it uses, so a later --print-gates (PRD D15) can collect them from
the modules. run, read_export and source_delta each fork ONE `bash -c` per
call. Paths, the function name and its arguments travel as argv
and are never interpolated into the script text.

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
_SOURCE_AND_PRINT = 'source "$1" || exit; printenv -- "$2" || exit 0'
_ENV_AROUND_SOURCE = 'env -0; printf "\\0"; . "$1" || exit; env -0; exit "$?"'
_DROPPED_ENV = ("BASH_ENV", "ENV")


@dataclass(frozen=True)
class BashCall:
    """A function of a bash library, the library path relative to the repo root."""

    lib: str
    function: str

    def __str__(self) -> str:
        return f"{self.lib}::{self.function}"


@dataclass(frozen=True)
class BashExport:
    """A variable a bash library exports when sourced, the library path relative to the repo root."""

    lib: str
    name: str

    def __str__(self) -> str:
        return f"{self.lib}::${self.name}"


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


@dataclass(frozen=True)
class SourcedFile:
    """A shell file sourced for the environment it exports, relative to $HOME.

    It lives outside the repo, so it is never a --print-gates path.
    """

    home_relative: str

    def path(self, env: Mapping[str, str]) -> str:
        """bash's "$HOME/<file>"."""
        return f"{env.get('HOME', '')}/{self.home_relative}"

    def __str__(self) -> str:
        return f"$HOME/{self.home_relative}"


@dataclass(frozen=True)
class EnvDelta:
    """What sourcing a file changed in the exported environment."""

    assigned: tuple[tuple[str, str], ...]
    removed: tuple[str, ...]

    def applied_to(self, env: Mapping[str, str]) -> dict[str, str]:
        adopted = {name: value for name, value in env.items() if name not in self.removed}
        adopted.update(self.assigned)
        return adopted


class DataFileUnreadable(Exception):
    def __init__(self, file: DataFile, reason: str):
        super().__init__(f"{file.path} is unreadable ({reason})")
        self.file = file


class BashCallError(Exception):
    def __init__(self, call: "BashCall | BashExport | SourcedFile", result: CallResult):
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


def read_export(export: BashExport, repo_root: Path, env: Mapping[str, str]) -> str:
    """The value `export.lib` exports as `export.name` when sourced under `env`, "" when it exports none.

    bash's `${NAME:-}` reads an unset variable as empty, so this does too. A
    failed `source` raises BashCallError.
    """
    result = childproc.run(
        ["bash", "-c", _SOURCE_AND_PRINT, "bashcall", str(repo_root / export.lib), export.name],
        repo_root,
        {name: value for name, value in env.items() if name not in _DROPPED_ENV},
    )
    if result.returncode != 0:
        raise BashCallError(export, result)
    return result.stdout.removesuffix("\n")


def source_delta(file: SourcedFile, repo_root: Path, env: Mapping[str, str]) -> EnvDelta:
    """Every exported name `file` sets, changes or removes when sourced under `env`.

    One fork prints `env -0` before and after the `.`, split by an empty NUL
    record (no NAME=VALUE record is empty). The trailing `exit` keeps bash from
    exec'ing the last `env`, which would lower its SHLVL into the delta. A
    failed `.` raises BashCallError.
    """
    result = childproc.run(
        ["bash", "-c", _ENV_AROUND_SOURCE, "bashcall", file.path(env)],
        repo_root,
        {name: value for name, value in env.items() if name not in _DROPPED_ENV},
    )
    if result.returncode != 0:
        raise BashCallError(file, result)
    records = result.stdout.split("\0")
    boundary = records.index("")
    before = _environ(records[:boundary])
    after = _environ(records[boundary + 1:])
    return EnvDelta(
        assigned=tuple((name, value) for name, value in after.items() if before.get(name) != value),
        removed=tuple(name for name in before if name not in after),
    )


def _environ(records: list[str]) -> dict[str, str]:
    return dict(record.partition("=")[::2] for record in records if record)
