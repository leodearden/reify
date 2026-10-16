"""Crate-graph-aware OverlapFootprintDetector for reify.

Implements the dark_factory:γ OverlapFootprintDetector seam (PRD
dark-factory/plans/two-layer-merge-queue-prd.md §5.1, task κ #4750).

Footprint members:
  ``crate:<name>``   — every crate in the reverse-dependency closure of the
                       changed paths' seed crates, plus every unioned crate.
  ``path:<p>``       — every changed path that seeds no crate (preserves the
                       textual-conflict⇒overlap invariant).
  ``\\x00ALL\\x00``  — sentinel for workspace-global files or cargo failures.

Path attribution
----------------
Whether a path is global, and which crates it seeds or unions, comes from
``reify_path_attribution.py``, the mirror of ``scripts/affected-crates-lib.sh``'s
per-path rules. Their parity is enforced by
``tests/infra/test_affected_crates_lib.sh``'s OVERLAP-MIRROR-PARITY guard.

Cost of the corpus edge: an ``examples/*.ri`` edit seeds every corpus reader,
so its footprint is wide. That over-reports overlap, which is the safe
direction.

Deliberate divergence
---------------------
Attribution matches bash exactly; the detector differs from bash in two places.

(A) Unattributed paths. Bash splits a path no rule attributes into skip
    (``_is_noncrate``: ``reify_is_inert_path``'s docs/**, *.md, *.yaml,
    *.yml, plus gui/src/** and tests/infra/**) versus C5 ALL for anything
    else. The detector collapses both into a ``path:<p>`` member, making its
    footprint a superset of the ``DefaultPathOverlapDetector`` footprint.

    Task 7427's widening of the inert class to *.md/*.yaml/*.yml is NOT
    ported. It answers verify narrowing's question ("which crates' tests can
    this diff break"), not overlap's ("do these two changesets interact"):
    porting it would drop ``path:README.md``, so two changesets editing the
    same README.md would be reported non-overlapping, breaking
    textual-conflict⇒overlap (PRD §5.1).

    The maintained contract is:

        default.overlaps(A, B) ⇒ reify.overlaps(A, B)

    which holds because any shared ``path:<p>`` member in the default
    detector is also present in the reify footprint.  Two changesets
    touching DIFFERENT unknown build-affecting root files (e.g. Makefile
    vs flake.nix) are reported non-overlapping by reify where bash would
    fail-wide — an intentional, documented narrowing relative to bash
    (PRD §5.1).

(B) Closure model. ``_reverse_closure`` walks every dependency kind
    transitively: a superset of bash's dev-dep-non-transitive
    ``_reify_compile_closure``, so it can over-report overlap but never
    under-report it.

Orchestrator-side wiring (register_overlap_detector("reify", ...) at startup)
is activated at deploy by ξ (reify #4751) + ν (dark_factory #1897), not here.
"""

from __future__ import annotations

import json
import subprocess
import threading
from collections.abc import Sequence
from pathlib import Path

from orchestrator.overlap_footprint import (
    Footprint,
    register_overlap_detector,
)

import reify_path_attribution as rpa

# Project root: two directory levels above this file (scripts/ → reify/).
_REIFY_ROOT = Path(__file__).resolve().parent.parent

# ALL sentinel: this member means "overlaps with any non-empty footprint".
_ALL = "\x00ALL\x00"


def _reverse_closure(metadata: dict, seed_crate_names: set) -> set:
    """Compute the inclusive reverse-dependency closure for seed_crate_names.

    Walks every dependency kind transitively, a superset of
    affected-crates-lib.sh's closure (module docstring, "Deliberate
    divergence" (B)). Returns a set of crate NAMES (not IDs) within
    workspace_members.
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

    Footprint members = {crate:<name>} ∪ {path:<p>} ∪ {_ALL (if global/error)};
    the algorithm is footprint()'s.
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

                Thread safety: a ``threading.Lock`` guards the lazy-load
                so concurrent first calls serialize — only one invokes the
                loader.  If the loader raises, the cache stays ``None`` and
                the next call retries (idempotent, fail-wide via _ALL).

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
        self._metadata_lock = threading.Lock()
        self._metadata_cache: dict | None = None

    def footprint(self, changed_paths: Sequence) -> Footprint:
        """Return the overlap footprint for the given changed paths.

        1. Any workspace-global path → _ALL sentinel (C4 fail-wide).
        2. A path that seeds no crate → a ``path:<p>`` member (superset
           invariant); seed crates accumulate.
        3. Reverse closure of the seeds → ``crate:<name>`` members; a loader
           exception or an unresolvable seed → _ALL (C5 fail-wide).
        4. Unioned crates (a manifest's DAG-gate crate) → ``crate:<name>``
           members, outside the closure.
        """
        attributions = {p: rpa.attribute(p) for p in changed_paths}
        if any(a.is_global for a in attributions.values()):
            return Footprint(members=frozenset({_ALL}))

        members: set = set()
        seed_crates: set = set()
        unioned_crates: set = set()
        for path, attribution in attributions.items():
            if attribution.seed_crates:
                seed_crates |= attribution.seed_crates
            else:
                members.add(f"path:{path}")
            unioned_crates |= attribution.unioned_crates

        if seed_crates:
            members |= self._closure_members(seed_crates)
        members |= {f"crate:{c}" for c in unioned_crates}
        return Footprint(members=frozenset(members))

    def _closure_members(self, seed_crates: set) -> set:
        """``crate:<name>`` for the seeds' reverse closure, or _ALL (C5)."""
        try:
            if self._metadata_cache is None:
                with self._metadata_lock:
                    if self._metadata_cache is None:
                        self._metadata_cache = self._metadata_loader()
            closure = _reverse_closure(self._metadata_cache, seed_crates)
        except Exception:
            return {_ALL}
        members = {f"crate:{c}" for c in closure}
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
        return members

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
