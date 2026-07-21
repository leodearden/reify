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

use reify_constraints::SimpleConstraintChecker;
use reify_core::ValueCellId;
use reify_ir::{ExportFormat, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

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
    // Read the fixture unconditionally so a missing file is caught even on
    // OCCT-less runners — fixture presence is a CI contract independent of OCCT.
    let source = std::fs::read_to_string(CONTAINS_BOX_PATH)
        .expect("examples/kernel_queries/contains_box.ri should exist (task 3611 step-8)");

    // Validate fixture compilation unconditionally — a grammar/compile regression
    // (e.g. `contains` signature change) should fail on every runner.
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/kernel_queries/contains_box.ri should compile with no \
         error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    // Skip the OCCT-dependent kernel build/bool assertions if OCCT is not built.
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

    // Helper: assert a Bool cell equals the expected value.
    let assert_bool = |cell_name: &str, expected: bool| {
        let cell = ValueCellId::new("ContainsBox", cell_name);
        let actual = result.values.get(&cell);
        assert_eq!(
            actual,
            Some(&Value::Bool(expected)),
            "ContainsBox.{cell_name} should be Value::Bool({expected}), got: {actual:?}"
        );
    };

    // Center (0, 0, 0): strictly inside the box → TopAbs_IN → true.
    assert_bool("inside", true);

    // Face centre (5 mm, 0, 0): exactly on the +X face → TopAbs_ON → true.
    assert_bool("on_face", true);

    // Corner vertex (5 mm, 5 mm, 5 mm): on the boundary → TopAbs_ON → true.
    assert_bool("corner", true);

    // Far outside (20 mm, 0, 0): well outside the box → TopAbs_OUT → false.
    assert_bool("outside", false);
}
