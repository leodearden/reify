//! Real-OCCT end-to-end pin test for `moment_of_inertia(Solid, Density)`
//! (task 3620, PRD `docs/prds/v0_3/kernel-geometry-queries.md` §9 KGQ-λ).
//!
//! The fixture `examples/kernel_queries/moment_of_inertia_box.ri` contains:
//!
//! ```ri
//! structure def MomentOfInertiaBox {
//!     let b = box(50mm, 30mm, 10mm)
//!     let steel_density = 7850kg/m^3
//!     let i = moment_of_inertia(b, steel_density)
//! }
//! ```
//!
//! The user-observable signal: `i` evaluates to a rank-2 `Value::Tensor`
//! (3 rows × 3 cols) of `MOMENT_OF_INERTIA`-dimensioned `Value::Scalar`s whose
//! diagonal entries match the analytic centroidal moments:
//!
//! ```
//! m = ρ·V = 7850·(0.05·0.03·0.01) = 0.11775 kg
//! I_xx = (1/12)·m·(H² + D²)  ≈ 9.8125e-6 kg·m²   (H=0.03 m, D=0.01 m)
//! I_yy = (1/12)·m·(W² + D²)  ≈ 2.55125e-5 kg·m²  (W=0.05 m, D=0.01 m)
//! I_zz = (1/12)·m·(W² + H²)  ≈ 3.33625e-5 kg·m²  (W=0.05 m, H=0.03 m)
//! off-diagonals = 0 (axis-aligned box, centroidal frame)
//! ```
//!
//! Tolerance: 1e-9 kg·m² (OCCT integrates a planar-faced box exactly via
//! Gauss quadrature — ~1e-12 relative error on ~1e-5-magnitude values).
//!
//! Real-OCCT builds go through `fixture_scaffolding`'s shared OCCT gate, so
//! the OCCT assertions skip cleanly on runners without OCCT.

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DimensionVector, ValueCellId};
use reify_ir::{ExportFormat, Value};
use reify_test_support::{MockGeometryKernel, parse_and_compile_with_stdlib};

use super::fixture_scaffolding::{build_source_with_occt, compile_and_build_with_occt};

const MOMENT_OF_INERTIA_BOX_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kernel_queries/moment_of_inertia_box.ri"
);

/// Asserts `actual` is a rank-2 3×3 `MOMENT_OF_INERTIA` tensor whose every
/// entry lies within 1e-9 kg·m² of the analytic centroidal tensor of the
/// 50 mm × 30 mm × 10 mm box at 7850 kg/m³ (m = 0.11775 kg): `I_xx`, `I_yy`,
/// `I_zz` on the diagonal, 0 off it.
///
/// Every pin in this module checks that one box, so this is the module's single
/// copy of the reference data. `label` names the struct whose `i` cell is read.
#[track_caller]
fn assert_moi_box_analytic_tensor(actual: Option<&Value>, label: &str) {
    let (w, h, d) = (0.05_f64, 0.03_f64, 0.01_f64);
    let mass = 7850.0 * w * h * d;
    let expected = [
        [(1.0 / 12.0) * mass * (h * h + d * d), 0.0, 0.0],
        [0.0, (1.0 / 12.0) * mass * (w * w + d * d), 0.0],
        [0.0, 0.0, (1.0 / 12.0) * mass * (w * w + h * h)],
    ];
    let tol = 1e-9_f64; // kg·m²

    let rows = match actual {
        Some(Value::Tensor(rows)) if rows.len() == 3 => rows,
        other => panic!(
            "{label}.i should be a rank-2 Value::Tensor (3 rows × 3 cols) of \
             MOMENT_OF_INERTIA-dimensioned scalars, got: {other:?}"
        ),
    };
    for (r, (row, expected_row)) in rows.iter().zip(&expected).enumerate() {
        let cols = match row {
            Value::Tensor(cols) if cols.len() == 3 => cols,
            other => panic!("{label}.i row {r} should be a 3-entry Value::Tensor, got: {other:?}"),
        };
        for (c, (entry, want)) in cols.iter().zip(expected_row).enumerate() {
            let got = match entry {
                Value::Scalar {
                    si_value,
                    dimension,
                } if *dimension == DimensionVector::MOMENT_OF_INERTIA => *si_value,
                other => panic!(
                    "{label}.i[{r},{c}] should be Value::Scalar {{ dimension: \
                     MOMENT_OF_INERTIA, .. }}, got: {other:?}"
                ),
            };
            let delta = (got - want).abs();
            assert!(
                delta < tol,
                "{label}.i[{r},{c}]: expected {want:.3e}, got {got:.3e} \
                 (delta {delta:.3e}, tol {tol:.0e})"
            );
        }
    }
}

