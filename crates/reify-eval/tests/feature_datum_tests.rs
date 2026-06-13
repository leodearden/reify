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

use reify_core::{DiagnosticCode, Severity};
use reify_eval::SweptKind;
use reify_eval::feature_datum::{Datum, feature_datum_bundle, feature_datum_projection};
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

// ─── Feature → datum PROJECTION (step-15) ─────────────────────────────────────
//
// `feature_datum_projection(bundle, member, diags)` is the resolve-time refinement
// step the `feature.<proj>` eval performs over a realized feature's deduplicated
// [`FeatureDatumBundle`] (geometric-relations ε, design §7.2): it selects the
// projection group named by `member` (`axis` / `plane` / `point` / `dir`) and
//
//   * **exactly one** datum in the group ⇒ returns that datum as its runtime
//     `Value` (the unambiguous arm of the `Axis | Axis?` refinement), with NO
//     diagnostic; or
//   * **zero or many** ⇒ the ambiguous arm: pushes a select-a-subfeature
//     diagnostic (`DiagnosticCode::FeatureDatumAmbiguous`, `Severity::Error` —
//     a user-actionable resolve-time ambiguity, mirroring the fillet
//     empty-selection precedent) and returns `Value::Undef`.
//
// These tests drive the projection directly over bundles built by
// `feature_datum_bundle` with a staged [`MockGeometryKernel`] (+ optional
// [`SweptKind`] history), exercising BOTH arms of the refinement. The
// receiver-resolution wiring (`feature.axis` member-access → feature handle →
// bundle) is the concern of the end-to-end `.ri` example step (17 / 18); here we
// pin the select-one-or-diagnose core the eval depends on.

/// Assert a `Value::Axis` lies on the world Z line: origin x ≈ y ≈ 0 and
/// direction parallel to ±Z (|z| ≈ 1, x ≈ y ≈ 0) — the runtime-`Value` analogue
/// of [`assert_axis_is_z_line`].
fn assert_value_axis_is_z_line(v: &Value) {
    match v {
        Value::Axis { origin, direction } => {
            let o = match origin.as_ref() {
                Value::Point(c) if c.len() == 3 => [
                    c[0].as_f64().expect("axis origin x is numeric"),
                    c[1].as_f64().expect("axis origin y is numeric"),
                    c[2].as_f64().expect("axis origin z is numeric"),
                ],
                other => panic!("axis origin must be a 3-component Point, got {other:?}"),
            };
            let d = match direction.as_ref() {
                Value::Direction { x, y, z } => [*x, *y, *z],
                other => panic!("axis direction must be a Direction, got {other:?}"),
            };
            assert!(
                o[0].abs() < 1e-9 && o[1].abs() < 1e-9,
                "projected axis origin must lie on the world Z line, got {o:?}"
            );
            assert!(
                d[0].abs() < 1e-9 && d[1].abs() < 1e-9 && (d[2].abs() - 1.0).abs() < 1e-9,
                "projected axis direction must be parallel to ±Z, got {d:?}"
            );
        }
        other => panic!("expected Value::Axis, got {other:?}"),
    }
}

/// Build the revolved-rectangle cylinder's feature → datum bundle (the staging of
/// `revolved_cylinder_bundle_unions_analytic_and_history_to_one_axis`): the
/// analytic end-arc axes (±Z) ∪ the `Revolve`-history axis dedup to exactly ONE
/// coaxial Z axis. Factored so the projection test reuses the same realized-feature
/// fixture the bundle test pins.
fn revolved_cylinder_bundle() -> reify_eval::feature_datum::FeatureDatumBundle {
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
        .with_face_analytic_datum_result(cap_top, plane_value([0.0, 0.0, h], [0.0, 0.0, 1.0]))
        .with_face_analytic_datum_result(cap_bottom, plane_value([0.0, 0.0, -h], [0.0, 0.0, 1.0]))
        .with_edge_analytic_datum_result(arc_top, axis_value([0.0, 0.0, h], [0.0, 0.0, 1.0]))
        .with_edge_analytic_datum_result(arc_bottom, axis_value([0.0, 0.0, -h], [0.0, 0.0, -1.0]));

    let history = SweptKind::Revolve {
        axis_origin: [0.0, 0.0, 0.0],
        axis_dir: [0.0, 0.0, 1.0],
        angle_rad: 2.0 * std::f64::consts::PI,
    };

    feature_datum_bundle(feature, &mut kernel, Some(&history))
}

