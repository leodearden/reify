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

use super::fixture_scaffolding::{assert_bool_cell, compile_and_build_with_occt};

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
    let Some(result) = compile_and_build_with_occt(
        GEO_EQUIV_SMOKE_PATH,
        "examples/kernel_queries/geo_equiv_smoke.ri",
    ) else {
        return;
    };

    // §8.2 three-case pin:

    // identical (a vs b, Δ=0, topology match) → true
    assert_bool_cell(&result, "GeoEquivSmoke", "identical", true);

    // within_tol (a vs c, displacement 5e-8 m < tol 1e-6 m, topology match) → true
    assert_bool_cell(&result, "GeoEquivSmoke", "within_tol", true);

    // diff_topo (box(10mm) vs cylinder(5mm,10mm), 6 faces vs 3 faces) → false
    assert_bool_cell(&result, "GeoEquivSmoke", "diff_topo", false);
}
