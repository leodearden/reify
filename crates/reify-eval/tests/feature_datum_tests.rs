//! Feature → datum *bundle* tests (geometric-relations ε, step-11).
//!
//! Exercises [`reify_eval::feature_datum::feature_datum_bundle`] — the function
//! that gathers a realized feature's datum candidates from the **union** of
//!
//!   * **analytic classification** — per sub-face / sub-edge
//!     `FaceAnalyticDatum` / `EdgeAnalyticDatum` kernel queries, and
//!   * **construction-history datum-traits** — `Revolve → Axis`,
//!     `Extrude → Direction`,
//!
//! then deduplicates each projection group by geometric equivalence (design
//! §2.3). These tests drive the function directly with a staged
//! [`MockGeometryKernel`] (no engine build needed) plus an explicit
//! [`SweptKind`] standing in for the realization's recovered construction
//! history; the kernel-end-to-end `cylinder.axis` projection over real OCCT
//! geometry is the concern of the later eval / example steps (15 / 17).
//!
//! ## Construction-history source (deviation note)
//!
//! The revolution axis / extrusion direction *geometry* is NOT carried by the
//! `TopologyAttributeTable` / `AttributeHistory` (those record only role
//! markers — `RevolvedFace`, `Cap`, `Side`). The post-build source that
//! actually carries the axis origin/direction is the
//! [`SweptKindTable`](reify_eval::SweptKindTable) (`SweptKind::Revolve { axis_origin,
//! axis_dir, .. }` / `SweptKind::Extrude { axis, .. }`), recovered from the
//! `GeometryOp` stream at realization time. `feature_datum_bundle` therefore
//! takes the recovered [`SweptKind`] as its construction-history input — a
//! faithful realization of the plan's "Revolve→Axis / Extrude→Direction"
//! contract via the table that genuinely holds that geometry.

use reify_eval::SweptKind;
use reify_eval::feature_datum::{Datum, feature_datum_bundle};
use reify_ir::{GeometryHandleId, Value};
use reify_test_support::MockGeometryKernel;

/// Build a `Value::Axis` from an origin point and a (possibly non-unit)
/// direction, mirroring what the OCCT kernel's `EdgeAnalyticDatum` dispatch
/// composes for a circle / line edge.
fn axis_value(origin: [f64; 3], dir: [f64; 3]) -> Value {
    Value::Axis {
        origin: Box::new(Value::Point(vec![
            Value::length(origin[0]),
            Value::length(origin[1]),
            Value::length(origin[2]),
        ])),
        direction: Box::new(Value::Direction {
            x: dir[0],
            y: dir[1],
            z: dir[2],
        }),
    }
}

/// Build a `Value::Plane` from an origin point and a normal, mirroring the
/// OCCT kernel's `FaceAnalyticDatum` dispatch for a planar face.
fn plane_value(origin: [f64; 3], normal: [f64; 3]) -> Value {
    Value::Plane {
        origin: Box::new(Value::Point(vec![
            Value::length(origin[0]),
            Value::length(origin[1]),
            Value::length(origin[2]),
        ])),
        normal: Box::new(Value::Direction {
            x: normal[0],
            y: normal[1],
            z: normal[2],
        }),
    }
}

/// Assert a `Datum::Axis` is coaxial with the world Z axis: origin on the Z
/// line (x ≈ y ≈ 0) and direction parallel to ±Z (|z| ≈ 1, x ≈ y ≈ 0).
fn assert_axis_is_z_line(d: &Datum) {
    match d {
        Datum::Axis {
            origin, direction, ..
        } => {
            assert!(
                origin[0].abs() < 1e-9 && origin[1].abs() < 1e-9,
                "surviving axis origin must lie on the world Z line, got {origin:?}"
            );
            assert!(
                direction[0].abs() < 1e-9
                    && direction[1].abs() < 1e-9
                    && (direction[2].abs() - 1.0).abs() < 1e-9,
                "surviving axis direction must be parallel to ±Z, got {direction:?}"
            );
        }
        other => panic!("expected a Datum::Axis, got {other:?}"),
    }
}

