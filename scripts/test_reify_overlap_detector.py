#!/usr/bin/env python3
"""
test_reify_overlap_detector.py — stdlib unittest for reify_overlap_detector.py.

Implements the two-way boundary test for the dark_factory:γ OverlapFootprintDetector
seam (PRD dark-factory/plans/two-layer-merge-queue-prd.md §5.1, task κ #4750).

Run directly:  python3 scripts/test_reify_overlap_detector.py
Run via infra:  tests/infra/test_reify_overlap_detector.sh
"""

import sys
import os
import unittest

# Insert scripts/ on sys.path so sibling imports resolve without installation.
_SCRIPTS_DIR = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, _SCRIPTS_DIR)

# ── SKIP-guard: dark-factory γ seam must be importable ────────────────────
# Mirrors the shell-harness SKIP-guard: if the dark-factory venv is absent we
# exit 0 cleanly rather than crashing with an opaque ImportError.
try:
    import orchestrator.overlap_footprint as ov
except ImportError:
    print(
        "SKIP: orchestrator.overlap_footprint not importable; "
        "skipping all tests (dark-factory venv absent)."
    )
    sys.exit(0)

# The module under test. ImportError here is the RED state for step-1:
# the module doesn't exist yet until step-2 creates it.
import reify_overlap_detector as rod

# ─── Fixtures ────────────────────────────────────────────────────────────────

# Minimal one-crate fixture: just crate-a, no deps.
# Used by TestBasicGammaContract (step-1).
_MINIMAL_A_ID = "crate-a 0.1.0 (path+file:///workspace/crates/crate-a)"
_MINIMAL_FIXTURE = {
    "packages": [{"id": _MINIMAL_A_ID, "name": "crate-a"}],
    "workspace_members": [_MINIMAL_A_ID],
    "resolve": {
        "nodes": [{"id": _MINIMAL_A_ID, "deps": []}],
    },
}

# Four-crate fixture: A, B (B→A), C (disjoint), reify-gui.
# Used by TestCrateGraphBehavior (step-3) and later steps.
_A_ID = "crate-a 0.1.0 (path+file:///workspace/crates/crate-a)"
_B_ID = "crate-b 0.1.0 (path+file:///workspace/crates/crate-b)"
_C_ID = "crate-c 0.1.0 (path+file:///workspace/crates/crate-c)"
_GUI_ID = "reify-gui 0.1.0 (path+file:///workspace/gui/src-tauri)"

_FIXTURE = {
    "packages": [
        {"id": _A_ID, "name": "crate-a"},
        {"id": _B_ID, "name": "crate-b"},
        {"id": _C_ID, "name": "crate-c"},
        {"id": _GUI_ID, "name": "reify-gui"},
    ],
    "workspace_members": [_A_ID, _B_ID, _C_ID, _GUI_ID],
    "resolve": {
        "nodes": [
            {"id": _A_ID, "deps": []},
            {
                "id": _B_ID,
                "deps": [{"pkg": _A_ID, "dep_kinds": [{"kind": None}]}],
            },
            {"id": _C_ID, "deps": []},
            {"id": _GUI_ID, "deps": []},
        ]
    },
}


# ─── Step-1: Basic γ-contract cases ──────────────────────────────────────────

