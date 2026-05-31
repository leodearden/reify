//! Real-OCCT end-to-end pin test for `distance(Geometry, Point3<Length>) -> Scalar<Length>`
//! (task 3610, PRD `docs/prds/v0_3/kernel-geometry-queries.md` §9 KGQ-α).
//!
//! The fixture `examples/kernel_queries/distance_box_point.ri` contains:
//!
//! ```ri
//! structure def DistanceBoxPoint {
//!     let b = box(10mm, 20mm, 30mm)
//!     let p = point3(20mm, 0mm, 0mm)
//!     let d = distance(b, p)
//! }
//! ```
//!
//! Box geometry: 10 mm × 20 mm × 30 mm centred at origin ⟹ X-faces at ±5 mm
//! (±0.005 m in SI). Closest surface point to `p = (20mm, 0, 0)` is `(5mm, 0, 0)`.
//! Expected distance: `‖(0.020 − 0.005, 0, 0)‖ = 0.015 m = 15 mm`.
//!
//! The compilation check runs unconditionally so a grammar or compile regression
//! fails on every runner. The kernel build + assertion is gated on
//! `reify_kernel_occt::OCCT_AVAILABLE` — skips cleanly on runners without OCCT.
//!
//! Modelled on `crates/reify-eval/tests/kernel_queries_contains.rs` (real-OCCT
//! harness) and `crates/reify-eval/tests/kernel_queries_angle_smoke.rs` (Scalar
//! epsilon-match assertion shape).

use reify_constraints::SimpleConstraintChecker;
use reify_core::ValueCellId;
use reify_ir::{ExportFormat, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

const DISTANCE_BOX_POINT_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kernel_queries/distance_box_point.ri"
);

/// Pins the user-observable signal for KGQ-α: `distance(box(10mm,20mm,30mm),
/// point3(20mm,0mm,0mm))` must evaluate to `Value::Scalar{LENGTH, si_value ≈ 0.015}`.
///
/// The box is centred at origin (X-half-extent 5mm), so the closest surface
/// point to (20mm,0,0) is (5mm,0,0) → distance = 15mm = 0.015m.
///
/// Skips cleanly (via early return) when OCCT is not available.
#[test]
fn distance_box_point_evals_to_15mm() {
    // Read the fixture unconditionally so a missing file is caught even on
    // OCCT-less runners — fixture presence is a CI contract independent of OCCT.
    let source = std::fs::read_to_string(DISTANCE_BOX_POINT_PATH).expect(
        "examples/kernel_queries/distance_box_point.ri should exist (task 3610 pre-1)",
    );

    // Validate fixture compilation unconditionally — a grammar/compile regression
    // (e.g. `distance` signature change) should fail on every runner.
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/kernel_queries/distance_box_point.ri should compile with no \
         error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    // Skip the OCCT-dependent kernel build/value assertion if OCCT is not built.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions: OCCT not available");
        return;
    }

    // Build with real OCCT kernel (SingleKernelHolder + OcctKernelHandle::spawn).
    let checker = SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("DistanceBoxPoint", "d");
    let actual = result.values.get(&cell);

    // Allow a small floating-point epsilon on the si_value while requiring the
    // LENGTH dimension. Modelled on the Scalar epsilon-match in
    // kernel_queries_angle_smoke.rs::angle_smoke_evals_to_ninety_degrees.
    match actual {
        Some(Value::Scalar { si_value, dimension })
            if *dimension == reify_core::DimensionVector::LENGTH =>
        {
            let expected = 0.015_f64; // 15 mm in SI metres
            let epsilon = 1e-9;
            assert!(
                (si_value - expected).abs() < epsilon,
                "DistanceBoxPoint.d si_value should be 0.015 (15 mm), \
                 got {si_value:.15} (delta {delta:.3e})",
                delta = (si_value - expected).abs()
            );
        }
        other => panic!(
            "DistanceBoxPoint.d should be Value::Scalar{{LENGTH, ≈0.015}}, got: {:?}",
            other
        ),
    }
}
