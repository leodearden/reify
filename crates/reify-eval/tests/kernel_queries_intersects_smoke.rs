//! Real-OCCT end-to-end pin test for `intersects(Geometry, Geometry) -> Bool`
//! (task 3612, PRD `docs/prds/v0_3/kernel-geometry-queries.md` §9 KGQ-γ).
//!
//! The fixture `examples/kernel_queries/intersects_smoke.ri` contains:
//!
//! ```ri
//! structure def IntersectsSmoke {
//!     let a         = box(10mm, 10mm, 10mm)
//!     let b_overlap = translate(box(10mm, 10mm, 10mm), 5mm, 0mm, 0mm)
//!     let b_far     = translate(box(10mm, 10mm, 10mm), 100mm, 0mm, 0mm)
//!     let overlapping = intersects(a, b_overlap)
//!     let apart       = intersects(a, b_far)
//! }
//! ```
//!
//! Box geometry: `a` is 10 mm × 10 mm × 10 mm centred at origin ⟹ spans ±5 mm
//! (±0.005 m in SI).  `b_overlap` translated 5 mm in X spans 0..10 mm in X —
//! positive-volume overlap with `a` (0..5 mm in X), so BRep min distance = 0.0 →
//! `intersects` = `true`.  `b_far` translated 100 mm in X spans 95..105 mm in X
//! — ~90 mm face gap from `a`, so BRep min distance ≈ 0.09 m > 0.0 →
//! `intersects` = `false`.
//!
//! Dispatch route (task 3612 design decision): routes through
//! `GeometryQuery::Distance{from,to}` classifying `d <= 0.0 → Bool`, identical
//! to the shipped `shapes_intersect` adapter
//! (`reify-kernel-occt/src/lib.rs:770`) and the `interferes_with` helper
//! (`geometry_ops.rs:1601`).
//!
//! The compilation check runs unconditionally so a grammar or compile regression
//! fails on every runner. The kernel build + assertion is gated on
//! `reify_kernel_occt::OCCT_AVAILABLE` — skips cleanly on runners without OCCT.
//!
//! Modelled on `crates/reify-eval/tests/kernel_queries_contains.rs` (Bool
//! assertion pattern) and `crates/reify-eval/tests/kernel_queries_distance_smoke.rs`
//! (unconditional compile check + OCCT-gated value assertions).

use reify_constraints::SimpleConstraintChecker;
use reify_core::ValueCellId;
use reify_ir::{ExportFormat, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

const INTERSECTS_SMOKE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kernel_queries/intersects_smoke.ri"
);

/// Pins the user-observable signal for KGQ-γ: `intersects(Geometry, Geometry)`
/// on two 10 mm boxes must evaluate to `Value::Bool(true)` when the boxes have
/// positive-volume overlap, and `Value::Bool(false)` when they are well apart.
///
/// Dispatches via `GeometryQuery::Distance` classified `d <= 0.0`:
/// - `overlapping`: BRep distance = 0.0 (touching/overlapping) → `true`.
/// - `apart`: BRep distance ≈ 0.09 m (90 mm face gap) → `false`.
///
/// Skips cleanly (via early return) when OCCT is not available.
#[test]
fn intersects_smoke_evals_expected_booleans() {
    // Read the fixture unconditionally so a missing file is caught even on
    // OCCT-less runners — fixture presence is a CI contract independent of OCCT.
    let source = std::fs::read_to_string(INTERSECTS_SMOKE_PATH).expect(
        "examples/kernel_queries/intersects_smoke.ri should exist (task 3612 pre-1)",
    );

    // Validate fixture compilation unconditionally — a grammar/compile regression
    // (e.g. `intersects` signature change) should fail on every runner.
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/kernel_queries/intersects_smoke.ri should compile with no \
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

    // Helper: assert a Bool cell on IntersectsSmoke equals the expected value.
    let assert_bool = |cell_name: &str, expected: bool| {
        let cell = ValueCellId::new("IntersectsSmoke", cell_name);
        let actual = result.values.get(&cell);
        assert_eq!(
            actual,
            Some(&Value::Bool(expected)),
            "IntersectsSmoke.{cell_name} should be Value::Bool({expected}), got: {actual:?}"
        );
    };

    // b_overlap translated 5mm in X → overlaps a (both span ±5mm centred at origin)
    // by 5mm in X → BRep min distance = 0.0 → intersects = true.
    assert_bool("overlapping", true);

    // b_far translated 100mm in X → spans 95..105mm in X, ~90mm face gap from a
    // → BRep min distance ≈ 0.09m > 0.0 → intersects = false.
    assert_bool("apart", false);
}