/// The unambiguous arm: a realized revolved cylinder's bundle carries exactly one
/// axis datum, so `cyl.axis` projects to that single `Value::Axis` (≈ the
/// revolution axis) with no diagnostic. This is B8's refinement in miniature.
#[test]
fn cylinder_axis_projection_resolves_to_single_axis() {
    let bundle = revolved_cylinder_bundle();
    // Precondition the projection relies on (already pinned by the bundle test).
    assert_eq!(bundle.axes.len(), 1, "fixture must dedup to one axis");

    let mut diagnostics = Vec::new();
    let projected = feature_datum_projection(&bundle, "axis", &mut diagnostics);

    assert_value_axis_is_z_line(&projected);
    assert!(
        diagnostics.is_empty(),
        "an unambiguous feature.axis must emit no diagnostic; got {diagnostics:?}"
    );
}

/// The ambiguous arm: a box-like feature exposes several non-coaxial straight
/// edges, so its bundle carries multiple axis candidates. `plate.axis` cannot
/// refine to a single `Axis`, so the projection emits a select-a-subfeature
/// diagnostic (`FeatureDatumAmbiguous`, `Severity::Error`) and evaluates to
/// `Value::Undef`.
#[test]
fn box_axis_projection_is_ambiguous_select_a_subfeature() {
    let feature = GeometryHandleId(3);
    let edge_x = GeometryHandleId(30);
    let edge_y = GeometryHandleId(31);

    // Two genuinely non-coaxial straight edges (an X-axis edge and an offset
    // Y-axis edge): a box's many parallel-but-offset / perpendicular edges, in
    // miniature. No construction history (a box is a primitive, not a swept body).
    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(feature, vec![])
        .with_extracted_edges(feature, vec![edge_x, edge_y])
        .with_edge_analytic_datum_result(edge_x, axis_value([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]))
        .with_edge_analytic_datum_result(edge_y, axis_value([0.0, 1.0, 0.0], [0.0, 1.0, 0.0]));

    let bundle = feature_datum_bundle(feature, &mut kernel, None);
    // Precondition: the two edges are distinct axes (no over-merge).
    assert_eq!(
        bundle.axes.len(),
        2,
        "fixture must expose two non-coaxial axis candidates; got {:?}",
        bundle.axes
    );

    let mut diagnostics = Vec::new();
    let projected = feature_datum_projection(&bundle, "axis", &mut diagnostics);

    assert_eq!(
        projected,
        Value::Undef,
        "an ambiguous feature.axis must evaluate to Undef"
    );
    let ambiguous: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error && d.code == Some(DiagnosticCode::FeatureDatumAmbiguous)
        })
        .collect();
    assert_eq!(
        ambiguous.len(),
        1,
        "an ambiguous feature.axis must emit exactly one FeatureDatumAmbiguous \
         (select-a-subfeature) error; got {diagnostics:?}"
    );
}

// ─── B8 end-to-end over REAL OCCT geometry (step-17 / step-18) ────────────────
//
// The worked-example boundary test (PRD §7.2 / design §2.2): the
// `examples/geometric_relations/feature_datum_axis.ri` cylinder is built by
// revolving a rectangle 360° about a known axis through a real OCCT kernel, and
// `cyl.axis` must refine to EXACTLY ONE `Value::Axis` equal to that revolution
// axis within dedup tolerance — no select-a-subfeature ambiguity.
//
// This is the OCCT-backed half of the example test; the OCCT-free compile-clean
// half is `crates/reify-compiler/tests/feature_datum_axis_example_tests.rs`. It
// lives HERE (not the compiler crate) because realizing the revolve needs an
// OCCT engine and reify-compiler is intentionally not an OCCT-touching crate
// (scripts/occt-touching-crates.txt); see esc-4385-134.

