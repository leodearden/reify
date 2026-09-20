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
//! Modelled on `crates/reify-eval/tests/harness_kernel_realization/kernel_queries_contains.rs` (real-OCCT
//! harness) and `crates/reify-eval/tests/harness_kernel_realization/kernel_queries_angle_smoke.rs` (Scalar
//! epsilon-match assertion shape).

use super::fixture_scaffolding::{assert_length_cell, compile_and_build_with_occt};

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
    let Some(result) = compile_and_build_with_occt(
        DISTANCE_BOX_POINT_PATH,
        "examples/kernel_queries/distance_box_point.ri (task 3610 pre-1)",
    ) else {
        return;
    };

    // Allow a small floating-point epsilon on the si_value while requiring the
    // LENGTH dimension. 15 mm = 0.015 m in SI metres.
    assert_length_cell(&result, "DistanceBoxPoint", "d", 0.015, 1e-9);
}
