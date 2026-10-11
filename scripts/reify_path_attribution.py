"""Attribute a changed repo path to the workspace crates whose tests it can affect.

The Python mirror of scripts/affected-crates-lib.sh's per-path rules:
``_is_global``, ``_file_to_crate`` (including its examples/*.ri arm),
``_RI_CORPUS_CRATES``, ``_is_crate_manifest`` and ``_REIFY_DAG_GATE_CRATE``.
tests/infra/test_affected_crates_lib.sh's OVERLAP-MIRROR-PARITY compares the
two sides over every tracked path, so edit both together.

Corpus leaves and per-crate manifests are attributed because a file a crate's
tests read is a semantic input to that crate: docs/prds/verify-scope-contract.md
§3, "Corpus mapping" and "Per-crate manifest touches also contribute the
crate-DAG gate".

Not mirrored: ``_is_noncrate`` / ``reify_is_inert_path``. They only route paths
no rule attributes; see reify_overlap_detector.py, "Deliberate divergence".

Stdlib-only, and a module of its own, so the gate's /usr/bin/python3 (no
dark-factory venv) can import it.
"""

from __future__ import annotations

from collections.abc import Iterable
from dataclasses import dataclass
from fnmatch import fnmatchcase

RI_CORPUS_CRATES: frozenset[str] = frozenset(
    {
        "reify-cli",
        "reify-compiler",
        "reify-eval",
        "reify-eval-fea-tests",
        "reify-gui",
        "reify-test-support",
    }
)

DAG_GATE_CRATE = "reify-build-utils"

# The bash `case` arms' own pattern strings. Like a `case` glob, fnmatchcase's
# `*` spans `/` and the match is case-sensitive.
_GLOBAL_GLOBS = ("Cargo.toml", "Cargo.lock", ".cargo/*", "tree-sitter-reify/*", "rust-toolchain*")
_CRATE_MANIFEST_GLOBS = ("crates/*/Cargo.toml", "gui/src-tauri/Cargo.toml")
_CRATE_DIR_GLOB = "crates/*/*"
_GUI_CRATE_GLOB = "gui/src-tauri/*"
_GUI_CRATE = "reify-gui"
_RI_CORPUS_GLOB = "examples/*.ri"


@dataclass(frozen=True)
class PathAttribution:
    """What one path contributes: a C4 global, closure seeds, and crates
    unioned into the closure's result without being seeded into it."""

    is_global: bool
    seed_crates: frozenset[str]
    unioned_crates: frozenset[str]


def attribute(path: str) -> PathAttribution:
    """Classify one repo-relative path. The three fields are independent;
    precedence between them is the caller's."""
    return PathAttribution(
        is_global=_matches_any(path, _GLOBAL_GLOBS),
        seed_crates=_seed_crates(path),
        unioned_crates=_unioned_crates(path),
    )


def _matches_any(path: str, globs: Iterable[str]) -> bool:
    return any(fnmatchcase(path, glob) for glob in globs)


def _seed_crates(path: str) -> frozenset[str]:
    """``_file_to_crate``: the first matching arm wins."""
    if fnmatchcase(path, _CRATE_DIR_GLOB):
        # `crates//x` matches with an empty name, which bash prints as an
        # empty line that affected_crates skips.
        name = path.removeprefix("crates/").split("/", 1)[0]
        return frozenset({name}) if name else frozenset()
    if fnmatchcase(path, _GUI_CRATE_GLOB):
        return frozenset({_GUI_CRATE})
    if fnmatchcase(path, _RI_CORPUS_GLOB):
        return RI_CORPUS_CRATES
    return frozenset()


def _unioned_crates(path: str) -> frozenset[str]:
    """``_is_crate_manifest`` -> ``_REIFY_DAG_GATE_CRATE``."""
    if _matches_any(path, _CRATE_MANIFEST_GLOBS):
        return frozenset({DAG_GATE_CRATE})
    return frozenset()