/// A revolved-rectangle cylinder's feature → datum bundle gathers axis
/// candidates from BOTH the analytic end-arc edges AND the construction-history
/// Revolve trait, and dedups them to **exactly one** Axis (B8's dedup, in
/// miniature).
///
/// Staging models a revolve about the world Z axis:
///   * side face (`GeomAbs_SurfaceOfRevolution`, "Other") — NON-analytic, so no
///     `FaceAnalyticDatum` is staged; the query misses and the face contributes
///     no analytic axis (the exact non-analytic-tail case the history trait
///     exists to cover);
///   * two cap faces — planar, `z = ±h`, opposite parallel planes;
///   * two end-arc edges — `GeomAbs_Circle`, axis on Z with OPPOSITE direction
///     sense (the load-bearing sign-insensitive merge);
///   * construction history — `SweptKind::Revolve` about Z.
///
/// All three axis candidates (two analytic arcs ±Z + one history Z) are
/// coaxial → dedup → 1. The two caps are distinct parallel planes → 2.
#[test]
fn revolved_cylinder_bundle_unions_analytic_and_history_to_one_axis() {
    let feature = GeometryHandleId(1);
    let side_face = GeometryHandleId(10);
    let cap_top = GeometryHandleId(11);
    let cap_bottom = GeometryHandleId(12);
    let arc_top = GeometryHandleId(20);
    let arc_bottom = GeometryHandleId(21);

    let h = 0.005; // 5 mm half-height

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(feature, vec![side_face, cap_top, cap_bottom])
        .with_extracted_edges(feature, vec![arc_top, arc_bottom])
        // side_face: GeomAbs_SurfaceOfRevolution → non-analytic; intentionally
        // NOT staged so `FaceAnalyticDatum(side_face)` misses (→ skipped).
        .with_face_analytic_datum_result(cap_top, plane_value([0.0, 0.0, h], [0.0, 0.0, 1.0]))
        .with_face_analytic_datum_result(
            cap_bottom,
            plane_value([0.0, 0.0, -h], [0.0, 0.0, 1.0]),
        )
        // Two end-arc circles: coaxial on Z with OPPOSITE direction sense.
        .with_edge_analytic_datum_result(arc_top, axis_value([0.0, 0.0, h], [0.0, 0.0, 1.0]))
        .with_edge_analytic_datum_result(
            arc_bottom,
            axis_value([0.0, 0.0, -h], [0.0, 0.0, -1.0]),
        );

    let history = SweptKind::Revolve {
        axis_origin: [0.0, 0.0, 0.0],
        axis_dir: [0.0, 0.0, 1.0],
        angle_rad: 2.0 * std::f64::consts::PI,
    };

    let bundle = feature_datum_bundle(feature, &mut kernel, Some(&history));

    assert_eq!(
        bundle.axes.len(),
        1,
        "analytic end-arc axes (±Z) ∪ the Revolve-history axis must dedup to \
         exactly ONE coaxial axis; got {:?}",
        bundle.axes
    );
    assert_axis_is_z_line(&bundle.axes[0]);

    // The two caps are distinct parallel planes (z = +h, z = -h) → not merged.
    assert_eq!(
        bundle.planes.len(),
        2,
        "the two opposite cap planes are parallel-but-offset and must NOT merge; \
         got {:?}",
        bundle.planes
    );
    assert!(
        bundle.directions.is_empty(),
        "a revolve contributes no Direction trait; got {:?}",
        bundle.directions
    );
}

/// An extruded solid's feature → datum bundle includes an
/// `Extrude → Direction` construction-history trait.
///
/// Staging isolates the history path: the feature exposes no analytic
/// sub-shapes (empty face / edge extraction), so the only bundle contribution
/// is the `SweptKind::Extrude` direction (+Z). Pins that the bundle surfaces
/// the extrusion direction as a first-class `Datum::Direction`.
#[test]
fn extruded_solid_bundle_includes_extrude_direction_trait() {
    let feature = GeometryHandleId(2);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(feature, vec![])
        .with_extracted_edges(feature, vec![]);

    let history = SweptKind::Extrude {
        axis: [0.0, 0.0, 1.0],
        length: Value::length(0.01),
    };

    let bundle = feature_datum_bundle(feature, &mut kernel, Some(&history));

    assert_eq!(
        bundle.directions.len(),
        1,
        "an extruded solid's bundle must include exactly one Extrude→Direction \
         trait; got {:?}",
        bundle.directions
    );
    match &bundle.directions[0] {
        Datum::Direction { direction } => {
            assert!(
                direction[0].abs() < 1e-9
                    && direction[1].abs() < 1e-9
                    && (direction[2] - 1.0).abs() < 1e-9,
                "extrusion direction must be +Z, got {direction:?}"
            );
        }
        other => panic!("expected a Datum::Direction, got {other:?}"),
    }
    assert!(
        bundle.axes.is_empty() && bundle.planes.is_empty() && bundle.points.is_empty(),
        "extrude bundle (no analytic sub-shapes staged) must carry only the \
         Direction trait; got axes={:?} planes={:?} points={:?}",
        bundle.axes,
        bundle.planes,
        bundle.points
    );
}
