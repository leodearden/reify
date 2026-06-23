"""Crate-graph-aware OverlapFootprintDetector for reify.

Implements the dark_factory:γ OverlapFootprintDetector seam (PRD
dark-factory/plans/two-layer-merge-queue-prd.md §5.1, task κ #4750).

Footprint members (established incrementally across steps 2/4/6):
  ``crate:<name>``   — every crate in the reverse-dependency closure of
                       crate-mapped changed paths.
  ``path:<p>``       — every non-crate / unmapped path (preserves the
                       textual-conflict⇒overlap invariant).
  ``\\x00ALL\\x00``  — sentinel for workspace-global files or cargo failures.

Rule parity with affected-crates-lib.sh
----------------------------------------
``_is_global`` and ``_file_to_crate`` mirror the bash equivalents in
``scripts/affected-crates-lib.sh`` verbatim.  Keep them in sync: if you add
a new global path or crate location on one side, update the other.
``scripts/test_reify_overlap_detector.py::TestRuleParityWithAffectedCratesLib``
asserts the Python rules cover the known cases from the bash script so a
future one-sided edit trips a test.

Deliberate divergence — ``_is_noncrate``:
  Bash: ``docs/**``, ``gui/src/**`` → ``_is_noncrate`` → path skipped
        (contributes no member to the affected-crate set).
  Python: same paths → NOT crate-mapped AND NOT global → ``path:<p>``
          member emitted (required so the reify footprint is a provable
          superset of the DefaultPathOverlapDetector footprint, upholding
          the textual-conflict⇒overlap invariant for non-crate files,
          PRD §5.1).

Orchestrator-side wiring (register_overlap_detector("reify", ...) at startup)
is activated at deploy by ξ (reify #4751) + ν (dark_factory #1897), not here.
"""

from __future__ import annotations

import json
import subprocess
from collections.abc import Sequence
from pathlib import Path

from orchestrator.overlap_footprint import (
    Footprint,
    register_overlap_detector,
)

# Project root: two directory levels above this file (scripts/ → reify/).
_REIFY_ROOT = Path(__file__).resolve().parent.parent

# ALL sentinel: this member means "overlaps with any non-empty footprint".
_ALL = "\x00ALL\x00"


def _is_global(path: str) -> bool:
    """Return True for C4 workspace-global files (§5, affected-crates-lib.sh).

    Mirrors ``_is_global()`` in ``scripts/affected-crates-lib.sh``.  If you
    add a new global path here, add it there too (and vice versa).

    Matches: root Cargo.toml, Cargo.lock, .cargo/**, tree-sitter-reify/**,
    rust-toolchain, rust-toolchain.toml.

    Note: bash uses the shell glob ``rust-toolchain*`` which would also match
    hypothetical variants like ``rust-toolchain.custom``.  Python matches only
    the two known concrete filenames; they agree on all real files in the repo.
    """
    if path in ("Cargo.toml", "Cargo.lock"):
        return True
    if path.startswith(".cargo/"):
        return True
    if path.startswith("tree-sitter-reify/"):
        return True
    if path in ("rust-toolchain", "rust-toolchain.toml"):
        return True
    return False


def _file_to_crate(path: str) -> str | None:
    """Map a crate-owned path to its crate name (§5 rules), or None.

    Mirrors ``_file_to_crate()`` in ``scripts/affected-crates-lib.sh``.  If
    you add a new crate location here, add it there too (and vice versa).

      crates/<name>/**  →  <name>
      gui/src-tauri/**  →  reify-gui
    """
    if path.startswith("crates/"):
        rest = path[len("crates/"):]
        slash = rest.find("/")
        if slash > 0:
            return rest[:slash]
    if path.startswith("gui/src-tauri/"):
        return "reify-gui"
    return None


def _reverse_closure(metadata: dict, seed_crate_names: set) -> set:
    """Compute the inclusive reverse-dependency closure for seed_crate_names.

    Mirrors the BFS algorithm embedded in affected-crates-lib.sh:_reverse_closure.
    Returns a set of crate NAMES (not IDs) within workspace_members.
    """
    members = set(metadata["workspace_members"])
    id_to_name: dict = {p["id"]: p["name"] for p in metadata["packages"]}
    name_to_ids: dict = {}
    for p in metadata["packages"]:
        name_to_ids.setdefault(p["name"], []).append(p["id"])

    # Build reverse adjacency over workspace-internal edges, all dep kinds.
    # rev[dep_id] = set of pkg_ids in workspace that depend on dep_id.
    rev: dict = {}
    for node in metadata["resolve"]["nodes"]:
        if node["id"] not in members:
            continue
        for d in node.get("deps", []):
            if d["pkg"] not in members:
                continue
            rev.setdefault(d["pkg"], set()).add(node["id"])

    # BFS from all IDs matching any seed name, inclusive.
    seed_ids: set = set()
    for sn in seed_crate_names:
        seed_ids.update(name_to_ids.get(sn, []))

    visited: set = set(seed_ids)
    queue = list(seed_ids)
    while queue:
        curr = queue.pop()
        for dep_on_curr in rev.get(curr, set()):
            if dep_on_curr not in visited:
                visited.add(dep_on_curr)
                queue.append(dep_on_curr)

    return {id_to_name[i] for i in visited if i in members and i in id_to_name}