class TestBasicGammaContract(unittest.TestCase):
    """Basic γ-contract cases for CrateGraphOverlapDetector (step-1).

    Uses the minimal one-crate fixture so tests survive step-4's crate-mapping
    rewrite (crate-a path → crate:crate-a member, still non-empty).
    """

    def setUp(self):
        self.det = rod.CrateGraphOverlapDetector(
            metadata_loader=lambda: _MINIMAL_FIXTURE
        )

    def test_footprint_empty_returns_footprint_instance(self):
        """footprint([]) returns an ov.Footprint instance."""
        f = self.det.footprint([])
        self.assertIsInstance(f, ov.Footprint)

    def test_footprint_nonempty_returns_footprint_instance(self):
        """footprint([crate path]) returns an ov.Footprint instance."""
        f = self.det.footprint(["crates/crate-a/src/lib.rs"])
        self.assertIsInstance(f, ov.Footprint)

    def test_overlaps_symmetric(self):
        """overlaps is symmetric: overlaps(a, b) == overlaps(b, a)."""
        fa = self.det.footprint(["crates/crate-a/src/lib.rs"])
        fb = self.det.footprint(["crates/crate-a/src/other.rs"])
        self.assertEqual(
            self.det.overlaps(fa, fb),
            self.det.overlaps(fb, fa),
        )

    def test_overlaps_reflexive_nonempty(self):
        """overlaps(f, f) is True for a non-empty footprint."""
        f = self.det.footprint(["crates/crate-a/src/lib.rs"])
        self.assertTrue(self.det.overlaps(f, f))

    def test_empty_footprint_disjoint_from_nonempty(self):
        """footprint([]) overlaps nothing when compared to a non-empty footprint."""
        empty = self.det.footprint([])
        nonempty = self.det.footprint(["crates/crate-a/src/lib.rs"])
        self.assertFalse(self.det.overlaps(empty, nonempty))
        self.assertFalse(self.det.overlaps(nonempty, empty))

    def test_empty_footprint_disjoint_from_empty(self):
        """footprint([]) does not overlap another footprint([])."""
        e1 = self.det.footprint([])
        e2 = self.det.footprint([])
        self.assertFalse(self.det.overlaps(e1, e2))

    def test_protocol_conformance(self):
        """CrateGraphOverlapDetector isinstance of ov.OverlapFootprintDetector."""
        self.assertIsInstance(self.det, ov.OverlapFootprintDetector)


# ─── Step-3: Crate-graph behavior cases ──────────────────────────────────────

class TestCrateGraphBehavior(unittest.TestCase):
    """Crate-graph behavior tests using the four-crate synthetic fixture (step-3).

    Fixture dep graph: B → A; C and reify-gui are disjoint from each other and A/B.
    Reverse-dependency closure of A = {A, B}; of B = {B}; of C = {C}; of gui = {gui}.
    """

    def setUp(self):
        self.det = rod.CrateGraphOverlapDetector(
            metadata_loader=lambda: _FIXTURE
        )

    def test_same_crate_different_files_overlap(self):
        """The κ headline signal: same crate, different files → overlaps True.

        DefaultPathOverlapDetector would return False (different paths);
        the crate-graph closure collapses them to the same crate:<name> member.
        """
        fa = self.det.footprint(["crates/crate-a/src/lib.rs"])
        fb = self.det.footprint(["crates/crate-a/src/other.rs"])
        self.assertTrue(
            self.det.overlaps(fa, fb),
            "same crate / different files must overlap (crate-graph signal)",
        )

    def test_dependent_crate_overlap(self):
        """B depends on A: a change to A overlaps a change to B (reverse closure)."""
        fa = self.det.footprint(["crates/crate-a/src/lib.rs"])
        fb = self.det.footprint(["crates/crate-b/src/lib.rs"])
        self.assertTrue(
            self.det.overlaps(fa, fb),
            "A's reverse closure includes B; their footprints must overlap",
        )

    def test_disjoint_crates_no_overlap(self):
        """C is disjoint from A and B: their footprints must NOT overlap."""
        fa = self.det.footprint(["crates/crate-a/src/lib.rs"])
        fc = self.det.footprint(["crates/crate-c/src/lib.rs"])
        self.assertFalse(
            self.det.overlaps(fa, fc),
            "disjoint crates (no dep edge) must not overlap",
        )

    def test_gui_src_tauri_maps_to_reify_gui(self):
        """gui/src-tauri/** paths map to the reify-gui crate."""
        f1 = self.det.footprint(["gui/src-tauri/src/main.rs"])
        f2 = self.det.footprint(["gui/src-tauri/src/lib.rs"])
        self.assertTrue(
            self.det.overlaps(f1, f2),
            "gui/src-tauri/** must collapse to crate:reify-gui → overlap",
        )


