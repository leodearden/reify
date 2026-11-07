"""gitcall — one git command, run as verify.sh's `git -C "$REPO_ROOT" ...` runs it.

The caller's env is passed unmodified: the hook's GIT_* env is never scrubbed
(consumer C18). git's stderr is captured, never printed. A caller mirroring a
bash call that keeps git's stderr returns those lines among its warnings; one
mirroring a `2>/dev/null` call drops them.
"""

from collections.abc import Mapping
from pathlib import Path

from reify_verify import childproc
from reify_verify.childproc import CallResult


def run(repo_root: Path, env: Mapping[str, str], *args: str) -> CallResult:
    """`git -C repo_root args`. A non-zero exit, or git failing to start, is returned, never raised."""
    return childproc.run(["git", "-C", str(repo_root), *args], None, env)