def _default_metadata_loader() -> dict:
    """Run cargo metadata and return the parsed JSON dict.

    Uses ``--manifest-path`` to pin the workspace to the reify project root
    (``_REIFY_ROOT``, derived from this file's location) so the result is
    independent of the orchestrator process's working directory.  Without this
    pin, a cargo invocation from an unrelated cwd could silently resolve a
    *different* valid Cargo workspace — succeeding but returning the wrong
    crate graph, which would not hit the try/except fail-wide path.
    """
    raw = subprocess.check_output(
        [
            "cargo",
            "metadata",
            "--format-version",
            "1",
            "--manifest-path",
            str(_REIFY_ROOT / "Cargo.toml"),
        ],
        stderr=subprocess.DEVNULL,
    )
    return json.loads(raw)


class CrateGraphOverlapDetector:
    """Crate-graph-aware OverlapFootprintDetector for the reify workspace.

    Footprint members = {crate:<name>} ∪ {path:<p>} ∪ {_ALL (if global/error)}.

    Step-4: footprint() now performs crate mapping + reverse closure.
    Non-crate paths still get path: members (textual-conflict⇒overlap invariant).
    Global-file ALL sentinel and cargo-failure fail-wide are added in step-6.
    """

    def __init__(self, metadata_loader=None):
        """Initialise the detector.

        Args:
            metadata_loader: Callable returning a ``cargo metadata
                --format-version 1`` JSON dict.  Defaults to the real
                ``cargo metadata`` subprocess.  Inject a synthetic fixture
                for testing.  The result is memoized after the first
                successful load; subsequent footprint() calls reuse the
                cached dict to avoid a repeated subprocess per changeset.

        Cache-staleness note:
            The metadata dict is frozen at the first successful load for the
            lifetime of this detector instance (which, when registered via
            ``register_for_reify()``, is the lifetime of the orchestrator
            process).  Two possible staleness failure modes:

            *   **New crate added** — unknown crate name → not in closure →
                ``_ALL`` sentinel (fail-wide, safe).
            *   **New intra-workspace dep edge between existing crates** —
                e.g. crate-c starts depending on crate-a after startup: the
                new edge is absent from the frozen graph, so changesets
                touching crate-a and crate-c will (incorrectly) report
                ``overlaps=False`` until the orchestrator is restarted.
                This is a false-negative (missed re-verify).  The window
                is bounded by orchestrator restarts; accept it for now.
        """
        self._metadata_loader = metadata_loader or _default_metadata_loader
        self._metadata_cache: dict | None = None

    def footprint(self, changed_paths: Sequence) -> Footprint:
        """Return the overlap footprint for the given changed paths.

        Algorithm (final form):
        1. Any workspace-global path → _ALL sentinel (C4 fail-wide).
        2. Partition remaining paths into crate-mapped seeds and unmapped.
        3. Unmapped paths → ``path:<p>`` members (superset invariant).
        4. Call metadata_loader; on any exception → _ALL sentinel (C5 fail-wide).
        5. BFS reverse closure of seed crates → ``crate:<name>`` members.
        """
        paths = list(changed_paths)
        members: set = set()

        # ── C4: global file → ALL sentinel (touches entire workspace) ────────
        for p in paths:
            if _is_global(p):
                members.add(_ALL)
                return Footprint(members=frozenset(members))

        # ── Partition: crate-mapped seeds vs. unmapped raw paths ─────────────
        seed_crates: set = set()
        for p in paths:
            crate = _file_to_crate(p)
            if crate is not None:
                seed_crates.add(crate)
            else:
                # Non-crate / unmapped path: add as path: member so that the
                # textual-conflict⇒overlap invariant holds for all file types
                # (a pure crate-set would give empty for docs/**, violating §5.1).
                members.add(f"path:{p}")

        # ── Crate reverse closure; C5: cargo failure or unresolvable seed → ALL ─
        if seed_crates:
            try:
                if self._metadata_cache is None:
                    self._metadata_cache = self._metadata_loader()
                metadata = self._metadata_cache
                closure = _reverse_closure(metadata, seed_crates)
            except Exception:
                members.add(_ALL)
            else:
                for crate_name in closure:
                    members.add(f"crate:{crate_name}")
                # _reverse_closure is INCLUSIVE: a seed name that maps to a
                # workspace-member ID is returned in the closure.  Any name NOT
                # in the closure either (a) is absent from packages/name_to_ids
                # (new crate not yet in metadata) or (b) has an id in packages
                # but is not in workspace_members (workspace-excluded).  In
                # either case the reverse-dep blast radius is unbounded — fail
                # wide via the _ALL sentinel (C5) to uphold the
                # textual-conflict⇒overlap invariant (PRD §5.1).
                if seed_crates - closure:
                    members.add(_ALL)

        return Footprint(members=frozenset(members))

    def overlaps(self, a: Footprint, b: Footprint) -> bool:
        """Return True iff footprints a and b overlap (re-verify required).

        Rules (final form, stays unchanged across all impl steps):
        - Either footprint empty → False (nothing in common).
        - _ALL in either (with the other non-empty) → True (global/fail-wide).
        - Otherwise: non-empty set intersection.
        """
        if not a.members or not b.members:
            return False
        if _ALL in a.members or _ALL in b.members:
            return True
        return bool(a.members & b.members)


def register_for_reify() -> None:
    """Register a CrateGraphOverlapDetector for project_id="reify".

    Called at orchestrator startup via the ξ (reify #4751) + ν (dark_factory
    #1897) deploy seam.  The default real-cargo detector is constructed here;
    its footprint() is fail-wide on cargo errors (try/except → _ALL sentinel).
    """
    register_overlap_detector("reify", CrateGraphOverlapDetector())