# ─── Step-5: γ-contract superset / invariant cases ───────────────────────────

class TestGammaContractSuperset(unittest.TestCase):
    """γ-contract superset and invariant cases (step-5).

    Tests the textual-conflict⇒overlap invariant (PRD §5.1):
    reify.overlaps ⊇ default.overlaps — the reify footprint is a provable
    superset of the default path footprint.

    RED with step-4 impl: workspace-global-file (_ALL sentinel) and cargo
    metadata failure (fail-wide) are not yet handled; added in step-6.
    Passing in step-4: crate textual conflict, non-crate textual conflict
    (path: members present via step-4), disjoint non-crate (different path:).
    """

    def setUp(self):
        self.det = rod.CrateGraphOverlapDetector(
            metadata_loader=lambda: _FIXTURE
        )

    def test_crate_path_textual_conflict_overlaps(self):
        """Textual conflict on a crate path: same file in both changesets → True."""
        fa = self.det.footprint(["crates/crate-a/src/lib.rs"])
        fb = self.det.footprint(["crates/crate-a/src/lib.rs"])
        self.assertTrue(self.det.overlaps(fa, fb))

    def test_noncrate_path_textual_conflict_overlaps(self):
        """Textual conflict on a non-crate path: docs/*.md in both → True.

        A pure crate-set footprint would give empty members for docs/ files
        (no crate mapping) → overlaps=False, violating the invariant.
        The path:<p> member preserves textual-conflict⇒overlap for all paths.
        """
        fa = self.det.footprint(["docs/guide.md"])
        fb = self.det.footprint(["docs/guide.md"])
        self.assertTrue(
            self.det.overlaps(fa, fb),
            "same non-crate path in both changesets must overlap (superset invariant)",
        )

    def test_disjoint_noncrate_paths_no_overlap(self):
        """Different non-crate paths (docs/a vs docs/b) → False."""
        fa = self.det.footprint(["docs/a.md"])
        fb = self.det.footprint(["docs/b.md"])
        self.assertFalse(self.det.overlaps(fa, fb))

    def test_workspace_global_file_overlaps_crate_change(self):
        """Cargo.toml (workspace-global) overlaps any non-empty footprint."""
        f_global = self.det.footprint(["Cargo.toml"])
        f_crate = self.det.footprint(["crates/crate-a/src/lib.rs"])
        self.assertTrue(self.det.overlaps(f_global, f_crate))
        self.assertTrue(self.det.overlaps(f_crate, f_global))

    def test_workspace_global_file_overlaps_global(self):
        """Two changesets both touching Cargo.toml → True (textual + global)."""
        fa = self.det.footprint(["Cargo.toml"])
        fb = self.det.footprint(["Cargo.lock"])
        self.assertTrue(self.det.overlaps(fa, fb))

    def test_cargo_failure_fail_wide(self):
        """Cargo metadata failure → ALL sentinel → overlaps any non-empty footprint."""
        def raising_loader():
            raise RuntimeError("simulated cargo metadata failure")

        failing_det = rod.CrateGraphOverlapDetector(metadata_loader=raising_loader)
        f_fail = failing_det.footprint(["crates/crate-a/src/lib.rs"])
        f_other = failing_det.footprint(["crates/crate-c/src/lib.rs"])
        self.assertTrue(
            failing_det.overlaps(f_fail, f_other),
            "cargo failure must produce ALL sentinel → overlap with everything non-empty",
        )


# ─── Step-7: Two-way boundary + registration + fail-open cases ───────────────

