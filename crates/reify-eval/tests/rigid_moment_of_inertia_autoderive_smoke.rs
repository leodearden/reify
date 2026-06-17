//! End-to-end auto-derive smoke test for `Rigid.moment_of_inertia` (task 4229,
//! Option A: geometry-derived tensor via `moment_of_inertia(geometry, body_density)`).
//!
//! A `Rigid` conformer that omits `moment_of_inertia` must:
//!   1. Compile clean unconditionally (OCCT-independent).
//!   2. Under OCCT: evaluate `moment_of_inertia` to a non-Undef rank-2 3×3
//!      `MOMENT_OF_INERTIA`-dimensioned tensor matching the analytic centroidal
//!      moments for a 100 mm × 100 mm × 300 mm steel box (ρ = 7850 kg/m³).
//!   3. Under OCCT: evaluate `moi_principal` to a `Value::List` of three sorted-
//!      ascending MOMENT_OF_INERTIA scalars matching the eigenvalues of the diagonal
//!      tensor, with the smallest eigenvalue > 0 (PD-constraint witness).
//!
//! Analytic values (box W=H=0.1 m, D=0.3 m, ρ=7850 kg/m³):
//!   m  = ρ·V  = 7850·(0.1·0.1·0.3) = 23.55 kg
//!   I_xx = I_yy = (1/12)·m·(H²+D²) = (1/12)·23.55·(0.01+0.09) = 0.19625 kg·m²
//!   I_zz         = (1/12)·m·(W²+H²) = (1/12)·23.55·(0.01+0.01) = 0.03925 kg·m²
//!   off-diagonals = 0 (axis-aligned centroidal box)
//!   eigenvalues (sorted) = [0.03925, 0.19625, 0.19625]
//!
//! Tolerance: 1e-9 kg·m² (OCCT Gauss quadrature on a planar box is essentially exact).
//!
//! Modelled on `crates/reify-eval/tests/kernel_queries_moment_of_inertia_smoke.rs`
//! (same SingleKernelHolder + OcctKernelHandle::spawn harness, same OCCT_AVAILABLE
//! gating, same 1e-9 tolerance).

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DimensionVector, ValueCellId};
use reify_ir::{ExportFormat, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

/// Inline source: a Rigid conformer that OMITS moment_of_inertia entirely.
/// After task 4229, the trait auto-derives it from `geometry` + `material.density`.
///
/// Box: 100 mm × 100 mm × 300 mm.  Material: steel (ρ = 7850 kg/m³).
const AUTO_MOI_POST_SRC: &str = r#"
structure def AutoMoiPost : Rigid {
    param geometry : Solid = box(100mm, 100mm, 300mm)
    param material : Material = Material(name: "steel", density: 7850kg/m^3, youngs_modulus: 200GPa)
}
"#;

/// Pins the user-observable auto-derive signal for task 4229 Option A.
///
/// Unconditionally: the omitting conformer compiles clean (auto-derive
/// type-checks at compile time, OCCT not required).
///
/// Under OCCT: `AutoMoiPost.moment_of_inertia` evaluates to the correct 3×3
/// centroidal tensor; `AutoMoiPost.moi_principal` is a sorted-ascending List
/// of three MOMENT_OF_INERTIA scalars whose first element > 0 (PD witness).
///
/// RED against base: the base has `moment_of_inertia` as a required member —
/// omitting it is a conformance error, so `errors_only` is non-empty.
/// GREEN after step-3 impl (stdlib edit + conformer migrations).
#[test]
fn rigid_moment_of_inertia_auto_derives_from_geometry() {
    // ─── compile-time assertion (unconditional, OCCT-independent) ──────────────
    let compiled = parse_and_compile_with_stdlib(AUTO_MOI_POST_SRC);
    assert!(
        errors_only(&compiled).is_empty(),
        "task-4229: AutoMoiPost (omitting moment_of_inertia) should compile with no \
         error-severity diagnostics once the trait auto-derives it; got:\n{:#?}",
        errors_only(&compiled)
    );

    // ─── real-OCCT eval assertions (gated) ────────────────────────────────────
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions: OCCT not available");
        return;
    }

    let checker = SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // ── Analytic centroidal moments for a 100×100×300 mm box at 7850 kg/m³ ────
    //   m   = ρ·V  = 7850·(0.1·0.1·0.3)           = 23.55 kg
    //   I_xx = I_yy = (1/12)·m·(H²+D²)  H=0.1, D=0.3  = 0.19625 kg·m²
    //   I_zz         = (1/12)·m·(W²+H²)  W=0.1, H=0.1  = 0.03925 kg·m²
    let w = 0.1_f64;
    let h = 0.1_f64;
    let d = 0.3_f64;
    let mass = 7850.0 * w * h * d;
    let i_xx = (1.0 / 12.0) * mass * (h * h + d * d); // 0.19625
    let i_yy = (1.0 / 12.0) * mass * (w * w + d * d); // 0.19625
    let i_zz = (1.0 / 12.0) * mass * (w * w + h * h); // 0.03925
    let tol = 1e-9_f64; // kg·m²

    // Helper: extract si_value from a MOMENT_OF_INERTIA Scalar, panic otherwise.
    fn extract_moi(v: &Value, label: &str) -> f64 {
        match v {
            Value::Scalar {
                si_value,
                dimension,
            } if *dimension == DimensionVector::MOMENT_OF_INERTIA => *si_value,
            other => panic!(
                "AutoMoiPost[{label}] should be \
                 Value::Scalar {{ dimension: MOMENT_OF_INERTIA }}, got: {other:?}"
            ),
        }
    }

    fn get_row(row: &Value) -> &Vec<Value> {
        match row {
            Value::Tensor(cols) => cols,
            _ => unreachable!("already validated rank-2 shape above"),
        }
    }

    // ── Assert moment_of_inertia cell is a rank-2 3×3 tensor ──────────────────
    let moi_cell = ValueCellId::new("AutoMoiPost", "moment_of_inertia");
    let moi_actual = result.values.get(&moi_cell);

    let rows = match moi_actual {
        Some(Value::Tensor(rows))
            if rows.len() == 3
                && rows
                    .iter()
                    .all(|r| matches!(r, Value::Tensor(cols) if cols.len() == 3)) =>
        {
            rows
        }
        other => panic!(
            "task-4229: AutoMoiPost.moment_of_inertia should be a rank-2 Value::Tensor \
             (3×3) of MOMENT_OF_INERTIA-dimensioned scalars (auto-derived from geometry), \
             got: {other:?}"
        ),
    };

    let r0 = get_row(&rows[0]);
    let r1 = get_row(&rows[1]);
    let r2 = get_row(&rows[2]);

    let v00 = extract_moi(&r0[0], "0,0");
    let v11 = extract_moi(&r1[1], "1,1");
    let v22 = extract_moi(&r2[2], "2,2");

    assert!(
        (v00 - i_xx).abs() < tol,
        "AutoMoiPost I_xx=[0,0]: expected {i_xx:.6}, got {v00:.6} \
         (delta {delta:.3e}, tol {tol:.0e})",
        delta = (v00 - i_xx).abs()
    );
    assert!(
        (v11 - i_yy).abs() < tol,
        "AutoMoiPost I_yy=[1,1]: expected {i_yy:.6}, got {v11:.6} \
         (delta {delta:.3e}, tol {tol:.0e})",
        delta = (v11 - i_yy).abs()
    );
    assert!(
        (v22 - i_zz).abs() < tol,
        "AutoMoiPost I_zz=[2,2]: expected {i_zz:.6}, got {v22:.6} \
         (delta {delta:.3e}, tol {tol:.0e})",
        delta = (v22 - i_zz).abs()
    );

    // Off-diagonals must be zero (axis-aligned centroidal box).
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
            "AutoMoiPost off-diagonal [{label}]: expected 0, got {v:.3e} (tol {tol:.0e})"
        );
    }

    // ── Assert moi_principal cell is a sorted-ascending List of 3 MOI scalars ──
    //
    // Box W=H → I_xx == I_yy, so eigenvalues = [I_zz, I_xx, I_yy] sorted =
    //   [0.03925, 0.19625, 0.19625].  The PD constraint `moi_principal[0] > 0`
    //   is satisfied because the smallest eigenvalue 0.03925 > 0.
    let principal_expected = [i_zz, i_xx, i_yy]; // sorted ascending

    let principal_cell = ValueCellId::new("AutoMoiPost", "moi_principal");
    let principal_actual = result.values.get(&principal_cell);

    let items = match principal_actual {
        Some(Value::List(items)) if items.len() == 3 => items,
        other => panic!(
            "task-4229: AutoMoiPost.moi_principal should be a Value::List of 3 \
             MOMENT_OF_INERTIA scalars (sorted ascending eigenvalues of moment_of_inertia), \
             got: {other:?}"
        ),
    };

    for (i, (item, &expected)) in items.iter().zip(principal_expected.iter()).enumerate() {
        let actual_val = extract_moi(item, &format!("moi_principal[{i}]"));
        assert!(
            (actual_val - expected).abs() < tol,
            "AutoMoiPost moi_principal[{i}]: expected {expected:.6}, got {actual_val:.6} \
             (delta {delta:.3e}, tol {tol:.0e})",
            delta = (actual_val - expected).abs()
        );
    }

    // PD-constraint witness: smallest principal moment must be > 0.
    let min_principal = extract_moi(&items[0], "moi_principal[0]");
    assert!(
        min_principal > 0.0,
        "task-4229: PD constraint witness — moi_principal[0] must be > 0; got {min_principal:.6}"
    );
}
