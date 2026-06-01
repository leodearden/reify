//! Real-OCCT end-to-end dispatch pins for the whole-handle geometry queries
//! `volume()` / `area()` / `centroid()` / `bounding_box()` on a
//! `Value::GeometryHandle` (task 3608, GHR-ζ; PRD
//! `docs/prds/v0_3/geometry-handle-runtime.md` §8 Phase 6).
//!
//! Each test compiles an inline DSL structure that realizes a primitive
//! (`box`/`sphere`/`cylinder`) and binds a geometry-query `let` over it, builds
//! the module through a real-OCCT `Engine`, and asserts the resulting value
//! cell is the correct typed `Value` (`Scalar<Volume>` / `Scalar<Area>` /
//! `Point3<Length>` / `BoundingBox`) within an analytic tolerance.
//!
//! The compile-clean assertion runs unconditionally so a grammar/compile
//! regression fails on every runner; the kernel build + numeric assertions are
//! gated on `reify_kernel_occt::OCCT_AVAILABLE` and skip cleanly otherwise
//! (mirrors `kernel_queries_distance_smoke.rs`).
//!
//! **Placement convention:** Reify's `box(w,h,d)` is CENTERED at the origin
//! (`occt_wrapper.cpp` `make_box` uses corner `(-w/2,-h/2,-d/2)`), so
//! `box(10mm,20mm,30mm)` has centroid `(0,0,0)` and bounding box
//! `min(-5,-10,-15)mm` / `max(5,10,15)mm`. Volume and surface area are
//! placement-invariant. (The plan's corner-at-origin premise was a documented
//! assumption to confirm; the centered convention is authoritative here and is
//! consistent with `examples/kernel_queries/distance_box_point.ri`.)

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DimensionVector, ValueCellId};
use reify_ir::{ExportFormat, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

/// Compile `source` (asserting no error-severity diagnostics), then — if OCCT
/// is available — build it through a real-OCCT `Engine` and return the
/// `BuildResult`. Returns `None` when OCCT is unavailable, signalling the caller
/// to skip the numeric assertions.
fn compile_and_build_occt(source: &str) -> Option<reify_eval::BuildResult> {
    let compiled = parse_and_compile_with_stdlib(source);
    assert!(
        errors_only(&compiled).is_empty(),
        "fixture should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions: OCCT not available");
        return None;
    }

    let checker = SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    Some(engine.build(&compiled, ExportFormat::Step))
}

/// Assert `value` is a `Value::Scalar` of dimension `dim` whose `si_value` is
/// within 1e-6 relative of `expected` (which must be non-zero).
fn assert_scalar_rel(value: Option<&Value>, dim: DimensionVector, expected: f64, what: &str) {
    match value {
        Some(Value::Scalar { si_value, dimension }) => {
            assert_eq!(
                *dimension, dim,
                "{what}: expected dimension {dim:?}, got {dimension:?}"
            );
            let rel = (si_value - expected).abs() / expected.abs();
            assert!(
                rel < 1e-6,
                "{what}: si_value {si_value:.12} not within 1e-6 relative of \
                 {expected:.12} (rel={rel:.3e})"
            );
        }
        other => panic!("{what}: expected Value::Scalar{{{dim:?}}}, got {other:?}"),
    }
}

// ── volume() ────────────────────────────────────────────────────────────────

const VOLUME_SOURCE: &str = r#"
structure def VolBox {
    let body = box(10mm, 20mm, 30mm)
    let v = volume(body)
}
structure def VolSphere {
    let body = sphere(10mm)
    let v = volume(body)
}
structure def VolCyl {
    let body = cylinder(10mm, 20mm)
    let v = volume(body)
}
"#;

/// `volume(handle)` dispatches to OCCT and yields `Scalar<Volume>` for box,
/// sphere, and cylinder primitives, matching the analytic volumes:
///   - box(10,20,30)mm  → 0.010·0.020·0.030          = 6.0e-6 m³
///   - sphere(10mm)      → (4/3)π·0.010³              ≈ 4.18879e-6 m³
///   - cylinder(10,20)mm → π·0.010²·0.020             ≈ 6.28319e-6 m³
#[test]
fn volume_dispatch_box_sphere_cylinder() {
    let Some(result) = compile_and_build_occt(VOLUME_SOURCE) else {
        return;
    };

    let box_v = 0.010 * 0.020 * 0.030;
    let sphere_v = (4.0 / 3.0) * std::f64::consts::PI * 0.010_f64.powi(3);
    let cyl_v = std::f64::consts::PI * 0.010_f64.powi(2) * 0.020;

    assert_scalar_rel(
        result.values.get(&ValueCellId::new("VolBox", "v")),
        DimensionVector::VOLUME,
        box_v,
        "volume(box(10,20,30)mm)",
    );
    assert_scalar_rel(
        result.values.get(&ValueCellId::new("VolSphere", "v")),
        DimensionVector::VOLUME,
        sphere_v,
        "volume(sphere(10mm))",
    );
    assert_scalar_rel(
        result.values.get(&ValueCellId::new("VolCyl", "v")),
        DimensionVector::VOLUME,
        cyl_v,
        "volume(cylinder(10mm,20mm))",
    );
}
