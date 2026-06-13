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

/// SI-metre fallback length scale used to convert the linear dedup tolerance
/// into a scale-aware angular tolerance (design §2.3:
/// `ang_tol ≈ lin_tol / characteristic_length`, deliberately **not** OCCT's
/// `Precision::Angular`).
///
/// The characteristic length is the separation of the two datums' reference
/// points: an angular misalignment `θ` between two would-be-coincident datums
/// produces a linear drift of about `separation · θ` at those points, so
/// allowing `lin_tol` of drift permits `θ ≤ lin_tol / separation`. When the
/// reference points are (near-)coincident the separation cannot supply the
/// scale, so it is floored at this value (1 m, the SI unit) instead of dividing
/// by zero. The companion point-on-line / point-on-plane *linear* check is the
/// precise positional arbiter; this angular term is the parallelism gate that
/// rejects datums meeting at a point but oriented differently.
const ANGULAR_REFERENCE_LENGTH_M: f64 = 1.0;

/// Pure geometric core of an axis datum (an infinite line): a reference point
/// and a direction, both in SI-metre kernel coordinates. The direction is a
/// *line orientation* — compared up to sign — not an oriented ray.
///
/// Distinct from the richer `Datum` carrier (step-8), which also retains the
/// projected radius / provenance; this is just the geometry the equivalence
/// predicates compute over.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct AxisGeom {
    pub(crate) origin: [f64; 3],
    pub(crate) direction: [f64; 3],
}

/// Pure geometric core of a plane datum: a reference point and a normal, both
/// in SI-metre kernel coordinates. The normal is compared up to sign (an
/// *unsigned* plane), so a plane and its flip are coplanar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlaneGeom {
    pub(crate) origin: [f64; 3],
    pub(crate) normal: [f64; 3],
}

#[inline]
fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[inline]
fn norm3(a: [f64; 3]) -> f64 {
    dot3(a, a).sqrt()
}

#[inline]
fn dist3(a: [f64; 3], b: [f64; 3]) -> f64 {
    norm3(sub3(a, b))
}

/// Normalize to unit length; a (near-)zero vector is returned unchanged so the
/// downstream checks degrade predictably rather than producing NaNs. Datum
/// directions arriving from the analytic FFI are already unit, so this is
/// defensive.
#[inline]
fn normalize3(a: [f64; 3]) -> [f64; 3] {
    let m = norm3(a);
    if m < f64::EPSILON {
        a
    } else {
        [a[0] / m, a[1] / m, a[2] / m]
    }
}

/// Scale-aware angular tolerance for a comparison whose reference points are
/// `separation` apart — see [`ANGULAR_REFERENCE_LENGTH_M`].
#[inline]
fn angular_tol(separation: f64, lin_tol: f64) -> f64 {
    lin_tol / separation.max(ANGULAR_REFERENCE_LENGTH_M)
}

/// Two axes are **coaxial** iff they lie on the same infinite line: their
/// directions are parallel *up to sign* (within the scale-aware angular tol)
/// AND each reference point lies on the other's line within `lin_tol`.
///
/// Sign-insensitive by construction — `|da × db|` ignores the relative sense —
/// which is the property B8 relies on (a revolved rectangle's two end-arc
/// circles carry opposite-sense axis directions on one shared line).
pub(crate) fn axes_coaxial(a: AxisGeom, b: AxisGeom, lin_tol: f64) -> bool {
    let da = normalize3(a.direction);
    let db = normalize3(b.direction);
    // (1) Directions parallel up to sign: |da × db| = sin(angle) for unit dirs.
    let separation = dist3(a.origin, b.origin);
    if norm3(cross3(da, db)) > angular_tol(separation, lin_tol) {
        return false;
    }
    // (2) Each reference point lies on the other's infinite line: the
    //     perpendicular distance |Δ × dir| (dir unit) is within the linear tol.
    let perp_b = norm3(cross3(sub3(b.origin, a.origin), da));
    let perp_a = norm3(cross3(sub3(a.origin, b.origin), db));
    perp_b <= lin_tol && perp_a <= lin_tol
}

