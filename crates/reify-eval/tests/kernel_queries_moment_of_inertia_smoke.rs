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
//! Gated on `reify_kernel_occt::OCCT_AVAILABLE` — skips cleanly on runners
//! without OCCT. Modelled on `boolean_ops_e2e.rs` for the real-kernel harness
//! (`SingleKernelHolder + OcctKernelHandle::spawn`) and on
//! `kernel_queries_angle_smoke.rs` for the CARGO_MANIFEST_DIR path pattern.

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DimensionVector, ValueCellId};
use reify_ir::{ExportFormat, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib, MockGeometryKernel};

const MOMENT_OF_INERTIA_BOX_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kernel_queries/moment_of_inertia_box.ri"
);

/// Pins the user-observable signal for KGQ-λ: `moment_of_inertia` on a
/// 50 mm × 30 mm × 10 mm steel box must evaluate to a rank-2 3×3
/// `MOMENT_OF_INERTIA`-dimensioned tensor matching the analytic centroidal
/// moments to within 1e-9 kg·m², with all off-diagonals below 1e-9 kg·m².
///
/// Skips cleanly (via early return) when OCCT is not available.
#[test]
fn moment_of_inertia_box_evals_to_analytic_tensor() {
    // Read the fixture unconditionally so a missing file is caught even on
    // OCCT-less runners — fixture presence is a CI contract independent of OCCT.
    let source = std::fs::read_to_string(MOMENT_OF_INERTIA_BOX_PATH)
        .expect("examples/kernel_queries/moment_of_inertia_box.ri should exist (task 3620 step-4)");

    // Validate fixture compilation unconditionally — a grammar/compile regression
    // (e.g. moment_of_inertia signature change) should fail on every runner,
    // not just those with OCCT.
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/kernel_queries/moment_of_inertia_box.ri should compile with no \
         error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    // Skip the OCCT-dependent kernel build/tensor assertions if OCCT is not built.
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

    let cell = ValueCellId::new("MomentOfInertiaBox", "i");
    let actual = result.values.get(&cell);

    // Analytic centroidal moments for a 50 mm × 30 mm × 10 mm box at 7850 kg/m³.
    //   m = ρ·V = 7850 · 0.05 · 0.03 · 0.01 = 0.11775 kg
    //   I_xx = (1/12)·m·(H² + D²)   H=0.03, D=0.01
    //   I_yy = (1/12)·m·(W² + D²)   W=0.05, D=0.01
    //   I_zz = (1/12)·m·(W² + H²)   W=0.05, H=0.03
    let w = 0.05_f64;
    let h = 0.03_f64;
    let d = 0.01_f64;
    let mass = 7850.0 * w * h * d;
    let i_xx = (1.0 / 12.0) * mass * (h * h + d * d);
    let i_yy = (1.0 / 12.0) * mass * (w * w + d * d);
    let i_zz = (1.0 / 12.0) * mass * (w * w + h * h);
    let tol = 1e-9_f64; // kg·m²

    // Extract the 3×3 tensor and validate every entry.
    let rows = match actual {
        Some(Value::Tensor(rows))
            if rows.len() == 3
                && rows
                    .iter()
                    .all(|r| matches!(r, Value::Tensor(cols) if cols.len() == 3)) =>
        {
            rows
        }
        other => panic!(
            "MomentOfInertiaBox.i should be a rank-2 Value::Tensor (3 rows × 3 cols) \
             of MOMENT_OF_INERTIA-dimensioned scalars (PRD §9 KGQ-λ), got: {other:?}"
        ),
    };

    // Helper: extract si_value from a MOMENT_OF_INERTIA Scalar, panic otherwise.
    fn extract(v: &Value, label: &str) -> f64 {
        match v {
            Value::Scalar {
                si_value,
                dimension,
            } if *dimension == DimensionVector::MOMENT_OF_INERTIA => *si_value,
            other => panic!(
                "MomentOfInertiaBox.i[{label}] should be \
                 Value::Scalar {{ dimension: MOMENT_OF_INERTIA, .. }}, got: {other:?}"
            ),
        }
    }

    fn get_row(row: &Value) -> &Vec<Value> {
        match row {
            Value::Tensor(cols) => cols,
            _ => unreachable!("already validated rank-2 shape above"),
        }
    }

    let r0 = get_row(&rows[0]);
    let r1 = get_row(&rows[1]);
    let r2 = get_row(&rows[2]);

    // Diagonals must match analytic values within tol.
    let v00 = extract(&r0[0], "0,0");
    let v11 = extract(&r1[1], "1,1");
    let v22 = extract(&r2[2], "2,2");

    assert!(
        (v00 - i_xx).abs() < tol,
        "I_xx=[0,0]: expected {i_xx:.3e}, got {v00:.3e} (delta {delta:.3e}, tol {tol:.0e})",
        delta = (v00 - i_xx).abs()
    );
    assert!(
        (v11 - i_yy).abs() < tol,
        "I_yy=[1,1]: expected {i_yy:.3e}, got {v11:.3e} (delta {delta:.3e}, tol {tol:.0e})",
        delta = (v11 - i_yy).abs()
    );
    assert!(
        (v22 - i_zz).abs() < tol,
        "I_zz=[2,2]: expected {i_zz:.3e}, got {v22:.3e} (delta {delta:.3e}, tol {tol:.0e})",
        delta = (v22 - i_zz).abs()
    );

    // Off-diagonals must be zero (axis-aligned centroidal box).
    let off_diag_entries = [
        (extract(&r0[1], "0,1"), "0,1"),
        (extract(&r0[2], "0,2"), "0,2"),
        (extract(&r1[0], "1,0"), "1,0"),
        (extract(&r1[2], "1,2"), "1,2"),
        (extract(&r2[0], "2,0"), "2,0"),
        (extract(&r2[1], "2,1"), "2,1"),
    ];
    for (v, label) in &off_diag_entries {
        assert!(
            v.abs() < tol,
            "off-diagonal [{label}]: expected 0, got {v:.3e} (tol {tol:.0e})"
        );
    }
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

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    assert!(
        errors_only(&compiled).is_empty(),
        "MoiViaMaterial should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions: OCCT not available");
        return;
    }

    let checker = SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    let result = engine.build(&compiled, reify_ir::ExportFormat::Step);

    let cell = ValueCellId::new("MoiViaMaterial", "i");
    let actual = result.values.get(&cell);

    // Same analytic values as moment_of_inertia_box_evals_to_analytic_tensor.
    let w = 0.05_f64;
    let h = 0.03_f64;
    let d = 0.01_f64;
    let mass = 7850.0 * w * h * d;
    let i_xx = (1.0 / 12.0) * mass * (h * h + d * d);
    let i_yy = (1.0 / 12.0) * mass * (w * w + d * d);
    let i_zz = (1.0 / 12.0) * mass * (w * w + h * h);
    let tol = 1e-9_f64;

    let rows = match actual {
        Some(Value::Tensor(rows))
            if rows.len() == 3
                && rows
                    .iter()
                    .all(|r| matches!(r, Value::Tensor(cols) if cols.len() == 3)) =>
        {
            rows
        }
        other => panic!(
            "MoiViaMaterial.i should be a rank-2 Value::Tensor (3×3) of \
             MOMENT_OF_INERTIA-dimensioned scalars (task 4486 Contract A), got: {other:?}"
        ),
    };

    fn extract_moi(v: &Value, label: &str) -> f64 {
        match v {
            Value::Scalar {
                si_value,
                dimension,
            } if *dimension == DimensionVector::MOMENT_OF_INERTIA => *si_value,
            other => panic!(
                "MoiViaMaterial.i[{label}] should be \
                 Value::Scalar {{ dimension: MOMENT_OF_INERTIA, .. }}, got: {other:?}"
            ),
        }
    }

    fn get_row_moi(row: &Value) -> &Vec<Value> {
        match row {
            Value::Tensor(cols) => cols,
            _ => unreachable!("already validated rank-2 shape"),
        }
    }

    let r0 = get_row_moi(&rows[0]);
    let r1 = get_row_moi(&rows[1]);
    let r2 = get_row_moi(&rows[2]);

    let v00 = extract_moi(&r0[0], "0,0");
    let v11 = extract_moi(&r1[1], "1,1");
    let v22 = extract_moi(&r2[2], "2,2");

    assert!(
        (v00 - i_xx).abs() < tol,
        "MoiViaMaterial I_xx=[0,0]: expected {i_xx:.3e}, got {v00:.3e} (delta {:.3e}, tol {tol:.0e})",
        (v00 - i_xx).abs()
    );
    assert!(
        (v11 - i_yy).abs() < tol,
        "MoiViaMaterial I_yy=[1,1]: expected {i_yy:.3e}, got {v11:.3e} (delta {:.3e}, tol {tol:.0e})",
        (v11 - i_yy).abs()
    );
    assert!(
        (v22 - i_zz).abs() < tol,
        "MoiViaMaterial I_zz=[2,2]: expected {i_zz:.3e}, got {v22:.3e} (delta {:.3e}, tol {tol:.0e})",
        (v22 - i_zz).abs()
    );

    let off_diag_entries = [
        (extract_moi(&r0[1], "0,1"), "0,1"),
        (extract_moi(&r0[2], "0,2"), "0,2"),
        (extract_moi(&r1[0], "1,0"), "1,0"),
        (extract_moi(&r1[2], "1,2"), "1,2"),
        (extract_moi(&r2[0], "2,0"), "2,0"),
        (extract_moi(&r2[1], "2,1"), "2,1"),
    ];
    for (v, label) in &off_diag_entries {
        assert!(
            v.abs() < tol,
            "MoiViaMaterial off-diagonal [{label}]: expected 0, got {v:.3e} (tol {tol:.0e})"
        );
    }
}