class TestTwoWayBoundary(unittest.TestCase):
    """Two-way boundary tests proving the γ-contract (step-7).

    (a) Override earns its place: reify detector returns True where the default
        path detector returns False for the same-crate/different-file and
        dependent-crate pairs (the κ headline signal).
    (b) Textual-conflict⇒overlap holds for BOTH detectors on a shared path.
    (c) Registration round-trip via register_for_reify().
    (d) Fail-open via the γ changesets_overlap wrapper.
    """

    def _make_det(self):
        return rod.CrateGraphOverlapDetector(metadata_loader=lambda: _FIXTURE)

    def test_override_earns_place_same_crate_different_files(self):
        """Reify True, default False for same crate / different files."""
        reify_det = self._make_det()
        default_det = ov.DefaultPathOverlapDetector()

        paths_a = ["crates/crate-a/src/lib.rs"]
        paths_b = ["crates/crate-a/src/other.rs"]

        fa_r = reify_det.footprint(paths_a)
        fb_r = reify_det.footprint(paths_b)
        fa_d = default_det.footprint(paths_a)
        fb_d = default_det.footprint(paths_b)

        self.assertTrue(
            reify_det.overlaps(fa_r, fb_r),
            "reify detector must detect same-crate/different-file overlap",
        )
        self.assertFalse(
            default_det.overlaps(fa_d, fb_d),
            "default detector must NOT detect same-crate/different-file overlap",
        )

    def test_override_earns_place_dependent_crate(self):
        """Reify True, default False for B-depends-on-A pair."""
        reify_det = self._make_det()
        default_det = ov.DefaultPathOverlapDetector()

        paths_a = ["crates/crate-a/src/lib.rs"]
        paths_b = ["crates/crate-b/src/lib.rs"]

        fa_r = reify_det.footprint(paths_a)
        fb_r = reify_det.footprint(paths_b)
        fa_d = default_det.footprint(paths_a)
        fb_d = default_det.footprint(paths_b)

        self.assertTrue(reify_det.overlaps(fa_r, fb_r))
        self.assertFalse(default_det.overlaps(fa_d, fb_d))

    def test_textual_conflict_both_detectors_true(self):
        """Shared path: both reify and default return True (superset invariant)."""
        reify_det = self._make_det()
        default_det = ov.DefaultPathOverlapDetector()
        shared = ["crates/crate-a/src/lib.rs"]

        fa_r = reify_det.footprint(shared)
        fb_r = reify_det.footprint(shared)
        fa_d = default_det.footprint(shared)
        fb_d = default_det.footprint(shared)

        self.assertTrue(reify_det.overlaps(fa_r, fb_r))
        self.assertTrue(default_det.overlaps(fa_d, fb_d))

    def test_registration_round_trip(self):
        """register_for_reify() registers a CrateGraphOverlapDetector under 'reify'."""
        # Snapshot and restore _DETECTORS to avoid cross-test contamination.
        original = dict(ov._DETECTORS)
        try:
            rod.register_for_reify()
            registered = ov.get_overlap_detector("reify")
            self.assertIsInstance(registered, rod.CrateGraphOverlapDetector)
        finally:
            ov._DETECTORS.clear()
            ov._DETECTORS.update(original)

    def test_unregistered_projects_use_default(self):
        """get_overlap_detector for unregistered project returns DEFAULT."""
        # Restore state after registration from previous test.
        original = dict(ov._DETECTORS)
        try:
            self.assertIs(
                ov.get_overlap_detector("dark_factory"),
                ov.DEFAULT_OVERLAP_DETECTOR,
            )
            self.assertIs(
                ov.get_overlap_detector(None),
                ov.DEFAULT_OVERLAP_DETECTOR,
            )
        finally:
            ov._DETECTORS.clear()
            ov._DETECTORS.update(original)

    def test_changesets_overlap_routes_through_registered_detector(self):
        """changesets_overlap routes through a registered CrateGraphOverlapDetector.

        Uses a fixture-based detector (not real cargo) so the test is hermetic
        and independent of the real reify workspace crate set.
        """
        original = dict(ov._DETECTORS)
        try:
            # Register a fixture-based detector under a test-only project id.
            fixture_det = rod.CrateGraphOverlapDetector(metadata_loader=lambda: _FIXTURE)
            ov.register_overlap_detector("reify-routing-test", fixture_det)

            # Same-crate/different-file: our detector → True.
            result = ov.changesets_overlap(
                "reify-routing-test",
                ["crates/crate-a/src/lib.rs"],
                ["crates/crate-a/src/other.rs"],
            )
            self.assertTrue(
                result,
                "routing through CrateGraphOverlapDetector must detect same-crate overlap",
            )

            # Default detector for same paths → False (proves routing matters).
            default_result = ov.changesets_overlap(
                "unregistered-xyz",
                ["crates/crate-a/src/lib.rs"],
                ["crates/crate-a/src/other.rs"],
            )
            self.assertFalse(
                default_result,
                "default path detector must return False for same-crate/different-file",
            )
        finally:
            ov._DETECTORS.clear()
            ov._DETECTORS.update(original)

    def test_changesets_overlap_fail_open_on_detector_exception(self):
        """changesets_overlap is fail-open: detector exception → True."""
        original = dict(ov._DETECTORS)
        try:
            def raising_loader():
                raise RuntimeError("injected failure")

            ov.register_overlap_detector(
                "reify-test-fail",
                rod.CrateGraphOverlapDetector(metadata_loader=raising_loader),
            )
            result = ov.changesets_overlap(
                "reify-test-fail",
                ["crates/crate-a/src/lib.rs"],
                ["crates/crate-c/src/lib.rs"],
            )
            self.assertTrue(result, "changesets_overlap must be fail-open → True on error")
        finally:
            ov._DETECTORS.clear()
            ov._DETECTORS.update(original)


