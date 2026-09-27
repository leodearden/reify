//! Real-OCCT end-to-end pin test for `contains(Solid, Point3<Length>) -> Bool`
//! (task 3611, PRD `docs/prds/v0_3/kernel-geometry-queries.md` §9 KGQ-β).
//!
//! The fixture `examples/kernel_queries/contains_box.ri` contains:
//!
//! ```ri
//! structure def ContainsBox {
//!     let solid     = box(10mm, 10mm, 10mm)
//!     let center    = point3(0mm, 0mm, 0mm)
//!     let on_face_p = point3(5mm, 0mm, 0mm)
//!     let corner_p  = point3(5mm, 5mm, 5mm)
//!     let far       = point3(20mm, 0mm, 0mm)
//!     let inside    = contains(solid, center)
//!     let on_face   = contains(solid, on_face_p)
//!     let corner    = contains(solid, corner_p)
//!     let outside   = contains(solid, far)
//! }
//! ```
//!
//! Box geometry: 10 mm × 10 mm × 10 mm centred at origin ⟹ faces at ±5 mm
//! (±0.005 m in SI).  `BRepClass3d_SolidClassifier` semantics:
//!
//! | cell    | point (SI)             | OCCT state  | expected  |
//! |---------|------------------------|-------------|-----------|
//! | inside  | (0.000, 0, 0)          | TopAbs_IN   | true      |
//! | on_face | (0.005, 0, 0)          | TopAbs_ON   | true      |
//! | corner  | (0.005, 0.005, 0.005)  | TopAbs_ON   | true      |
//! | outside | (0.020, 0, 0)          | TopAbs_OUT  | false     |
//!
//! Gated on `reify_kernel_occt::OCCT_AVAILABLE` — skips cleanly on runners
//! without OCCT.  Modelled on `kernel_queries_moment_of_inertia_smoke.rs` for
//! the real-kernel harness (`SingleKernelHolder + OcctKernelHandle::spawn`)
//! and on `kernel_queries_angle_smoke.rs` for the CARGO_MANIFEST_DIR path pattern.

use super::fixture_scaffolding::{assert_bool_cell, compile_and_build_with_occt};

const CONTAINS_BOX_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kernel_queries/contains_box.ri"
);

/// Pins the user-observable signal for KGQ-β: `contains(solid, point)` on a
/// 10 mm × 10 mm × 10 mm box must evaluate to `Value::Bool(true)` for a
/// centre/on-face/corner point, and `Value::Bool(false)` for a far-outside point.
///
/// The fixture uses `BRepClass3d_SolidClassifier` (TopAbs_IN || TopAbs_ON → true).
///
/// Skips cleanly (via early return) when OCCT is not available.
#[test]
fn contains_box_evals_expected_booleans() {
    let Some(result) = compile_and_build_with_occt(
        CONTAINS_BOX_PATH,
        "examples/kernel_queries/contains_box.ri (task 3611 step-8)",
    ) else {
        return;
    };

    // Center (0, 0, 0): strictly inside the box → TopAbs_IN → true.
    assert_bool_cell(&result, "ContainsBox", "inside", true);

    // Face centre (5 mm, 0, 0): exactly on the +X face → TopAbs_ON → true.
    assert_bool_cell(&result, "ContainsBox", "on_face", true);

    // Corner vertex (5 mm, 5 mm, 5 mm): on the boundary → TopAbs_ON → true.
    assert_bool_cell(&result, "ContainsBox", "corner", true);

    // Far outside (20 mm, 0, 0): well outside the box → TopAbs_OUT → false.
    assert_bool_cell(&result, "ContainsBox", "outside", false);
}