/// Shared analytic-tensor assertion for the 50 mm × 30 mm × 10 mm steel box at
/// 7850 kg/m³ (m = 0.11775 kg). Validates `actual` is a rank-2 3×3
/// `MOMENT_OF_INERTIA` tensor whose diagonal matches the analytic centroidal
/// moments within 1e-9 kg·m² and whose off-diagonals are below 1e-9 kg·m².
/// Used by the task ε inline-density test to assert the same validated values
/// the let-bound fixtures above assert, without inventing new reference data.
fn assert_moi_box_analytic_tensor(actual: Option<&Value>, label: &str) {
    let w = 0.05_f64;
    let h = 0.03_f64;
    let d = 0.01_f64;
    let mass = 7850.0 * w * h * d;
    let i_xx = (1.0 / 12.0) * mass * (h * h + d * d);
    let i_yy = (1.0 / 12.0) * mass * (w * w + d * d);
    let i_zz = (1.0 / 12.0) * mass * (w * w + h * h);
    let tol = 1e-9_f64;

    let rows = match actual {
        Some(Value::Tensor(rows))
            if rows.len() == 3
                && rows
                    .iter()
                    .all(|r| matches!(r, Value::Tensor(cols) if cols.len() == 3)) =>
        {
            rows
        }
        other => panic!(
            "{label}.i should be a rank-2 Value::Tensor (3 rows × 3 cols) of \
             MOMENT_OF_INERTIA-dimensioned scalars, got: {other:?}"
        ),
    };

    fn extract(v: &Value, label: &str) -> f64 {
        match v {
            Value::Scalar {
                si_value,
                dimension,
            } if *dimension == DimensionVector::MOMENT_OF_INERTIA => *si_value,
            other => panic!(
                "entry [{label}] should be Value::Scalar {{ dimension: \
                 MOMENT_OF_INERTIA, .. }}, got: {other:?}"
            ),
        }
    }

    fn get_row(row: &Value) -> &Vec<Value> {
        match row {
            Value::Tensor(cols) => cols,
            _ => unreachable!("already validated rank-2 shape above"),
        }
    }

    let r0 = get_row(&rows[0]);
    let r1 = get_row(&rows[1]);
    let r2 = get_row(&rows[2]);

    let v00 = extract(&r0[0], "0,0");
    let v11 = extract(&r1[1], "1,1");
    let v22 = extract(&r2[2], "2,2");

    assert!(
        (v00 - i_xx).abs() < tol,
        "{label} I_xx=[0,0]: expected {i_xx:.3e}, got {v00:.3e} (tol {tol:.0e})"
    );
    assert!(
        (v11 - i_yy).abs() < tol,
        "{label} I_yy=[1,1]: expected {i_yy:.3e}, got {v11:.3e} (tol {tol:.0e})"
    );
    assert!(
        (v22 - i_zz).abs() < tol,
        "{label} I_zz=[2,2]: expected {i_zz:.3e}, got {v22:.3e} (tol {tol:.0e})"
    );

    let off_diag_entries = [
        (extract(&r0[1], "0,1"), "0,1"),
        (extract(&r0[2], "0,2"), "0,2"),
        (extract(&r1[0], "1,0"), "1,0"),
        (extract(&r1[2], "1,2"), "1,2"),
        (extract(&r2[0], "2,0"), "2,0"),
        (extract(&r2[1], "2,1"), "2,1"),
    ];
    for (v, lbl) in &off_diag_entries {
        assert!(
            v.abs() < tol,
            "{label} off-diagonal [{lbl}]: expected 0, got {v:.3e} (tol {tol:.0e})"
        );
    }
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

    let compiled = parse_and_compile_with_stdlib(SOURCE);
    assert!(
        errors_only(&compiled).is_empty(),
        "MomentOfInertiaInline should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

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

    // (3) Analytic tensor under real OCCT.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions: OCCT not available");
        return;
    }

    let checker = SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("MomentOfInertiaInline", "i");
    assert_moi_box_analytic_tensor(result.values.get(&cell), "MomentOfInertiaInline");
}