# ─── Step-9: Unresolvable-crate fail-wide (dropped-crate invariant) ──────────

# Second fixture: "crate-excluded" appears in packages but is NOT a workspace
# member (workspace-excluded case, distinct from absent-from-metadata).
_EXCLUDED_ID = (
    "crate-excluded 0.1.0 (path+file:///workspace/crates/crate-excluded)"
)
_FIXTURE_WITH_EXCLUDED = {
    "packages": [
        {"id": _A_ID, "name": "crate-a"},
        {"id": _EXCLUDED_ID, "name": "crate-excluded"},
    ],
    "workspace_members": [_A_ID],  # NOTE: _EXCLUDED_ID intentionally absent
    "resolve": {
        "nodes": [
            {"id": _A_ID, "deps": []},
        ]
    },
}


class TestUnresolvableCrateFailWide(unittest.TestCase):
    """Unresolvable seed crate → _ALL sentinel (fail-wide), not silent drop (step-9).

    Root cause being tested:
      _file_to_crate returns a non-None crate name that is NOT in workspace_members.
      The current (step-8) code: _reverse_closure returns empty set (the crate name
      maps to no workspace-member IDs), so footprint() emits NO member for that
      path — it is silently dropped.  The result is an empty Footprint, which
      overlaps() reports as disjoint (False), violating the textual-conflict⇒overlap
      invariant (PRD §5.1) and the C5 fail-wide rule.

    Expected (post-step-10) behaviour: when any seed crate name does not appear in
    the closure returned by _reverse_closure, the _ALL sentinel is added to members
    (fail-wide), guaranteeing the invariant for crate-mapped paths just as path:
    members guarantee it for non-crate paths.
    """

    def setUp(self):
        # _FIXTURE has known crates {crate-a, crate-b, crate-c, reify-gui}.
        # "newcrate" is absent from packages / workspace_members entirely.
        self.det = rod.CrateGraphOverlapDetector(
            metadata_loader=lambda: _FIXTURE
        )

    # ── (a) Same unknown path → must overlap (textual⇒overlap on crate-mapped path) ──

    def test_same_unknown_crate_path_overlaps(self):
        """Two footprints over the identical unresolvable-crate path must overlap.

        crates/newcrate/src/lib.rs appears in both changesets → True.
        Fails RED with step-8: both footprints are empty → overlaps=False.
        """
        fa = self.det.footprint(["crates/newcrate/src/lib.rs"])
        fb = self.det.footprint(["crates/newcrate/src/lib.rs"])
        self.assertTrue(
            self.det.overlaps(fa, fb),
            "identical unresolvable-crate path in both changesets must overlap"
            " (textual-conflict⇒overlap invariant for crate-mapped paths)",
        )

    # ── (b) Same unknown crate, different files → must overlap ───────────────

    def test_same_unknown_crate_different_files_overlap(self):
        """Same unresolvable crate, different files → must overlap (True).

        Fails RED: both footprints are empty → overlaps=False.
        """
        fa = self.det.footprint(["crates/newcrate/src/lib.rs"])
        fb = self.det.footprint(["crates/newcrate/src/other.rs"])
        self.assertTrue(
            self.det.overlaps(fa, fb),
            "same unresolvable crate / different files must overlap",
        )

    # ── (c) Unknown-crate footprint overlaps UNRELATED known-crate footprint ─

    def test_unknown_crate_overlaps_unrelated_known_crate(self):
        """Unresolvable-crate footprint must overlap an unrelated known-crate footprint.

        crates/newcrate/... vs crates/crate-c/... → True.
        This proves the fallback is the _ALL sentinel (fail-wide), not a per-path
        member: if we used path:crates/newcrate/src/lib.rs and crate:crate-c they
        would be disjoint (different members, no intersection), but _ALL ∩ any
        non-empty footprint → True.
        Fails RED: newcrate footprint is empty → overlaps=False.
        """
        fa = self.det.footprint(["crates/newcrate/src/lib.rs"])
        fc = self.det.footprint(["crates/crate-c/src/lib.rs"])
        self.assertTrue(
            self.det.overlaps(fa, fc),
            "unresolvable-crate footprint must be fail-wide (_ALL), not empty;"
            " must overlap an unrelated known-crate footprint",
        )

    # ── (d) Workspace-excluded crate (id in packages, not workspace_members) ─

    def test_workspace_excluded_crate_footprints_overlap(self):
        """Workspace-excluded crate → _ALL sentinel → its two footprints overlap.

        Uses _FIXTURE_WITH_EXCLUDED: crate-excluded is in packages but NOT in
        workspace_members.  _reverse_closure visits its ID but the i∈members
        guard filters it out, giving an empty closure — same drop-hole as absent.
        Both footprints must overlap after the step-10 fix.
        Fails RED: same empty-closure → empty footprint → overlaps=False.
        """
        det = rod.CrateGraphOverlapDetector(
            metadata_loader=lambda: _FIXTURE_WITH_EXCLUDED
        )
        fa = det.footprint(["crates/crate-excluded/src/lib.rs"])
        fb = det.footprint(["crates/crate-excluded/src/other.rs"])
        self.assertTrue(
            det.overlaps(fa, fb),
            "workspace-excluded crate must produce fail-wide (_ALL) footprint;"
            " its two footprints must overlap",
        )

    # ── (e) Non-regression: resolvable disjoint pair must NOT fail wide ───────

    def test_resolvable_disjoint_pair_does_not_fail_wide(self):
        """Resolvable disjoint crates (crate-a vs crate-c) must still return False.

        The fix must add _ALL ONLY when a seed crate is unresolvable; it must NOT
        cause every crate-mapped changeset to fail wide.
        """
        fa = self.det.footprint(["crates/crate-a/src/lib.rs"])
        fc = self.det.footprint(["crates/crate-c/src/lib.rs"])
        self.assertFalse(
            self.det.overlaps(fa, fc),
            "resolvable disjoint crates must not fail wide (overlaps must be False)",
        )


if __name__ == "__main__":
    unittest.main()
