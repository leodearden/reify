//! Feature → datum projection (geometric-relations ε).
//!
//! Builds the deduplicated bundle of datums a realized feature projects onto
//! (the "real missing bridge", PRD §7.2 / design §2.2): `feature.<projection>
//! : Datum`, total downward per the datum lattice. The provenance of a bundle
//! is the union of
//!
//!   * **analytic classification** — `BRepAdaptor_*` → `GeomAbs_*` → `.Axis()`
//!     extraction per sub-face / sub-edge (via the kernel's
//!     [`FaceAnalyticDatum`] / [`EdgeAnalyticDatum`] queries), and
//!   * **construction-history datum-traits** — `Revolute → Axis`,
//!     `Extruded → Direction` read from the topology attribute table,
//!
//! canonicalized by geometric equivalence (coaxial / coplanar / coincident
//! merge within `tol = max(confusion_floor, localTol(A), localTol(B))`).
//!
//! This module owns ε's datum-equivalence + dedup primitive so that ζ (which
//! depends on both γ and ε) can reuse the **same** primitive — fulfilling the
//! design §2.3 coherence law by shared code rather than duplicated magic
//! numbers.
//!
//! # Status
//!
//! Pre-2 scaffolding: this is the registered module path the ε GREEN steps
//! (6 / 8 / 10 / 12) fill in. The equivalence predicates (`axes_coaxial`,
//! `planes_coplanar`, `points_coincident`), the `Datum` carrier, `dedup_datums`,
//! `dedup_tolerance`, and `feature_datum_bundle` land in those steps alongside
//! their RED tests.
//!
//! [`FaceAnalyticDatum`]: reify_ir::GeometryQuery::FaceAnalyticDatum
//! [`EdgeAnalyticDatum`]: reify_ir::GeometryQuery::EdgeAnalyticDatum

#[cfg(test)]
mod equivalence_tests {
    use super::*;

    /// Linear dedup tolerance used across the equivalence tests (1 µm) — wide
    /// enough that the `1e-9` within-tolerance perturbations merge and the
    /// macroscopic (`>= 1`) offsets do not.
    const TOL: f64 = 1e-6;

    fn axis(origin: [f64; 3], direction: [f64; 3]) -> AxisGeom {
        AxisGeom { origin, direction }
    }

    fn plane(origin: [f64; 3], normal: [f64; 3]) -> PlaneGeom {
        PlaneGeom { origin, normal }
    }

    // ----- axes_coaxial -----------------------------------------------------

    #[test]
    fn coaxial_same_line_identical_sense() {
        // Both rays lie on the world Z axis, same direction sense, offset only
        // *along* the shared line — the canonical "same axis" case.
        assert!(axes_coaxial(
            axis([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            axis([0.0, 0.0, 5.0], [0.0, 0.0, 1.0]),
            TOL,
        ));
    }

    #[test]
    fn coaxial_same_line_opposite_sense() {
        // B8's load-bearing case: a revolved rectangle's two end-arc circles
        // share the revolution axis but with OPPOSITE direction sense. A
        // sign-sensitive test would leave two axes and fail B8.
        assert!(axes_coaxial(
            axis([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            axis([0.0, 0.0, 5.0], [0.0, 0.0, -1.0]),
            TOL,
        ));
    }

    #[test]
    fn coaxial_within_tolerance_offset_merges() {
        // Reference point off the shared line by < tol still counts as coaxial.
        assert!(axes_coaxial(
            axis([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            axis([1e-9, 0.0, 5.0], [0.0, 0.0, 1.0]),
            TOL,
        ));
    }

    #[test]
    fn not_coaxial_parallel_but_offset() {
        // Identical direction, but the lines are 1 unit apart — distinct
        // parallel axes, must NOT merge.
        assert!(!axes_coaxial(
            axis([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            axis([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            TOL,
        ));
    }

    #[test]
    fn not_coaxial_skew() {
        // Different direction AND offset origin — skew lines.
        assert!(!axes_coaxial(
            axis([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            axis([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            TOL,
        ));
    }

    #[test]
    fn not_coaxial_crossing_at_origin_different_direction() {
        // Both lines pass through the origin (so a point-on-line check alone
        // would pass) but meet at 90° — the angular gate must reject them.
        assert!(!axes_coaxial(
            axis([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            axis([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            TOL,
        ));
    }

    // ----- planes_coplanar --------------------------------------------------

    #[test]
    fn coplanar_same_normal_sense() {
        // Both the z = 0 plane, anchored at different in-plane points.
        assert!(planes_coplanar(
            plane([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            plane([3.0, 4.0, 0.0], [0.0, 0.0, 1.0]),
            TOL,
        ));
    }

    #[test]
    fn coplanar_opposite_normal_sense() {
        // Same z = 0 plane, opposite normal sense — coplanarity is unsigned.
        assert!(planes_coplanar(
            plane([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            plane([3.0, 4.0, 0.0], [0.0, 0.0, -1.0]),
            TOL,
        ));
    }

    #[test]
    fn coplanar_within_tolerance_offset() {
        assert!(planes_coplanar(
            plane([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            plane([3.0, 4.0, 1e-9], [0.0, 0.0, 1.0]),
            TOL,
        ));
    }

    #[test]
    fn not_coplanar_parallel_offset() {
        // Parallel planes z = 0 and z = 2 — equal normal, distinct offset.
        assert!(!planes_coplanar(
            plane([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            plane([0.0, 0.0, 2.0], [0.0, 0.0, 1.0]),
            TOL,
        ));
    }

    #[test]
    fn not_coplanar_tilted() {
        // z = 0 plane vs y = 0 plane through the same point.
        assert!(!planes_coplanar(
            plane([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            plane([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            TOL,
        ));
    }

    // ----- points_coincident ------------------------------------------------

    #[test]
    fn points_coincident_exact_and_within_tolerance() {
        assert!(points_coincident([1.0, 2.0, 3.0], [1.0, 2.0, 3.0], TOL));
        assert!(points_coincident(
            [1.0, 2.0, 3.0],
            [1.0, 2.0, 3.0 + 1e-9],
            TOL,
        ));
    }

    #[test]
    fn points_not_coincident_beyond_tolerance() {
        assert!(!points_coincident([1.0, 2.0, 3.0], [1.0, 2.0, 4.0], TOL));
    }
}
