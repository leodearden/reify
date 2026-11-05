"""retry — the failed_only retry: whether a run may narrow, each profile's exact-id subset, and the marker reporting what narrowed (PRD D1).

A retry narrows its nextest passes only when the attempt-0 sidecar's tree_oid
is the tree DF retries, so the warm target/ is the one that tree built.
read_sidecar_tree_oid is the one reader of that sidecar. Every refusal is
loud: the pass runs in full and stderr says why, in bash's words. Two lines
are the package's own, where bash prints a broken plan or its shell's error:
an id the filterset's quotes cannot hold refuses its pass, and a ceiling
that is not an integer warns that none applies.

The marker opens stdout iff some suite really narrowed: a nextest profile
that applied its subset, run_all's matched members, or the gui specs.
"""

import os
import re
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path

from reify_verify import childproc, knobs
from reify_verify.gatepaths import RUN_ALL
from reify_verify.plan import QUOTE_NAMES, NextestFlags, RetryMarker
from reify_verify.policy import PROFILE_TABLE, Profile

RETRY_SCOPE_NARROWED = "failed_only"
REFUSED = "verify.sh: retry refused: "
NO_NEXTEST_REFUSAL = f"{REFUSED}no nextest — cargo-test fallback has no -E filterset support (full verify)"
RUN_ALL_SUBSET_MATCHED = re.compile(r"@@REIFY_RUN_ALL_SUBSET_MATCHED=([0-9]+)@@")
IFS_WORDS = re.compile(r"[^ \t\n]+")
# bash's [[:space:]] over ASCII, the class its sed extractor and id trimming use.
SPACE = b" \t\n\v\f\r"
SIDECAR_TREE_OID = re.compile(rb'.*"tree_oid"[ \t\n\v\f\r]*:[ \t\n\v\f\r]*"([^"]*)"')
# What bash's `[ n -gt max ]` accepts as an integer; anything else applies no ceiling.
CEILING = re.compile(r"[ \t\n\v\f\r]*[+-]?[0-9]+[ \t]*", re.ASCII)


def is_retry(planning: Mapping[str, str]) -> bool:
    return knobs.resolve(knobs.RETRY_SCOPE, planning).value == RETRY_SCOPE_NARROWED


def read_sidecar_tree_oid(path: Path) -> str:
    """The tree_oid on the first sidecar line holding one (its last on that line), or '' when there is none."""
    try:
        data = path.read_bytes() if path.is_file() else b""
    except OSError:
        data = b""
    for line in data.split(b"\n"):
        found = SIDECAR_TREE_OID.match(line)
        if found:
            return found.group(1).decode("utf-8", "surrogateescape")
    return ""


def eligibility(sidecar: Path, planning: Mapping[str, str]) -> tuple[bool, tuple[str, ...]]:
    """Whether the sidecar pins the tree REIFY_VERIFY_RETRY_TREE_OID names; else the tree-drift refusal."""
    have = read_sidecar_tree_oid(sidecar)
    want = knobs.resolve(knobs.RETRY_TREE_OID, planning).value
    if want and have == want:
        return True, ()
    return False, (
        f"{REFUSED}tree drift — sidecar tree_oid {have or '<absent>'} != "
        f"{knobs.RETRY_TREE_OID.name} {want or '<unset>'} (full verify)",
    )


def subset(
    profile: Profile, planning: Mapping[str, str], max_subset: str, repo_root: Path
) -> tuple[tuple[str, ...], tuple[str, ...]]:
    """The profile pass's exact test ids, or none and the refusal that runs it in full; and the lines it prints.

    The profile's own filter file wins when set; the base file stands in.
    max_subset is REIFY_VERIFY_RETRY_MAX_SUBSET's text, which bash reads at
    load. A ceiling that is not an integer warns where bash's `[ -gt ]` errors,
    and applies none.
    """
    base = knobs.resolve(knobs.RETRY_NEXTEST_FILTER_FILE, planning).value
    named = str(knobs.resolve(PROFILE_TABLE[profile].retry_filter_env, planning, default=base).value)
    ids = _filter_ids(repo_root / named) if named else ()
    where = f"(profile={profile.value}, full verify)"
    if not ids:
        return (), (f"{REFUSED}no subset — filter file {named or '<unset>'} absent/empty/unreadable {where}",)
    if not CEILING.fullmatch(max_subset):
        warned = (no_ceiling_warning(max_subset),)
    elif len(ids) > int(max_subset):
        return (), (f"{REFUSED}subset too large — {len(ids)} > ceiling {max_subset} {where}",)
    else:
        warned = ()
    quote = NextestFlags.RETRY_ID_QUOTE
    unquotable = next((test_id for test_id in ids if quote in test_id), None)
    if unquotable is not None:
        return (), (
            *warned,
            f"{REFUSED}unquotable id — {unquotable} holds a {QUOTE_NAMES[quote]} quote {where}",
        )
    return ids, warned


def no_ceiling_warning(max_subset: str) -> str:
    return f"verify.sh: WARNING: invalid {knobs.RETRY_MAX_SUBSET.name}={max_subset}; applying no subset ceiling"


def _filter_ids(path: Path) -> tuple[str, ...]:
    """The file's lines, trimmed, without the empty ones; none when it cannot be read."""
    if not os.access(path, os.R_OK):
        return ()
    try:
        data = path.read_bytes()
    except OSError:
        return ()
    trimmed = (line.strip(SPACE) for line in data.split(b"\n"))
    return tuple(line.decode("utf-8", "surrogateescape") for line in trimmed if line)


@dataclass(frozen=True)
class NextestSubsetsApplied:
    """The within-ceiling nextest subset sizes the test region applied, per profile."""

    debug: int
    release: int


NO_NEXTEST_SUBSETS = NextestSubsetsApplied(debug=0, release=0)


def marker(
    nextest: NextestSubsetsApplied, gui: int, env: Mapping[str, str], repo_root: Path
) -> RetryMarker | None:
    """The retry's applied subset sizes; None unless it is a retry and at least one suite really narrowed."""
    if not is_retry(env):
        return None
    applied = RetryMarker(
        nextest_debug=nextest.debug,
        nextest_release=nextest.release,
        run_all=_run_all_subset_size(env, repo_root),
        gui=gui,
    )
    narrowed = (applied.nextest_debug, applied.nextest_release, applied.run_all, applied.gui)
    return applied if any(narrowed) else None


def _run_all_subset_size(env: Mapping[str, str], repo_root: Path) -> int:
    """How many REIFY_RUN_ALL_MEMBER_SUBSET members run_all.sh's count-only probe matches.

    run_all.sh owns the member-skip predicate, so its count is the truth. Its
    stderr is dropped. Without the script, or without the count in its
    stdout, the subset's word count stands in; the words are never globbed.
    """
    members = knobs.resolve(knobs.RUN_ALL_MEMBER_SUBSET, env).value
    if not members:
        return 0
    if RUN_ALL.is_file(repo_root):
        probe = childproc.run(
            ["bash", RUN_ALL.path],
            repo_root,
            {**env, "REIFY_RUN_ALL_SUBSET_COUNT_ONLY": "1", knobs.RUN_ALL_MEMBER_SUBSET.name: members},
        )
        matched = RUN_ALL_SUBSET_MATCHED.search(probe.stdout)
        if matched:
            return int(matched.group(1))
    return len(IFS_WORDS.findall(members))