/// Pins the user-observable signal for KGQ-λ: `moment_of_inertia` on a
/// 50 mm × 30 mm × 10 mm steel box must evaluate to a rank-2 3×3
/// `MOMENT_OF_INERTIA`-dimensioned tensor matching the analytic centroidal
/// moments to within 1e-9 kg·m², with all off-diagonals below 1e-9 kg·m².
///
/// Skips cleanly (via early return) when OCCT is not available.
#[test]
fn moment_of_inertia_box_evals_to_analytic_tensor() {
    // The fixture is read and compiled unconditionally (a missing file or a
    // grammar/compile regression fails on every runner); the OCCT build is
    // skipped cleanly when OCCT is not built.
    let Some(result) = compile_and_build_with_occt(
        MOMENT_OF_INERTIA_BOX_PATH,
        "examples/kernel_queries/moment_of_inertia_box.ri (task 3620 step-4)",
    ) else {
        return;
    };

    let cell = ValueCellId::new("MomentOfInertiaBox", "i");
    assert_moi_box_analytic_tensor(result.values.get(&cell), "MomentOfInertiaBox");
}

/// Pins task 4486 (type-hygiene γ, Contract A): `moment_of_inertia` must accept
/// a `material.density` field — a `Value::Scalar{MASS_DENSITY}` — and evaluate
/// to the same analytic centroidal tensor as the bare-Real fixture.
///
/// Inline source (probe-5 shape, LetBoundFieldDensity):
/// ```ri
/// structure def MoiViaMaterial {
///     param material : Material = Material(name: "steel", density: 7850kg/m^3,
///                                          youngs_modulus: 200GPa)
///     let b = box(50mm, 30mm, 10mm)
///     let d = material.density
///     let i = moment_of_inertia(b, d)
/// }
/// ```
///
/// Asserts compile-time clean unconditionally; under OCCT asserts `i` is a
/// non-Undef rank-2 3×3 `MOMENT_OF_INERTIA` tensor matching the same analytic
/// values as `moment_of_inertia_box_evals_to_analytic_tensor` (same box + density).
#[test]
fn moment_of_inertia_via_material_density_evals_to_tensor() {
    const SOURCE: &str = r#"
structure def MoiViaMaterial {
    param material : Material = Material(name: "steel", density: 7850kg/m^3, youngs_modulus: 200GPa)
    let b = box(50mm, 30mm, 10mm)
    let d = material.density
    let i = moment_of_inertia(b, d)
}
"#;

    let Some(result) = build_source_with_occt(SOURCE, "MoiViaMaterial") else {
        return;
    };

    let cell = ValueCellId::new("MoiViaMaterial", "i");
    assert_moi_box_analytic_tensor(result.values.get(&cell), "MoiViaMaterial");
}

/// Task ε (type-hygiene, evaluate-then-accept): the INLINE density form
/// `moment_of_inertia(b, 7850kg/m^3)` — with NO intermediate `let` binding the
/// density — must
///   (1) compile clean (no error-severity diagnostics),
///   (2) build WITHOUT emitting the γ "density argument … not yet supported /
///       must be bound to a let" Warning (the eval-upgrade flips that silent
///       fall-through), and
///   (3) under real OCCT evaluate `i` to the SAME validated analytic 3×3
///       `MOMENT_OF_INERTIA` tensor as the let-bound
///       `moment_of_inertia_box_evals_to_analytic_tensor` fixture
///       (m = 0.11775 kg for a 50 × 30 × 10 mm box at 7850 kg/m³).
///
/// Assertion (2) runs on EVERY runner via a `MockGeometryKernel`: the
/// density-arg resolution (and its potential Warning) happens in the
/// `try_eval_topology_selector` post-process BEFORE any kernel query, so it is
/// independent of OCCT. Before ε this fixture emitted the "density argument"
/// Warning → RED; after ε the accepted inline density emits none → GREEN.
#[test]
fn moment_of_inertia_inline_density_evals_to_analytic_tensor() {
    const SOURCE: &str = r#"
structure def MomentOfInertiaInline {
    let b = box(50mm, 30mm, 10mm)
    let i = moment_of_inertia(b, 7850kg/m^3)
}
"#;

    // (1) `parse_and_compile_with_stdlib` panics on any error-severity diagnostic.
    let compiled = parse_and_compile_with_stdlib(SOURCE);

    // (2) No density-arg Warning on ANY runner — build with a MockGeometryKernel
    //     so the post-process density resolution runs without OCCT.
    {
        let checker = SimpleConstraintChecker;
        let mut engine =
            reify_eval::Engine::new(Box::new(checker), Some(Box::new(MockGeometryKernel::new())));
        let result = engine.build(&compiled, ExportFormat::Step);
        let density_warnings: Vec<&str> = result
            .diagnostics
            .iter()
            .filter(|d| d.message.to_lowercase().contains("density argument"))
            .map(|d| d.message.as_str())
            .collect();
        assert!(
            density_warnings.is_empty(),
            "inline `moment_of_inertia(b, 7850kg/m^3)` must NOT emit a density-arg Warning \
             (task ε flips γ's 'not yet supported' fall-through); got: {density_warnings:#?}"
        );
    }

    // (3) Analytic tensor under real OCCT. The shared gate takes source, not a
    //     `CompiledModule`, so it recompiles SOURCE.
    let Some(result) = build_source_with_occt(SOURCE, "MomentOfInertiaInline") else {
        return;
    };

    let cell = ValueCellId::new("MomentOfInertiaInline", "i");
    assert_moi_box_analytic_tensor(result.values.get(&cell), "MomentOfInertiaInline");
}
