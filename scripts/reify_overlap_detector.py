"""Crate-graph-aware OverlapFootprintDetector for reify.

Implements the dark_factory:γ OverlapFootprintDetector seam (PRD
dark-factory/plans/two-layer-merge-queue-prd.md §5.1, task κ #4750).

Footprint members (established incrementally across steps 2/4/6):
  ``crate:<name>``   — every crate in the reverse-dependency closure of
                       crate-mapped changed paths.
  ``path:<p>``       — every non-crate / unmapped path (preserves the
                       textual-conflict⇒overlap invariant).
  ``\\x00ALL\\x00``  — sentinel for workspace-global files or cargo failures.

Orchestrator-side wiring (register_overlap_detector("reify", ...) at startup)
is activated at deploy by ξ (reify #4751) + ν (dark_factory #1897), not here.
"""

from __future__ import annotations

import json
import subprocess
from collections.abc import Sequence

from orchestrator.overlap_footprint import (
    Footprint,
    OverlapFootprintDetector,  # noqa: F401 — imported for Protocol conformance
    DefaultPathOverlapDetector,  # noqa: F401 — re-exported for boundary tests
    register_overlap_detector,
)

# ALL sentinel: this member means "overlaps with any non-empty footprint".
_ALL = "\x00ALL\x00"


def _default_metadata_loader() -> dict:
    """Run cargo metadata and return the parsed JSON dict."""
    raw = subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1"],
        stderr=subprocess.DEVNULL,
    )
    return json.loads(raw)


class CrateGraphOverlapDetector:
    """Crate-graph-aware OverlapFootprintDetector for the reify workspace.

    Footprint members = {crate:<name>} ∪ {path:<p>} ∪ {_ALL (if global/error)}.

    This is the step-2 placeholder: footprint() namespaces all paths as
    ``path:<p>`` establishing the return-type contract.  Full crate-closure
    logic is added in step-4; global-file ALL sentinel and cargo-failure
    fail-wide are added in step-6.
    """

    def __init__(self, metadata_loader=None):
        """Initialise the detector.

        Args:
            metadata_loader: Callable returning a ``cargo metadata
                --format-version 1`` JSON dict.  Defaults to the real
                ``cargo metadata`` subprocess.  Inject a synthetic fixture
                for testing.
        """
        self._metadata_loader = metadata_loader or _default_metadata_loader

    def footprint(self, changed_paths: Sequence) -> Footprint:
        """Return the overlap footprint for the given changed paths.

        Step-2 placeholder: namespace every path as ``path:<p>``.
        Expanded in steps 4 and 6 to use crate closure + ALL sentinel.
        """
        members = frozenset(f"path:{p}" for p in changed_paths)
        return Footprint(members=members)

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

    Added in step-8.  Called at orchestrator startup (ξ/#4751 + ν/#1897 seam).
    """
    raise NotImplementedError("register_for_reify() added in step-8")
