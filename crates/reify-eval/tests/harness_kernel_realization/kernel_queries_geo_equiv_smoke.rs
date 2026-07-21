//! Real-OCCT end-to-end pin test for `geo_equiv(Geometry, Geometry, Length) -> Bool`
//! (task 3613, PRD `docs/prds/v0_3/kernel-geometry-queries.md` §9 KGQ-δ).
//!
//! The fixture `examples/kernel_queries/geo_equiv_smoke.ri` contains:
//!
//! ```ri
//! structure def GeoEquivSmoke {
//!     let a   = box(10mm, 10mm, 10mm)       // reference shape
//!     let b   = box(10mm, 10mm, 10mm)       // identical to a
//!     let c   = box(10.0001mm, 10mm, 10mm)  // within-tol (displacement 5e-8 m < tol)
//!     let d   = cylinder(5mm, 10mm)         // topology-different (3 faces vs 6)
//!     let tol = 0.001mm                     // = 1e-6 m
//!     let identical  = geo_equiv(a, b, tol) // true
//!     let within_tol = geo_equiv(a, c, tol) // true
//!     let diff_topo  = geo_equiv(a, d, tol) // false
//! }
//! ```
//!
//! | cell       | shapes    | displacement    | tol    | expected |
//! |------------|-----------|-----------------|--------|----------|
//! | identical  | a vs b    | 0               | 1e-6 m | true     |
//! | within_tol | a vs c    | 5e-8 m          | 1e-6 m | true     |
//! | diff_topo  | a vs d    | box≠cylinder    | 1e-6 m | false    |
//!
//! Gated on `reify_kernel_occt::OCCT_AVAILABLE` — skips cleanly on runners
//! without OCCT. Modelled on `kernel_queries_contains.rs` for the harness.

use reify_constraints::SimpleConstraintChecker;
use reify_core::ValueCellId;
use reify_ir::{ExportFormat, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

const GEO_EQUIV_SMOKE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kernel_queries/geo_equiv_smoke.ri"
);

/// Pins the user-observable signal for KGQ-δ: `geo_equiv(left, right, tol)` on
/// box/box-identical, box/box-within-tol, and box/cylinder must evaluate to
/// the expected Bool per §8.2.
///
/// Skips cleanly (via early return) when OCCT is not available.
#[test]
fn geo_equiv_smoke_evals_expected_booleans() {
    // Read the fixture unconditionally so a missing file is caught even on
    // OCCT-less runners — fixture presence is a CI contract independent of OCCT.
    let source = std::fs::read_to_string(GEO_EQUIV_SMOKE_PATH)
        .expect("examples/kernel_queries/geo_equiv_smoke.ri should exist (task 3613 step-8)");

    // Validate fixture compilation unconditionally — a grammar/compile regression
    // (e.g. `geo_equiv` arity change) should fail on every runner.
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/kernel_queries/geo_equiv_smoke.ri should compile with no \
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
        let cell = ValueCellId::new("GeoEquivSmoke", cell_name);
        let actual = result.values.get(&cell);
        assert_eq!(
            actual,
            Some(&Value::Bool(expected)),
            "GeoEquivSmoke.{cell_name} should be Value::Bool({expected}), got: {actual:?}"
        );
    };

    // §8.2 three-case pin:

    // identical (a vs b, Δ=0, topology match) → true
    assert_bool("identical", true);

    // within_tol (a vs c, displacement 5e-8 m < tol 1e-6 m, topology match) → true
    assert_bool("within_tol", true);

    // diff_topo (box(10mm) vs cylinder(5mm,10mm), 6 faces vs 3 faces) → false
    assert_bool("diff_topo", false);
}