/// Two planes are **coplanar** iff they are the same unsigned plane: their
/// normals are parallel *up to sign* (within the scale-aware angular tol) AND a
/// reference point of one lies on the other within `lin_tol` (the perpendicular
/// point-to-plane distance `|Δ · n|`, itself sign-insensitive).
pub(crate) fn planes_coplanar(a: PlaneGeom, b: PlaneGeom, lin_tol: f64) -> bool {
    let na = normalize3(a.normal);
    let nb = normalize3(b.normal);
    let separation = dist3(a.origin, b.origin);
    if norm3(cross3(na, nb)) > angular_tol(separation, lin_tol) {
        return false;
    }
    dot3(sub3(b.origin, a.origin), na).abs() <= lin_tol
}

/// Two points are **coincident** iff their separation is within `lin_tol`.
pub(crate) fn points_coincident(a: [f64; 3], b: [f64; 3], lin_tol: f64) -> bool {
    dist3(a, b) <= lin_tol
}

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

#[cfg(test)]
mod dedup_tests {
    use super::*;

    const TOL: f64 = 1e-6;

    fn ax(origin: [f64; 3], direction: [f64; 3]) -> Datum {
        Datum::Axis {
            origin,
            direction,
            radius: None,
        }
    }

    fn pl(origin: [f64; 3], normal: [f64; 3]) -> Datum {
        Datum::Plane { origin, normal }
    }

    fn count_axes(ds: &[Datum]) -> usize {
        ds.iter().filter(|d| matches!(d, Datum::Axis { .. })).count()
    }

    fn count_planes(ds: &[Datum]) -> usize {
        ds.iter()
            .filter(|d| matches!(d, Datum::Plane { .. }))
            .count()
    }

    #[test]
    fn three_coaxial_axes_collapse_to_one() {
        // Three rays on the world Z axis — identical sense, opposite sense, and
        // offset-along-the-line — are one axis. This is B8's dedup in miniature
        // (the opposite-sense member is the revolved rectangle's far end-arc).
        let ds = dedup_datums(
            vec![
                ax([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
                ax([0.0, 0.0, 2.0], [0.0, 0.0, -1.0]),
                ax([0.0, 0.0, -3.0], [0.0, 0.0, 1.0]),
            ],
            TOL,
        );
        assert_eq!(ds.len(), 1, "three coaxial axes must collapse to one");
        assert_eq!(count_axes(&ds), 1);
    }

    #[test]
    fn distinct_axes_do_not_merge() {
        // A genuine disagreement: the Z axis vs an offset, perpendicular X axis.
        // Dedup must not paper over a real conflict by over-merging.
        let ds = dedup_datums(
            vec![
                ax([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
                ax([1.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            ],
            TOL,
        );
        assert_eq!(ds.len(), 2, "non-coaxial axes must stay distinct");
    }

    #[test]
    fn duplicate_coplanar_planes_collapse_to_one() {
        // Same z = 0 plane, opposite normal sense + different in-plane anchor.
        let ds = dedup_datums(
            vec![
                pl([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
                pl([5.0, 7.0, 0.0], [0.0, 0.0, -1.0]),
            ],
            TOL,
        );
        assert_eq!(ds.len(), 1, "coplanar planes must collapse to one");
        assert_eq!(count_planes(&ds), 1);
    }

    #[test]
    fn axes_and_planes_never_cross_merge() {
        // An axis and a plane sharing origin + Z geometry are different kinds:
        // dedup is kind-partitioned, so both survive.
        let ds = dedup_datums(
            vec![
                ax([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
                pl([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            ],
            TOL,
        );
        assert_eq!(ds.len(), 2, "axis and plane must not cross-merge");
        assert_eq!(count_axes(&ds), 1);
        assert_eq!(count_planes(&ds), 1);
    }
}
