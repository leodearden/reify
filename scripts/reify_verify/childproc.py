"""childproc — one child process, run to completion and returned as a CallResult.

bashcall and gitcall both run their commands through run. A command that
cannot be started is returned with the status a shell gives it, never raised,
so the caller's policy for a failed command covers it as well.
"""

import subprocess
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path

NOT_FOUND = 127
NOT_EXECUTABLE = 126


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


def run(argv: Sequence[str], cwd: Path | None, env: Mapping[str, str]) -> CallResult:
    """argv run with stdin closed and its output captured.

    If it cannot be started, the result has status 127 (not found) or 126 (any
    other failure to start), like a shell's, and the reason on stderr.
    """
    try:
        proc = subprocess.run(
            list(argv), cwd=cwd, env=dict(env), stdin=subprocess.DEVNULL, capture_output=True
        )
    except FileNotFoundError as err:
        return CallResult(NOT_FOUND, "", (f"cannot run {argv[0]}: {err}",))
    except OSError as err:
        return CallResult(NOT_EXECUTABLE, "", (f"cannot run {argv[0]}: {err}",))
    return CallResult.of(proc)