/// Absolute path to the B8 example, resolved from this crate's manifest dir.
const FEATURE_DATUM_AXIS_EXAMPLE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/geometric_relations/feature_datum_axis.ri"
);

/// B8: `cyl.axis` over the realized revolved-rectangle cylinder resolves to
/// exactly one `Value::Axis` coaxial with the example's revolution axis (the
/// line through (-10 mm, 0, 0) along +Y), with no `FeatureDatumAmbiguous`
/// diagnostic. Drives the full source → compile → OCCT build → feature-datum
/// post-process path; the analytic end-arc circle axes ∪ the `Revolve` history
/// axis all dedup to the single revolution axis.
///
/// RED until step-18 creates the `.ri` fixture (panics on the missing read).
#[test]
fn feature_datum_axis_example_resolves_to_single_revolution_axis() {
    use reify_core::{ModulePath, Severity, ValueCellId};
    use reify_ir::ExportFormat;

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping feature_datum_axis B8: OCCT not available");
        return;
    }

    let source = std::fs::read_to_string(FEATURE_DATUM_AXIS_EXAMPLE).expect(
        "failed to read examples/geometric_relations/feature_datum_axis.ri — \
         created by step-18",
    );

    let parsed = reify_syntax::parse(&source, ModulePath::single("feature_datum_axis"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = reify_compiler::compile(&parsed);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(compile_errors.is_empty(), "compile errors: {:#?}", compile_errors);

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // No select-a-subfeature ambiguity for the unambiguous cylinder.
    let ambiguous: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::FeatureDatumAmbiguous))
        .collect();
    assert!(
        ambiguous.is_empty(),
        "cyl.axis over a revolved cylinder must be unambiguous, got: {ambiguous:?}"
    );

    // `let a : Axis = cyl.axis` resolves to exactly one Axis = the revolution axis.
    let cell = ValueCellId::new("Cyl", "a");
    let axis = result.values.get(&cell).unwrap_or_else(|| {
        panic!(
            "Cyl.a must resolve to a value (the projected revolution axis); \
             build diagnostics: {:?}",
            result.diagnostics
        )
    });

    // Known revolution axis: origin on the line {x = -10 mm, z = 0}, direction ±Y.
    const TOL: f64 = 1e-6; // 1 µm — within dedup tol; empirical values are exact.
    match axis {
        Value::Axis { origin, direction } => {
            let o = match origin.as_ref() {
                Value::Point(c) if c.len() == 3 => [
                    c[0].as_f64().expect("origin x numeric"),
                    c[1].as_f64().expect("origin y numeric"),
                    c[2].as_f64().expect("origin z numeric"),
                ],
                other => panic!("axis origin must be a 3-component Point, got {other:?}"),
            };
            let d = match direction.as_ref() {
                Value::Direction { x, y, z } => [*x, *y, *z],
                other => panic!("axis direction must be a Direction, got {other:?}"),
            };
            // Direction parallel to ±Y (sign-insensitive coaxiality).
            assert!(
                d[0].abs() < TOL && d[2].abs() < TOL && (d[1].abs() - 1.0).abs() < TOL,
                "projected axis direction must be parallel to ±Y, got {d:?}"
            );
            // Origin lies on the revolution line {x = -0.01 m, z = 0} (y free).
            assert!(
                (o[0] - (-0.01)).abs() < TOL && o[2].abs() < TOL,
                "projected axis origin must lie on the line x=-10mm, z=0, got {o:?}"
            );
        }
        other => panic!("Cyl.a must be a Value::Axis, got {other:?}"),
    }
}
